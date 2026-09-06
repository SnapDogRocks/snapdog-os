from __future__ import annotations

import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HELPERS = ROOT / "scripts" / "r2_helpers.sh"


class R2HelperTests(unittest.TestCase):
    def invoke(self, aws_body: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            fake_aws = Path(directory) / "aws"
            fake_aws.write_text(f"#!/usr/bin/env bash\n{aws_body}\n", encoding="utf-8")
            fake_aws.chmod(fake_aws.stat().st_mode | stat.S_IXUSR)
            environment = {
                **os.environ,
                "AWS_ENDPOINT_URL": "https://r2.invalid",
                "PATH": f"{directory}:{os.environ['PATH']}",
            }
            return subprocess.run(
                [
                    "bash",
                    "-c",
                    f"source {HELPERS!s}; r2_get_optional s3://bucket/key output",
                ],
                cwd=directory,
                env=environment,
                text=True,
                capture_output=True,
                check=False,
            )

    def test_success_is_downloaded(self) -> None:
        result = self.invoke('printf data > "$4"')
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_explicit_missing_key_has_distinct_status(self) -> None:
        result = self.invoke('echo "fatal error: (404) Not Found" >&2; exit 1')
        self.assertEqual(result.returncode, 44)
        self.assertIn("is absent", result.stderr)

    def test_authentication_failure_is_not_treated_as_missing(self) -> None:
        result = self.invoke('echo "AccessDenied: bad token" >&2; exit 17')
        self.assertEqual(result.returncode, 17)
        self.assertIn("AccessDenied", result.stderr)
        self.assertNotIn("is absent", result.stderr)


if __name__ == "__main__":
    unittest.main()
