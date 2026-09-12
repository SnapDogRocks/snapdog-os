from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
ROOT = SCRIPTS_DIR.parent
sys.path.insert(0, str(SCRIPTS_DIR))

from homebrew_updater import (  # noqa: E402
    FormulaError,
    checks_registered,
    decide,
    orphan_branch_is_deletable,
    parse_formula,
    semver_key,
    validate_formula,
)


def formula(tag: str, x86_sha: str = "a" * 64, arm_sha: str = "b" * 64) -> str:
    return f'''class SnapdogUpdate < Formula
  desc "Firmware update client for SnapDog OS"
  homepage "https://github.com/SnapDogRocks/snapdog-os"
  license "GPL-3.0-only"
  version_scheme 1

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/{tag}/{tag}-x86_64-apple-darwin.tar.gz"
      sha256 "{x86_sha}"
    else
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/{tag}/{tag}-aarch64-apple-darwin.tar.gz"
      sha256 "{arm_sha}"
    end
  end
end
'''


def legacy_formula() -> str:
    return '''class SnapdogUpdate < Formula
  desc "Firmware update client for SnapDog OS"
  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/v0.16.6/snapdog-update-v0.16.6-x86_64-apple-darwin.tar.gz"
      sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    else
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/v0.16.6/snapdog-update-v0.16.6-aarch64-apple-darwin.tar.gz"
      sha256 "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    end
  end
end
'''


class HomebrewUpdaterTests(unittest.TestCase):
    def test_semver_suffix_validation_and_ordering(self) -> None:
        for invalid in ("01.2.3", "1.2", "1.2.3-", "1.2.3+", "1.2.3-rc..1",
                        "1.2.3-01", "1.2.3+x+y", "1.2.3+bad/metadata"):
            with self.subTest(version=invalid), self.assertRaises(FormulaError):
                semver_key(invalid)
        self.assertLess(semver_key("1.2.3-rc.2"), semver_key("1.2.3-rc.10"))
        self.assertLess(semver_key("1.2.3-rc.10"), semver_key("1.2.3"))
        self.assertEqual(semver_key("1.2.3+001"), semver_key("1.2.3+abc"))

    def test_legacy_os_version_migrates_to_updater_scheme(self) -> None:
        state = parse_formula(legacy_formula())
        self.assertEqual(state.version, "0.16.6")
        self.assertEqual(state.scheme, 0)
        self.assertTrue(state.legacy)

        decision = decide(legacy_formula(), "0.4.2")
        self.assertEqual(decision["action"], "migrate")
        # The old OS version must never be compared numerically with the new
        # updater stream (0.16.6 would otherwise incorrectly win over 0.4.2).
        self.assertEqual(decision["validation_version"], "0.4.2")

    def test_version_scheme_one_is_preserved_for_later_updates(self) -> None:
        current = formula("snapdog-update-v0.4.2")
        self.assertEqual(decide(current, "0.4.3")["action"], "update")
        self.assertEqual(parse_formula(current).scheme, 1)
        self.assertFalse(parse_formula(current).legacy)

        missing_directive = current.replace("  version_scheme 1\n", "")
        self.assertEqual(parse_formula(missing_directive).scheme, 0)
        self.assertEqual(decide(missing_directive, "0.4.2")["action"], "migrate")

    def test_same_version_requires_exact_formula_integrity(self) -> None:
        tag = "snapdog-update-v0.4.2"
        good = formula(tag)
        self.assertEqual(decide(good, "0.4.2")["action"], "noop")
        validate_formula(
            good,
            tag=tag,
            version="0.4.2",
            x86_sha="a" * 64,
            arm_sha="b" * 64,
        )
        with self.assertRaises(FormulaError):
            validate_formula(
                good.replace("sha256 \"" + "a" * 64, "sha256 \"" + "c" * 64),
                tag=tag,
                version="0.4.2",
                x86_sha="a" * 64,
                arm_sha="b" * 64,
            )

    def test_pending_check_exit_status_does_not_hide_registered_checks(self) -> None:
        pending = json.dumps([{"name": "Formula qualification", "state": "PENDING"}])
        self.assertTrue(checks_registered(pending))
        self.assertFalse(checks_registered("[]"))
        self.assertFalse(checks_registered("not-json"))

    def test_cleanup_is_forbidden_when_an_open_pr_uses_the_branch(self) -> None:
        self.assertTrue(orphan_branch_is_deletable(0))
        self.assertFalse(orphan_branch_is_deletable(1))

    def test_workflow_contains_the_regression_guards(self) -> None:
        workflow = (ROOT / ".github/workflows/release-snapdog-update.yml").read_text()
        self.assertIn("version_scheme 1", workflow)
        self.assertIn("--json name,state,bucket", workflow)
        self.assertIn("gh pr list", workflow)
        self.assertIn("open PR", workflow)
        self.assertIn("snapdog-update-homebrew-tap", workflow)
        self.assertIn("required_status_checks", workflow)
        self.assertIn("Formula qualification", workflow)
        self.assertIn("pull_request", workflow)


if __name__ == "__main__":
    unittest.main()
