use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};
use planar_core::db::{Store, SyncScope};
use planar_core::env::{HarnessRoots, build_resume_command, default_db_path, hydrate_macos_path};
use planar_core::ingest::scan_all_harnesses;
use planar_core::mcp::serve_stdio;
use planar_core::model::{
    ScanStats, SearchQuery, SessionFocus, SyncEstimateDto, SyncStreamEvent, SyncSummaryDto,
    SyncUsageDto,
};
use planar_core::pricing::{format_tokens_compact, format_usd_compact};
use planar_core::processor::{estimate_sync_cost, regroup_discovered_lines, spawn_sync_stream};
use planar_core::render::{
    render_l0_status, render_l1_lines, render_l2_sessions, render_search_results,
};
use planar_core::server::{
    AppState, build_embedded_router, build_router, execute_session_open, serve_until,
};

#[derive(Parser, Debug)]
#[command(
    name = "planar",
    version,
    about = "Planar 2D work plane CLI, read-only MCP server & local HTTP daemon"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show L0 progressive-disclosure plane snapshot (~300-600 tokens)
    #[command(visible_alias = "lines")]
    Status {
        /// Lookback window in days (1..=30)
        #[arg(long, default_value_t = 7)]
        days: usize,
        /// Optional workspace path to highlight matching Lines and live sibling sessions
        #[arg(long)]
        cwd: Option<String>,
        /// Emit raw JSON snapshot
        #[arg(long)]
        json: bool,
        /// Skip incremental trajectory scan before reading SQLite
        #[arg(long)]
        no_scan: bool,
    },
    /// Show L1 Line drill-down (`1..=8` Lines: Line Brief, digests, and dependencies)
    Line {
        /// 1..=8 Line slug IDs or case-insensitive Line names
        #[arg(required = true, num_args = 1..=8)]
        lines: Vec<String>,
        /// Maximum recent sessions returned per Line
        #[arg(long, visible_alias = "limit", default_value_t = 8)]
        limit_per_line: usize,
        /// Emit raw JSON payload
        #[arg(long)]
        json: bool,
        /// Skip incremental trajectory scan before reading SQLite
        #[arg(long)]
        no_scan: bool,
    },
    /// Show L2 tool-stripped Head-Middle-Tail session slices (`1..=3` sessions)
    Session {
        /// 1..=3 session IDs (or unambiguous ID prefixes/suffixes)
        #[arg(required = true, num_args = 1..=3)]
        session_ids: Vec<String>,
        /// Slice focus: `full` (Head + Middle + Tail) or `handoff` (Digest + Middle + Tail)
        #[arg(long, default_value = "full", value_parser = ["full", "handoff"])]
        focus: String,
        /// Emit raw JSON payload
        #[arg(long)]
        json: bool,
        /// Skip incremental trajectory scan before reading SQLite
        #[arg(long)]
        no_scan: bool,
    },
    /// Search session digests and opening prompts via local SQLite FTS5
    Search {
        /// Keywords or phrase to search
        query: String,
        /// Optional Line slug ID or name filter
        #[arg(long)]
        line: Option<String>,
        /// Optional lookback window in days
        #[arg(long)]
        days: Option<usize>,
        /// Maximum ranked hits to return (1..=50)
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Emit raw JSON payload
        #[arg(long)]
        json: bool,
        /// Skip incremental trajectory scan before reading SQLite
        #[arg(long)]
        no_scan: bool,
    },
    /// Run read-only MCP `2025-03-26` server over Stdio (or print harness config snippets)
    Mcp {
        #[command(subcommand)]
        action: Option<McpAction>,
    },
    /// Incrementally scan Claude Code, Codex, Antigravity, and DSH logs into SQLite
    Scan {
        /// Emit raw JSON scan metrics
        #[arg(long)]
        json: bool,
    },
    /// Summarize pending or grown sessions into Work Lines using the configured processor
    Sync {
        /// Lookback window in days (1..=31)
        #[arg(long, default_value_t = 7)]
        days: usize,
        /// Optional anchor end date (`YYYY-MM-DD`)
        #[arg(long)]
        date: Option<String>,
        /// Optional harness filter (`antigravity`, `claude`, `codex`, `dsh`)
        #[arg(long)]
        harness: Option<String>,
        /// Maximum sessions to process (omit to drain all pending sessions in window)
        #[arg(long)]
        limit: Option<usize>,
        /// Re-summarize eligible sessions even if an up-to-date digest already exists
        #[arg(long)]
        force: bool,
        /// Estimate pending Sync token usage and USD cost without calling any LLM
        #[arg(long)]
        estimate: bool,
        /// Skip incremental trajectory scan before syncing
        #[arg(long)]
        no_scan: bool,
        /// Emit raw JSON summary
        #[arg(long)]
        json: bool,
    },
    /// Re-cluster sessions inside unpinned discovered Lines (never modifies pinned Lines)
    Regroup {
        /// Emit raw JSON summary
        #[arg(long)]
        json: bool,
    },
    /// Copy a session's harness resume command (or open its workspace)
    Open {
        /// Exact session ID or unambiguous ID prefix/suffix
        session_id: String,
        /// Print the shell resume command to stdout without launching or touching clipboard
        #[arg(long)]
        print: bool,
        /// Emit raw JSON launch metadata
        #[arg(long)]
        json: bool,
        /// Skip incremental trajectory scan before lookup
        #[arg(long)]
        no_scan: bool,
    },
    /// Serve local Axum HTTP API (`/api/*` + `POST /mcp`) and `./ui` frontend
    Serve {
        /// Bind host address
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Listen port
        #[arg(long, default_value_t = 4310)]
        port: u16,
        /// Path to ./ui directory
        #[arg(long)]
        ui_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
enum McpAction {
    /// Print ready-to-paste MCP config snippets for Antigravity, Codex, Claude Code, and Cursor
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = default_db_path().context("failed to resolve planar database path")?;
    let roots = HarnessRoots::from_env_or_home().context("failed to resolve harness log roots")?;

    match cli.command {
        Commands::Status {
            days,
            cwd,
            json,
            no_scan,
        } => cmd_status(&db_path, &roots, days, cwd.as_deref(), json, no_scan),
        Commands::Line {
            lines,
            limit_per_line,
            json,
            no_scan,
        } => cmd_line(&db_path, &roots, &lines, limit_per_line, json, no_scan),
        Commands::Session {
            session_ids,
            focus,
            json,
            no_scan,
        } => cmd_session(&db_path, &roots, &session_ids, &focus, json, no_scan),
        Commands::Search {
            query,
            line,
            days,
            limit,
            json,
            no_scan,
        } => cmd_search(&db_path, &roots, query, line, days, limit, json, no_scan),
        Commands::Mcp { action } => match action {
            None => cmd_mcp(db_path, roots).await,
            Some(McpAction::Config) => {
                print!("{}", render_mcp_config_snippets());
                Ok(())
            }
        },
        Commands::Scan { json } => cmd_scan(&db_path, &roots, json),
        Commands::Sync {
            days,
            date,
            harness,
            limit,
            force,
            estimate,
            no_scan,
            json,
        } => {
            cmd_sync(
                db_path,
                roots,
                SyncCmdArgs {
                    days,
                    date,
                    harness,
                    limit,
                    force,
                    estimate,
                    no_scan,
                    json,
                },
            )
            .await
        }
        Commands::Regroup { json } => cmd_regroup(db_path, roots, json).await,
        Commands::Open {
            session_id,
            print,
            json,
            no_scan,
        } => cmd_open(&db_path, &roots, &session_id, print, json, no_scan),
        Commands::Serve { host, port, ui_dir } => {
            cmd_serve(db_path, roots, &host, port, ui_dir).await
        }
    }
}

fn open_store_and_maybe_scan(db_path: &Path, roots: &HarnessRoots, no_scan: bool) -> Result<Store> {
    let mut store = Store::open(db_path).context("failed to open planar SQLite store")?;
    if !no_scan {
        scan_all_harnesses(&mut store, roots).context("incremental trajectory scan failed")?;
    }
    Ok(store)
}

fn cmd_status(
    db_path: &Path,
    roots: &HarnessRoots,
    days: usize,
    cwd: Option<&str>,
    json: bool,
    no_scan: bool,
) -> Result<()> {
    let store = open_store_and_maybe_scan(db_path, roots, no_scan)?;
    let snapshot = store
        .build_status_snapshot_with_cwd(days, cwd, Local::now())
        .context("failed to compute L0 status snapshot")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        print!("{}", render_l0_status(&snapshot));
    }
    Ok(())
}

