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
struct CodexLine<'a> {
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(rename = "type", borrow)]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct CodexSessionMeta<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    #[serde(borrow)]
    parent_thread_id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    source: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct CodexSourceSubagent<'a> {
    #[serde(borrow)]
    subagent: Option<CodexSubagentKind<'a>>,
}

#[derive(Deserialize)]
struct CodexSubagentKind<'a> {
    #[serde(borrow)]
    thread_spawn: Option<CodexThreadSpawn<'a>>,
}

#[derive(Deserialize)]
struct CodexThreadSpawn<'a> {
    #[serde(borrow)]
    parent_thread_id: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct CodexTurnContext<'a> {
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct CodexResponseHeader<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct CodexUserMessagePayload<'a> {
    #[serde(borrow)]
    content: Option<Vec<CodexContentPart<'a>>>,
}

#[derive(Deserialize)]
struct CodexContentPart<'a> {
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy, Default, Deserialize)]
struct CodexUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

#[derive(Deserialize)]
struct CodexTokenCountInfo {
    total_token_usage: Option<CodexUsage>,
}

#[derive(Deserialize)]
struct CodexTokenUsageRecord {
    usage: Option<CodexUsage>,
}

#[derive(Deserialize)]
struct CodexEventMsg<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
    info: Option<CodexTokenCountInfo>,
}

