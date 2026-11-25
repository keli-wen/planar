use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::db::{
    LineCatalogEntry, SessionDigestWrite, SyncCandidate, SyncScope, UnpinnedClusterItem,
};
use crate::defaults::{
    API_RETRY_BASE_DELAY_MS, API_RETRY_MAX_DELAY_MS, API_TIMEOUT_SECS, API_TRANSIENT_MAX_RETRIES,
    CLI_TIMEOUT_SECS, FEWEST_USER_INPUTS_MIN, MAX_BATCH_PROMPT_CHARS, SLICER_CHAR_BUDGET_MIN,
    STREAM_HEARTBEAT_SECS, SYNC_BATCH_LIMIT_MAX, SYNC_BATCH_LIMIT_MIN, SYNC_CLI_CONCURRENCY_CAP,
    SYNC_CONCURRENCY_MAX, SYNC_CONCURRENCY_MIN, WINDOW_DAYS_MAX, WINDOW_DAYS_MIN, api_provider,
    cli_engine_order,
};
use crate::env::resolve_cli_binary;
use crate::error::{PlanarError, Result};
use crate::ingest::antigravity::extract_antigravity_user_request;
use crate::ingest::claude::is_claude_synthetic_wrapper;
use crate::ingest::codex::is_codex_synthetic_block;
use crate::ingest::dsh::extract_text_from_value;
use crate::ingest::{stream_jsonl_lines, strip_delegation_envelope};
use crate::model::{
    AppSettings, CostUsdRangeDto, INTERNAL_DIGEST_SENTINEL, ProcessorTestResult, SyncBatchDoneDto,
    SyncBatchPreviewItem, SyncEstimateDto, SyncFiledPreview, SyncStreamEvent, SyncSummaryDto,
    SyncUsageDto,
};
use crate::pricing::{canonical_billing_provider, estimate_api_cost_usd};
use crate::server::AppState;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);
static RETRY_JITTER_SEQ: AtomicU64 = AtomicU64::new(1);

/// Constructs the shared `reqwest::Client` held on `AppState` so concurrent `Sync` requests
/// reuse TLS sessions and HTTP/2 connection pools while preserving `API_TIMEOUT_SECS`.
pub(crate) fn build_shared_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(API_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Resolves the effective concurrent batch count for `settings`: `1..=16` in direct API mode
/// (default `4`), and capped at `SYNC_CLI_CONCURRENCY_CAP` (`2`) in local CLI subprocess mode.
pub(crate) fn effective_sync_concurrency(settings: &AppSettings) -> usize {
    let configured = settings
        .sync_concurrency
        .clamp(SYNC_CONCURRENCY_MIN, SYNC_CONCURRENCY_MAX);
    if settings.processor_mode == "api" {
        configured
    } else {
        configured.min(SYNC_CLI_CONCURRENCY_CAP)
    }
}

/// Single dialogue turn extracted cleanly for Head–Middle–Tail trajectory slicing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlicedStep {
    pub index: usize,
    pub role: &'static str,
    pub text: String,
}

/// Stripped Head–Middle–Tail representation of one trajectory (`contexts/project/design.md §2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlicedTrajectory {
    pub total_steps: usize,
    pub user_count: usize,
    pub head: Vec<SlicedStep>,
    pub middle_users: Vec<SlicedStep>,
    pub tail: Vec<SlicedStep>,
}

/// Structured recap and Line assignment produced by the configured trajectory processor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedDigest {
    pub title: String,
    pub stopped_at: String,
    pub left_open: String,
    pub suggested_line: String,
    pub is_one_off: bool,
}

