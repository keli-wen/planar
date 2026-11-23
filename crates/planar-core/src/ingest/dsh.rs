use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::burst::RawSpan;
use crate::db::{FileCursor, IngestMode, IngestedSession, Store};
use crate::error::Result;
use crate::ingest::{
    ScanDecision, StreamStatus, decide_scan, file_meta, normalize_prompt, parse_rfc3339_ms,
    scan_mode_for, stream_jsonl_lines, update_bounds,
};
use crate::model::ScanStats;

#[derive(Deserialize)]
struct DshLine<'a> {
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    session_id: Option<Cow<'a, str>>,
    #[serde(rename = "sessionId", borrow)]
    session_id_camel: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow)]
    workspace: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    #[serde(rename = "isSubagent")]
    is_subagent: Option<bool>,
    #[serde(rename = "parentSessionId", borrow)]
    parent_session_id: Option<Cow<'a, str>>,
    timestamp: Option<Value>,
    created_at: Option<Value>,
    ts: Option<Value>,
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
    content: Option<Value>,
    message: Option<Value>,
    header: Option<DshHeader<'a>>,
    payload: Option<Value>,
    usage: Option<Value>,
}

#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy, Default)]
struct DshUsage {
    input_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

#[derive(Deserialize)]
struct DshHeader<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    timestamp: Option<Value>,
}

/// Scans `DeepSeek` Harness (`dsh`) JSONL sessions under `root` (`~/.dsh/sessions/**/*.jsonl`).
pub(crate) fn scan_dsh(
    store: &mut Store,
    root: &Path,
    cursors: &HashMap<String, FileCursor>,
    stats: &mut ScanStats,
) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        stats.files_discovered += 1;

        let path_str = path.to_string_lossy().to_string();
        let Ok((size, mtime_ms)) = file_meta(path) else {
            stats.files_errored += 1;
            continue;
        };

        let decision = decide_scan(&path_str, size, mtime_ms, cursors);
        if decision == ScanDecision::Unchanged {
            stats.files_unchanged += 1;
            continue;
        }

        let (start_offset, mode) = scan_mode_for(decision);
        let existing_id = cursors.get(&path_str).map(|c| c.session_id.as_str());

        let parsed =
            match parse_dsh_file(path, &path_str, mtime_ms, start_offset, mode, existing_id) {
                Ok(Some(res)) => res,
                Ok(None) => {
                    if let Ok(Some(res)) = parse_dsh_file(
                        path,
                        &path_str,
                        mtime_ms,
                        0,
                        IngestMode::FullReplace,
                        existing_id,
                    ) {
                        res
                    } else {
                        stats.files_errored += 1;
                        continue;
                    }
                }
                Err(_) => {
                    stats.files_errored += 1;
                    continue;
                }
            };

        if parsed.step_count == 0 && mode == IngestMode::FullReplace {
            continue;
        }

        match parsed.mode {
            IngestMode::FullReplace => stats.files_full_scanned += 1,
            IngestMode::Incremental => stats.files_incremental += 1,
        }
        store.upsert_ingested_session(&parsed)?;
    }

    Ok(())
}

