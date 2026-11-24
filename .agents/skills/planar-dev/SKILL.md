---
name: planar-dev
description: >-
  Contributor and implementation workflow for developing Planar (`planar-core`,
  `planar-cli`, `src-tauri`, `./ui`, `contexts/`, and `skills/planar/`). Use when
  implementing features or bug fixes, adding a harness ingestor or API provider,
  editing the zero-fork vanilla UI, filing contexts-first GitHub issues, running
  pre-PR final checks (`REVIEW.md` tiers), or preparing a versioned release.
---

# Planar Dev

Implementation, verification, issue-filing, and release workflow for the `keli-wen/planar` repository.

## Start Here

1. Read `contexts/CONTEXT_MAP.md` before reading or writing any durable context.
2. Dispatch only to the context & reference for the surface you are touching:

| Target Surface | Durable Context (Source of Truth) | Dev Workflow Reference |
| :--- | :--- | :--- |
| `crates/planar-core/` | `contexts/project/architecture.md`, `contexts/project/design.md` | `references/architecture.md` + `rust-best-practices` + `rust-async-patterns` |
| `crates/planar-cli/` | `contexts/project/architecture.md` (§5) | `references/architecture.md` + `rust-best-practices` |
| `src-tauri/` | `contexts/project/architecture.md` (§6) | `references/architecture.md` |
| `ui/` & `assets/` | `DESIGN.md`, `contexts/engineering/ui-zero-fork.md` | `references/ui-zero-fork.md` |
| New harness ingestor / API provider | `contexts/project/architecture.md` | `references/add-harness-or-provider.md` |
| Issues (`gh issue create`) | `contexts/CONTEXT_MAP.md`, `REVIEW.md` | `references/issue-filing.md` |
| Pre-PR & Review (`gh pr create`) | `REVIEW.md`, `contexts/engineering/hooks-and-gates.md` | `references/verify-and-pre-pr.md` |
| Versioning & Release | `contexts/engineering/version-control-and-release.md` | `references/release.md` |

## Companion Repo-Local Skills

- **Rust edits**: Always pair with `.agents/skills/rust-best-practices/SKILL.md` and `.agents/skills/rust-async-patterns/SKILL.md`; run `.agents/skills/rust-review/SKILL.md` during Tier 1+ code review.
- **Frontend (`./ui`)**: Planar deliberately does **not** use React, JSX, or bundlers (see `contexts/engineering/ui-zero-fork.md`). Never add a React skill or introduce React dependencies — follow `references/ui-zero-fork.md`.

## Default Workflow

1. **Check invariants first**: Read `AGENTS.md` and the target section of `contexts/project/architecture.md` before designing changes.
2. **Isolate concurrent work**: When multiple agents work in parallel, use `git worktree` so edits and `cargo test` builds never collide.
3. **Reuse shared primitives**: Keep all persistence, slicing, resume-command generation, Markdown rendering, and MCP logic in `crates/planar-core/` — never duplicate logic across `planar-cli`, `src-tauri`, or `./ui`.
4. **Separate `ctx:` and code commits**: Keep pure `contexts/` edits in dedicated `ctx:` commits unless an API/schema contract update belongs atomically with the code change.
5. **Climb the Verification Ladder** before declaring done:
   - **L1 — Static & Hygiene**: `npm run lint && npm run check:hygiene`
   - **L2 — Unit & Integration Tests**: `cargo test --workspace && npm run test:hooks`
   - **L3 — Surface Smoke**: CLI isolated temp-DB check (`PLANAR_DB_PATH=$(mktemp -d)/planar.db cargo run -p planar-cli -- status --no-scan`) and/or UI preview (`npm run preview` with `?mock=1` & `?mock=1&sidebar=350`) when `./ui` changed.
   - **L4 — Review Tier & Pre-PR Gate**: Apply the required review depth from `REVIEW.md` (`T0`–`T3`), then run `npm run pre-pr` (writes the `HEAD`-bound `.planar-agent/pre-pr-check-pass.json` marker required by the `gh pr create` guard).

## Sub-Command & Reference Router

Load only the reference needed for the task:

- **Module & invariant lookup** → `references/architecture.md`
- **Pre-PR check / review / `gh pr create`** → `references/verify-and-pre-pr.md`
- **Filing a contexts-first issue / `gh issue create`** → `references/issue-filing.md`
- **Editing `./ui` or refreshing `assets/ui-*.png`** → `references/ui-zero-fork.md`
- **Adding a harness adapter or LLM API provider** → `references/add-harness-or-provider.md`
- **Bumping version / cutting a release / updating local `Planar.app`** → `references/release.md`
