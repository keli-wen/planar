# Planar — Agentic Development Workflow (`WORKFLOW.md`)

> Orthogonal orchestration contract for `keli-wen/planar`: defines issue intake, episode delivery order, conflict-domain parallelism, verification target assignment, and human authorization gates.
>
> **ADLC contract separation**: [`AGENTS.md`](AGENTS.md) holds always-on identity, invariants, and scenario routing; `WORKFLOW.md` governs *how work moves*; independent judge rubrics live in [`REVIEW.md`](REVIEW.md), [`DESIGN.md`](DESIGN.md), and [`SIMPLIFY.md`](SIMPLIFY.md).

---

## 1. Issue-First Intake & Filing

Every non-trivial bug fix, refactor, performance change, or feature starts as a **contexts-first GitHub Issue** that a clean-session agent can execute without chat history:

1. **Choose the matching form** under `.github/ISSUE_TEMPLATE/`:

   | Template | Default Labels | When to use |
   | :--- | :--- | :--- |
   | `bug.yml` | `bug` | Unexpected behavior, crash, wrong active time, parser bug, UI regression |
   | `feature.yml` | `enhancement` | New user-facing capability or workflow improvement |
   | `harness-adapter.yml` | `enhancement`, `area:harness` | New coding-agent harness ingestor or LLM API provider |
   | `context-update.yml` | `documentation`, `area:context` | Updating `contexts/` or evolving workflows, rubrics, or hooks |

2. **Required handoff sections**:
   - `Source Context` (exact `contexts/` files, code paths, and related issues/PRs)
   - `Objective & Scope Boundaries` (explicit **In Scope** and **Out of Scope**)
   - `Suggested Review Tier` (`T0`–`T3` per [`REVIEW.md`](REVIEW.md))
   - `Context Action & Target Context` (`Update existing` / `Create new` / `No context impact`)
   - `Verification & Done Criteria` (concrete runnable commands and assertions)
3. **Agent-native labels** (4 core axes per [`CONTRIBUTING.md`](CONTRIBUTING.md)): `1 Type` + `1+ Surface (area:*)` + `1 Review Tier (tier:T*)` + `1 Dispatch State (agent:ready | agent:needs-spec | agent:human-gate)` (+ optional `P0`–`P2`).
4. **Mechanical filing guard**: Write the draft body to a temporary file, record the `(title, body)` marker, create the issue, and delete the temp file (if `gh` is unavailable or only a draft was requested, return the Markdown draft in chat):
   ```bash
   python3 scripts/hooks/pre_tool_use_issue_guard.py --write-marker \
     --title "<issue title>" \
     --body-file /tmp/issue-body.md
   gh issue create --title "<issue title>" --body-file /tmp/issue-body.md --label "..."
   rm -f /tmp/issue-body.md
   ```

---

## 2. Episode Delivery Order

Follow one closed loop per issue (`A → B → A'`):

1. **Intake**: Read the issue (`gh issue view <n>`) and load only the `contexts/` docs and skills referenced in `Source Context` (for Rust edits, pair with `.agents/skills/rust-best-practices/SKILL.md` and `.agents/skills/rust-async-patterns/SKILL.md`).
2. **Isolate**: Work on a dedicated feature branch (`<prefix>/<short-kebab-topic>`) or sibling `git worktree` — never commit feature work directly to `master`.
3. **Implement & Fast Inner Loop**: Make tightly scoped changes, add offline unit tests, update `contexts/` if required, and iterate with targeted checks (§4).
4. **Independent Review ([`REVIEW.md`](REVIEW.md))**: Run the tier-appropriate review (`T1` structured single-agent, `T2` fresh-context adversarial, `T3` pre-code Design Gate + `T2`).
5. **Simplify Pass ([`SIMPLIFY.md`](SIMPLIFY.md))**: Run a behavior-preserving Occam's Razor pass to trim dead state, redundant checks, and wrapper bloat before publishing.
6. **Outer Pre-PR Gate**: Separate `ctx:` commits from code commits when practical, then run `npm run pre-pr` on the final `HEAD` (writes `.planar-agent/pre-pr-check-pass.json` bound to `git rev-parse HEAD`, unblocking the `PreToolUse` guard on `gh pr create`).
   > **HEAD staleness**: Even a 1-line commit after `npm run pre-pr` advances `HEAD` and causes the `PreToolUse` guard to block `gh pr create` until you re-run `npm run pre-pr`.
