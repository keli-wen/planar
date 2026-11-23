use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::value::RawValue;
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
struct ClaudeLine<'a> {
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
    #[serde(rename = "sessionId", borrow)]
    session_id: Option<Cow<'a, str>>,
    #[serde(rename = "requestId", borrow)]
    request_id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow)]
    message: Option<&'a RawValue>,
}

#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy, Default, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Deserialize)]
struct ClaudeAssistantMeta<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    usage: Option<ClaudeUsage>,
}

#[derive(Deserialize)]
struct ClaudeMessage<'a> {
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
    #[serde(borrow)]
    content: Option<ClaudeContent<'a>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ClaudeContent<'a> {
    Text(#[serde(borrow)] Cow<'a, str>),
    Blocks(#[serde(borrow)] Vec<ClaudeBlock<'a>>),
}

#[derive(Deserialize)]
struct ClaudeBlock<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

/// Scans Claude Code JSONL transcripts under `root` (`~/.claude/projects/**/*.jsonl`).
pub(crate) fn scan_claude(
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
        let Ok((_size, mtime_ms)) = file_meta(path) else {
            stats.files_errored += 1;
            continue;
        };
        let Ok(actual_size) = std::fs::metadata(path).map(|m| m.len()) else {
            stats.files_errored += 1;
            continue;
        };

        let decision = decide_scan(&path_str, actual_size, mtime_ms, cursors);
        if decision == ScanDecision::Unchanged {
            stats.files_unchanged += 1;
            continue;
        }

        let (start_offset, mode) = scan_mode_for(decision);
        let existing_id = cursors.get(&path_str).map(|c| c.session_id.as_str());

        let parsed =
            match parse_claude_file(path, &path_str, mtime_ms, start_offset, mode, existing_id) {
                Ok(Some(res)) => res,
                Ok(None) => {
                    if let Ok(Some(res)) = parse_claude_file(
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

fn parse_claude_file(
    path: &Path,
    path_str: &str,
    mtime_ms: i64,
    start_offset: u64,
    mode: IngestMode,
    existing_session_id: Option<&str>,
) -> Result<Option<IngestedSession>> {
    let is_subagent_path = is_claude_subagent_path(path);
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let mut native_sid: Option<String> = None;
    let mut parent_sid: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut step_count = 0_u64;
    let mut user_input_count = 0_u64;
    let mut first_user_prompt = String::new();
    let mut prompt_timestamps_ms = Vec::new();
    let mut raw_spans = Vec::new();
    let mut is_sidechain_seen = is_subagent_path;
    let mut started_at = 0_i64;
    let mut last_active_at = 0_i64;
    let mut seen_usage: HashMap<String, ClaudeUsage> = HashMap::new();
    let mut unkeyed_usage = ClaudeUsage::default();

    let status = stream_jsonl_lines(path, start_offset, |line| {
        let Ok(rec) = serde_json::from_str::<ClaudeLine<'_>>(line) else {
            return Err(());
        };

        let rtype = rec.record_type.as_deref().unwrap_or("");
        if matches!(rtype, "file-history-snapshot" | "summary") {
            return Ok(());
        }

        if let Some(ref sid) = rec.session_id
            && !sid.is_empty()
        {
            if is_subagent_path {
                if parent_sid.is_none() {
                    parent_sid = Some(format!("claude:{sid}"));
                }
            } else if native_sid.is_none() {
                native_sid = Some(sid.to_string());
            }
        }

        if let Some(ref c) = rec.cwd
            && !c.is_empty()
        {
            cwd = Some(c.to_string());
        }

        if rec.is_sidechain.unwrap_or(false) {
            is_sidechain_seen = true;
        }

        let ts_ms = rec.timestamp.as_deref().and_then(parse_rfc3339_ms);
        if let Some(ts) = ts_ms {
            raw_spans.push(RawSpan::point(ts));
            update_bounds(&mut started_at, &mut last_active_at, ts);
        }

        if !rtype.is_empty() {
            step_count += 1;
        }

        if rtype == "assistant"
            && let Some(msg_raw) = rec.message
            && let Ok(meta) = serde_json::from_str::<ClaudeAssistantMeta<'_>>(msg_raw.get())
        {
            if let Some(ref m) = meta.model {
                let trimmed = m.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('<') {
                    model = Some(trimmed.to_string());
                }
            }
            if let Some(u) = meta.usage {
                let key_opt = meta
                    .id
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or_else(|| rec.request_id.as_deref().filter(|s| !s.is_empty()));
                if let Some(key) = key_opt {
                    let entry = seen_usage.entry(key.to_string()).or_default();
                    entry.input_tokens = entry.input_tokens.max(u.input_tokens);
                    entry.cache_creation_input_tokens = entry
                        .cache_creation_input_tokens
                        .max(u.cache_creation_input_tokens);
                    entry.cache_read_input_tokens =
                        entry.cache_read_input_tokens.max(u.cache_read_input_tokens);
                    entry.output_tokens = entry.output_tokens.max(u.output_tokens);
                } else {
                    unkeyed_usage.input_tokens =
                        unkeyed_usage.input_tokens.saturating_add(u.input_tokens);
                    unkeyed_usage.cache_creation_input_tokens = unkeyed_usage
                        .cache_creation_input_tokens
                        .saturating_add(u.cache_creation_input_tokens);
                    unkeyed_usage.cache_read_input_tokens = unkeyed_usage
                        .cache_read_input_tokens
                        .saturating_add(u.cache_read_input_tokens);
                    unkeyed_usage.output_tokens =
                        unkeyed_usage.output_tokens.saturating_add(u.output_tokens);
                }
            }
        }

        if rtype == "user"
            && !rec.is_sidechain.unwrap_or(false)
            && !rec.is_meta.unwrap_or(false)
            && let Some(msg_raw) = rec.message
            && let Ok(msg) = serde_json::from_str::<ClaudeMessage<'_>>(msg_raw.get())
            && msg.role.as_deref().unwrap_or("user") == "user"
            && let Some(human_text) = extract_claude_human_text(msg.content)
        {
            user_input_count += 1;
            if let Some(ts) = ts_ms {
                prompt_timestamps_ms.push(ts);
            }
            if first_user_prompt.is_empty() {
                first_user_prompt = normalize_prompt(&human_text);
            }
        }

        Ok(())
    })?;

    let StreamStatus::Complete { consumed_bytes } = status else {
        return Ok(None);
    };

    let mut input_tokens = unkeyed_usage.input_tokens;
    let mut cache_write_tokens = unkeyed_usage.cache_creation_input_tokens;
    let mut cache_read_tokens = unkeyed_usage.cache_read_input_tokens;
    let mut output_tokens = unkeyed_usage.output_tokens;
    for u in seen_usage.values() {
        input_tokens = input_tokens.saturating_add(u.input_tokens);
        cache_write_tokens = cache_write_tokens.saturating_add(u.cache_creation_input_tokens);
        cache_read_tokens = cache_read_tokens.saturating_add(u.cache_read_input_tokens);
        output_tokens = output_tokens.saturating_add(u.output_tokens);
    }
    let total_tokens = input_tokens
        .saturating_add(cache_write_tokens)
        .saturating_add(cache_read_tokens)
        .saturating_add(output_tokens);

    let id = match existing_session_id {
        Some(existing) if mode == IngestMode::Incremental => existing.to_string(),
        _ => {
            if is_subagent_path {
                let p_slug = parent_sid
                    .as_deref()
                    .and_then(|s| s.strip_prefix("claude:"))
                    .unwrap_or("sub");
                format!("claude:{p_slug}:{file_stem}")
            } else {
                let sid = native_sid.unwrap_or_else(|| file_stem.to_string());
                format!("claude:{sid}")
            }
        }
    };

    Ok(Some(IngestedSession {
        id,
        harness: "claude".to_string(),
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
        is_subagent: is_sidechain_seen,
        parent_session_id: parent_sid,
        saw_turn_state: false,
        has_open_turn: false,
        started_at,
        last_active_at,
    }))
}

fn is_claude_subagent_path(path: &Path) -> bool {
    if path.components().any(|c| c.as_os_str() == "subagents") {
        return true;
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem.starts_with("agent-"))
}

fn extract_claude_human_text(content: Option<ClaudeContent<'_>>) -> Option<String> {
    let raw = match content? {
        ClaudeContent::Text(t) => t.into_owned(),
        ClaudeContent::Blocks(blocks) => {
            let mut joined = String::new();
            for b in blocks {
                if b.kind.as_deref() != Some("text") {
                    continue;
                }
                if let Some(txt) = b.text {
                    let trimmed = txt.trim();
                    if trimmed.is_empty() || is_claude_synthetic_wrapper(trimmed) {
                        continue;
                    }
                    if !joined.is_empty() {
                        joined.push(' ');
                    }
                    joined.push_str(trimmed);
                }
            }
            joined
        }
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() || is_claude_synthetic_wrapper(trimmed) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn is_claude_synthetic_wrapper(trimmed: &str) -> bool {
    const PREFIXES: [&str; 6] = [
        "<local-command-",
        "<command-name>",
        "<command-message>",
        "<command-args>",
        "<task-notification>",
        "<system-reminder>",
    ];
    PREFIXES.iter().any(|p| trimmed.starts_with(p))
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn claude_ingestor_filters_tool_results_and_subagents_and_appends_incrementally() {
        let dir = TempDir::new().unwrap();
        let proj = dir.path().join("project-alpha");
        let sub_dir = proj.join("sess-1").join("subagents");
        fs::create_dir_all(&sub_dir).unwrap();

        let main_file = proj.join("sess-1.jsonl");
        let initial_lines = concat!(
            r#"{"type":"user","isSidechain":false,"sessionId":"sess-1","cwd":"/tmp/ws","timestamp":"2026-11-14T08:00:00Z","message":{"role":"user","content":"First human prompt"}}"#,
            "\n",
            r#"{"type":"assistant","isSidechain":false,"sessionId":"sess-1","requestId":"req-1","timestamp":"2026-11-14T08:00:50Z","message":{"id":"msg-1","model":"claude-sonnet-4-6","role":"assistant","usage":{"input_tokens":100,"cache_creation_input_tokens":200,"cache_read_input_tokens":1000,"output_tokens":5},"content":[{"type":"thinking","thinking":"..."}]}}"#,
            "\n",
            r#"{"type":"assistant","isSidechain":false,"sessionId":"sess-1","requestId":"req-1","timestamp":"2026-11-14T08:01:00Z","message":{"id":"msg-1","model":"claude-sonnet-4-6","role":"assistant","usage":{"input_tokens":100,"cache_creation_input_tokens":200,"cache_read_input_tokens":1000,"output_tokens":80},"content":[{"type":"tool_use","name":"bash"}]}}"#,
            "\n",
            r#"{"type":"user","isSidechain":false,"sessionId":"sess-1","timestamp":"2026-11-14T08:01:30Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"massive output ignored"}]}}"#,
            "\n",
            r#"{"type":"user","isSidechain":false,"sessionId":"sess-1","timestamp":"2026-11-14T08:02:00Z","message":{"role":"user","content":[{"type":"text","text":"Second human turn"}]}}"#,
            "\n"
        );
        fs::write(&main_file, initial_lines).unwrap();

        // Synthetic subagent transcript under subagents/ sharing sessionId
        let sub_file = sub_dir.join("agent-sub1.jsonl");
        fs::write(
            &sub_file,
            concat!(
                r#"{"type":"user","isSidechain":true,"sessionId":"sess-1","timestamp":"2026-11-14T08:01:10Z","message":{"role":"user","content":"Subagent task"}}"#,
                "\n"
            ),
        )
        .unwrap();

        let db_path = dir.path().join("planar.db");
        let mut store = Store::open(&db_path).unwrap();

        let mut stats = ScanStats::default();
        let cursors = store.load_file_cursors().unwrap();
        scan_claude(&mut store, dir.path(), &cursors, &mut stats).unwrap();
        assert_eq!(stats.files_full_scanned, 2);

        let (count, first_prompt, is_sub): (i64, String, i64) = store
            .connection()
            .query_row(
                "SELECT user_input_count, first_user_prompt, is_subagent FROM sessions WHERE id = 'claude:sess-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        // tool_result-only user line must NOT increment user_input_count
        assert_eq!(count, 2);
        assert_eq!(first_prompt, "First human prompt");
        assert_eq!(is_sub, 0);

        let sub_flag: i64 = store
            .connection()
            .query_row(
                "SELECT is_subagent FROM sessions WHERE id = 'claude:sess-1:agent-sub1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sub_flag, 1);

        // Append a 3rd human prompt so session grows from short (2) to unsummarized (3)
        {
            let mut fh = OpenOptions::new().append(true).open(&main_file).unwrap();
            writeln!(
                fh,
                r#"{{"type":"user","isSidechain":false,"sessionId":"sess-1","timestamp":"2026-11-14T08:03:00Z","message":{{"role":"user","content":"Third human turn"}}}}"#
            )
            .unwrap();
        }

        let mut stats2 = ScanStats::default();
        let cursors2 = store.load_file_cursors().unwrap();
        scan_claude(&mut store, dir.path(), &cursors2, &mut stats2).unwrap();
        assert_eq!(stats2.files_incremental, 1);
        assert_eq!(stats2.files_unchanged, 1);

        let (count2, steps2, model2, total_tok2): (i64, i64, Option<String>, i64) = store
            .connection()
            .query_row(
                "SELECT user_input_count, step_count, model, total_tokens FROM sessions WHERE id = 'claude:sess-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(count2, 3);
        assert_eq!(steps2, 6);
        assert_eq!(model2.as_deref(), Some("claude-sonnet-4-6"));
        // Streaming chunks sharing msg-1 deduplicate to 100 + 200 + 1000 + 80 = 1380
        assert_eq!(total_tok2, 1380);
    }
}
