use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::db::{LineCatalogEntry, SyncCandidate, SyncScope, UnpinnedClusterItem};
use crate::defaults::{
    API_TIMEOUT_SECS, CLI_TIMEOUT_SECS, FEWEST_USER_INPUTS_MIN, MAX_BATCH_PROMPT_CHARS,
    SLICER_CHAR_BUDGET_MIN, STREAM_HEARTBEAT_SECS, SYNC_BATCH_LIMIT_MAX, SYNC_BATCH_LIMIT_MIN,
    WINDOW_DAYS_MAX, WINDOW_DAYS_MIN, api_provider, cli_engine_order,
};
use crate::env::resolve_cli_binary;
use crate::error::{PlanarError, Result};
use crate::ingest::antigravity::extract_antigravity_user_request;
use crate::ingest::claude::is_claude_synthetic_wrapper;
use crate::ingest::codex::is_codex_synthetic_block;
use crate::ingest::dsh::extract_text_from_value;
use crate::ingest::{stream_jsonl_lines, strip_delegation_envelope};
use crate::model::{
    AppSettings, INTERNAL_DIGEST_SENTINEL, ProcessorTestResult, SyncBatchDoneDto,
    SyncBatchPreviewItem, SyncFiledPreview, SyncStreamEvent, SyncSummaryDto,
};
use crate::server::AppState;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

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
    let started = Instant::now();
    let ping_prompt = "Respond with strictly valid JSON matching this schema and nothing else: {\"ok\": true, \"pong\": \"planar\"}";
    let (reply, backend_used) = match run_llm_prompt(ping_prompt, settings, hydrated_path).await {
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

    let ok = extract_json_object(&reply)
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .is_some_and(|v| v.get("ok").and_then(Value::as_bool).unwrap_or(true));

    let preview = truncate_chars(&normalize_step_text(&reply), 120);
    Ok(ProcessorTestResult {
        ok,
        backend_used,
        latency_ms,
        message: if ok {
            format!("Connected ({preview})")
        } else {
            format!("Unexpected reply: {preview}")
        },
    })
}

struct PreparedSession {
    cand: SyncCandidate,
    sliced: SlicedTrajectory,
    approx_chars: usize,
}

struct BatchRunResult {
    filed: Vec<SyncFiledPreview>,
    failed: usize,
    backend_used: Option<String>,
    last_error: Option<String>,
    fatal: bool,
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

    let cfg_clone = settings.clone();
    let sliced_results = tokio::task::spawn_blocking(move || {
        to_slice
            .into_iter()
            .map(|cand| {
                let res =
                    slice_trajectory_file(Path::new(&cand.source_path), &cand.harness, &cfg_clone);
                (cand, res)
            })
            .collect::<Vec<_>>()
    })
    .await;

