# Reference: Contexts-First Issue Filing

> Follow this SOP whenever creating a GitHub issue (`gh issue create` or drafting an issue for a human). Every issue must work as a standalone **clean-session handoff** so a fresh coding agent can pick it up without chat history.

## Step 1 — Choose the Right Template

Templates live in `.github/ISSUE_TEMPLATE/`:

| Template | Labels | When to use |
| :--- | :--- | :--- |
| `bug.yml` | `bug` | Unexpected behavior, crash, wrong active time, parser bug, UI regression |
| `feature.yml` | `enhancement` | New user-facing capability or workflow improvement |
| `harness-adapter.yml` | `enhancement`, `harness` | Support for a new coding-agent harness or LLM API provider |
| `context-update.yml` | `documentation`, `context` | Updating `contexts/` or evolving agent workflows/skills |

## Step 2 — Populate Contexts-First Handoff Fields

Before invoking `gh issue create`, ensure the issue body explicitly contains:

1. **Source Context**: Exact `contexts/` files, code paths, or related issues/PRs to read first.
2. **Objective & Scope Boundaries**: What "done" looks like AND what is explicitly out of scope.
3. **Suggested Review Tier (`T0` / `T1` / `T2` / `T3`)**: Assigned per `REVIEW.md`.
4. **Context Action**: `Update existing context` | `Create new context doc` | `No context impact` + target path.
5. **Verification Plan**: Exact commands (`npm run verify`, targeted unit tests, UI `?mock=1` check, CLI smoke test).

## Step 3 — Record Issue Check Marker (Required by `PreToolUse` Guard)

When filing via CLI (`gh issue create`), the `PreToolUse` hook (`scripts/hooks/pre_tool_use_issue_guard.py`) verifies that the draft has been checked and recorded for the exact `(title, body)` hash:

```bash
python3 scripts/hooks/pre_tool_use_issue_guard.py --write-marker \
  --title "<issue title>" \
  --body-file /tmp/issue-body.md
```

Then run `gh issue create --title "<issue title>" --body-file /tmp/issue-body.md --label "..."` and remove `/tmp/issue-body.md`.

If GitHub CLI / MCP is unavailable or the user asked for a draft only, present the structured Markdown issue draft in chat instead.
