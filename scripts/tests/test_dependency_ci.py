"""Keep dependency qualification reproducible and safe for untrusted PRs."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]


class DependencyCiTests(unittest.TestCase):
    def test_ci_installs_the_committed_webui_lockfile(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertNotRegex(workflow, r"\bnpm install\b")
        self.assertEqual(workflow.count("npm ci --prefer-offline"), 3)
        self.assertIn(
            "git diff --exit-code -- snapdog-ctrl/webui/package.json "
            "snapdog-ctrl/webui/package-lock.json",
            workflow,
        )

    def test_audits_need_no_secrets_or_write_permissions(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        audit = workflow.split("  security-audit:\n", 1)[1].split(
            "  cross-compile:\n", 1
        )[0]
        self.assertNotIn("rustsec/audit-check@", audit)
        self.assertNotIn("secrets.", audit)
        self.assertNotIn("continue-on-error", audit)
        self.assertNotIn("pull_request_target", workflow)
        self.assertRegex(audit, r"cargo install cargo-audit --version \d+\.\d+\.\d+ --locked")
        for crate in ("snapdog-ctrl", "snapdog-update"):
            self.assertIn(f"cargo audit --file {crate}/Cargo.lock --deny warnings", audit)
        self.assertIn("npm audit --omit=dev --audit-level=high", audit)
        self.assertIn("npm audit --audit-level=high", audit)

    def test_ci_actions_are_immutable(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for action in re.findall(r"uses:\s+(\S+)", workflow):
            with self.subTest(action=action):
                self.assertRegex(action, r"^[\w./-]+@[0-9a-f]{40}$")

    def test_maintainer_push_does_not_reenable_auto_merge(self):
        workflow = (ROOT / ".github/workflows/dependabot-auto-merge.yml").read_text()
        self.assertIn("github.actor == 'dependabot[bot]'", workflow)


if __name__ == "__main__":
    unittest.main()
