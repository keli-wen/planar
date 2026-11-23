#!/usr/bin/env python3
"""PreToolUse guard that requires a contexts-first issue draft marker before
allowing `gh issue create`.

Also provides `--write-marker --title <t> (--body <b> | --body-file <path>)`
to record the verified `(title, body)` SHA-256 digest under
`.planar-agent/issue-file-check-pass.json`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any

WORKFLOW_ID = "planar-dev/issue-filing"
DEFAULT_MARKER_REL = Path(".planar-agent") / "issue-file-check-pass.json"
MARKER_ENV_VAR = "PLANAR_ISSUE_MARKER"
SEGMENT_SPLIT_RE = re.compile(r"\s*(?:&&|\|\||;|\||\n)\s*")
MIN_BODY_CHARS = 20


def resolve_repo_root(cwd: Path) -> Path:
    try:
        proc = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=str(cwd),
            check=True,
            capture_output=True,
            text=True,
        )
        out = proc.stdout.strip()
        if out:
            return Path(out)
    except (OSError, subprocess.CalledProcessError):
        pass
    return cwd


def marker_path_for(repo_root: Path, explicit: str | None = None) -> Path:
    if explicit:
        return Path(explicit).expanduser().resolve()
    env_override = os.environ.get(MARKER_ENV_VAR)
    if env_override:
        return Path(env_override).expanduser().resolve()
    return repo_root / DEFAULT_MARKER_REL


def compute_draft_hash(title: str, body: str) -> str:
    canonical = json.dumps(
        {
            "title": title.replace("\r\n", "\n").strip(),
            "body": body.replace("\r\n", "\n").strip(),
        },
        sort_keys=True,
        ensure_ascii=False,
    )
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def write_marker(
    cwd: Path,
    title: str,
    body: str,
    explicit_marker: str | None = None,
) -> Path:
    clean_title = title.strip()
    clean_body = body.strip()
    if not clean_title:
        raise ValueError("Issue title cannot be empty.")
    if len(clean_body) < MIN_BODY_CHARS:
        raise ValueError(
            f"Issue body is too short ({len(clean_body)} chars; need >= {MIN_BODY_CHARS}). "
            "Include Source Context, Objective, Review Tier, and Verification Plan."
        )

    repo_root = resolve_repo_root(cwd)
    path = marker_path_for(repo_root, explicit_marker)
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schemaVersion": 1,
        "workflow": WORKFLOW_ID,
        "title": clean_title,
        "draftHash": compute_draft_hash(clean_title, clean_body),
        "createdAt": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
    }
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return path


def extract_gh_issue_create_tokens(command: str) -> list[str] | None:
    """Return tokenized `gh issue create ...` flags if present in command."""
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

        idx = 0
        while idx < len(tokens) and "=" in tokens[idx] and not tokens[idx].startswith("-"):
            idx += 1
        if idx >= len(tokens):
            continue

        if Path(tokens[idx]).name != "gh":
            continue

        rest = tokens[idx + 1 :]
        if len(rest) >= 2 and rest[0] == "issue" and rest[1] == "create":
            flags = rest[2:]
            if any(flag in ("--help", "-h") for flag in flags):
                return None
            return flags
    return None


def parse_issue_fields(flags: list[str], cwd: Path) -> tuple[str | None, str | None, str | None]:
    """Return (title, body, error_message) parsed from `gh issue create` flags."""
    title: str | None = None
    body: str | None = None
    body_file: str | None = None

    i = 0
    while i < len(flags):
        token = flags[i]
        if token in ("--title", "-t") and i + 1 < len(flags):
            title = flags[i + 1]
            i += 2
            continue
        if token.startswith("--title="):
            title = token.split("=", 1)[1]
            i += 1
            continue
        if token in ("--body", "-b") and i + 1 < len(flags):
            body = flags[i + 1]
            i += 2
            continue
        if token.startswith("--body="):
            body = token.split("=", 1)[1]
            i += 1
            continue
        if token in ("--body-file", "-F") and i + 1 < len(flags):
            body_file = flags[i + 1]
            i += 2
            continue
        if token.startswith("--body-file="):
            body_file = token.split("=", 1)[1]
            i += 1
            continue
        i += 1

    if body_file is not None:
        if body_file == "-":
            return title, None, "Reading issue body from stdin (`--body-file -`) cannot be verified."
        path = Path(body_file)
        if not path.is_absolute():
            path = cwd / path
        if not path.is_file():
            return title, None, f"Issue body file does not exist: {path}"
        body = path.read_text(encoding="utf-8")

    return title, body, None


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

    flags = extract_gh_issue_create_tokens(command)
    if flags is None:
        return None

    cwd = Path(payload.get("cwd") or os.getcwd())
    title, body, parse_err = parse_issue_fields(flags, cwd)
    if parse_err:
        return deny(
            f"Blocked `gh issue create`: {parse_err} "
            "Follow `.agents/skills/planar-dev/references/issue-filing.md` and pass "
            "`--title` with `--body` or `--body-file <path>`."
        )
    if not title or body is None:
        return deny(
            "Blocked `gh issue create`: missing explicit `--title` and `--body`/`--body-file`. "
            "Follow `.agents/skills/planar-dev/references/issue-filing.md` and record the draft "
            "marker with `python3 scripts/hooks/pre_tool_use_issue_guard.py --write-marker` first."
        )

    repo_root = resolve_repo_root(cwd)
    marker_path = marker_path_for(repo_root)
    if not marker_path.is_file():
        return deny(
            f"Blocked `gh issue create`: missing issue-filing marker at {marker_path}. "
            "Follow `.agents/skills/planar-dev/references/issue-filing.md` and run "
            "`python3 scripts/hooks/pre_tool_use_issue_guard.py --write-marker --title ... --body-file ...`."
        )

    try:
        data = json.loads(marker_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return deny(
            f"Blocked `gh issue create`: unreadable issue marker ({exc}). "
            "Re-run `pre_tool_use_issue_guard.py --write-marker`."
        )

    if (
        not isinstance(data, dict)
        or data.get("schemaVersion") != 1
        or data.get("workflow") != WORKFLOW_ID
    ):
        return deny(
            "Blocked `gh issue create`: invalid issue marker schema. "
            "Re-run `pre_tool_use_issue_guard.py --write-marker`."
        )

    expected_hash = compute_draft_hash(title, body)
    if data.get("draftHash") != expected_hash:
        return deny(
            "Blocked `gh issue create`: draft title/body changed since marker was written. "
            "Re-run `python3 scripts/hooks/pre_tool_use_issue_guard.py --write-marker` for the final draft."
        )

    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Planar PreToolUse guard for `gh issue create`.")
    parser.add_argument(
        "--write-marker",
        action="store_true",
        help="Validate and record a contexts-first issue draft marker.",
    )
    parser.add_argument("--title", default=None, help="Issue title (with --write-marker).")
    parser.add_argument("--body", default=None, help="Inline issue body (with --write-marker).")
    parser.add_argument(
        "--body-file",
        default=None,
        help="Path to file containing issue body (with --write-marker).",
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
        if not args.title:
            parser.error("--title is required with --write-marker")
        if args.body is None and args.body_file is None:
            parser.error("either --body or --body-file is required with --write-marker")
        body_text = (
            Path(args.body_file).read_text(encoding="utf-8")
            if args.body_file is not None
            else (args.body or "")
        )
        path = write_marker(cwd, args.title, body_text, args.marker)
        print(f"Wrote Planar issue-filing marker: {path}")
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