    let Ok(pairs) = sliced_results else {
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

/// Executes a single multi-session LLM call for `batch` (e.g. 4 sessions at once) and commits digests.
async fn execute_prepared_batch(
    state: &AppState,
    batch: &[PreparedSession],
    settings: &AppSettings,
    slice_hash: &str,
) -> BatchRunResult {
    if !settings.is_processor_configured() {
        return BatchRunResult {
            filed: Vec::new(),
            failed: batch.len(),
            backend_used: None,
            last_error: Some(unconfigured_processor_message(settings)),
            fatal: true,
        };
    }

    let catalog = match state.with_store(|store| store.list_line_catalog()).await {
        Ok(c) => c,
        Err(err) => {
            return BatchRunResult {
                filed: Vec::new(),
                failed: batch.len(),
                backend_used: None,
                last_error: Some(err.to_string()),
                fatal: false,
            };
        }
    };

    let prompt = if batch.len() == 1 {
        build_digest_prompt(&batch[0].cand, &batch[0].sliced, &catalog, settings)
    } else {
        build_batch_digest_prompt(batch, &catalog, settings)
    };

    let (reply, backend_used) = match run_llm_prompt(&prompt, settings, &state.hydrated_path).await
    {
        Ok(res) => res,
        Err(err) => {
            let fatal = is_fatal_processor_error(&err);
            return BatchRunResult {
                filed: Vec::new(),
                failed: batch.len(),
                backend_used: None,
                last_error: Some(err.to_string()),
                fatal,
            };
        }
    };

    let expected_ids: Vec<String> = batch.iter().map(|p| p.cand.id.clone()).collect();
    let mut parsed_map = match parse_batch_digest_json(&reply, &expected_ids) {
        Ok(m) => m,
        Err(err) => {
            return BatchRunResult {
                filed: Vec::new(),
                failed: batch.len(),
                backend_used: Some(backend_used),
                last_error: Some(err.to_string()),
                fatal: false,
            };
        }
    };

    let mut filed = Vec::new();
    let mut failed = 0_usize;
    let mut last_error = None;
    let mut omitted: Vec<&PreparedSession> = Vec::new();

    for item in batch {
        if let Some(digest) = parsed_map.remove(&item.cand.id) {
            match apply_digest_to_store(state, &item.cand, slice_hash, &digest).await {
                Ok(()) => {
                    filed.push(SyncFiledPreview {
                        session_id: item.cand.id.clone(),
                        title: digest.title,
                        line_name: digest.suggested_line,
                    });
                }
                Err(err) => {
                    failed += 1;
                    last_error = Some(err.to_string());
                }
            }
        } else {
            omitted.push(item);
        }
    }

    // If the batch call omitted any session ID, fall back once per omitted item so no work is dropped.
    if !omitted.is_empty() {
        let refreshed_catalog = state
            .with_store(|store| store.list_line_catalog())
            .await
            .unwrap_or(catalog);
        for item in omitted {
            let solo_prompt =
                build_digest_prompt(&item.cand, &item.sliced, &refreshed_catalog, settings);
            match run_llm_prompt(&solo_prompt, settings, &state.hydrated_path).await {
                Ok((solo_reply, _)) => match parse_digest_json(&solo_reply) {
                    Ok(digest) => {
                        match apply_digest_to_store(state, &item.cand, slice_hash, &digest).await {
                            Ok(()) => {
                                filed.push(SyncFiledPreview {
                                    session_id: item.cand.id.clone(),
                                    title: digest.title,
                                    line_name: digest.suggested_line,
                                });
                            }
                            Err(err) => {
                                failed += 1;
                                last_error = Some(err.to_string());
                            }
                        }
                    }
                    Err(err) => {
                        failed += 1;
                        last_error = Some(err.to_string());
                    }
                },
                Err(err) => {
                    failed += 1;
                    last_error = Some(err.to_string());
                }
            }
        }
    }

    BatchRunResult {
        filed,
        failed,
        backend_used: Some(backend_used),
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
    let (settings, slice_hash, candidates) = state
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
            Ok((settings, slice_hash, candidates))
        })
        .await?;

    let default_backend = planned_backend_label(&settings);
    let mut summary = SyncSummaryDto {
        processed: 0,
        failed: 0,
        skipped: 0,
        backend_used: default_backend,
        last_error: None,
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

    let batches = chunk_prepared_into_batches(prepared, settings.sync_batch_limit);
    let total_batches = batches.len();
    for (idx, batch) in batches.iter().enumerate() {
        let res = execute_prepared_batch(state, batch, &settings, &slice_hash).await;
        if let Some(backend) = res.backend_used {
            summary.backend_used = backend;
        }
        summary.processed += res.filed.len();
        summary.failed += res.failed;
        if res.last_error.is_some() {
            summary.last_error = res.last_error;
        }
        if res.fatal {
            let remaining: usize = batches[idx + 1..].iter().map(Vec::len).sum();
            summary.skipped += remaining;
            let _ = total_batches;
            break;
        }
    }

    Ok(summary)
}

/// Spawns a multi-session batch streaming `Sync` coordinator scoped to `scope`, emitting NDJSON
/// events as each batch starts and completes while keeping the connection alive with heartbeats.
pub fn spawn_sync_stream(
    state: AppState,
    scope: SyncScope,
    force_all: bool,
    limit_override: Option<usize>,
) -> tokio::sync::mpsc::Receiver<SyncStreamEvent> {
    let (tx, rx) = tokio::sync::mpsc::channel(24);
    tokio::spawn(async move {
        run_sync_stream_coordinator(state, scope, force_all, limit_override, tx).await;
    });
    rx
}

async fn run_sync_stream_coordinator(
    state: AppState,
    scope: SyncScope,
    force_all: bool,
    limit_override: Option<usize>,
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
            Ok((settings, slice_hash, candidates))
        })
        .await;