fn cmd_line(
    db_path: &Path,
    roots: &HarnessRoots,
    lines: &[String],
    limit_per_line: usize,
    json: bool,
    no_scan: bool,
) -> Result<()> {
    let store = open_store_and_maybe_scan(db_path, roots, no_scan)?;
    let resp = store
        .query_lines(lines, limit_per_line, Local::now())
        .context("failed to query L1 Lines")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        print!("{}", render_l1_lines(&resp));
    }
    Ok(())
}

fn cmd_session(
    db_path: &Path,
    roots: &HarnessRoots,
    session_ids: &[String],
    raw_focus: &str,
    json: bool,
    no_scan: bool,
) -> Result<()> {
    let focus = SessionFocus::parse_str(raw_focus).with_context(|| {
        format!("invalid `--focus` value `{raw_focus}` (expected `full` or `handoff`)")
    })?;
    let store = open_store_and_maybe_scan(db_path, roots, no_scan)?;
    let resp = store
        .query_sessions(session_ids, focus, Local::now())
        .context("failed to slice L2 session trajectories")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        print!("{}", render_l2_sessions(&resp));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_search(
    db_path: &Path,
    roots: &HarnessRoots,
    query: String,
    line: Option<String>,
    days: Option<usize>,
    limit: usize,
    json: bool,
    no_scan: bool,
) -> Result<()> {
    let store = open_store_and_maybe_scan(db_path, roots, no_scan)?;
    let req = SearchQuery {
        query,
        line,
        days,
        limit,
    };
    let resp = store
        .search_sessions(&req, Local::now())
        .context("failed to execute FTS5 session search")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        print!("{}", render_search_results(&resp));
    }
    Ok(())
}

