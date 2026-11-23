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
- **Isolate Work**: Create a focused feature branch (or `git worktree` when running concurrent agents): `feat/<topic>`, `fix/<topic>`, `ctx/<topic>`, or `chore/<topic>`.
- **Commit Prefixes**: Use Planar's Conventional Commit prefixes (`ctx:`, `core:`, `cli:`, `app:`, `ui:`, `skill:`, `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`, `ci:`, `release:`) — see [`contexts/engineering/version-control-and-release.md`](contexts/engineering/version-control-and-release.md). Keep pure `contexts/` edits in separate `ctx:` commits when practical.
- **Keep Clean-Room Hygiene**: Never commit personal paths (`/Users/<you>/...`), private project names, unreleased internal model codenames, real session logs, or API keys.

---

## 4. Verification & Pull Request Checklist

Before opening a Pull Request:

1. Determine the PR's **Review Tier (`T0`–`T3`)** using [`REVIEW.md`](REVIEW.md) and complete its required review pass.
2. Run the full verification gate (and write the `HEAD`-bound pre-PR marker for agent `PreToolUse` guards):
   ```bash
   npm run pre-pr
   ```
3. Fill out [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md) concisely — emphasize **Why**, **Design Trade-offs**, **Review Tier**, **Context Impact**, and **Verification Proof**.
4. For **T3** architectural proposals, open an RFC Issue (or context spec PR under `contexts/`) for Design Gate approval before implementing across crates.
