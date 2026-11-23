//! Shared token-frugal Markdown formatters used 1:1 by both `planar` CLI subcommands
//! and Planar's read-only MCP tools/resources (`planar_status`, `planar_line`,
//! `planar_session`, `planar_search`).

use std::fmt::Write as _;

use crate::env::build_resume_command;
use crate::model::{
    BatchLinesResponse, BatchSessionsResponse, SearchResponse, SessionDepLink, SessionFocus,
    SlicedTurnDto, StatusSnapshot,
};
use crate::pricing::format_tokens_compact;

/// Formats an L0 `StatusSnapshot` into scannable Markdown (~300–600 tokens).
#[must_use]
pub fn render_l0_status(snap: &StatusSnapshot) -> String {
    let mut out = String::new();

    let harnesses = if snap.harness_counts.is_empty() {
        "none".to_string()
    } else {
        snap.harness_counts
            .iter()
            .map(|h| format!("{}={}", h.harness, h.count))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let cwd_suffix = snap
        .cwd
        .as_deref()
        .map_or_else(String::new, |c| format!(" · cwd={c}"));
    let _ = writeln!(
        out,
        "Planar L0 · {} ({}) · window={}d{cwd_suffix}",
        snap.generated_at_local, snap.tz_label, snap.window_days
    );
    let _ = writeln!(
        out,
        "Active agent time: today {} | {}d {}{} · harnesses: {harnesses}",
        fmt_minutes(snap.total_today_minutes),
        snap.window_days,
        fmt_minutes(snap.total_window_minutes),
        fmt_usage_suffix(snap.total_window_tokens, snap.total_window_cost_usd)
    );

    if !snap.live_sessions.is_empty() {
        let _ = writeln!(out, "\nLive sibling sessions:");
        for live in &snap.live_sessions {
            let mut flags = Vec::new();
            if live.matches_cwd {
                flags.push("same cwd");
            }
            if live.has_open_turn {
                flags.push("open turn");
            } else {
                flags.push("active");
            }
            let line_tag = live
                .line_name
                .as_deref()
                .map_or_else(String::new, |ln| format!(" · Line: {ln}"));
            let _ = writeln!(
                out,
                "  - [{}] {} (`{}`) [{}]{line_tag} · {} steps",
                live.harness,
                truncate_chars(&live.label, 72),
                live.id,
                flags.join(", "),
                live.steps
            );
        }
    }

    let _ = writeln!(out, "\nLines:");
    if snap.lines.is_empty() {
        let _ = writeln!(out, "  (none)");
    } else {
        for line in &snap.lines {
            let mut tags = vec![
                fmt_priority(line.priority).to_string(),
                line.status.clone(),
                if line.pinned {
                    "pinned".to_string()
                } else {
                    "discovered".to_string()
                },
            ];
            if line.matches_cwd {
                tags.push("cwd".to_string());
            }
            if line.running_sessions > 0 {
                tags.push(format!("{} running", line.running_sessions));
            }
            let mut tail_notes = String::new();
            if !line.last_active_recency.is_empty() && line.last_active_recency != "running" {
                let _ = write!(tail_notes, " · last {}", line.last_active_recency);
            }
            if line.open_followups > 0 {
                let _ = write!(tail_notes, " · {} left open", line.open_followups);
            }
            let _ = writeln!(
                out,
                "  - {} (`{}`) [{}] · today {} | {}d {}{} · {}/{} sessions{tail_notes}",
                line.name,
                line.id,
                tags.join(", "),
                fmt_minutes(line.today_minutes),
                snap.window_days,
                fmt_minutes(line.window_minutes),
                fmt_usage_suffix(line.window_tokens, line.window_cost_usd),
                line.active_today_sessions,
                line.total_sessions
            );
            if !line.description.trim().is_empty() {
                let _ = writeln!(
                    out,
                    "    Brief: {}",
                    truncate_chars(line.description.trim(), 120)
                );
            }
        }
    }

    let drift_notes = compute_drift_notes(snap);
    if !drift_notes.is_empty() {
        let _ = writeln!(out, "\nPriority vs. Time drift:");
        for note in drift_notes {
            let _ = writeln!(out, "  - {note}");
        }
    }

    let _ = writeln!(
        out,
        "\nUnsummarized (Sync queue: {} · today {} | {}d {}{}):",
        snap.unsummarized_count,
        fmt_minutes(snap.unsummarized_today_minutes),
        snap.window_days,
        fmt_minutes(snap.unsummarized_window_minutes),
        fmt_usage_suffix(
            snap.unsummarized_window_tokens,
            snap.unsummarized_window_cost_usd
        )
    );
    if snap.top_unsummarized.is_empty() {
        let _ = writeln!(out, "  (none)");
    } else {
        for item in &snap.top_unsummarized {
            let preview = truncate_chars(&item.prompt, 88);
            let _ = writeln!(
                out,
                "  - [{}] {} · {} steps{} · {} (`{}`)",
                item.harness,
                item.recency,
                item.steps,
                fmt_usage_suffix(item.total_tokens, item.cost_usd.unwrap_or(0.0)),
                preview,
                item.id
            );
        }
    }

    let _ = writeln!(
        out,
        "\nShort sessions: {} · today {} | {}d {}{}",
        snap.short_count,
        fmt_minutes(snap.short_today_minutes),
        snap.window_days,
        fmt_minutes(snap.short_window_minutes),
        fmt_usage_suffix(snap.short_window_tokens, snap.short_window_cost_usd)
    );

    if !snap.dependencies.is_empty() {
        let _ = writeln!(out, "\nDependencies:");
        for dep in &snap.dependencies {
            let _ = writeln!(
                out,
                "  - `{}` -> `{}` ({}, {})",
                dep.from_session_id, dep.to_session_id, dep.kind, dep.source
            );
        }
    }

    out
}

/// Formats a batch L1 `BatchLinesResponse` into scannable Markdown.
#[must_use]
pub fn render_l1_lines(resp: &BatchLinesResponse) -> String {
    let mut out = String::new();

    for (idx, line) in resp.lines.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let kind = if line.pinned { "pinned" } else { "discovered" };
        let _ = writeln!(out, "## Line: {} (`{}`)", line.name, line.id);
        let _ = writeln!(
            out,
            "Meta: {} · {} · {kind} · today {} | 7d {} | total {}{} · {} sessions ({} archived, {} running)",
            fmt_priority(line.priority),
            line.status,
            fmt_minutes(line.today_minutes),
            fmt_minutes(line.week_minutes),
            fmt_minutes(line.total_minutes),
            fmt_usage_suffix(line.total_tokens, line.total_cost_usd),
            line.total_sessions,
            line.archived_sessions,
            line.running_sessions
        );
        if line.description.trim().is_empty() {
            let _ = writeln!(out, "Line Brief: (none)");
        } else {
            let _ = writeln!(out, "Line Brief: {}", line.description.trim());
        }

        if line.sessions.is_empty() {
            let _ = writeln!(out, "Sessions: (none)");
            continue;
        }

        let _ = writeln!(
            out,
            "Sessions (showing {}/{}):",
            line.returned_sessions, line.total_sessions
        );
        for sess in &line.sessions {
            let mut flags = Vec::new();
            if sess.running {
                if sess.has_open_turn {
                    flags.push("open turn");
                } else {
                    flags.push("running");
                }
            }
            if sess.archived {
                flags.push("archived");
            }
            let flag_str = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(", "))
            };
            let _ = writeln!(
                out,
                "  - **{}** (`{}`) · [{}] {} ({}) · {} · {} steps{}{flag_str}",
                sess.title,
                sess.id,
                sess.harness,
                sess.recency,
                sess.last_active_local,
                fmt_minutes(sess.active_minutes),
                sess.steps,
                fmt_usage_suffix(sess.total_tokens, sess.cost_usd.unwrap_or(0.0))
            );
            if !sess.stopped_at.trim().is_empty() {
                let _ = writeln!(out, "    Stopped at: {}", sess.stopped_at.trim());
            }
            if !sess.left_open.trim().is_empty() {
                let _ = writeln!(out, "    Left open: {}", sess.left_open.trim());
            }
            if !sess.depends_on.is_empty() {
                let _ = writeln!(out, "    Depends on: {}", fmt_dep_links(&sess.depends_on));
            }
            if !sess.depended_by.is_empty() {
                let _ = writeln!(out, "    Unblocks: {}", fmt_dep_links(&sess.depended_by));
            }
        }
    }

    if !resp.not_found.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "Not found: {}",
            resp.not_found
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        if !resp.available_lines.is_empty() {
            let hints = resp
                .available_lines
                .iter()
                .map(|h| format!("`{}` ({}, {})", h.id, h.name, fmt_priority(h.priority)))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "Available lines: {hints}");
        }
    }

    out
}

