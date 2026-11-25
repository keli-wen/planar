use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::db::{LineUpdate, SessionLaunchInfo, Store, SyncScope};
use crate::defaults::{AUTO_SCAN_THROTTLE_MS, DEFAULTS_JSON};
use crate::env::HarnessRoots;
use crate::error::{PlanarError, Result};
use crate::ingest::scan_all_harnesses;
use crate::model::{
    AppSettings, LineIdUpdate, LitePlaneResponse, PlaneResponse, ProcessorTestResult,
    SyncEstimateDto, SyncStreamEvent, SyncSummaryDto,
};
use crate::processor;

const INDEX_HTML: &str = include_str!("../../../ui/index.html");
const STYLES_CSS: &str = include_str!("../../../ui/styles.css");
const APP_JS: &str = include_str!("../../../ui/app.js");
const ICON_SVG: &str = include_str!("../../../assets/icon.svg");

const PENDING_STOP_TTL: Duration = Duration::from_secs(300);
const MAX_PENDING_STOPS: usize = 64;
static SYNC_RUN_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct SyncDrainRegistry {
    active: HashMap<String, Weak<AtomicBool>>,
    pending_stops: HashMap<String, Instant>,
}

impl SyncDrainRegistry {
    fn prune_stale(&mut self, now: Instant) {
        self.active.retain(|_, w| w.strong_count() > 0);
        self.pending_stops.retain(|_, recorded_at| {
            now.saturating_duration_since(*recorded_at) < PENDING_STOP_TTL
        });
    }
}

const MAX_RUN_ID_LEN: usize = 128;

