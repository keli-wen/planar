# Planar — Agent Workspace Guide

Planar — **a shared map of the work you do with agents** — is an **experimental**, local-first `Lines × Time` engine built on a **Rust core (`planar-core`) + native CLI & read-only MCP (`planar-cli`) + thin Tauri v2 desktop/menu-bar shell (`src-tauri`) + zero-fork vanilla Web UI (`./ui`)**.

> `CLAUDE.md` is a symlink to this file, and `.claude/skills` is a symlink to `.agents/skills`, so Claude Code, Codex, Antigravity, DeepSeek Harness (`dsh`), and Cursor share one source of truth.

## Progressive-Disclosure Reading Map (`L0 → L1 → L2`)

Do not scan the whole repository blindly. Start at [`contexts/CONTEXT_MAP.md`](contexts/CONTEXT_MAP.md) and read only the layer you are touching:

| If you are touching… | Read first |
| :--- | :--- |
| Context index & placement rules | [`contexts/CONTEXT_MAP.md`](contexts/CONTEXT_MAP.md) |
| Product vision, positioning & roadmap | [`contexts/project/overview.md`](contexts/project/overview.md) |
| Slicing math, `Sync` vs `Regroup`, bursts, visual `bandPath` | [`contexts/project/design.md`](contexts/project/design.md) |
| Crate map, SQLite WAL+FTS5 schema, HTTP `/api/*`, CLI/MCP parity | [`contexts/project/architecture.md`](contexts/project/architecture.md) |
| Branching, commit prefixes, SemVer lockstep & releases | [`contexts/engineering/version-control-and-release.md`](contexts/engineering/version-control-and-release.md) |
| Git hooks, Agent `PreToolUse` guards, hygiene & CI | [`contexts/engineering/hooks-and-gates.md`](contexts/engineering/hooks-and-gates.md) |
| Visual identity, color/type tokens, SVG geometry, UI components & design QA | [`DESIGN.md`](DESIGN.md) *(mandatory before any UI, icon, asset, or diagram edit)* |
| Zero-fork vanilla `./ui` rules & README screenshot capture | [`contexts/engineering/ui-zero-fork.md`](contexts/engineering/ui-zero-fork.md) |
| Adding a new harness ingestor or direct LLM API provider | [`contexts/engineering/add-harness-or-provider.md`](contexts/engineering/add-harness-or-provider.md) |
| Orchestration, issue filing, pre-PR, parallelism & auth gates | [`WORKFLOW.md`](WORKFLOW.md) |
| Tiered review (`T0`–`T3`), Occam's Razor simplification & contributing | [`REVIEW.md`](REVIEW.md), [`SIMPLIFY.md`](SIMPLIFY.md) & [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| Rust idioms, async patterns & review rubric | `.agents/skills/rust-best-practices/SKILL.md`, `rust-async-patterns/SKILL.md`, `rust-review/SKILL.md` |
| End-user runtime Skill (`L0 → L1 → L2` progressive disclosure) | [`skills/planar/SKILL.md`](skills/planar/SKILL.md) |

## Directory Contracts

| Path | Responsibility | Boundary Rule |
| :--- | :--- | :--- |
| `crates/planar-core/` | Read-only harness ingestors, burst math, SQLite WAL+FTS5 store, Head–Middle–Tail slicer, CLI/API summarizer, offline pricing, Axum HTTP server, MCP `2025-03-26` engine, Markdown renderers | Library crate (`thiserror`, zero `unwrap()`/`dbg!()` outside tests, `unsafe_code = "forbid"`). Owns all persistence and domain invariants. |
| `crates/planar-cli/` | Sub-5ms native `planar` binary (`status`, `line`, `session`, `search`, `sync`, `regroup`, `open`, `mcp`, `scan`, `serve`) | Thin `clap` + `anyhow` wrapper over `planar-core`. Never write logs to `stdout` in `planar mcp`. |
| `src-tauri/` | Thin Tauri v2 macOS desktop window + Menu-Bar tray (`Planar.app`) | Mounts embedded `planar-core` loopback router; no duplicated business logic or Tauri-only data IPC. |
| `DESIGN.md` | Unified visual & UI design system (brand mark, Light/Dark tokens, `PALETTE`, typography, `bandPath` geometry, components, QA checklist) | Mandatory source of truth before touching `./ui`, `assets/`, icons, diagrams, or any new surface; keep synced with live CSS tokens. |
| `ui/` | Framework-free vanilla HTML/CSS/ESM frontend (`Week`/`Day`/`Month` swimlanes, heatmap, `RECAP`, drag-and-drop deps, Settings, `@container` narrow sidebar) | Zero build step, zero npm runtime deps, standard `fetch('/api/*')` only. Follow [`DESIGN.md`](DESIGN.md) tokens & anti-patterns; must work identically in Tauri, `planar serve`, `npm run preview` (`?mock=1`), and IDE sidebars. |
| `contexts/` | Single durable context root (product, design, architecture, engineering) | Register every new `.md` file in [`contexts/CONTEXT_MAP.md`](contexts/CONTEXT_MAP.md). Separate `ctx:` commits from code commits. |
| `.agents/skills/` | Vendored repo-local engineering skills (`rust-*`, symlinked at `.claude/skills`) | Do not hand-edit vendored `rust-*` skills managed by `skills-lock.json`. |
| `skills/planar/` | Shipped end-user Agent Skill (`SKILL.md`) | Stays 100% aligned with `planar_status` / `planar_line` / `planar_session` / `planar_search` MCP & CLI flags. |
| `.githooks/` & `scripts/hooks/` | Universal Git hooks + Claude/Codex `PreToolUse` PR & Issue guards | Enable locally via `npm run hooks:install`; unit-tested by `npm run test:hooks`. |