fn parse_dsh_file(
    path: &Path,
    path_str: &str,
    mtime_ms: i64,
    start_offset: u64,
    mode: IngestMode,
    existing_session_id: Option<&str>,
) -> Result<Option<IngestedSession>> {
    let fallback_slug = derive_dsh_fallback_slug(path);
    let mut native_sid: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut step_count = 0_u64;
    let mut user_input_count = 0_u64;
    let mut first_user_prompt = String::new();
    let mut prompt_timestamps_ms = Vec::new();
    let mut raw_spans = Vec::new();
    let mut is_subagent = path.components().any(|c| c.as_os_str() == "subagents");
    let mut parent_session_id: Option<String> = None;
    let mut saw_turn_state = false;
    let mut has_open_turn = false;
    let mut started_at = 0_i64;
    let mut last_active_at = 0_i64;
    let mut seen_usage: HashMap<String, DshUsage> = HashMap::new();
    let mut unkeyed_usage = DshUsage::default();

    let status = stream_jsonl_lines(path, start_offset, |line| {
        let Ok(rec) = serde_json::from_str::<DshLine<'_>>(line) else {
            return Err(());
        };

        if let Some(ref hdr) = rec.header {
            if native_sid.is_none()
                && let Some(ref hid) = hdr.id
                && !hid.is_empty()
            {
                native_sid = Some(hid.to_string());
            }
            if cwd.is_none()
                && let Some(ref hcwd) = hdr.cwd
                && !hcwd.is_empty()
            {
                cwd = Some(hcwd.to_string());
            }
            if let Some(ts) = hdr.timestamp.as_ref().and_then(parse_flexible_ts) {
                update_bounds(&mut started_at, &mut last_active_at, ts);
            }
        }

        if native_sid.is_none()
            && let Some(sid) = rec
                .session_id
                .as_deref()
                .or(rec.session_id_camel.as_deref())
                .or(rec
                    .id
                    .as_deref()
                    .filter(|_| rec.record_type.as_deref() == Some("session")))
            && !sid.is_empty()
        {
            native_sid = Some(sid.to_string());
        }

        if let Some(c) = rec.cwd.as_deref().or(rec.workspace.as_deref())
            && !c.is_empty()
        {
            cwd = Some(c.to_string());
        }

        if rec.is_sidechain.unwrap_or(false) || rec.is_subagent.unwrap_or(false) {
            is_subagent = true;
        }
        if parent_session_id.is_none()
            && let Some(ref pid) = rec.parent_session_id
            && !pid.is_empty()
        {
            is_subagent = true;
            parent_session_id = Some(format!("dsh:{pid}"));
        }

        let ts_ms = rec
            .timestamp
            .as_ref()
            .or(rec.created_at.as_ref())
            .or(rec.ts.as_ref())
            .and_then(parse_flexible_ts);

        if let Some(ts) = ts_ms {
            raw_spans.push(RawSpan::point(ts));
            update_bounds(&mut started_at, &mut last_active_at, ts);
        }

        let rtype = rec.record_type.as_deref().unwrap_or("");
        match rtype {
            "turn_start" | "task_started" => {
                saw_turn_state = true;
                has_open_turn = true;
            }
            "turn_end" | "task_complete" | "turn_aborted" => {
                saw_turn_state = true;
                has_open_turn = false;
            }
            _ => {}
        }

        if !rtype.is_empty() || rec.role.is_some() || rec.message.is_some() {
            step_count += 1;
        }

        if let Some(m) = extract_dsh_model(&rec) {
            model = Some(m);
        }

        if let Some((msg_key, u)) = extract_dsh_usage(&rec) {
            if let Some(key) = msg_key {
                let entry = seen_usage.entry(key).or_default();
                entry.input_tokens = entry.input_tokens.max(u.input_tokens);
                entry.cache_read_tokens = entry.cache_read_tokens.max(u.cache_read_tokens);
                entry.cache_write_tokens = entry.cache_write_tokens.max(u.cache_write_tokens);
                entry.output_tokens = entry.output_tokens.max(u.output_tokens);
                entry.total_tokens = entry.total_tokens.max(u.total_tokens);
            } else {
                unkeyed_usage.input_tokens =
                    unkeyed_usage.input_tokens.saturating_add(u.input_tokens);
                unkeyed_usage.cache_read_tokens = unkeyed_usage
                    .cache_read_tokens
                    .saturating_add(u.cache_read_tokens);
                unkeyed_usage.cache_write_tokens = unkeyed_usage
                    .cache_write_tokens
                    .saturating_add(u.cache_write_tokens);
                unkeyed_usage.output_tokens =
                    unkeyed_usage.output_tokens.saturating_add(u.output_tokens);
                unkeyed_usage.total_tokens =
                    unkeyed_usage.total_tokens.saturating_add(u.total_tokens);
            }
        }

        if let Some(prompt_text) = extract_dsh_user_text(&rec) {
            user_input_count += 1;
            if let Some(ts) = ts_ms {
                prompt_timestamps_ms.push(ts);
            }
            if first_user_prompt.is_empty() {
                first_user_prompt = normalize_prompt(&prompt_text);
            }
        }

        Ok(())
    })?;

    let StreamStatus::Complete { consumed_bytes } = status else {
        return Ok(None);
    };

    let mut input_tokens = unkeyed_usage.input_tokens;
    let mut cache_read_tokens = unkeyed_usage.cache_read_tokens;
    let mut cache_write_tokens = unkeyed_usage.cache_write_tokens;
    let mut output_tokens = unkeyed_usage.output_tokens;
    let mut total_tokens = unkeyed_usage.total_tokens;
    for u in seen_usage.values() {
        input_tokens = input_tokens.saturating_add(u.input_tokens);
        cache_read_tokens = cache_read_tokens.saturating_add(u.cache_read_tokens);
        cache_write_tokens = cache_write_tokens.saturating_add(u.cache_write_tokens);
        output_tokens = output_tokens.saturating_add(u.output_tokens);
        total_tokens = total_tokens.saturating_add(u.total_tokens);
    }

    let id = match existing_session_id {
        Some(existing) if mode == IngestMode::Incremental => existing.to_string(),
        _ => format!("dsh:{}", native_sid.unwrap_or(fallback_slug)),
    };

    Ok(Some(IngestedSession {
        id,
        harness: "dsh".to_string(),
        source_path: path_str.to_string(),
        source_size: consumed_bytes,
        source_mtime_ms: mtime_ms,
        mode,
        cwd,
        model,
        step_count,
        user_input_count,
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        total_tokens,
        usage_is_cumulative: false,
        first_user_prompt,
        prompt_timestamps_ms,
        raw_spans,
        archived: false,
        is_subagent,
        parent_session_id,
        saw_turn_state,
        has_open_turn,
        started_at,
        last_active_at,
    }))
}

