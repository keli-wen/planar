# Contributing to Planar

Thank you for contributing to **Planar**! This repository is built to welcome both **human engineers** and **coding agents** (Claude Code, Codex, Antigravity, DeepSeek Harness, Cursor) through explicit context navigation, tiered reviews, and fast local verification gates.

> **Experimental Notice**: Planar is under active experimental development (`v0.1.0-alpha.0`). Interfaces, SQLite schemas (with backward-compatible migrations), and CLI/MCP surfaces evolve rapidly — please read `contexts/CONTEXT_MAP.md` before starting large work.

---

## 1. Quick Local Setup

**Prerequisites**: Rust (stable, Edition `2024`), Node.js `>= 20`, Python `3` (stdlib only, for agent hook tests).

```bash
git clone https://github.com/keli-wen/planar.git
cd planar
npm install
npm run hooks:install      # Activates repo-tracked .githooks/ (pre-commit, commit-msg, pre-push)
npm run verify             # Runs full lint, hygiene check, cargo test --workspace, and hook tests
```

Run local surfaces:
- Static mock UI preview (zero backend): `npm run preview` → `http://127.0.0.1:4310/ui/?mock=1` (`&sidebar=350` for narrow mode)
- Headless local daemon + UI + `/mcp`: `cargo run -p planar-cli -- serve`
- macOS desktop & menu-bar app dev mode: `npm run app:dev`

---

## 2. How to Orient Yourself (Progressive Disclosure)

1. **Coding Agents**: Start at [`AGENTS.md`](AGENTS.md) (`CLAUDE.md` is a symlink) and `.agents/skills/planar-dev/SKILL.md`.
2. **All Contributors**: Read [`contexts/CONTEXT_MAP.md`](contexts/CONTEXT_MAP.md) to jump directly to the relevant design, architecture, UI, or release doc — never guess invariants.
3. **Rust Contributors**: Follow workspace lints (`unsafe_code = "forbid"`, `clippy::pedantic`, `unwrap_used = "deny"`) and repo-local references in `.agents/skills/rust-best-practices/` and `.agents/skills/rust-async-patterns/`.
4. **Frontend (`./ui`) Contributors**: Planar intentionally uses **zero-dependency vanilla HTML/CSS/ESM** (no React or bundler — see [`contexts/engineering/ui-zero-fork.md`](contexts/engineering/ui-zero-fork.md)).

---

## 3. Issue, Branch & Commit Workflow

