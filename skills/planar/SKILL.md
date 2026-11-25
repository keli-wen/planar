---
name: planar
description: >-
  Read-only progressive-disclosure context and planning engine over your active
  Lines of agent work, recent multi-harness trajectories, active agent time,
  and cross-session dependencies. Use when recapping what you have been working
  on, restoring context after a break, resolving session dependencies, or
  planning what to tackle next across Claude Code, Codex, Antigravity, and DSH.
---

# Planar — Progressive-Disclosure Work Plane & Planning Skill

Planar organizes scattered coding-agent sessions (even 10+ concurrent sessions launched from a single centralized workspace folder across Claude Code, Codex, Antigravity, and DSH) into semantic **Lines of work** on a `Lines × Time` plane.

Never read raw multi-megabyte `.jsonl` session transcripts to figure out what the user was doing. Traverse Planar's **strictly read-only** disclosure layers via Planar MCP tools (preferred when connected) or the matching `planar` CLI subcommands in **2–3 calls and `< 3k` tokens**:

## Progressive Disclosure Surface (MCP & CLI Parity)

| Layer | MCP Tool | CLI Command | Batch & Token Guardrails | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| **L0** | `planar_status` | `planar status [--days 7] [--cwd <path>]` | Single snapshot (`~300–600 tok`) | Global `Lines × Time` overview, `cwd`-matched Lines, live `has_open_turn` sibling sessions, Sync queue, and **Priority vs. Time drift**. |
| **L1** | `planar_line` | `planar line <line>... [--limit 8]` | **Batch `1..=8` lines** (`~400–700 tok` / line); partial-match tolerant | Pull human `Line Brief` (`description`), `priority`, `status`, active agent time, ordered session digests (`title`, `stopped_at`, `left_open`), and upstream/downstream dependencies in one round-trip. |
| **L2** | `planar_session` | `planar session <id>... [--focus full\|handoff]` | **Bounded batch `1..=3` sessions**; strips all raw tool payloads | Inspect compressed **Head–Middle–Tail** dialogue slices, raw `source_path`, and ready-to-run `Resume` command. Pass `focus: "handoff"` to omit `head` and halve token cost when resuming or unblocking work. |
| **Search** | `planar_search` | `planar search <query> [--line <slug>] [--days <n>]` | Ranked hits (`~300–800 tok`, default `10`) | Local SQLite `FTS5` (`bm25`) recall across `title`, `stopped_at`, `left_open`, and `first_user_prompt`. |
| **Resume / Sync (CLI)** | *(CLI only — MCP v1 is strictly read-only)* | `planar open <id> [--print]` · `planar sync --estimate [--days 7]` · `planar sync [--days 7]` | Sub-5ms local open; read-only dry-run cost/token preview (`--estimate`); explicit batched digest sync | Copy or print the exact harness resume command (`codex resume`, `claude --resume`, `agy --conversation`, `dsh --resume`), preview pending Sync token & USD cost strictly read-only (`--estimate`, without calling any LLM or modifying SQLite), or run end-of-day summarization when explicitly requested by the user. |

### MCP Resources & Prompt
- **Resources** (for `@` mention in chat without spending a tool turn):
  - `planar://status` — 7-day L0 plane snapshot.
  - `planar://line/{slug}` — L1 Line Brief, session digests, and dependency graph for `{slug}`.
- **Prompt**:
  - `planar-plan` — Guided `L0 -> batch L1 -> optional L2` planning rubric (`days`, `cwd`, `focus_line`).

## Batch Planning & Recap Workflow

When the user asks *"what was I doing?"*, *"where do all my Lines stand?"*, *"help me plan the next block"*, or *"where did I leave off?"*:

1. **L0 Snapshot**: Call `planar_status` (passing the current workspace `cwd` when known) to inspect active Lines, live `has_open_turn` sibling sessions running in other harnesses, and Priority vs. Time drift.
2. **Single Batch L1**: Call `planar_line` **once** with an array of relevant `Active` / `P1 Urgent` / `P2 High` / `cwd`-matched Line slugs (`lines: ["...", "..."]`, up to 8 Lines). Treat each Line's `Line Brief` (`description`) as the human's authoritative scope & forward intent.
3. **Optional Bounded L2 / Search**: Only if a specific session handoff or blocker needs deeper dialogue detail, call `planar_session` (`session_ids: [...]`, up to 3 IDs, `focus: "handoff"`) or `planar_search`.
4. **Planning Synthesis Output** — always structure your planning brief around three concrete signals:
   - **1. Unblocked / 80%-done sessions to close**: Threads whose upstream dependency finished or that sit near completion with a crisp `Left open` step.
   - **2. Priority vs. Time drift**: Mismatches between declared `P1`/`P2` priorities (or `Line Brief` goals) and where active agent time actually went (e.g., `One-off` or unpinned drift).
   - **3. Concrete next session to resume or spawn**: One recommended focus (citing exact Line slug + session ID) that minimizes context-switching and avoids colliding with live sibling sessions in the same `cwd`.

## Guardrails
- Planar MCP v1 is **100% read-only**: never attempt to mutate Lines, session assignments, or dependency links unless the user explicitly asks to perform an API/CLI administrative action outside MCP planning.
- Never open or grep raw `~/.claude`, `~/.codex`, or `~/.gemini/antigravity` logs unless L2's `Transcript` pointer is explicitly needed for line-by-line archaeology.
