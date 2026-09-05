from __future__ import annotations

import io
import json
import sys
import unittest
from copy import deepcopy
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from finalize_release import expected_assets, main, publication_state, validate_release  # noqa: E402

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
    def run_cli(self, payload, kind="os", tag=OS_TAG):
        output, errors = io.StringIO(), io.StringIO()
        with (
            patch("sys.argv", ["finalize_release.py", "--kind", kind, "--tag", tag]),
            patch("sys.stdin", io.StringIO(payload)),
            patch("sys.stdout", output),
            patch("sys.stderr", errors),
        ):
            code = main()
        return code, output.getvalue(), errors.getvalue()

    def test_complete_os_release_can_be_published(self):
        release = release_fixture()
        self.assertEqual(len(release["assets"]), 28)
        self.assertEqual(self.run_cli(json.dumps(release)), (0, "draft\n", ""))

    def test_no_uploads_never_publishes(self):
        release = release_fixture()
        release["assets"] = []
        code, output, errors = self.run_cli(json.dumps(release))
        self.assertEqual(code, 1)
        self.assertEqual(output, "")
        self.assertIn("Missing release assets", errors)

    def test_every_missing_firmware_or_updater_asset_blocks_publication(self):
        complete = release_fixture()
        for asset in complete["assets"]:
            with self.subTest(missing=asset["name"]):
                release = deepcopy(complete)
                release["assets"].remove(asset)
                code, output, errors = self.run_cli(json.dumps(release))
                self.assertEqual(code, 1)
                self.assertEqual(output, "")
                self.assertIn(asset["name"], errors)

    def test_empty_and_unfinished_assets_block_publication(self):
        for changes in ({"size": 0}, {"state": "starter"}, {"state": "failed"}):
            with self.subTest(changes=changes):
                release = release_fixture()
                release["assets"][0].update(changes)
                with self.assertRaisesRegex(ValueError, "Empty or incomplete"):
                    validate_release(release, expected_assets("os", OS_TAG), OS_TAG)

    def test_already_published_complete_release_is_a_read_only_success(self):
        result = self.run_cli(json.dumps(release_fixture(draft=False)))
        self.assertEqual(result, (0, "published\n", ""))

    def test_already_published_incomplete_release_still_fails(self):
        release = release_fixture(draft=False)
        release["assets"].pop()
        with self.assertRaisesRegex(ValueError, "Missing release assets"):
            publication_state("os", OS_TAG, release)

    def test_standalone_updater_requires_aggregate_checksums(self):
        tag = "snapdog-update-v0.3.0"
        release = release_fixture("updater", tag)
        self.assertEqual(len(release["assets"]), 9)
        release["assets"] = [a for a in release["assets"] if a["name"] != "SHA256SUMS"]
        with self.assertRaisesRegex(ValueError, "Missing release assets: SHA256SUMS"):
            publication_state("updater", tag, release)

    def test_stable_and_prerelease_updater_assets_are_supported(self):
        for version in ("0.3.0", "0.3.0-rc.1"):
            with self.subTest(version=version):
                tag = f"snapdog-update-v{version}"
                release = release_fixture("updater", tag)
                self.assertEqual(self.run_cli(json.dumps(release), "updater", tag), (0, "draft\n", ""))

    def test_tag_mismatch_and_duplicate_assets_are_rejected(self):
        release = release_fixture()
        release["tagName"] = "v0.16.4"
        with self.assertRaisesRegex(ValueError, "Release tag does not match"):
            publication_state("os", OS_TAG, release)
        release = release_fixture()
        release["assets"].append(release["assets"][0])
        with self.assertRaisesRegex(ValueError, "duplicate asset names"):
            publication_state("os", OS_TAG, release)

    def test_invalid_tag_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Invalid os release tag"):
            publication_state("os", "snapdog-ctrl-v0.14.3", release_fixture())

    def test_missing_or_malformed_api_data_never_emits_a_publishable_state(self):
        for payload in ("", "not json", "null", "[]", "{}"):
            with self.subTest(payload=payload):
                code, output, errors = self.run_cli(payload)
                self.assertEqual(code, 1)
                self.assertEqual(output, "")
                self.assertIn("Release publication refused", errors)

    def test_draft_state_must_be_a_boolean(self):
        for state in (None, "false", "true", 0, 1):
            with self.subTest(state=state):
                release = release_fixture()
                release["isDraft"] = state
                code, output, errors = self.run_cli(json.dumps(release))
                self.assertEqual(code, 1)
                self.assertEqual(output, "")
                self.assertIn("invalid release draft state", errors)


if __name__ == "__main__":
    unittest.main()
