"""Hermetic API-sync base tests with synthetic GitHub responses and local Git repos."""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "api_sync_base", Path(__file__).with_name("check-api-sync-base.py")
)
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)
SHA = "a" * 40
NEW_SHA = "b" * 40


class BaseUnitTests(unittest.TestCase):
    def setUp(self):
        self.commands = []
        self.remote_sha = SHA
        self.api_code = 0
        self.git_code = 0

    def runner(self, command, repository):
        self.commands.append(command)
        if command[0] == "gh":
            return subprocess.CompletedProcess(command, self.api_code, self.remote_sha + "\n", "")
        return subprocess.CompletedProcess(command, self.git_code, "", "")

    def check(self, base=SHA, branch="main", repository="example/project"):
        BASE.check_base(Path.cwd(), base, branch, repository, self.runner)

    def test_current_base_checks_ancestry_and_workflows_without_mutations(self):
        self.check()
        self.assertEqual(self.commands, [
            ["git", "check-ref-format", "refs/heads/main"],
            ["gh", "api", "repos/example/project/git/ref/heads/main", "--jq", ".object.sha"],
            ["git", "merge-base", "--is-ancestor", SHA, "HEAD"],
            ["git", "diff", "--exit-code", SHA, "--", ".github/workflows"],
        ])

    def test_stale_base_stops_before_any_publish_or_local_git_check(self):
        self.remote_sha = NEW_SHA
        with self.assertRaisesRegex(RuntimeError, "start a fresh workflow run"):
            self.check()
        self.assertEqual(len(self.commands), 2)

    def test_unverifiable_remote_fails_closed(self):
        for value, code in [("", 0), ("not-a-sha", 0), (SHA, 1), (SHA + "\n" + NEW_SHA, 0)]:
            with self.subTest(value=value, code=code):
                self.remote_sha, self.api_code = value, code
                with self.assertRaisesRegex(RuntimeError, "Could not verify"):
                    self.check()

    def test_invalid_base_and_repository_fail_before_any_command(self):
        for value in ["", "main", SHA[:7], SHA + "\n", "-" + SHA]:
            with self.subTest(base=value), self.assertRaisesRegex(RuntimeError, "recorded generation base"):
                self.check(base=value)
        for value in ["", "example", "../project", "example/project/extra", "example/project\n"]:
            with self.subTest(repository=value), self.assertRaisesRegex(RuntimeError, "GITHUB_REPOSITORY"):
                self.check(repository=value)
        self.assertEqual(self.commands, [])

    def test_invalid_ref_is_rejected_before_github(self):
        self.git_code = 1
        with self.assertRaisesRegex(RuntimeError, "invalid default branch"):
            self.check(branch="main..invalid")
        self.assertEqual(len(self.commands), 1)

    def test_branch_names_are_url_encoded(self):
        self.check(branch="release/current")
        self.assertEqual(self.commands[1][2], "repos/example/project/git/ref/heads/release%2Fcurrent")

    def test_timeout_is_not_treated_as_current(self):
        with patch.object(BASE, "run", side_effect=subprocess.TimeoutExpired("gh", 60)), \
             patch.object(sys, "argv", ["check-api-sync-base.py", "--base-sha", SHA, "--default-branch", "main"]), \
             patch.dict(os.environ, {"GITHUB_REPOSITORY": "example/project"}), \
             patch("sys.stderr") as stderr:
            self.assertEqual(BASE.main(), 1)
        self.assertIn("::error::", str(stderr.write.call_args_list))


class LocalGitIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="api-sync-tests-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Test Author")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.hooksPath", str(self.root / "no-hooks"))
        workflow = self.root / ".github" / "workflows" / "ci.yml"
        workflow.parent.mkdir(parents=True)
        workflow.write_text("name: original\n", encoding="utf-8")
        self.commit()
        self.base = self.git("rev-parse", "HEAD")
        self.remote = self.base
        self.commands = []

    def git(self, *args):
        result = BASE.run(["git", *args], self.root)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def commit(self):
        self.git("add", "--all")
        self.git("commit", "--message", "test fixture")

    def runner(self, command, repository):
        self.commands.append(command)
        if command[0] == "gh":
            return subprocess.CompletedProcess(command, 0, self.remote + "\n", "")
        return BASE.run(command, repository)

    def check(self):
        BASE.check_base(self.root, self.base, "main", "example/project", self.runner)

    def test_generated_code_commit_passes_without_changing_git_state(self):
        (self.root / "source.rs").write_text("// generated\n", encoding="utf-8")
        self.commit()
        before = self.git("rev-parse", "HEAD")
        self.check()
        self.assertEqual(self.git("rev-parse", "HEAD"), before)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_default_branch_advancing_during_generation_blocks_publication(self):
        (self.root / "source.rs").write_text("// newer main\n", encoding="utf-8")
        self.commit()
        self.remote = self.git("rev-parse", "HEAD")
        with self.assertRaisesRegex(RuntimeError, "default branch advanced"):
            self.check()
        self.assertEqual(len(self.commands), 2)

    def assert_workflow_blocked(self):
        self.commit()
        with self.assertRaisesRegex(RuntimeError, "must not change GitHub workflow files"):
            self.check()

    def test_committed_changed_workflow_is_blocked(self):
        (self.root / ".github" / "workflows" / "ci.yml").write_text("name: changed\n", encoding="utf-8")
        self.assert_workflow_blocked()

    def test_committed_deleted_workflow_is_blocked(self):
        (self.root / ".github" / "workflows" / "ci.yml").unlink()
        self.assert_workflow_blocked()

    def test_committed_added_workflow_is_blocked(self):
        (self.root / ".github" / "workflows" / "new.yml").write_text("name: new\n", encoding="utf-8")
        self.assert_workflow_blocked()

    def test_uncommitted_workflow_edits_are_also_blocked(self):
        (self.root / ".github" / "workflows" / "ci.yml").write_text("name: changed\n", encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "must not change GitHub workflow files"):
            self.check()

    def test_unrelated_generated_branch_is_blocked(self):
        self.git("checkout", "--orphan", "unrelated")
        (self.root / "unrelated.txt").write_text("different root commit\n", encoding="utf-8")
        self.commit()
        with self.assertRaisesRegex(RuntimeError, "not based on the recorded revision"):
            self.check()

    def test_cli_invalid_input_exits_nonzero_without_mutating_checkout(self):
        before = self.git("rev-parse", "HEAD")
        result = subprocess.run(
            [sys.executable, str(Path(BASE.__file__).resolve()),
             "--base-sha", "invalid", "--default-branch", "main"],
            cwd=self.root, capture_output=True, text=True, timeout=60, check=False,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("::error::Missing or invalid recorded generation base", result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(self.git("rev-parse", "HEAD"), before)
        self.assertEqual(self.git("status", "--porcelain"), "")


if __name__ == "__main__":
    unittest.main()