/// Computes the deterministic SHA-256 hash of `AppSettings` slicing parameters so cached
/// digests invalidate automatically when `opening_steps`, `closing_steps`, char budgets,
/// or `fewest_user_inputs` change.
pub fn compute_slice_config_hash(settings: &AppSettings) -> String {
    let raw = format!(
        "open={};close={};u={};a={};min_u={}",
        settings.opening_steps,
        settings.closing_steps,
        settings.longest_user_chars,
        settings.longest_agent_chars,
        settings.fewest_user_inputs
    );
    let digest = Sha256::digest(raw.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Streams a read-only source JSONL trajectory and returns its Head–Middle–Tail slice,
/// stripping tool payloads and ignoring `[PLANAR_INTERNAL_DIGEST]` echoes.
pub fn slice_trajectory_file(
    source_path: &Path,
    harness: &str,
    settings: &AppSettings,
) -> Result<SlicedTrajectory> {
    let mut raw_turns: Vec<(usize, &'static str, String)> = Vec::new();
    let mut user_count = 0_usize;
    let user_limit = usize::try_from(settings.longest_user_chars)
        .unwrap_or(usize::MAX)
        .max(SLICER_CHAR_BUDGET_MIN);
    let agent_limit = usize::try_from(settings.longest_agent_chars)
        .unwrap_or(usize::MAX)
        .max(SLICER_CHAR_BUDGET_MIN);

    stream_jsonl_lines(source_path, 0, |line| {
        let Some((role, text)) = extract_step_text_fast(harness, line) else {
            return Ok(());
        };
        if text.contains(INTERNAL_DIGEST_SENTINEL) {
            return Ok(());
        }
        let cleaned = normalize_step_text(&text);
        if cleaned.is_empty() {
            return Ok(());
        }
        let max_len = if role == "user" {
            user_count += 1;
            user_limit
        } else {
            agent_limit
        };
        let idx = raw_turns.len() + 1;
        raw_turns.push((idx, role, truncate_chars(&cleaned, max_len)));
        Ok(())
    })?;

    let total_steps = raw_turns.len();
    let open_n = usize::try_from(settings.opening_steps).unwrap_or(usize::MAX);
    let close_n = usize::try_from(settings.closing_steps).unwrap_or(usize::MAX);
    let head_end = open_n.min(total_steps);
    let tail_start = total_steps.saturating_sub(close_n).max(head_end);

    let to_step = |(index, role, text): &(usize, &'static str, String)| SlicedStep {
        index: *index,
        role,
        text: text.clone(),
    };

    let head = raw_turns[..head_end].iter().map(to_step).collect();
    let middle_users = raw_turns[head_end..tail_start]
        .iter()
        .filter(|(_, role, _)| *role == "user")
        .map(to_step)
        .collect();
    let tail = raw_turns[tail_start..].iter().map(to_step).collect();

    Ok(SlicedTrajectory {
        total_steps,
        user_count,
        head,
        middle_users,
        tail,
    })
}

#[derive(Deserialize)]
struct RawDigest {
    #[serde(default, alias = "sessionId", alias = "session_id", alias = "id")]
    session_id: String,
    #[serde(default)]
    title: String,
    #[serde(default, alias = "stoppedAt", alias = "stopped_at")]
    stopped_at: String,
    #[serde(default, alias = "leftOpen", alias = "left_open")]
    left_open: String,
    #[serde(
        default,
        alias = "suggestedLine",
        alias = "suggested_line",
        alias = "line"
    )]
    suggested_line: String,
    #[serde(default, alias = "isOneOff", alias = "is_one_off")]
    is_one_off: bool,
}

fn finalize_raw_digest(parsed: &RawDigest) -> Result<GeneratedDigest> {
    let title = parsed.title.trim().to_string();
    if title.is_empty() {
        return Err(PlanarError::InvalidInput(
            "processor digest JSON omitted non-empty `title`".into(),
        ));
    }

    let suggested_line = parsed.suggested_line.trim().to_string();
    let is_one_off = parsed.is_one_off
        || suggested_line.is_empty()
        || suggested_line.eq_ignore_ascii_case("one-off");

    Ok(GeneratedDigest {
        title,
        stopped_at: parsed.stopped_at.trim().to_string(),
        left_open: parsed.left_open.trim().to_string(),
        suggested_line: if is_one_off {
            "One-off".to_string()
        } else {
            suggested_line
        },
        is_one_off,
    })
}

/// Parses a single-session digest JSON payload, tolerating Markdown code fences.
pub fn parse_digest_json(raw: &str) -> Result<GeneratedDigest> {
    let json_slice = extract_json_object(raw).ok_or_else(|| {
        PlanarError::InvalidInput("processor reply did not contain a JSON object".into())
    })?;
    let parsed: RawDigest = serde_json::from_str(json_slice)
        .map_err(|err| PlanarError::InvalidInput(format!("failed to parse digest JSON: {err}")))?;
    finalize_raw_digest(&parsed)
}

/// Parses a multi-session batch digest JSON payload mapping `sessionId -> GeneratedDigest`,
/// tolerating wrapped `{"digests": {...}}`, top-level object maps, arrays, and 1-item objects.
pub fn parse_batch_digest_json(
    raw: &str,
    expected_ids: &[String],
) -> Result<HashMap<String, GeneratedDigest>> {
    if expected_ids.len() == 1
        && let Ok(single) = parse_digest_json(raw)
    {
        let mut map = HashMap::with_capacity(1);
        map.insert(expected_ids[0].clone(), single);
        return Ok(map);
    }

    let json_slice = extract_json_payload(raw).ok_or_else(|| {
        PlanarError::InvalidInput("batch processor reply did not contain JSON".into())
    })?;
    let root: Value = serde_json::from_str(json_slice).map_err(|err| {
        PlanarError::InvalidInput(format!("failed to parse batch digest JSON: {err}"))
    })?;

    let mut raw_entries: Vec<(String, Value)> = Vec::new();
    match root {
        Value::Object(mut obj) => {
            if let Some(inner) = obj.remove("digests").or_else(|| obj.remove("sessions")) {
                match inner {
                    Value::Object(map) => raw_entries.extend(map),
                    Value::Array(arr) => {
                        for item in arr {
                            if let Some(id) = item
                                .get("sessionId")
                                .or_else(|| item.get("session_id"))
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                            {
                                raw_entries.push((id.to_string(), item));
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                raw_entries.extend(obj);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(id) = item
                    .get("sessionId")
                    .or_else(|| item.get("session_id"))
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                {
                    raw_entries.push((id.to_string(), item));
                }
            }
        }
        _ => {}
    }

    let mut out = HashMap::new();
    for (raw_key, val) in raw_entries {
        let Ok(raw_d) = serde_json::from_value::<RawDigest>(val) else {
            continue;
        };
        let key_candidate = if raw_key.trim().is_empty() {
            raw_d.session_id.trim()
        } else {
            raw_key.trim()
        };
        let matched_id = expected_ids
            .iter()
            .find(|exp| exp.as_str() == key_candidate || exp.eq_ignore_ascii_case(key_candidate))
            .cloned()
            .unwrap_or_else(|| key_candidate.to_string());
        if let Ok(digest) = finalize_raw_digest(&raw_d) {
            out.insert(matched_id, digest);
        }
    }

    if out.is_empty() {
        return Err(PlanarError::InvalidInput(
            "batch digest JSON contained no valid session digests".into(),
        ));
    }
    Ok(out)
}

/// Parses a `Regroup` clustering response mapping `sessionId -> lineName`.
pub fn parse_regroup_json(raw: &str) -> Result<HashMap<String, String>> {
    #[derive(Deserialize)]
    struct Wrapper {
        assignments: HashMap<String, String>,
    }

    let json_slice = extract_json_object(raw).ok_or_else(|| {
        PlanarError::InvalidInput("regroup reply did not contain a JSON object".into())
    })?;

    if let Ok(wrapped) = serde_json::from_str::<Wrapper>(json_slice) {
        return Ok(wrapped.assignments);
    }

    serde_json::from_str::<HashMap<String, String>>(json_slice)
        .map_err(|err| PlanarError::InvalidInput(format!("failed to parse regroup JSON: {err}")))
}

/// Tests connectivity and round-trip latency for the active processor mode (`cli` or `api`).
pub async fn test_processor_connection(
    settings: &AppSettings,
    hydrated_path: &OsStr,
) -> Result<ProcessorTestResult> {
    let client = build_shared_http_client();
    test_processor_connection_with_client(settings, hydrated_path, &client).await
}

/// Tests connectivity and round-trip latency reusing a shared `reqwest::Client`.
pub async fn test_processor_connection_with_client(
    settings: &AppSettings,
    hydrated_path: &OsStr,
    http_client: &reqwest::Client,
) -> Result<ProcessorTestResult> {
    let started = Instant::now();
    let ping_prompt = "Respond with strictly valid JSON matching this schema and nothing else: {\"ok\": true, \"pong\": \"planar\"}";
    let output = match run_llm_prompt(ping_prompt, settings, hydrated_path, http_client).await {
        Ok(res) => res,
        Err(err) => {
            return Ok(ProcessorTestResult {
                ok: false,
                backend_used: planned_backend_label(settings),
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                message: truncate_chars(&err.to_string(), 220),
            });
        }
    };
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let reply = match output.reply {
        Ok(text) => text,
        Err(err) => {
            return Ok(ProcessorTestResult {
                ok: false,
                backend_used: output.backend_used,
                latency_ms,
                message: truncate_chars(&err.to_string(), 220),
            });
        }
    };

    let ok = extract_json_object(&reply)
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .is_some_and(|v| v.get("ok").and_then(Value::as_bool).unwrap_or(true));

    let preview = truncate_chars(&normalize_step_text(&reply), 120);
    Ok(ProcessorTestResult {
        ok,
        backend_used: output.backend_used,
        latency_ms,
        message: if ok {
            format!("Connected ({preview})")
        } else {
            format!("Unexpected reply: {preview}")
        },
    })
}

#[derive(Debug)]
struct LlmPromptOutput {
    reply: Result<String>,
    backend_used: String,
    usage: SyncUsageDto,
}

struct PreparedSession {
    cand: SyncCandidate,
    sliced: SlicedTrajectory,
    approx_chars: usize,
}

struct BatchLlmResult {
    batch_index: usize,
    batch_len: usize,
    digests: Vec<(SyncCandidate, GeneratedDigest)>,
    failed: usize,
    backend_used: Option<String>,
    usage: Option<SyncUsageDto>,
    last_error: Option<String>,
    fatal: bool,
}

fn merge_opt_usage(acc: &mut Option<SyncUsageDto>, incoming: Option<&SyncUsageDto>) {
    let Some(other) = incoming else {
        return;
    };
    match acc {
        Some(existing) => existing.merge_from(other),
        None => *acc = Some(other.clone()),
    }
}

fn is_fatal_processor_error(err: &PlanarError) -> bool {
    match err {
        PlanarError::InvalidInput(_) => true,
        PlanarError::External(msg) => msg.contains("API HTTP 401") || msg.contains("API HTTP 403"),
        _ => false,
    }
}

fn candidate_preview_label(cand: &SyncCandidate) -> String {
    let clean = normalize_step_text(strip_delegation_envelope(&cand.first_user_prompt));
    if clean.is_empty() {
        cand.id.clone()
    } else {
        truncate_chars(&clean, 64)
    }
}

fn estimate_sliced_chars(sliced: &SlicedTrajectory) -> usize {
    let sum_steps = |steps: &[SlicedStep]| -> usize {
        steps.iter().map(|s| s.text.len().saturating_add(16)).sum()
    };
    sum_steps(&sliced.head)
        .saturating_add(sum_steps(&sliced.middle_users))
        .saturating_add(sum_steps(&sliced.tail))
        .saturating_add(240)
}

fn normalize_cwd_key(cwd: Option<&str>) -> String {
    let Some(raw) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let trimmed = raw.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        raw.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Groups prepared candidates by normalized `cwd` affinity while preserving first-seen workspace
/// priority and relative recency within each workspace, so same-project sessions pack into the
/// same batch while distinct projects run concurrently with minimal cross-batch Line fragmentation.
fn order_prepared_by_cwd_affinity(items: Vec<PreparedSession>) -> Vec<PreparedSession> {
    if items.len() <= 1 {
        return items;
    }
    let mut group_idx: HashMap<String, usize> = HashMap::new();
    let mut groups: Vec<Vec<PreparedSession>> = Vec::new();
    for item in items {
        let key = normalize_cwd_key(item.cand.cwd.as_deref());
        if let Some(&idx) = group_idx.get(&key) {
            groups[idx].push(item);
        } else {
            let idx = groups.len();
            group_idx.insert(key, idx);
            groups.push(vec![item]);
        }
    }
    groups.into_iter().flatten().collect()
}

fn chunk_prepared_into_batches(
    items: Vec<PreparedSession>,
    batch_limit: usize,
) -> Vec<Vec<PreparedSession>> {
    let cap = batch_limit.clamp(SYNC_BATCH_LIMIT_MIN, SYNC_BATCH_LIMIT_MAX);
    let mut batches: Vec<Vec<PreparedSession>> = Vec::new();
    let mut current: Vec<PreparedSession> = Vec::new();
    let mut current_chars = 0_usize;

    for item in items {
        let would_exceed_chars = !current.is_empty()
            && current_chars.saturating_add(item.approx_chars) > MAX_BATCH_PROMPT_CHARS;
        if current.len() >= cap || would_exceed_chars {
            batches.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current_chars = current_chars.saturating_add(item.approx_chars);
        current.push(item);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

async fn apply_digest_to_store(
    state: &AppState,
    cand: &SyncCandidate,
    slice_hash: &str,
    digest: &GeneratedDigest,
) -> Result<()> {
    let sid = cand.id.clone();
    let step_count = cand.step_count;
    let hash_clone = slice_hash.to_string();
    let title = digest.title.clone();
    let stopped_at = digest.stopped_at.clone();
    let left_open = digest.left_open.clone();
    let suggested_line = digest.suggested_line.clone();
    let is_one_off = digest.is_one_off;
    state
        .with_store(move |store| {
            store.apply_session_digest(
                &sid,
                step_count,
                &hash_clone,
                &title,
                &stopped_at,
                &left_open,
                &suggested_line,
                is_one_off,
            )
        })
        .await
}

async fn commit_batch_digests(
    state: &AppState,
    digests: Vec<(SyncCandidate, GeneratedDigest)>,
    slice_hash: &str,
) -> (Vec<SyncFiledPreview>, usize, Option<String>) {
    if digests.is_empty() {
        return (Vec::new(), 0, None);
    }
    let count = digests.len();
    let mut writes = Vec::with_capacity(count);
    let mut previews = Vec::with_capacity(count);
    for (cand, digest) in digests {
        previews.push(SyncFiledPreview {
            session_id: cand.id.clone(),
            title: digest.title.clone(),
            line_name: digest.suggested_line.clone(),
        });
        writes.push(SessionDigestWrite {
            session_id: cand.id,
            step_count: cand.step_count,
            slice_hash: slice_hash.to_string(),
            title: digest.title,
            stopped_at: digest.stopped_at,
            left_open: digest.left_open,
            suggested_line: digest.suggested_line,
            is_one_off: digest.is_one_off,
        });
    }
    match state
        .with_store(move |store| store.apply_session_digests_batch(&writes))
        .await
    {
        Ok(()) => (previews, 0, None),
        Err(err) => (Vec::new(), count, Some(err.to_string())),
    }
}

/// Fans out `slice_trajectory_file` across a bounded multi-core Tokio blocking pool while
/// preserving the exact input candidate order.
async fn slice_candidates_parallel(
    to_slice: Vec<SyncCandidate>,
    settings: &AppSettings,
) -> Result<Vec<(SyncCandidate, Result<SlicedTrajectory>)>> {
    if to_slice.is_empty() {
        return Ok(Vec::new());
    }

    let max_workers = std::thread::available_parallelism()
        .map_or(4, NonZeroUsize::get)
        .min(to_slice.len())
        .max(1);
    let sem = Arc::new(Semaphore::new(max_workers));
    let shared_settings = Arc::new(settings.clone());
    let total = to_slice.len();
    let mut join_set: JoinSet<(usize, SyncCandidate, Result<SlicedTrajectory>)> = JoinSet::new();

    for (idx, cand) in to_slice.into_iter().enumerate() {
        let permit = sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|err| PlanarError::TaskJoin(err.to_string()))?;
        let cfg = shared_settings.clone();
        join_set.spawn_blocking(move || {
            let _permit = permit;
            let res = slice_trajectory_file(Path::new(&cand.source_path), &cand.harness, &cfg);
            (idx, cand, res)
        });
    }

    let mut ordered: Vec<Option<(SyncCandidate, Result<SlicedTrajectory>)>> =
        (0..total).map(|_| None).collect();
    while let Some(joined) = join_set.join_next().await {
        let (idx, cand, res) = joined.map_err(|err| PlanarError::TaskJoin(err.to_string()))?;
        if let Some(slot) = ordered.get_mut(idx) {
            *slot = Some((cand, res));
        }
    }

    Ok(ordered.into_iter().flatten().collect())
}

/// Prepares candidates locally (handling pruned logs and reconciling post-filter short sessions)
/// and returns `(pruned_filed, skipped_short, slice_failures, last_err, prepared_for_llm)`.
async fn prepare_candidates_for_batching(
    state: &AppState,
    candidates: Vec<SyncCandidate>,
    settings: &AppSettings,
    slice_hash: &str,
) -> (
    Vec<SyncFiledPreview>,
    usize,
    usize,
    Option<String>,
    Vec<PreparedSession>,
) {
    let min_inputs = usize::try_from(settings.fewest_user_inputs.max(FEWEST_USER_INPUTS_MIN))
        .unwrap_or(usize::MAX);
    let mut pruned_filed = Vec::new();
    let mut skipped_short = 0_usize;
    let mut failed = 0_usize;
    let mut last_err = None;
    let mut to_slice = Vec::new();

    for cand in candidates {
        if Path::new(&cand.source_path).exists() {
            to_slice.push(cand);
        } else {
            let fallback = build_pruned_fallback_digest(&cand, settings);
            match apply_digest_to_store(state, &cand, slice_hash, &fallback).await {
                Ok(()) => {
                    pruned_filed.push(SyncFiledPreview {
                        session_id: cand.id,
                        title: fallback.title,
                        line_name: fallback.suggested_line,
                    });
                }
                Err(err) => {
                    failed += 1;
                    last_err = Some(err.to_string());
                }
            }
        }
    }

    if to_slice.is_empty() {
        return (pruned_filed, skipped_short, failed, last_err, Vec::new());
    }

    let Ok(pairs) = slice_candidates_parallel(to_slice, settings).await else {
        return (
            pruned_filed,
            skipped_short,
            failed + 1,
            Some("background trajectory slicing task failed".to_string()),
            Vec::new(),
        );
    };

    let mut prepared = Vec::new();
    let mut reconcile_list: Vec<(String, usize)> = Vec::new();
    for (cand, res) in pairs {
        match res {
            Ok(sliced) => {
                if sliced.user_count < min_inputs && cand.line_id.is_none() {
                    reconcile_list.push((cand.id, sliced.user_count));
                    skipped_short += 1;
                } else {
                    let approx_chars = estimate_sliced_chars(&sliced);
                    prepared.push(PreparedSession {
                        cand,
                        sliced,
                        approx_chars,
                    });
                }
            }
            Err(err) => {
                failed += 1;
                last_err = Some(err.to_string());
            }
        }
    }

    if !reconcile_list.is_empty() {
        let _ = state
            .with_store(move |store| {
                for (sid, cnt) in reconcile_list {
                    store.reconcile_user_input_count(&sid, cnt)?;
                }
                Ok(())
            })
            .await;
    }

    (pruned_filed, skipped_short, failed, last_err, prepared)
}

fn augment_catalog_with_batch_digests(
    base: &[LineCatalogEntry],
    digests: &[(SyncCandidate, GeneratedDigest)],
) -> Vec<LineCatalogEntry> {
    let mut out = base.to_vec();
    let mut seen: HashSet<String> = out.iter().map(|e| e.name.to_ascii_lowercase()).collect();
    for (_, digest) in digests {
        if digest.is_one_off {
            continue;
        }
        let trimmed = digest.suggested_line.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("one-off") {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if seen.insert(lower) {
            out.push(LineCatalogEntry {
                id: String::new(),
                name: trimmed.to_string(),
                description: String::new(),
                pinned: false,
                session_count: 1,
            });
        }
    }
    out
}

fn spawn_in_join_set<T, F>(set: &mut JoinSet<T>, fut: F)
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    #[cfg(test)]
    {
        let billing = TEST_BILLING_PROVIDER_OVERRIDE
            .try_with(|v| *v)
            .ok()
            .flatten();
        let retry_ms = TEST_RETRY_BASE_DELAY_MS_OVERRIDE
            .try_with(|v| *v)
            .ok()
            .flatten();
        set.spawn(TEST_BILLING_PROVIDER_OVERRIDE.scope(
            billing,
            TEST_RETRY_BASE_DELAY_MS_OVERRIDE.scope(retry_ms, fut),
        ));
    }
    #[cfg(not(test))]
    {
        set.spawn(fut);
    }
}

/// Executes a single multi-session LLM call for `batch` (plus parallel omitted-item fallbacks)
/// bounded by `llm_sem`, returning parsed digests ready for coordinator SQLite commit.
async fn execute_prepared_batch_llm(
    state: &AppState,
    batch_index: usize,
    batch: Vec<PreparedSession>,
    settings: &AppSettings,
    llm_sem: Arc<Semaphore>,
) -> BatchLlmResult {
    let batch_len = batch.len();
    if !settings.is_processor_configured() {
        return BatchLlmResult {
            batch_index,
            batch_len,
            digests: Vec::new(),
            failed: batch_len,
            backend_used: None,
            usage: None,
            last_error: Some(unconfigured_processor_message(settings)),
            fatal: true,
        };
    }

    let catalog = match state.with_store(|store| store.list_line_catalog()).await {
        Ok(c) => c,
        Err(err) => {
            return BatchLlmResult {
                batch_index,
                batch_len,
                digests: Vec::new(),
                failed: batch_len,
                backend_used: None,
                usage: None,
                last_error: Some(err.to_string()),
                fatal: false,
            };
        }
    };

    let prompt = if batch.len() == 1 {
        build_digest_prompt(&batch[0].cand, &batch[0].sliced, &catalog, settings)
    } else {
        build_batch_digest_prompt(&batch, &catalog, settings)
    };

    let main_out = {
        let Ok(_permit) = llm_sem.acquire().await else {
            return BatchLlmResult {
                batch_index,
                batch_len,
                digests: Vec::new(),
                failed: batch_len,
                backend_used: None,
                usage: None,
                last_error: Some("sync concurrency semaphore closed".into()),
                fatal: false,
            };
        };
        run_llm_prompt(&prompt, settings, &state.hydrated_path, &state.http_client).await
    };

    let main_out = match main_out {
        Ok(res) => res,
        Err(err) => {
            let fatal = is_fatal_processor_error(&err);
            return BatchLlmResult {
                batch_index,
                batch_len,
                digests: Vec::new(),
                failed: batch_len,
                backend_used: None,
                usage: None,
                last_error: Some(err.to_string()),
                fatal,
            };
        }
    };

    let backend_used = main_out.backend_used;
    let mut batch_usage = main_out.usage;

    let main_reply = match main_out.reply {
        Ok(text) => text,
        Err(err) => {
            let fatal = is_fatal_processor_error(&err);
            return BatchLlmResult {
                batch_index,
                batch_len,
                digests: Vec::new(),
                failed: batch_len,
                backend_used: Some(backend_used),
                usage: Some(batch_usage),
                last_error: Some(err.to_string()),
                fatal,
            };
        }
    };

    let expected_ids: Vec<String> = batch.iter().map(|p| p.cand.id.clone()).collect();
    let mut parsed_map = match parse_batch_digest_json(&main_reply, &expected_ids) {
        Ok(m) => m,
        Err(err) => {
            return BatchLlmResult {
                batch_index,
                batch_len,
                digests: Vec::new(),
                failed: batch_len,
                backend_used: Some(backend_used),
                usage: Some(batch_usage),
                last_error: Some(err.to_string()),
                fatal: false,
            };
        }
    };

    let mut digests = Vec::with_capacity(batch_len);
    let mut failed = 0_usize;
    let mut last_error = None;
    let mut omitted: Vec<PreparedSession> = Vec::new();

    for item in batch {
        if let Some(digest) = parsed_map.remove(&item.cand.id) {
            digests.push((item.cand, digest));
        } else {
            omitted.push(item);
        }
    }

    // If the batch call omitted any session ID, fall back concurrently per omitted item so no work is dropped.
    if !omitted.is_empty() {
        let augmented_catalog = augment_catalog_with_batch_digests(&catalog, &digests);
        let mut solo_set: JoinSet<(SyncCandidate, Option<SyncUsageDto>, Result<GeneratedDigest>)> =
            JoinSet::new();

        for item in omitted {
            let solo_prompt =
                build_digest_prompt(&item.cand, &item.sliced, &augmented_catalog, settings);
            let cfg = settings.clone();
            let hydrated = state.hydrated_path.clone();
            let client = state.http_client.clone();
            let sem = llm_sem.clone();
            let cand = item.cand;
            spawn_in_join_set(&mut solo_set, async move {
                let Ok(_permit) = sem.acquire_owned().await else {
                    return (
                        cand,
                        None,
                        Err(PlanarError::External(
                            "sync concurrency semaphore closed".into(),
                        )),
                    );
                };
                match run_llm_prompt(&solo_prompt, &cfg, &hydrated, &client).await {
                    Ok(solo_out) => {
                        let parsed = solo_out.reply.and_then(|reply| parse_digest_json(&reply));
                        (cand, Some(solo_out.usage), parsed)
                    }
                    Err(err) => (cand, None, Err(err)),
                }
            });
        }

        while let Some(joined) = solo_set.join_next().await {
            match joined {
                Ok((cand, usage_opt, Ok(digest))) => {
                    if let Some(u) = usage_opt {
                        batch_usage.merge_from(&u);
                    }
                    digests.push((cand, digest));
                }
                Ok((_, usage_opt, Err(err))) => {
                    if let Some(u) = usage_opt {
                        batch_usage.merge_from(&u);
                    }
                    failed += 1;
                    last_error = Some(err.to_string());
                }
                Err(join_err) => {
                    failed += 1;
                    last_error = Some(join_err.to_string());
                }
            }
        }
    }

    BatchLlmResult {
        batch_index,
        batch_len,
        digests,
        failed,
        backend_used: Some(backend_used),
        usage: Some(batch_usage),
        last_error,
        fatal: false,
    }
}

/// Runs incremental trajectory slicing and summarization for one batch of eligible sessions.
pub async fn sync_pending_sessions(state: &AppState, force_all: bool) -> Result<SyncSummaryDto> {
    sync_pending_sessions_in_scope(state, force_all, None, None).await
}

/// Runs incremental trajectory slicing within an optional view `SyncScope` and candidate limit.
pub async fn sync_pending_sessions_in_scope(
    state: &AppState,
    force_all: bool,
    limit_override: Option<usize>,
    scope: Option<SyncScope>,
) -> Result<SyncSummaryDto> {
    let (settings, slice_hash, candidates, catalog_is_empty) = state
        .with_store(move |store| {
            let settings = store.get_settings()?;
            let slice_hash = compute_slice_config_hash(&settings);
            let effective_limit = limit_override.or_else(|| {
                Some(
                    settings
                        .sync_batch_limit
                        .clamp(SYNC_BATCH_LIMIT_MIN, SYNC_BATCH_LIMIT_MAX),
                )
            });
            let candidates = store.load_sync_candidates_in_scope(
                &settings,
                &slice_hash,
                force_all,
                effective_limit,
                scope.as_ref(),
                Local::now(),
            )?;
            let catalog_is_empty = store.list_line_catalog()?.is_empty();
            Ok((settings, slice_hash, candidates, catalog_is_empty))
        })
        .await?;

    let default_backend = planned_backend_label(&settings);
    let mut summary = SyncSummaryDto {
        processed: 0,
        failed: 0,
        skipped: 0,
        backend_used: default_backend,
        last_error: None,
        usage: None,
    };

    if candidates.is_empty() {
        return Ok(summary);
    }

    let (pruned_filed, skipped_short, slice_failed, slice_err, prepared) =
        prepare_candidates_for_batching(state, candidates, &settings, &slice_hash).await;
    summary.processed += pruned_filed.len();
    summary.skipped += skipped_short;
    summary.failed += slice_failed;
    if slice_err.is_some() {
        summary.last_error = slice_err;
    }

    let prepared = order_prepared_by_cwd_affinity(prepared);
    let batches = chunk_prepared_into_batches(prepared, settings.sync_batch_limit);
    let total_batches = batches.len();
    if total_batches == 0 {
        return Ok(summary);
    }

    let total_batch_items: usize = batches.iter().map(Vec::len).sum();
    let concurrency = effective_sync_concurrency(&settings);
    let mut warming_up = catalog_is_empty && total_batches > 1 && concurrency > 1;
    let llm_sem = Arc::new(Semaphore::new(concurrency));
    let mut join_set: JoinSet<BatchLlmResult> = JoinSet::new();
    let mut queue = batches.into_iter().enumerate().peekable();
    let mut completed_items = 0_usize;

    loop {
        let active_limit = if warming_up { 1 } else { concurrency };
        while join_set.len() < active_limit && queue.peek().is_some() {
            let Some((idx, batch)) = queue.next() else {
                break;
            };
            let st = state.clone();
            let cfg = settings.clone();
            let lsem = llm_sem.clone();
            spawn_in_join_set(&mut join_set, async move {
                execute_prepared_batch_llm(&st, idx + 1, batch, &cfg, lsem).await
            });
        }

        let Some(joined) = join_set.join_next().await else {
            break;
        };

        let batch_res = match joined {
            Ok(res) => res,
            Err(join_err) => BatchLlmResult {
                batch_index: 0,
                batch_len: 0,
                digests: Vec::new(),
                failed: 0,
                backend_used: None,
                usage: None,
                last_error: Some(join_err.to_string()),
                fatal: false,
            },
        };

        let (filed, commit_failed, commit_err) =
            commit_batch_digests(state, batch_res.digests, &slice_hash).await;
        if let Some(backend) = batch_res.backend_used {
            summary.backend_used = backend;
        }
        merge_opt_usage(&mut summary.usage, batch_res.usage.as_ref());
        summary.processed += filed.len();
        summary.failed += batch_res.failed.saturating_add(commit_failed);
        let batch_err = commit_err.or(batch_res.last_error);
        if batch_err.is_some() {
            summary.last_error = batch_err;
        }
        completed_items = completed_items.saturating_add(batch_res.batch_len);

        if warming_up {
            warming_up = false;
        }
        if batch_res.fatal {
            join_set.abort_all();
            break;
        }
    }

    let remaining = total_batch_items.saturating_sub(completed_items);
    if remaining > 0 {
        summary.skipped = summary.skipped.saturating_add(remaining);
    }

    Ok(summary)
}

/// Estimates pending `Sync` token usage and API USD cost strictly read-only without calling any
/// LLM, creating a missing database, or mutating SQLite.
pub async fn estimate_sync_cost(
    state: &AppState,
    force_all: bool,
    limit_override: Option<usize>,
    scope: Option<SyncScope>,
) -> Result<SyncEstimateDto> {
    let loaded = state
        .with_store_readonly(move |store| {
            let settings = store.get_settings()?;
            let slice_hash = compute_slice_config_hash(&settings);
            let catalog = store.list_line_catalog()?;
            let candidates = store.load_sync_candidates_in_scope(
                &settings,
                &slice_hash,
                force_all,
                limit_override,
                scope.as_ref(),
                Local::now(),
            )?;
            Ok((settings, catalog, candidates))
        })
        .await?;

    let Some((settings, catalog, candidates)) = loaded else {
        return Ok(estimate_prepared_batches_cost(
            &[],
            &[],
            &AppSettings::default(),
        ));
    };

    let existing: Vec<SyncCandidate> = candidates
        .into_iter()
        .filter(|cand| Path::new(&cand.source_path).exists())
        .collect();

    if existing.is_empty() {
        return Ok(estimate_prepared_batches_cost(&[], &catalog, &settings));
    }

    let min_inputs = usize::try_from(settings.fewest_user_inputs.max(FEWEST_USER_INPUTS_MIN))
        .unwrap_or(usize::MAX);
    let pairs = slice_candidates_parallel(existing, &settings).await?;
    let mut prepared = Vec::new();
    for (cand, res) in pairs {
        let Ok(sliced) = res else {
            continue;
        };
        if sliced.user_count < min_inputs && cand.line_id.is_none() {
            continue;
        }
        let approx_chars = estimate_sliced_chars(&sliced);
        prepared.push(PreparedSession {
            cand,
            sliced,
            approx_chars,
        });
    }

    let prepared = order_prepared_by_cwd_affinity(prepared);
    let batches = chunk_prepared_into_batches(prepared, settings.sync_batch_limit);
    Ok(estimate_prepared_batches_cost(
        &batches, &catalog, &settings,
    ))
}

/// Spawns a multi-session concurrent batch streaming `Sync` coordinator scoped to `scope`,
/// emitting NDJSON events as batches start and complete while keeping the connection alive
/// with heartbeats. Registers a drain handle on `state` for `POST /api/sync/stop`.
pub fn spawn_sync_stream(
    state: AppState,
    scope: SyncScope,
    force_all: bool,
    limit_override: Option<usize>,
) -> tokio::sync::mpsc::Receiver<SyncStreamEvent> {
    let drain_flag = state
        .prepare_sync_drain(None)
        .unwrap_or_else(|_| Arc::new(AtomicBool::new(false)));
    spawn_sync_stream_with_drain(state, scope, force_all, limit_override, drain_flag)
}

/// Spawns a streaming `Sync` coordinator controlled by an explicit `drain_flag`: setting
/// `drain_flag` to `true` stops scheduling new batches while letting in-flight batches drain
/// cleanly; dropping the returned receiver immediately aborts all in-flight tasks.
pub fn spawn_sync_stream_with_drain(
    state: AppState,
    scope: SyncScope,
    force_all: bool,
    limit_override: Option<usize>,
    drain_flag: Arc<AtomicBool>,
) -> tokio::sync::mpsc::Receiver<SyncStreamEvent> {
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    tokio::spawn(async move {
        run_sync_stream_coordinator(state, scope, force_all, limit_override, drain_flag, tx).await;
    });
    rx
}

async fn run_sync_stream_coordinator(
    state: AppState,
    scope: SyncScope,
    force_all: bool,
    limit_override: Option<usize>,
    drain_flag: Arc<AtomicBool>,
    tx: tokio::sync::mpsc::Sender<SyncStreamEvent>,
) {
    let days = scope.days.clamp(WINDOW_DAYS_MIN, WINDOW_DAYS_MAX);
    let anchor = scope.anchor_date;
    let scope_for_query = scope;

    let init_res = state
        .with_store(move |store| {
            let settings = store.get_settings()?;
            let slice_hash = compute_slice_config_hash(&settings);
            let candidates = store.load_sync_candidates_in_scope(
                &settings,
                &slice_hash,
                force_all,
                limit_override,
                Some(&scope_for_query),
                Local::now(),
            )?;
            let catalog_is_empty = store.list_line_catalog()?.is_empty();
            Ok((settings, slice_hash, candidates, catalog_is_empty))
        })
        .await;

    let (settings, slice_hash, candidates, catalog_is_empty) = match init_res {
        Ok(tuple) => tuple,
        Err(err) => {
            let fallback_summary = SyncSummaryDto {
                processed: 0,
                failed: 1,
                skipped: 0,
                backend_used: "error".to_string(),
                last_error: Some(err.to_string()),
                usage: None,
            };
            state.record_last_sync(fallback_summary.clone());
            if let Ok(plane) = state.build_plane_snapshot(days, anchor).await {
                let _ = tx
                    .send(SyncStreamEvent::Complete {
                        summary: fallback_summary,
                        plane: Box::new(plane),
                    })
                    .await;
            }
            return;
        }
    };

    let default_backend = planned_backend_label(&settings);
    let batch_size = settings
        .sync_batch_limit
        .clamp(SYNC_BATCH_LIMIT_MIN, SYNC_BATCH_LIMIT_MAX);
    let concurrency = effective_sync_concurrency(&settings);
    let mut summary = SyncSummaryDto {
        processed: 0,
        failed: 0,
        skipped: 0,
        backend_used: default_backend.clone(),
        last_error: None,
        usage: None,
    };

    // Exit early when nothing is pending or Stop was requested before slicing began (e.g. during `run_scan`).
    if candidates.is_empty() || drain_flag.load(Ordering::Acquire) {
        let total = candidates.len();
        summary.skipped = total;
        let _ = tx
            .send(SyncStreamEvent::Start {
                total,
                total_batches: 0,
                batch_size,
                concurrency,
                backend: default_backend,
            })
            .await;
        state.record_last_sync(summary.clone());
        if let Ok(plane) = state.build_plane_snapshot(days, anchor).await {
            let _ = tx
                .send(SyncStreamEvent::Complete {
                    summary,
                    plane: Box::new(plane),
                })
                .await;
        }
        return;
    }

    let (pruned_filed, skipped_short, slice_failed, slice_err, prepared) =
        prepare_candidates_for_batching(&state, candidates, &settings, &slice_hash).await;
    summary.processed += pruned_filed.len();
    summary.skipped += skipped_short;
    summary.failed += slice_failed;
    if slice_err.is_some() {
        summary.last_error = slice_err;
    }

    let prepared = order_prepared_by_cwd_affinity(prepared);
    let batches = chunk_prepared_into_batches(prepared, batch_size);
    let total_batches = batches.len();
    let total_items: usize = pruned_filed
        .len()
        .saturating_add(slice_failed)
        .saturating_add(batches.iter().map(Vec::len).sum());

    if tx
        .send(SyncStreamEvent::Start {
            total: total_items,
            total_batches,
            batch_size,
            concurrency,
            backend: default_backend,
        })
        .await
        .is_err()
    {
        return;
    }

    let mut completed = pruned_filed.len().saturating_add(slice_failed);
    if !pruned_filed.is_empty() {
        state.record_last_sync(summary.clone());
        let ev = SyncStreamEvent::BatchDone(Box::new(SyncBatchDoneDto {
            batch_index: 0,
            total_batches,
            batch_processed: pruned_filed.len(),
            batch_failed: slice_failed,
            filed: pruned_filed,
            error: summary.last_error.clone(),
            processed: summary.processed,
            failed: summary.failed,
            skipped: summary.skipped,
            completed,
            total: total_items,
            backend_used: summary.backend_used.clone(),
            batch_usage: None,
            cumulative_usage: summary.usage.clone(),
            plane: None,
        }));
        if tx.send(ev).await.is_err() {
            return;
        }
    }

    let mut heartbeat = tokio::time::interval(Duration::from_secs(STREAM_HEARTBEAT_SECS));
    heartbeat.tick().await;

    let mut warming_up = catalog_is_empty && total_batches > 1 && concurrency > 1;
    let llm_sem = Arc::new(Semaphore::new(concurrency));
    let mut join_set: JoinSet<BatchLlmResult> = JoinSet::new();
    let mut queue = batches.into_iter().enumerate().peekable();
    let mut fatal_stopped = false;

    loop {
        let active_limit = if warming_up { 1 } else { concurrency };
        while !fatal_stopped
            && !drain_flag.load(Ordering::Acquire)
            && join_set.len() < active_limit
            && queue.peek().is_some()
        {
            let Some((idx, batch)) = queue.next() else {
                break;
            };
            let batch_index = idx + 1;
            let cur_batch_len = batch.len();
            let preview_items: Vec<SyncBatchPreviewItem> = batch
                .iter()
                .map(|p| SyncBatchPreviewItem {
                    session_id: p.cand.id.clone(),
                    harness: p.cand.harness.clone(),
                    label: candidate_preview_label(&p.cand),
                })
                .collect();

            if tx
                .send(SyncStreamEvent::BatchStart {
                    batch_index,
                    total_batches,
                    batch_size: cur_batch_len,
                    items: preview_items,
                })
                .await
                .is_err()
            {
                join_set.abort_all();
                return;
            }

            let st = state.clone();
            let cfg = settings.clone();
            let lsem = llm_sem.clone();
            spawn_in_join_set(&mut join_set, async move {
                execute_prepared_batch_llm(&st, batch_index, batch, &cfg, lsem).await
            });
        }

        if join_set.is_empty() {
            break;
        }

        let finished = tokio::select! {
            () = tx.closed() => {
                join_set.abort_all();
                return;
            }
            _ = heartbeat.tick() => {
                if tx.send(SyncStreamEvent::Heartbeat).await.is_err() {
                    join_set.abort_all();
                    return;
                }
                continue;
            }
            Some(joined) = join_set.join_next() => joined,
        };

        let batch_res = match finished {
            Ok(res) => res,
            Err(join_err) => BatchLlmResult {
                batch_index: 0,
                batch_len: 0,
                digests: Vec::new(),
                failed: 0,
                backend_used: None,
                usage: None,
                last_error: Some(join_err.to_string()),
                fatal: false,
            },
        };

        let (filed, commit_failed, commit_err) =
            commit_batch_digests(&state, batch_res.digests, &slice_hash).await;
        let batch_processed = filed.len();
        let batch_failed = batch_res.failed.saturating_add(commit_failed);
        let batch_error = commit_err.or(batch_res.last_error);
        let batch_usage = batch_res.usage;
        let is_fatal = batch_res.fatal;

        if let Some(backend) = batch_res.backend_used {
            summary.backend_used = backend;
        }
        merge_opt_usage(&mut summary.usage, batch_usage.as_ref());
        summary.processed += batch_processed;
        summary.failed += batch_failed;
        if batch_error.is_some() {
            summary.last_error.clone_from(&batch_error);
        }
        completed = completed.saturating_add(batch_res.batch_len);

        if warming_up {
            warming_up = false;
        }
        if is_fatal {
            fatal_stopped = true;
            join_set.abort_all();
            let remaining_items = total_items.saturating_sub(completed);
            summary.skipped = summary.skipped.saturating_add(remaining_items);
        }

        state.record_last_sync(summary.clone());
        let done_ev = SyncStreamEvent::BatchDone(Box::new(SyncBatchDoneDto {
            batch_index: batch_res.batch_index,
            total_batches,
            batch_processed,
            batch_failed,
            filed,
            error: batch_error,
            processed: summary.processed,
            failed: summary.failed,
            skipped: summary.skipped,
            completed,
            total: total_items,
            backend_used: summary.backend_used.clone(),
            batch_usage,
            cumulative_usage: summary.usage.clone(),
            plane: None,
        }));
        if tx.send(done_ev).await.is_err() {
            join_set.abort_all();
            return;
        }

        if fatal_stopped {
            break;
        }
    }

    if !fatal_stopped {
        let remaining_items = total_items.saturating_sub(completed);
        if remaining_items > 0 {
            summary.skipped = summary.skipped.saturating_add(remaining_items);
        }
    }

    state.record_last_sync(summary.clone());
    if let Ok(plane) = state.build_plane_snapshot(days, anchor).await {
        let _ = tx
            .send(SyncStreamEvent::Complete {
                summary,
                plane: Box::new(plane),
            })
            .await;
    }
}

/// Re-clusters sessions belonging to unpinned Lines (`pinned = 0`) using the active processor.
pub async fn regroup_discovered_lines(state: &AppState) -> Result<SyncSummaryDto> {
    let (settings, catalog, items) = state
        .with_store(|store| {
            let settings = store.get_settings()?;
            let catalog = store.list_line_catalog()?;
            let items = store.list_unpinned_items_for_regroup()?;
            Ok((settings, catalog, items))
        })
        .await?;

    let default_backend = planned_backend_label(&settings);
    if items.is_empty() {
        return Ok(SyncSummaryDto {
            processed: 0,
            failed: 0,
            skipped: 0,
            backend_used: default_backend,
            last_error: None,
            usage: None,
        });
    }

    if !settings.is_processor_configured() {
        return Err(PlanarError::InvalidInput(unconfigured_processor_message(
            &settings,
        )));
    }

    let prompt = build_regroup_prompt(&items, &catalog, &settings);
    let out = run_llm_prompt(&prompt, &settings, &state.hydrated_path, &state.http_client).await?;
    let reply = out.reply?;
    let assignments = parse_regroup_json(&reply)?;
    let moved = state
        .with_store(move |store| store.apply_regroup_mapping(&assignments))
        .await?;

    Ok(SyncSummaryDto {
        processed: moved,
        failed: 0,
        skipped: items.len().saturating_sub(moved),
        backend_used: out.backend_used,
        last_error: None,
        usage: Some(out.usage),
    })
}

/// Estimates token count for arbitrary text using an ASCII/CJK-aware offline heuristic
/// (`~4.0 chars/tok` ASCII/Latin, `~1.6 chars/tok` CJK/East Asian).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn estimate_text_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    let mut cjk_chars = 0_u64;
    let mut other_chars = 0_u64;
    for ch in text.chars() {
        if is_cjk_char(ch) {
            cjk_chars += 1;
        } else {
            other_chars += 1;
        }
    }
    let est = (other_chars as f64) / 4.0 + (cjk_chars as f64) / 1.6;
    est.ceil().max(1.0) as u64
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3000..=0x30FF // CJK punctuation, Hiragana, Katakana
            | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
            | 0x4E00..=0x9FFF // CJK Unified Ideographs
            | 0xAC00..=0xD7AF // Hangul Syllables
            | 0xF900..=0xFAFF // CJK Compatibility Ideographs
            | 0xFF00..=0xFFEF // Halfwidth and Fullwidth Forms
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThinkingTier {
    None,
    Light,
    High,
}

fn thinking_tier_for_model(raw_model: &str) -> ThinkingTier {
    let lower = raw_model.trim().to_ascii_lowercase();
    let m = lower.rsplit('/').next().unwrap_or(lower.as_str());
    if m.is_empty()
        || m.contains("flash-lite")
        || m.contains("nano")
        || m.starts_with("gpt-4o")
        || m.starts_with("gpt-4.1")
        || m.contains("haiku")
        || m == "deepseek-chat"
        || m.contains("non-reasoning")
    {
        return ThinkingTier::None;
    }
    if m.starts_with("gemini-4-")
        || m.starts_with("gemini-3-pro")
        || m.starts_with("gemini-3.1-pro")
        || m.starts_with("gemini-2.5-pro")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
        || m.starts_with("gpt-5-pro")
        || m.starts_with("gpt-6-astra")
        || m.starts_with("gpt-6-rosalind")
        || m.starts_with("gpt-6-cyber")
        || m.contains("reasoner")
        || m.contains("thinking")
        || m.contains("-reasoning")
    {
        return ThinkingTier::High;
    }
    if (m.starts_with("gemini-") && m.contains("flash") && !m.starts_with("gemini-2.0"))
        || m.starts_with("gpt-5")
        || m.starts_with("gpt-6")
        || m.starts_with("claude-opus-4")
        || m.starts_with("claude-sonnet-4")
        || m.starts_with("deepseek-v4-pro")
    {
        return ThinkingTier::Light;
    }
    ThinkingTier::None
}

#[cfg(test)]
tokio::task_local! {
    static TEST_BILLING_PROVIDER_OVERRIDE: Option<&'static str>;
    static TEST_RETRY_BASE_DELAY_MS_OVERRIDE: Option<u64>;
}

/// Derives billing provider attribution from BOTH the configured provider ID and the resolved
/// HTTP endpoint URL. Only requests targeting a known provider's official HTTPS host inherit that
/// vendor's USD list pricing; overridden local or proxy Base URLs return `None` so tokens are
/// retained without fabricating vendor USD spend.
fn resolve_billing_provider(provider: &str, resolved_base: &str) -> Option<&'static str> {
    #[cfg(test)]
    if let Ok(Some(override_provider)) = TEST_BILLING_PROVIDER_OVERRIDE.try_with(|p| *p) {
        return Some(override_provider);
    }

    let canonical = canonical_billing_provider(provider)?;
    let spec = api_provider(provider)?;
    let target_url = reqwest::Url::parse(resolved_base.trim()).ok()?;
    if target_url.scheme() != "https" {
        return None;
    }
    let target_host = target_url.host_str()?.to_ascii_lowercase();
    let default_url = reqwest::Url::parse(spec.default_base.trim()).ok()?;
    let official_host = default_url.host_str()?.to_ascii_lowercase();
    if target_host == official_host
        || (canonical == "moonshotai" && target_host == "api.moonshot.ai")
    {
        Some(canonical)
    } else {
        None
    }
}

fn active_processor_model(settings: &AppSettings) -> (bool, String, Option<&'static str>) {
    if settings.processor_mode == "api" {
        let provider = settings.api_provider.trim().to_ascii_lowercase();
        let spec = api_provider(&provider);
        let model = if settings.api_model.trim().is_empty() {
            spec.map_or_default(|s| s.default_model.clone())
        } else {
            settings.api_model.trim().to_string()
        };
        let resolved_base = if settings.api_base_url.trim().is_empty() {
            spec.map_or("", |s| s.default_base.as_str())
        } else {
            settings.api_base_url.trim()
        };
        let billing_provider = resolve_billing_provider(&provider, resolved_base);
        (false, model, billing_provider)
    } else {
        (true, settings.cli_model.trim().to_string(), None)
    }
}

fn estimate_prepared_batches_cost(
    batches: &[Vec<PreparedSession>],
    catalog: &[LineCatalogEntry],
    settings: &AppSettings,
) -> SyncEstimateDto {
    let (is_local_cli, model, billing_provider_opt) = active_processor_model(settings);
    let backend = planned_backend_label(settings);
    let session_count: usize = batches.iter().map(Vec::len).sum();
    let batch_count = batches.len();

    if session_count == 0 {
        return SyncEstimateDto {
            session_count: 0,
            batch_count: 0,
            estimated_input_tokens: 0,
            estimated_output_tokens: 0,
            estimated_thinking_tokens: 0,
            estimated_cost_usd_range: None,
            model,
            backend,
            is_local_cli,
        };
    }

    let tier = thinking_tier_for_model(&model);
    let mut est_input = 0_u64;
    let mut out_mid = 0_u64;
    let mut think_mid = 0_u64;
    let mut cost_low_total = 0.0_f64;
    let mut cost_high_total = 0.0_f64;
    let mut all_batches_priced = !is_local_cli && billing_provider_opt.is_some();

    for batch in batches {
        let prompt = if batch.len() == 1 {
            build_digest_prompt(&batch[0].cand, &batch[0].sliced, catalog, settings)
        } else {
            build_batch_digest_prompt(batch, catalog, settings)
        };
        let batch_input = estimate_text_tokens(&prompt);
        est_input = est_input.saturating_add(batch_input);

        let n = u64::try_from(batch.len()).unwrap_or(1);
        let batch_out_low = n.saturating_mul(120).saturating_add(40);
        let batch_out_mid = n.saturating_mul(180).saturating_add(60);
        let batch_out_high = n.saturating_mul(260).saturating_add(90);
        out_mid = out_mid.saturating_add(batch_out_mid);

        let (batch_think_low, batch_think_mid, batch_think_high) = match tier {
            ThinkingTier::High => (
                n.saturating_mul(200).saturating_add(150),
                n.saturating_mul(400).saturating_add(250),
                n.saturating_mul(650).saturating_add(400),
            ),
            ThinkingTier::Light => (
                n.saturating_mul(80).saturating_add(60),
                n.saturating_mul(180).saturating_add(120),
                n.saturating_mul(320).saturating_add(220),
            ),
            ThinkingTier::None => (0, 0, 0),
        };
        think_mid = think_mid.saturating_add(batch_think_mid);

        // Price each batch individually so multi-batch runs do not falsely cross
        // single-request long-context pricing tiers (e.g. Gemini 2.5 Pro >200K).
        if all_batches_priced {
            let batch_in_low = batch_input.saturating_mul(9) / 10;
            let batch_in_high = batch_input.saturating_mul(11).div_ceil(10);
            let lo = estimate_api_cost_usd(
                billing_provider_opt,
                Some(&model),
                batch_in_low,
                0,
                0,
                batch_out_low,
                batch_think_low,
            );
            let hi = estimate_api_cost_usd(
                billing_provider_opt,
                Some(&model),
                batch_in_high,
                0,
                0,
                batch_out_high,
                batch_think_high,
            );
            if let (Some(lo), Some(hi)) = (lo, hi) {
                cost_low_total += lo.min(hi);
                cost_high_total += lo.max(hi);
            } else {
                all_batches_priced = false;
            }
        }
    }

    let estimated_cost_usd_range = all_batches_priced.then_some(CostUsdRangeDto {
        low: cost_low_total,
        high: cost_high_total,
    });

    SyncEstimateDto {
        session_count,
        batch_count,
        estimated_input_tokens: est_input,
        estimated_output_tokens: out_mid,
        estimated_thinking_tokens: think_mid,
        estimated_cost_usd_range,
        model,
        backend,
        is_local_cli,
    }
}

fn planned_backend_label(settings: &AppSettings) -> String {
    if settings.processor_mode == "api" {
        format!("api:{}", settings.api_provider)
    } else if settings.cli_harness.trim().is_empty() {
        "cli:unconfigured".to_string()
    } else {
        format!("cli:{}", settings.cli_harness.trim())
    }
}

fn unconfigured_processor_message(settings: &AppSettings) -> String {
    if settings.language == "zh" {
        "尚未配置摘要引擎，请先在「设置 → 摘要引擎」中选择 CLI 或 API。".to_string()
    } else {
        "Summarizer is not configured yet — select a CLI or API engine in Settings → Summarizer."
            .to_string()
    }
}

fn build_pruned_fallback_digest(cand: &SyncCandidate, settings: &AppSettings) -> GeneratedDigest {
    let raw_prompt = normalize_step_text(&cand.first_user_prompt);
    let title = if raw_prompt.is_empty() {
        cand.id.clone()
    } else {
        truncate_chars(&raw_prompt, 48)
    };
    let stopped_at = if settings.language == "zh" {
        "源轨迹日志已被对应工具清理，仅保留初始提问与活跃时间段。".to_string()
    } else {
        "Source trajectory log was pruned by its harness; preserved initial prompt and active spans."
            .to_string()
    };
    let ws_line = cand
        .cwd
        .as_deref()
        .and_then(|p| Path::new(p).file_name())
        .and_then(|n| n.to_str())
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let (suggested_line, is_one_off) = match ws_line {
        Some(name) => (name.to_string(), false),
        None => ("One-off".to_string(), true),
    };
    GeneratedDigest {
        title,
        stopped_at,
        left_open: String::new(),
        suggested_line,
        is_one_off,
    }
}

async fn run_llm_prompt(
    prompt: &str,
    settings: &AppSettings,
    hydrated_path: &OsStr,
    http_client: &reqwest::Client,
) -> Result<LlmPromptOutput> {
    if settings.processor_mode == "api" {
        run_api_prompt(prompt, settings, http_client).await
    } else {
        run_cli_prompt(prompt, settings, hydrated_path).await
    }
}

async fn run_cli_prompt(
    prompt: &str,
    settings: &AppSettings,
    hydrated_path: &OsStr,
) -> Result<LlmPromptOutput> {
    let candidates = resolve_cli_candidates(settings, hydrated_path)?;
    let guarded_prompt = format!(
        "{INTERNAL_DIGEST_SENTINEL} Read-only summarization request. Do NOT run shell commands, inspect files, or invoke tools. Reply ONLY with valid JSON.\n\n{prompt}"
    );
    let model_override = settings.cli_model.trim();

    let mut errors = Vec::new();
    for (harness, bin_path) in candidates {
        match invoke_single_cli(
            &harness,
            &bin_path,
            &guarded_prompt,
            model_override,
            hydrated_path,
        )
        .await
        {
            Ok(result) => return Ok(result),
            Err(err) => errors.push(err.to_string()),
        }
    }

    Err(PlanarError::External(errors.join(" | ")))
}

async fn invoke_single_cli(
    harness: &str,
    bin_path: &Path,
    guarded_prompt: &str,
    model_override: &str,
    hydrated_path: &OsStr,
) -> Result<LlmPromptOutput> {
    let workdir = std::env::temp_dir();

    let (output, codex_tmp_file) = match harness {
        "claude" => {
            let mut cmd = Command::new(bin_path);
            cmd.kill_on_drop(true)
                .current_dir(&workdir)
                .env("PATH", hydrated_path)
                .arg("-p")
                .arg("--no-session-persistence")
                .arg("--tools")
                .arg("");
            if !model_override.is_empty() {
                cmd.arg("--model").arg(model_override);
            }
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            (run_command_with_stdin(cmd, guarded_prompt).await?, None)
        }
        "codex" => {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let tmp_out = workdir.join(format!(
                "planar-codex-digest-{}-{seq}.txt",
                std::process::id()
            ));
            let mut cmd = Command::new(bin_path);
            cmd.kill_on_drop(true)
                .current_dir(&workdir)
                .env("PATH", hydrated_path)
                .arg("exec")
                .arg("--ephemeral")
                .arg("--skip-git-repo-check")
                .arg("--ignore-rules")
                .arg("--color")
                .arg("never")
                .arg("-s")
                .arg("read-only")
                .arg("-c")
                .arg("mcp_servers={}")
                .arg("-c")
                .arg("model_reasoning_effort=\"low\"")
                .arg("-o")
                .arg(&tmp_out);
            if !model_override.is_empty() {
                cmd.arg("-m").arg(model_override);
            }
            cmd.arg("-")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            (
                run_command_with_stdin(cmd, guarded_prompt).await?,
                Some(tmp_out),
            )
        }
        "antigravity" => {
            let mut cmd = Command::new(bin_path);
            cmd.kill_on_drop(true)
                .current_dir(&workdir)
                .env("PATH", hydrated_path)
                .arg("--disable-slash-commands")
                .arg("--sandbox")
                .arg("--effort")
                .arg("low");
            if !model_override.is_empty() {
                cmd.arg("--model").arg(model_override);
            }
            cmd.arg("-p")
                .arg(guarded_prompt)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            (run_command_output(cmd).await?, None)
        }
        "dsh" => {
            let mut cmd = Command::new(bin_path);
            cmd.kill_on_drop(true)
                .current_dir(&workdir)
                .env("PATH", hydrated_path)
                .arg("--profile")
                .arg("headless");
            if !model_override.is_empty() {
                cmd.arg("--model").arg(model_override);
            }
            cmd.arg(guarded_prompt)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            (run_command_output(cmd).await?, None)
        }
        other => {
            return Err(PlanarError::InvalidInput(format!(
                "unsupported CLI harness `{other}`"
            )));
        }
    };

    let tmp_text = codex_tmp_file.as_ref().and_then(|p| {
        let content = fs::read_to_string(p).ok();
        let _ = fs::remove_file(p);
        content
    });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(PlanarError::External(format!(
            "`{harness}` CLI exited with {}: {}",
            output.status,
            truncate_chars(detail, 260)
        )));
    }

    let reply = tmp_text
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| String::from_utf8_lossy(&output.stdout).into_owned());

    if reply.trim().is_empty() {
        return Err(PlanarError::External(format!(
            "`{harness}` CLI returned an empty response"
        )));
    }

    Ok(LlmPromptOutput {
        reply: Ok(reply),
        backend_used: format!("cli:{harness}"),
        usage: SyncUsageDto::local_cli(
            (!model_override.is_empty()).then(|| model_override.to_string()),
        ),
    })
}

fn resolve_cli_candidates(
    settings: &AppSettings,
    hydrated_path: &OsStr,
) -> Result<Vec<(String, PathBuf)>> {
    let harness = settings.cli_harness.trim();
    if harness.is_empty() {
        return Err(PlanarError::InvalidInput(unconfigured_processor_message(
            settings,
        )));
    }
    let custom = settings.cli_bin_override.trim();
    if harness == "auto" && custom.is_empty() {
        let candidates: Vec<(String, PathBuf)> = cli_engine_order()
            .iter()
            .filter_map(|candidate| {
                let candidate = candidate.as_str();
                let bin_name =
                    crate::env::cli_binary_name_for_harness(candidate).unwrap_or(candidate);
                resolve_cli_binary(bin_name, "", hydrated_path)
                    .map(|found| (candidate.to_string(), found))
            })
            .collect();
        if candidates.is_empty() {
            return Err(PlanarError::InvalidInput(
                "no local harness CLI (`agy`, `codex`, `dsh`, or `claude`) found on PATH".into(),
            ));
        }
        Ok(candidates)
    } else {
        Ok(vec![resolve_target_cli(settings, hydrated_path)?])
    }
}

fn resolve_target_cli(settings: &AppSettings, hydrated_path: &OsStr) -> Result<(String, PathBuf)> {
    let harness = settings.cli_harness.trim();
    if harness.is_empty() {
        return Err(PlanarError::InvalidInput(unconfigured_processor_message(
            settings,
        )));
    }
    let custom = settings.cli_bin_override.trim();
    if harness == "auto" {
        for candidate in cli_engine_order() {
            let candidate = candidate.as_str();
            let bin_name = crate::env::cli_binary_name_for_harness(candidate).unwrap_or(candidate);
            if let Some(found) = resolve_cli_binary(bin_name, custom, hydrated_path) {
                return Ok((candidate.to_string(), found));
            }
        }
        return Err(PlanarError::InvalidInput(
            "no local harness CLI (`agy`, `codex`, `dsh`, or `claude`) found on PATH".into(),
        ));
    }

    let Some(bin_name) = crate::env::cli_binary_name_for_harness(harness) else {
        return Err(PlanarError::InvalidInput(format!(
            "unknown CLI harness `{harness}`"
        )));
    };
    let Some(found) = resolve_cli_binary(bin_name, custom, hydrated_path) else {
        return Err(PlanarError::InvalidInput(format!(
            "CLI binary `{bin_name}` for harness `{harness}` not found on PATH"
        )));
    };
    Ok((harness.to_string(), found))
}

async fn run_command_with_stdin(
    mut cmd: Command,
    stdin_payload: &str,
) -> Result<std::process::Output> {
    let mut child = cmd
        .spawn()
        .map_err(|err| PlanarError::External(format!("failed to spawn CLI process: {err}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_payload.as_bytes())
            .await
            .map_err(|err| PlanarError::External(format!("failed writing CLI stdin: {err}")))?;
    }
    tokio::time::timeout(
        Duration::from_secs(CLI_TIMEOUT_SECS),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| {
        PlanarError::External(format!("CLI processor timed out after {CLI_TIMEOUT_SECS}s"))
    })?
    .map_err(|err| PlanarError::External(format!("failed waiting for CLI output: {err}")))
}

async fn run_command_output(mut cmd: Command) -> Result<std::process::Output> {
    tokio::time::timeout(Duration::from_secs(CLI_TIMEOUT_SECS), cmd.output())
        .await
        .map_err(|_| {
            PlanarError::External(format!("CLI processor timed out after {CLI_TIMEOUT_SECS}s"))
        })?
        .map_err(|err| PlanarError::External(format!("failed executing CLI process: {err}")))
}

fn resolve_api_endpoint<'a>(
    provider: &str,
    settings: &'a AppSettings,
) -> Result<(&'a str, &'a str)> {
    let spec = api_provider(provider).ok_or_else(|| {
        PlanarError::InvalidInput(format!("unsupported API provider `{provider}`"))
    })?;
    let base = if settings.api_base_url.trim().is_empty() {
        if spec.requires_base_url || spec.default_base.trim().is_empty() {
            return Err(PlanarError::InvalidInput(format!(
                "API Base URL is required for `{provider}` provider"
            )));
        }
        spec.default_base.trim_end_matches('/')
    } else {
        settings.api_base_url.trim().trim_end_matches('/')
    };
    let model = if settings.api_model.trim().is_empty() {
        spec.default_model.as_str()
    } else {
        settings.api_model.trim()
    };
    Ok((base, model))
}

async fn run_api_prompt(
    prompt: &str,
    settings: &AppSettings,
    client: &reqwest::Client,
) -> Result<LlmPromptOutput> {
    // Pre-dispatch configuration validation: keep these as hard `Err` with no usage record.
    let provider = settings.api_provider.trim().to_ascii_lowercase();
    let api_key = resolve_api_key(&provider, settings)?;
    let (base, model) = resolve_api_endpoint(&provider, settings)?;

    let backend_label = format!("api:{provider}");
    let billing_provider = resolve_billing_provider(&provider, base);

    let dispatched = match provider.as_str() {
        "gemini" => call_gemini_api(client, base, model, &api_key, prompt, billing_provider).await,
        "anthropic" => {
            call_anthropic_api(client, base, model, &api_key, prompt, billing_provider).await
        }
        other => {
            call_openai_compat_api(
                client,
                other,
                base,
                model,
                &api_key,
                prompt,
                billing_provider,
            )
            .await
        }
    };

    match dispatched {
        Ok((reply, parsed_usage)) => {
            let usage = parsed_usage
                .unwrap_or_else(|| SyncUsageDto::missing_api_usage(Some(model.to_string())));
            Ok(LlmPromptOutput {
                reply,
                backend_used: backend_label,
                usage,
            })
        }
        Err(err) => {
            // Keep pre-billing auth/config rejections fatal without fabricating usage; for any
            // other post-dispatch failure (truncated body, invalid JSON, 5xx, etc.), preserve an
            // incomplete usage record so multi-batch totals cannot masquerade as complete.
            if is_fatal_processor_error(&err) {
                Err(err)
            } else {
                Ok(LlmPromptOutput {
                    reply: Err(err),
                    backend_used: backend_label,
                    usage: SyncUsageDto::missing_api_usage(Some(model.to_string())),
                })
            }
        }
    }
}

fn resolve_api_key(provider: &str, settings: &AppSettings) -> Result<String> {
    let trimmed = settings.api_key.trim();
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }

    if let Some(spec) = api_provider(provider) {
        for var in &spec.env_vars {
            if let Ok(val) = std::env::var(var)
                && !val.trim().is_empty()
            {
                return Ok(val.trim().to_string());
            }
        }
        if spec.allow_keyless {
            return Ok(String::new());
        }
    }

    let base_lower = settings.api_base_url.to_ascii_lowercase();
    if provider == "openai-compatible"
        && (base_lower.contains("127.0.0.1") || base_lower.contains("localhost"))
    {
        return Ok(String::new());
    }

    Err(PlanarError::InvalidInput(format!(
        "missing API key for provider `{provider}` (set it in Settings → Processor or via environment variable)"
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeminiGenerationProfile {
    thinking_level: Option<&'static str>,
    include_temperature: bool,
}

fn strip_gemini_models_prefix(model: &str) -> &str {
    let trimmed = model.trim();
    if trimmed
        .as_bytes()
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"models/"))
    {
        &trimmed[7..]
    } else {
        trimmed
    }
}

fn gemini_generation_profile(model: &str) -> GeminiGenerationProfile {
    let m = strip_gemini_models_prefix(model).to_ascii_lowercase();

    if m.starts_with("gemini-4-") || m.starts_with("gemini-4.") {
        return GeminiGenerationProfile {
            thinking_level: Some("low"),
            include_temperature: false,
        };
    }

    if m.starts_with("gemini-3.6-flash") {
        return GeminiGenerationProfile {
            thinking_level: Some("minimal"),
            include_temperature: false,
        };
    }

    if m.starts_with("gemini-3.5-flash-lite")
        || m.starts_with("gemini-3.1-flash-lite")
        || m == "gemini-3-flash"
        || m.starts_with("gemini-3-flash-")
    {
        return GeminiGenerationProfile {
            thinking_level: Some("minimal"),
            include_temperature: true,
        };
    }

    if m.starts_with("gemini-3.8-flash") {
        return GeminiGenerationProfile {
            thinking_level: Some("low"),
            include_temperature: false,
        };
    }

    if m.starts_with("gemini-3.1-pro") || m.starts_with("gemini-3-pro") {
        return GeminiGenerationProfile {
            thinking_level: Some("low"),
            include_temperature: true,
        };
    }

    GeminiGenerationProfile {
        thinking_level: None,
        include_temperature: true,
    }
}

fn build_gemini_request_body(model: &str, prompt: &str) -> Value {
    let profile = gemini_generation_profile(model);
    let mut generation_config = serde_json::Map::new();
    if profile.include_temperature {
        generation_config.insert("temperature".to_string(), json!(0.2));
    }
    generation_config.insert("responseMimeType".to_string(), json!("application/json"));
    if let Some(level) = profile.thinking_level {
        generation_config.insert(
            "thinkingConfig".to_string(),
            json!({ "thinkingLevel": level }),
        );
    }

    json!({
        "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
        "generationConfig": Value::Object(generation_config),
    })
}

fn extract_gemini_reply_text(val: &Value) -> Result<String> {
    let mut text = String::new();
    if let Some(parts) = val
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(Value::as_array)
    {
        for p in parts {
            if p.get("thought").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            if let Some(t) = p.get("text").and_then(Value::as_str) {
                text.push_str(t);
            }
        }
    }
    if text.trim().is_empty() {
        Err(PlanarError::External(
            "Gemini API returned empty content".into(),
        ))
    } else {
        Ok(text)
    }
}

fn json_u64(obj: &Value, key: &str) -> Option<u64> {
    obj.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
    })
}

fn build_api_usage(
    billing_provider: Option<&str>,
    model: &str,
    input_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    output_tokens: u64,
    thinking_tokens: u64,
) -> Option<SyncUsageDto> {
    let total_tokens = input_tokens
        .saturating_add(cache_read_tokens)
        .saturating_add(cache_write_tokens)
        .saturating_add(output_tokens)
        .saturating_add(thinking_tokens);
    if total_tokens == 0 {
        return None;
    }
    let cost_usd = billing_provider.and_then(|bp| {
        estimate_api_cost_usd(
            Some(bp),
            Some(model),
            input_tokens,
            cache_read_tokens,
            cache_write_tokens,
            output_tokens,
            thinking_tokens,
        )
    });
    Some(SyncUsageDto {
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        thinking_tokens,
        total_tokens,
        cost_usd,
        model: Some(model.to_string()),
        is_local_cli: false,
        api_calls: 1,
        priced_calls: usize::from(cost_usd.is_some()),
        incomplete: cost_usd.is_none(),
    })
}

/// Extracts token usage and computes USD cost from a Gemini `generateContent` JSON response.
#[must_use]
pub fn parse_gemini_usage(val: &Value, model: &str) -> Option<SyncUsageDto> {
    parse_gemini_usage_with_billing(val, Some("google"), model)
}

fn parse_gemini_usage_with_billing(
    val: &Value,
    billing_provider: Option<&str>,
    model: &str,
) -> Option<SyncUsageDto> {
    let meta = val.get("usageMetadata")?;
    let prompt_tokens = json_u64(meta, "promptTokenCount").unwrap_or(0);
    let cache_read_tokens = json_u64(meta, "cachedContentTokenCount").unwrap_or(0);
    let input_tokens = prompt_tokens.saturating_sub(cache_read_tokens);
    let output_tokens = json_u64(meta, "candidatesTokenCount").unwrap_or(0);
    let thinking_tokens = json_u64(meta, "thoughtsTokenCount").unwrap_or(0);
    build_api_usage(
        billing_provider,
        model,
        input_tokens,
        cache_read_tokens,
        0,
        output_tokens,
        thinking_tokens,
    )
}

/// Extracts token usage and computes USD cost from an Anthropic `/messages` JSON response.
#[must_use]
pub fn parse_anthropic_usage(val: &Value, model: &str) -> Option<SyncUsageDto> {
    parse_anthropic_usage_with_billing(val, Some("anthropic"), model)
}

fn parse_anthropic_usage_with_billing(
    val: &Value,
    billing_provider: Option<&str>,
    model: &str,
) -> Option<SyncUsageDto> {
    let u = val.get("usage")?;
    let input_tokens = json_u64(u, "input_tokens").unwrap_or(0);
    let cache_read_tokens = json_u64(u, "cache_read_input_tokens").unwrap_or(0);
    let cache_write_tokens = json_u64(u, "cache_creation_input_tokens").unwrap_or(0);
    let output_tokens = json_u64(u, "output_tokens").unwrap_or(0);
    build_api_usage(
        billing_provider,
        model,
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        0,
    )
}

/// Extracts token usage and computes USD cost from an OpenAI-compatible `/chat/completions`
/// JSON response (including `prompt_tokens_details.cached_tokens`, `DeepSeek`
/// `prompt_cache_hit_tokens`, and optional `completion_tokens_details.reasoning_tokens`).
#[must_use]
pub fn parse_openai_compat_usage(val: &Value, provider: &str, model: &str) -> Option<SyncUsageDto> {
    parse_openai_compat_usage_with_billing(val, canonical_billing_provider(provider), model)
}

fn parse_openai_compat_usage_with_billing(
    val: &Value,
    billing_provider: Option<&str>,
    model: &str,
) -> Option<SyncUsageDto> {
    let u = val.get("usage")?;
    let cache_read_tokens = u
        .get("prompt_tokens_details")
        .and_then(|d| json_u64(d, "cached_tokens"))
        .or_else(|| json_u64(u, "prompt_cache_hit_tokens"))
        .unwrap_or(0);
    let input_tokens = if let Some(prompt_tok) = json_u64(u, "prompt_tokens") {
        prompt_tok.saturating_sub(cache_read_tokens)
    } else if let Some(miss) = json_u64(u, "prompt_cache_miss_tokens") {
        miss
    } else {
        json_u64(u, "input_tokens")
            .unwrap_or(0)
            .saturating_sub(cache_read_tokens)
    };
    let raw_completion = json_u64(u, "completion_tokens")
        .or_else(|| json_u64(u, "output_tokens"))
        .unwrap_or(0);
    let thinking_tokens = u
        .get("completion_tokens_details")
        .and_then(|d| json_u64(d, "reasoning_tokens"))
        .or_else(|| {
            u.get("output_tokens_details")
                .and_then(|d| json_u64(d, "reasoning_tokens"))
        })
        .unwrap_or(0);
    let output_tokens = raw_completion.saturating_sub(thinking_tokens);
    build_api_usage(
        billing_provider,
        model,
        input_tokens,
        cache_read_tokens,
        0,
        output_tokens,
        thinking_tokens,
    )
}

async fn call_gemini_api(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
    billing_provider: Option<&str>,
) -> Result<(Result<String>, Option<SyncUsageDto>)> {
    let model_path = strip_gemini_models_prefix(model);
    let url = format!("{base}/models/{model_path}:generateContent?key={api_key}");
    let body = build_gemini_request_body(model_path, prompt);

    let val = send_json_post(client.post(&url).json(&body), "Gemini").await?;
    // Capture reported usage before validating text content so billable empty replies are accounted for.
    let usage = parse_gemini_usage_with_billing(&val, billing_provider, model);
    let reply = extract_gemini_reply_text(&val);
    Ok((reply, usage))
}

async fn call_anthropic_api(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
    billing_provider: Option<&str>,
) -> Result<(Result<String>, Option<SyncUsageDto>)> {
    let url = format!("{base}/messages");
    let body = json!({
        "model": model,
        "max_tokens": 4096,
        "temperature": 0.2,
        "messages": [{ "role": "user", "content": prompt }]
    });

    let val = send_json_post(
        client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body),
        "Anthropic",
    )
    .await?;

    let usage = parse_anthropic_usage_with_billing(&val, billing_provider, model);
    let mut text = String::new();
    if let Some(blocks) = val.get("content").and_then(Value::as_array) {
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some("text")
                && let Some(t) = block.get("text").and_then(Value::as_str)
            {
                text.push_str(t);
            }
        }
    }
    let reply = if text.trim().is_empty() {
        Err(PlanarError::External(
            "Anthropic API returned empty content".into(),
        ))
    } else {
        Ok(text)
    };
    Ok((reply, usage))
}

async fn call_openai_compat_api(
    client: &reqwest::Client,
    provider: &str,
    base: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
    billing_provider: Option<&str>,
) -> Result<(Result<String>, Option<SyncUsageDto>)> {
    let url = format!("{base}/chat/completions");
    let body = json!({
        "model": model,
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": "Reply with strictly valid JSON only." },
            { "role": "user", "content": prompt }
        ]
    });

    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }

    let val = send_json_post(req, provider).await?;
    let usage = parse_openai_compat_usage_with_billing(&val, billing_provider, model);
    let text = val
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let reply = if text.trim().is_empty() {
        Err(PlanarError::External(format!(
            "`{provider}` API returned empty content"
        )))
    } else {
        Ok(text)
    };
    Ok((reply, usage))
}

fn is_transient_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
}

