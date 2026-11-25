"""Hermetic unit and subprocess integration tests; no registry or GitHub access."""

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "cargo_security_updates", Path(__file__).with_name("cargo-security-updates.py")
)
SECURITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SECURITY)
REGISTRY = SECURITY.REGISTRY


def lockfile(versions):
    return ('version = 4\n' + ''.join(
        f'\n[[package]]\nname = "{name}"\nversion = "{version}"\nsource = "{REGISTRY}"\n'
        for name, version in versions.items()
    )).encode()


def finding(name="url", version="2.5.8", patched=None, source=REGISTRY):
    return {
        "package": {"name": name, "version": version, "source": source},
        "advisory": {"id": "RUSTSEC-2026-0592"},
        "versions": {"patched": [">=2.5.9"] if patched is None else patched},
    }


def report(findings=None, warnings=None):
    entries = [] if findings is None else findings
    return {
        "vulnerabilities": {"list": entries, "count": len(entries), "found": bool(entries)},
        "warnings": {} if warnings is None else warnings,
        "settings": {"ignore": []},
    }


class RemediationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.lock = self.root / "Cargo.lock"
        self.original = lockfile({"url": "2.5.8", "other": "1.0.0"})
        self.lock.write_bytes(self.original)
        (self.root / "Cargo.toml").write_text('[package]\nrust-version = "1.100.0"\n')
        self.output = self.root / "report.md"
        self.commands = []
        self.audit_reports = [report([finding()]), report()]
        self.license = "MIT OR Apache-2.0"
        self.rust_version = "1.63"
        self.fail_update = False
        self.update_version = "2.5.9"

    def runner(self, command, repository):
        self.commands.append(command)
        if command[:2] == ["git", "diff"]:
            return subprocess.CompletedProcess(command, 0, "", "")
        if command[:2] == ["cargo", "audit"]:
            value = self.audit_reports.pop(0)
            code = int(bool(value["vulnerabilities"]["list"] or any(value["warnings"].values())))
            return subprocess.CompletedProcess(command, code, json.dumps(value), "")
        if command[:2] == ["cargo", "update"]:
            if self.fail_update:
                self.lock.write_bytes(b"failed resolver partial write")
                return subprocess.CompletedProcess(command, 1, "", "resolver failed")
            values = {name: version for name, version, _ in SECURITY.packages(self.lock.read_bytes())}
            name = command[command.index("--package") + 1].split("@")[0]
            values[name] = self.update_version if name == "url" else "1.0.1"
            self.lock.write_bytes(lockfile(values))
            return subprocess.CompletedProcess(command, 0, "", "")
        if command[:2] == ["cargo", "metadata"]:
            data = {"packages": [
                {"name": name, "version": version, "source": source,
                 "license": self.license, "rust_version": self.rust_version}
                for name, version, source in SECURITY.packages(self.lock.read_bytes())
            ]}
            return subprocess.CompletedProcess(command, 0, json.dumps(data), "")
        self.fail(f"Unexpected command: {command}")

    def prepare(self):
        return SECURITY.prepare(self.root, self.output, self.runner)

    def assert_blocked(self, message):
        with self.assertRaisesRegex(RuntimeError, message):
            self.prepare()
        self.assertEqual(self.lock.read_bytes(), self.original)
        self.assertIn("Blocked:", self.output.read_text())

    def test_clean_audit_does_not_update_or_resolve_metadata(self):
        self.audit_reports = [report()]
        self.assertFalse(self.prepare())
        self.assertEqual([command[:2] for command in self.commands], [["git", "diff"], ["cargo", "audit"]])
        self.assertEqual(self.lock.read_bytes(), self.original)

    def test_all_patchable_crates_are_updated_once(self):
        self.audit_reports = [report([finding(), finding(), finding("other", "1.0.0", patched=[">=1.0.1"])]), report()]
        self.assertTrue(self.prepare())
        updates = [command for command in self.commands if command[:2] == ["cargo", "update"]]
        self.assertEqual(len(updates), 2)
        self.assertEqual({command[3] for command in updates}, {"url@2.5.8", "other@1.0.0"})
        for command in updates:
            self.assertIn('resolver.incompatible-rust-versions="fallback"', command)
            self.assertIn("--precise", command)
        self.assertIn("All findings resolved", self.output.read_text())
        self.assertIn("--no-fetch", [command for command in self.commands if command[:2] == ["cargo", "audit"]][-1])

    def test_unpatched_advisory_blocks_publication(self):
        self.audit_reports = [report([finding(patched=[])]), report([finding(patched=[])])]
        self.assert_blocked("remaining findings")
        self.assertFalse(any(command[:2] == ["cargo", "update"] for command in self.commands))

    def test_partial_fixes_are_rolled_back_when_another_advisory_remains(self):
        other = finding("other", "1.0.0", patched=[])
        self.audit_reports = [report([finding(), other]), report([other])]
        self.assert_blocked("remaining findings")
        self.assertIn("other 1.0.0", self.output.read_text())

    def test_failed_resolver_is_rolled_back_and_reported(self):
        self.fail_update = True
        self.audit_reports[-1] = report([finding()])
        self.assert_blocked("remaining findings")
        self.assertIn("Could not resolve", self.output.read_text())

    def test_major_or_prerelease_update_is_not_published(self):
        for version in ["3.0.0", "2.5.9-beta.1"]:
            with self.subTest(version=version):
                self.setUp()
                self.update_version = version
                self.audit_reports[-1] = report([finding()])
                self.assert_blocked("remaining findings|Prerelease")

    def test_nonpermissive_and_unknown_licenses_block_publication(self):
        for license in ["GPL-3.0-only", "MIT OR GPL-3.0-only", None, "custom"]:
            with self.subTest(license=license):
                self.setUp()
                self.license = license
                self.assert_blocked("License needs manual review")

    def test_incompatible_msrv_blocks_publication(self):
        self.rust_version = "1.101"
        self.assert_blocked("exceeds project MSRV")

    def test_warning_only_report_is_not_treated_as_clean(self):
        warning = report(warnings={"unmaintained": [finding()]})
        self.audit_reports = [warning, warning]
        self.assert_blocked("remaining findings")

    def test_malformed_audit_and_suppression_fail_closed(self):
        values = [{}] + [
            {**report(), "settings": {key: value}}
            for key, value in [
                ("ignore", ["RUSTSEC-2026-0592"]), ("severity", "high"),
                ("target_os", ["linux"]), ("target_arch", ["x86_64"]),
            ]
        ]
        for value in values:
            value = dict(value)
            with self.subTest(value=value):
                self.setUp()
                self.audit_reports = [value]
                with self.assertRaises((KeyError, RuntimeError)):
                    self.prepare()
                self.assertEqual(self.lock.read_bytes(), self.original)

    def test_report_cannot_overwrite_input_files(self):
        for name in ["Cargo.lock", "Cargo.toml"]:
            destination = self.root / name
            original = destination.read_bytes()
            with self.assertRaisesRegex(RuntimeError, "must not overwrite"):
                SECURITY.prepare(self.root, destination, self.runner)
            self.assertEqual(destination.read_bytes(), original)

    def test_dirty_lockfile_is_not_overwritten(self):
        self.lock.write_bytes(b"user changes")
        def dirty(command, repository):
            return subprocess.CompletedProcess(command, 1, "", "")
        with self.assertRaisesRegex(RuntimeError, "Command failed"):
            SECURITY.prepare(self.root, self.output, dirty)
        self.assertEqual(self.lock.read_bytes(), b"user changes")

    def test_nonregistry_dependency_changes_require_manual_review(self):
        new = self.original.replace(REGISTRY.encode(), b"git+https://example.com/dependency.git")
        with self.assertRaisesRegex(RuntimeError, "non-crates.io"):
            SECURITY.validate_changes(self.original, new, {"packages": []}, "1.100.0")

    def test_semver_compatibility_and_license_allowlist(self):
        for old, new, expected in [
            ("1.2.3", "1.9.0", True), ("1.2.3", "2.0.0", False),
            ("0.2.3", "0.2.4", True), ("0.2.3", "0.3.0", False),
            ("0.0.3", "0.0.4", False), ("1.2.3", "1.2.2", False),
        ]:
            self.assertEqual(SECURITY.compatible(old, new), expected)
        self.assertTrue(SECURITY.permissive("(MIT OR Apache-2.0) AND BSD-3-Clause"))
        self.assertFalse(SECURITY.permissive("MIT WITH unknown-exception"))
        self.assertFalse(SECURITY.permissive("MIT / Apache-2.0"))
        for expression in ["MIT OR", "AND MIT", "()", "(MIT", "MIT Apache-2.0", "MIT)"]:
            self.assertFalse(SECURITY.permissive(expression))

    def test_patched_range_selection_deduplicates_and_rejects_incompatible_bounds(self):
        data = report([
            finding(patched=[">=3.0.0", ">=2.5.9, <3.0.0"]),
            finding(patched=["^2.5.10"]),
            finding("other", "1.0.0", patched=["> 1.0.0"]),
            finding("git-crate", "1.0.0", source="git+https://example.com/source.git"),
        ])
        self.assertEqual(SECURITY.targets(data), [(("url", "2.5.8", REGISTRY), "2.5.10")])

    def test_real_subprocess_transport_and_cli_error_exit(self):
        # Exercise JSON/exit-code transport without running cargo or accessing a tenant.
        def subprocess_runner(command, repository):
            result = self.runner(command, repository)
            return subprocess.run([
                sys.executable, "-c",
                "import sys; print(sys.argv[1]); sys.exit(int(sys.argv[2]))",
                result.stdout, str(result.returncode),
            ], cwd=repository, capture_output=True, text=True, check=False)
        self.assertTrue(SECURITY.prepare(self.root, self.output, subprocess_runner))
        self.lock.write_bytes(b"dirty lockfile")
        result = subprocess.run([
            sys.executable, str(Path(SECURITY.__file__)), "--repository", str(self.root),
            "--report", str(self.output),
        ], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("Security remediation blocked", result.stderr)
        self.assertEqual(self.lock.read_bytes(), b"dirty lockfile")


if __name__ == "__main__":
    unittest.main()
