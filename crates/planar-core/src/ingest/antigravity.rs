use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::burst::RawSpan;
use crate::db::{FileCursor, IngestMode, IngestedSession, Store};
use crate::error::Result;
use crate::ingest::{
    ScanDecision, StreamStatus, decide_scan, file_meta, normalize_prompt, parse_rfc3339_ms,
    scan_mode_for, stream_jsonl_lines, update_bounds,
};
use crate::model::ScanStats;
use serde::Deserialize;

#[derive(Deserialize)]
struct AgRecord<'a> {
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    status: Option<Cow<'a, str>>,
    #[serde(borrow)]
    created_at: Option<Cow<'a, str>>,
    #[serde(borrow)]
    content: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    input_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

/// Scans Antigravity `transcript.jsonl` files under `root` (`~/.gemini/antigravity/brain`).
pub(crate) fn scan_antigravity(
    store: &mut Store,
    root: &Path,
    cursors: &HashMap<String, FileCursor>,
    stats: &mut ScanStats,
) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };

    let mut subagent_links: Vec<(String, String)> = Vec::new();

    for entry in entries.flatten() {
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let Some(conv_id) = dir_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let transcript = dir_path
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        if !transcript.is_file() {
            continue;
        }
        stats.files_discovered += 1;

        let path_str = transcript.to_string_lossy().to_string();
        let Ok((size, mtime_ms)) = file_meta(&transcript) else {
            stats.files_errored += 1;
            continue;
        };

        let decision = decide_scan(&path_str, size, mtime_ms, cursors);
        if decision == ScanDecision::Unchanged {
            stats.files_unchanged += 1;
            continue;
        }

        let (start_offset, mode) = scan_mode_for(decision);
        let session_id = format!("antigravity:{conv_id}");

        let parsed = match parse_antigravity_file(
            &transcript,
            &path_str,
            &session_id,
            conv_id,
            mtime_ms,
            start_offset,
            mode,
            &mut subagent_links,
        ) {
            Ok(Some(res)) => res,
            Ok(None) => {
                if let Ok(Some(res)) = parse_antigravity_file(
                    &transcript,
                    &path_str,
                    &session_id,
                    conv_id,
                    mtime_ms,
                    0,
                    IngestMode::FullReplace,
                    &mut subagent_links,
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

    if !subagent_links.is_empty() {
        stats.subagents_linked += store.mark_subagent_sessions(&subagent_links)?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_antigravity_file(
    path: &Path,
    path_str: &str,
    session_id: &str,
    conv_id: &str,
    mtime_ms: i64,
    start_offset: u64,
    mode: IngestMode,
    subagent_links: &mut Vec<(String, String)>,
) -> Result<Option<IngestedSession>> {
    let mut cwd: Option<String> = None;
    let mut step_count = 0_u64;
    let mut user_input_count = 0_u64;
    let mut first_user_prompt = String::new();
    let mut prompt_timestamps_ms = Vec::new();
    let mut raw_spans = Vec::new();
    let mut saw_turn_state = false;
    let mut has_open_turn = false;
    let mut started_at = 0_i64;
    let mut last_active_at = 0_i64;
    let mut model: Option<String> = None;
    let mut input_tokens = 0_u64;
    let mut cache_read_tokens = 0_u64;
    let mut cache_write_tokens = 0_u64;
    let mut output_tokens = 0_u64;

    let status = stream_jsonl_lines(path, start_offset, |line| {
        let Ok(rec) = serde_json::from_str::<AgRecord<'_>>(line) else {
            return Err(());
        };

        step_count += 1;

        if let Some(ref m) = rec.model {
            let trimmed = m.trim();
            if !trimmed.is_empty() {
                model = Some(trimmed.to_string());
            }
        }
        input_tokens = input_tokens.saturating_add(rec.input_tokens.unwrap_or(0));
        cache_read_tokens = cache_read_tokens.saturating_add(rec.cache_read_tokens.unwrap_or(0));
        cache_write_tokens = cache_write_tokens.saturating_add(rec.cache_write_tokens.unwrap_or(0));
        output_tokens = output_tokens.saturating_add(rec.output_tokens.unwrap_or(0));

        if let Some(ref st) = rec.status {
            saw_turn_state = true;
            has_open_turn = st.as_ref() == "RUNNING";
        }

        let ts_ms = rec.created_at.as_deref().and_then(parse_rfc3339_ms);
        if let Some(ts) = ts_ms {
            raw_spans.push(RawSpan::point(ts));
            update_bounds(&mut started_at, &mut last_active_at, ts);
        }

        let rtype = rec.record_type.as_deref().unwrap_or("");
        if let Some(ref content) = rec.content {
            if rtype == "USER_INPUT" {
                let prompt = extract_antigravity_user_request(content);
                if !prompt.is_empty() {
                    user_input_count += 1;
                    if let Some(ts) = ts_ms {
                        prompt_timestamps_ms.push(ts);
                    }
                    if first_user_prompt.is_empty() {
                        first_user_prompt = normalize_prompt(&prompt);
                    }
                }
            }

            if cwd.is_none() && (rtype == "USER_INPUT" || rtype == "SYSTEM_MESSAGE") {
                cwd = extract_cwd_hint(content);
            }

            if content.contains("conversationId") {
                for child_uuid in extract_conversation_ids(content) {
                    if child_uuid != conv_id {
                        subagent_links
                            .push((format!("antigravity:{child_uuid}"), session_id.to_string()));
                    }
                }
            }
        }

        Ok(())
    })?;

    let StreamStatus::Complete { consumed_bytes } = status else {
        return Ok(None);
    };

    let total_tokens = input_tokens
        .saturating_add(cache_read_tokens)
        .saturating_add(cache_write_tokens)
        .saturating_add(output_tokens);

    Ok(Some(IngestedSession {
        id: session_id.to_string(),
        harness: "antigravity".to_string(),
        source_path: path_str.to_string(),
        source_size: consumed_bytes,
        source_mtime_ms: mtime_ms,
        mode,
        cwd,
        step_count,
        user_input_count,
        first_user_prompt,
        prompt_timestamps_ms,
        raw_spans,
        archived: false,
        is_subagent: false,
        parent_session_id: None,
        saw_turn_state,
        has_open_turn,
        started_at,
        last_active_at,
        model,
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        total_tokens,
        usage_is_cumulative: false,
    }))
}

pub(crate) fn extract_antigravity_user_request(content: &str) -> String {
    const OPEN: &str = "<USER_REQUEST>";
    const CLOSE: &str = "</USER_REQUEST>";

    if let Some(start) = content.find(OPEN) {
        let after = &content[start + OPEN.len()..];
        let inner = after.find(CLOSE).map_or(after, |end| &after[..end]);
        return inner.trim().to_string();
    }

    // Fallback when <USER_REQUEST> tags are absent: strip appended metadata blocks.
    let mut cleaned = content;
    for marker in ["<ADDITIONAL_METADATA>", "<USER_SETTINGS_CHANGE>"] {
        if let Some(idx) = cleaned.find(marker) {
            cleaned = &cleaned[..idx];
        }
    }
    cleaned.trim().to_string()
}

fn extract_cwd_hint(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Command Working Directory:") {
            let path = rest.trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

fn extract_conversation_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let needle = "\"conversationId\"";
    let mut rest = text;

    while let Some(pos) = rest.find(needle) {
        rest = &rest[pos + needle.len()..];
        let trimmed = rest.trim_start();
        let Some(after_colon) = trimmed.strip_prefix(':') else {
            continue;
        };
        let after_colon = after_colon.trim_start();
        let Some(after_quote) = after_colon.strip_prefix('"') else {
            continue;
        };
        let Some(end_quote) = after_quote.find('"') else {
            continue;
        };
        let candidate = &after_quote[..end_quote];
        if looks_like_uuid(candidate) {
            ids.push(candidate.to_string());
        }
        rest = &after_quote[end_quote + 1..];
    }

    ids
}

fn looks_like_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn antigravity_ingestor_extracts_user_request_and_links_subagents() {
        let dir = TempDir::new().unwrap();
        let parent_id = "11111111-1111-1111-1111-111111111111";
        let child_id = "22222222-2222-2222-2222-222222222222";

        let parent_logs = dir
            .path()
            .join(parent_id)
            .join(".system_generated")
            .join("logs");
        let child_logs = dir
            .path()
            .join(child_id)
            .join(".system_generated")
            .join("logs");
        fs::create_dir_all(&parent_logs).unwrap();
        fs::create_dir_all(&child_logs).unwrap();

        let parent_jsonl = concat!(
            r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-14T02:00:00Z","content":"<USER_REQUEST>\nBuild Planar Rust ingestor\n</USER_REQUEST>\n<ADDITIONAL_METADATA>ignored</ADDITIONAL_METADATA>"}"#,
            "\n",
            r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-14T02:01:00Z","input_tokens":4200,"cache_read_tokens":20000,"output_tokens":800,"content":"Subagent finished: {\"conversationId\": \"22222222-2222-2222-2222-222222222222\"}"}"#,
            "\n"
        );
        fs::write(parent_logs.join("transcript.jsonl"), parent_jsonl).unwrap();

        let child_jsonl = concat!(
            r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-14T02:00:10Z","content":"<USER_REQUEST>\nInternal subagent prompt\n</USER_REQUEST>"}"#,
            "\n",
            r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-14T02:00:25Z","input_tokens":1000,"cache_read_tokens":5000,"output_tokens":300,"content":"Done"}"#,
            "\n"
        );
        fs::write(child_logs.join("transcript.jsonl"), child_jsonl).unwrap();

        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();
        let mut stats = ScanStats::default();
        let cursors = store.load_file_cursors().unwrap();
        scan_antigravity(&mut store, dir.path(), &cursors, &mut stats).unwrap();

        assert_eq!(stats.files_full_scanned, 2);
        assert_eq!(stats.subagents_linked, 1);

        let (prompt, p_sub, p_tot): (String, i64, i64) = store
            .connection()
            .query_row(
                "SELECT first_user_prompt, is_subagent, total_tokens FROM sessions WHERE id = ?1",
                [format!("antigravity:{parent_id}")],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(prompt, "Build Planar Rust ingestor");
        assert_eq!(p_sub, 0);
        // Antigravity input_tokens (4200) is uncached additive: 4200 + 20000 + 800 = 25000
        assert_eq!(p_tot, 25_000);

        let (c_sub, c_tot): (i64, i64) = store
            .connection()
            .query_row(
                "SELECT is_subagent, total_tokens FROM sessions WHERE id = ?1",
                [format!("antigravity:{child_id}")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(c_sub, 1);
        assert_eq!(c_tot, 6_300);
    }
}
