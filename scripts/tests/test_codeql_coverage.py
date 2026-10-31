import argparse
import importlib.util
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("coverage_gate", ROOT / "scripts/check_codeql_coverage.py")
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class CodeqlCoverageTests(unittest.TestCase):
    def test_pagination_stays_on_the_authenticated_github_origin(self):
        self.assertEqual(
            GATE.next_page('<https://api.github.com/repos/o/r/checks?page=2>; rel="next"'),
            "/repos/o/r/checks?page=2",
        )
        self.assertIsNone(GATE.next_page(""))
        for url in ("https://evil.example/checks", "http://api.github.com/checks",
                    "https://api.github.com@evil.example/checks"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                GATE.next_page(f'<{url}>; rel="next"')

    def setUp(self):
        self.sha = "a" * 40
        self.run = {"head_sha": self.sha, "status": "completed", "conclusion": "success",
                    "run_started_at": "2026-09-12T12:00:00Z"}
        self.jobs = [{"name": f"Analyze ({lang})", "conclusion": "success"}
                     for lang in GATE.LANGUAGES]
        self.checks = [{"id": 1, "name": "CodeQL", "app": {"id": 57789},
                        "head_sha": self.sha, "conclusion": "success",
                        "completed_at": "2026-09-12T12:01:00Z"}]

    def complete(self):
        return GATE.coverage_complete(self.run, self.jobs, self.checks, self.sha)

    def test_all_languages_and_final_result_required(self):
        self.assertTrue(self.complete())
        self.jobs.pop()
        self.assertFalse(self.complete())

    def test_neutral_skipped_pending_and_failed_do_not_pass(self):
        for conclusion in (None, "neutral", "skipped", "failure", "cancelled"):
            with self.subTest(conclusion=conclusion):
                self.jobs[0]["conclusion"] = conclusion
                self.assertFalse(self.complete())

    def test_other_sha_or_running_attempt_does_not_pass(self):
        self.run["head_sha"] = "b" * 40
        self.assertFalse(self.complete())
        self.run["head_sha"] = self.sha
        self.run["status"] = "in_progress"
        self.assertFalse(self.complete())

    def test_latest_codeql_result_must_pass(self):
        self.checks.append(dict(self.checks[0], id=2, conclusion="neutral"))
        self.assertFalse(self.complete())

    def test_missing_or_spoofed_result_does_not_pass(self):
        self.checks[0]["app"]["id"] = 15368
        self.assertFalse(self.complete())

    def test_result_from_previous_attempt_does_not_pass(self):
        self.run["run_started_at"] = "2026-09-12T13:00:00Z"
        self.assertFalse(self.complete())
        self.checks[0]["completed_at"] = "2026-09-12T13:01:00Z"
        self.assertTrue(self.complete())
        self.checks = []
        self.assertFalse(self.complete())


class CodeqlWorkflowTests(unittest.TestCase):
    """The gate and .github/workflows/codeql.yml must describe the same analysis."""

    def setUp(self):
        self.workflow = (ROOT / ".github/workflows/codeql.yml").read_text()
        self.ci = (ROOT / ".github/workflows/ci.yml").read_text()

    def test_gate_reads_the_repository_workflow(self):
        self.assertIn("--workflow codeql.yml", self.ci)
        self.assertNotRegex(self.ci, r"--workflow-id")

    def test_workflow_argument_stays_inside_the_api_path(self):
        for value in ("codeql.yml", "codeql.yaml", "308299760"):
            self.assertEqual(GATE.workflow_name(value), value)
        for value in ("", "../codeql.yml", "a/codeql.yml", "codeql.yml?per_page=1",
                      "codeql", "codeql.yml/runs"):
            with self.subTest(value=value), self.assertRaises(argparse.ArgumentTypeError):
                GATE.workflow_name(value)

    def test_matrix_and_job_names_match_the_gate(self):
        matrix = re.search(r"^\s+language: \[([^\]]+)\]$", self.workflow, re.M)
        self.assertIsNotNone(matrix)
        self.assertEqual(tuple(lang.strip() for lang in matrix[1].split(",")), GATE.LANGUAGES)
        self.assertIn("name: Analyze (${{ matrix.language }})", self.workflow)
        self.assertIn("category: /language:${{ matrix.language }}", self.workflow)
        self.assertIn("build-mode: none", self.workflow)

    def test_dependabot_pull_requests_are_analysed_and_uploaded(self):
        on = self.workflow.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertRegex(on, r"\n  pull_request:\n    branches: \[main\]")
        self.assertNotIn("pull_request_target", self.workflow)
        self.assertNotIn("dependabot", on)
        self.assertNotIn("github.actor", self.workflow)
        self.assertRegex(self.workflow, r"\npermissions: \{\}\n")
        self.assertRegex(self.workflow, r"\n      security-events: write\n")

    def test_actions_are_immutable(self):
        for action in re.findall(r"uses:\s+(\S+)", self.workflow):
            with self.subTest(action=action):
                self.assertRegex(action, r"^[\w./-]+@[0-9a-f]{40}$")


if __name__ == "__main__":
    unittest.main()