/// Scans Codex JSONL rollouts under `root` (`~/.codex/sessions` or `~/.codex/archived_sessions`).
pub(crate) fn scan_codex(
    store: &mut Store,
    root: &Path,
    archived: bool,
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

        let parsed = match parse_codex_file(
            path,
            &path_str,
            archived,
            mtime_ms,
            start_offset,
            mode,
            existing_id,
        ) {
            Ok(Some(res)) => res,
            Ok(None) => {
                if let Ok(Some(res)) = parse_codex_file(
                    path,
                    &path_str,
                    archived,
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

fn parse_codex_file(
    path: &Path,
    path_str: &str,
    archived: bool,
    mtime_ms: i64,
    start_offset: u64,
    mode: IngestMode,
    existing_session_id: Option<&str>,
) -> Result<Option<IngestedSession>> {
    let mut native_sid: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut step_count = 0_u64;
    let mut user_input_count = 0_u64;
    let mut first_user_prompt = String::new();
    let mut prompt_timestamps_ms = Vec::new();
    let mut raw_spans = Vec::new();
    let mut is_subagent = false;
    let mut parent_session_id: Option<String> = None;
    let mut saw_turn_state = false;
    let mut has_open_turn = false;
    let mut started_at = 0_i64;
    let mut last_active_at = 0_i64;
    let mut latest_cumulative: Option<CodexUsage> = None;
    let mut delta_usage = CodexUsage::default();

    let status = stream_jsonl_lines(path, start_offset, |line| {
        let Ok(rec) = serde_json::from_str::<CodexLine<'_>>(line) else {
            return Err(());
        };
        let rtype = rec.record_type.as_deref().unwrap_or("");
        let ts_ms = rec.timestamp.as_deref().and_then(parse_rfc3339_ms);

        match rtype {
            "session_meta" => {
                if let Some(pl_raw) = rec.payload
                    && let Ok(meta) = serde_json::from_str::<CodexSessionMeta<'_>>(pl_raw.get())
                {
                    if native_sid.is_none()
                        && let Some(ref id) = meta.id
                        && !id.is_empty()
                    {
                        native_sid = Some(id.to_string());
                    }
                    if let Some(ref c) = meta.cwd
                        && !c.is_empty()
                    {
                        cwd = Some(c.to_string());
                    }
                    if let Some(ref m) = meta.model
                        && !m.is_empty()
                    {
                        model = Some(m.to_string());
                    }
                    if let Some(ref ptid) = meta.parent_thread_id
                        && !ptid.is_empty()
                    {
                        is_subagent = true;
                        parent_session_id = Some(format!("codex:{ptid}"));
                    }
                    if let Some(src_raw) = meta.source {
                        let raw_txt = src_raw.get().trim_start();
                        if raw_txt.starts_with('{') && raw_txt.contains("subagent") {
                            is_subagent = true;
                            if parent_session_id.is_none()
                                && let Ok(src) =
                                    serde_json::from_str::<CodexSourceSubagent<'_>>(raw_txt)
                                && let Some(ptid) = src
                                    .subagent
                                    .and_then(|s| s.thread_spawn)
                                    .and_then(|ts| ts.parent_thread_id)
                                && !ptid.is_empty()
                            {
                                parent_session_id = Some(format!("codex:{ptid}"));
                            }
                        }
                    }
                }
            }
            "turn_context" => {
                if let Some(pl_raw) = rec.payload
                    && let Ok(tc) = serde_json::from_str::<CodexTurnContext<'_>>(pl_raw.get())
                {
                    if let Some(ref c) = tc.cwd
                        && !c.is_empty()
                    {
                        cwd = Some(c.to_string());
                    }
                    if let Some(ref m) = tc.model
                        && !m.is_empty()
                    {
                        model = Some(m.to_string());
                    }
                }
            }
            "response_item" => {
                step_count += 1;
                if let Some(ts) = ts_ms {
                    raw_spans.push(RawSpan::point(ts));
                    update_bounds(&mut started_at, &mut last_active_at, ts);
                }

                if let Some(pl_raw) = rec.payload
                    && let Ok(hdr) = serde_json::from_str::<CodexResponseHeader<'_>>(pl_raw.get())
                    && hdr.kind.as_deref() == Some("message")
                    && hdr.role.as_deref() == Some("user")
                    && let Ok(msg) =
                        serde_json::from_str::<CodexUserMessagePayload<'_>>(pl_raw.get())
                    && let Some(human_text) = extract_codex_human_text(msg.content)
                {
                    user_input_count += 1;
                    if let Some(ts) = ts_ms {
                        prompt_timestamps_ms.push(ts);
                    }
                    if first_user_prompt.is_empty() {
                        first_user_prompt = normalize_prompt(&human_text);
                    }
                }
            }
            "event_msg" => {
                if let Some(pl_raw) = rec.payload
                    && let Ok(ev) = serde_json::from_str::<CodexEventMsg<'_>>(pl_raw.get())
                {
                    match ev.kind.as_deref().unwrap_or("") {
                        "task_started" => {
                            step_count += 1;
                            saw_turn_state = true;
                            has_open_turn = true;
                            if let Some(ts) = ts_ms {
                                raw_spans.push(RawSpan::point(ts));
                                update_bounds(&mut started_at, &mut last_active_at, ts);
                            }
                        }
                        "task_complete" | "turn_aborted" => {
                            step_count += 1;
                            saw_turn_state = true;
                            has_open_turn = false;
                            if let Some(ts) = ts_ms {
                                raw_spans.push(RawSpan::point(ts));
                                update_bounds(&mut started_at, &mut last_active_at, ts);
                            }
                        }
                        "item_completed" => {
                            step_count += 1;
                            match (ev.started_at_ms, ev.completed_at_ms) {
                                (Some(s_ms), Some(e_ms)) if s_ms > 0 => {
                                    let clamped = RawSpan::clamped(s_ms, e_ms);
                                    raw_spans.push(clamped);
                                    update_bounds(
                                        &mut started_at,
                                        &mut last_active_at,
                                        clamped.start_ms,
                                    );
                                    update_bounds(
                                        &mut started_at,
                                        &mut last_active_at,
                                        clamped.end_ms,
                                    );
                                }
                                _ => {
                                    if let Some(ts) = ts_ms {
                                        raw_spans.push(RawSpan::point(ts));
                                        update_bounds(&mut started_at, &mut last_active_at, ts);
                                    }
                                }
                            }
                        }
                        "token_count" => {
                            if let Some(u) = ev.info.and_then(|i| i.total_token_usage) {
                                latest_cumulative = Some(u);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "token_usage_record" => {
                if let Some(pl_raw) = rec.payload
                    && let Ok(tr) = serde_json::from_str::<CodexTokenUsageRecord>(pl_raw.get())
                    && let Some(u) = tr.usage
                {
                    delta_usage.input_tokens =
                        delta_usage.input_tokens.saturating_add(u.input_tokens);
                    delta_usage.cached_input_tokens = delta_usage
                        .cached_input_tokens
                        .saturating_add(u.cached_input_tokens);
                    delta_usage.cache_write_input_tokens = delta_usage
                        .cache_write_input_tokens
                        .saturating_add(u.cache_write_input_tokens);
                    delta_usage.output_tokens =
                        delta_usage.output_tokens.saturating_add(u.output_tokens);
                    delta_usage.total_tokens =
                        delta_usage.total_tokens.saturating_add(u.total_tokens);
                }
            }
            _ => {
                // Skip world_state, etc. without parsing payload.
            }
        }

        Ok(())
    })?;

    let StreamStatus::Complete { consumed_bytes } = status else {
        return Ok(None);
    };

    let (eff_usage, usage_is_cumulative) = match latest_cumulative {
        Some(u) => (u, true),
        None => (delta_usage, false),
    };
    // OpenAI / Codex `input_tokens` includes `cached_input_tokens`.
    let input_tokens = eff_usage
        .input_tokens
        .saturating_sub(eff_usage.cached_input_tokens)
        .saturating_sub(eff_usage.cache_write_input_tokens);
    let cache_read_tokens = eff_usage.cached_input_tokens;
    let cache_write_tokens = eff_usage.cache_write_input_tokens;
    let output_tokens = eff_usage.output_tokens;
    let computed_sum = input_tokens
        .saturating_add(cache_read_tokens)
        .saturating_add(cache_write_tokens)
        .saturating_add(output_tokens);
    let total_tokens = eff_usage.total_tokens.max(computed_sum);

    let id = match existing_session_id {
        Some(existing) if mode == IngestMode::Incremental => existing.to_string(),
        _ => {
            let sid = native_sid.unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });
            format!("codex:{sid}")
        }
    };

    Ok(Some(IngestedSession {
        id,
        harness: "codex".to_string(),
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
        usage_is_cumulative,
        first_user_prompt,
        prompt_timestamps_ms,
        raw_spans,
        archived,
        is_subagent,
        parent_session_id,
        saw_turn_state,
        has_open_turn,
        started_at,
        last_active_at,
    }))
}

fn extract_codex_human_text(content: Option<Vec<CodexContentPart<'_>>>) -> Option<String> {
    let parts = content?;
    let mut joined = String::new();

    for part in parts {
        let Some(txt) = part.text else {
            continue;
        };
        let trimmed = txt.trim();
        if trimmed.is_empty() || is_codex_synthetic_block(trimmed) {
            continue;
        }
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(trimmed);
    }

    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

pub(crate) fn is_codex_synthetic_block(trimmed: &str) -> bool {
    const PREFIXES: [&str; 11] = [
        "# AGENTS.md instructions",
        "<INSTRUCTIONS>",
        "<environment_context",
        "<permissions instructions",
        "<collaboration_mode",
        "<subagent_notification",
        "<system-reminder",
        "<turn_aborted",
        "<user_instructions>",
        "<skill>",
        "<skill ",
    ];
    PREFIXES.iter().any(|p| trimmed.starts_with(p))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn codex_ingestor_strips_synthetic_blocks_detects_subagents_and_preserves_item_spans() {
        let dir = TempDir::new().unwrap();
        let sess_dir = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("11")
            .join("14");
        let arch_dir = dir.path().join("archived_sessions");
        fs::create_dir_all(&sess_dir).unwrap();
        fs::create_dir_all(&arch_dir).unwrap();

        let main_file = sess_dir.join("rollout-main.jsonl");
        let main_content = concat!(
            r#"{"timestamp":"2026-11-14T01:00:00Z","type":"session_meta","payload":{"id":"codex-main-1","cwd":"/tmp/planar","source":"vscode"}}"#,
            "\n",
            r##"{"timestamp":"2026-11-14T01:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /tmp/planar\n..."},{"type":"input_text","text":"<environment_context><cwd>/tmp/planar</cwd></environment_context>"}]}}"##,
            "\n",
            r#"{"timestamp":"2026-11-14T01:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<skill>\n<name>demo</name>\n</skill>"},{"type":"input_text","text":"[cospan delegation context]\n- Workspace: /tmp/planar\n\n[Task]\nImplement streaming Codex parser"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-11-14T01:05:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<subagent_notification>{\"agent_id\":\"x\"}</subagent_notification>"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-11-14T01:22:00Z","type":"event_msg","payload":{"type":"item_completed","started_at_ms":1794618120000,"completed_at_ms":1794619320000}}"#,
            "\n",
            r#"{"timestamp":"2026-11-14T01:43:00Z","type":"response_item","payload":{"type":"custom_tool_call_output","output":[{"text":"giant payload ignored"}]}}"#,
            "\n"
        );
        fs::write(&main_file, main_content).unwrap();

        let sub_file = sess_dir.join("rollout-sub.jsonl");
        let sub_content = concat!(
            r#"{"timestamp":"2026-11-14T01:02:00Z","type":"session_meta","payload":{"id":"codex-sub-1","parent_thread_id":"codex-main-1","source":{"subagent":{"thread_spawn":{"parent_thread_id":"codex-main-1"}}}}}"#,
            "\n",
            r#"{"timestamp":"2026-11-14T01:02:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Subagent prompt"}]}}"#,
            "\n"
        );
        fs::write(&sub_file, sub_content).unwrap();

        let arch_file = arch_dir.join("rollout-arch.jsonl");
        let arch_content = concat!(
            r#"{"timestamp":"2026-11-13T10:00:00Z","type":"session_meta","payload":{"id":"codex-arch-1","source":"exec"}}"#,
            "\n",
            r#"{"timestamp":"2026-11-13T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Archived task"}]}}"#,
            "\n"
        );
        fs::write(&arch_file, arch_content).unwrap();

        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();
        let mut stats = ScanStats::default();
        let cursors = store.load_file_cursors().unwrap();
        scan_codex(
            &mut store,
            &dir.path().join("sessions"),
            false,
            &cursors,
            &mut stats,
        )
        .unwrap();
        scan_codex(
            &mut store,
            &dir.path().join("archived_sessions"),
            true,
            &cursors,
            &mut stats,
        )
        .unwrap();
        assert_eq!(stats.files_full_scanned, 3);

        let (u_cnt, prompt, arch): (i64, String, i64) = store
            .connection()
            .query_row(
                "SELECT user_input_count, first_user_prompt, archived FROM sessions WHERE id = 'codex:codex-main-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(u_cnt, 1);
        assert_eq!(prompt, "Implement streaming Codex parser");
        assert_eq!(arch, 0);

        let iv_count: i64 = store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM activity_intervals WHERE session_id = 'codex:codex-main-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // 01:00..01:05 is one burst; gap to 01:22 is 17m (> 12m), then 01:22..01:42 (20m tool span) merges with 01:43 -> 2 bursts
        assert_eq!(iv_count, 2);

        let sub_flag: i64 = store
            .connection()
            .query_row(
                "SELECT is_subagent FROM sessions WHERE id = 'codex:codex-sub-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sub_flag, 1);

        let arch_flag: i64 = store
            .connection()
            .query_row(
                "SELECT archived FROM sessions WHERE id = 'codex:codex-arch-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(arch_flag, 1);
    }

    #[test]
    fn codex_ingestor_prefers_cumulative_token_count_without_double_counting_paired_records() {
        let dir = TempDir::new().unwrap();
        let sess_dir = dir.path().join("sessions");
        fs::create_dir_all(&sess_dir).unwrap();

        let rollout = sess_dir.join("rollout-usage.jsonl");
        let content = concat!(
            r#"{"timestamp":"2026-11-15T10:00:00Z","type":"session_meta","payload":{"id":"codex-u1","cwd":"/tmp/planar","source":"cli"}}"#,
            "\n",
            r#"{"timestamp":"2026-11-15T10:00:01Z","type":"turn_context","payload":{"cwd":"/tmp/planar","model":"gpt-5.4"}}"#,
            "\n",
            r#"{"timestamp":"2026-11-15T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Measure token cost"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-11-15T10:00:10Z","type":"token_usage_record","payload":{"usage":{"input_tokens":27563,"cached_input_tokens":24000,"output_tokens":305,"total_tokens":27868}}}"#,
            "\n",
            r#"{"timestamp":"2026-11-15T10:00:10Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":27563,"cached_input_tokens":24000,"output_tokens":305,"total_tokens":27868}}}}"#,
            "\n"
        );
        fs::write(&rollout, content).unwrap();

        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();
        let mut stats = ScanStats::default();
        let cursors = store.load_file_cursors().unwrap();
        scan_codex(&mut store, &sess_dir, false, &cursors, &mut stats).unwrap();

        let (model, inp, cache_rd, out, tot): (Option<String>, i64, i64, i64, i64) = store
            .connection()
            .query_row(
                "SELECT model, input_tokens, cache_read_tokens, output_tokens, total_tokens FROM sessions WHERE id = 'codex:codex-u1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(model.as_deref(), Some("gpt-5.4"));
        assert_eq!(inp, 3563);
        assert_eq!(cache_rd, 24000);
        assert_eq!(out, 305);
        assert_eq!(tot, 27868);
    }
}