- **File Contexts-First Issues**: Use `.github/ISSUE_TEMPLATE/` forms (`bug`, `feature`, `harness-adapter`, `context-update`) so any human or clean-session agent can pick up the issue with clear `Source Context`, `Review Tier`, and `Verification Plan`.
- **Apply Agent-Native Labels**: Every issue (and PR when triaging) should carry labels from the orthogonal taxonomy in [§4](#4-issue--pr-labeling-guide-agent-native-taxonomy) so humans and coding agents can route context, review depth, and dispatch readiness without reading prose first.
- **Isolate Work**: Create a focused feature branch (or `git worktree` when running concurrent agents): `feat/<topic>`, `fix/<topic>`, `ctx/<topic>`, or `chore/<topic>`.
- **Commit Prefixes**: Use Planar's Conventional Commit prefixes (`ctx:`, `core:`, `cli:`, `app:`, `ui:`, `skill:`, `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`, `ci:`, `release:`) — see [`contexts/engineering/version-control-and-release.md`](contexts/engineering/version-control-and-release.md). Keep pure `contexts/` edits in separate `ctx:` commits when practical.
- **Keep Clean-Room Hygiene**: Never commit personal paths (`/Users/<you>/...`), private project names, unreleased internal model codenames, real session logs, or API keys.

---

## 4. Issue & PR Labeling Guide (Agent-Native Taxonomy)

Planar treats GitHub labels as **machine-readable routing metadata** for both human maintainers and autonomous coding agents. Every new issue should carry **4 core axes** (plus optional Priority):

> **Minimum labeling rule**: `1 Type` + `1+ Surface (area:*)` + `1 Review Tier (tier:*)` + `1 Agent Readiness (agent:*)` *(+ optional `P0`/`P1`/`P2`)*.

| Axis | Labels | Purpose & Agent Routing Semantics |
| :--- | :--- | :--- |
| **Type** *(pick 1)* | `bug`, `enhancement`, `perf`, `refactor`, `rfc`, `documentation` | Classifies intent. Use `rfc` for cross-cutting T3 designs before code; `perf` for latency/concurrency/memory work; `refactor` for behavior-preserving cleanup or SSOT consolidation. |
| **Surface** *(pick 1+)* | `area:core`, `area:cli`, `area:ui`, `area:app`, `area:mcp`, `area:harness`, `area:context` | Maps 1:1 to [`AGENTS.md`](AGENTS.md) directory contracts so a clean-session agent loads only the matching `contexts/` doc and skill (e.g., `area:ui` → [`DESIGN.md`](DESIGN.md) + `ui-zero-fork.md`; `area:harness` → `add-harness-or-provider.md`). |
| **Review Tier** *(pick 1)* | `tier:T0`, `tier:T1`, `tier:T2`, `tier:T3` | Mirrors [`REVIEW.md`](REVIEW.md) blast-radius policy (**Rule of Highest Tier**). Determines mechanical gates (`check:hygiene` vs `pre-pr`) and review mode (self-check, fresh-context, adversarial, or two-gate RFC). |
| **Agent Dispatch** *(pick 1)* | `agent:ready`, `agent:needs-spec`, `agent:human-gate` | Controls autonomous pickup:<br>• `agent:ready`: Contexts-first handoff complete (`Source Context`, scope boundaries, verification plan) — safe for a fresh coding agent to claim.<br>• `agent:needs-spec`: Missing context links, repro steps, or open design choices — refine before dispatching an agent.<br>• `agent:human-gate`: Blocked on human maintainer decision, T3 Design Gate, visual sign-off, or credentials/release action. |
| **Priority** *(optional)* | `P0`, `P1`, `P2` | Matches [`REVIEW.md`](REVIEW.md) severity: `P0` (blocker / data loss / invariant break), `P1` (high-impact UX/speed pain point or active milestone), `P2` (incremental polish). |
| **Triage** *(as needed)* | `good first issue`, `help wanted`, `accessibility`, `duplicate`, `invalid`, `question`, `wontfix` | Standard community & maintainer triage signals. |

### Labeling Tips for Humans & Coding Agents

1. **Follow the Rule of Highest Tier**: If an issue touches both `./ui` (`T1`) and `crates/planar-core/src/processor.rs` (`T2`), tag both `area:ui` and `area:core`, and assign `tier:T2`.
2. **Never mark half-baked issues `agent:ready`**: Only apply `agent:ready` once `Source Context`, concrete acceptance criteria, out-of-scope boundaries, and exact verification commands are filled in (see `.agents/skills/planar-dev/references/issue-filing.md`).
3. **Flip dispatch state on transitions**: When an RFC is awaiting Design Gate approval, tag `rfc` + `tier:T3` + `agent:human-gate`; once approved with a crisp spec, flip to `agent:ready`.
4. **CLI example** (after writing the draft marker via `scripts/hooks/pre_tool_use_issue_guard.py --write-marker`):
   ```bash
   gh issue create \
     --title "[Perf] Parallelize Sync pipeline with bounded concurrency" \
     --body-file /tmp/issue-body.md \
     --label "perf,area:core,area:ui,tier:T2,agent:ready,P1"
   ```

---

## 5. Verification & Pull Request Checklist

Before opening a Pull Request:

1. Determine the PR's **Review Tier (`T0`–`T3`)** using [`REVIEW.md`](REVIEW.md) and complete its required review pass.
2. Run the full verification gate (and write the `HEAD`-bound pre-PR marker for agent `PreToolUse` guards):
   ```bash
   npm run pre-pr
   ```
3. Fill out [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md) concisely — emphasize **Why**, **Design Trade-offs**, **Review Tier**, **Context Impact**, and **Verification Proof**.
4. For **T3** architectural proposals, open an RFC Issue (or context spec PR under `contexts/`) for Design Gate approval before implementing across crates.
