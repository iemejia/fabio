"""Run private Foundry evaluations; publish only a numeric, fail-closed summary."""

import argparse
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import urlsplit


def validate_environment(env):
    required = (
        "AZURE_OPENAI_API_BASE_URL",
        "AZURE_OPENAI_DEPLOYMENT_NAME",
        "AZURE_OPENAI_MODEL_NAME",
        "AZURE_OPENAI_API_KEY",
    )
    missing = [key for key in required if not env.get(key, "").strip()]
    if missing:
        raise ValueError("Missing evaluation configuration: " + ", ".join(missing))
    try:
        endpoint = urlsplit(env["AZURE_OPENAI_API_BASE_URL"])
    except ValueError:
        raise ValueError("AZURE_OPENAI_API_BASE_URL must be an HTTPS account origin") from None
    if (
        endpoint.scheme != "https"
        or not endpoint.hostname
        or endpoint.username is not None
        or endpoint.password is not None
        or endpoint.query
        or endpoint.fragment
        or endpoint.path not in ("", "/")
    ):
        raise ValueError("AZURE_OPENAI_API_BASE_URL must be an HTTPS account origin")
    deployment = env["AZURE_OPENAI_DEPLOYMENT_NAME"]
    if deployment in (".", "..") or not all(
        char.isascii() and (char.isalnum() or char in "-_.") for char in deployment
    ):
        raise ValueError("AZURE_OPENAI_DEPLOYMENT_NAME must be a deployment alias")


def summarize(report, minimum, pattern=None):
    """Reject missing, incomplete, or errored runs regardless of their pass rate."""
    if not math.isfinite(minimum) or not 0 <= minimum <= 100:
        raise ValueError("Minimum pass rate must be between 0 and 100")
    rows = report.get("results", {}).get("results", [])
    tests = report.get("config", {}).get("tests", [])
    if pattern:
        tests = [test for test in tests if re.search(pattern, test.get("description", ""))]
    providers = report.get("config", {}).get("providers", [])
    expected = len(tests) * len(providers)
    if not expected or len(rows) != expected:
        raise ValueError("Evaluation results are empty or incomplete")
    if any(not isinstance(row.get("success"), bool) for row in rows):
        raise ValueError("Evaluation result has no boolean success status")
    if len({(row["testIdx"], row["promptIdx"]) for row in rows}) != expected:
        raise ValueError("Evaluation results contain duplicate test/provider pairs")
    if sorted(row["testCase"]["description"] for row in rows) != sorted(
        test["description"] for test in tests for _ in providers
    ):
        raise ValueError("Evaluation results do not match the selected cases")
    # promptfoo ResultFailureReason: NONE=0, ASSERT=1, ERROR=2. A rubric
    # provider failure may also be nested inside a failed grading result.
    def errored(result):
        if (
            result.get("error")
            or result.get("failureReason") == 2
            or result.get("metadata", {}).get("graderError") is True
        ):
            return True
        if result.get("pass") is False and str(result.get("reason", "")).startswith(
            ("API call error", "Grading failed", "Error:", "Custom function threw error")
        ):
            return True
        return any(errored(child) for child in result.get("componentResults", []) or [])

    errors = sum(
        row.get("failureReason") not in (0, 1)
        or bool(row.get("response", {}).get("error"))
        or row.get("response", {}).get("finishReason") == "length"
        or errored(row.get("gradingResult") or {})
        for row in rows
    )
    passed = sum(row["success"] for row in rows)
    rate = 100 * passed / expected
    failed_descriptions = {
        row["testCase"]["description"] for row in rows if not row["success"]
    }
    return {
        "total": expected,
        "passed": passed,
        "failed": expected - passed,
        "errors": errors,
        "passRate": round(rate, 2),
        "minimumPassRate": minimum,
        "accepted": errors == 0 and rate >= minimum,
        # Index into the authored config, not model/provider text. This lets a
        # contributor locate regressions without publishing raw responses.
        "failedCaseIndices": [
            index
            for index, test in enumerate(report["config"]["tests"])
            if test["description"] in failed_descriptions
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--filter-pattern", help="Run only matching case descriptions")
    parser.add_argument("--min-pass-rate", type=float, default=70)
    parser.add_argument("--timeout", type=int, default=1200)
    parser.add_argument("--summary", type=Path, help="Write a publishable numeric summary")
    args = parser.parse_args()
    try:
        if args.summary:
            # A failed rerun must not leave a previous accepted summary behind.
            args.summary.write_text('{"accepted":false}\n', encoding="utf-8")
        try:
            validate_environment(os.environ)
        except ValueError as error:
            # These messages enumerate variable names, never their values.
            print(f"Invalid evaluation configuration: {error}")
            return 1
        executable = shutil.which("promptfoo")
        if executable is None:
            raise ValueError("Install promptfoo as documented in tests/eval/README.md")
        if not math.isfinite(args.min_pass_rate) or not 0 <= args.min_pass_rate <= 100:
            raise ValueError("Minimum pass rate must be between 0 and 100")
        if args.timeout <= 0:
            raise ValueError("Evaluation timeout must be positive")
        # Never stream provider output or upload raw results: both can contain
        # resolved endpoints, deployment aliases, credentials, and tenant data.
        with tempfile.TemporaryDirectory(prefix="fabio-eval-") as temporary:
            directory = Path(temporary)
            env = dict(os.environ)
            env.update(
                PROMPTFOO_DISABLE_TELEMETRY="1",
                PROMPTFOO_DISABLE_UPDATE="1",
                PROMPTFOO_CONFIG_DIR=str(directory / "state"),
            )
            command = [
                executable, "eval", "-c", "promptfooconfig.yaml",
                "--no-cache", "--no-write", "--no-share", "--no-table",
                "--no-progress-bar", "--max-concurrency", "3",
                "-o", str(directory / "results.json"),
            ]
            if args.filter_pattern:
                command.extend(["--filter-pattern", args.filter_pattern])
            with (directory / "output.log").open("w", encoding="utf-8") as log:
                result = subprocess.run(
                    command, cwd=Path(__file__).resolve().parent, env=env,
                    stdout=log, stderr=subprocess.STDOUT, timeout=args.timeout,
                    check=False,
                )
            # promptfoo's documented assertion-failure exit is 100; apply our
            # configured pass-rate gate, but never swallow operational failures.
            if result.returncode not in (0, 100):
                raise ValueError("Evaluation process failed; private output withheld")
            report = json.loads((directory / "results.json").read_text(encoding="utf-8"))
            summary = summarize(report, args.min_pass_rate, args.filter_pattern)
        if args.summary:
            args.summary.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(summary))
        return 0 if summary["accepted"] else 1
    except (ValueError, KeyError, TypeError, AttributeError, OSError, subprocess.SubprocessError):
        # Exception messages can embed URLs, response bodies, or subprocess args.
        print(
            "Evaluation failed: check required configuration, process completion, "
            "and result integrity; private output withheld."
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
