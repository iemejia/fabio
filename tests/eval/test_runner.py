"""Hermetic unit and subprocess-boundary tests for the evaluation safety gate."""

import contextlib
import copy
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import run


def environment():
    return {
        "AZURE_OPENAI_API_BASE_URL": "https://example.openai.azure.com",
        "AZURE_OPENAI_DEPLOYMENT_NAME": "example-deployment",
        "AZURE_OPENAI_MODEL_NAME": "gpt-6.1-sol",
        "AZURE_OPENAI_API_KEY": "test-only-key",
    }


def report(outcomes=(True, True)):
    return {
        "config": {
            "tests": [{"description": f"case-{index}"} for index, _ in enumerate(outcomes)],
            "providers": [{"id": "example"}],
        },
        "results": {
            "results": [
                {
                    "testIdx": index, "promptIdx": 0,
                    "testCase": {"description": f"case-{index}"},
                    "success": success, "failureReason": 0 if success else 1,
                }
                for index, success in enumerate(outcomes)
            ]
        },
    }


class EnvironmentTests(unittest.TestCase):
    def test_explicit_env_file_loads_only_evaluation_values_without_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / ".env"
            path.write_text(
                "IGNORED=$(not-a-command)\r\n"
                "export AZURE_OPENAI_MODEL_NAME='gpt-6.1-sol'\r\n"
                "AZURE_OPENAI_API_KEY='literal$NOT_EXPANDED#key' # ignored comment\r\n",
                encoding="utf-8",
            )
            original = {"UNCHANGED": "value"}
            loaded = run.load_environment(original, path)
            self.assertEqual(loaded["AZURE_OPENAI_MODEL_NAME"], "gpt-6.1-sol")
            self.assertEqual(loaded["AZURE_OPENAI_API_KEY"], "literal$NOT_EXPANDED#key")
            self.assertNotIn("IGNORED", loaded)
            self.assertEqual(original, {"UNCHANGED": "value"})

    def test_process_environment_takes_precedence_over_explicit_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / ".env"
            path.write_text("AZURE_OPENAI_API_KEY=file-value\n", encoding="utf-8")
            for existing, expected in (("process-value", "process-value"), (" ", "file-value")):
                loaded = run.load_environment({"AZURE_OPENAI_API_KEY": existing}, path)
                self.assertEqual(loaded["AZURE_OPENAI_API_KEY"], expected)

    def test_malformed_or_duplicate_env_settings_fail_without_echoing_values(self):
        for content in [
            "AZURE_OPENAI_API_KEY='secret",
            "AZURE_OPENAI_API_KEY=secret two-values",
            "AZURE_OPENAI_API_KEY=secret\nAZURE_OPENAI_API_KEY=other-secret",
        ]:
            with tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary) / ".env"
                path.write_text(content, encoding="utf-8")
                with self.assertRaises(ValueError) as error:
                    run.load_environment({}, path)
                self.assertNotIn("secret", str(error.exception))

    def test_hash_inside_an_unquoted_value_is_not_silently_removed(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / ".env"
            endpoint = "https://example.openai.azure.com/#fragment"
            path.write_text(f"AZURE_OPENAI_API_BASE_URL={endpoint}\n", encoding="utf-8")
            loaded = run.load_environment(environment(), path)
            self.assertEqual(loaded["AZURE_OPENAI_API_BASE_URL"], environment()["AZURE_OPENAI_API_BASE_URL"])
            loaded = run.load_environment({}, path)
            self.assertEqual(loaded["AZURE_OPENAI_API_BASE_URL"], endpoint)
            loaded.update({name: value for name, value in environment().items() if name != "AZURE_OPENAI_API_BASE_URL"})
            with self.assertRaises(ValueError):
                run.validate_environment(loaded)

    def test_accepts_https_account_origin(self):
        run.validate_environment(environment())

    def test_all_required_variables_fail_fast_without_values_in_errors(self):
        for key in environment():
            with self.subTest(key=key):
                env = environment()
                env[key] = " "
                with self.assertRaisesRegex(ValueError, key):
                    run.validate_environment(env)

    def test_rejects_plaintext_credentials_paths_and_query_strings(self):
        for endpoint in (
            "http://example.openai.azure.com", "https:///", "https://user:pass@example.org",
            "https://example.org/openai/v1", "https://example.org?token=private",
            "https://example.org/#private", "not-an-endpoint",
            "https://[private",
        ):
            with self.subTest(endpoint=endpoint):
                env = environment()
                env["AZURE_OPENAI_API_BASE_URL"] = endpoint
                with self.assertRaises(ValueError) as error:
                    run.validate_environment(env)
                self.assertNotIn(endpoint, str(error.exception))

    def test_rejects_alias_path_injection(self):
        for deployment in (".", "..", "../deployment", "a?key=private", "a/b", "{{unresolved}}", " a"):
            env = environment()
            env["AZURE_OPENAI_DEPLOYMENT_NAME"] = deployment
            with self.assertRaises(ValueError):
                run.validate_environment(env)


class SummaryTests(unittest.TestCase):
    def test_threshold_uses_unrounded_rate(self):
        data = report((True, True, False))
        self.assertTrue(run.summarize(data, 66)["accepted"])
        self.assertFalse(run.summarize(data, 67)["accepted"])
        self.assertEqual(run.summarize(data, 66)["passRate"], 66.67)
        self.assertEqual(run.summarize(data, 66)["failedCaseIndices"], [2])

    def test_empty_missing_incomplete_and_duplicate_reports_fail_closed(self):
        complete = report()
        incomplete = copy.deepcopy(complete)
        incomplete["results"]["results"].pop()
        duplicate = copy.deepcopy(complete)
        duplicate["results"]["results"][1] = duplicate["results"]["results"][0]
        wrong_case = copy.deepcopy(complete)
        wrong_case["results"]["results"][1]["testCase"]["description"] = "unexpected"
        no_status = copy.deepcopy(complete)
        no_status["results"]["results"][0]["success"] = None
        for data in ({}, report(()), incomplete, duplicate, wrong_case, no_status):
            with self.subTest(data=data), self.assertRaises(ValueError):
                run.summarize(data, 70)

    def test_provider_and_nested_grader_errors_cannot_pass_threshold(self):
        for update in (
            {"failureReason": 2},
            {"response": {"error": "private provider error"}},
            {"response": {"finishReason": "length"}},
            {"gradingResult": {"componentResults": [{"pass": False, "reason": "API call error: private"}]}},
            {"gradingResult": {"componentResults": [{"pass": False, "reason": "Custom function threw error: undefined"}]}},
            {"gradingResult": {"componentResults": [{"pass": False, "metadata": {"graderError": True}}]}},
        ):
            data = report()
            data["results"]["results"][0].update(update)
            summary = run.summarize(data, 0)
            self.assertFalse(summary["accepted"])
            self.assertEqual(summary["errors"], 1)
            self.assertNotIn("private", json.dumps(summary))

    def test_filtered_expectation_does_not_use_the_full_config_count(self):
        data = report()
        data["results"]["results"] = data["results"]["results"][:1]
        self.assertEqual(run.summarize(data, 100, "^case-0$")["total"], 1)

    def test_threshold_bounds(self):
        for minimum in (-1, 101, float("nan"), float("inf")):
            with self.assertRaises(ValueError):
                run.summarize(report(), minimum)


class RunnerTests(unittest.TestCase):
    def invoke(self, args=(), returncode=0, payload=None, failure=None, process_environment=None):
        self.command = None
        self.process_env = None

        def fake_process(command, **kwargs):
            self.command = command
            self.process_env = kwargs["env"]
            kwargs["stdout"].write("private output that must not be published")
            if failure:
                raise failure
            if payload is not None:
                Path(command[command.index("-o") + 1]).write_text(json.dumps(payload), encoding="utf-8")
            return subprocess.CompletedProcess(command, returncode)

        output = io.StringIO()
        with (
            patch.dict(
                os.environ,
                environment() if process_environment is None else process_environment,
                clear=True,
            ),
            patch("sys.argv", ["run.py", *args]),
            patch("run.shutil.which", return_value="promptfoo"),
            patch("run.subprocess.run", side_effect=fake_process),
            contextlib.redirect_stdout(output),
        ):
            status = run.main()
        self.assertNotIn("private", output.getvalue().replace("private output withheld", ""))
        return status, output.getvalue()

    def test_success_writes_numeric_summary_and_disables_persistence_sharing_and_telemetry(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "summary.json"
            status, output = self.invoke(("--summary", str(path)), payload=report())
            self.assertEqual(status, 0)
            self.assertTrue(json.loads(path.read_text())["accepted"])
            self.assertEqual(json.loads(output)["passed"], 2)
        for flag in ("--no-cache", "--no-write", "--no-share", "--no-table"):
            self.assertIn(flag, self.command)
        self.assertEqual(self.process_env["PROMPTFOO_DISABLE_TELEMETRY"], "1")
        self.assertFalse(Path(self.process_env["PROMPTFOO_CONFIG_DIR"]).parent.exists())

    def test_assertion_exit_applies_threshold_but_operational_exit_cannot_pass(self):
        status, _ = self.invoke(returncode=100, payload=report((True, True, True, False)))
        self.assertEqual(status, 0)
        status, _ = self.invoke(returncode=1, payload=report())
        self.assertEqual(status, 1)

    def test_timeout_and_absent_results_fail_without_publishing_logs(self):
        status, _ = self.invoke(failure=subprocess.TimeoutExpired("private endpoint", 1))
        self.assertEqual(status, 1)
        status, _ = self.invoke()
        self.assertEqual(status, 1)

    def test_environment_failure_never_starts_a_process(self):
        output = io.StringIO()
        with (
            patch.dict(os.environ, {}, clear=True),
            patch("sys.argv", ["run.py"]),
            patch("run.subprocess.run") as process,
            contextlib.redirect_stdout(output),
        ):
            self.assertEqual(run.main(), 1)
            process.assert_not_called()
        for key in environment():
            self.assertIn(key, output.getvalue())

    def test_explicit_file_supplies_missing_provider_configuration_to_the_child(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / ".env"
            path.write_text(
                "".join(f"{name}='{value}'\n" for name, value in environment().items()),
                encoding="utf-8",
            )
            status, _ = self.invoke(
                ("--env-file", str(path)), payload=report(), process_environment={}
            )
            self.assertEqual(status, 0)
            for name, value in environment().items():
                self.assertEqual(self.process_env[name], value)

    def test_errored_results_fail_even_with_permissive_pass_rate(self):
        payload = report()
        payload["results"]["results"][0]["failureReason"] = 2
        status, _ = self.invoke(("--min-pass-rate", "0"), payload=payload)
        self.assertEqual(status, 1)

    def test_invalid_threshold_and_timeout_never_start_a_process(self):
        for args in (("--min-pass-rate", "nan"), ("--timeout", "0")):
            status, _ = self.invoke(args)
            self.assertEqual(status, 1)
            self.assertIsNone(self.command)

    def test_failed_rerun_cannot_leave_a_stale_accepted_summary(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "summary.json"
            path.write_text('{"accepted":true}', encoding="utf-8")
            status, _ = self.invoke(("--summary", str(path)))
            self.assertEqual(status, 1)
            self.assertFalse(json.loads(path.read_text())["accepted"])


if __name__ == "__main__":
    unittest.main()