/// Parses an RFC 9110 §10.2.3 `Retry-After` header in either `delay-seconds` or `HTTP-date`
/// (IMF-fixdate / RFC 2822, obsolete RFC 850, or ANSI C `asctime()`) format into a minimum delay
/// in milliseconds relative to `now`.
fn parse_retry_after_ms(raw: &str, now: DateTime<Utc>) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return trimmed.parse::<u64>().ok().map(|s| s.saturating_mul(1_000));
    }

    let target_utc = DateTime::parse_from_rfc2822(trimmed)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(trimmed, "%a, %d %b %Y %H:%M:%S GMT")
                .ok()
                .map(|dt| dt.and_utc())
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(trimmed, "%A, %d-%b-%y %H:%M:%S GMT")
                .ok()
                .map(|dt| dt.and_utc())
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(trimmed, "%a %b %e %H:%M:%S %Y")
                .ok()
                .map(|dt| dt.and_utc())
        })?;

    let delta_ms = (target_utc - now).num_milliseconds();
    if delta_ms <= 0 {
        Some(0)
    } else {
        Some(u64::try_from(delta_ms).unwrap_or(u64::MAX))
    }
}

/// Computes the retry delay in milliseconds for a transient HTTP `429` / `503` response.
/// Returns `None` (skipping automatic retry) when a provider's `Retry-After` minimum delay
/// exceeds our retry budget (`API_RETRY_MAX_DELAY_MS`), so Planar never violates provider
/// minimums or stalls workers. Always preserves bounded jitter when budget permits.
fn compute_retry_backoff_ms(
    attempt: usize,
    retry_after_header: Option<&str>,
    now: DateTime<Utc>,
) -> Option<u64> {
    let provider_min_ms = retry_after_header.and_then(|raw| parse_retry_after_ms(raw, now));
    if let Some(min_ms) = provider_min_ms
        && min_ms > API_RETRY_MAX_DELAY_MS
    {
        return None;
    }

    #[cfg(test)]
    let test_override = TEST_RETRY_BASE_DELAY_MS_OVERRIDE
        .try_with(|v| *v)
        .ok()
        .flatten();
    #[cfg(not(test))]
    let test_override: Option<u64> = None;

    if let Some(base) = test_override {
        let shift = u32::try_from(attempt).unwrap_or(0).min(4);
        return Some(base.saturating_mul(1_u64 << shift));
    }

    let shift = u32::try_from(attempt).unwrap_or(0).min(4);
    let base_delay =
        provider_min_ms.unwrap_or_else(|| API_RETRY_BASE_DELAY_MS.saturating_mul(1_u64 << shift));
    let remaining_budget = API_RETRY_MAX_DELAY_MS.saturating_sub(base_delay);
    let max_jitter = (API_RETRY_BASE_DELAY_MS / 2).min(remaining_budget);
    let jitter = if max_jitter == 0 {
        0
    } else {
        let seq = RETRY_JITTER_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = u64::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos()),
        );
        (seq.wrapping_mul(6_364_136_223_846_793_005) ^ nanos) % (max_jitter + 1)
    };
    Some(
        base_delay
            .saturating_add(jitter)
            .min(API_RETRY_MAX_DELAY_MS),
    )
}