async fn cmd_mcp(db_path: PathBuf, roots: HarnessRoots) -> Result<()> {
    // Ensure SQLite WAL schema and FTS5 index exist before serving requests; never touch stdout.
    Store::open(&db_path).context("failed to initialize planar SQLite store")?;
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let state = AppState::with_hydrated_path(db_path, roots, current_path);
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();
    serve_stdio(state, stdin, stdout)
        .await
        .context("planar MCP stdio loop failed")
}

fn cmd_scan(db_path: &Path, roots: &HarnessRoots, json: bool) -> Result<()> {
    let mut store = Store::open(db_path).context("failed to open planar SQLite store")?;
    let stats = scan_all_harnesses(&mut store, roots).context("incremental scan failed")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!("{}", render_scan_stats(&stats));
    }
    Ok(())
}

struct SyncCmdArgs {
    days: usize,
    date: Option<String>,
    harness: Option<String>,
    limit: Option<usize>,
    force: bool,
    estimate: bool,
    no_scan: bool,
    json: bool,
}

async fn cmd_sync(db_path: PathBuf, roots: HarnessRoots, args: SyncCmdArgs) -> Result<()> {
    let anchor_date = match args
        .date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => Some(
            NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                .with_context(|| format!("invalid `--date` value `{raw}` (expected YYYY-MM-DD)"))?,
        ),
        None => None,
    };
    let scope = SyncScope {
        days: args.days.clamp(
            planar_core::defaults::WINDOW_DAYS_MIN,
            planar_core::defaults::WINDOW_DAYS_MAX,
        ),
        anchor_date,
        harness: args.harness,
    };

    let hydrated_path = hydrate_macos_path();
    let state = AppState::with_hydrated_path(db_path, roots, hydrated_path);

    // `--estimate` is strictly read-only: never scan harness logs or mutate/create SQLite.
    if args.estimate {
        let est = estimate_sync_cost(&state, args.force, args.limit, Some(scope))
            .await
            .context("failed to estimate Sync cost")?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&est)?);
        } else {
            print!("{}", render_sync_estimate(&est));
        }
        return Ok(());
    }

    if !args.no_scan {
        state
            .run_scan()
            .await
            .context("incremental trajectory scan failed")?;
    }

    let mut rx = spawn_sync_stream(state, scope, args.force, args.limit);
    let mut final_summary: Option<SyncSummaryDto> = None;

    while let Some(evt) = rx.recv().await {
        match evt {
            SyncStreamEvent::Start {
                total,
                total_batches,
                backend,
                ..
            } => {
                if !args.json && total > 0 {
                    eprintln!(
                        "Syncing {total} session(s) in {total_batches} batch(es) via {backend}..."
                    );
                }
            }
            SyncStreamEvent::BatchDone(done) => {
                if !args.json && done.batch_index > 0 {
                    let usage_note = done
                        .batch_usage
                        .as_ref()
                        .map(format_sync_usage_inline)
                        .filter(|s| !s.is_empty())
                        .map_or_else(String::new, |s| format!(" [{s}]"));
                    eprintln!(
                        "  batch {}/{}: +{} filed ({} failed){usage_note}",
                        done.batch_index,
                        done.total_batches,
                        done.batch_processed,
                        done.batch_failed
                    );
                }
            }
            SyncStreamEvent::Complete { summary, .. } => {
                final_summary = Some(summary);
                break;
            }
            SyncStreamEvent::BatchStart { .. } | SyncStreamEvent::Heartbeat => {}
        }
    }

    let summary = final_summary.unwrap_or_else(|| SyncSummaryDto {
        processed: 0,
        failed: 0,
        skipped: 0,
        backend_used: "unknown".to_string(),
        last_error: None,
        usage: None,
    });

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    let usage_note = summary
        .usage
        .as_ref()
        .map(format_sync_usage_inline)
        .filter(|s| !s.is_empty())
        .map_or_else(String::new, |s| format!(" ({s})"));
    println!(
        "Sync complete via {}: processed={}, failed={}, skipped={}{usage_note}",
        summary.backend_used, summary.processed, summary.failed, summary.skipped
    );
    if let Some(ref err) = summary.last_error {
        if summary.processed == 0 && summary.failed > 0 {
            anyhow::bail!("{err}");
        }
        eprintln!("Warning: {err}");
    }
    Ok(())
}

