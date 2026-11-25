Implement the assigned GitHub issue or task on a dedicated feature branch (if currently on `master`, create a new feature branch from current `HEAD`; if already on a dedicated feature branch such as a worktree branch, stay on it).

- Load context progressively: run `gh issue view <n>` if an issue number is referenced, then follow `AGENTS.md`, `contexts/CONTEXT_MAP.md`, and `.agents/skills/planar-dev/SKILL.md`.
- Keep edits tightly scoped to the issue and preserve every Planar core invariant (`AGENTS.md` #1–#7).
- Add or update targeted offline unit tests, and update `contexts/` when required by the issue spec.
- Run `npm run verify` and confirm every check passes with zero warnings.
- Do NOT commit, push, or open a PR yet — leave verified changes in the working tree for review and simplification.
