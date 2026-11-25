<p align="center">
  <img src="./assets/icon.svg" width="76" height="76" alt="Planar icon" />
</p>

<h1 align="center">Planar</h1>

<p align="center">
  <strong>A shared map of the work you do with agents.</strong><br />
  <a href="./README.md">English</a> · <a href="./README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  Planar quietly reads the sessions your agents already leave on disk — Claude Code, Codex, Antigravity, and DeepSeek Harness (<code>dsh</code>) — and lays them out as lines of work over time: where each one stopped, where the hours went, and what to hand your next agent.
</p>

<p align="center">
  <code>local-first</code> · <code>read-only</code> · <code>macOS App · Skill · MCP</code> · <code>MIT</code>
</p>

> [!NOTE]
> **Experimental (`0.1.0-alpha.0`)** — Planar is under active experimental development and doubles as a reference **Agent-Native Open-Source Repository**. Interfaces, schemas, and agent workflows evolve rapidly while we stabilize `v0.1.0`.

<p align="center">
  <img src="./assets/ecosystem.svg" width="860" alt="Planar Ecosystem Overview" />
</p>

## What You Get

Planar runs on one strictly **read-only, zero-wrapper** local core and meets you through two primary surfaces (with a `320px`-ready UI designed to mount in IDE sidebars next):

1. **Planar App (macOS Desktop & Menu Bar)** — A calm visual map of your lines of work over time (`Week`, `Day`, `Month`, 20-week heatmap, inline `RECAP` cards, drag-and-drop session dependencies, and one-click resume).
2. **Agent Skill & Read-Only MCP** — Connect [`skills/planar/SKILL.md`](./skills/planar/SKILL.md) so coding agents pull context on demand with zero idle token overhead, or attach Planar's read-only **MCP** server for Claude Desktop and any personal agent without shell access. You never need to memorize CLI flags — just talk to your agent naturally.

## Core User Journeys

| **Where things stand (Plan)** | **Where the time went (Reflect)** | **Where your agent picks up (Context)** |
| :--- | :--- | :--- |
| Return after a break or overnight and see every active line of work, where it `Stopped at`, what is `Left open`, and how sessions depend on each other. | Audit honest active `agent time` vs. `wall clock` (12m idle cap), 20-week consistency, and separate offline `Tokens` & `Cost` lenses. | Ask any personal agent *"what stopped yesterday?"* or *"continue the search refactor"* — it reads `< 3k` tokens of noise-stripped progressive context instead of megabytes of raw logs. |

<p align="center">
  <img src="./assets/ui-week.png" width="760" alt="Planar Week View" />
</p>

<p align="center">
  <img src="./assets/ui-day.png" width="760" alt="Planar Day View and Session Recap" />
</p>

## Why a Map?

- **Semantic Lines, Not Folder Buckets**: Even when ten parallel sessions share one centralized workspace, Planar organizes them into stable lines (`Pinned` + `Found by Planar` + `One-off`). Incremental `Sync` never overwrites your pinned structure; explicit `Regroup` reorganizes unpinned lines only when you ask.
- **Head–Middle–Tail Slicing**: Throwaway `< 3`-turn chats stay out of the way. Tool output noise is stripped unconditionally while opening turns, **mid-session user prompts** (capturing direction changes), and closing turns are preserved at ~5% token cost.

## Try It Locally (Experimental)

Prebuilt GitHub Releases are not published yet while `v0.1.0` stabilizes. You can run Planar from source today (requires Rust stable Edition `2024` and Node.js `>= 20`):

<details>
<summary><strong>Build the macOS App, preview mock UI, or install the Agent Skill / MCP</strong></summary>

```bash
# Install dev tooling & repo Git hooks
npm install
npm run hooks:install

# Instant zero-backend UI preview (?mock=1, add &sidebar=350 for narrow mode)
npm run preview

# Build & launch macOS Planar.app (desktop + menu bar)
npm run app:install && npm run app:open

# Install the local engine for Agent Skill / MCP integration
cargo install --path crates/planar-cli
planar mcp config    # Prints ready-to-paste MCP config for your agent client
```

Link or copy [`skills/planar/SKILL.md`](./skills/planar/SKILL.md) into your agent's skills directory to enable on-demand planning, reflection, and session handoff.

</details>

## Agent-Native Repository

Planar is structured so any coding agent (**Claude Code**, **Codex**, **Antigravity**, **DSH**, **Cursor**) or human contributor can orient in seconds and self-verify mechanically:

| Layer | Path | Purpose |
| :--- | :--- | :--- |
| **Agent Router** | [`AGENTS.md`](./AGENTS.md) (`CLAUDE.md` symlink) | Compact L0 entry point: invariants, directory boundaries, verification commands |
| **Design System** | [`DESIGN.md`](./DESIGN.md) | Unified visual identity, Light/Dark tokens, typography, `bandPath` geometry, and UI QA checklist |
| **Durable Contexts** | [`contexts/CONTEXT_MAP.md`](./contexts/CONTEXT_MAP.md) | Single home for product design, architecture, release/versioning, UI contract, and hooks |
| **Agentic Workflow & Simplifier** | [`WORKFLOW.md`](./WORKFLOW.md) & [`SIMPLIFY.md`](./SIMPLIFY.md) | Issue-first delivery loop, verification ladder, auth gates, and Occam's Razor diff rubric (plus vendored Rust skills under `.agents/skills/`, symlinked at `.claude/skills`) |
| **Risk-Tiered Review** | [`REVIEW.md`](./REVIEW.md) & [`CONTRIBUTING.md`](./CONTRIBUTING.md) | `T0`–`T3` review tiers by blast radius, invariant checklists, and contribution guide |
| **Mechanical Guardrails** | [`.githooks/`](./.githooks) & [`scripts/hooks/`](./scripts/hooks) | Universal Git hooks, Claude/Codex `PreToolUse` PR & Issue guards, and `npm run verify` |
| **Shipped Agent Skill** | [`skills/planar/SKILL.md`](./skills/planar/SKILL.md) | End-user progressive-disclosure protocol (`L0 → batch L1 → bounded L2`) |

## License

Released under the [MIT License](./LICENSE).
