#!/usr/bin/env python3
"""PreToolUse guard that requires a fresh HEAD-bound pre-PR verification marker
before allowing `gh pr create`.

Also provides `--write-marker` (called by `npm run pre-pr`) to record the
verified HEAD SHA under `.planar-agent/pre-pr-check-pass.json`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any

WORKFLOW_ID = "planar-dev/pre-pr"
DEFAULT_MARKER_REL = Path(".planar-agent") / "pre-pr-check-pass.json"
MARKER_ENV_VAR = "PLANAR_PRE_PR_MARKER"
SEGMENT_SPLIT_RE = re.compile(r"\s*(?:&&|\|\||;|\||\n)\s*")


def run_git(args: list[str], cwd: Path) -> str | None:
    try:
        proc = subprocess.run(
            ["git", *args],
            cwd=str(cwd),
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return proc.stdout.strip()


def resolve_repo_root(cwd: Path) -> Path:
    top = run_git(["rev-parse", "--show-toplevel"], cwd)
    return Path(top) if top else cwd


def marker_path_for(repo_root: Path, explicit: str | None = None) -> Path:
    if explicit:
        return Path(explicit).expanduser().resolve()
    env_override = os.environ.get(MARKER_ENV_VAR)
    if env_override:
        return Path(env_override).expanduser().resolve()
    return repo_root / DEFAULT_MARKER_REL


def write_marker(cwd: Path, explicit_marker: str | None = None) -> Path:
    repo_root = resolve_repo_root(cwd)
    head = run_git(["rev-parse", "HEAD"], repo_root)
    if not head:
        raise RuntimeError(f"Could not resolve git HEAD in {repo_root}")

    path = marker_path_for(repo_root, explicit_marker)
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schemaVersion": 1,
        "workflow": WORKFLOW_ID,
        "head": head,
        "createdAt": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
    }
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return path


def extract_gh_pr_create_tokens(command: str) -> list[str] | None:
    """Return tokenized `gh pr create ...` invocation if present in command."""
    for segment in SEGMENT_SPLIT_RE.split(command):
        segment = segment.strip()
        if not segment or segment.startswith("#"):
            continue
        try:
            tokens = shlex.split(segment)
        except ValueError:
            tokens = segment.split()
        if not tokens:
            continue

        # Skip leading environment assignments (e.g. GH_TOKEN=... gh pr create)
        idx = 0
        while idx < len(tokens) and "=" in tokens[idx] and not tokens[idx].startswith("-"):
            idx += 1
        if idx >= len(tokens):
            continue

        cmd_name = Path(tokens[idx]).name
        if cmd_name != "gh":
            continue

        rest = tokens[idx + 1 :]
        if len(rest) >= 2 and rest[0] == "pr" and rest[1] == "create":
            flags = rest[2:]
            if any(flag in ("--help", "-h") for flag in flags):
                return None
            return flags
    return None


def extract_head_ref(flags: list[str]) -> str | None:
    i = 0
    while i < len(flags):
        token = flags[i]
        if token in ("--head", "-H") and i + 1 < len(flags):
            return flags[i + 1]
        if token.startswith("--head="):
            return token.split("=", 1)[1]
        i += 1
    return None


def resolve_target_root_and_head(cwd: Path, head_ref: str | None) -> tuple[Path, str | None]:
    repo_root = resolve_repo_root(cwd)
    if head_ref:
        # Handle `owner:branch` syntax from `gh pr create --head`
        branch_name = head_ref.split(":", 1)[-1]
        resolved = run_git(["rev-parse", "--verify", branch_name], repo_root)
        if resolved:
            # Check if a linked worktree currently checks out that branch
            worktree_out = run_git(["worktree", "list", "--porcelain"], repo_root)
            if worktree_out:
                current_wt: Path | None = None
                for line in worktree_out.splitlines():
                    if line.startswith("worktree "):
                        current_wt = Path(line.split(" ", 1)[1].strip())
                    elif line.startswith("branch ") and current_wt is not None:
                        ref = line.split(" ", 1)[1].strip()
                        if ref == f"refs/heads/{branch_name}":
                            return current_wt, resolved
            return repo_root, resolved

    return repo_root, run_git(["rev-parse", "HEAD"], repo_root)


def deny(reason: str) -> dict[str, Any]:
    return {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    }


def evaluate_hook_payload(payload: dict[str, Any]) -> dict[str, Any] | None:
    tool_name = payload.get("tool_name") or payload.get("toolName") or ""
    if tool_name and tool_name not in ("Bash", "bash", "shell", "Shell"):
        return None

    tool_input = payload.get("tool_input") or payload.get("toolInput") or {}
    if not isinstance(tool_input, dict):
        return None
    command = tool_input.get("command") or tool_input.get("cmd") or ""
    if not isinstance(command, str) or "gh" not in command:
        return None

    flags = extract_gh_pr_create_tokens(command)
    if flags is None:
        return None

    cwd_str = payload.get("cwd") or os.getcwd()
    cwd = Path(cwd_str)
    head_ref = extract_head_ref(flags)
    target_root, expected_head = resolve_target_root_and_head(cwd, head_ref)
    if not expected_head:
        return deny(
            "Blocked `gh pr create`: could not determine git HEAD. "
            "Run `npm run pre-pr` inside the Planar git repository first."
        )

    marker_path = marker_path_for(target_root)
    if not marker_path.is_file():
        return deny(
            f"Blocked `gh pr create`: missing pre-PR verification marker at {marker_path}. "
            "Follow `.agents/skills/planar-dev/references/verify-and-pre-pr.md` and run "
            "`npm run pre-pr` before creating the PR."
        )

    try:
        data = json.loads(marker_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return deny(
            f"Blocked `gh pr create`: unreadable pre-PR marker ({exc}). "
            "Re-run `npm run pre-pr` to regenerate the marker."
        )

    if (
        not isinstance(data, dict)
        or data.get("schemaVersion") != 1
        or data.get("workflow") != WORKFLOW_ID
    ):
        return deny(
            "Blocked `gh pr create`: invalid pre-PR marker schema. "
            "Re-run `npm run pre-pr` before creating the PR."
        )

    recorded_head = data.get("head")
    if recorded_head != expected_head:
        return deny(
            f"Blocked `gh pr create`: pre-PR marker is stale "
            f"(recorded {str(recorded_head)[:12]}, current HEAD {expected_head[:12]}). "
            "Re-run `npm run pre-pr` after your latest commit."
        )

    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Planar PreToolUse guard for `gh pr create`.")
    parser.add_argument(
        "--write-marker",
        action="store_true",
        help="Write a fresh HEAD-bound pre-PR verification marker and exit.",
    )
    parser.add_argument(
        "--cwd",
        default=None,
        help="Working directory used with --write-marker (defaults to current directory).",
    )
    parser.add_argument(
        "--marker",
        default=None,
        help="Optional explicit marker path override.",
    )
    args = parser.parse_args(argv)

    if args.write_marker:
        cwd = Path(args.cwd) if args.cwd else Path.cwd()
        path = write_marker(cwd, args.marker)
        print(f"Wrote Planar pre-PR marker: {path}")
        return 0

    raw = sys.stdin.read().strip()
    if not raw:
        return 0
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError:
        return 0
    if not isinstance(payload, dict):
        return 0

    decision = evaluate_hook_payload(payload)
    if decision is not None:
        print(json.dumps(decision))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
