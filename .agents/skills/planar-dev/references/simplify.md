# Reference: Diff Simplifier (Occam's Razor Pass)

> Read-only post-review pass for removing unnecessary complexity before opening a PR. Goal: keep the smallest, clearest diff that satisfies the issue spec while preserving all tests and Planar invariants.

## North Star

- **Occam's Razor**: Prefer fewer abstractions, fewer branches, fewer knobs, and fewer lines when behavior and clarity are identical.
- **No Busywork**: If the diff is already lean and idiomatic, return `verdict: "pass"` immediately. Never invent cosmetic rewrites or style churn.
- **Strictly Behavior-Preserving**: Simplifications must not weaken error handling, drop edge-case coverage, or change external contracts.

## What to Trim

1. **Speculative Generality & Dead Code**:
   - Single-use traits, wrappers, helper structs, or indirection layers that obscure rather than clarify.
   - Unused fields, unreachable fallback branches, or config flags nobody requested.
2. **Duplicated Constants & Logic**:
   - Repeated magic numbers, model IDs, clamp ranges, or provider specs that should live in one SSOT.
   - Copy-pasted branches that collapse cleanly without becoming cryptic.
3. **Over-Complicated Control Flow**:
   - Deeply nested `if` / `match` blocks replaceable by early returns or standard Rust / JS combinators.
   - Redundant `.clone()`, unnecessary intermediate allocations, or comments that merely restate the code.
4. **Scope Creep**:
   - Unrelated edits or formatting churn outside the issue's stated scope.

## Hard Do-Not-Touch Guardrails

Never recommend removing or weakening:

- **Core Invariants (`AGENTS.md` #1–#7)**: read-only harness logs, `[PLANAR_INTERNAL_DIGEST]` stripping, pinned Line immutability, unpadded `activity_intervals`, SQLite WAL/FK PRAGMAs, `"dragDropEnabled": false`, clean-room hygiene.
- **Rust Safety & Error Context**: `unsafe_code = "forbid"`, `unwrap_used = "deny"`, meaningful `PlanarError` messages, and regression tests guarding real failure modes.
- **Zero-Fork `./ui` Parity**: `DESIGN.md` Light/Dark tokens, `zh`/`en` `I18N` parity, narrow `320px` sidebar, and `?mock=1` preview compatibility.

## Output Rules

- Always inspect every file listed in `Touched Files` directly (especially newly created files or when a git diff is truncated).
- Return at most **5 concrete, high-value simplifications** with exact `file:line`, what to delete/collapse, and why.
- Skip pure formatting/naming nits unless a name is actively misleading.