fn validate_run_id(raw: Option<&str>) -> Result<Option<String>> {
    let Some(raw_val) = raw else {
        return Ok(None);
    };
    let trimmed = raw_val.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_RUN_ID_LEN
        || trimmed.starts_with("__")
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return Err(PlanarError::InvalidInput(
            "invalid `runId` query parameter (must be 1..=128 ASCII [A-Za-z0-9._:-] chars and not start with `__`)".into(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

/// Shared state for the Planar local HTTP API server.
#[derive(Clone)]
pub struct AppState {
    pub db_path: PathBuf,
    pub roots: HarnessRoots,
    pub hydrated_path: Arc<OsString>,
    pub http_client: reqwest::Client,
    pub scan_lock: Arc<Mutex<()>>,
    pub last_scan_ms: Arc<AtomicI64>,
    pub last_sync: Arc<StdMutex<Option<SyncSummaryDto>>>,
    sync_drains: Arc<StdMutex<SyncDrainRegistry>>,
}

impl AppState {
    /// Creates a new `AppState` for `db_path` and `roots`, hydrating `$PATH` in GUI builds.
    pub fn new(db_path: PathBuf, roots: HarnessRoots) -> Self {
        #[cfg(test)]
        let path_os = std::env::var_os("PATH").unwrap_or_default();
        #[cfg(not(test))]
        let path_os = crate::env::hydrate_macos_path();
        Self::with_hydrated_path(db_path, roots, path_os)
    }

    /// Creates a new `AppState` using an explicit pre-hydrated `PATH` value.
    pub fn with_hydrated_path(
        db_path: PathBuf,
        roots: HarnessRoots,
        hydrated_path: OsString,
    ) -> Self {
        Self {
            db_path,
            roots,
            hydrated_path: Arc::new(hydrated_path),
            http_client: processor::build_shared_http_client(),
            scan_lock: Arc::new(Mutex::new(())),
            last_scan_ms: Arc::new(AtomicI64::new(0)),
            last_sync: Arc::new(StdMutex::new(None)),
            sync_drains: Arc::new(StdMutex::new(SyncDrainRegistry::default())),
        }
    }

    /// Allocates and registers a per-run drain flag before scanning/streaming begins.
    /// Rejects invalid `run_id` values and duplicate active `run_id` registrations.
    /// If a Stop request for `run_id` already arrived before registration (e.g. during scan),
    /// the returned flag starts pre-set to `true` so Stop intent is never lost.
    pub(crate) fn prepare_sync_drain(&self, run_id: Option<&str>) -> Result<Arc<AtomicBool>> {
        let key = validate_run_id(run_id)?.unwrap_or_else(|| {
            let seq = SYNC_RUN_SEQ.fetch_add(1, Ordering::Relaxed);
            format!("__anon_sync_{seq}")
        });
        let now = Instant::now();
        let mut guard = self
            .sync_drains
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.prune_stale(now);
        if guard
            .active
            .get(&key)
            .is_some_and(|weak| weak.strong_count() > 0)
        {
            return Err(PlanarError::InvalidInput(format!(
                "sync stream runId `{key}` is already active"
            )));
        }
        let pre_stopped = guard.pending_stops.remove(&key).is_some();
        let flag = Arc::new(AtomicBool::new(pre_stopped));
        guard.active.insert(key, Arc::downgrade(&flag));
        Ok(flag)
    }

    /// Signals a specific `run_id` (or all currently active runs when `run_id` is `None`) to stop
    /// dispatching new batches while letting currently in-flight batches drain cleanly.
    /// Rejects invalid `run_id` values rather than falling back to an unscoped stop.
    /// When a valid `run_id` has not registered yet (e.g. still initializing), the Stop intent is
    /// recorded so it takes effect immediately upon registration.
    pub fn request_sync_drain_for(&self, run_id: Option<&str>) -> Result<bool> {
        let validated = validate_run_id(run_id)?;
        let now = Instant::now();
        let mut guard = self
            .sync_drains
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.prune_stale(now);

        if let Some(key) = validated {
            if let Some(flag) = guard.active.get(&key).and_then(Weak::upgrade) {
                flag.store(true, Ordering::Release);
                return Ok(true);
            }
            if guard.pending_stops.len() >= MAX_PENDING_STOPS
                && let Some(oldest_key) = guard
                    .pending_stops
                    .iter()
                    .min_by_key(|(_, ts)| *ts)
                    .map(|(k, _)| k.clone())
            {
                guard.pending_stops.remove(&oldest_key);
            }
            guard.pending_stops.insert(key, now);
            return Ok(true);
        }

        let mut signalled = false;
        guard.active.retain(|_, weak| {
            if let Some(flag) = weak.upgrade() {
                flag.store(true, Ordering::Release);
                signalled = true;
                true
            } else {
                false
            }
        });
        Ok(signalled)
    }

    /// Signals all currently active streaming `Sync` runs on this `AppState` to drain.
    pub fn request_sync_drain(&self) -> bool {
        self.request_sync_drain_for(None).unwrap_or(false)
    }

    /// Runs an incremental scan under `scan_lock` and records the completion timestamp.
    pub async fn run_scan(&self) -> Result<()> {
        let _guard = self.scan_lock.lock().await;
        let db_path = self.db_path.clone();
        let roots = self.roots.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = Store::open(&db_path)?;
            scan_all_harnesses(&mut store, &roots)?;
            Ok::<_, PlanarError>(())
        })
        .await
        .map_err(|e| PlanarError::TaskJoin(e.to_string()))??;
        self.last_scan_ms
            .store(Local::now().timestamp_millis(), Ordering::Relaxed);
        Ok(())
    }

    /// Runs an incremental trajectory scan unless another scan completed within `3s`
    /// (`AUTO_SCAN_THROTTLE_MS`), unless `force` is `true`.
    pub async fn maybe_throttled_scan(&self, force: bool) -> Result<()> {
        if force {
            return self.run_scan().await;
        }
        let now_ms = Local::now().timestamp_millis();
        let prev = self.last_scan_ms.load(Ordering::Relaxed);
        if now_ms.saturating_sub(prev) < AUTO_SCAN_THROTTLE_MS {
            return Ok(());
        }
        let Ok(_guard) = self.scan_lock.try_lock() else {
            return Ok(());
        };
        let db_path = self.db_path.clone();
        let roots = self.roots.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = Store::open(&db_path)?;
            scan_all_harnesses(&mut store, &roots)?;
            Ok::<_, PlanarError>(())
        })
        .await
        .map_err(|e| PlanarError::TaskJoin(e.to_string()))??;
        self.last_scan_ms
            .store(Local::now().timestamp_millis(), Ordering::Relaxed);
        Ok(())
    }

    pub(crate) async fn with_store<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Store) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = Store::open(&db_path)?;
            f(&mut store)
        })
        .await
        .map_err(|e| PlanarError::TaskJoin(e.to_string()))?
    }

    /// Executes a strictly read-only query against an existing SQLite store without creating
    /// directories, creating a missing database file, or running schema migrations.
    pub(crate) async fn with_store_readonly<F, T>(&self, f: F) -> Result<Option<T>>
    where
        F: FnOnce(&Store) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let Some(store) = Store::open_readonly(&db_path)? else {
                return Ok(None);
            };
            f(&store).map(Some)
        })
        .await
        .map_err(|e| PlanarError::TaskJoin(e.to_string()))?
    }

    pub(crate) async fn build_plane_snapshot(
        &self,
        days: usize,
        anchor: Option<NaiveDate>,
    ) -> Result<PlaneResponse> {
        let roots = self.roots.clone();
        let hydrated = self.hydrated_path.clone();
        let mut plane = self
            .with_store(move |store| {
                store.build_plane_response_with_context(
                    days,
                    anchor,
                    Some(&roots),
                    &hydrated,
                    Local::now(),
                )
            })
            .await?;
        plane.last_sync = self.current_last_sync();
        Ok(plane)
    }

    async fn build_plane(&self, query: &PlaneQuery) -> Result<PlaneResponse> {
        self.mutate_and_build_plane(query, |_| Ok(())).await
    }

    async fn build_lite_plane(&self, query: &PlaneQuery) -> Result<LitePlaneResponse> {
        let days = query.days.unwrap_or(7);
        let anchor = parse_anchor_date(query.date.as_deref())?;
        let mut plane = self
            .with_store(move |store| store.build_lite_plane_response(days, anchor, Local::now()))
            .await?;
        plane.last_sync = self.current_last_sync();
        Ok(plane)
    }

    async fn mutate_and_build_plane<F>(
        &self,
        query: &PlaneQuery,
        mutate: F,
    ) -> Result<PlaneResponse>
    where
        F: FnOnce(&mut Store) -> Result<()> + Send + 'static,
    {
        let days = query.days.unwrap_or(7);
        let anchor = parse_anchor_date(query.date.as_deref())?;
        let roots = self.roots.clone();
        let hydrated = self.hydrated_path.clone();
        let mut plane = self
            .with_store(move |store| {
                mutate(store)?;
                store.build_plane_response_with_context(
                    days,
                    anchor,
                    Some(&roots),
                    &hydrated,
                    Local::now(),
                )
            })
            .await?;
        plane.last_sync = self.current_last_sync();
        Ok(plane)
    }

    pub(crate) fn record_last_sync(&self, summary: SyncSummaryDto) {
        let mut guard = self
            .last_sync
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(summary);
    }

    fn current_last_sync(&self) -> Option<SyncSummaryDto> {
        self.last_sync
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn parse_anchor_date(raw: Option<&str>) -> Result<Option<NaiveDate>> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(Some)
        .map_err(|err| PlanarError::InvalidInput(format!("invalid `date` query parameter: {err}")))
}

