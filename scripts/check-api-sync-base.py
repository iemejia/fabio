#!/usr/bin/env python3
"""Refuse to publish API-sync work from a stale base or with workflow edits.

Only reads Git/GitHub state. GH_TOKEN authenticates the GitHub CLI; this helper
never configures credentials, rebases generated changes, or pushes anything.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote


def run(command: list[str], repository: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        command, cwd=repository, capture_output=True, text=True, timeout=60,
        check=False,
    )


def check_base(repository: Path, base_sha: str, default_branch: str,
               github_repository: str, runner=run) -> None:
    if not re.fullmatch(r"[0-9a-f]{40}", base_sha):
        raise RuntimeError("Missing or invalid recorded generation base; start a fresh workflow run")
    if not re.fullmatch(r"[A-Za-z0-9-]+/[A-Za-z0-9_.-]+", github_repository):
        raise RuntimeError("Missing or invalid GITHUB_REPOSITORY")
    branch = runner(["git", "check-ref-format", f"refs/heads/{default_branch}"], repository)
    if not default_branch or branch.returncode:
        raise RuntimeError("Missing or invalid default branch")
    result = runner([
        "gh", "api", f"repos/{github_repository}/git/ref/heads/{quote(default_branch, safe='')}",
        "--jq", ".object.sha",
    ], repository)
    if result.returncode or not re.fullmatch(r"[0-9a-f]{40}", result.stdout.strip()):
        raise RuntimeError("Could not verify the current default-branch revision; refusing publication")
    if result.stdout.strip() != base_sha:
        raise RuntimeError(
            "The default branch advanced since generation began; start a fresh workflow run "
            "on the default branch, not a re-run of the old attempt"
        )
    ancestry = runner(["git", "merge-base", "--is-ancestor", base_sha, "HEAD"], repository)
    if ancestry.returncode:
        raise RuntimeError("The generated branch is not based on the recorded revision; refusing publication")
    workflows = runner([
        "git", "diff", "--exit-code", base_sha, "--", ".github/workflows",
    ], repository)
    if workflows.returncode:
        raise RuntimeError(
            "API sync must not change GitHub workflow files; review those changes separately "
            "instead of granting Workflows permission"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--default-branch", required=True)
    args = parser.parse_args()
    try:
        check_base(
            Path.cwd(), args.base_sha, args.default_branch,
            os.environ.get("GITHUB_REPOSITORY", ""), runner=run,
        )
    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
        print(f"::error::{error}", file=sys.stderr)
        return 1
    print("API sync base is current; no workflow changes detected.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