fn format_sync_usage_inline(usage: &SyncUsageDto) -> String {
    if usage.is_local_cli {
        return "local CLI".to_string();
    }
    if usage.total_tokens == 0 && usage.cost_usd.is_none() && !usage.incomplete {
        return String::new();
    }
    let mut parts = Vec::new();
    if usage.total_tokens > 0 {
        let suffix = if usage.incomplete && usage.cost_usd.is_none() {
            "*"
        } else {
            ""
        };
        parts.push(format!(
            "{} tok{suffix}",
            format_tokens_compact(usage.total_tokens)
        ));
    }
    if let Some(cost) = usage.cost_usd {
        let formatted = format_usd_compact(cost);
        if usage.incomplete {
            parts.push(format!("{formatted}* (partial)"));
        } else {
            parts.push(formatted);
        }
    } else if usage.incomplete {
        parts.push("partial usage".to_string());
    }
    parts.join(" · ")
}

fn render_sync_estimate(est: &SyncEstimateDto) -> String {
    let model_suffix = if est.model.is_empty() {
        String::new()
    } else {
        format!(" · {}", est.model)
    };
    if est.session_count == 0 {
        return format!(
            "Sync estimate ({}{model_suffix}): 0 pending sessions (nothing to sync)\n",
            est.backend
        );
    }

    let thinking_suffix = if est.estimated_thinking_tokens > 0 {
        format!(
            " (+~{} thinking)",
            format_tokens_compact(est.estimated_thinking_tokens)
        )
    } else {
        String::new()
    };
    let cost_line = if est.is_local_cli {
        "Local CLI / subscription (no direct API cost)".to_string()
    } else if let Some(range) = est.estimated_cost_usd_range {
        let lo = format_usd_compact(range.low);
        let hi = format_usd_compact(range.high);
        if lo == hi {
            format!("~{lo} USD")
        } else {
            format!("~{lo}–{hi} USD")
        }
    } else {
        "— (model pricing unavailable)".to_string()
    };

    format!(
        "Sync estimate ({}{model_suffix}):\n  Pending: {} session(s) in {} batch(es)\n  Tokens:  ~{} input · ~{} output{thinking_suffix}\n  Cost:    {cost_line}\n",
        est.backend,
        est.session_count,
        est.batch_count,
        format_tokens_compact(est.estimated_input_tokens),
        format_tokens_compact(est.estimated_output_tokens),
    )
}