fn derive_dsh_fallback_slug(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    if (stem == "session" || stem == "transcript")
        && let Some(parent_name) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
    {
        return parent_name.to_string();
    }
    stem.to_string()
}

fn parse_flexible_ts(val: &Value) -> Option<i64> {
    match val {
        Value::String(s) => {
            parse_rfc3339_ms(s).or_else(|| s.trim().parse::<i64>().ok().map(normalize_epoch_ms))
        }
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .map(normalize_epoch_ms),
        _ => None,
    }
}

fn normalize_epoch_ms(raw: i64) -> i64 {
    if raw <= 0 {
        0
    } else if raw < 10_000_000_000 {
        raw.saturating_mul(1_000)
    } else {
        raw
    }
}

fn extract_dsh_model(rec: &DshLine<'_>) -> Option<String> {
    let candidate = rec
        .model
        .as_deref()
        .or_else(|| {
            rec.message
                .as_ref()
                .and_then(|m| m.get("model"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            rec.payload
                .as_ref()
                .and_then(|p| p.get("model"))
                .and_then(Value::as_str)
        })?;
    let trimmed = candidate.trim();
    if trimmed.is_empty() || trimmed.starts_with('<') {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn extract_dsh_usage(rec: &DshLine<'_>) -> Option<(Option<String>, DshUsage)> {
    let usage_val = rec
        .usage
        .as_ref()
        .or_else(|| rec.message.as_ref().and_then(|m| m.get("usage")))
        .or_else(|| rec.payload.as_ref().and_then(|p| p.get("usage")))?;
    let u = usage_val.as_object()?;

    let get_u64 = |k: &str| u.get(k).and_then(Value::as_u64);
    let cache_read = get_u64("prompt_cache_hit_tokens")
        .or_else(|| {
            u.get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(Value::as_u64)
        })
        .or_else(|| get_u64("cached_input_tokens"))
        .or_else(|| get_u64("cache_read_input_tokens"))
        .or_else(|| get_u64("cache_read_tokens"))
        .unwrap_or(0);
    let cache_write = get_u64("cache_creation_input_tokens")
        .or_else(|| get_u64("cache_write_input_tokens"))
        .or_else(|| get_u64("cache_write_tokens"))
        .unwrap_or(0);

    let input_tokens = if let Some(miss) = get_u64("prompt_cache_miss_tokens") {
        miss
    } else if let Some(prompt_tok) = get_u64("prompt_tokens") {
        prompt_tok.saturating_sub(cache_read)
    } else if let Some(in_tok) = get_u64("input_tokens") {
        if get_u64("cached_input_tokens").is_some() {
            in_tok
                .saturating_sub(cache_read)
                .saturating_sub(cache_write)
        } else {
            in_tok
        }
    } else {
        0
    };

    let output_tokens = get_u64("completion_tokens")
        .or_else(|| get_u64("output_tokens"))
        .unwrap_or(0);

    let computed_sum = input_tokens
        .saturating_add(cache_read)
        .saturating_add(cache_write)
        .saturating_add(output_tokens);
    let total_tokens = get_u64("total_tokens").unwrap_or(0).max(computed_sum);
    if total_tokens == 0 {
        return None;
    }

    let msg_key = rec
        .message
        .as_ref()
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            rec.id
                .as_deref()
                .filter(|s| !s.is_empty() && rec.record_type.as_deref() != Some("session"))
        })
        .map(str::to_string);

    Some((
        msg_key,
        DshUsage {
            input_tokens,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            output_tokens,
            total_tokens,
        },
    ))
}

fn extract_dsh_user_text(rec: &DshLine<'_>) -> Option<String> {
    let rtype = rec.record_type.as_deref().unwrap_or("");
    let top_role = rec.role.as_deref();
    let msg_role = rec
        .message
        .as_ref()
        .and_then(|v| v.get("role"))
        .and_then(Value::as_str);
    let payload_role = rec
        .payload
        .as_ref()
        .and_then(|v| v.get("role"))
        .and_then(Value::as_str);

    let effective_role = top_role.or(msg_role).or(payload_role);
    let is_user_event = match effective_role {
        Some("user") => true,
        Some(_) => false,
        None => matches!(rtype, "user" | "USER_INPUT" | "user_message"),
    };
    if !is_user_event {
        return None;
    }

    if let Some(ref txt) = rec.text {
        let trimmed = txt.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    for candidate in [
        rec.content.as_ref(),
        rec.message
            .as_ref()
            .and_then(|m| m.get("content"))
            .or(rec.message.as_ref()),
        rec.payload
            .as_ref()
            .and_then(|p| p.get("content"))
            .or(rec.payload.as_ref()),
    ] {
        if let Some(val) = candidate
            && let Some(extracted) = extract_text_from_value(val)
        {
            return Some(extracted);
        }
    }

    None
}

pub(crate) fn extract_text_from_value(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Array(items) => {
            let mut joined = String::new();
            for item in items {
                let kind = item.get("type").and_then(Value::as_str).unwrap_or("text");
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
                if let Some(t) = item.get("text").and_then(Value::as_str) {
                    let trimmed = t.trim();
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
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn dsh_ingestor_parses_sessions_and_appends_incrementally() {
        let dir = TempDir::new().unwrap();
        let sess_dir = dir.path().join("dsh-alpha");
        fs::create_dir_all(&sess_dir).unwrap();

        let log_file = sess_dir.join("session.jsonl");
        let initial = concat!(
            r#"{"type":"session","header":{"id":"dsh-101","cwd":"/tmp/dsh-repo","timestamp":"2026-11-14T09:00:00Z"}}"#,
            "\n",
            r#"{"type":"message","role":"user","timestamp":"2026-11-14T09:00:05Z","content":"Inspect DSH session persistence"}"#,
            "\n",
            r#"{"type":"message","id":"m-1","role":"assistant","model":"deepseek-chat","timestamp":"2026-11-14T09:01:00Z","usage":{"prompt_tokens":10000,"prompt_cache_hit_tokens":8000,"prompt_cache_miss_tokens":2000,"completion_tokens":500,"total_tokens":10500},"content":[{"type":"tool_result","text":"giant log ignored"},{"type":"text","text":"Checking schema now."}]}"#,
            "\n",
            r#"{"type":"message","role":"user","timestamp":1794647000000,"content":[{"type":"text","text":"Add burst union tests"}]}"#,
            "\n"
        );
        fs::write(&log_file, initial).unwrap();

        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();
        let mut stats = ScanStats::default();
        let cursors = store.load_file_cursors().unwrap();
        scan_dsh(&mut store, dir.path(), &cursors, &mut stats).unwrap();
        assert_eq!(stats.files_full_scanned, 1);

        let (u_cnt, prompt, cwd): (i64, String, Option<String>) = store
            .connection()
            .query_row(
                "SELECT user_input_count, first_user_prompt, cwd FROM sessions WHERE id = 'dsh:dsh-101'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(u_cnt, 2);
        assert_eq!(prompt, "Inspect DSH session persistence");
        assert_eq!(cwd.as_deref(), Some("/tmp/dsh-repo"));

        {
            let mut fh = OpenOptions::new().append(true).open(&log_file).unwrap();
            writeln!(
                fh,
                r#"{{"type":"message","role":"user","timestamp":"2026-11-14T09:05:00Z","content":"Run cargo test now"}}"#
            )
            .unwrap();
        }

        let mut stats2 = ScanStats::default();
        let cursors2 = store.load_file_cursors().unwrap();
        scan_dsh(&mut store, dir.path(), &cursors2, &mut stats2).unwrap();
        assert_eq!(stats2.files_incremental, 1);

        let (u_cnt2, model2, inp2, cr2, out2, tot2): (i64, Option<String>, i64, i64, i64, i64) =
            store
                .connection()
                .query_row(
                    "SELECT user_input_count, model, input_tokens, cache_read_tokens, output_tokens, total_tokens FROM sessions WHERE id = 'dsh:dsh-101'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )
                .unwrap();
        assert_eq!(u_cnt2, 3);
        assert_eq!(model2.as_deref(), Some("deepseek-chat"));
        assert_eq!(inp2, 2000);
        assert_eq!(cr2, 8000);
        assert_eq!(out2, 500);
        assert_eq!(tot2, 10500);
    }
}
