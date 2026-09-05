from __future__ import annotations

import json
# Only exception/response fixtures; all subprocess calls are mocked.
import subprocess  # nosec B404
import sys
import unittest
from copy import deepcopy
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from finalize_release import expected_assets, finalize_release, validate_release  # noqa: E402

REPO = "SnapDogRocks/snapdog-os"
OS_TAG = "v0.16.5"


def release_fixture(kind="os", tag=OS_TAG, draft=True):
    return {
        "tagName": tag,
        "isDraft": draft,
        "assets": [
            {"name": name, "state": "uploaded", "size": 1024}
            for name in sorted(expected_assets(kind, tag))
        ],
    }


class ReleasePublicationTests(unittest.TestCase):
    def setUp(self):
        cli_path = patch("finalize_release.shutil.which", return_value="/usr/bin/gh")
        cli_path.start()
        self.addCleanup(cli_path.stop)

    def finalize(self, release, kind="os", tag=OS_TAG):
        with patch("finalize_release.subprocess.run") as gh:
            gh.return_value.stdout = json.dumps(release)
            finalize_release(kind, tag, REPO)
            return [call.args[0] for call in gh.call_args_list]

    def test_complete_os_release_publishes_as_latest(self):
        release = release_fixture()
        self.assertEqual(len(release["assets"]), 28)
        commands = self.finalize(release)
        self.assertEqual(commands[0][:4], ["/usr/bin/gh", "release", "view", OS_TAG])
        self.assertEqual(commands[1], [
            "/usr/bin/gh", "release", "edit", OS_TAG, "--repo", REPO,
            "--draft=false", "--verify-tag", "--prerelease=false", "--latest=true",
        ])

    def test_no_uploads_never_publishes(self):
        release = release_fixture()
        release["assets"] = []
        with patch("finalize_release.subprocess.run") as gh:
            gh.return_value.stdout = json.dumps(release)
            with self.assertRaisesRegex(ValueError, "Missing release assets"):
                finalize_release("os", OS_TAG, REPO)
            gh.assert_called_once()

    def test_every_missing_firmware_or_updater_asset_blocks_publication(self):
        complete = release_fixture()
        for asset in complete["assets"]:
            with self.subTest(missing=asset["name"]):
                release = deepcopy(complete)
                release["assets"].remove(asset)
                with patch("finalize_release.subprocess.run") as gh:
                    gh.return_value.stdout = json.dumps(release)
                    with self.assertRaisesRegex(ValueError, "Missing release assets"):
                        finalize_release("os", OS_TAG, REPO)
                    gh.assert_called_once()

    def test_empty_and_unfinished_assets_block_publication(self):
        for changes in ({"size": 0}, {"state": "starter"}, {"state": "failed"}):
            with self.subTest(changes=changes):
                release = release_fixture()
                release["assets"][0].update(changes)
                with self.assertRaisesRegex(ValueError, "Empty or incomplete"):
                    validate_release(release, expected_assets("os", OS_TAG), OS_TAG)

    def test_already_published_complete_release_is_a_read_only_success(self):
        commands = self.finalize(release_fixture(draft=False))
        self.assertEqual(len(commands), 1)

    def test_already_published_incomplete_release_still_fails(self):
        release = release_fixture(draft=False)
        release["assets"].pop()
        with self.assertRaisesRegex(ValueError, "Missing release assets"):
            self.finalize(release)

    def test_standalone_updater_requires_aggregate_checksums(self):
        tag = "snapdog-update-v0.3.0"
        release = release_fixture("updater", tag)
        self.assertEqual(len(release["assets"]), 9)
        release["assets"] = [a for a in release["assets"] if a["name"] != "SHA256SUMS"]
        with self.assertRaisesRegex(ValueError, "Missing release assets: SHA256SUMS"):
            self.finalize(release, "updater", tag)

    def test_updater_preserves_prerelease_and_does_not_replace_latest_os(self):
        for version, prerelease in (("0.3.0", False), ("0.3.0-rc.1", True)):
            with self.subTest(version=version):
                tag = f"snapdog-update-v{version}"
                commands = self.finalize(release_fixture("updater", tag), "updater", tag)
                self.assertIn(f"--prerelease={str(prerelease).lower()}", commands[1])
                self.assertIn("--latest=false", commands[1])

    def test_tag_mismatch_and_duplicate_assets_are_rejected(self):
        release = release_fixture()
        release["tagName"] = "v0.16.4"
        with self.assertRaisesRegex(ValueError, "Release tag does not match"):
            self.finalize(release)
        release = release_fixture()
        release["assets"].append(release["assets"][0])
        with self.assertRaisesRegex(ValueError, "duplicate asset names"):
            self.finalize(release)

    def test_invalid_tag_fails_before_contacting_github(self):
        with patch("finalize_release.subprocess.run") as gh:
            with self.assertRaisesRegex(ValueError, "Invalid os release tag"):
                finalize_release("os", "snapdog-ctrl-v0.14.3", REPO)
            gh.assert_not_called()

    def test_invalid_repository_fails_before_contacting_github(self):
        with patch("finalize_release.subprocess.run") as gh:
            with self.assertRaisesRegex(ValueError, "Invalid GitHub repository"):
                finalize_release("os", OS_TAG, "owner/repo; command")
            gh.assert_not_called()

    def test_missing_github_cli_fails_before_publication(self):
        with patch("finalize_release.shutil.which", return_value=None):
            with self.assertRaisesRegex(ValueError, "GitHub CLI .* is not installed"):
                finalize_release("os", OS_TAG, REPO)

    def test_github_read_failure_never_publishes(self):
        with patch("finalize_release.subprocess.run") as gh:
            gh.side_effect = subprocess.CalledProcessError(1, "gh", stderr="API unavailable")
            with self.assertRaises(subprocess.CalledProcessError):
                finalize_release("os", OS_TAG, REPO)
            gh.assert_called_once()

    def test_github_publish_failure_is_not_reported_as_success(self):
        with patch("finalize_release.subprocess.run") as gh:
            gh.side_effect = [
                subprocess.CompletedProcess("gh", 0, json.dumps(release_fixture())),
                subprocess.CalledProcessError(1, "gh"),
            ]
            with self.assertRaises(subprocess.CalledProcessError):
                finalize_release("os", OS_TAG, REPO)


if __name__ == "__main__":
    unittest.main()
