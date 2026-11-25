# Planar — Project Overview & Roadmap

> **Status**: Experimental (`v0.1.0-alpha.0`) — active dogfooding and pre-v0.1 public release preparation (`keli-wen/planar`).

## 1. Product Positioning

**A shared map of the work you do with agents.**

Planar quietly reads the sessions your coding agents already leave on disk and lays them out as persistent lines of work over time (`Lines × Time`) — giving both humans and personal agents one shared substrate for three compounding loops:

- **Plan (Where things stand)**: Instant orientation across active lines of work, where each session stopped, what is left open, and cross-session dependencies — even when 10+ concurrent sessions share a single centralized workspace or monorepo.
- **Reflect (Where the time went)**: Honest accounting of real active `agent time` vs. `wall clock`, 20-week contribution rhythm, and separate `Tokens` & `Cost` lenses.
- **Context (Where your agent picks up)**: Progressive-disclosure context (`L0 → L1 → L2` + FTS5) via CLI, read-only MCP, and Skills so any personal agent can read the story of a line in `< 3k` tokens instead of grepping megabytes of raw tool logs.

## 2. Core Value Proposition

1. **Semantic Lines of Work (`Pinned` vs `Found by Planar`)**: Organizes sessions into persistent project lanes with optional user `Scope:` notes and Merge-to-Teach consolidation — regardless of shared `cwd`.
2. **Token-Frugal Head–Middle–Tail Slicing**: Skips `< 3`-turn throwaway chats; reads full opening (`20`) and closing (`40`) turns plus **middle user prompts only**, stripping tool payloads unconditionally (~5% token cost).
3. **Zero-Surprise Stability (`Sync` vs `Regroup`)**: Incremental `Sync` never renames, splits, or merges Lines behind your back; explicit `Regroup` only re-clusters unpinned Lines on demand.
4. **Honest Telemetry (`Time`, `Tokens`, `Cost`)**: Clamped burst union (`12 min` idle cap) separates real active agent bursts from idle open tabs, distinguishing **`agent time` vs `wall clock`** alongside a 20-week contribution calendar and offline `Tokens` / `Cost` lenses.
5. **One Read-Only Local Core, Three Surfaces**:
   - **Standalone Local App (`Planar.app` / `planar serve`)**: ~15 MB Tauri v2 macOS desktop + Menu-Bar tray and loopback HTTP server (`127.0.0.1:4310`).
   - **Container-Responsive Sidebar UI**: Zero-fork vanilla HTML/CSS/ESM `./ui` adapting cleanly down to `320px` IDE auxiliary panes (`?sidebar=350`).
   - **Progressive-Disclosure CLI, Read-Only MCP (`2025-03-26`) & Skill**: Sub-5ms `planar` CLI + Stdio/HTTP MCP + [`skills/planar/SKILL.md`](../../skills/planar/SKILL.md) exposing `L0 → batch L1 → bounded L2 + FTS5 Search` in `< 3k` tokens.

## 3. Non-Goals

- **Not an agent wrapper**: Never intercepts, proxies, locks, or mutates source harness transcripts (`~/.claude`, `~/.codex`, `~/.gemini/antigravity`, `~/.dsh`).
- **Not heavyweight RAG**: No vector database over megabytes of raw tool outputs; deterministic Head–Middle–Tail slicing + local SQLite FTS5 solves 95% of recall and planning workflows with zero external infra.
- **Never overrides human taxonomy during Sync**: Pinned Lines and explicit `Scope:` notes always outrank model clustering.

## 4. Roadmap

| Phase | Status | Scope |
| :--- | :--- | :--- |
| **Phase 0 — Design, Icon & Reference UI** | Done | Clean-room visual spec, `24×24` geometric vector icon, OKLab/CVD-validated palette, interactive `./ui` (`bandProfile`/`bandPath`, MD3 live indicator, 3-step staircase priority, 3-zone Settings slicer, `@container` narrow sidebar). |
| **Phase 1 — Rust Core, CLI & macOS `Planar.app`** | Done | `planar-core` + `planar-cli` + `src-tauri` workspace, SQLite WAL store, 4 read-only incremental JSONL ingestors (Claude Code, Codex, Antigravity, DSH), clamped burst math, 20-week heatmap, offline Token/Cost lenses, embedded loopback Axum router. |
| **Phase 2 — LLM `Sync` / `Regroup` & Persistence** | Done | Head–Middle–Tail slicer, `slice_config_hash`, dual CLI/API fast-model router, Merge-to-Teach scope notes, NDJSON streaming `Sync`, manual drag-and-drop & searchable session dependency tree. |
| **Phase 3 — Progressive-Disclosure CLI, MCP & Skill** | Done | Full `L0 status → L1 line → L2 session + FTS5 search` parity across native CLI and read-only MCP `2025-03-26` (Stdio + Streamable HTTP `/mcp`), `planar sync` / `regroup` / `open`, tray one-click Sync, and `skills/planar/SKILL.md`. |
| **Phase 4 — Experimental Public Release & v0.2** | In Progress | Agent-Native open-source repo scaffolding (`contexts/`, `WORKFLOW.md`, `REVIEW.md`, `SIMPLIFY.md`, dual-layer hooks, CI), clean-room `v0.1.0` public release, automatic 2-stage LLM cross-session Link Pass (`manual`-wins + `dismissed` guardrails), packaged IDE/Harness sidebar wrappers, and guarded background `auto_sync` scheduling. |