fn build_api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/mcp", post(post_mcp_handler))
        .route("/plane", get(get_plane_handler))
        .route("/scan", post(post_scan_handler))
        .route("/sync", post(post_sync_handler))
        .route("/sync/estimate", get(get_sync_estimate_handler))
        .route("/sync/stream", post(post_sync_stream_handler))
        .route("/sync/stop", post(post_sync_stop_handler))
        .route("/regroup", post(post_regroup_handler))
        .route("/processor/test", post(post_processor_test_handler))
        .route("/settings", put(put_settings_handler))
        .route("/lines", post(post_line_handler))
        .route("/lines/{id}", patch(patch_line_handler))
        .route("/lines/{id}/merge", post(post_merge_line_handler))
        .route("/sessions/{id}", patch(patch_session_handler))
        .route("/sessions/{id}/open", post(open_session_handler))
        .route("/dependencies", put(put_dependency_handler))
        .route("/dependencies/{from_id}", delete(delete_dependency_handler))
        .with_state(state)
}

fn static_asset(content_type: &'static str, body: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate, max-age=0",
            ),
        ],
        body,
    )
        .into_response()
}

/// Builds the Axum router serving `./ui`, `./assets`, `/mcp`, and `/api/*` from disk.
pub fn build_router(state: AppState, ui_dir: &Path, assets_dir: &Path) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::temporary("/ui/") }))
        .route("/mcp", post(post_mcp_handler).with_state(state.clone()))
        .nest_service("/ui", ServeDir::new(ui_dir))
        .nest_service("/assets", ServeDir::new(assets_dir))
        .nest("/api", build_api_router(state))
        .layer(CorsLayer::permissive())
}

/// Builds a standalone Axum router with `./ui`, `./assets/icon.svg`, and `/mcp` embedded in the binary.
pub fn build_embedded_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::temporary("/ui/") }))
        .route("/mcp", post(post_mcp_handler).with_state(state.clone()))
        .route("/ui", get(|| async { Redirect::temporary("/ui/") }))
        .route(
            "/ui/",
            get(|| async { static_asset("text/html; charset=utf-8", INDEX_HTML) }),
        )
        .route(
            "/ui/index.html",
            get(|| async { static_asset("text/html; charset=utf-8", INDEX_HTML) }),
        )
        .route(
            "/ui/styles.css",
            get(|| async { static_asset("text/css; charset=utf-8", STYLES_CSS) }),
        )
        .route(
            "/ui/app.js",
            get(|| async { static_asset("application/javascript; charset=utf-8", APP_JS) }),
        )
        .route(
            "/ui/defaults.json",
            get(|| async { static_asset("application/json; charset=utf-8", DEFAULTS_JSON) }),
        )
        .route(
            "/assets/icon.svg",
            get(|| async { static_asset("image/svg+xml", ICON_SVG) }),
        )
        .nest("/api", build_api_router(state))
        .layer(CorsLayer::permissive())
}

