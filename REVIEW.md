# Review Policy (`REVIEW.md`)

> Planar uses **Risk-Based Tiered Review**: every Issue and Pull Request declares a Review Tier (`T0`–`T3`) based on the blast radius and invariants it touches, determining the exact review depth, automation gates, and approval mode required before merge.

---

## 1. Review Tier Matrix

> **Rule of Highest Tier**: If an Issue or PR touches multiple surfaces, the **highest applicable tier** governs the entire change.

| Tier | Typical Issues & Scope | Trigger Paths / Surfaces | Review Mode | Required Mechanical Gate | Human Approval |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **T0 — Trivial / Pure Context** | Typos, README updates, `.github/` templates, pure `contexts/` documentation edits, code comments | `README*.md`, `CONTRIBUTING.md`, `contexts/project/*` (docs only), `.github/ISSUE_TEMPLATE/*` | **Automated Hygiene + Author Self-Check** (confirm `CONTEXT_MAP.md` registration and zero link/privacy issues) | `npm run check:hygiene` | Optional for maintainers; quick skim for external PRs |
| **T1 — Standard Scoped Change** | Bounded bug fix, CLI flag/output tweak, new OpenAI-compatible API provider, `./ui` visual/copy refinement, test coverage expansion | `crates/planar-cli/src/main.rs`, `render.rs`, `pricing.rs`, `ui/*` (no API break), unit tests | **Single-Agent Structured Review** (fresh-context review applying `.agents/skills/rust-review/SKILL.md` for Rust or `contexts/engineering/ui-zero-fork.md` for UI) | `npm run pre-pr` (`lint` + `check:hygiene` + `cargo test` + `test:hooks`) | 1 maintainer approval |
| **T2 — Core Invariant / Schema / Security / Release** | SQLite schema/migration, new harness ingestor, burst math, Head–Middle–Tail slicer, `Sync`/`Regroup` clustering, MCP `2025-03-26` protocol, HTTP API routes, Tauri permissions/shell, Git/Agent hooks, versioned release | `db.rs`, `burst.rs`, `ingest/*`, `processor.rs`, `mcp.rs`, `server.rs`, `src-tauri/*`, `.githooks/*`, `scripts/hooks/*`, `.github/workflows/*`, `Cargo.toml` / `package.json` version bumps | **Deep Adversarial / Multi-Perspective Review** (fresh-context or cross-model review explicitly auditing core invariants, migration backward-compatibility, read-only safety, and zero P0/P1 `rust-review` findings) | `npm run pre-pr` + targeted regression tests + isolated CLI/UI smoke check | Mandatory maintainer deep review & explicit approval |
| **T3 — Architectural / Multi-Crate RFC** | New delivery surface (e.g., IDE sidebar sidecar), automatic LLM dependency Link Pass, background `auto_sync` daemon scheduler, MCP write tools, storage/UI architecture shift | Cross-crate / new crate / new runtime daemon / breaking schema or protocol change | **Two-Gate RFC / Council Review**:<br>1. **Design Gate (Pre-Code)**: Approved RFC issue / `contexts/` design update + multi-perspective critique before implementation.<br>2. **Code Gate (Pre-Merge)**: Full **T2** Adversarial Review + real local dogfooding verification. | Design Gate approval + full **T2** mechanical gate + updated `contexts/` | Mandatory maintainer sign-off at **both** Design Gate and Merge Gate |

---

## 2. Review Execution Rules (For Humans & Coding Agents)

1. **No Self-Approval on T2/T3**: The coding agent (or author) that implemented a **T2** or **T3** change runs `npm run pre-pr` and self-checks, but the required review pass MUST happen in a **fresh context** (independent review subagent, cross-model reviewer, or human maintainer) to eliminate authoring bias.
2. **Core Invariants Checklist (Mandatory for T2/T3)**:
   - **Read-Only Harness Safety**: Zero writes, locks, or mutations to `~/.claude`, `~/.codex`, `~/.gemini/antigravity`, or `~/.dsh`.
   - **Slicing & Anti-Self-Ingestion**: Tool payloads stripped unconditionally; `[PLANAR_INTERNAL_DIGEST]` filtered everywhere; pruned source logs keep digests locked.
   - **Taxonomy Stability**: `Sync` never overwrites `pinned = 1` Lines or renames/merges existing Lines; archived sessions remain in historical metrics.
   - **Burst Associativity**: Unpadded spans persisted in `activity_intervals`; `MIN_BURST_MS` applied only at read/projection time.
   - **Platform Contracts**: `"dragDropEnabled": false` preserved in `tauri.conf.json`; SQLite WAL PRAGMAs preserved; single `hydrate_macos_path()` call; zero `stdout` logs in `planar mcp`.
   - **Clean-Room Open Source**: Zero personal paths, private workspace/project names, unreleased internal model codenames, real transcripts, or secrets in diffs, tests, or screenshots.
3. **Structured Review Findings Format**:
   Every review comment or agent review report must prioritize findings clearly:
   - **`P0 — Blocker`**: Correctness bug, data loss, security/privacy leak, or core invariant violation (blocks merge).
   - **`P1 — Should Fix`**: Idiom/maintainability defect, missing edge-case test, or contract/context drift (fix or explicitly defer to a linked issue).
   - **`P2 — Suggestion`**: Minor clarity or polish improvement (non-blocking).
   - **`Pre-existing`**: Defect that already existed outside the PR's changes (non-blocking; note briefly for follow-up instead of blocking merge).
   Each finding cites exact `file:line`, the violated rule/invariant, and a concrete fix.
4. **Context Coherence Check**:
   If a PR changes CLI flags, HTTP routes, SQLite tables, MCP schemas, or engineering workflows, the reviewer verifies that the corresponding file under `contexts/` (and `skills/planar/SKILL.md` if user-facing) was updated in the same PR.

---

## 3. Scope, Signal & Convergence Discipline

Keep automated and human reviews high-signal and convergent:

1. **Judge strictly within the PR's scope**: Review what this change introduces against the issue objective and Core Invariants above. Do not block a PR on untouched code, unrelated refactors, or `Pre-existing` issues — report those as non-blocking notes.
2. **Focus on realistic, concrete failures**: Request `revise` only for genuine `P0`/`P1` defects (broken behavior, invariant violations, real data loss, missing coverage for changed logic). Avoid blocking on low-probability hypotheticals (e.g., speculative future config changes or rare multi-process races on single-user local startup paths) unless the PR directly introduces concurrency or migration logic.
3. **Converge on re-review rounds**: On Round 1, flag the full *class of bug* so all instances get fixed together. On follow-up rounds, verify that prior blockers were resolved and no new `P0`/`P1` regression was introduced — do not peel deeper layers of pre-existing or secondary edge cases across rounds.
4. **Right-size verification during iteration**: Intermediate fix rounds only need `npm run verify` and targeted unit tests; full `npm run pre-pr` and live CLI/browser smoke checks run once before opening the PR.