7. **Publish PR + Deterministic Telemetry**: Push the branch (`.githooks/pre-push` enforces `npm run verify`), open the PR using Planar's commit-prefix title grammar and [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md) (fill in **Why**, **What Changed**, **Review Tier**, **Invariant & Context Impact**, **Verification Proof**, and `Closes #N` / `Refs #N`; drop unused sections), and attach machine-generated workflow traces *deterministically after PR creation* rather than asking an LLM to transcribe Mermaid/tables.
8. **CI Green, Human Merge & Return to Theme**: Wait for all 3 GitHub CI jobs, merge upon human sign-off, remove temporary worktrees/branches, and distill durable lessons back into `contexts/`.

**Executable reference flow**: [`.cospan/flows/planar-dev/flow.mjs`](.cospan/flows/planar-dev/flow.mjs) automates Steps 1–7 with Progress-First resilience (halt early *only* on true P0 `blocked`; carry non-blocking residual review notes transparently to the PR body; attach full workflow traces post-PR via best-effort JS).

---

## 3. Parallelism & Conflict-Domain Partitioning

- **Worktree isolation for concurrent writers**: Never run two writable agents in one working directory. Give each concurrent issue a clean sibling worktree and install dev dependencies:
  ```bash
  git worktree add ../planar-<topic> -b <prefix>/<topic> master
  (cd ../planar-<topic> && npm ci --prefer-offline --no-audit --no-fund)
  ```
- **Hotspot ownership & sequencing**: Planar's highest-contention files are:
  - Core engine: `crates/planar-core/src/processor.rs`, `crates/planar-core/src/db.rs`, `crates/planar-core/src/model.rs`
  - Frontend & SSOT: `ui/app.js`, `ui/defaults.json`
  - Context map: `contexts/project/architecture.md`
  Partition parallel tasks across disjoint domains freely; when two changes touch the same hotspot lightly, expect a clean rebase after the first lands; when one task heavily refactors a hotspot (e.g., pipeline concurrency or schema restructure), sequence it *after* prerequisite PRs merge to `master`.
- **Parallel read-only fan-out**: Run independent read-only research, review, or verification lenses concurrently rather than chaining redundant serial passes.

---

## 4. Verification Target Assignment

Assign the cheapest check that proves the touched surface during iteration, then run the full outer gate once before publishing:

| Tier | Command | Scope & Cost |
| :--- | :--- | :--- |
| **Inner — UI & JS** | `npm run lint:ui && npm run test:ui` | Fast (`< 3s`): Biome lint, `node --check`, and offline UI contract tests (`scripts/test-ui-defaults.mjs`). |
| **Inner — Rust unit** | `cargo test -p planar-core <filter>` | Fast targeted test execution inside `planar-core` (offline, zero network/LLM calls). |
| **Inner — Hooks & Hygiene** | `npm run check:hygiene && npm run test:hooks` | Fast (`< 1s`): clean-room privacy, doc links, `CONTEXT_MAP.md`, version lockstep, and Python guard tests. |
| **Smoke — Visual / CLI** | `npm run preview` (`?mock=1`, `&sidebar=350`) / `PLANAR_DB_PATH=$(mktemp -d)/planar.db cargo run -p planar-cli -- status --no-scan` | Isolated surface verification without touching host transcripts or `~/.planar/planar.db`. |
| **Outer Gate (Pre-PR)** | `npm run pre-pr` | Full `npm run verify` (`lint` + `check:hygiene` + `cargo test --workspace` + `test:ui` + `test:hooks`) + `HEAD`-bound PR marker. |

---

## 5. Authorization Gates (Human Approval Required)

Agents may iterate, run offline tests, open draft/review PRs, and file issues autonomously when requested, but **must obtain explicit human approval** before:

1. Merging Pull Requests into `master` (mandatory for all `tier:T2` and `tier:T3` changes).
2. Starting implementation on a `tier:T3` architectural / cross-crate RFC (Design Gate first).
3. Creating release commits or local annotated tags (`vX.Y.Z`), pushing any version bump or release tag, or publishing GitHub Releases ([`version-control-and-release.md`](contexts/engineering/version-control-and-release.md)).
4. Force-pushing shared branches or installing/overwriting local binaries (`/Applications/Planar.app`, `~/.cargo/bin/planar`).
5. Modifying clean-room hygiene exclusions or bypassing `.githooks/` (`--no-verify` is forbidden).
