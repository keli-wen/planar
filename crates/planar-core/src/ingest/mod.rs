pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod dsh;

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use chrono::DateTime;

use crate::db::{FileCursor, IngestMode, Store};
use crate::defaults::MAX_PROMPT_CHARS;
use crate::env::HarnessRoots;
use crate::error::Result;
use crate::model::ScanStats;

/// Decision for how to scan a source trajectory file given its previous cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanDecision {
    Unchanged,
    Full,
    AppendFrom(u64),
}

/// Result of streaming complete JSONL lines from a byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamStatus {
    Complete { consumed_bytes: u64 },
    NeedsFullRescan,
}

/// Scans all enabled local agent harness roots in read-only mode and updates `store`.
pub fn scan_all_harnesses(store: &mut Store, base_roots: &HarnessRoots) -> Result<ScanStats> {
    let started = Instant::now();
    let settings = store.get_settings()?;
    let roots = base_roots.effective_for(&settings);
    let cursors = store.load_file_cursors()?;
    let mut stats = ScanStats::default();

    if settings.enabled_harnesses.claude {
        claude::scan_claude(store, &roots.claude_projects, &cursors, &mut stats)?;
    }
    if settings.enabled_harnesses.codex {
        codex::scan_codex(store, &roots.codex_sessions, false, &cursors, &mut stats)?;
        codex::scan_codex(store, &roots.codex_archived, true, &cursors, &mut stats)?;
    }
    if settings.enabled_harnesses.antigravity {
        antigravity::scan_antigravity(store, &roots.antigravity_brain, &cursors, &mut stats)?;
    }
    if settings.enabled_harnesses.dsh {
        dsh::scan_dsh(store, &roots.dsh_sessions, &cursors, &mut stats)?;
    }

    stats.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(stats)
}

pub(crate) fn file_meta(path: &Path) -> Result<(u64, i64)> {
    let meta = fs::metadata(path)?;
    let size = meta.len();
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);
    Ok((size, mtime_ms))
}

pub(crate) fn decide_scan(
    path_str: &str,
    size: u64,
    mtime_ms: i64,
    cursors: &HashMap<String, FileCursor>,
) -> ScanDecision {
    let Some(cur) = cursors.get(path_str) else {
        return ScanDecision::Full;
    };
    if cur.source_size == size && cur.source_mtime_ms == mtime_ms {
        return ScanDecision::Unchanged;
    }
    if size > cur.source_size && cur.source_size > 0 {
        return ScanDecision::AppendFrom(cur.source_size);
    }
    ScanDecision::Full
}

pub(crate) fn stream_jsonl_lines<F>(
    path: &Path,
    start_offset: u64,
    mut on_line: F,
) -> Result<StreamStatus>
where
    F: FnMut(&str) -> std::result::Result<(), ()>,
{
    let mut file = File::open(path)?;
    if start_offset > 0 {
        file.seek(SeekFrom::Start(start_offset))?;
    }

    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut raw_buf = Vec::with_capacity(8 * 1024);
    let mut consumed_bytes = start_offset;
    let mut lines_ok = 0_usize;

    loop {
        raw_buf.clear();
        let bytes_read = reader.read_until(b'\n', &mut raw_buf)?;
        if bytes_read == 0 {
            break;
        }

        let has_newline = raw_buf.last() == Some(&b'\n');
        let line_str = String::from_utf8_lossy(&raw_buf);
        let trimmed = line_str.trim();

        if trimmed.is_empty() {
            if has_newline {
                consumed_bytes = consumed_bytes.saturating_add(bytes_read as u64);
                continue;
            }
            break;
        }

        if on_line(trimmed).is_ok() {
            lines_ok += 1;
            consumed_bytes = consumed_bytes.saturating_add(bytes_read as u64);
        } else {
            if start_offset > 0 && lines_ok == 0 {
                return Ok(StreamStatus::NeedsFullRescan);
            }
            if !has_newline {
                // Partial line currently being flushed by the source harness.
                break;
            }
            // Skip malformed complete line so one corrupt line cannot stall future scans.
            consumed_bytes = consumed_bytes.saturating_add(bytes_read as u64);
        }
    }

    Ok(StreamStatus::Complete { consumed_bytes })
}

pub(crate) fn parse_rfc3339_ms(raw: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Strips headless delegation envelope headers (e.g. `[cospan delegation context]` /
/// `[cospan follow-up ...]`) when followed by an explicit `[Task]` section or body paragraph,
/// preserving the underlying human/delegated task text.
pub(crate) fn strip_delegation_envelope(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with("[cospan delegation context]") {
        for marker in ["\n[Task]\n", "\r\n[Task]\r\n", "\n[Task] "] {
            if let Some(idx) = trimmed.find(marker) {
                let rest = trimmed[idx + marker.len()..].trim();
                if !rest.is_empty() {
                    return rest;
                }
            }
        }
        for sep in ["\n\n", "\r\n\r\n"] {
            if let Some(idx) = trimmed.find(sep) {
                let rest = trimmed[idx + sep.len()..].trim();
                if !rest.is_empty() {
                    return rest;
                }
            }
        }
    } else if trimmed.starts_with("[cospan follow-up") {
        for sep in ["\n\n", "\r\n\r\n"] {
            if let Some(idx) = trimmed.find(sep) {
                let rest = trimmed[idx + sep.len()..].trim();
                if !rest.is_empty() {
                    return rest;
                }
            }
        }
    }
    trimmed
}

pub(crate) fn normalize_prompt(raw: &str) -> String {
    let unwrapped = strip_delegation_envelope(raw);
    let mut collapsed = String::with_capacity(unwrapped.len().min(MAX_PROMPT_CHARS + 16));
    let mut char_count = 0_usize;
    let mut truncated = false;

    for word in unwrapped.split_whitespace() {
        let needed = word.chars().count() + usize::from(!collapsed.is_empty());
        if char_count + needed > MAX_PROMPT_CHARS {
            let remaining = MAX_PROMPT_CHARS.saturating_sub(char_count);
            if !collapsed.is_empty() && remaining > 1 {
                collapsed.push(' ');
                for ch in word.chars().take(remaining - 1) {
                    collapsed.push(ch);
                }
            } else if collapsed.is_empty() {
                for ch in word.chars().take(MAX_PROMPT_CHARS) {
                    collapsed.push(ch);
                }
            }
            truncated = true;
            break;
        }
        if !collapsed.is_empty() {
            collapsed.push(' ');
            char_count += 1;
        }
        collapsed.push_str(word);
        char_count += word.chars().count();
    }

    if truncated && !collapsed.is_empty() {
        collapsed.push_str("...");
    }
    collapsed
}

pub(crate) fn update_bounds(started_at: &mut i64, last_active_at: &mut i64, ts_ms: i64) {
    if ts_ms <= 0 {
        return;
    }
    if *started_at <= 0 || ts_ms < *started_at {
        *started_at = ts_ms;
    }
    if ts_ms > *last_active_at {
        *last_active_at = ts_ms;
    }
}

pub(crate) fn scan_mode_for(decision: ScanDecision) -> (u64, IngestMode) {
    match decision {
        ScanDecision::AppendFrom(offset) => (offset, IngestMode::Incremental),
        ScanDecision::Full | ScanDecision::Unchanged => (0, IngestMode::FullReplace),
    }
}
