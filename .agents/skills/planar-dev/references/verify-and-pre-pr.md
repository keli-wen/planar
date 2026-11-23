# Reference: Verification Ladder & Pre-PR Final Check

> Run this SOP before opening any Pull Request (`gh pr create`). Rules source: `REVIEW.md` and `contexts/engineering/hooks-and-gates.md`.

## Step 1 — Classify Review Tier (`REVIEW.md`)

Determine the highest matching tier from your diff:

- **T0 (Trivial / Docs / Pure Context)**: Only `contexts/`, root Markdown docs, comments, or `.github/` templates.
- **T1 (Standard Scoped Change)**: Scoped UI tweak, CLI flag/output update, offline test/fixture addition, or non-breaking bug fix with no schema/invariant changes.
- **T2 (Core Invariant / Schema / Security / Release)**: Any touch to `db.rs` schema/migrations, `burst.rs`, `ingest/*`, `processor.rs` slicing/prompts, `mcp.rs`, `server.rs` security/routes, `src-tauri/`, `.githooks/`, `scripts/hooks/`, or versioned release bump.
- **T3 (Architectural / Multi-Crate RFC)**: New delivery surface, new persistence architecture, MCP write capabilities, or cross-crate breaking contract changes.

## Step 2 — Run Required Review Pass

- **T0**: Run `npm run check:hygiene`; verify `contexts/CONTEXT_MAP.md` registers every new context doc.
- **T1**: Self-review full `git diff master...HEAD`; if Rust files changed, apply `.agents/skills/rust-review/SKILL.md` checklist.
- **T2**: Run full `rust-review` rubric + independent/fresh-context review pass checking every invariant in `references/architecture.md` and backward-compatibility of migrations; human maintainer approval required before merge.
- **T3**: Confirm an approved RFC issue or `contexts/` design spec exists before implementation, plus T2 checks.

## Step 3 — Run `npm run pre-pr` (Writes Guard Marker)

```bash
npm run pre-pr
```

What `npm run pre-pr` executes:
1. `npm run lint` (`biome lint`, `node --check`, `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`)
2. `npm run check:hygiene` (privacy patterns, relative Markdown links, `CONTEXT_MAP.md` registration, version lockstep, symlinks)
3. `cargo test --workspace`
4. `npm run test:hooks`
5. Writes `.planar-agent/pre-pr-check-pass.json` bound to `git rev-parse HEAD`, which unblocks the `PreToolUse` guard on `gh pr create`.

> If you make even a 1-line commit after `npm run pre-pr`, `HEAD` changes and the `PreToolUse` guard will block `gh pr create` until you re-run `npm run pre-pr`.

## Step 4 — Draft PR Using `.github/PULL_REQUEST_TEMPLATE.md`

- PR title follows Planar's commit prefix grammar (`core:`, `cli:`, `ui:`, `app:`, `ctx:`, `skill:`, `feat:`, `fix:`, `chore:`).
- Fill in **Why**, **What Changed**, **Review Tier**, **Invariant & Context Impact**, **Verification Proof** (exact commands + pass counts), and **Linked Issues** (`Closes #N` / `Refs #N`).
- Delete optional sections that do not apply.