async fn cmd_regroup(db_path: PathBuf, roots: HarnessRoots, json: bool) -> Result<()> {
    let hydrated_path = hydrate_macos_path();
    let state = AppState::with_hydrated_path(db_path, roots, hydrated_path);
    let summary = regroup_discovered_lines(&state)
        .await
        .context("regroup failed")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "Regroup complete via {}: moved={}, skipped={}",
            summary.backend_used, summary.processed, summary.skipped
        );
    }
    Ok(())
}

fn cmd_open(
    db_path: &Path,
    roots: &HarnessRoots,
    session_id: &str,
    print_only: bool,
    json: bool,
    no_scan: bool,
) -> Result<()> {
    let store = open_store_and_maybe_scan(db_path, roots, no_scan)?;
    let info = store
        .resolve_session_launch_info(session_id)
        .with_context(|| format!("failed to resolve session `{session_id}`"))?;

    if print_only {
        let cmd = build_resume_command(&info.harness, &info.id, info.cwd.as_deref()).with_context(
            || {
                format!(
                    "no CLI resume command available for harness `{}` ({})",
                    info.harness, info.id
                )
            },
        )?;
        println!("{cmd}");
        return Ok(());
    }

    let resp = execute_session_open(&info);
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        println!("{} ({})", resp.message, info.id);
        if let Some(cmd) = resp.resume_command {
            println!("Resume: {cmd}");
        }
    }
    Ok(())
}

async fn cmd_serve(
    db_path: PathBuf,
    roots: HarnessRoots,
    host: &str,
    port: u16,
    ui_dir_arg: Option<PathBuf>,
) -> Result<()> {
    let hydrated_path = hydrate_macos_path();
    let ui_dir = resolve_ui_dir(ui_dir_arg)?;

    let state = AppState::with_hydrated_path(db_path, roots, hydrated_path);
    state
        .run_scan()
        .await
        .context("initial incremental scan failed")?;

    let app = match ui_dir {
        Some(dir) => {
            let assets_dir = dir
                .parent()
                .map_or_else(|| PathBuf::from("assets"), |p| p.join("assets"));
            build_router(state.clone(), &dir, &assets_dir)
        }
        None => build_embedded_router(state.clone()),
    };
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("invalid bind address {host}:{port}"))?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    println!("Planar serving at http://{addr}/ui/ (MCP HTTP: http://{addr}/mcp)");

    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let bg_state = state;
    let bg_handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(15));
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(err) = bg_state.run_scan().await {
                        eprintln!("planar background scan warning: {err}");
                    }
                }
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });

    serve_until(listener, app, async move {
        shutdown_signal().await;
        let _ = stop_tx.send(true);
    })
    .await
    .context("HTTP server exited with error")?;

    let _ = bg_handle.await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

