#!/usr/bin/env python3
"""Context-coverage audit for fabio (release-time heuristic reviewer).

The mechanical agent-discovery surfaces are already drift-tested
(`agent_schema_covers_*`, `subskills_match_generated`,
`every_command_group_has_a_knowledge_home`, `skill_family_shared_references_exist`).
What has NO forcing function is the *judgment* surface: when a change introduces
a new behavior/gotcha an agent must know, the authored `fabio context` knowledge
(skill `key_gotchas`/`troubleshooting`/`prefer`, best-practices, examples,
workflows) can silently lag.

This script codifies the manual audit: over a commit range it surfaces
CANDIDATES that a human triages before a release —

  1. NEW subcommands (from commands.json) not referenced by name anywhere in the
     context data (split into non-CRUD = high signal vs CRUD = usually fine).
  2. NEW teaching errors (with_hint/with_typed_hint added in src/**.rs) — listed
     for review (a teaching error usually encodes a gotcha agents should also
     discover proactively via context, not only hit at runtime).
  3. NEW API-BEHAVIORS-DISCOVERED.md section headings — listed for review.

It is intentionally heuristic (a judgment gap cannot be a hard mechanical gate):
by default it prints a report and exits 0. Pass --strict to exit non-zero when
any NON-CRUD new command is unreferenced in context (the strongest signal).

Usage:
  scripts/audit-context-coverage.py [--since <ref>] [--strict]

  --since <ref>   Base git ref to diff against (default: latest tag, else the
                  first commit).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
COMMANDS_JSON = "src/commands/context/data/agent/commands.json"
CONTEXT_DATA_DIR = REPO / "src/commands/context/data"
API_BEHAVIORS = ".agents/API-BEHAVIORS-DISCOVERED.md"
SRC_DIR = "src"

# Subcommand verbs that are self-explanatory CRUD/read — a new one usually does
# NOT need a bespoke gotcha/example (low signal). Anything else is high signal.
CRUD_PREFIXES = (
    "list",
    "show",
    "get",
    "create",
    "update",
    "delete",
    "add",
    "remove",
    "set",
    "describe",
)


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=REPO, capture_output=True, text=True, check=False
    ).stdout


def default_base() -> str:
    tag = git("describe", "--tags", "--abbrev=0").strip()
    if tag:
        return tag
    return git("rev-list", "--max-parents=0", "HEAD").split()[0]


def subcommands(schema: dict) -> set[str]:
    out: set[str] = set()
    for group, gdef in schema.items():
        if not isinstance(gdef, dict):
            continue
        for sub in (gdef.get("subcommands") or {}):
            out.add(f"{group} {sub}")
    return out


def load_schema_at(ref: str) -> dict:
    raw = git("show", f"{ref}:{COMMANDS_JSON}")
    if not raw.strip():
        return {}
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return {}


def context_data_blob() -> str:
    """All authored context data concatenated (lowercased) for substring search."""
    parts: list[str] = []
    for path in CONTEXT_DATA_DIR.rglob("*.json"):
        # Skip the generated command schema itself — we want AUTHORED knowledge.
        if path.name == "commands.json":
            continue
        parts.append(path.read_text(encoding="utf-8"))
    return "\n".join(parts).lower()


def is_crud(sub_name: str) -> bool:
    return any(sub_name == p or sub_name.startswith(p + "-") for p in CRUD_PREFIXES)


def new_teaching_errors(base: str) -> list[str]:
    diff = git("diff", f"{base}..HEAD", "--", f"{SRC_DIR}/**/*.rs", SRC_DIR)
    hints: list[str] = []
    lines = diff.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        is_added = line.startswith("+") and not line.startswith("+++")
        if is_added and ("with_hint(" in line or "with_typed_hint(" in line):
            # Gather the message/hint string literals from the following added
            # lines (the call spans several lines: code, message, hint).
            snippet: list[str] = []
            j = i + 1
            while j < len(lines) and len(snippet) < 4:
                nxt = lines[j]
                if nxt.startswith("+++"):
                    break
                if nxt.startswith("+") and '"' in nxt:
                    snippet.append(nxt[1:].strip().strip(","))
                elif not nxt.startswith(("+", " ")):
                    break  # left the added hunk region
                j += 1
            msg = " | ".join(snippet) if snippet else line[1:].strip()
            hints.append(msg)
        i += 1
    return hints


def new_behavior_sections(base: str) -> list[str]:
    diff = git("diff", f"{base}..HEAD", "--", API_BEHAVIORS)
    headings: list[str] = []
    for line in diff.splitlines():
        m = re.match(r"^\+(#{2,4})\s+(.*)$", line)
        if m:
            headings.append(m.group(2).strip())
    return headings


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--since", help="base git ref (default: latest tag)")
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit non-zero if a non-CRUD new command is unreferenced in context",
    )
    args = ap.parse_args()

    base = args.since or default_base()
    head = git("rev-parse", "--short", "HEAD").strip()
    print(f"Context-coverage audit: {base}..{head} (HEAD)\n")

    old = subcommands(load_schema_at(base))
    new_schema = json.loads((REPO / COMMANDS_JSON).read_text(encoding="utf-8"))
    cur = subcommands(new_schema)
    added = sorted(cur - old)

    blob = context_data_blob()

    high, low = [], []
    for cmd in added:
        group, sub = cmd.split(" ", 1)
        # Covered if the "group sub" pair OR the bare subcommand appears in any
        # authored context file.
        referenced = cmd.lower() in blob or f" {sub.lower()}" in blob
        if referenced:
            continue
        (low if is_crud(sub) else high).append(cmd)

    print(f"== NEW subcommands since {base}: {len(added)} ==")
    if high:
        print(
            f"\n[!] {len(high)} NON-CRUD new command(s) NOT referenced in any "
            "context data (skills gotchas/prefer, best-practices, examples, "
            "workflows) — likely need a gotcha/example/workflow so agents "
            "discover them:"
        )
        for c in high:
            print(f"      - {c}")
    if low:
        print(
            f"\n[.] {len(low)} CRUD-style new command(s) not referenced "
            "(usually fine — self-explanatory; review only if non-obvious):"
        )
        for c in low:
            print(f"      - {c}")
    if not high and not low:
        print("    all new subcommands are referenced in context data. Good.")

    hints = new_teaching_errors(base)
    print(f"\n== NEW teaching errors (with_hint/with_typed_hint): {len(hints)} ==")
    print(
        "    Review each: if it encodes a non-obvious gotcha, add it to the "
        "relevant fabio-<family> skill key_gotchas/troubleshooting so agents "
        "discover it proactively (not only at runtime)."
    )
    for h in hints[:40]:
        print(f"      + {h[:160]}")
    if len(hints) > 40:
        print(f"      ... and {len(hints) - 40} more")

    sections = new_behavior_sections(base)
    if sections:
        print(f"\n== NEW API-BEHAVIORS section headings: {len(sections)} ==")
        print(
            "    Confirm each agent-relevant behavior is ALSO reflected in a "
            "fabio context surface (skill gotcha / best-practice / example)."
        )
        for s in sections:
            print(f"      # {s}")

    print(
        "\nThis is a heuristic reviewer — triage the candidates above. "
        "Not every item needs an edit, but each should be a conscious decision."
    )

    if args.strict and high:
        print(
            f"\nFAIL (--strict): {len(high)} non-CRUD new command(s) have no "
            "context discovery aid."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
