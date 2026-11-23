# Hooks & Mechanical Verification Gates

> Authoritative design of Planar's two-layer guardrail system: universal Git hooks (`.githooks/`) and Agent `PreToolUse` hooks (`.claude/settings.json` + `.codex/hooks.json`), backed by GitHub Actions CI.

---

## 1. Philosophy: Mechanical Gates Over Prose Reminders

Prose instructions in `AGENTS.md` or `CONTRIBUTING.md` drift or get skipped under long sessions. Planar turns every critical invariant into a **fast, deterministic, offline check** enforced at four concentric rings:

| Ring | Trigger | Mechanism | Latency |
| :--- | :--- | :--- | :--- |
| **1. Pre-Commit** | `git commit` | [`.githooks/pre-commit`](../../.githooks/pre-commit) — staged privacy/hygiene scan, Biome/syntax check on staged UI/scripts, `cargo fmt --check` on staged Rust | `< 1s` |
| **2. Commit-Msg** | `git commit` | [`.githooks/commit-msg`](../../.githooks/commit-msg) — validates Planar Conventional Commit prefixes | `< 50ms` |
| **3. Pre-Push / Pre-PR** | `git push` or `gh pr create` | [`.githooks/pre-push`](../../.githooks/pre-push) runs `npm run verify`; Agent `PreToolUse` guard blocks `gh pr create` without a fresh `HEAD`-bound marker from `npm run pre-pr` | `3–15s` |
| **4. CI** | GitHub PR / push to `master` | [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) — runs full `npm run verify` on clean runners | `1–3m` |

---

## 2. Layer A — Universal Git Hooks (`.githooks/`)

Tracked directly in the repository (zero npm hook manager dependencies like Husky). Enable once per clone:

```bash
npm run hooks:install
# equivalent to: git config core.hooksPath .githooks
```

- **`.githooks/pre-commit`**:
  1. Runs `node ./scripts/check-hygiene.mjs` (privacy patterns, relative Markdown links, `contexts/CONTEXT_MAP.md` registration, lockstep versions across `Cargo.toml`/`package.json`/`tauri.conf.json`).
  2. If staged files touch `ui/` or `scripts/`, runs `npm run lint:ui` (Biome + `node --check`).
  3. If staged files touch `*.rs`, runs `cargo fmt --all -- --check`.
- **`.githooks/commit-msg`**:
  - Rejects commit subjects that do not match Planar's commit prefix grammar (`ctx`, `core`, `cli`, `app`, `ui`, `skill`, `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `ci`, `release`) while allowing Git merge/revert commits.
- **`.githooks/pre-push`**:
  - Runs `npm run verify` (`lint` + `check:hygiene` + `cargo test --workspace` + `test:hooks`) so broken builds or Clippy warnings never leave your machine.

---

## 3. Layer B — Agent `PreToolUse` Hooks (`.claude/` & `.codex/`)

Inspired by durable harness-engineering workflows, Planar ships lightweight, pure-stdlib Python `PreToolUse` guards wired into both [`.claude/settings.json`](../../.claude/settings.json) (Claude Code) and [`.codex/hooks.json`](../../.codex/hooks.json) (Codex):

1. **[`scripts/hooks/pre_tool_use_pr_guard.py`](../../scripts/hooks/pre_tool_use_pr_guard.py)**:
   - Watches Bash tool invocations for `gh pr create`.
   - Requires a valid JSON marker at `.planar-agent/pre-pr-check-pass.json` whose `head` matches the target branch/worktree's current `git rev-parse HEAD` (generated automatically by `npm run pre-pr` or the `planar-dev` pre-PR workflow).
   - Prevents coding agents from opening PRs with unverified diffs or stale check passes after making "one last quick tweak".
2. **[`scripts/hooks/pre_tool_use_issue_guard.py`](../../scripts/hooks/pre_tool_use_issue_guard.py)**:
   - Watches Bash tool invocations for `gh issue create`.
   - Requires a fresh issue-filing marker at `.planar-agent/issue-file-check-pass.json` matching the drafted title/body hash (or written during `.agents/skills/planar-dev/references/issue-filing.md`), ensuring agent-filed issues always include `contexts-first` handoff fields and a suggested Review Tier.
   - Humans filing issues in the GitHub UI use `.github/ISSUE_TEMPLATE/*.yml` directly and are unaffected.

All marker files live under `.planar-agent/` (gitignored) and both guards are unit-tested by `npm run test:hooks` (`python3 -m unittest discover -s scripts/hooks -p 'test_*.py'`).

---

## 4. Hygiene Checker (`scripts/check-hygiene.mjs`)

Zero-dependency Node ESM checker invoked by `npm run check:hygiene`, `.githooks/pre-commit`, and CI:

1. **Privacy scan**: Fails on personal absolute paths (`/Users/<name>` other than `/Users/demo`), private identifiers, unreleased internal model codenames, or leaked API key prefixes.
2. **Relative Markdown link check**: Fails on broken relative links in `README.md`, `README.zh-CN.md`, `AGENTS.md`, `CONTRIBUTING.md`, `REVIEW.md`, `contexts/**/*.md`, `skills/**/*.md`, and `.agents/skills/planar-dev/**/*.md`.
3. **`CONTEXT_MAP.md` registration check**: Fails if any `.md` file exists under `contexts/` without being listed in `contexts/CONTEXT_MAP.md`.
4. **Version lockstep check**: Fails if `Cargo.toml`, `package.json`, and `src-tauri/tauri.conf.json` disagree on version.
5. **Dual-entry symlink check**: Confirms `CLAUDE.md -> AGENTS.md` and `.claude/skills -> ../.agents/skills` stay intact.