    let (settings, slice_hash, candidates) = match init_res {
        Ok(tuple) => tuple,
        Err(err) => {
            let fallback_summary = SyncSummaryDto {
                processed: 0,
                failed: 1,
                skipped: 0,
                backend_used: "error".to_string(),
                last_error: Some(err.to_string()),
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
    let mut summary = SyncSummaryDto {
        processed: 0,
        failed: 0,
        skipped: 0,
        backend_used: default_backend.clone(),
        last_error: None,
    };

    if candidates.is_empty() {
        let _ = tx
            .send(SyncStreamEvent::Start {
                total: 0,
                total_batches: 0,
                batch_size,
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
        if let Ok(plane) = state.build_plane_snapshot(days, anchor).await {
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
                plane,
            }));
            if tx.send(ev).await.is_err() {
                return;
            }
        }
    }

    let mut heartbeat = tokio::time::interval(Duration::from_secs(STREAM_HEARTBEAT_SECS));
    heartbeat.tick().await;

    for (idx, batch) in batches.into_iter().enumerate() {
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
            return;
        }

        let st = state.clone();
        let cfg = settings.clone();
        let sh = slice_hash.clone();
        let mut batch_handle =
            tokio::spawn(async move { execute_prepared_batch(&st, &batch, &cfg, &sh).await });

        let batch_res = loop {
            tokio::select! {
                () = tx.closed() => {
                    batch_handle.abort();
                    return;
                }
                _ = heartbeat.tick() => {
                    if tx.send(SyncStreamEvent::Heartbeat).await.is_err() {
                        batch_handle.abort();
                        return;
                    }
                }
                joined = &mut batch_handle => {
                    match joined {
                        Ok(res) => break res,
                        Err(join_err) => break BatchRunResult {
                            filed: Vec::new(),
                            failed: cur_batch_len,
                            backend_used: None,
                            last_error: Some(join_err.to_string()),
                            fatal: false,
                        },
                    }
                }
            }
        };

        let batch_processed = batch_res.filed.len();
        let batch_failed = batch_res.failed;
        let batch_error = batch_res.last_error.clone();
        let is_fatal = batch_res.fatal;

        if let Some(backend) = batch_res.backend_used {
            summary.backend_used = backend;
        }
        summary.processed += batch_processed;
        summary.failed += batch_failed;
        if batch_res.last_error.is_some() {
            summary.last_error = batch_res.last_error;
        }
        completed = completed.saturating_add(cur_batch_len);

        if is_fatal {
            let remaining_items = total_items.saturating_sub(completed);
            summary.skipped += remaining_items;
        }

        state.record_last_sync(summary.clone());
        if let Ok(plane) = state.build_plane_snapshot(days, anchor).await {
            let done_ev = SyncStreamEvent::BatchDone(Box::new(SyncBatchDoneDto {
                batch_index,
                total_batches,
                batch_processed,
                batch_failed,
                filed: batch_res.filed,
                error: batch_error,
                processed: summary.processed,
                failed: summary.failed,
                skipped: summary.skipped,
                completed,
                total: total_items,
                backend_used: summary.backend_used.clone(),
                plane,
            }));
            if tx.send(done_ev).await.is_err() {
                return;
            }
        }

        if is_fatal {
            break;
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
        });
    }

    if !settings.is_processor_configured() {
        return Err(PlanarError::InvalidInput(unconfigured_processor_message(
            &settings,
        )));
    }

    let prompt = build_regroup_prompt(&items, &catalog, &settings);
    let (reply, backend_used) = run_llm_prompt(&prompt, &settings, &state.hydrated_path).await?;
    let assignments = parse_regroup_json(&reply)?;
    let moved = state
        .with_store(move |store| store.apply_regroup_mapping(&assignments))
        .await?;

    Ok(SyncSummaryDto {
        processed: moved,
        failed: 0,
        skipped: items.len().saturating_sub(moved),
        backend_used,
        last_error: None,
    })
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
) -> Result<(String, String)> {
    if settings.processor_mode == "api" {
        run_api_prompt(prompt, settings).await
    } else {
        run_cli_prompt(prompt, settings, hydrated_path).await
    }
}

async fn run_cli_prompt(
    prompt: &str,
    settings: &AppSettings,
    hydrated_path: &OsStr,
) -> Result<(String, String)> {
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
) -> Result<(String, String)> {
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

    Ok((reply, format!("cli:{harness}")))
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

async fn run_api_prompt(prompt: &str, settings: &AppSettings) -> Result<(String, String)> {
    let provider = settings.api_provider.trim().to_ascii_lowercase();
    let api_key = resolve_api_key(&provider, settings)?;
    let (base, model) = resolve_api_endpoint(&provider, settings)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(API_TIMEOUT_SECS))
        .build()
        .map_err(|err| PlanarError::External(format!("HTTP client init failed: {err}")))?;

    let backend_label = format!("api:{provider}");
    let reply = match provider.as_str() {
        "gemini" => call_gemini_api(&client, base, model, &api_key, prompt).await?,
        "anthropic" => call_anthropic_api(&client, base, model, &api_key, prompt).await?,
        other => call_openai_compat_api(&client, other, base, model, &api_key, prompt).await?,
    };

    Ok((reply, backend_label))
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
        return Err(PlanarError::External(
            "Gemini API returned empty content".into(),
        ));
    }
    Ok(text)
}

async fn call_gemini_api(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<String> {
    let model_path = strip_gemini_models_prefix(model);
    let url = format!("{base}/models/{model_path}:generateContent?key={api_key}");
    let body = build_gemini_request_body(model_path, prompt);

    let val = send_json_post(client.post(&url).json(&body), "Gemini").await?;
    extract_gemini_reply_text(&val)
}

async fn call_anthropic_api(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<String> {
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
    if text.trim().is_empty() {
        return Err(PlanarError::External(
            "Anthropic API returned empty content".into(),
        ));
    }
    Ok(text)
}

async fn call_openai_compat_api(
    client: &reqwest::Client,
    provider: &str,
    base: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<String> {
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
    let text = val
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if text.trim().is_empty() {
        return Err(PlanarError::External(format!(
            "`{provider}` API returned empty content"
        )));
    }
    Ok(text)
}

async fn send_json_post(req: reqwest::RequestBuilder, label: &str) -> Result<Value> {
    let resp = req
        .send()
        .await
        .map_err(|err| PlanarError::External(format!("{label} request failed: {err}")))?;
    let status = resp.status();
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
    serde_json::from_str::<Value>(&body_text)
        .map_err(|err| PlanarError::External(format!("{label} returned invalid JSON: {err}")))
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
        let err = run_api_prompt("ping", &settings).await.unwrap_err();
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
}
