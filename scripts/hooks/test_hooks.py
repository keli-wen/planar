"""Unit tests for Planar PreToolUse PR and issue guards."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HOOKS_DIR = Path(__file__).resolve().parent
if str(HOOKS_DIR) not in sys.path:
    sys.path.insert(0, str(HOOKS_DIR))

import pre_tool_use_issue_guard as issue_guard  # noqa: E402
import pre_tool_use_pr_guard as pr_guard  # noqa: E402


GIT_ENV_KEYS = (
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
)


def clean_git_env() -> dict[str, str]:
    return {k: v for k, v in os.environ.items() if k not in GIT_ENV_KEYS}


def init_temp_git_repo(root: Path) -> None:
    env = clean_git_env()
    subprocess.run(["git", "init", "-q"], cwd=str(root), env=env, check=True)
    subprocess.run(["git", "config", "user.name", "Planar Test"], cwd=str(root), env=env, check=True)
    subprocess.run(
        ["git", "config", "user.email", "test@planar.local"],
        cwd=str(root),
        env=env,
        check=True,
    )
    (root / "README.md").write_text("# temp\n", encoding="utf-8")
    subprocess.run(["git", "add", "README.md"], cwd=str(root), env=env, check=True)
    subprocess.run(
        ["git", "commit", "-q", "-m", "chore: init"],
        cwd=str(root),
        env=env,
        check=True,
    )


class PrGuardTests(unittest.TestCase):
    def setUp(self) -> None:
        self.prev_git_env = {k: os.environ.pop(k) for k in GIT_ENV_KEYS if k in os.environ}
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        init_temp_git_repo(self.repo)
        self.prev_env = os.environ.pop(pr_guard.MARKER_ENV_VAR, None)

    def tearDown(self) -> None:
        if self.prev_env is not None:
            os.environ[pr_guard.MARKER_ENV_VAR] = self.prev_env
        else:
            os.environ.pop(pr_guard.MARKER_ENV_VAR, None)
        for k, v in self.prev_git_env.items():
            os.environ[k] = v
        self.tmp.cleanup()

    def test_ignores_unrelated_bash_commands(self) -> None:
        payload = {
            "tool_name": "Bash",
            "cwd": str(self.repo),
            "tool_input": {"command": "echo 'gh pr create' && git status"},
        }
        self.assertIsNone(pr_guard.evaluate_hook_payload(payload))

    def test_ignores_gh_pr_create_help(self) -> None:
        payload = {
            "tool_name": "Bash",
            "cwd": str(self.repo),
            "tool_input": {"command": "gh pr create --help"},
        }
        self.assertIsNone(pr_guard.evaluate_hook_payload(payload))

    def test_denies_when_marker_missing(self) -> None:
        payload = {
            "tool_name": "Bash",
            "cwd": str(self.repo),
            "tool_input": {"command": "gh pr create --title 'core: test' --body 'ok'"},
        }
        decision = pr_guard.evaluate_hook_payload(payload)
        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertEqual(decision["hookSpecificOutput"]["permissionDecision"], "deny")

    def test_allows_when_marker_matches_head_and_denies_after_new_commit(self) -> None:
        pr_guard.write_marker(self.repo)
        payload = {
            "tool_name": "Bash",
            "cwd": str(self.repo),
            "tool_input": {"command": "gh pr create --title 'core: fix burst' --body 'verified'"},
        }
        self.assertIsNone(pr_guard.evaluate_hook_payload(payload))

        # Make a new commit so HEAD moves ahead of the marker
        env = clean_git_env()
        (self.repo / "note.txt").write_text("change\n", encoding="utf-8")
        subprocess.run(["git", "add", "note.txt"], cwd=str(self.repo), env=env, check=True)
        subprocess.run(
            ["git", "commit", "-q", "-m", "core: tweak"],
            cwd=str(self.repo),
            env=env,
            check=True,
        )

        decision = pr_guard.evaluate_hook_payload(payload)
        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertEqual(decision["hookSpecificOutput"]["permissionDecision"], "deny")
        self.assertIn("stale", decision["hookSpecificOutput"]["permissionDecisionReason"])

    def test_init_temp_git_repo_ignores_inherited_git_dir(self) -> None:
        parent_head = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=str(self.repo),
            env=clean_git_env(),
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        with tempfile.TemporaryDirectory() as child_tmp:
            os.environ["GIT_DIR"] = str(self.repo / ".git")
            try:
                init_temp_git_repo(Path(child_tmp))
            finally:
                os.environ.pop("GIT_DIR", None)
        after_head = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=str(self.repo),
            env=clean_git_env(),
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        self.assertEqual(parent_head, after_head)


class IssueGuardTests(unittest.TestCase):
    def setUp(self) -> None:
        self.prev_git_env = {k: os.environ.pop(k) for k in GIT_ENV_KEYS if k in os.environ}
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        init_temp_git_repo(self.repo)
        self.prev_env = os.environ.pop(issue_guard.MARKER_ENV_VAR, None)

    def tearDown(self) -> None:
        if self.prev_env is not None:
            os.environ[issue_guard.MARKER_ENV_VAR] = self.prev_env
        else:
            os.environ.pop(issue_guard.MARKER_ENV_VAR, None)
        for k, v in self.prev_git_env.items():
            os.environ[k] = v
        self.tmp.cleanup()

    def test_ignores_non_issue_commands(self) -> None:
        payload = {
            "tool_name": "Bash",
            "cwd": str(self.repo),
            "tool_input": {"command": "gh issue list --limit 5"},
        }
        self.assertIsNone(issue_guard.evaluate_hook_payload(payload))

    def test_denies_without_marker(self) -> None:
        payload = {
            "tool_name": "Bash",
            "cwd": str(self.repo),
            "tool_input": {
                "command": "gh issue create --title 'bug: burst cap' --body 'Source Context: contexts/project/design.md'"
            },
        }
        decision = issue_guard.evaluate_hook_payload(payload)
        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertEqual(decision["hookSpecificOutput"]["permissionDecision"], "deny")

    def test_allows_matching_body_file_and_denies_mutated_draft(self) -> None:
        body_path = self.repo / "draft.md"
        title = "core: preserve tool spans across idle gaps"
        body = (
            "Source Context: contexts/project/design.md\n"
            "Review Tier: T2\n"
            "Verification: cargo test --workspace\n"
        )
        body_path.write_text(body, encoding="utf-8")

        issue_guard.write_marker(self.repo, title, body)
        payload = {
            "tool_name": "Bash",
            "cwd": str(self.repo),
            "tool_input": {
                "command": f"gh issue create --title '{title}' --body-file draft.md --label bug"
            },
        }
        self.assertIsNone(issue_guard.evaluate_hook_payload(payload))

        # Mutate the body file without updating the marker
        body_path.write_text(body + "\nextra edit\n", encoding="utf-8")
        decision = issue_guard.evaluate_hook_payload(payload)
        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertEqual(decision["hookSpecificOutput"]["permissionDecision"], "deny")
        self.assertIn("changed", decision["hookSpecificOutput"]["permissionDecisionReason"])

    def test_rejects_short_body_in_write_marker(self) -> None:
        with self.assertRaises(ValueError):
            issue_guard.write_marker(self.repo, "bug: short", "too short")


if __name__ == "__main__":
    unittest.main()
