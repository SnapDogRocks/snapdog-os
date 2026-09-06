from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from r2_stable_retention import (  # noqa: E402
    artifact_version,
    load_policy,
    marker_key,
    marker_version,
)


class StableRetentionTests(unittest.TestCase):
    def test_recognizes_only_canonical_stable_payloads_and_manifests(self) -> None:
        self.assertEqual(artifact_version("os/images/snapdog-os-pi4-1.2.3.img.gz"), "1.2.3")
        self.assertEqual(artifact_version("os/images/manifests/release/1.2.3.json"), "1.2.3")
        self.assertIsNone(artifact_version("os/images/snapdog-os-pi4-release.img.gz"))
        self.assertIsNone(artifact_version("os/images/snapdog-os-pi4-1.2.3-beta.1.img.gz"))
        self.assertIsNone(artifact_version("os/images/archive/snapdog-os-pi4-1.2.3.img.gz"))

    def test_policy_is_strict_and_supports_pins(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps({"stable_releases": 5, "pinned_versions": ["1.2.3"]}))
            self.assertEqual(load_policy(path), (5, {"1.2.3"}))
            path.write_text(json.dumps({"stable_releases": 0, "pinned_versions": []}))
            with self.assertRaisesRegex(ValueError, "positive integer"):
                load_policy(path)

    def test_release_markers_are_namespaced(self) -> None:
        key = marker_key("1.2.3")
        self.assertEqual(key, "os/.retention/release/1.2.3.json")
        self.assertEqual(marker_version(key), "1.2.3")
        self.assertIsNone(marker_version("os/.retention/beta/1.2.3.json"))


if __name__ == "__main__":
    unittest.main()
