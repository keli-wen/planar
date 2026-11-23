# Reference: Architecture & Core Invariants Quick-Check

> Pointer cheat-sheet for day-to-day code changes. Canonical details live in `contexts/project/architecture.md` and `contexts/project/design.md`.

## Ownership Map

| If you need to change… | Primary file(s) | Keep out of… |
| :--- | :--- | :--- |
| Wire DTOs, `AppSettings`, palettes, sentinels | `crates/planar-core/src/model.rs` | `planar-cli`, `src-tauri` |
| Log roots, `$PATH` hydration, CLI binary resolution, `build_resume_command` | `crates/planar-core/src/env.rs` | `server.rs` (reuse `env::build_resume_command`) |
| Clamped burst union & local-midnight day splitting | `crates/planar-core/src/burst.rs` | `db.rs` / `ui/app.js` |
| Harness JSONL scanners | `crates/planar-core/src/ingest/{mod,claude,codex,antigravity,dsh}.rs` | `processor.rs` |
| SQLite WAL + FTS5 schema, migrations, CRUD, L0/L1/L2/Search queries | `crates/planar-core/src/db.rs` | `server.rs`, `mcp.rs` |
| Head–Middle–Tail slicer, `slice_config_hash`, `Sync` / `Regroup` LLM adapters | `crates/planar-core/src/processor.rs` | `db.rs` |
| Offline USD list-price model table | `crates/planar-core/src/pricing.rs` | `ui/app.js` |
| Shared Markdown renderers for CLI & MCP | `crates/planar-core/src/render.rs` | `crates/planar-cli/src/main.rs`, `mcp.rs` |
| Read-only MCP `2025-03-26` JSON-RPC engine | `crates/planar-core/src/mcp.rs` | Any write/mutating tool in MCP v1 |
| Axum routes, embedded `./ui`, `execute_session_open` | `crates/planar-core/src/server.rs` | `src-tauri/src/lib.rs` |
| CLI subcommands & arg parsing | `crates/planar-cli/src/main.rs` | Business logic (delegate to `planar-core`) |
| macOS desktop window & tray shell | `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json` | Data IPC (use embedded loopback HTTP) |

## Non-Negotiable Invariant Checklist

Before finishing any Rust edit, verify:

1. **Zero `unsafe`, zero `unwrap()`/`dbg!()` in production code** (workspace lint enforces this).
2. **Read-only on source logs**: Never write, lock, or truncate files under `~/.claude`, `~/.codex`, `~/.gemini/antigravity`, or `~/.dsh`.
3. **Associative burst intervals**: Persist unpadded `[start_ms, end_ms]` spans in `activity_intervals`; apply `MIN_BURST_MS = 60s` only at read/projection time.
4. **Anti-self-ingestion**: All internal CLI summarizer prompts carry `[PLANAR_INTERNAL_DIGEST]` and are dropped by ingestors, slicer, and FTS5 sync.
5. **Pinned Line immutability**: `Sync` and `Regroup` never overwrite `pinned = 1` Line assignments or rename/merge pinned Lines.
6. **Pruned-log digest lock**: Never re-summarize or delete a saved digest whose `source_path` no longer exists on disk.
7. **Archived retention**: Archived sessions remain in contribution heatmaps, `planar status`, and filed Line totals.
8. **Single `$PATH` hydration**: Call `hydrate_macos_path()` once at startup and pass it via `AppState::with_hydrated_path(...)`.
9. **Zero `stdout` pollution in `planar mcp`**: Diagnostics go to `stderr` only.
10. **Strict Tauri DnD**: Never flip `"dragDropEnabled": false` in `src-tauri/tauri.conf.json`.
