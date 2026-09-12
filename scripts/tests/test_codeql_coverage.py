import importlib.util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("coverage_gate", ROOT / "scripts/check_codeql_coverage.py")
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class CodeqlCoverageTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
