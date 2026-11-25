#!/usr/bin/env python3
"""Prepare compatible, registry-only Cargo.lock security fixes; never publish them.

Requires Python 3.11+ and cargo-audit. A nonzero exit blocks publication, including
when some fixes succeeded but other advisories still need manual intervention.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
# Conservative SPDX allowlist: unfamiliar licenses/exception expressions need review.
PERMISSIVE = {
    "MIT", "MIT-0", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "0BSD",
    "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016", "BSL-1.0", "Unlicense",
    "CC0-1.0", "BlueOak-1.0.0",
}


def run(command: list[str], repository: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        command, cwd=repository, capture_output=True, text=True, timeout=300,
        check=False,
    )


def checked(command: list[str], repository: Path, runner=run) -> str:
    result = runner(command, repository)
    if result.returncode:
        raise RuntimeError(f"Command failed: {' '.join(command)}")
    return result.stdout


def audit(repository: Path, runner=run, *, fetch: bool) -> tuple[dict, bool]:
    command = ["cargo", "audit", "--deny", "warnings", "--json"]
    if not fetch:
        command.append("--no-fetch")
    result = runner(command, repository)
    report = json.loads(result.stdout)
    vulnerabilities = report["vulnerabilities"]
    findings = vulnerabilities["list"]
    warnings = report["warnings"]
    if (
        not isinstance(findings, list) or not isinstance(warnings, dict)
        or vulnerabilities["count"] != len(findings)
        or vulnerabilities["found"] != bool(findings)
        or any(not isinstance(entries, list) for entries in warnings.values())
        or result.returncode not in (0, 1)
        or (result.returncode == 1 and not findings and not any(warnings.values()))
        or (result.returncode == 0 and findings)
        or report.get("error")
        or report.get("settings", {}).get("ignore")
        or report.get("settings", {}).get("severity")
        or report.get("settings", {}).get("target_arch")
        or report.get("settings", {}).get("target_os")
    ):
        raise RuntimeError("Invalid audit result or suppressed advisories; refusing publication")
    return report, result.returncode == 0 and not any(warnings.values())


def identity(package: dict) -> tuple[str, str, str | None]:
    return package["name"], package["version"], package.get("source")


def packages(lockfile: bytes) -> set[tuple[str, str, str | None]]:
    return {identity(package) for package in tomllib.loads(lockfile.decode())["package"]}


def release(version: str) -> tuple[int, int, int]:
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise RuntimeError(f"Prerelease or unfamiliar version needs manual review: {version}")
    return tuple(int(part) for part in version.split("."))


def compatible(old: str, new: str) -> bool:
    before, after = release(old), release(new)
    width = 1 if before[0] else (2 if before[1] else 3)
    return after >= before and after[:width] == before[:width]


def permissive(expression: str | None) -> bool:
    if not expression:
        return False
    tokens = re.findall(r"[A-Za-z0-9.+-]+|[()]", expression)
    if "".join(tokens) != re.sub(r"\s+", "", expression):
        return False
    expecting_license, depth = True, 0
    for token in tokens:
        if token == "(" and expecting_license:
            depth += 1
        elif token in PERMISSIVE and expecting_license:
            expecting_license = False
        elif token in {"AND", "OR"} and not expecting_license:
            expecting_license = True
        elif token == ")" and not expecting_license and depth:
            depth -= 1
        else:
            return False
    return not expecting_license and depth == 0


def validate_changes(before: bytes, after: bytes, metadata: dict, msrv: str) -> None:
    old, new = packages(before), packages(after)
    # Existing git/path dependencies may not be refreshed by this automation.
    if {entry for entry in old if entry[2] != REGISTRY} != {
        entry for entry in new if entry[2] != REGISTRY
    }:
        raise RuntimeError("A non-crates.io dependency changed; manual review required")
    resolved = {identity(package): package for package in metadata["packages"]}
    for entry in new - old:
        package = resolved[entry]
        if not permissive(package.get("license")):
            raise RuntimeError(f"License needs manual review: {entry[0]} {entry[1]}")
        minimum = package.get("rust_version")
        if minimum and release(minimum if minimum.count(".") == 2 else minimum + ".0") > release(msrv):
            raise RuntimeError(f"Rust version exceeds project MSRV: {entry[0]} {entry[1]}")


def targets(report: dict) -> list[tuple[tuple[str, str, str | None], str]]:
    # Use published stable lower bounds, not an unconstrained dependency refresh.
    # cargo update without --precise can retain a vulnerable transitive crate when
    # its fix needs a newer dependency that the conservative resolver keeps locked.
    selected = {}
    for finding in report["vulnerabilities"]["list"]:
        key = identity(finding["package"])
        if key[2] != REGISTRY:
            continue
        candidates = []
        for requirement in finding["versions"]["patched"]:
            match = re.match(r"^(?:>=|\^|~|=)?\s*(\d+\.\d+\.\d+)(?=\s*(?:,|$))", requirement)
            if match and compatible(key[1], match[1]) and release(match[1]) > release(key[1]):
                candidates.append(match[1])
        if candidates:
            candidate = min(candidates, key=release)
            selected[key] = max(candidate, selected.get(key, candidate), key=release)
    return sorted(selected.items())


def describe(report: dict) -> list[str]:
    lines = []
    entries = report["vulnerabilities"]["list"] + [
        entry for values in report["warnings"].values() for entry in values
    ]
    for entry in entries:
        package = entry["package"]
        advisory = entry.get("advisory", {})
        lines.append(f"- `{package['name']} {package['version']}`: `{advisory.get('id', 'warning')}`")
    return lines


def prepare(repository: Path, report_file: Path, runner=run) -> bool:
    lock = repository / "Cargo.lock"
    if report_file.resolve() in {lock.resolve(), (repository / "Cargo.toml").resolve()}:
        raise RuntimeError("The report path must not overwrite a Cargo input file")
    original = lock.read_bytes()
    manifest = (repository / "Cargo.toml").read_bytes()
    notes = ["# Cargo security remediation", ""]
    mutated = False
    try:
        checked(["git", "diff", "--exit-code", "HEAD", "--", "Cargo.lock", "Cargo.toml"], repository, runner)
        initial, clean = audit(repository, runner, fetch=True)
        notes += ["## Initial findings", *describe(initial)]
        if clean:
            notes.append("No security findings; no dependency updates needed.")
            return False
        msrv = tomllib.loads(manifest.decode())["package"]["rust-version"]
        for (name, version, source), patched in targets(initial):
            if (name, version, source) not in packages(lock.read_bytes()):
                continue  # A previous update already replaced this transitive dependency.
            snapshot = lock.read_bytes()
            mutated = True
            result = runner([
                "cargo", "update", "--package", f"{name}@{version}", "--precise", patched,
                "--config", 'resolver.incompatible-rust-versions="fallback"',
            ], repository)
            if result.returncode:
                lock.write_bytes(snapshot)
                notes.append(f"- Could not resolve a compatible update for `{name} {version}`.")
                continue
            updated = packages(lock.read_bytes())
            if (name, patched, source) not in updated or (name, version, source) in updated:
                lock.write_bytes(snapshot)
                notes.append(f"- No compatible newer version of `{name} {version}` was selected.")
            else:
                notes.append(f"- Updated `{name}` from `{version}` to patched `{patched}`.")
        final, clean = audit(repository, runner, fetch=False)
        notes += ["", "## Remaining findings", *describe(final)]
        if not clean:
            raise RuntimeError("Strict audit still fails; remaining findings need manual intervention")
        if lock.read_bytes() != original:
            metadata = json.loads(checked(["cargo", "metadata", "--locked", "--format-version", "1"], repository, runner))
            validate_changes(original, lock.read_bytes(), metadata, msrv)
        if (repository / "Cargo.toml").read_bytes() != manifest:
            raise RuntimeError("Cargo.toml changed; lockfile-only remediation required")
        notes += ["All findings resolved. Normal CI and human review are still required.", "",
                  "This PR is maintained by the scheduled security workflow; do not edit its bot-owned branch."]
        return lock.read_bytes() != original
    except Exception as error:
        if mutated:
            lock.write_bytes(original)
        notes += ["", f"Blocked: {error}"]
        raise
    finally:
        report_file.write_text("\n".join(notes) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    try:
        changed = prepare(args.repository.resolve(), args.report)
    except (RuntimeError, ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        print(f"Security remediation blocked: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"changed": changed, "audit_clean": True}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