/// Formats a bounded-batch L2 `BatchSessionsResponse` into scannable Markdown.
#[must_use]
pub fn render_l2_sessions(resp: &BatchSessionsResponse) -> String {
    let mut out = String::new();

    for (idx, sess) in resp.sessions.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let heading_title = sess
            .digest
            .as_ref()
            .map(|d| d.title.as_str())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(sess.first_user_prompt.as_str());

        let _ = writeln!(
            out,
            "## Session: {} (`{}`)",
            truncate_chars(heading_title, 96),
            sess.id
        );
        let line_label = sess.line_name.as_deref().unwrap_or("(unassigned)");
        let _ = writeln!(
            out,
            "Meta: [{}] · Line: {line_label} · focus={} · {} ({}) · {} · {}/{} dialogue/raw steps{}",
            sess.harness,
            sess.focus.as_str(),
            sess.recency,
            sess.last_active_local,
            fmt_minutes(sess.active_minutes),
            sess.extracted_dialogue_steps,
            sess.steps,
            fmt_usage_suffix(sess.total_tokens, sess.cost_usd.unwrap_or(0.0))
        );
        let _ = writeln!(
            out,
            "Workspace: {}",
            sess.cwd.as_deref().unwrap_or("(unknown)")
        );
        let src_state = if sess.source_exists {
            "exists"
        } else {
            "pruned"
        };
        let _ = writeln!(out, "Transcript: {} ({src_state})", sess.source_path);
        if let Some(resume_cmd) = build_resume_command(&sess.harness, &sess.id, sess.cwd.as_deref())
        {
            let _ = writeln!(out, "Resume: `{resume_cmd}`");
        }

        if !sess.depends_on.is_empty() {
            let _ = writeln!(out, "Depends on: {}", fmt_dep_links(&sess.depends_on));
        }
        if !sess.depended_by.is_empty() {
            let _ = writeln!(out, "Unblocks: {}", fmt_dep_links(&sess.depended_by));
        }

        if let Some(ref digest) = sess.digest {
            if !digest.stopped_at.trim().is_empty() {
                let _ = writeln!(out, "Stopped at: {}", digest.stopped_at.trim());
            }
            if !digest.left_open.trim().is_empty() {
                let _ = writeln!(out, "Left open: {}", digest.left_open.trim());
            }
        } else if sess.focus == SessionFocus::Handoff || !sess.source_exists {
            let _ = writeln!(out, "Opening prompt: {}", sess.first_user_prompt);
        }

        if !sess.source_exists {
            let _ = writeln!(
                out,
                "_Source trajectory log was pruned on disk; showing stored digest and metadata only._"
            );
            continue;
        }

        append_turn_section(&mut out, "HEAD (Opening turns)", &sess.head);
        append_turn_section(&mut out, "MIDDLE (User prompts only)", &sess.middle_users);
        append_turn_section(&mut out, "TAIL (Closing turns)", &sess.tail);
    }

    if !resp.not_found.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "Not found: {}",
            resp.not_found
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    for amb in &resp.ambiguous {
        let _ = writeln!(
            out,
            "Ambiguous `{}` -> candidates: {}",
            amb.query,
            amb.candidates
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    out
}

/// Formats an FTS5 `SearchResponse` into compact ranked Markdown hits.
#[must_use]
pub fn render_search_results(resp: &SearchResponse) -> String {
    let mut out = String::new();

    let line_part = resp
        .line_filter
        .as_deref()
        .map_or_else(String::new, |l| format!(" · line={l}"));
    let days_part = resp
        .days
        .map_or_else(String::new, |d| format!(" · window={d}d"));
    let _ = writeln!(
        out,
        "Planar Search · query=\"{}\"{line_part}{days_part} · {} hit(s)",
        resp.query,
        resp.hits.len()
    );

    if resp.hits.is_empty() {
        let _ = writeln!(out, "  (no matching sessions)");
        return out;
    }

    for hit in &resp.hits {
        let line_label = hit.line_name.as_deref().unwrap_or("(unassigned)");
        let _ = writeln!(
            out,
            "  - **{}** (`{}`) · [{}] · Line: {line_label} · {} ({}) · {}",
            hit.title,
            hit.session_id,
            hit.harness,
            hit.recency,
            hit.last_active_local,
            fmt_minutes(hit.active_minutes)
        );
        if !hit.stopped_at.trim().is_empty() {
            let _ = writeln!(out, "    Stopped at: {}", hit.stopped_at.trim());
        }
        if !hit.left_open.trim().is_empty() {
            let _ = writeln!(out, "    Left open: {}", hit.left_open.trim());
        }
        if hit.stopped_at.trim().is_empty() && hit.left_open.trim().is_empty() {
            let _ = writeln!(
                out,
                "    Prompt: {}",
                truncate_chars(&hit.first_user_prompt, 120)
            );
        }
    }

    out
}

fn compute_drift_notes(snap: &StatusSnapshot) -> Vec<String> {
    let mut notes = Vec::new();
    if snap.total_window_minutes < 30 {
        return notes;
    }

    let mut has_high_prio = false;
    for line in &snap.lines {
        if line.status == "active" && matches!(line.priority, 1 | 2) {
            has_high_prio = true;
            if line.window_minutes == 0 {
                notes.push(format!(
                    "{} Line `{}` (`{}`) logged 0m in the last {}d.",
                    fmt_priority(line.priority),
                    line.name,
                    line.id,
                    snap.window_days
                ));
            }
        }
    }

    if has_high_prio && snap.total_window_minutes >= 60 {
        let unpinned_or_bucket_mins: i64 = snap
            .lines
            .iter()
            .filter(|l| !l.pinned || l.id == "one-off")
            .map(|l| l.window_minutes)
            .sum::<i64>()
            + snap.unsummarized_window_minutes
            + snap.short_window_minutes;
        let pct = (unpinned_or_bucket_mins * 100) / snap.total_window_minutes.max(1);
        if pct >= 55 {
            notes.push(format!(
                "Unpinned / One-off / unsummarized work consumed {pct}% ({}) of {}d agent time.",
                fmt_minutes(unpinned_or_bucket_mins),
                snap.window_days
            ));
        }
    }

    notes
}

fn append_turn_section(out: &mut String, heading: &str, turns: &[SlicedTurnDto]) {
    if turns.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n### {heading}");
    for turn in turns {
        let _ = writeln!(out, "[{} #{}] {}", turn.role, turn.index, turn.text);
    }
}

fn fmt_dep_links(links: &[SessionDepLink]) -> String {
    links
        .iter()
        .map(|l| {
            let state = if l.running { ", running" } else { "" };
            format!(
                "{} (`{}`, {}{state})",
                truncate_chars(&l.title, 48),
                l.session_id,
                l.kind
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn fmt_usage_suffix(tokens: u64, cost_usd: f64) -> String {
    if tokens == 0 {
        return String::new();
    }
    let tok = format_tokens_compact(tokens);
    if cost_usd >= 0.005 {
        format!(" · {tok} tok (≈ ${cost_usd:.2})")
    } else if cost_usd > 0.0 {
        format!(" · {tok} tok (≈ <$0.01)")
    } else {
        format!(" · {tok} tok")
    }
}

fn fmt_minutes(mins: i64) -> String {
    let m = mins.max(0);
    if m == 0 {
        return "0m".to_string();
    }
    let h = m / 60;
    let rem = m % 60;
    if h == 0 {
        format!("{rem}m")
    } else {
        format!("{h}h {rem:02}m")
    }
}

fn fmt_priority(priority: u8) -> &'static str {
    match priority {
        1 => "P1 Urgent",
        2 => "P2 High",
        3 => "P3 Medium",
        4 => "P4 Low",
        _ => "No priority",
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    s.push_str("...");
    s
}