async fn send_json_post(req: reqwest::RequestBuilder, label: &str) -> Result<Value> {
    let mut current = req;
    for attempt in 0..=API_TRANSIENT_MAX_RETRIES {
        let next_attempt = if attempt < API_TRANSIENT_MAX_RETRIES {
            current.try_clone()
        } else {
            None
        };
        let resp = current
            .send()
            .await
            .map_err(|err| PlanarError::External(format!("{label} request failed: {err}")))?;
        let status = resp.status();

        // Decide transient retry eligibility BEFORE reading the response body so truncated/reset
        // 429/503 gateway responses still retry cleanly, while skipping automatic retry whenever
        // the provider's `Retry-After` minimum delay exceeds our retry budget.
        if is_transient_retryable_status(status)
            && let Some(cloned) = next_attempt
        {
            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            if let Some(delay_ms) =
                compute_retry_backoff_ms(attempt, retry_after.as_deref(), Utc::now())
            {
                drop(resp);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                current = cloned;
                continue;
            }
        }

        let body_text = resp
            .text()
            .await
            .map_err(|err| PlanarError::External(format!("{label} body read failed: {err}")))?;

        if !status.is_success() {
            return Err(PlanarError::External(format!(
                "{label} API HTTP {status}: {}",
                truncate_chars(body_text.trim(), 240)
            )));
        }
        return serde_json::from_str::<Value>(&body_text)
            .map_err(|err| PlanarError::External(format!("{label} returned invalid JSON: {err}")));
    }
    Err(PlanarError::External(format!(
        "{label} API failed after {API_TRANSIENT_MAX_RETRIES} retries"
    )))
}