/// Serves `router` on `listener` until `shutdown` resolves.
pub async fn serve_until<F>(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

struct ApiError(PlanarError);

impl From<PlanarError> for ApiError {
    fn from(value: PlanarError) -> Self {
        Self(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            PlanarError::InvalidInput(_) => StatusCode::BAD_REQUEST,
            PlanarError::NotFound(_) => StatusCode::NOT_FOUND,
            PlanarError::External(_) => StatusCode::BAD_GATEWAY,
            PlanarError::Io(_)
            | PlanarError::Sqlite(_)
            | PlanarError::Json(_)
            | PlanarError::TaskJoin(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ErrorBody {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
struct PlaneQuery {
    days: Option<usize>,
    rescan: Option<u8>,
    date: Option<String>,
    lite: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncQuery {
    days: Option<usize>,
    date: Option<String>,
    harness: Option<String>,
    force: Option<u8>,
    limit: Option<usize>,
    run_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncStopQuery {
    run_id: Option<String>,
}

impl SyncQuery {
    fn as_plane_query(&self) -> PlaneQuery {
        PlaneQuery {
            days: self.days,
            rescan: None,
            date: self.date.clone(),
            lite: None,
        }
    }

    fn to_scope(&self) -> Result<SyncScope> {
        Ok(SyncScope {
            days: self.days.unwrap_or(7),
            anchor_date: parse_anchor_date(self.date.as_deref())?,
            harness: self.harness.clone(),
        })
    }
}

struct NdjsonEventStream {
    rx: tokio::sync::mpsc::Receiver<SyncStreamEvent>,
}

impl futures_core::Stream for NdjsonEventStream {
    type Item = std::result::Result<Bytes, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            std::task::Poll::Ready(Some(event)) => {
                let mut buf = serde_json::to_vec(&event).unwrap_or_else(|_| b"{}".to_vec());
                buf.push(b'\n');
                std::task::Poll::Ready(Some(Ok(Bytes::from(buf))))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

#[derive(Serialize)]
struct HealthBody {
    ok: bool,
}

async fn health_handler() -> Json<HealthBody> {
    Json(HealthBody { ok: true })
}

async fn post_mcp_handler(State(state): State<AppState>, body: Bytes) -> Response {
    match crate::mcp::handle_mcp_json_bytes(&state, &body).await {
        crate::mcp::McpDispatchOutcome::Response(val) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store, no-cache")],
            Json(val),
        )
            .into_response(),
        crate::mcp::McpDispatchOutcome::NoResponse => {
            (StatusCode::ACCEPTED, Body::empty()).into_response()
        }
        crate::mcp::McpDispatchOutcome::ParseError(val) => (
            StatusCode::BAD_REQUEST,
            [(header::CACHE_CONTROL, "no-store, no-cache")],
            Json(val),
        )
            .into_response(),
    }
}

async fn get_plane_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaneQuery>,
) -> ApiResult<Response> {
    if query.lite == Some(1) {
        let lite = state.build_lite_plane(&query).await?;
        return Ok(Json(lite).into_response());
    }
    let force = query.rescan == Some(1);
    state.maybe_throttled_scan(force).await?;
    let plane = state.build_plane(&query).await?;
    Ok(Json(plane).into_response())
}

async fn post_scan_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaneQuery>,
) -> ApiResult<Json<PlaneResponse>> {
    state.run_scan().await?;
    let plane = state.build_plane(&query).await?;
    Ok(Json(plane))
}

async fn get_sync_estimate_handler(
    State(state): State<AppState>,
    Query(query): Query<SyncQuery>,
) -> ApiResult<Json<SyncEstimateDto>> {
    let scope = query.to_scope()?;
    let estimate =
        processor::estimate_sync_cost(&state, query.force == Some(1), query.limit, Some(scope))
            .await?;
    Ok(Json(estimate))
}

async fn post_sync_handler(
    State(state): State<AppState>,
    Query(query): Query<SyncQuery>,
) -> ApiResult<Json<PlaneResponse>> {
    let scope = query.to_scope()?;
    state.run_scan().await?;
    let summary = processor::sync_pending_sessions_in_scope(
        &state,
        query.force == Some(1),
        query.limit,
        Some(scope),
    )
    .await?;
    state.record_last_sync(summary);
    let plane = state.build_plane(&query.as_plane_query()).await?;
    Ok(Json(plane))
}

async fn post_sync_stream_handler(
    State(state): State<AppState>,
    Query(query): Query<SyncQuery>,
) -> ApiResult<Response> {
    let scope = query.to_scope()?;
    // Validate and register drain handle BEFORE `run_scan()` so invalid/duplicate runIds are
    // rejected up front and a Stop click during scan is never lost.
    let drain_flag = state.prepare_sync_drain(query.run_id.as_deref())?;
    state.run_scan().await?;
    let rx = processor::spawn_sync_stream_with_drain(
        state,
        scope,
        query.force == Some(1),
        query.limit,
        drain_flag,
    );
    let body = Body::from_stream(NdjsonEventStream { rx });
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store, no-cache"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        body,
    )
        .into_response())
}

#[derive(Serialize)]
struct SyncStopBody {
    ok: bool,
    draining: bool,
}

async fn post_sync_stop_handler(
    State(state): State<AppState>,
    Query(query): Query<SyncStopQuery>,
) -> ApiResult<Json<SyncStopBody>> {
    let draining = state.request_sync_drain_for(query.run_id.as_deref())?;
    Ok(Json(SyncStopBody { ok: true, draining }))
}

async fn post_regroup_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaneQuery>,
) -> ApiResult<Json<PlaneResponse>> {
    let summary = processor::regroup_discovered_lines(&state).await?;
    state.record_last_sync(summary);
    let plane = state.build_plane(&query).await?;
    Ok(Json(plane))
}

async fn post_processor_test_handler(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<Json<ProcessorTestResult>> {
    let settings = if body.iter().all(u8::is_ascii_whitespace) {
        state.with_store(|store| store.get_settings()).await?
    } else {
        serde_json::from_slice::<AppSettings>(&body)
            .map_err(|err| PlanarError::InvalidInput(format!("invalid settings payload: {err}")))?
    };
    let result = processor::test_processor_connection_with_client(
        &settings,
        &state.hydrated_path,
        &state.http_client,
    )
    .await?;
    Ok(Json(result))
}

async fn put_settings_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaneQuery>,
    Json(settings): Json<AppSettings>,
) -> ApiResult<Json<PlaneResponse>> {
    let plane = state
        .mutate_and_build_plane(&query, move |store| store.save_settings(&settings))
        .await?;
    Ok(Json(plane))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateLineBody {
    name: String,
    #[serde(default)]
    description: String,
}

async fn post_line_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaneQuery>,
    Json(body): Json<CreateLineBody>,
) -> ApiResult<Json<PlaneResponse>> {
    let plane = state
        .mutate_and_build_plane(&query, move |store| {
            store.create_line_with_description(&body.name, &body.description)?;
            Ok(())
        })
        .await?;
    Ok(Json(plane))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchLineBody {
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
    priority: Option<u8>,
    status: Option<String>,
    pinned: Option<bool>,
}

async fn patch_line_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<PlaneQuery>,
    Json(body): Json<PatchLineBody>,
) -> ApiResult<Json<PlaneResponse>> {
    let update = LineUpdate {
        name: body.name,
        description: body.description,
        color: body.color,
        priority: body.priority,
        status: body.status,
        pinned: body.pinned,
    };
    let plane = state
        .mutate_and_build_plane(&query, move |store| store.update_line(&id, &update))
        .await?;
    Ok(Json(plane))
}

const fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeLineBody {
    target_line_id: String,
    #[serde(default = "default_true")]
    pin_target: bool,
    #[serde(default = "default_true")]
    append_scope_hint: bool,
}

async fn post_merge_line_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<PlaneQuery>,
    Json(body): Json<MergeLineBody>,
) -> ApiResult<Json<PlaneResponse>> {
    let plane = state
        .mutate_and_build_plane(&query, move |store| {
            store.merge_lines(
                &id,
                &body.target_line_id,
                body.pin_target,
                body.append_scope_hint,
            )?;
            Ok(())
        })
        .await?;
    Ok(Json(plane))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchSessionBody {
    #[serde(default)]
    line_id: LineIdUpdate,
    hidden: Option<bool>,
}

async fn patch_session_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<PlaneQuery>,
    Json(body): Json<PatchSessionBody>,
) -> ApiResult<Json<PlaneResponse>> {
    let plane = state
        .mutate_and_build_plane(&query, move |store| {
            store.update_session(&id, &body.line_id, body.hidden)
        })
        .await?;
    Ok(Json(plane))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutDependencyBody {
    from_session_id: String,
    to_session_id: String,
    kind: Option<String>,
}

async fn put_dependency_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaneQuery>,
    Json(body): Json<PutDependencyBody>,
) -> ApiResult<Json<PlaneResponse>> {
    let plane = state
        .mutate_and_build_plane(&query, move |store| {
            let kind = body.kind.as_deref().unwrap_or("depends_on");
            store.upsert_dependency(&body.from_session_id, &body.to_session_id, kind, "manual")
        })
        .await?;
    Ok(Json(plane))
}

async fn delete_dependency_handler(
    State(state): State<AppState>,
    AxumPath(from_id): AxumPath<String>,
    Query(query): Query<PlaneQuery>,
) -> ApiResult<Json<PlaneResponse>> {
    let plane = state
        .mutate_and_build_plane(&query, move |store| {
            store.delete_dependencies_from(&from_id)
        })
        .await?;
    Ok(Json(plane))
}

/// Outcome returned when opening or copying a resume command for a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSessionResponse {
    pub ok: bool,
    pub harness: String,
    pub action: String,
    pub message: String,
    pub resume_command: Option<String>,
}

#[cfg(all(target_os = "macos", not(test)))]
fn copy_to_macos_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let Ok(mut child) = Command::new("/usr/bin/pbcopy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().is_ok_and(|s| s.success())
}

/// Opens a session workspace in its source app (macOS Antigravity) or copies its CLI resume command.
#[cfg(all(target_os = "macos", not(test)))]
pub fn execute_session_open(info: &SessionLaunchInfo) -> OpenSessionResponse {
    use std::process::{Command, Stdio};

    let resume_command =
        crate::env::build_resume_command(&info.harness, &info.id, info.cwd.as_deref());
    let cwd_dir = info.cwd.as_deref().filter(|p| Path::new(p).is_dir());

    if info.harness == "antigravity"
        && Path::new("/Applications/Antigravity.app").exists()
        && let Some(cwd) = cwd_dir
        && Command::new("/usr/bin/open")
            .args(["-a", "/Applications/Antigravity.app", cwd])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    {
        return OpenSessionResponse {
            ok: true,
            harness: info.harness.clone(),
            action: "opened_antigravity".to_string(),
            message: "Opened workspace in Antigravity".to_string(),
            resume_command,
        };
    }

    if let Some(ref cmd) = resume_command {
        let copied = copy_to_macos_clipboard(cmd);
        return OpenSessionResponse {
            ok: copied,
            harness: info.harness.clone(),
            action: "copied_resume_command".to_string(),
            message: if copied {
                "Copied resume command to clipboard".to_string()
            } else {
                cmd.clone()
            },
            resume_command,
        };
    }

    if let Some(cwd) = cwd_dir
        && Command::new("/usr/bin/open")
            .arg(cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    {
        return OpenSessionResponse {
            ok: true,
            harness: info.harness.clone(),
            action: "opened_workspace".to_string(),
            message: "Opened workspace folder".to_string(),
            resume_command: None,
        };
    }

    if !info.source_path.is_empty()
        && Path::new(&info.source_path).exists()
        && Command::new("/usr/bin/open")
            .args(["-R", &info.source_path])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    {
        return OpenSessionResponse {
            ok: true,
            harness: info.harness.clone(),
            action: "revealed_transcript".to_string(),
            message: "Revealed transcript in Finder".to_string(),
            resume_command: None,
        };
    }

    OpenSessionResponse {
        ok: false,
        harness: info.harness.clone(),
        action: "no_target".to_string(),
        message: "No local workspace or transcript found to open".to_string(),
        resume_command: None,
    }
}

/// Resolves the session resume command or workspace launch metadata on non-macOS / test targets.
#[cfg(any(not(target_os = "macos"), test))]
pub fn execute_session_open(info: &SessionLaunchInfo) -> OpenSessionResponse {
    let resume_command =
        crate::env::build_resume_command(&info.harness, &info.id, info.cwd.as_deref());
    if info.harness == "antigravity"
        && let Some(cwd) = info.cwd.as_deref()
        && Path::new(cwd).is_dir()
    {
        return OpenSessionResponse {
            ok: true,
            harness: info.harness.clone(),
            action: "opened_antigravity".to_string(),
            message: "Opened workspace in Antigravity".to_string(),
            resume_command,
        };
    }
    if resume_command.is_some() {
        return OpenSessionResponse {
            ok: true,
            harness: info.harness.clone(),
            action: "copied_resume_command".to_string(),
            message: "Copied resume command to clipboard".to_string(),
            resume_command,
        };
    }
    OpenSessionResponse {
        ok: true,
        harness: info.harness.clone(),
        action: "no_target".to_string(),
        message: "Session metadata resolved".to_string(),
        resume_command: None,
    }
}

async fn open_session_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<OpenSessionResponse>> {
    let info = state
        .with_store(move |store| store.get_session_launch_info(&id))
        .await?;
    let resp = tokio::task::spawn_blocking(move || execute_session_open(&info))
        .await
        .map_err(|e| PlanarError::TaskJoin(e.to_string()))?;
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::*;

    fn test_roots(base: &Path) -> HarnessRoots {
        HarnessRoots {
            claude_projects: base.join("claude"),
            codex_sessions: base.join("codex").join("sessions"),
            codex_archived: base.join("codex").join("archived_sessions"),
            antigravity_brain: base.join("antigravity"),
            dsh_sessions: base.join("dsh").join("sessions"),
        }
    }

    async fn send_json(
        app: Router,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let req_builder = Request::builder().method(method).uri(uri);
        let req = match body {
            Some(val) => req_builder
                .header("content-type", "application/json")
                .body(Body::from(val.to_string()))
                .unwrap(),
            None => req_builder.body(Body::empty()).unwrap(),
        };
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn api_endpoints_scan_create_lines_assign_sessions_and_link_dependencies() {
        let dir = TempDir::new().unwrap();
        let roots = test_roots(dir.path());
        let sess_dir = roots.codex_sessions.join("2026").join("11").join("14");
        fs::create_dir_all(&sess_dir).unwrap();

        let now = Local::now();
        let t1 = (now - chrono::Duration::minutes(20)).to_rfc3339();
        let t2 = (now - chrono::Duration::minutes(18)).to_rfc3339();
        let t3 = (now - chrono::Duration::minutes(15)).to_rfc3339();

        for idx in 1..=2 {
            let content = format!(
                concat!(
                    "{{\"timestamp\":\"{t1}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"s{idx}\",\"cwd\":\"/tmp/ws\",\"source\":\"vscode\"}}}}\n",
                    "{{\"timestamp\":\"{t1}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 1 for s{idx}\"}}]}}}}\n",
                    "{{\"timestamp\":\"{t2}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 2 for s{idx}\"}}]}}}}\n",
                    "{{\"timestamp\":\"{t3}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 3 for s{idx}\"}}]}}}}\n"
                ),
                t1 = t1,
                t2 = t2,
                t3 = t3,
                idx = idx,
            );
            fs::write(sess_dir.join(format!("rollout-{idx}.jsonl")), content).unwrap();
        }

        let db_path = dir.path().join("planar.db");
        let state = AppState::new(db_path, roots);
        let ui_dir = dir.path().join("ui");
        let assets_dir = dir.path().join("assets");
        fs::create_dir_all(&ui_dir).unwrap();
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(ui_dir.join("index.html"), "<html>ok</html>").unwrap();
        fs::write(ui_dir.join("app.js"), "console.log('ok');").unwrap();
        fs::write(ui_dir.join("defaults.json"), DEFAULTS_JSON).unwrap();
        let app = build_router(state, &ui_dir, &assets_dir);

        let ui_res = app
            .clone()
            .oneshot(Request::builder().uri("/ui/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(ui_res.status(), StatusCode::OK);

        let js_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ui/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(js_res.status(), StatusCode::OK);

        let (defaults_st, defaults_json) =
            send_json(app.clone(), Method::GET, "/ui/defaults.json", None).await;
        assert_eq!(defaults_st, StatusCode::OK);
        assert!(defaults_json["apiProviders"].is_array());

        let (st, plane) = send_json(app.clone(), Method::GET, "/api/plane?days=7", None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(plane["unsummarized"].as_array().unwrap().len(), 2);

        // Pre-flight read-only Sync cost estimate (default local CLI mode -> no fabricated API cost).
        let db_before = fs::read(dir.path().join("planar.db")).unwrap();
        let (est_st, cli_est) =
            send_json(app.clone(), Method::GET, "/api/sync/estimate?days=7", None).await;
        assert_eq!(est_st, StatusCode::OK);
        assert_eq!(cli_est["sessionCount"], 2);
        assert_eq!(cli_est["batchCount"], 1);
        assert_eq!(cli_est["isLocalCli"], true);
        assert!(cli_est["estimatedInputTokens"].as_u64().unwrap() > 0);
        assert!(cli_est["estimatedOutputTokens"].as_u64().unwrap() > 0);
        assert!(cli_est.get("estimatedCostUsdRange").is_none());
        let db_after = fs::read(dir.path().join("planar.db")).unwrap();
        assert_eq!(
            db_before, db_after,
            "GET /api/sync/estimate must not mutate SQLite database bytes"
        );

        // GET /api/sync/estimate on a non-existent DB must return 0 sessions without creating the DB.
        let missing_db = dir.path().join("uncreated").join("planar.db");
        let missing_app =
            build_embedded_router(AppState::new(missing_db.clone(), test_roots(dir.path())));
        let (missing_st, missing_est) =
            send_json(missing_app, Method::GET, "/api/sync/estimate?days=7", None).await;
        assert_eq!(missing_st, StatusCode::OK);
        assert_eq!(missing_est["sessionCount"], 0);
        assert!(
            !missing_db.exists(),
            "GET /api/sync/estimate must never create a missing SQLite database"
        );

        // Switch to direct Gemini API mode and verify pre-flight USD range is computed offline.
        let (cfg_st, _) = send_json(
            app.clone(),
            Method::PUT,
            "/api/settings",
            Some(serde_json::json!({
                "processorConfigured": true,
                "processorMode": "api",
                "apiProvider": "gemini",
                "apiModel": "gemini-4-hydrogen"
            })),
        )
        .await;
        assert_eq!(cfg_st, StatusCode::OK);
        let (api_est_st, api_est) =
            send_json(app.clone(), Method::GET, "/api/sync/estimate?days=7", None).await;
        assert_eq!(api_est_st, StatusCode::OK);
        assert_eq!(api_est["sessionCount"], 2);
        assert_eq!(api_est["batchCount"], 1);
        assert_eq!(api_est["isLocalCli"], false);
        assert_eq!(api_est["model"], "gemini-4-hydrogen");
        assert!(api_est["estimatedThinkingTokens"].as_u64().unwrap() > 0);
        let low = api_est["estimatedCostUsdRange"]["low"].as_f64().unwrap();
        let high = api_est["estimatedCostUsdRange"]["high"].as_f64().unwrap();
        assert!(
            low > 0.0 && high >= low,
            "expected positive USD range: {api_est}"
        );

        let (st, created) = send_json(
            app.clone(),
            Method::POST,
            "/api/lines",
            Some(serde_json::json!({ "name": "Planar Core" })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let new_line_id = created["lines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "Planar Core")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        for sid in ["codex:s1", "codex:s2"] {
            let (st, _) = send_json(
                app.clone(),
                Method::PATCH,
                &format!("/api/sessions/{sid}"),
                Some(serde_json::json!({ "lineId": new_line_id })),
            )
            .await;
            assert_eq!(st, StatusCode::OK);
        }

        let (st, dep_plane) = send_json(
            app.clone(),
            Method::PUT,
            "/api/dependencies",
            Some(serde_json::json!({
                "fromSessionId": "codex:s2",
                "toSessionId": "codex:s1"
            })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let target_line = dep_plane["lines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == new_line_id)
            .unwrap();
        let s2 = target_line["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "codex:s2")
            .unwrap();
        assert_eq!(s2["dependsOnId"], "codex:s1");

        let (st, open_resp) = send_json(
            app.clone(),
            Method::POST,
            "/api/sessions/codex:s1/open",
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(open_resp["ok"], true);
        assert_eq!(open_resp["resumeCommand"], "codex resume 's1'");

        let (st, updated_settings) = send_json(
            app.clone(),
            Method::PUT,
            "/api/settings",
            Some(serde_json::json!({ "fewestUserInputs": 4, "showShortOnTimeline": true, "metricLens": "cost" })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(updated_settings["settings"]["fewestUserInputs"], 4);
        assert_eq!(updated_settings["settings"]["showShortOnTimeline"], true);
        assert_eq!(updated_settings["settings"]["metricLens"], "cost");

        // Create a second line with description, then merge it into the first line via API
        let (st, created_second) = send_json(
            app.clone(),
            Method::POST,
            "/api/lines",
            Some(serde_json::json!({ "name": "rank-worker", "description": "Ranking worker service" })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let rank_worker = created_second["lines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "rank-worker")
            .unwrap();
        assert_eq!(rank_worker["description"], "Ranking worker service");
        let rank_worker_id = rank_worker["id"].as_str().unwrap().to_string();

        let (st, merged_plane) = send_json(
            app,
            Method::POST,
            &format!("/api/lines/{rank_worker_id}/merge"),
            Some(serde_json::json!({
                "targetLineId": new_line_id,
                "pinTarget": true,
                "appendScopeHint": true
            })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert!(
            !merged_plane["lines"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l["id"] == rank_worker_id)
        );
        let planar_line = merged_plane["lines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == new_line_id)
            .unwrap();
        assert!(
            planar_line["description"]
                .as_str()
                .unwrap()
                .contains("rank-worker")
        );
    }

    #[tokio::test]
    async fn embedded_router_serves_ui_assets_and_api() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("planar.db");
        Store::open(&db_path).unwrap();
        let app = build_embedded_router(AppState::new(db_path, test_roots(dir.path())));

        for uri in [
            "/ui/",
            "/ui/styles.css",
            "/ui/app.js",
            "/ui/defaults.json",
            "/assets/icon.svg",
        ] {
            let res = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "failed embedded asset {uri}");
        }

        let (defaults_st, defaults_json) =
            send_json(app.clone(), Method::GET, "/ui/defaults.json", None).await;
        assert_eq!(defaults_st, StatusCode::OK);
        assert_eq!(
            defaults_json["palette"].as_array().unwrap().len(),
            crate::defaults::EXPECTED_PALETTE_LEN
        );

        let (st, health) = send_json(app.clone(), Method::GET, "/api/health", None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(health["ok"], true);

        let (st, month_plane) =
            send_json(app.clone(), Method::GET, "/api/plane?days=30", None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(month_plane["days"].as_array().unwrap().len(), 30);
        assert!(month_plane["contributions"].as_array().unwrap().len() >= 134);
        assert_eq!(month_plane["harnesses"].as_array().unwrap().len(), 4);

        let (lite_st, lite_plane) =
            send_json(app.clone(), Method::GET, "/api/plane?days=7&lite=1", None).await;
        assert_eq!(lite_st, StatusCode::OK);
        assert_eq!(lite_plane["isLite"], true);
        assert_eq!(lite_plane["days"].as_array().unwrap().len(), 7);
        assert!(lite_plane.get("contributions").is_none());
        assert!(lite_plane.get("harnesses").is_none());

        let (stop_st, stop_body) =
            send_json(app.clone(), Method::POST, "/api/sync/stop", None).await;
        assert_eq!(stop_st, StatusCode::OK);
        assert_eq!(stop_body["ok"], true);
        assert_eq!(stop_body["draining"], false);

        let (scoped_stop_st, scoped_stop_body) = send_json(
            app.clone(),
            Method::POST,
            "/api/sync/stop?runId=pre-reg-run",
            None,
        )
        .await;
        assert_eq!(scoped_stop_st, StatusCode::OK);
        assert_eq!(scoped_stop_body["ok"], true);
        assert_eq!(scoped_stop_body["draining"], true);

        // Invalid runIds are rejected with 400 Bad Request rather than falling back to unscoped stop.
        for bad_uri in [
            "/api/sync/stop?runId=",
            "/api/sync/stop?runId=__reserved",
            "/api/sync/stop?runId=bad%20id",
            "/api/sync/stream?runId=bad%20id",
        ] {
            let (bad_st, _) = send_json(app.clone(), Method::POST, bad_uri, None).await;
            assert_eq!(
                bad_st,
                StatusCode::BAD_REQUEST,
                "expected 400 for {bad_uri}"
            );
        }

        // Duplicate active runId registration on `/api/sync/stream` returns 400 Bad Request.
        let dup_state = AppState::new(dir.path().join("planar.db"), test_roots(dir.path()));
        let _active_guard = dup_state.prepare_sync_drain(Some("dup-live")).unwrap();
        let dup_app = build_embedded_router(dup_state);
        let (dup_st, _) = send_json(
            dup_app,
            Method::POST,
            "/api/sync/stream?runId=dup-live",
            None,
        )
        .await;
        assert_eq!(dup_st, StatusCode::BAD_REQUEST);

        let stream_resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/sync/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stream_resp.status(), StatusCode::OK);
        let raw_ndjson = to_bytes(stream_resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let events: Vec<serde_json::Value> = std::str::from_utf8(&raw_ndjson)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "start");
        assert_eq!(events[0]["total"], 0);
        assert_eq!(events[1]["type"], "complete");

        // Verify Streamable HTTP MCP endpoint (`POST /mcp`) on embedded router
        let (mcp_st, mcp_init) = send_json(
            build_embedded_router(AppState::new(
                dir.path().join("planar.db"),
                test_roots(dir.path()),
            )),
            Method::POST,
            "/mcp",
            Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2025-03-26" }
            })),
        )
        .await;
        assert_eq!(mcp_st, StatusCode::OK);
        assert_eq!(mcp_init["result"]["protocolVersion"], "2025-03-26");

        let notif_resp = build_embedded_router(AppState::new(
            dir.path().join("planar.db"),
            test_roots(dir.path()),
        ))
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(notif_resp.status(), StatusCode::ACCEPTED);

        let bad_resp = build_embedded_router(AppState::new(
            dir.path().join("planar.db"),
            test_roots(dir.path()),
        ))
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(Body::from("{broken-json"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(bad_resp.status(), StatusCode::BAD_REQUEST);
    }
}
