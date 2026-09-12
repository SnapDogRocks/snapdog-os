from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "stable-retention.yml"


def _step(workflow: str, name: str) -> str:
    """Return one named step without depending on a YAML parser."""

    marker = f"      - name: {name}\n"
    start = workflow.index(marker)
    end = workflow.find("\n      - name:", start + len(marker))
    return workflow[start:] if end == -1 else workflow[start:end]


class StableRetentionWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = WORKFLOW.read_text()
        cls.acquire = _step(cls.workflow, "Acquire R2 mutation lock")
        cls.retain = _step(cls.workflow, "Retain five stable releases")
        cls.release = _step(cls.workflow, "Release R2 mutation lock")

    def test_retention_step_is_bounded_well_inside_lease_ttl(self) -> None:
        timeout = int(re.search(r"timeout-minutes:\s*(\d+)", self.retain).group(1))
        ttl = int(
            re.search(r"--ttl-seconds\s+(\d+)", self.acquire).group(1)
        )

        self.assertGreater(timeout, 0)
        self.assertLess(timeout * 60, ttl)
        self.assertEqual(timeout, 30)
        self.assertEqual(ttl, 7200)

    def test_retention_renews_lease_before_mutating(self) -> None:
        owner = self.retain.index('owner=$(cat /tmp/r2-lock-owner)')
        renewal = self.retain.index("tools/r2_lock.py renew")
        operation = self.retain.index("tools/r2_stable_retention.py")

        self.assertLess(owner, renewal)
        self.assertLess(renewal, operation)
        self.assertIn('--owner "$owner"', self.retain)
        self.assertIn("--ttl-seconds 7200", self.retain)
        self.assertIn('test -n "$owner"', self.retain)

    def test_lock_lifecycle_fails_closed_and_releases_always(self) -> None:
        self.assertIn("set -euo pipefail", self.acquire)
        self.assertIn("set -euo pipefail", self.retain)
        self.assertIn("set -euo pipefail", self.release)
        self.assertIn('if: always()', self.release)
        self.assertIn('[ -s /tmp/r2-lock-owner ]', self.release)
        self.assertIn('[ -x /tmp/r2venv/bin/python ]', self.release)
        self.assertIn("tools/r2_lock.py release", self.release)

        acquire = self.workflow.index("- name: Acquire R2 mutation lock")
        retain = self.workflow.index("- name: Retain five stable releases")
        release = self.workflow.index("- name: Release R2 mutation lock")
        self.assertLess(acquire, retain)
        self.assertLess(retain, release)

    def test_apply_policy_gate_remains_unchanged(self) -> None:
        self.assertIn('apply=""', self.retain)
        self.assertIn(
            'if [ "${{ github.event_name }}" = schedule ] || '
            '[ "${{ inputs.apply }}" = true ]; then apply="--apply"; fi',
            self.retain,
        )
        self.assertIn("tools/r2_stable_retention.py $apply", self.retain)


if __name__ == "__main__":
    unittest.main()
