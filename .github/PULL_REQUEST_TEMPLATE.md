<!-- PR title must follow Planar commit prefix grammar: core:, cli:, app:, ui:, ctx:, skill:, feat:, fix:, chore:, release: -->
<!-- Delete optional sections that do not apply. -->

## Why

<!-- What problem, bug, or workflow friction does this PR solve? Focus on motivation (the diff shows what changed). -->

## What Changed

<!-- Group key design decisions and trade-offs logically. Avoid listing every modified file. -->

- ...

## Review Tier (`REVIEW.md`)

<!-- Check the HIGHEST applicable tier from REVIEW.md -->

- [ ] **T0** — Trivial / Pure Context / Docs (`npm run check:hygiene`)
- [ ] **T1** — Standard Scoped Change (Single-Agent Structured Review + `npm run pre-pr`)
- [ ] **T2** — Core Invariant / Schema / Ingestor / MCP / Tauri / Hooks / Release (Adversarial / Fresh-Context Review + Human Approval)
- [ ] **T3** — Architectural / Multi-Crate RFC (Design Gate approved + full T2 gate)

## Invariant & Context Check

- [ ] Read-only on harness logs preserved (zero writes/locks under `~/.claude`, `~/.codex`, `~/.gemini/antigravity`, `~/.dsh`)
- [ ] Clean-room open source verified (zero personal paths, private workspace names, internal model codenames, real transcripts, or secrets)
- [ ] Context impact handled:
  - [ ] Updated affected `contexts/` docs (and registered any new file in `contexts/CONTEXT_MAP.md`), OR
  - [ ] Explicitly `No context impact`

## Verification Proof

<!-- Paste exact commands executed and concise pass results -->

- [ ] `npm run pre-pr` (`lint` + `check:hygiene` + `cargo test --workspace` + `test:hooks`)
- Additional smoke tests (CLI temp DB / `?mock=1` UI / narrow sidebar `?sidebar=350`):

## Linked Issues

<!-- Closes #N | Refs #N -->