fn resolve_ui_dir(explicit: Option<PathBuf>) -> Result<Option<PathBuf>> {
    if let Some(dir) = explicit {
        anyhow::ensure!(
            dir.is_dir(),
            "ui directory does not exist: {}",
            dir.display()
        );
        return Ok(Some(dir));
    }
    let cwd_ui = PathBuf::from("ui");
    if cwd_ui.is_dir() {
        return Ok(Some(cwd_ui));
    }
    let manifest_ui = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("ui");
    if manifest_ui.is_dir() {
        return Ok(Some(manifest_ui));
    }
    Ok(None)
}

fn render_scan_stats(stats: &ScanStats) -> String {
    format!(
        "Scan complete in {}ms: discovered={}, full={}, incremental={}, unchanged={}, subagents_linked={}, errors={}",
        stats.elapsed_ms,
        stats.files_discovered,
        stats.files_full_scanned,
        stats.files_incremental,
        stats.files_unchanged,
        stats.subagents_linked,
        stats.files_errored
    )
}

fn render_mcp_config_snippets() -> String {
    let bin = std::env::current_exe().map_or_else(
        |_| "planar".to_string(),
        |p| p.to_string_lossy().into_owned(),
    );
    format!(
        "# Antigravity (~/.gemini/config/mcp_config.json -> mcpServers.planar)\n\
{{\n  \"mcpServers\": {{\n    \"planar\": {{\n      \"command\": \"{bin}\",\n      \"args\": [\"mcp\"]\n    }}\n  }}\n}}\n\n\
# Codex (~/.codex/config.toml)\n\
[mcp_servers.planar]\n\
command = \"{bin}\"\n\
args = [\"mcp\"]\n\n\
# Claude Code\n\
claude mcp add --scope user planar -- \"{bin}\" mcp\n\n\
# Cursor (~/.cursor/mcp.json -> mcpServers.planar)\n\
{{\n  \"mcpServers\": {{\n    \"planar\": {{\n      \"command\": \"{bin}\",\n      \"args\": [\"mcp\"]\n    }}\n  }}\n}}\n\n\
# Streamable HTTP (when Planar.app or `planar serve` is running)\n\
http://127.0.0.1:4310/mcp\n"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_mcp_stdio_and_config_and_batch_commands() {
        let mcp_stdio = Cli::try_parse_from(["planar", "mcp"]).unwrap();
        assert!(matches!(mcp_stdio.command, Commands::Mcp { action: None }));

        let mcp_cfg = Cli::try_parse_from(["planar", "mcp", "config"]).unwrap();
        assert!(matches!(
            mcp_cfg.command,
            Commands::Mcp {
                action: Some(McpAction::Config)
            }
        ));

        let line_cmd =
            Cli::try_parse_from(["planar", "line", "planar", "one-off", "--json"]).unwrap();
        if let Commands::Line {
            lines,
            limit_per_line,
            json,
            ..
        } = line_cmd.command
        {
            assert_eq!(lines, vec!["planar", "one-off"]);
            assert_eq!(limit_per_line, 8);
            assert!(json);
        } else {
            panic!("expected Line command");
        }

        let sess_cmd = Cli::try_parse_from([
            "planar",
            "session",
            "codex:s1",
            "claude:s2",
            "--focus",
            "handoff",
        ])
        .unwrap();
        if let Commands::Session {
            session_ids, focus, ..
        } = sess_cmd.command
        {
            assert_eq!(session_ids, vec!["codex:s1", "claude:s2"]);
            assert_eq!(focus, "handoff");
        } else {
            panic!("expected Session command");
        }

        assert!(
            Cli::try_parse_from(["planar", "session", "s1", "s2", "s3", "s4"]).is_err(),
            "more than 3 session IDs should be rejected by CLI parser"
        );

        let sync_cmd = Cli::try_parse_from([
            "planar",
            "sync",
            "--days",
            "14",
            "--harness",
            "codex",
            "--force",
            "--estimate",
            "--json",
        ])
        .unwrap();
        assert!(matches!(
            sync_cmd.command,
            Commands::Sync {
                days: 14,
                force: true,
                estimate: true,
                json: true,
                ..
            }
        ));

        let regroup_cmd = Cli::try_parse_from(["planar", "regroup", "--json"]).unwrap();
        assert!(matches!(
            regroup_cmd.command,
            Commands::Regroup { json: true }
        ));

        let open_cmd = Cli::try_parse_from(["planar", "open", "alpha-1111", "--print"]).unwrap();
        assert!(matches!(
            open_cmd.command,
            Commands::Open {
                print: true,
                json: false,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn sync_estimate_cli_forms_never_scan_or_mutate_sqlite() {
        let dir = tempfile::TempDir::new().unwrap();
        let roots = HarnessRoots {
            claude_projects: dir.path().join("claude"),
            codex_sessions: dir.path().join("codex").join("sessions"),
            codex_archived: dir.path().join("codex").join("archived_sessions"),
            antigravity_brain: dir.path().join("antigravity"),
            dsh_sessions: dir.path().join("dsh").join("sessions"),
        };

        // Place a real rollout file on disk so an unintended scan would ingest it and mutate DB.
        let sess_dir = roots.codex_sessions.join("2026").join("11").join("25");
        std::fs::create_dir_all(&sess_dir).unwrap();
        let now = Local::now();
        let t1 = (now - chrono::Duration::minutes(10)).to_rfc3339();
        let t2 = (now - chrono::Duration::minutes(8)).to_rfc3339();
        let t3 = (now - chrono::Duration::minutes(5)).to_rfc3339();
        std::fs::write(
            sess_dir.join("rollout-ro.jsonl"),
            format!(
                concat!(
                    "{{\"timestamp\":\"{t1}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"ro-1\",\"cwd\":\"/tmp/ws\",\"source\":\"vscode\"}}}}\n",
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

        // 1. Non-existent DB: neither `--estimate` nor `--estimate --no-scan` may create it.
        let missing_db = dir.path().join("uncreated").join("planar.db");
        for no_scan in [false, true] {
            cmd_sync(
                missing_db.clone(),
                roots.clone(),
                SyncCmdArgs {
                    days: 7,
                    date: None,
                    harness: None,
                    limit: None,
                    force: false,
                    estimate: true,
                    no_scan,
                    json: true,
                },
            )
            .await
            .unwrap();
            assert!(
                !missing_db.exists(),
                "sync --estimate (no_scan={no_scan}) must not create a missing SQLite DB"
            );
        }

        // 2. Existing initialized DB: neither `--estimate` nor `--estimate --no-scan` may scan or modify bytes.
        let existing_db = dir.path().join("existing.db");
        {
            let _store = Store::open(&existing_db).unwrap();
        }
        let bytes_before = std::fs::read(&existing_db).unwrap();
        for no_scan in [false, true] {
            cmd_sync(
                existing_db.clone(),
                roots.clone(),
                SyncCmdArgs {
                    days: 7,
                    date: None,
                    harness: None,
                    limit: None,
                    force: false,
                    estimate: true,
                    no_scan,
                    json: true,
                },
            )
            .await
            .unwrap();
            let bytes_after = std::fs::read(&existing_db).unwrap();
            assert_eq!(
                bytes_before, bytes_after,
                "sync --estimate (no_scan={no_scan}) must not mutate existing SQLite DB"
            );
        }

        // Verify partial usage formatting marks incomplete totals honestly.
        let partial = SyncUsageDto {
            total_tokens: 14_000,
            cost_usd: Some(0.03),
            api_calls: 3,
            priced_calls: 2,
            incomplete: true,
            ..SyncUsageDto::default()
        };
        let rendered = format_sync_usage_inline(&partial);
        assert!(rendered.contains("$0.03*"), "got: {rendered}");
        assert!(rendered.contains("partial"), "got: {rendered}");
    }
}