## Non-Negotiable Core Invariants

1. **Read-Only on Harness Logs**: Never mutate, lock, or wrap source transcripts under `~/.claude`, `~/.codex`, `~/.gemini/antigravity`, or `~/.dsh`. Stream JSONL incrementally via byte-offset + `(size, mtime_ms)` cursors.
2. **Head–Middle–Tail Slicing (`crates/planar-core/src/processor.rs`)**:
   - Skip LLM summarization when `user_input_count < fewest_user_inputs` (default `3`); render under `Short sessions`.
   - Strip all tool payloads unconditionally; keep full `Opening 20` + **middle user prompts only** + full `Closing 40`, truncated to `longest_user_chars` (`1200`) / `longest_agent_chars` (`1500`).
   - Tag internal CLI digest prompts with `[PLANAR_INTERNAL_DIGEST]`; re-summarize on `slice_config_hash` change or `step_count` growth; permanently lock saved digests if the source `.jsonl` file is pruned on disk.
3. **Stable Taxonomy (`Sync` vs `Regroup`)**: Incremental `Sync` never renames, splits, merges, or overwrites pinned (`pinned = 1`) Lines; explicit `Regroup` only re-clusters unpinned (`pinned = 0`) Lines; archived sessions are never dropped from historical metrics or Line totals.
4. **Honest Active Agent Time (`crates/planar-core/src/burst.rs`)**: Compute active bursts via clamped union (`IDLE_GAP_CAP_MS = 12 min`, `MIN_BURST_MS = 60s`), preserve explicit tool spans, store unpadded intervals in `activity_intervals` for associative incremental merges, distinguish `agent time` vs `wall clock`, and keep `Time` / `Tokens` / `Cost` in separate lenses.
5. **Tauri, SQLite & macOS Runtime Gotchas**: Keep `"dragDropEnabled": false` in `src-tauri/tauri.conf.json`; initialize every `rusqlite` connection with `WAL` + `busy_timeout = 5000` + `synchronous = NORMAL` + `foreign_keys = ON`; hydrate macOS `$PATH` once via `planar_core::env::hydrate_macos_path()`; keep MCP v1 strictly read-only (`readOnlyHint: true`, `destructiveHint: false`).
6. **Unified Design System ([`DESIGN.md`](DESIGN.md))**: Before changing `./ui`, `assets/`, icons, diagrams, or any visual surface, read [`DESIGN.md`](DESIGN.md) first and reuse its exact Light/Dark CSS tokens, `PALETTE` hues, typography, `bandPath` geometry, and anti-patterns (no neon glow, glassmorphism blur, rainbow badges, or unrequested framework dependencies).
7. **Clean-Room Open Source**: Never commit personal paths (`/Users/<you>/...`), private workspace/project names, unreleased internal model codenames, real user transcripts, or API keys.

## Verification & Pre-PR Commands

```bash
npm run hooks:install    # One-time: activate .githooks/ (pre-commit, commit-msg, pre-push)
npm run verify           # Full gate: Biome + node --check + rustfmt + clippy -D warnings + hygiene + cargo test + hook tests
npm run pre-pr           # Runs `npm run verify` and writes HEAD-bound marker required before `gh pr create`
npm run preview          # Static mock UI (?mock=1, add &sidebar=350 for narrow mode)
```