fn format_existing_lines(catalog: &[LineCatalogEntry]) -> String {
    if catalog.is_empty() {
        "(none yet)".to_string()
    } else {
        catalog
            .iter()
            .map(|entry| {
                let kind = if entry.pinned { "pinned" } else { "discovered" };
                let desc = entry.description.trim();
                if desc.is_empty() {
                    format!(
                        "- {} [{}, {} sessions]",
                        entry.name, kind, entry.session_count
                    )
                } else {
                    format!(
                        "- {} [{}, {} sessions] — Scope: {}",
                        entry.name, kind, entry.session_count, desc
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn digest_lang_hint(settings: &AppSettings) -> &'static str {
    if settings.language == "zh" {
        "Write `title`, `stoppedAt`, and `leftOpen` in concise natural Simplified Chinese (keep technical symbols/file names as-is). For `suggestedLine`, mirror the naming style and language of `[pinned]` Lines, keeping proper repo/product codenames as-is."
    } else {
        "Write `title`, `stoppedAt`, and `leftOpen` in concise natural English. For `suggestedLine`, mirror the naming style of `[pinned]` Lines, keeping proper repo/product codenames as-is."
    }
}

fn build_digest_prompt(
    cand: &SyncCandidate,
    sliced: &SlicedTrajectory,
    catalog: &[LineCatalogEntry],
    settings: &AppSettings,
) -> String {
    let lang_hint = digest_lang_hint(settings);
    let existing_lines = format_existing_lines(catalog);

    let mut transcript = String::new();
    append_slice_section(&mut transcript, "HEAD (Opening turns)", &sliced.head);
    append_slice_section(
        &mut transcript,
        "MIDDLE (User prompts only)",
        &sliced.middle_users,
    );
    append_slice_section(&mut transcript, "TAIL (Closing turns)", &sliced.tail);

    format!(
        "You are Planar's work-plane summarizer. Summarize one coding-agent session into a scannable `title`, recap (`stoppedAt`, `leftOpen`), and its `suggestedLine` on a 2D (Lines × Time) plane.\n\
{lang_hint}\n\
Return ONLY one JSON object matching:\n\
{{\"title\":\"concise session task summary\",\"stoppedAt\":\"concrete 1-2 sentence outcome\",\"leftOpen\":\"remaining work or empty string\",\"suggestedLine\":\"existing or short Work Line handle\",\"isOneOff\":false}}\n\
Context & objectives:\n\
- A Work Line represents an ongoing multi-session project, subsystem, or focus stream (distinct from this single session's task `title`), rendered in a narrow ~100px swimlane sidebar where short, recognizable handles scan best.\n\
- Reuse an existing Line name verbatim whenever this session continues that effort; treat `[pinned]` Lines as exemplars of the user's preferred granularity, and follow any explicit `Scope:` note as the user's authoritative inclusion rule.\n\
- Treat `cwd` as context rather than an automatic bucket: centralized workspaces (vaults, monorepos, home/work roots) often host many distinct Lines, while a dedicated project folder often carries its natural handle.\n\
- Set `isOneOff: true` and `suggestedLine: \"One-off\"` for self-contained lookups, environment tweaks, or isolated errands unlikely to span multiple sessions.\n\
\n\
Session metadata:\n\
- Harness: {}\n\
- Workspace (cwd): {}\n\
- Total extracted dialogue steps: {}\n\
\n\
Existing Lines:\n{existing_lines}\n\n\
Sliced Trajectory:\n{transcript}",
        cand.harness,
        cand.cwd.as_deref().unwrap_or("(unknown)"),
        sliced.total_steps
    )
}

fn build_batch_digest_prompt(
    batch: &[PreparedSession],
    catalog: &[LineCatalogEntry],
    settings: &AppSettings,
) -> String {
    let lang_hint = digest_lang_hint(settings);
    let existing_lines = format_existing_lines(catalog);

    let mut sessions_blob = String::new();
    for (idx, item) in batch.iter().enumerate() {
        let _ = writeln!(
            sessions_blob,
            "=== SESSION {} [id=\"{}\"] ===",
            idx + 1,
            item.cand.id
        );
        let _ = writeln!(
            sessions_blob,
            "Harness: {} | Workspace (cwd): {} | Total extracted dialogue steps: {}",
            item.cand.harness,
            item.cand.cwd.as_deref().unwrap_or("(unknown)"),
            item.sliced.total_steps
        );
        append_slice_section(
            &mut sessions_blob,
            "HEAD (Opening turns)",
            &item.sliced.head,
        );
        append_slice_section(
            &mut sessions_blob,
            "MIDDLE (User prompts only)",
            &item.sliced.middle_users,
        );
        append_slice_section(
            &mut sessions_blob,
            "TAIL (Closing turns)",
            &item.sliced.tail,
        );
        sessions_blob.push('\n');
    }

    format!(
        "You are Planar's work-plane summarizer. Summarize all {} coding-agent sessions below and assign each session's `suggestedLine` on Planar's 2D (Lines × Time) plane.\n\
{lang_hint}\n\
Return ONLY one JSON object mapping EVERY exact `id` from the sessions below to its digest:\n\
{{\"digests\":{{\"{}\":{{\"title\":\"concise session task summary\",\"stoppedAt\":\"concrete 1-2 sentence outcome\",\"leftOpen\":\"remaining work or empty string\",\"suggestedLine\":\"existing or short Work Line handle\",\"isOneOff\":false}}}}}}\n\
Context & objectives:\n\
- Include an entry in `digests` for every single session `id` listed below.\n\
- A Work Line represents an ongoing multi-session project, subsystem, or focus stream (distinct from a single session's task `title`), rendered in a narrow ~100px swimlane sidebar where short, recognizable handles scan best.\n\
- Reuse an existing Line name verbatim whenever a session continues that effort; when multiple sessions in this batch belong to the same ongoing effort, assign them the exact same `suggestedLine`.\n\
- Treat `[pinned]` Lines as exemplars of the user's preferred granularity, follow any explicit `Scope:` note as the user's authoritative inclusion rule, and treat `cwd` as context rather than an automatic bucket (centralized vaults, monorepos, or work roots often host multiple distinct Lines).\n\
- Set `isOneOff: true` and `suggestedLine: \"One-off\"` for self-contained lookups, environment tweaks, or isolated errands unlikely to span multiple sessions.\n\
\n\
Existing Lines:\n{existing_lines}\n\n\
Sessions to summarize ({} total):\n{sessions_blob}",
        batch.len(),
        batch.first().map_or("<sessionId>", |p| p.cand.id.as_str()),
        batch.len()
    )
}

fn build_regroup_prompt(
    items: &[UnpinnedClusterItem],
    catalog: &[LineCatalogEntry],
    settings: &AppSettings,
) -> String {
    let lang_hint = if settings.language == "zh" {
        "Match the naming style and language of Pinned Lines (concise Simplified Chinese or proper repo/project codenames)."
    } else {
        "Match the naming style of Pinned Lines (crisp English handles or proper repo/project codenames)."
    };
    let pinned_names: Vec<String> = catalog
        .iter()
        .filter(|entry| entry.pinned)
        .map(|entry| {
            let desc = entry.description.trim();
            if desc.is_empty() {
                format!("- {} ({} sessions)", entry.name, entry.session_count)
            } else {
                format!(
                    "- {} ({} sessions) — Scope: {}",
                    entry.name, entry.session_count, desc
                )
            }
        })
        .collect();
    let item_lines: Vec<String> = items
        .iter()
        .map(|it| {
            format!(
                "- id=\"{}\" | current=\"{}\" | cwd=\"{}\" | title=\"{}\" | stoppedAt=\"{}\"",
                it.session_id,
                it.current_line,
                it.cwd.as_deref().unwrap_or("(unknown)"),
                it.title,
                truncate_chars(&it.stopped_at, 180)
            )
        })
        .collect();

    format!(
        "You are organizing unpinned coding-agent sessions on Planar's 2D (Lines × Time) plane into coherent, durable Work Lines. {lang_hint}\n\
Return ONLY valid JSON mapping every `sessionId` to its best Line name (or \"One-off\" if isolated):\n\
{{\"assignments\": {{\"<sessionId>\": \"<Line Name>\"}}}}\n\
Context & objectives:\n\
- Line names render in a narrow ~100px swimlane sidebar, so short, recognizable handles (project/subsystem codenames or concise topic handles rather than single-task sentences) scan best.\n\
- Weigh `cwd`, `title`, and `stoppedAt` together: consolidate fragmented single-session Lines that belong to the same broader effort, reuse a Pinned Line verbatim whenever a session matches its name or `Scope:` note, avoid collapsing unrelated work that merely shares a centralized `cwd`, and assign isolated errands to \"One-off\".\n\
\n\
Pinned Lines (user-kept exemplars; reuse verbatim if strongly related):\n{}\n\
\n\
Unpinned sessions to cluster:\n{}",
        if pinned_names.is_empty() {
            "(none)".to_string()
        } else {
            pinned_names.join("\n")
        },
        item_lines.join("\n")
    )
}

fn append_slice_section(buf: &mut String, heading: &str, steps: &[SlicedStep]) {
    if steps.is_empty() {
        return;
    }
    let _ = writeln!(buf, "### {heading}");
    for step in steps {
        let _ = writeln!(buf, "[{} #{}] {}", step.role, step.index, step.text);
    }
}

/// Conservative byte-level pre-filter that skips non-dialogue lines (such as multi-megabyte tool
/// call payloads, progress snapshots, and telemetry events) before invoking `serde_json`.
/// Any line containing JSON `\u` escapes conservatively falls through to full parsing.
fn can_possibly_be_dialogue_line(harness: &str, line: &str) -> bool {
    if line.contains("\\u") {
        return true;
    }
    match harness {
        "antigravity" => line.contains("\"USER_INPUT\"") || line.contains("\"PLANNER_RESPONSE\""),
        "codex" => {
            line.contains("\"response_item\"")
                && line.contains("\"message\"")
                && (line.contains("\"user\"") || line.contains("\"assistant\""))
        }
        "claude" => {
            (line.contains("\"user\"") || line.contains("\"assistant\""))
                && line.contains("\"message\"")
                && line.contains("\"content\"")
        }
        "dsh" => {
            line.contains("\"user\"")
                || line.contains("\"USER_INPUT\"")
                || line.contains("\"user_message\"")
                || line.contains("\"assistant\"")
                || line.contains("\"model\"")
                || line.contains("\"agent\"")
                || line.contains("\"PLANNER_RESPONSE\"")
                || line.contains("\"assistant_message\"")
        }
        _ => false,
    }
}

#[derive(Deserialize)]
struct AgSliceLine<'a> {
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    content: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct ClaudeSliceLine<'a> {
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    #[serde(borrow)]
    message: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct ClaudeSliceMessage<'a> {
    #[serde(borrow)]
    content: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct ClaudeSliceTextObj<'a> {
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct ClaudeSliceBlock<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct CodexSliceLine<'a> {
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct CodexSlicePayloadHeader<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
    #[serde(borrow)]
    content: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct DshSliceHeader<'a> {
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
    #[serde(borrow)]
    message: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct DshSliceMsgRole<'a> {
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
}

fn extract_step_text_fast(harness: &str, line: &str) -> Option<(&'static str, String)> {
    // Baseline `extract_step_text` only extracts fields from JSON objects (`val.get(...)`);
    // rejecting non-object lines up front also prevents serde struct `visit_seq` array matching.
    if !line.trim_start().starts_with('{') || !can_possibly_be_dialogue_line(harness, line) {
        return None;
    }
    let fast_res = match harness {
        "claude" => extract_claude_step_fast(line),
        "codex" => extract_codex_step_fast(line),
        "antigravity" => extract_antigravity_step_fast(line),
        "dsh" => extract_dsh_step_fast(line),
        _ => return None,
    };
    if let Ok(step) = fast_res {
        step
    } else {
        let val = serde_json::from_str::<Value>(line).ok()?;
        extract_step_text(harness, &val)
    }
}

fn extract_antigravity_step_fast(
    line: &str,
) -> std::result::Result<Option<(&'static str, String)>, ()> {
    let rec: AgSliceLine<'_> = serde_json::from_str(line).map_err(|_| ())?;
    let Some(rtype) = rec.record_type.as_deref() else {
        return Ok(None);
    };
    let Some(content) = rec.content.as_deref() else {
        return Ok(None);
    };
    if rtype == "USER_INPUT" {
        let req = extract_antigravity_user_request(content);
        let unwrapped = strip_delegation_envelope(&req);
        Ok((!unwrapped.is_empty()).then(|| ("user", unwrapped.to_string())))
    } else if rtype == "PLANNER_RESPONSE" {
        let trimmed = content.trim();
        Ok((!trimmed.is_empty()).then(|| ("agent", trimmed.to_string())))
    } else {
        Ok(None)
    }
}

fn extract_claude_step_fast(line: &str) -> std::result::Result<Option<(&'static str, String)>, ()> {
    let rec: ClaudeSliceLine<'_> = serde_json::from_str(line).map_err(|_| ())?;
    let Some(rtype) = rec.record_type.as_deref() else {
        return Ok(None);
    };
    if !matches!(rtype, "user" | "assistant") {
        return Ok(None);
    }
    if rec.is_meta.unwrap_or(false) || rec.is_sidechain.unwrap_or(false) {
        return Ok(None);
    }
    let Some(msg_raw) = rec.message else {
        return Ok(None);
    };
    let msg_json = msg_raw.get().trim_start();
    if !msg_json.starts_with('{') {
        return Ok(None);
    }
    let msg: ClaudeSliceMessage<'_> = serde_json::from_str(msg_json).map_err(|_| ())?;
    let Some(content_raw) = msg.content else {
        return Ok(None);
    };
    let content_json = content_raw.get().trim_start();
    let first_byte = content_json.as_bytes().first().copied();

    if rtype == "user" {
        let human = match first_byte {
            Some(b'"') => {
                let s: Cow<'_, str> = serde_json::from_str(content_json).map_err(|_| ())?;
                let trimmed = s.trim();
                if trimmed.is_empty() || is_claude_synthetic_wrapper(trimmed) {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Some(b'[') => {
                let raw_blocks: Vec<&RawValue> =
                    serde_json::from_str(content_json).map_err(|_| ())?;
                let mut joined = String::new();
                for raw_block in raw_blocks {
                    let block_json = raw_block.get().trim_start();
                    if !block_json.starts_with('{') {
                        continue;
                    }
                    let block: ClaudeSliceBlock<'_> =
                        serde_json::from_str(block_json).map_err(|_| ())?;
                    if block.kind.as_deref() != Some("text") {
                        continue;
                    }
                    let Some(txt) = block.text.as_deref() else {
                        continue;
                    };
                    let trimmed = txt.trim();
                    if trimmed.is_empty() || is_claude_synthetic_wrapper(trimmed) {
                        continue;
                    }
                    if !joined.is_empty() {
                        joined.push(' ');
                    }
                    joined.push_str(trimmed);
                }
                (!joined.is_empty()).then_some(joined)
            }
            _ => None,
        };
        let Some(human) = human else {
            return Ok(None);
        };
        let unwrapped = strip_delegation_envelope(&human);
        Ok((!unwrapped.is_empty()).then(|| ("user", unwrapped.to_string())))
    } else {
        let text = match first_byte {
            Some(b'"') => {
                let s: Cow<'_, str> = serde_json::from_str(content_json).map_err(|_| ())?;
                let trimmed = s.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            Some(b'[') => {
                let raw_blocks: Vec<&RawValue> =
                    serde_json::from_str(content_json).map_err(|_| ())?;
                let mut joined = String::new();
                for raw_block in raw_blocks {
                    let block_json = raw_block.get().trim_start();
                    if !block_json.starts_with('{') {
                        continue;
                    }
                    let block: ClaudeSliceBlock<'_> =
                        serde_json::from_str(block_json).map_err(|_| ())?;
                    let kind = block.kind.as_deref().unwrap_or("text");
                    if matches!(
                        kind,
                        "tool_use"
                            | "tool_result"
                            | "thinking"
                            | "reasoning"
                            | "tool_call"
                            | "function_call"
                            | "function_call_output"
                            | "custom_tool_call"
                            | "custom_tool_call_output"
                    ) {
                        continue;
                    }
                    if let Some(txt) = block.text.as_deref() {
                        let trimmed = txt.trim();
                        if !trimmed.is_empty() {
                            if !joined.is_empty() {
                                joined.push(' ');
                            }
                            joined.push_str(trimmed);
                        }
                    }
                }
                (!joined.is_empty()).then_some(joined)
            }
            Some(b'{') => {
                let obj: ClaudeSliceTextObj<'_> =
                    serde_json::from_str(content_json).map_err(|_| ())?;
                obj.text
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            }
            _ => None,
        };
        Ok(text.map(|t| ("agent", t)))
    }
}

fn extract_codex_step_fast(line: &str) -> std::result::Result<Option<(&'static str, String)>, ()> {
    let rec: CodexSliceLine<'_> = serde_json::from_str(line).map_err(|_| ())?;
    if rec.record_type.as_deref() != Some("response_item") {
        return Ok(None);
    }
    let Some(payload_raw) = rec.payload else {
        return Ok(None);
    };
    let payload_json = payload_raw.get().trim_start();
    if !payload_json.starts_with('{') {
        return Ok(None);
    }
    let header: CodexSlicePayloadHeader<'_> = serde_json::from_str(payload_json).map_err(|_| ())?;
    if header.kind.as_deref() != Some("message") {
        return Ok(None);
    }
    let Some(role) = header.role.as_deref() else {
        return Ok(None);
    };
    if role != "user" && role != "assistant" {
        return Ok(None);
    }
    let Some(content_raw) = header.content else {
        return Ok(None);
    };
    let content_val: Value = serde_json::from_str(content_raw.get()).map_err(|_| ())?;
    if role == "user" {
        let Some(human) = extract_codex_user_value(&content_val) else {
            return Ok(None);
        };
        let unwrapped = strip_delegation_envelope(&human);
        Ok((!unwrapped.is_empty()).then(|| ("user", unwrapped.to_string())))
    } else {
        Ok(extract_text_from_value(&content_val).map(|t| ("agent", t)))
    }
}

fn extract_dsh_step_fast(line: &str) -> std::result::Result<Option<(&'static str, String)>, ()> {
    let header: DshSliceHeader<'_> = serde_json::from_str(line).map_err(|_| ())?;
    let rtype = header.record_type.as_deref().unwrap_or("");
    let msg_role = match header.message {
        Some(msg_raw) if msg_raw.get().trim_start().starts_with('{') => {
            let parsed: DshSliceMsgRole<'_> =
                serde_json::from_str(msg_raw.get().trim_start()).map_err(|_| ())?;
            parsed.role
        }
        _ => None,
    };
    let role = header.role.as_deref().or(msg_role.as_deref()).unwrap_or("");
    if role == "tool"
        || matches!(
            rtype,
            "tool" | "tool_result" | "tool_use" | "function_call" | "function_call_output"
        )
    {
        return Ok(None);
    }
    let is_user = role == "user" || matches!(rtype, "user" | "USER_INPUT" | "user_message");
    let is_agent = role == "assistant"
        || role == "model"
        || matches!(
            rtype,
            "assistant" | "agent" | "PLANNER_RESPONSE" | "assistant_message"
        );
    if !is_user && !is_agent {
        return Ok(None);
    }
    let val: Value = serde_json::from_str(line).map_err(|_| ())?;
    Ok(extract_dsh_step(&val))
}

fn extract_step_text(harness: &str, val: &Value) -> Option<(&'static str, String)> {
    match harness {
        "claude" => extract_claude_step(val),
        "codex" => extract_codex_step(val),
        "antigravity" => extract_antigravity_step(val),
        "dsh" => extract_dsh_step(val),
        _ => None,
    }
}

fn extract_claude_step(val: &Value) -> Option<(&'static str, String)> {
    let rtype = val.get("type").and_then(Value::as_str)?;
    if !matches!(rtype, "user" | "assistant") {
        return None;
    }
    if val.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
        || val
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return None;
    }
    let msg = val.get("message")?;
    let content = msg.get("content")?;
    if rtype == "user" {
        let human = extract_claude_user_value(content)?;
        let unwrapped = strip_delegation_envelope(&human);
        (!unwrapped.is_empty()).then(|| ("user", unwrapped.to_string()))
    } else {
        let text = extract_text_from_value(content)?;
        Some(("agent", text))
    }
}

fn extract_claude_user_value(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() || is_claude_synthetic_wrapper(trimmed) {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(blocks) => {
            let mut joined = String::new();
            for block in blocks {
                if block.get("type").and_then(Value::as_str) != Some("text") {
                    continue;
                }
                let Some(txt) = block.get("text").and_then(Value::as_str) else {
                    continue;
                };
                let trimmed = txt.trim();
                if trimmed.is_empty() || is_claude_synthetic_wrapper(trimmed) {
                    continue;
                }
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(trimmed);
            }
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

fn extract_codex_step(val: &Value) -> Option<(&'static str, String)> {
    if val.get("type").and_then(Value::as_str)? != "response_item" {
        return None;
    }
    let payload = val.get("payload")?;
    if payload.get("type").and_then(Value::as_str)? != "message" {
        return None;
    }
    let role = payload.get("role").and_then(Value::as_str)?;
    let content = payload.get("content")?;
    if role == "user" {
        let human = extract_codex_user_value(content)?;
        let unwrapped = strip_delegation_envelope(&human);
        (!unwrapped.is_empty()).then(|| ("user", unwrapped.to_string()))
    } else if role == "assistant" {
        let text = extract_text_from_value(content)?;
        Some(("agent", text))
    } else {
        None
    }
}

fn extract_codex_user_value(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() || is_codex_synthetic_block(trimmed) {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(parts) => {
            let mut joined = String::new();
            for part in parts {
                let txt = part
                    .as_str()
                    .or_else(|| part.get("text").and_then(Value::as_str));
                let Some(raw_txt) = txt else {
                    continue;
                };
                let trimmed = raw_txt.trim();
                if trimmed.is_empty() || is_codex_synthetic_block(trimmed) {
                    continue;
                }
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(trimmed);
            }
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

fn extract_antigravity_step(val: &Value) -> Option<(&'static str, String)> {
    let rtype = val.get("type").and_then(Value::as_str)?;
    let content = val.get("content").and_then(Value::as_str)?;
    if rtype == "USER_INPUT" {
        let req = extract_antigravity_user_request(content);
        let unwrapped = strip_delegation_envelope(&req);
        (!unwrapped.is_empty()).then(|| ("user", unwrapped.to_string()))
    } else if rtype == "PLANNER_RESPONSE" {
        let trimmed = content.trim();
        (!trimmed.is_empty()).then(|| ("agent", trimmed.to_string()))
    } else {
        None
    }
}

fn extract_dsh_step(val: &Value) -> Option<(&'static str, String)> {
    let rtype = val.get("type").and_then(Value::as_str).unwrap_or("");
    let role = val
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| {
            val.get("message")
                .and_then(|m| m.get("role"))
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    if role == "tool"
        || matches!(
            rtype,
            "tool" | "tool_result" | "tool_use" | "function_call" | "function_call_output"
        )
    {
        return None;
    }
    let is_user = role == "user" || matches!(rtype, "user" | "USER_INPUT" | "user_message");
    let is_agent = role == "assistant"
        || role == "model"
        || matches!(
            rtype,
            "assistant" | "agent" | "PLANNER_RESPONSE" | "assistant_message"
        );
    if !is_user && !is_agent {
        return None;
    }
    let text = val
        .get("content")
        .and_then(extract_text_from_value)
        .or_else(|| {
            val.get("message")
                .and_then(|m| m.get("content"))
                .and_then(extract_text_from_value)
        })
        .or_else(|| {
            val.get("message")
                .and_then(|m| m.get("text"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| val.get("text").and_then(Value::as_str).map(str::to_string))
        .or_else(|| {
            val.get("prompt")
                .and_then(Value::as_str)
                .map(str::to_string)
        })?;
    if is_user {
        let unwrapped = strip_delegation_envelope(&text);
        (!unwrapped.is_empty()).then(|| ("user", unwrapped.to_string()))
    } else {
        Some(("agent", text))
    }
}

fn normalize_step_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{collected}…")
    } else {
        collected
    }
}

fn strip_markdown_fence(raw: &str) -> &str {
    let mut cleaned = raw.trim();
    if let Some(after_fence) = cleaned.strip_prefix("```") {
        if let Some(nl) = after_fence.find('\n') {
            cleaned = &after_fence[nl + 1..];
        }
        if let Some(end_fence) = cleaned.rfind("```") {
            cleaned = &cleaned[..end_fence];
        }
    }
    cleaned.trim()
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let cleaned = strip_markdown_fence(raw);
    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}')?;
    (end >= start).then_some(&cleaned[start..=end])
}

fn extract_json_payload(raw: &str) -> Option<&str> {
    let cleaned = strip_markdown_fence(raw);
    let obj_bounds = cleaned.find('{').zip(cleaned.rfind('}'));
    let arr_bounds = cleaned.find('[').zip(cleaned.rfind(']'));
    match (obj_bounds, arr_bounds) {
        (Some((os, oe)), Some((as_, ae))) => {
            if os <= as_ && oe >= os {
                Some(&cleaned[os..=oe])
            } else if ae >= as_ {
                Some(&cleaned[as_..=ae])
            } else {
                None
            }
        }
        (Some((os, oe)), None) if oe >= os => Some(&cleaned[os..=oe]),
        (None, Some((as_, ae))) if ae >= as_ => Some(&cleaned[as_..=ae]),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn slice_config_hash_changes_when_slicer_knobs_change() {
        let mut s = AppSettings::default();
        let h1 = compute_slice_config_hash(&s);
        assert_eq!(h1.len(), 64);

        s.opening_steps = 15;
        let h2 = compute_slice_config_hash(&s);
        assert_ne!(h1, h2);
    }

    #[test]
    fn slice_trajectory_extracts_head_middle_tail_and_skips_sentinel() {
        let mut tmp = NamedTempFile::new().unwrap();
        let long_user = "U".repeat(250);
        let lines = [
            serde_json::json!({"type":"user","message":{"role":"user","content":"Step 1 opening"}}),
            serde_json::json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"bash"},{"type":"text","text":"Assistant open"}]}}),
            serde_json::json!({"type":"user","message":{"role":"user","content":long_user}}),
            serde_json::json!({"type":"assistant","message":{"role":"assistant","content":"Middle agent reply dropped"}}),
            serde_json::json!({"type":"user","message":{"role":"user","content":format!("{INTERNAL_DIGEST_SENTINEL} ignore me")}}),
            serde_json::json!({"type":"assistant","message":{"role":"assistant","content":"Closing response"}}),
        ];
        for line in lines {
            writeln!(tmp, "{line}").unwrap();
        }

        let settings = AppSettings {
            opening_steps: 2,
            closing_steps: 1,
            longest_user_chars: 100,
            longest_agent_chars: 120,
            ..AppSettings::default()
        };

        let sliced = slice_trajectory_file(tmp.path(), "claude", &settings).unwrap();
        assert_eq!(sliced.total_steps, 5);
        assert_eq!(sliced.user_count, 2);
        assert_eq!(sliced.head.len(), 2);
        assert_eq!(sliced.middle_users.len(), 1);
        assert!(sliced.middle_users[0].text.ends_with('…'));
        assert_eq!(sliced.middle_users[0].text.chars().count(), 101);
        assert_eq!(sliced.tail.len(), 1);
        assert_eq!(sliced.tail[0].text, "Closing response");
    }

    #[test]
    fn parse_digest_and_regroup_handle_markdown_fences() {
        let fenced = "```json\n{\"title\":\"Rust API cleanup\",\"stoppedAt\":\"Added processor module\",\"leftOpen\":\"Wire frontend tabs\",\"suggestedLine\":\"Planar Core\",\"isOneOff\":false}\n```";
        let digest = parse_digest_json(fenced).unwrap();
        assert_eq!(digest.title, "Rust API cleanup");
        assert_eq!(digest.suggested_line, "Planar Core");
        assert!(!digest.is_one_off);

        let regroup =
            parse_regroup_json("Here is JSON:\n{\"assignments\":{\"codex:1\":\"Planar Core\"}}")
                .unwrap();
        assert_eq!(
            regroup.get("codex:1").map(String::as_str),
            Some("Planar Core")
        );
    }

    #[tokio::test]
    async fn test_processor_connection_returns_structured_failure_without_api_key() {
        let settings = AppSettings {
            processor_mode: "api".to_string(),
            api_provider: "no-env-test".to_string(),
            api_key: String::new(),
            ..AppSettings::default()
        };
        let res = test_processor_connection(&settings, OsStr::new(""))
            .await
            .unwrap();
        assert!(!res.ok);
        assert!(res.backend_used.starts_with("api:"));
        assert!(res.message.contains("missing API key"));
    }

    #[test]
    fn auto_cli_order_places_claude_last_and_default_requires_explicit_setup() {
        assert_eq!(
            cli_engine_order().last().map(String::as_str),
            Some("claude")
        );
        let defaults = AppSettings::default();
        assert!(!defaults.is_processor_configured());
        assert_eq!(defaults.cli_harness, "");
        assert!(resolve_cli_candidates(&defaults, OsStr::new("")).is_err());

        let ollama_settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "ollama".to_string(),
            api_key: String::new(),
            ..AppSettings::default()
        };
        assert!(ollama_settings.is_processor_configured());
        assert_eq!(
            resolve_api_key("ollama", &ollama_settings).unwrap(),
            String::new()
        );
        assert!(api_provider("openrouter").is_some());
        assert!(api_provider("xai").is_some());
        assert!(api_provider("groq").is_some());
        assert!(api_provider("siliconflow").is_some());
        assert!(api_provider("openai-compatible").unwrap().requires_base_url);
    }

    #[tokio::test]
    async fn openai_compatible_requires_explicit_base_url() {
        let settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "openai-compatible".to_string(),
            api_base_url: String::new(),
            api_key: "test-key".to_string(),
            ..AppSettings::default()
        };
        let client = build_shared_http_client();
        let err = run_api_prompt("ping", &settings, &client)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("API Base URL is required"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn digest_and_regroup_prompts_include_catalog_mass_and_workspace_signals() {
        let catalog = vec![
            LineCatalogEntry {
                id: "planar".to_string(),
                name: "Planar".to_string(),
                description: "Includes desktop shell and timeline UI".to_string(),
                pinned: true,
                session_count: 5,
            },
            LineCatalogEntry {
                id: "search-eval".to_string(),
                name: "search-eval".to_string(),
                description: String::new(),
                pinned: false,
                session_count: 2,
            },
        ];
        let cand = SyncCandidate {
            id: "codex:demo".to_string(),
            harness: "codex".to_string(),
            source_path: "/tmp/demo.jsonl".to_string(),
            step_count: 10,
            user_input_count: 4,
            first_user_prompt: "Refine slicer".to_string(),
            cwd: Some("/Users/demo/vault".to_string()),
            line_id: None,
        };
        let sliced = SlicedTrajectory {
            total_steps: 2,
            user_count: 1,
            head: vec![SlicedStep {
                index: 1,
                role: "user",
                text: "Refine slicer".to_string(),
            }],
            middle_users: Vec::new(),
            tail: Vec::new(),
        };
        let settings = AppSettings::default();

        let digest_prompt = build_digest_prompt(&cand, &sliced, &catalog, &settings);
        assert!(digest_prompt.contains(
            "- Planar [pinned, 5 sessions] — Scope: Includes desktop shell and timeline UI"
        ));
        assert!(digest_prompt.contains("- search-eval [discovered, 2 sessions]"));
        assert!(digest_prompt.contains("Workspace (cwd): /Users/demo/vault"));

        let items = vec![UnpinnedClusterItem {
            session_id: "codex:demo".to_string(),
            title: "Refine slicer".to_string(),
            stopped_at: "Updated slicer config hash".to_string(),
            current_line: "Slicer Tweaks".to_string(),
            cwd: Some("/Users/demo/vault".to_string()),
        }];
        let regroup_prompt = build_regroup_prompt(&items, &catalog, &settings);
        assert!(
            regroup_prompt
                .contains("- Planar (5 sessions) — Scope: Includes desktop shell and timeline UI")
        );
        assert!(regroup_prompt.contains("cwd=\"/Users/demo/vault\""));
    }

    #[test]
    fn codex_and_claude_slicers_filter_synthetic_parts_and_unwrap_delegation_envelopes() {
        let mut codex_tmp = NamedTempFile::new().unwrap();
        let codex_lines = [
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "# AGENTS.md instructions for /tmp/demo\n..."},
                        {"type": "input_text", "text": "<environment_context><cwd>/tmp/demo</cwd></environment_context>"},
                        {"type": "input_text", "text": "<skill>\n<name>demo</name>\n</skill>"},
                        {"type": "input_text", "text": "[cospan delegation context]\n- Workspace: /tmp/demo\n\n[Task]\nAudit SQLite WAL migration"}
                    ]
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Audited migration cleanly."}]
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "[cospan follow-up | mode: readonly]\n\nVerify SHA-256 fixture"}
                    ]
                }
            }),
        ];
        for line in codex_lines {
            writeln!(codex_tmp, "{line}").unwrap();
        }
        let sliced_codex =
            slice_trajectory_file(codex_tmp.path(), "codex", &AppSettings::default()).unwrap();
        assert_eq!(sliced_codex.user_count, 2);
        assert_eq!(sliced_codex.total_steps, 3);
        assert_eq!(sliced_codex.head[0].text, "Audit SQLite WAL migration");
        assert_eq!(sliced_codex.head[2].text, "Verify SHA-256 fixture");

        let mut claude_tmp = NamedTempFile::new().unwrap();
        let claude_lines = [
            serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "content": "ignored giant tool output"},
                        {"type": "text", "text": "<system-reminder>hook note</system-reminder>"},
                        {"type": "text", "text": "Actual human follow-up"}
                    ]
                }
            }),
            serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "content": "pure tool result turn"}
                    ]
                }
            }),
        ];
        for line in claude_lines {
            writeln!(claude_tmp, "{line}").unwrap();
        }
        let sliced_claude =
            slice_trajectory_file(claude_tmp.path(), "claude", &AppSettings::default()).unwrap();
        assert_eq!(sliced_claude.user_count, 1);
        assert_eq!(sliced_claude.head[0].text, "Actual human follow-up");
    }

    #[test]
    fn batch_digest_prompt_and_parser_handle_multi_session_maps_and_default_granularity_4() {
        let settings = AppSettings::default();
        assert_eq!(settings.sync_batch_limit, 4);

        let mk_item = |id: &str, prompt_txt: &str| PreparedSession {
            cand: SyncCandidate {
                id: id.to_string(),
                harness: "codex".to_string(),
                source_path: format!("/tmp/{id}.jsonl"),
                step_count: 6,
                user_input_count: 3,
                first_user_prompt: prompt_txt.to_string(),
                cwd: Some("/Users/demo/planar".to_string()),
                line_id: None,
            },
            sliced: SlicedTrajectory {
                total_steps: 2,
                user_count: 1,
                head: vec![SlicedStep {
                    index: 1,
                    role: "user",
                    text: prompt_txt.to_string(),
                }],
                middle_users: Vec::new(),
                tail: Vec::new(),
            },
            approx_chars: 120,
        };

        let batch = vec![
            mk_item("codex:s1", "Implement batch slicer"),
            mk_item("codex:s2", "Wire NDJSON stream UI"),
        ];
        let prompt = build_batch_digest_prompt(&batch, &[], &settings);
        assert!(prompt.contains("=== SESSION 1 [id=\"codex:s1\"] ==="));
        assert!(prompt.contains("=== SESSION 2 [id=\"codex:s2\"] ==="));

        let raw_reply = "```json\n{\"digests\":{\"codex:s1\":{\"title\":\"Batch Slicer\",\"stoppedAt\":\"Packed 4 sessions per call\",\"leftOpen\":\"\",\"suggestedLine\":\"planar\",\"isOneOff\":false},\"codex:s2\":{\"title\":\"Stream UI Widget\",\"stoppedAt\":\"Updated progress strip\",\"leftOpen\":\"\",\"suggestedLine\":\"planar\",\"isOneOff\":false}}}\n```";
        let expected = vec!["codex:s1".to_string(), "codex:s2".to_string()];
        let parsed = parse_batch_digest_json(raw_reply, &expected).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["codex:s1"].suggested_line, "planar");
        assert_eq!(parsed["codex:s2"].title, "Stream UI Widget");
    }

    #[test]
    fn gemini_request_body_dispatches_thinking_level_and_temperature_by_model() {
        for model in [
            "gemini-4-hydrogen",
            "gemini-4-neon",
            "Models/Gemini-4-Hydrogen",
        ] {
            let body = build_gemini_request_body(model, "summarize");
            let cfg = &body["generationConfig"];
            assert_eq!(cfg["responseMimeType"], "application/json", "{model}");
            assert_eq!(cfg["thinkingConfig"]["thinkingLevel"], "low", "{model}");
            assert!(
                cfg.get("temperature").is_none(),
                "Gemini 4 model `{model}` must omit temperature"
            );
        }

        for model in [
            "gemini-3.5-flash-lite",
            "gemini-3.1-flash-lite",
            "gemini-3-flash-preview",
        ] {
            let body = build_gemini_request_body(model, "summarize");
            let cfg = &body["generationConfig"];
            assert_eq!(cfg["responseMimeType"], "application/json", "{model}");
            assert_eq!(cfg["thinkingConfig"]["thinkingLevel"], "minimal", "{model}");
            assert_eq!(cfg["temperature"], 0.2, "{model}");
        }

        let g36 = build_gemini_request_body("gemini-3.6-flash", "summarize");
        assert_eq!(
            g36["generationConfig"]["responseMimeType"],
            "application/json"
        );
        assert_eq!(
            g36["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "minimal"
        );
        assert!(g36["generationConfig"].get("temperature").is_none());

        let g38 = build_gemini_request_body("gemini-3.8-flash", "summarize");
        assert_eq!(
            g38["generationConfig"]["responseMimeType"],
            "application/json"
        );
        assert_eq!(
            g38["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "low"
        );
        assert!(g38["generationConfig"].get("temperature").is_none());

        for model in ["gemini-3.1-pro-preview", "gemini-3-pro-preview"] {
            let body = build_gemini_request_body(model, "summarize");
            let cfg = &body["generationConfig"];
            assert_eq!(cfg["responseMimeType"], "application/json", "{model}");
            assert_eq!(cfg["thinkingConfig"]["thinkingLevel"], "low", "{model}");
            assert_eq!(cfg["temperature"], 0.2, "{model}");
        }

        for model in [
            "gemini-2.5-flash-lite",
            "gemini-2.5-flash",
            "gemini-2.0-flash",
            "自定义模型-test",
        ] {
            let body = build_gemini_request_body(model, "summarize");
            let cfg = &body["generationConfig"];
            assert_eq!(cfg["responseMimeType"], "application/json", "{model}");
            assert!(
                cfg.get("thinkingConfig").is_none(),
                "legacy model `{model}` must omit thinkingConfig"
            );
            assert_eq!(cfg["temperature"], 0.2, "{model}");
        }
    }

    #[test]
    fn gemini_reply_extraction_ignores_thought_parts_and_endpoint_preserves_override() {
        let payload = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "thought": true, "text": "internal chain of thought" },
                        { "text": "{\"ok\":true}" }
                    ]
                }
            }]
        });
        assert_eq!(
            extract_gemini_reply_text(&payload).unwrap(),
            "{\"ok\":true}"
        );

        let blank_settings = AppSettings {
            api_provider: "gemini".to_string(),
            api_model: String::new(),
            ..AppSettings::default()
        };
        let (_, default_model) = resolve_api_endpoint("gemini", &blank_settings).unwrap();
        assert_eq!(default_model, "gemini-4-hydrogen");

        let override_settings = AppSettings {
            api_provider: "gemini".to_string(),
            api_model: "gemini-2.5-flash-lite".to_string(),
            ..AppSettings::default()
        };
        let (_, saved_model) = resolve_api_endpoint("gemini", &override_settings).unwrap();
        assert_eq!(saved_model, "gemini-2.5-flash-lite");

        let openrouter_settings = AppSettings {
            api_provider: "openrouter".to_string(),
            api_model: String::new(),
            ..AppSettings::default()
        };
        let (_, openrouter_model) =
            resolve_api_endpoint("openrouter", &openrouter_settings).unwrap();
        assert_eq!(openrouter_model, "google/gemini-4-hydrogen");
    }

    #[test]
    fn api_usage_extractors_parse_gemini_anthropic_and_openai_fixtures() {
        // Gemini: promptTokenCount (12_000) includes cachedContentTokenCount (2_000) -> 10_000 uncached.
        // candidatesTokenCount (600) + thoughtsTokenCount (1_400) = 2_000 output tokens.
        // gemini-4-hydrogen: $1.50/M in, $0.075/M cached, $7.50/M out.
        // Cost = 0.01 * 1.50 ($0.015) + 0.002 * 0.075 ($0.00015) + 0.002 * 7.50 ($0.015) = $0.03015.
        let gemini_resp = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "{}"}]}}],
            "usageMetadata": {
                "promptTokenCount": 12_000,
                "cachedContentTokenCount": 2_000,
                "candidatesTokenCount": 600,
                "thoughtsTokenCount": 1_400
            }
        });
        let g = parse_gemini_usage(&gemini_resp, "gemini-4-hydrogen").unwrap();
        assert_eq!(g.input_tokens, 10_000);
        assert_eq!(g.cache_read_tokens, 2_000);
        assert_eq!(g.cache_write_tokens, 0);
        assert_eq!(g.output_tokens, 600);
        assert_eq!(g.thinking_tokens, 1_400);
        assert_eq!(g.total_tokens, 14_000);
        assert!(!g.is_local_cli);
        assert!((g.cost_usd.unwrap() - 0.03015).abs() < 1e-6);

        // Anthropic: input_tokens is uncached; cache_read + cache_creation are additive.
        // claude-sonnet-4-20250514: $3/M in, $0.30/M read, $3.75/M write, $15/M out.
        // Cost = 0.01*3 ($0.03) + 0.1*0.30 ($0.03) + 0.02*3.75 ($0.075) + 0.001*15 ($0.015) = $0.15.
        let anthropic_resp = serde_json::json!({
            "content": [{"type": "text", "text": "{}"}],
            "usage": {
                "input_tokens": 10_000,
                "cache_read_input_tokens": 100_000,
                "cache_creation_input_tokens": 20_000,
                "output_tokens": 1_000
            }
        });
        let a = parse_anthropic_usage(&anthropic_resp, "claude-sonnet-4-20250514").unwrap();
        assert_eq!(a.input_tokens, 10_000);
        assert_eq!(a.cache_read_tokens, 100_000);
        assert_eq!(a.cache_write_tokens, 20_000);
        assert_eq!(a.output_tokens, 1_000);
        assert_eq!(a.total_tokens, 131_000);
        assert!((a.cost_usd.unwrap() - 0.15).abs() < 1e-6);

        // OpenAI-compatible: prompt_tokens (50_000) includes cached_tokens (40_000) -> 10_000 uncached;
        // completion_tokens (2_000) includes reasoning_tokens (1_200) -> 800 visible + 1_200 thinking.
        // gpt-5: $1.25/M in, $0.125/M cached, $10/M out.
        // Cost = 0.01*1.25 ($0.0125) + 0.04*0.125 ($0.005) + 0.002*10 ($0.02) = $0.0375.
        let openai_resp = serde_json::json!({
            "choices": [{"message": {"content": "{}"}}],
            "usage": {
                "prompt_tokens": 50_000,
                "completion_tokens": 2_000,
                "prompt_tokens_details": { "cached_tokens": 40_000 },
                "completion_tokens_details": { "reasoning_tokens": 1_200 }
            }
        });
        let o = parse_openai_compat_usage(&openai_resp, "openai", "gpt-5").unwrap();
        assert_eq!(o.input_tokens, 10_000);
        assert_eq!(o.cache_read_tokens, 40_000);
        assert_eq!(o.output_tokens, 800);
        assert_eq!(o.thinking_tokens, 1_200);
        assert_eq!(o.total_tokens, 52_000);
        assert!((o.cost_usd.unwrap() - 0.0375).abs() < 1e-6);

        // Ollama reports tokens without fabricating cloud API USD spend.
        let ollama = parse_openai_compat_usage(&openai_resp, "ollama", "qwen2.5:7b").unwrap();
        assert_eq!(ollama.total_tokens, 52_000);
        assert_eq!(ollama.cost_usd, None);

        // Batch merging accumulates tokens and USD spend cleanly.
        let mut combined = g.clone();
        combined.merge_from(&o);
        assert_eq!(combined.total_tokens, 66_000);
        assert!((combined.cost_usd.unwrap() - (0.03015 + 0.0375)).abs() < 1e-6);
    }

    #[test]
    fn estimate_sync_cost_handles_ascii_cjk_thinking_models_and_local_cli_offline() {
        // CJK (~1.6 chars/tok) yields significantly more tokens than ASCII (~4 chars/tok) of equal char length.
        let ascii = "a".repeat(1_600);
        let cjk = "测".repeat(1_600);
        assert_eq!(estimate_text_tokens(&ascii), 400);
        assert_eq!(estimate_text_tokens(&cjk), 1_000);

        let mk_session = |id: &str, text: &str| PreparedSession {
            cand: SyncCandidate {
                id: id.to_string(),
                harness: "codex".to_string(),
                source_path: format!("/tmp/{id}.jsonl"),
                step_count: 8,
                user_input_count: 4,
                first_user_prompt: text.to_string(),
                cwd: Some("/Users/demo/planar".to_string()),
                line_id: None,
            },
            sliced: SlicedTrajectory {
                total_steps: 4,
                user_count: 2,
                head: vec![SlicedStep {
                    index: 1,
                    role: "user",
                    text: text.to_string(),
                }],
                middle_users: Vec::new(),
                tail: vec![SlicedStep {
                    index: 4,
                    role: "agent",
                    text: text.to_string(),
                }],
            },
            approx_chars: text.len() * 2 + 240,
        };

        let ascii_batch = vec![vec![mk_session("codex:ascii", &ascii)]];
        let cjk_batch = vec![vec![mk_session("codex:cjk", &cjk)]];

        let gemini_settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "gemini".to_string(),
            api_model: "gemini-4-hydrogen".to_string(),
            ..AppSettings::default()
        };
        let ascii_est = estimate_prepared_batches_cost(&ascii_batch, &[], &gemini_settings);
        let cjk_est = estimate_prepared_batches_cost(&cjk_batch, &[], &gemini_settings);
        assert_eq!(ascii_est.session_count, 1);
        assert_eq!(ascii_est.batch_count, 1);
        assert!(!ascii_est.is_local_cli);
        assert!(ascii_est.estimated_thinking_tokens > 0);
        assert!(cjk_est.estimated_input_tokens > ascii_est.estimated_input_tokens);
        let range = cjk_est.estimated_cost_usd_range.unwrap();
        assert!(range.low > 0.0 && range.high >= range.low);

        // Non-thinking model (`gemini-3.5-flash-lite`) estimates 0 thinking tokens and lower cost.
        let lite_settings = AppSettings {
            api_model: "gemini-3.5-flash-lite".to_string(),
            ..gemini_settings
        };
        let lite_est = estimate_prepared_batches_cost(&cjk_batch, &[], &lite_settings);
        assert_eq!(lite_est.estimated_thinking_tokens, 0);
        assert!(
            lite_est.estimated_cost_usd_range.unwrap().high
                < cjk_est.estimated_cost_usd_range.unwrap().low
        );

        // Local CLI mode reports tokens/batches honestly with `is_local_cli = true` and no USD range.
        let cli_settings = AppSettings {
            processor_configured: true,
            processor_mode: "cli".to_string(),
            cli_harness: "codex".to_string(),
            ..AppSettings::default()
        };
        let cli_est = estimate_prepared_batches_cost(&ascii_batch, &[], &cli_settings);
        assert!(cli_est.is_local_cli);
        assert_eq!(cli_est.estimated_cost_usd_range, None);
        assert_eq!(cli_est.backend, "cli:codex");

        // Generic `openai-compatible` gateway using `gpt-4o-mini` retains token estimates
        // without fabricating OpenAI USD spend.
        let compat_settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "openai-compatible".to_string(),
            api_base_url: "http://127.0.0.1:8000/v1".to_string(),
            api_model: "gpt-4o-mini".to_string(),
            ..AppSettings::default()
        };
        let compat_est = estimate_prepared_batches_cost(&ascii_batch, &[], &compat_settings);
        assert!(!compat_est.is_local_cli);
        assert!(compat_est.estimated_input_tokens > 0);
        assert_eq!(compat_est.estimated_cost_usd_range, None);
    }

    #[test]
    fn estimate_sync_cost_prices_multi_batch_runs_per_batch_without_crossing_aggregate_tier() {
        // 20 single-session batches of ~15K input tokens each (~300K aggregate) on `gemini-2.5-pro`
        // (whose >200K single-request tier doubles input from $1.25/M to $2.50/M and output from $10/M to $15/M).
        // Each individual batch is ~15K input (< 200K), so total cost MUST stay at base tier (20 * single-batch).
        let text = "x".repeat(29_000); // ~14.5K input tokens across head + tail
        let mk_batch = |idx: usize| {
            vec![PreparedSession {
                cand: SyncCandidate {
                    id: format!("codex:b{idx}"),
                    harness: "codex".to_string(),
                    source_path: format!("/tmp/b{idx}.jsonl"),
                    step_count: 4,
                    user_input_count: 3,
                    first_user_prompt: "Analyze batch".to_string(),
                    cwd: Some("/Users/demo/planar".to_string()),
                    line_id: None,
                },
                sliced: SlicedTrajectory {
                    total_steps: 2,
                    user_count: 1,
                    head: vec![SlicedStep {
                        index: 1,
                        role: "user",
                        text: text.clone(),
                    }],
                    middle_users: Vec::new(),
                    tail: vec![SlicedStep {
                        index: 2,
                        role: "agent",
                        text: text.clone(),
                    }],
                },
                approx_chars: text.len() * 2 + 240,
            }]
        };
        let settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "gemini".to_string(),
            api_model: "gemini-2.5-pro".to_string(),
            sync_batch_limit: 1,
            ..AppSettings::default()
        };

        let one_batch = vec![mk_batch(0)];
        let twenty_batches: Vec<_> = (0..20).map(mk_batch).collect();

        let single_est = estimate_prepared_batches_cost(&one_batch, &[], &settings);
        let multi_est = estimate_prepared_batches_cost(&twenty_batches, &[], &settings);
        assert_eq!(multi_est.batch_count, 20);
        assert!(
            multi_est.estimated_input_tokens > 250_000,
            "aggregate tokens should exceed 200K tier threshold: {}",
            multi_est.estimated_input_tokens
        );

        let r1 = single_est.estimated_cost_usd_range.unwrap();
        let r20 = multi_est.estimated_cost_usd_range.unwrap();
        assert!(
            (r20.low - r1.low * 20.0).abs() < 1e-6,
            "20-batch low cost ({}) should equal 20 * 1-batch low ({})",
            r20.low,
            r1.low * 20.0
        );
        assert!(
            (r20.high - r1.high * 20.0).abs() < 1e-6,
            "20-batch high cost ({}) should equal 20 * 1-batch high ({})",
            r20.high,
            r1.high * 20.0
        );
    }

    #[tokio::test]
    async fn sync_preserves_empty_response_usage_and_marks_incomplete_coverage() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        use axum::routing::post;
        use axum::{Json, Router};
        use tempfile::TempDir;

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_ref = calls.clone();
        let mock_api = Router::new().route(
            "/models/{model_action}",
            post(move || {
                let c = calls_ref.clone();
                async move {
                    match c.fetch_add(1, Ordering::SeqCst) {
                        // Call 1: batch returns digest for s1 only (omits s2 & s3) with priced usageMetadata.
                        // gemini-4-hydrogen: 10K in ($0.015) + 1K out ($0.0075) = $0.0225.
                        0 => Json(serde_json::json!({
                            "candidates": [{"content": {"parts": [{"text": "{\"digests\":{\"codex:s1\":{\"title\":\"Session 1\",\"stoppedAt\":\"Done 1\",\"leftOpen\":\"\",\"suggestedLine\":\"planar\",\"isOneOff\":false}}}"}]}}],
                            "usageMetadata": {
                                "promptTokenCount": 10_000,
                                "candidatesTokenCount": 1_000
                            }
                        })),
                        // Call 2 (fallback s2): empty content BUT still reports billable usageMetadata!
                        // gemini-4-hydrogen: 5K in ($0.0075) + 2K thinking ($0.015) = $0.0225.
                        1 => Json(serde_json::json!({
                            "candidates": [{"content": {"parts": [{"text": ""}]}}],
                            "usageMetadata": {
                                "promptTokenCount": 5_000,
                                "candidatesTokenCount": 0,
                                "thoughtsTokenCount": 2_000
                            }
                        })),
                        // Call 3 (fallback s3): valid digest WITHOUT usageMetadata.
                        _ => Json(serde_json::json!({
                            "candidates": [{"content": {"parts": [{"text": "{\"title\":\"Session 3\",\"stoppedAt\":\"Done 3\",\"leftOpen\":\"\",\"suggestedLine\":\"planar\",\"isOneOff\":false}"}]}}]
                        })),
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let dir = TempDir::new().unwrap();
        let roots = crate::env::HarnessRoots {
            claude_projects: dir.path().join("claude"),
            codex_sessions: dir.path().join("codex").join("sessions"),
            codex_archived: dir.path().join("codex").join("archived_sessions"),
            antigravity_brain: dir.path().join("antigravity"),
            dsh_sessions: dir.path().join("dsh").join("sessions"),
        };
        let sess_dir = roots.codex_sessions.join("2026").join("11").join("25");
        fs::create_dir_all(&sess_dir).unwrap();
        let now = Local::now();
        let t1 = (now - chrono::Duration::minutes(15)).to_rfc3339();
        let t2 = (now - chrono::Duration::minutes(12)).to_rfc3339();
        let t3 = (now - chrono::Duration::minutes(9)).to_rfc3339();
        for idx in 1..=3 {
            fs::write(
                sess_dir.join(format!("rollout-{idx}.jsonl")),
                format!(
                    concat!(
                        "{{\"timestamp\":\"{t1}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"s{idx}\",\"cwd\":\"/tmp/ws\",\"source\":\"vscode\"}}}}\n",
                        "{{\"timestamp\":\"{t1}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 1 s{idx}\"}}]}}}}\n",
                        "{{\"timestamp\":\"{t2}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 2 s{idx}\"}}]}}}}\n",
                        "{{\"timestamp\":\"{t3}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 3 s{idx}\"}}]}}}}\n"
                    ),
                    t1 = t1,
                    t2 = t2,
                    t3 = t3,
                    idx = idx,
                ),
            )
            .unwrap();
        }

        let db_path = dir.path().join("planar.db");
        {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            let settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "gemini".to_string(),
                api_base_url: format!("http://{addr}"),
                api_model: "gemini-4-hydrogen".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 4,
                ..AppSettings::default()
            };
            store.save_settings(&settings).unwrap();
        }

        let state = AppState::new(db_path, roots);
        let summary = TEST_BILLING_PROVIDER_OVERRIDE
            .scope(Some("google"), async {
                sync_pending_sessions_in_scope(&state, false, None, None).await
            })
            .await
            .unwrap();
        server_task.abort();

        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(summary.processed, 2);
        assert_eq!(summary.failed, 1);
        let usage = summary.usage.expect("expected accumulated usage");
        assert_eq!(usage.api_calls, 3);
        assert_eq!(usage.priced_calls, 2);
        assert!(
            usage.incomplete,
            "missing usage on call 3 must mark run incomplete"
        );
        assert_eq!(usage.input_tokens, 15_000);
        assert_eq!(usage.output_tokens, 1_000);
        assert_eq!(usage.thinking_tokens, 2_000);
        assert_eq!(usage.total_tokens, 18_000);
        assert!(
            (usage.cost_usd.unwrap() - 0.045).abs() < 1e-6,
            "expected combined cost $0.045 from call 1 + empty-content call 2, got {:?}",
            usage.cost_usd
        );
    }

    #[tokio::test]
    async fn sync_marks_malformed_json_response_incomplete_and_keeps_pre_dispatch_errors_separate()
    {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        use axum::Router;
        use axum::http::{StatusCode, header};
        use axum::response::IntoResponse;
        use axum::routing::post;
        use tempfile::TempDir;

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_ref = calls.clone();
        let mock_api = Router::new().route(
            "/models/{model_action}",
            post(move || {
                let c = calls_ref.clone();
                async move {
                    match c.fetch_add(1, Ordering::SeqCst) {
                        // Batch 1: valid JSON + priced usageMetadata ($0.0225).
                        0 => (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            r#"{"candidates":[{"content":{"parts":[{"text":"{\"title\":\"Batch 1\",\"stoppedAt\":\"Done\",\"leftOpen\":\"\",\"suggestedLine\":\"planar\",\"isOneOff\":false}"}]}}],"usageMetadata":{"promptTokenCount":10000,"candidatesTokenCount":1000}}"#,
                        )
                            .into_response(),
                        // Batch 2: HTTP 200 with truncated / malformed JSON body (post-dispatch failure).
                        _ => (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            r#"{"candidates":[{"content":{"parts":[{"text":"{\"title\":\"Trun"#,
                        )
                            .into_response(),
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let dir = TempDir::new().unwrap();
        let roots = crate::env::HarnessRoots {
            claude_projects: dir.path().join("claude"),
            codex_sessions: dir.path().join("codex").join("sessions"),
            codex_archived: dir.path().join("codex").join("archived_sessions"),
            antigravity_brain: dir.path().join("antigravity"),
            dsh_sessions: dir.path().join("dsh").join("sessions"),
        };
        let sess_dir = roots.codex_sessions.join("2026").join("11").join("25");
        fs::create_dir_all(&sess_dir).unwrap();
        let now = Local::now();
        let t1 = (now - chrono::Duration::minutes(15)).to_rfc3339();
        let t2 = (now - chrono::Duration::minutes(12)).to_rfc3339();
        let t3 = (now - chrono::Duration::minutes(9)).to_rfc3339();
        for idx in 1..=2 {
            fs::write(
                sess_dir.join(format!("rollout-m{idx}.jsonl")),
                format!(
                    concat!(
                        "{{\"timestamp\":\"{t1}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"m{idx}\",\"cwd\":\"/tmp/ws\",\"source\":\"vscode\"}}}}\n",
                        "{{\"timestamp\":\"{t1}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 1 m{idx}\"}}]}}}}\n",
                        "{{\"timestamp\":\"{t2}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 2 m{idx}\"}}]}}}}\n",
                        "{{\"timestamp\":\"{t3}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 3 m{idx}\"}}]}}}}\n"
                    ),
                    t1 = t1,
                    t2 = t2,
                    t3 = t3,
                    idx = idx,
                ),
            )
            .unwrap();
        }

        let db_path = dir.path().join("planar.db");
        {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            let settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "gemini".to_string(),
                api_base_url: format!("http://{addr}"),
                api_model: "gemini-4-hydrogen".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 1,
                ..AppSettings::default()
            };
            store.save_settings(&settings).unwrap();
        }

        let state = AppState::new(db_path.clone(), roots.clone());
        let summary = TEST_BILLING_PROVIDER_OVERRIDE
            .scope(Some("google"), async {
                sync_pending_sessions_in_scope(&state, false, Some(2), None).await
            })
            .await
            .unwrap();
        server_task.abort();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(summary.processed, 1);
        assert_eq!(summary.failed, 1);
        let usage = summary.usage.expect("expected incomplete usage summary");
        assert_eq!(usage.api_calls, 2);
        assert_eq!(usage.priced_calls, 1);
        assert!(
            usage.incomplete,
            "truncated/malformed JSON on batch 2 must mark run incomplete"
        );
        assert_eq!(usage.total_tokens, 11_000);
        assert!((usage.cost_usd.unwrap() - 0.0225).abs() < 1e-6);

        // Pre-dispatch configuration error (missing Base URL on openai-compatible) stays a hard
        // failure with `usage == None` and never fabricates an API call count.
        {
            let store = crate::db::Store::open(&db_path).unwrap();
            let bad_settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "openai-compatible".to_string(),
                api_base_url: String::new(),
                api_model: "gpt-4o-mini".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 1,
                ..AppSettings::default()
            };
            store.save_settings(&bad_settings).unwrap();
        }
        let pre_dispatch_summary = sync_pending_sessions_in_scope(&state, false, Some(1), None)
            .await
            .unwrap();
        assert_eq!(pre_dispatch_summary.processed, 0);
        assert_eq!(pre_dispatch_summary.failed, 1);
        assert_eq!(
            pre_dispatch_summary.usage, None,
            "pre-dispatch configuration error must not fabricate an API usage record"
        );
    }

    #[tokio::test]
    async fn named_provider_with_overridden_local_base_url_suppresses_vendor_usd_while_retaining_tokens()
     {
        use axum::routing::post;
        use axum::{Json, Router};
        use tempfile::TempDir;

        // 1. Unit check on `resolve_billing_provider` and `estimate_prepared_batches_cost`:
        assert_eq!(
            resolve_billing_provider("openai", "https://api.openai.com/v1"),
            Some("openai")
        );
        assert_eq!(
            resolve_billing_provider("openai", "http://127.0.0.1:8080/v1"),
            None
        );
        assert_eq!(
            resolve_billing_provider("gemini", "https://my-local-proxy.example/v1beta"),
            None
        );
        assert_eq!(
            resolve_billing_provider("gemini", "https://generativelanguage.googleapis.com/v1beta"),
            Some("google")
        );

        let sample = vec![vec![PreparedSession {
            cand: SyncCandidate {
                id: "codex:local-override".to_string(),
                harness: "codex".to_string(),
                source_path: "/tmp/local-override.jsonl".to_string(),
                step_count: 4,
                user_input_count: 3,
                first_user_prompt: "Test local override".to_string(),
                cwd: Some("/Users/demo/planar".to_string()),
                line_id: None,
            },
            sliced: SlicedTrajectory {
                total_steps: 2,
                user_count: 1,
                head: vec![SlicedStep {
                    index: 1,
                    role: "user",
                    text: "Test local override".to_string(),
                }],
                middle_users: Vec::new(),
                tail: Vec::new(),
            },
            approx_chars: 280,
        }]];
        let local_override_settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "openai".to_string(),
            api_base_url: "http://127.0.0.1:8080/v1".to_string(),
            api_model: "gpt-4o-mini".to_string(),
            ..AppSettings::default()
        };
        let est = estimate_prepared_batches_cost(&sample, &[], &local_override_settings);
        assert!(est.estimated_input_tokens > 0);
        assert_eq!(
            est.estimated_cost_usd_range, None,
            "named provider with overridden local Base URL must suppress vendor USD estimate"
        );

        // 2. Live Sync call to overridden localhost URL using named provider `"openai"` + `"gpt-4o-mini"`:
        // reported tokens MUST be preserved while `cost_usd` stays `None` and `incomplete` is `true`.
        let mock_api = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "content": "{\"title\":\"Local Proxy\",\"stoppedAt\":\"Done\",\"leftOpen\":\"\",\"suggestedLine\":\"planar\",\"isOneOff\":false}"
                        }
                    }],
                    "usage": {
                        "prompt_tokens": 4_000,
                        "completion_tokens": 500
                    }
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let dir = TempDir::new().unwrap();
        let roots = crate::env::HarnessRoots {
            claude_projects: dir.path().join("claude"),
            codex_sessions: dir.path().join("codex").join("sessions"),
            codex_archived: dir.path().join("codex").join("archived_sessions"),
            antigravity_brain: dir.path().join("antigravity"),
            dsh_sessions: dir.path().join("dsh").join("sessions"),
        };
        let sess_dir = roots.codex_sessions.join("2026").join("11").join("25");
        fs::create_dir_all(&sess_dir).unwrap();
        let now = Local::now();
        let t1 = (now - chrono::Duration::minutes(10)).to_rfc3339();
        let t2 = (now - chrono::Duration::minutes(8)).to_rfc3339();
        let t3 = (now - chrono::Duration::minutes(5)).to_rfc3339();
        fs::write(
            sess_dir.join("rollout-local.jsonl"),
            format!(
                concat!(
                    "{{\"timestamp\":\"{t1}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"local-1\",\"cwd\":\"/tmp/ws\",\"source\":\"vscode\"}}}}\n",
                    "{{\"timestamp\":\"{t1}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 1\"}}]}}}}\n",
                    "{{\"timestamp\":\"{t2}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 2\"}}]}}}}\n",
                    "{{\"timestamp\":\"{t3}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 3\"}}]}}}}\n"
                ),
                t1 = t1,
                t2 = t2,
                t3 = t3,
            ),
        )
        .unwrap();

        let db_path = dir.path().join("planar.db");
        {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            let settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "openai".to_string(),
                api_base_url: format!("http://{addr}"),
                api_model: "gpt-4o-mini".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 4,
                ..AppSettings::default()
            };
            store.save_settings(&settings).unwrap();
        }

        let state = AppState::new(db_path, roots);
        let summary = sync_pending_sessions_in_scope(&state, false, None, None)
            .await
            .unwrap();
        server_task.abort();

        assert_eq!(summary.processed, 1);
        let usage = summary.usage.expect("expected token usage to be retained");
        assert_eq!(usage.input_tokens, 4_000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.total_tokens, 4_500);
        assert_eq!(
            usage.cost_usd, None,
            "overridden local endpoint must not inherit OpenAI USD pricing"
        );
        assert_eq!(usage.api_calls, 1);
        assert_eq!(usage.priced_calls, 0);
        assert!(usage.incomplete);
    }

    fn slice_trajectory_file_baseline(
        source_path: &Path,
        harness: &str,
        settings: &AppSettings,
    ) -> Result<SlicedTrajectory> {
        let mut raw_turns: Vec<(usize, &'static str, String)> = Vec::new();
        let mut user_count = 0_usize;
        let user_limit = usize::try_from(settings.longest_user_chars)
            .unwrap_or(usize::MAX)
            .max(SLICER_CHAR_BUDGET_MIN);
        let agent_limit = usize::try_from(settings.longest_agent_chars)
            .unwrap_or(usize::MAX)
            .max(SLICER_CHAR_BUDGET_MIN);

        stream_jsonl_lines(source_path, 0, |line| {
            let Ok(val) = serde_json::from_str::<Value>(line) else {
                return Ok(());
            };
            let Some((role, text)) = extract_step_text(harness, &val) else {
                return Ok(());
            };
            if text.contains(INTERNAL_DIGEST_SENTINEL) {
                return Ok(());
            }
            let cleaned = normalize_step_text(&text);
            if cleaned.is_empty() {
                return Ok(());
            }
            let max_len = if role == "user" {
                user_count += 1;
                user_limit
            } else {
                agent_limit
            };
            let idx = raw_turns.len() + 1;
            raw_turns.push((idx, role, truncate_chars(&cleaned, max_len)));
            Ok(())
        })?;

        let total_steps = raw_turns.len();
        let open_n = usize::try_from(settings.opening_steps).unwrap_or(usize::MAX);
        let close_n = usize::try_from(settings.closing_steps).unwrap_or(usize::MAX);
        let head_end = open_n.min(total_steps);
        let tail_start = total_steps.saturating_sub(close_n).max(head_end);

        let to_step = |(index, role, text): &(usize, &'static str, String)| SlicedStep {
            index: *index,
            role,
            text: text.clone(),
        };

        let head = raw_turns[..head_end].iter().map(to_step).collect();
        let middle_users = raw_turns[head_end..tail_start]
            .iter()
            .filter(|(_, role, _)| *role == "user")
            .map(to_step)
            .collect();
        let tail = raw_turns[tail_start..].iter().map(to_step).collect();

        Ok(SlicedTrajectory {
            total_steps,
            user_count,
            head,
            middle_users,
            tail,
        })
    }

    #[test]
    fn fast_skipped_tool_line_slicing_matches_baseline_across_all_four_harnesses() {
        let settings = AppSettings {
            opening_steps: 2,
            closing_steps: 2,
            longest_user_chars: 140,
            longest_agent_chars: 160,
            ..AppSettings::default()
        };
        let huge_payload = format!(
            "{} \"user\" \"assistant\" \"USER_INPUT\"",
            "X".repeat(8_192)
        );

        // 1. Claude fixture: progress/snapshot, huge tool_result, mixed tool_use + text, unicode escape,
        //    sentinel, malformed line, top-level array, non-object message, array/primitive blocks in content,
        //    and unexpected field types.
        let mut claude_tmp = NamedTempFile::new().unwrap();
        let claude_lines = [
            serde_json::json!({"type":"progress","data":{"msg":"ignored progress"}}).to_string(),
            serde_json::json!({"type":"file-history-snapshot","snapshot":{"files":[]}}).to_string(),
            serde_json::json!(["user", false, false, {"content": "fake array record"}]).to_string(),
            serde_json::json!({"type":"user","message":["fake array message"]}).to_string(),
            serde_json::json!({"type":"assistant","message":{"content":["fake-single-string-array"]}}).to_string(),
            serde_json::json!({"type":"user","message":{"role":"user","content":[["text","fake-array-block"], "bare-string", 42, {"type":"text","text":123}]}}).to_string(),
            serde_json::json!({"type":"assistant","message":{"role":"assistant","content":[["text","fake-array-block"], "bare-string", {"type":"text","text":"Mixed array valid reply"}]}}).to_string(),
            serde_json::json!({"type":"user","isMeta":true,"message":{"role":"user","content":"meta skipped"}}).to_string(),
            serde_json::json!({"type":"user","isMeta":"not-a-bool","message":{"role":"user","content":"Non-bool isMeta falls back cleanly"}}).to_string(),
            serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":huge_payload}]}}).to_string(),
            serde_json::json!({"type":"user","message":{"role":"user","content":"Claude turn 1"}}).to_string(),
            serde_json::json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":huge_payload},{"type":"tool_use","name":"bash","input":{"cmd":"ls"}},{"type":"text","text":"Claude reply 1"}]}}).to_string(),
            serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"big"},{"type":"text","text":"<system-reminder>skip</system-reminder>"},{"type":"text","text":"Claude turn 2"}]}}).to_string(),
            r#"{"type":"\u0075ser","message":{"role":"user","content":"Unicode escaped user turn"}}"#.to_string(),
            serde_json::json!({"type":"user","message":{"role":"user","content":format!("{INTERNAL_DIGEST_SENTINEL} echo")}}).to_string(),
            serde_json::json!({"type":"assistant","message":{"role":"assistant","content":{"text":"Object text reply"}}}).to_string(),
            "{broken json line".to_string(),
        ];
        for l in &claude_lines {
            writeln!(claude_tmp, "{l}").unwrap();
        }
        let fast_claude = slice_trajectory_file(claude_tmp.path(), "claude", &settings).unwrap();
        let base_claude =
            slice_trajectory_file_baseline(claude_tmp.path(), "claude", &settings).unwrap();
        assert_eq!(fast_claude, base_claude);
        assert_eq!(fast_claude.user_count, 4);
        assert_eq!(fast_claude.total_steps, 7);

        // 2. Codex fixture: session_meta, function_call_output with "user"/"message" inside, reasoning,
        //    top-level array, non-object payload, and real messages.
        let mut codex_tmp = NamedTempFile::new().unwrap();
        let codex_lines = [
            serde_json::json!({"type":"session_meta","payload":{"id":"c1","cwd":"/tmp/a"}}).to_string(),
            serde_json::json!(["response_item", ["message", "user", "fake array record"]]).to_string(),
            serde_json::json!({"type":"response_item","payload":["message","user","fake array payload"]}).to_string(),
            serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_tokens":999}}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"1","output":huge_payload}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"text":"thinking"}]}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Codex turn 1"}]}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Codex reply 1"}]}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":"Codex string turn 2"}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"developer","content":"ignored developer"}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"tool_call","text":"skip"},{"type":"output_text","text":"Codex reply 2"}]}}).to_string(),
        ];
        for l in &codex_lines {
            writeln!(codex_tmp, "{l}").unwrap();
        }
        let fast_codex = slice_trajectory_file(codex_tmp.path(), "codex", &settings).unwrap();
        let base_codex =
            slice_trajectory_file_baseline(codex_tmp.path(), "codex", &settings).unwrap();
        assert_eq!(fast_codex, base_codex);
        assert_eq!(fast_codex.user_count, 2);
        assert_eq!(fast_codex.total_steps, 4);

        // 3. Antigravity fixture: TOOL_CALL / TOOL_RESULT with huge payload, top-level array + USER_INPUT / PLANNER_RESPONSE
        let mut ag_tmp = NamedTempFile::new().unwrap();
        let ag_lines = [
            serde_json::json!(["USER_INPUT", "fake array record"]).to_string(),
            serde_json::json!(["PLANNER_RESPONSE", "fake array reply"]).to_string(),
            serde_json::json!({"type":"TOOL_CALL","content":"run_command"}).to_string(),
            serde_json::json!({"type":"TOOL_RESULT","content":huge_payload}).to_string(),
            serde_json::json!({"type":"USER_INPUT","content":"<USER_REQUEST>AG prompt 1</USER_REQUEST>"}).to_string(),
            serde_json::json!({"type":"PLANNER_RESPONSE","content":"AG reply 1"}).to_string(),
            serde_json::json!({"type":"THINKING","content":"internal thought"}).to_string(),
            serde_json::json!({"type":"USER_INPUT","content":"AG prompt 2"}).to_string(),
            serde_json::json!({"type":"PLANNER_RESPONSE","content":"AG reply 2"}).to_string(),
        ];
        for l in &ag_lines {
            writeln!(ag_tmp, "{l}").unwrap();
        }
        let fast_ag = slice_trajectory_file(ag_tmp.path(), "antigravity", &settings).unwrap();
        let base_ag =
            slice_trajectory_file_baseline(ag_tmp.path(), "antigravity", &settings).unwrap();
        assert_eq!(fast_ag, base_ag);
        assert_eq!(fast_ag.user_count, 2);
        assert_eq!(fast_ag.total_steps, 4);

        // 4. DSH fixture: role=tool with "user" inside content, top-level array, non-object message,
        //    tool_result, user_message, model, assistant.
        let mut dsh_tmp = NamedTempFile::new().unwrap();
        let dsh_lines = [
            serde_json::json!(["user", "user", ["user"]]).to_string(),
            serde_json::json!({"type":"other","message":["user"]}).to_string(),
            serde_json::json!({"type":"session_meta","session_id":"d1"}).to_string(),
            serde_json::json!({"type":"tool_result","role":"tool","content":huge_payload}).to_string(),
            serde_json::json!({"type":"function_call_output","content":"ignored output"}).to_string(),
            serde_json::json!({"role":"user","content":"DSH user 1"}).to_string(),
            serde_json::json!({"role":"assistant","content":[{"type":"tool_use","name":"read"},{"type":"text","text":"DSH agent 1"}]}).to_string(),
            serde_json::json!({"type":"user_message","message":{"role":"user","text":"DSH user 2"}}).to_string(),
            serde_json::json!({"role":"model","text":"DSH model 2"}).to_string(),
        ];
        for l in &dsh_lines {
            writeln!(dsh_tmp, "{l}").unwrap();
        }
        let fast_dsh = slice_trajectory_file(dsh_tmp.path(), "dsh", &settings).unwrap();
        let base_dsh = slice_trajectory_file_baseline(dsh_tmp.path(), "dsh", &settings).unwrap();
        assert_eq!(fast_dsh, base_dsh);
        assert_eq!(fast_dsh.user_count, 2);
        assert_eq!(fast_dsh.total_steps, 4);
    }

    #[test]
    fn cwd_affinity_ordering_packs_same_workspace_and_preserves_first_seen_priority() {
        let mk = |id: &str, cwd: Option<&str>| PreparedSession {
            cand: SyncCandidate {
                id: id.to_string(),
                harness: "codex".to_string(),
                source_path: format!("/tmp/{id}.jsonl"),
                step_count: 6,
                user_input_count: 3,
                first_user_prompt: id.to_string(),
                cwd: cwd.map(str::to_string),
                line_id: None,
            },
            sliced: SlicedTrajectory {
                total_steps: 2,
                user_count: 1,
                head: Vec::new(),
                middle_users: Vec::new(),
                tail: Vec::new(),
            },
            approx_chars: 256,
        };

        let items = vec![
            mk("a1", Some("/work/alpha")),
            mk("b1", Some("/work/beta")),
            mk("a2", Some("/work/alpha/")), // trailing slash normalizes to `/work/alpha`
            mk("n1", None),
            mk("b2", Some("/work/beta")),
            mk("a3", Some("/work/alpha")),
            mk("n2", Some("   ")),
        ];

        let ordered = order_prepared_by_cwd_affinity(items);
        let ids: Vec<&str> = ordered.iter().map(|p| p.cand.id.as_str()).collect();
        assert_eq!(ids, vec!["a1", "a2", "a3", "b1", "b2", "n1", "n2"]);

        let batches = chunk_prepared_into_batches(ordered, 3);
        assert_eq!(batches.len(), 3);
        let b0: Vec<&str> = batches[0].iter().map(|p| p.cand.id.as_str()).collect();
        let b1: Vec<&str> = batches[1].iter().map(|p| p.cand.id.as_str()).collect();
        assert_eq!(b0, vec!["a1", "a2", "a3"]);
        assert_eq!(b1, vec!["b1", "b2", "n1"]);
    }

    #[test]
    fn effective_sync_concurrency_bounds_api_and_caps_local_cli() {
        let mut s = AppSettings::default();
        assert_eq!(s.sync_concurrency, 4);

        s.processor_mode = "api".to_string();
        assert_eq!(effective_sync_concurrency(&s), 4);
        s.sync_concurrency = 99;
        assert_eq!(effective_sync_concurrency(&s), SYNC_CONCURRENCY_MAX);
        s.sync_concurrency = 0;
        assert_eq!(effective_sync_concurrency(&s), SYNC_CONCURRENCY_MIN);

        s.processor_mode = "cli".to_string();
        s.sync_concurrency = 4;
        assert_eq!(effective_sync_concurrency(&s), SYNC_CLI_CONCURRENCY_CAP);
        s.sync_concurrency = 1;
        assert_eq!(effective_sync_concurrency(&s), 1);
    }

    fn seed_codex_test_sessions(
        dir: &Path,
        specs: &[(&str, &str)],
    ) -> (PathBuf, crate::env::HarnessRoots) {
        let roots = crate::env::HarnessRoots {
            claude_projects: dir.join("claude"),
            codex_sessions: dir.join("codex").join("sessions"),
            codex_archived: dir.join("codex").join("archived_sessions"),
            antigravity_brain: dir.join("antigravity"),
            dsh_sessions: dir.join("dsh").join("sessions"),
        };
        let sess_dir = roots.codex_sessions.join("2026").join("11").join("25");
        fs::create_dir_all(&sess_dir).unwrap();
        let now = Local::now();
        for (idx, (sid, cwd)) in specs.iter().enumerate() {
            let offset = i64::try_from(idx + 1).unwrap_or(1) * 5;
            let t1 = (now - chrono::Duration::minutes(offset + 3)).to_rfc3339();
            let t2 = (now - chrono::Duration::minutes(offset + 2)).to_rfc3339();
            let t3 = (now - chrono::Duration::minutes(offset + 1)).to_rfc3339();
            fs::write(
                sess_dir.join(format!("rollout-{sid}.jsonl")),
                format!(
                    concat!(
                        "{{\"timestamp\":\"{t1}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{sid}\",\"cwd\":\"{cwd}\",\"source\":\"vscode\"}}}}\n",
                        "{{\"timestamp\":\"{t1}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 1 {sid}\"}}]}}}}\n",
                        "{{\"timestamp\":\"{t2}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 2 {sid}\"}}]}}}}\n",
                        "{{\"timestamp\":\"{t3}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Turn 3 {sid}\"}}]}}}}\n"
                    ),
                    t1 = t1,
                    t2 = t2,
                    t3 = t3,
                    sid = sid,
                    cwd = cwd,
                ),
            )
            .unwrap();
        }
        (dir.join("planar.db"), roots)
    }

    #[tokio::test]
    async fn concurrent_sync_respects_semaphore_bound_preserves_pinned_lines_and_streams_lightweight_batches()
     {
        use std::sync::atomic::AtomicUsize;

        use axum::routing::post;
        use axum::{Json, Router};
        use tempfile::TempDir;

        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak_in_flight = Arc::new(AtomicUsize::new(0));
        let in_flight_ref = in_flight.clone();
        let peak_ref = peak_in_flight.clone();

        let mock_api = Router::new().route(
            "/chat/completions",
            post(move || {
                let cur_arc = in_flight_ref.clone();
                let pk_arc = peak_ref.clone();
                async move {
                    let cur = cur_arc.fetch_add(1, Ordering::SeqCst) + 1;
                    pk_arc.fetch_max(cur, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(60)).await;
                    cur_arc.fetch_sub(1, Ordering::SeqCst);
                    Json(serde_json::json!({
                        "choices": [{
                            "message": {
                                "content": "{\"title\":\"Concurrent Digest\",\"stoppedAt\":\"Done\",\"leftOpen\":\"\",\"suggestedLine\":\"Shared Line\",\"isOneOff\":false}"
                            }
                        }],
                        "usage": { "prompt_tokens": 1000, "completion_tokens": 100 }
                    }))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let dir = TempDir::new().unwrap();
        let specs = [
            ("c1", "/tmp/ws-a"),
            ("c2", "/tmp/ws-b"),
            ("c3", "/tmp/ws-c"),
            ("c4", "/tmp/ws-d"),
            ("c5", "/tmp/ws-e"),
            ("c6", "/tmp/ws-f"),
        ];
        let (db_path, roots) = seed_codex_test_sessions(dir.path(), &specs);
        let pinned_line_id = {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            let pinned_id = store.create_line("Pinned Keep").unwrap();
            // Pin `codex:c1` to the pinned Line before Sync runs with `force_all = true`.
            store
                .update_session(
                    "codex:c1",
                    &crate::model::LineIdUpdate::Set(pinned_id.clone()),
                    None,
                )
                .unwrap();
            let settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "openai".to_string(),
                api_base_url: format!("http://{addr}"),
                api_model: "gpt-4o-mini".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 1,
                sync_concurrency: 3,
                ..AppSettings::default()
            };
            store.save_settings(&settings).unwrap();
            pinned_id
        };

        let state = AppState::new(db_path.clone(), roots);
        let mut rx = spawn_sync_stream(
            state,
            SyncScope {
                days: 7,
                anchor_date: None,
                harness: None,
            },
            true,
            None,
        );

        let mut start_concurrency = 0;
        let mut batch_done_count = 0;
        let mut complete_summary = None;
        let mut complete_plane = None;

        while let Some(evt) = rx.recv().await {
            match evt {
                SyncStreamEvent::Start { concurrency, .. } => start_concurrency = concurrency,
                SyncStreamEvent::BatchDone(done) => {
                    batch_done_count += 1;
                    assert!(
                        done.plane.is_none(),
                        "intermediate BatchDone must stay lightweight with plane == None"
                    );
                }
                SyncStreamEvent::Complete { summary, plane } => {
                    complete_summary = Some(summary);
                    complete_plane = Some(*plane);
                }
                SyncStreamEvent::BatchStart { .. } | SyncStreamEvent::Heartbeat => {}
            }
        }
        server_task.abort();

        assert_eq!(start_concurrency, 3);
        assert_eq!(batch_done_count, 6);
        let summary = complete_summary.expect("expected Complete event");
        assert_eq!(summary.processed, 6);
        assert_eq!(summary.failed, 0);
        let peak = peak_in_flight.load(Ordering::SeqCst);
        assert!(
            (2..=3).contains(&peak),
            "expected 2..=3 concurrent in-flight batches under syncConcurrency=3, got {peak}"
        );

        // Verify pinned Line immutability and zero duplicate unpinned lines under concurrency.
        let plane = complete_plane.expect("expected full PlaneResponse on Complete");
        let pinned_line = plane
            .lines
            .iter()
            .find(|l| l.id == pinned_line_id)
            .expect("pinned line must remain intact");
        assert!(
            pinned_line.sessions.iter().any(|s| s.id == "codex:c1"),
            "pinned session codex:c1 must never be moved by concurrent Sync"
        );
        let shared_lines: Vec<_> = plane
            .lines
            .iter()
            .filter(|l| l.name.eq_ignore_ascii_case("Shared Line"))
            .collect();
        assert_eq!(
            shared_lines.len(),
            1,
            "concurrent batches suggesting the same Line name must deduplicate cleanly"
        );
        assert_eq!(shared_lines[0].sessions.len(), 5);
    }

    #[tokio::test]
    async fn transient_http_429_and_503_trigger_bounded_backoff_retry() {
        use std::sync::atomic::AtomicUsize;

        use axum::Router;
        use axum::http::{StatusCode, header};
        use axum::response::IntoResponse;
        use axum::routing::post;

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = attempts.clone();
        let mock_api = Router::new().route(
            "/chat/completions",
            post(move || {
                let c = attempts_ref.clone();
                async move {
                    match c.fetch_add(1, Ordering::SeqCst) {
                        0 => (
                            StatusCode::TOO_MANY_REQUESTS,
                            [(header::RETRY_AFTER, "0")],
                            "rate limited",
                        )
                            .into_response(),
                        1 => (
                            StatusCode::SERVICE_UNAVAILABLE,
                            [(header::CONTENT_TYPE, "text/plain")],
                            "service overloaded",
                        )
                            .into_response(),
                        _ => (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}],"usage":{"prompt_tokens":100,"completion_tokens":20}}"#,
                        )
                            .into_response(),
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "openai".to_string(),
            api_base_url: format!("http://{addr}"),
            api_model: "gpt-4o-mini".to_string(),
            api_key: "test-key".to_string(),
            ..AppSettings::default()
        };
        let client = build_shared_http_client();

        let out = TEST_RETRY_BASE_DELAY_MS_OVERRIDE
            .scope(Some(5), async {
                run_api_prompt("ping", &settings, &client).await
            })
            .await
            .unwrap();
        server_task.abort();

        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(out.reply.unwrap(), r#"{"ok":true}"#);
        assert_eq!(out.usage.api_calls, 1);
        assert_eq!(out.usage.total_tokens, 120);
    }

    #[tokio::test]
    async fn sync_stream_cancellation_drains_on_first_stop_and_aborts_on_drop() {
        use std::sync::atomic::AtomicUsize;

        use axum::routing::post;
        use axum::{Json, Router};
        use tempfile::TempDir;

        let started_calls = Arc::new(AtomicUsize::new(0));
        let finished_calls = Arc::new(AtomicUsize::new(0));
        let started_ref = started_calls.clone();
        let finished_ref = finished_calls.clone();

        let mock_api = Router::new().route(
            "/chat/completions",
            post(move || {
                let st = started_ref.clone();
                let fin = finished_ref.clone();
                async move {
                    st.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    fin.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({
                        "choices": [{
                            "message": {
                                "content": "{\"title\":\"Drain OK\",\"stoppedAt\":\"Done\",\"leftOpen\":\"\",\"suggestedLine\":\"Cancel Line\",\"isOneOff\":false}"
                            }
                        }],
                        "usage": { "prompt_tokens": 200, "completion_tokens": 50 }
                    }))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let dir = TempDir::new().unwrap();
        let specs = [
            ("k1", "/tmp/ws-1"),
            ("k2", "/tmp/ws-2"),
            ("k3", "/tmp/ws-3"),
            ("k4", "/tmp/ws-4"),
            ("k5", "/tmp/ws-5"),
            ("k6", "/tmp/ws-6"),
        ];
        let (db_path, roots) = seed_codex_test_sessions(dir.path(), &specs);
        {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            // Seed one existing Line so concurrency starts immediately without warm-up seed.
            store.create_line("Existing").unwrap();
            let settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "openai".to_string(),
                api_base_url: format!("http://{addr}"),
                api_model: "gpt-4o-mini".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 1,
                sync_concurrency: 2,
                ..AppSettings::default()
            };
            store.save_settings(&settings).unwrap();
        }

        // Part A: First Stop (drain) — stops scheduling new batches while letting the 2 in-flight
        // batches complete and commit cleanly.
        let state = AppState::new(db_path.clone(), roots.clone());
        let mut rx = spawn_sync_stream(
            state.clone(),
            SyncScope {
                days: 7,
                anchor_date: None,
                harness: None,
            },
            false,
            None,
        );

        let mut started_events = 0_usize;
        let mut final_summary = None;
        while let Some(evt) = rx.recv().await {
            match evt {
                SyncStreamEvent::BatchStart { .. } => {
                    started_events += 1;
                    if started_events == 2 {
                        assert!(
                            state.request_sync_drain(),
                            "request_sync_drain should signal active stream"
                        );
                    }
                }
                SyncStreamEvent::Complete { summary, .. } => {
                    final_summary = Some(summary);
                }
                _ => {}
            }
        }

        let summary = final_summary.expect("expected Complete after drain");
        assert_eq!(summary.processed, 2);
        assert_eq!(summary.skipped, 4);
        assert_eq!(started_calls.load(Ordering::SeqCst), 2);
        assert_eq!(finished_calls.load(Ordering::SeqCst), 2);

        // Part B: Second Stop (drop receiver) — aborts in-flight JoinSet tasks immediately.
        started_calls.store(0, Ordering::SeqCst);
        finished_calls.store(0, Ordering::SeqCst);
        let mut rx2 = spawn_sync_stream(
            state,
            SyncScope {
                days: 7,
                anchor_date: None,
                harness: None,
            },
            false,
            None,
        );
        while let Some(evt) = rx2.recv().await {
            if matches!(evt, SyncStreamEvent::BatchStart { .. }) {
                drop(rx2);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
        server_task.abort();

        let store = crate::db::Store::open(&db_path).unwrap();
        let remaining = store
            .load_sync_candidates_in_scope(
                &store.get_settings().unwrap(),
                &compute_slice_config_hash(&store.get_settings().unwrap()),
                false,
                None,
                None,
                Local::now(),
            )
            .unwrap();
        assert_eq!(
            remaining.len(),
            4,
            "dropping stream receiver must abort in-flight tasks before committing their digests"
        );
    }

    #[tokio::test]
    async fn cold_start_warmup_runs_first_batch_before_fanning_out() {
        use std::sync::atomic::AtomicUsize;

        use axum::routing::post;
        use axum::{Json, Router};
        use tempfile::TempDir;

        let call_seq = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let first_batch_peak = Arc::new(AtomicUsize::new(0));
        let post_warmup_peak = Arc::new(AtomicUsize::new(0));

        let seq_ref = call_seq.clone();
        let cur_ref = in_flight.clone();
        let first_pk = first_batch_peak.clone();
        let post_pk = post_warmup_peak.clone();

        let mock_api = Router::new().route(
            "/chat/completions",
            post(move || {
                let seq = seq_ref.clone();
                let cur_arc = cur_ref.clone();
                let fpk = first_pk.clone();
                let ppk = post_pk.clone();
                async move {
                    let n = seq.fetch_add(1, Ordering::SeqCst);
                    let cur = cur_arc.fetch_add(1, Ordering::SeqCst) + 1;
                    if n == 0 {
                        fpk.fetch_max(cur, Ordering::SeqCst);
                    } else {
                        ppk.fetch_max(cur, Ordering::SeqCst);
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    cur_arc.fetch_sub(1, Ordering::SeqCst);
                    Json(serde_json::json!({
                        "choices": [{
                            "message": {
                                "content": "{\"title\":\"Seeded\",\"stoppedAt\":\"Done\",\"leftOpen\":\"\",\"suggestedLine\":\"Seed Line\",\"isOneOff\":false}"
                            }
                        }],
                        "usage": { "prompt_tokens": 300, "completion_tokens": 60 }
                    }))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let dir = TempDir::new().unwrap();
        let specs = [
            ("w1", "/tmp/ws-1"),
            ("w2", "/tmp/ws-2"),
            ("w3", "/tmp/ws-3"),
            ("w4", "/tmp/ws-4"),
        ];
        let (db_path, roots) = seed_codex_test_sessions(dir.path(), &specs);
        {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            // Ensure zero non-system Lines at start.
            assert_eq!(store.list_line_catalog().unwrap().len(), 0);
            let settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "openai".to_string(),
                api_base_url: format!("http://{addr}"),
                api_model: "gpt-4o-mini".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 1,
                sync_concurrency: 3,
                ..AppSettings::default()
            };
            store.save_settings(&settings).unwrap();
        }

        let state = AppState::new(db_path, roots);
        let summary = sync_pending_sessions_in_scope(&state, false, Some(4), None)
            .await
            .unwrap();
        server_task.abort();

        assert_eq!(summary.processed, 4);
        assert_eq!(
            first_batch_peak.load(Ordering::SeqCst),
            1,
            "cold-start warm-up must run Batch 1 alone before fanning out"
        );
        assert!(
            post_warmup_peak.load(Ordering::SeqCst) >= 2,
            "post-warmup batches must fan out concurrently, got {}",
            post_warmup_peak.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn sqlite_wal_concurrent_batch_commits_have_zero_busy_errors_and_preserve_pinned() {
        let dir = tempfile::TempDir::new().unwrap();
        let specs = [
            ("p1", "/tmp/ws-1"),
            ("p2", "/tmp/ws-2"),
            ("p3", "/tmp/ws-3"),
            ("p4", "/tmp/ws-4"),
            ("p5", "/tmp/ws-5"),
            ("p6", "/tmp/ws-6"),
            ("p7", "/tmp/ws-7"),
            ("p8", "/tmp/ws-8"),
        ];
        let (db_path, roots) = seed_codex_test_sessions(dir.path(), &specs);
        let pinned_id = {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            let pid = store.create_line("Immutable Pinned").unwrap();
            store
                .update_session(
                    "codex:p1",
                    &crate::model::LineIdUpdate::Set(pid.clone()),
                    None,
                )
                .unwrap();
            pid
        };

        let mut handles = Vec::new();
        for group in 0..4 {
            let path = db_path.clone();
            handles.push(std::thread::spawn(move || {
                let mut store = crate::db::Store::open(&path).unwrap();
                let s1 = format!("codex:p{}", group * 2 + 1);
                let s2 = format!("codex:p{}", group * 2 + 2);
                let writes = vec![
                    SessionDigestWrite {
                        session_id: s1,
                        step_count: 6,
                        slice_hash: "h1".to_string(),
                        title: format!("Digest {}a", group + 1),
                        stopped_at: "Done".to_string(),
                        left_open: String::new(),
                        suggested_line: "Concurrent Target".to_string(),
                        is_one_off: false,
                    },
                    SessionDigestWrite {
                        session_id: s2,
                        step_count: 6,
                        slice_hash: "h1".to_string(),
                        title: format!("Digest {}b", group + 1),
                        stopped_at: "Done".to_string(),
                        left_open: String::new(),
                        suggested_line: "Concurrent Target".to_string(),
                        is_one_off: false,
                    },
                ];
                store.apply_session_digests_batch(&writes)
            }));
        }

        for h in handles {
            h.join()
                .expect("worker thread panicked")
                .expect("concurrent WAL batch commit must not return SQLITE_BUSY");
        }

        let store = crate::db::Store::open(&db_path).unwrap();
        let plane = store.build_plane_response(7, Local::now()).unwrap();
        let pinned_line = plane
            .lines
            .iter()
            .find(|l| l.id == pinned_id)
            .expect("pinned line present");
        assert_eq!(pinned_line.sessions.len(), 1);
        assert_eq!(pinned_line.sessions[0].id, "codex:p1");
        assert_eq!(pinned_line.sessions[0].title, "Digest 1a");

        let target_lines: Vec<_> = plane
            .lines
            .iter()
            .filter(|l| l.name == "Concurrent Target")
            .collect();
        assert_eq!(target_lines.len(), 1);
        assert_eq!(target_lines[0].sessions.len(), 7);
    }

    #[tokio::test]
    async fn http_retry_decides_before_body_read_and_preserves_bounded_jitter_with_retry_after() {
        use std::collections::HashSet;
        use std::sync::atomic::AtomicUsize;

        use axum::Router;
        use axum::http::{StatusCode, header};
        use axum::response::IntoResponse;
        use axum::routing::post;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let fixed_now = DateTime::parse_from_rfc3339("2026-11-25T07:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        // 1. Parse both RFC 9110 Retry-After formats (delay-seconds + HTTP-date).
        assert_eq!(parse_retry_after_ms("0", fixed_now), Some(0));
        assert_eq!(parse_retry_after_ms("2", fixed_now), Some(2_000));
        assert_eq!(
            parse_retry_after_ms("Wed, 25 Nov 2026 07:00:03 GMT", fixed_now),
            Some(3_000)
        );
        assert_eq!(
            parse_retry_after_ms("Wed, 25 Nov 2026 06:59:55 GMT", fixed_now),
            Some(0)
        );
        assert_eq!(
            parse_retry_after_ms("Wednesday, 25-Nov-26 07:00:04 GMT", fixed_now),
            Some(4_000)
        );
        assert_eq!(
            parse_retry_after_ms("Wed Nov 25 07:00:05 2026", fixed_now),
            Some(5_000)
        );
        assert_eq!(parse_retry_after_ms("not-a-date", fixed_now), None);

        // 2. Verify bounded jitter is preserved for both delay-seconds and HTTP-date within budget,
        //    and over-budget Retry-After (both formats) skips automatic retry (`None`).
        let jitter_span = API_RETRY_BASE_DELAY_MS / 2;
        let mut samples_1s = HashSet::new();
        let mut samples_http_date = HashSet::new();
        for _ in 0..12 {
            let d1 = compute_retry_backoff_ms(0, Some("1"), fixed_now)
                .expect("1s Retry-After is within budget");
            assert!(
                (1_000..=1_000 + jitter_span).contains(&d1),
                "expected 1s Retry-After + bounded jitter, got {d1}"
            );
            samples_1s.insert(d1);

            let dd = compute_retry_backoff_ms(0, Some("Wed, 25 Nov 2026 07:00:02 GMT"), fixed_now)
                .expect("2s HTTP-date Retry-After is within budget");
            assert!(
                (2_000..=2_000 + jitter_span).contains(&dd),
                "expected 2s HTTP-date Retry-After + bounded jitter, got {dd}"
            );
            samples_http_date.insert(dd);
        }
        assert!(
            samples_1s.len() > 1,
            "Retry-After 1s must preserve jitter variance across calls, got {samples_1s:?}"
        );
        assert!(
            samples_http_date.len() > 1,
            "HTTP-date Retry-After must preserve jitter variance across calls, got {samples_http_date:?}"
        );

        // Over-budget delay-seconds and HTTP-date MUST skip automatic retry (`None`).
        assert_eq!(compute_retry_backoff_ms(0, Some("60"), fixed_now), None);
        assert_eq!(
            compute_retry_backoff_ms(0, Some("Wed, 25 Nov 2026 07:01:00 GMT"), fixed_now),
            None
        );
        // Exact budget boundary (`10s`) stays allowed without exceeding `API_RETRY_MAX_DELAY_MS`.
        assert_eq!(
            compute_retry_backoff_ms(0, Some("10"), fixed_now),
            Some(API_RETRY_MAX_DELAY_MS)
        );

        // 3. Verify live HTTP 429 with over-budget Retry-After skips automatic retry (1 attempt only).
        let over_budget_calls = Arc::new(AtomicUsize::new(0));
        let ob_ref = over_budget_calls.clone();
        let ob_router = Router::new().route(
            "/chat/completions",
            post(move || {
                let c = ob_ref.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [(header::RETRY_AFTER, "60")],
                        "quota window resets in 60s",
                    )
                        .into_response()
                }
            }),
        );
        let ob_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ob_addr = ob_listener.local_addr().unwrap();
        let ob_server = tokio::spawn(async move {
            let _ = axum::serve(ob_listener, ob_router).await;
        });
        let ob_settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "openai".to_string(),
            api_base_url: format!("http://{ob_addr}"),
            api_model: "gpt-4o-mini".to_string(),
            api_key: "test-key".to_string(),
            ..AppSettings::default()
        };
        let client = build_shared_http_client();
        let ob_out = TEST_RETRY_BASE_DELAY_MS_OVERRIDE
            .scope(Some(5), async {
                run_api_prompt("ping", &ob_settings, &client).await
            })
            .await
            .unwrap();
        ob_server.abort();
        assert_eq!(
            over_budget_calls.load(Ordering::SeqCst),
            1,
            "Retry-After exceeding retry budget must skip automatic retry"
        );
        assert!(
            ob_out.reply.is_err(),
            "skipped over-budget 429 must surface as error"
        );

        // 4. Verify truncated/broken 503 response body STILL retries because status is checked BEFORE body read.
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = attempts.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            for _ in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut req_buf = [0_u8; 2048];
                let _ = socket.read(&mut req_buf).await;
                if attempts_ref.fetch_add(1, Ordering::SeqCst) == 0 {
                    // Advertise 4096-byte body on 503 and abruptly close socket mid-body!
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 4096\r\nConnection: close\r\n\r\ntruncated",
                        )
                        .await;
                    let _ = socket.shutdown().await;
                } else {
                    let body = r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}],"usage":{"prompt_tokens":50,"completion_tokens":10}}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                }
            }
        });

        let settings = AppSettings {
            processor_configured: true,
            processor_mode: "api".to_string(),
            api_provider: "openai".to_string(),
            api_base_url: format!("http://{addr}"),
            api_model: "gpt-4o-mini".to_string(),
            api_key: "test-key".to_string(),
            ..AppSettings::default()
        };
        let out = TEST_RETRY_BASE_DELAY_MS_OVERRIDE
            .scope(Some(5), async {
                run_api_prompt("ping", &settings, &client).await
            })
            .await
            .unwrap();
        server_task.abort();

        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(out.reply.unwrap(), r#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn scoped_stop_targets_single_run_and_preserves_pre_registration_intent() {
        use std::sync::atomic::AtomicUsize;

        use axum::routing::post;
        use axum::{Json, Router};
        use tempfile::TempDir;

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_ref = calls.clone();
        let mock_api = Router::new().route(
            "/chat/completions",
            post(move || {
                let c = calls_ref.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(60)).await;
                    Json(serde_json::json!({
                        "choices": [{
                            "message": {
                                "content": "{\"title\":\"Scoped OK\",\"stoppedAt\":\"Done\",\"leftOpen\":\"\",\"suggestedLine\":\"Scoped Line\",\"isOneOff\":false}"
                            }
                        }],
                        "usage": { "prompt_tokens": 100, "completion_tokens": 20 }
                    }))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_api).await;
        });

        let dir = TempDir::new().unwrap();
        let specs = [
            ("s1", "/tmp/ws-1"),
            ("s2", "/tmp/ws-2"),
            ("s3", "/tmp/ws-3"),
            ("s4", "/tmp/ws-4"),
        ];
        let (db_path, roots) = seed_codex_test_sessions(dir.path(), &specs);
        {
            let mut store = crate::db::Store::open(&db_path).unwrap();
            crate::ingest::scan_all_harnesses(&mut store, &roots).unwrap();
            store.create_line("Seed").unwrap();
            let settings = AppSettings {
                processor_configured: true,
                processor_mode: "api".to_string(),
                api_provider: "openai".to_string(),
                api_base_url: format!("http://{addr}"),
                api_model: "gpt-4o-mini".to_string(),
                api_key: "test-key".to_string(),
                sync_batch_limit: 1,
                sync_concurrency: 1,
                ..AppSettings::default()
            };
            store.save_settings(&settings).unwrap();
        }

        let state = AppState::new(db_path, roots);

        // Part 0: Invalid runIds and duplicate active registrations are rejected cleanly.
        let overlong = "a".repeat(129);
        for bad in [
            "",
            "   ",
            "__anon_sync_1",
            "bad id",
            "a/b",
            overlong.as_str(),
        ] {
            assert!(
                state.prepare_sync_drain(Some(bad)).is_err(),
                "prepare_sync_drain must reject invalid runId `{bad}`"
            );
            assert!(
                state.request_sync_drain_for(Some(bad)).is_err(),
                "request_sync_drain_for must reject invalid runId `{bad}`"
            );
        }
        let held_flag = state.prepare_sync_drain(Some("dup-check")).unwrap();
        assert!(
            state.prepare_sync_drain(Some("dup-check")).is_err(),
            "duplicate active runId must be rejected"
        );
        // Invalid Stop must not affect the held active run.
        assert!(state.request_sync_drain_for(Some("")).is_err());
        assert!(!held_flag.load(Ordering::Acquire));
        drop(held_flag);
        // Once dropped, the runId can be cleanly reused.
        assert!(state.prepare_sync_drain(Some("dup-check")).is_ok());

        // Part A: Stop arrives BEFORE the stream registers (e.g. during scan) — intent is preserved.
        assert!(
            state
                .request_sync_drain_for(Some("early-stop-run"))
                .unwrap()
        );
        let pre_flag = state.prepare_sync_drain(Some("early-stop-run")).unwrap();
        let mut rx_early = spawn_sync_stream_with_drain(
            state.clone(),
            SyncScope {
                days: 7,
                anchor_date: None,
                harness: None,
            },
            false,
            None,
            pre_flag,
        );
        let mut early_summary = None;
        while let Some(evt) = rx_early.recv().await {
            if let SyncStreamEvent::Complete { summary, .. } = evt {
                early_summary = Some(summary);
            }
        }
        let early_summary = early_summary.expect("expected Complete on pre-stopped run");
        assert_eq!(early_summary.processed, 0);
        assert_eq!(early_summary.skipped, 4);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // Part B: Two concurrent streams (`run-a` and `run-b`) — stopping `run-a` must NOT stop `run-b`.
        let flag_a = state.prepare_sync_drain(Some("run-a")).unwrap();
        let flag_b = state.prepare_sync_drain(Some("run-b")).unwrap();
        let mut rx_a = spawn_sync_stream_with_drain(
            state.clone(),
            SyncScope {
                days: 7,
                anchor_date: None,
                harness: None,
            },
            true,
            Some(3),
            flag_a,
        );
        let mut rx_b = spawn_sync_stream_with_drain(
            state.clone(),
            SyncScope {
                days: 7,
                anchor_date: None,
                harness: None,
            },
            true,
            Some(3),
            flag_b,
        );

        let task_a = tokio::spawn(async move {
            let mut stopped = false;
            let mut summary = None;
            while let Some(evt) = rx_a.recv().await {
                match evt {
                    SyncStreamEvent::BatchStart { .. } if !stopped => {
                        stopped = true;
                        let _ = state.request_sync_drain_for(Some("run-a"));
                    }
                    SyncStreamEvent::Complete { summary: s, .. } => summary = Some(s),
                    _ => {}
                }
            }
            summary
        });

        let task_b = tokio::spawn(async move {
            let mut summary = None;
            while let Some(evt) = rx_b.recv().await {
                if let SyncStreamEvent::Complete { summary: s, .. } = evt {
                    summary = Some(s);
                }
            }
            summary
        });

        let sum_a = task_a.await.unwrap().expect("run-a Complete");
        let sum_b = task_b.await.unwrap().expect("run-b Complete");
        server_task.abort();

        assert_eq!(sum_a.processed, 1, "run-a should drain after 1 batch");
        assert_eq!(sum_a.skipped, 2, "run-a should skip remaining 2 batches");
        assert_eq!(
            sum_b.processed, 3,
            "run-b must complete unaffected when run-a is stopped"
        );
        assert_eq!(sum_b.skipped, 0);
    }
}
