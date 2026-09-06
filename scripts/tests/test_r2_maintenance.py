from __future__ import annotations

import json
import sys
import unittest
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from r2_maintenance import (  # noqa: E402
    CATALOG_KEY,
    BETA_LATEST_KEY,
    beta_artifact_version,
    beta_state_exists,
    channel_version_key,
    classify,
    delete_keys,
    filter_beta_catalog,
    incomplete_channel_artifacts,
    latest_release,
    manifest_cache_seconds,
    new_retirement_marker,
    optional_json_object,
    optional_json_object_state,
    publish_beta_catalog,
    required_channel_artifacts,
    required_version_artifacts,
    retain_beta_pointer,
    retirement_marker_version,
    semver_tuple,
    validate_committed_manifest,
    validate_retained_beta_catalog,
    validate_retirement_marker,
)


BOARDS = ("pi3", "pi4", "pi5", "zero2w")


def beta_manifest(version: str) -> dict[str, Any]:
    return {
        "schema_version": 2,
        "channel": "beta",
        "version": version,
        "commit": "a" * 40,
        "date": "2026-07-23T12:00:00Z",
        "boards": {
            board: {
                "image": f"snapdog-os-{board}-beta.img.gz",
                "sha256": "b" * 64,
                "raw_sha256": "c" * 64,
                "compressed_size": 100,
                "uncompressed_size": 200,
                "url": (
                    "https://updates.snapdog.cc/os/images/"
                    f"snapdog-os-{board}-{version}.img.gz"
                ),
                "bundle_url": (
                    "https://updates.snapdog.cc/os/bundles/"
                    f"snapdog-os-{board}-{version}.raucb"
                ),
            }
            for board in BOARDS
        },
    }


def beta_catalog(*versions: str) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "channel": "beta",
        "releases": [beta_manifest(version) for version in versions],
    }


def release_manifest(version: str) -> dict[str, Any]:
    manifest = beta_manifest(version)
    manifest["channel"] = "release"
    for board, entry in manifest["boards"].items():
        entry["image"] = f"snapdog-os-{board}-release.img.gz"
    return manifest


class FakeBody:
    def __init__(self, value: bytes) -> None:
        self.value = value

    def read(self) -> bytes:
        return self.value


class FakeS3Error(Exception):
    def __init__(self, code: str, status: int) -> None:
        super().__init__(code)
        self.response = {
            "Error": {"Code": code},
            "ResponseMetadata": {"HTTPStatusCode": status},
        }


class FakeS3:
    def __init__(self) -> None:
        self.get_result: dict[str, Any] | Exception | None = None
        self.get_results: dict[str, dict[str, Any] | Exception] = {}
        self.delete_result: dict[str, Any] = {}
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def get_object(self, **kwargs: Any) -> dict[str, Any]:
        self.calls.append(("get_object", kwargs))
        result = self.get_results.get(kwargs["Key"], self.get_result)
        if isinstance(result, Exception):
            raise result
        assert isinstance(result, dict)
        return result

    def put_object(self, **kwargs: Any) -> None:
        self.calls.append(("put_object", kwargs))

    def delete_object(self, **kwargs: Any) -> None:
        self.calls.append(("delete_object", kwargs))

    def delete_objects(self, **kwargs: Any) -> dict[str, Any]:
        self.calls.append(("delete_objects", kwargs))
        return self.delete_result


class R2MaintenanceRetentionTests(unittest.TestCase):
    release = semver_tuple("1.2.3")

    def test_keeps_beta_for_upcoming_release(self) -> None:
        self.assertEqual(
            classify(
                "os/bundles/snapdog-os-pi4-1.2.4-beta.1.raucb", self.release
            ),
            ("keep", "beta for upcoming release"),
        )

    def test_deletes_same_core_and_superseded_betas(self) -> None:
        for version in ("1.2.3-beta.99", "1.2.2-beta.99"):
            with self.subTest(version=version):
                self.assertEqual(
                    classify(
                        f"os/bundles/snapdog-os-pi4-{version}.raucb",
                        self.release,
                    ),
                    ("delete", "superseded beta"),
                )

    def test_keeps_channel_aliases(self) -> None:
        for key in (
            "os/bundles/snapdog-os-pi4-release.raucb",
            "os/images/snapdog-os-pi4-beta.img.gz",
        ):
            with self.subTest(key=key):
                self.assertEqual(
                    classify(key, self.release),
                    ("keep", "channel alias"),
                )

    def test_keeps_stable_release_versions(self) -> None:
        for version in ("1.2.3", "1.2.2"):
            with self.subTest(version=version):
                self.assertEqual(
                    classify(
                        f"os/bundles/snapdog-os-pi4-{version}.raucb",
                        self.release,
                    ),
                    ("keep", "release version"),
                )

    def test_deletes_versioned_manifests_for_superseded_betas(self) -> None:
        self.assertEqual(
            classify(
                "os/images/manifests/beta/1.2.3-beta.99.json",
                self.release,
            ),
            ("delete", "superseded beta manifest"),
        )
        self.assertEqual(
            beta_artifact_version(
                "os/images/manifests/beta/1.2.3-beta.99.json"
            ),
            "1.2.3-beta.99",
        )

    def test_keeps_malformed_and_nested_beta_like_objects(self) -> None:
        for key in (
            "os/bundles/snapdog-os-pi4-not-a-version-beta.notes.raucb",
            "os/bundles/backup/snapdog-os-pi4-1.2.3-beta.1.raucb",
            "os/bundles/snapdog-os-unknown-1.2.3-beta.1.raucb",
            "os/images/snapdog-os-pi4-01.2.3-beta.1.img.gz",
            "os/images/snapdog-os-pi4-1.2.3-beta.1.raucb",
            "os/bundles/snapdog-os-pi4-1.2.3-beta.1.img.gz",
            "os/bundles/snapdog-os-pi4-1.2.3-beta.1.sha256",
        ):
            with self.subTest(key=key):
                self.assertEqual(classify(key, self.release)[0], "keep")

    def test_channel_order_places_beta_before_same_core_stable(self) -> None:
        self.assertLess(channel_version_key("1.2.3-beta.99"), channel_version_key("1.2.3"))
        self.assertLess(channel_version_key("1.2.3"), channel_version_key("1.2.4-beta.1"))
        with self.assertRaisesRegex(ValueError, "invalid OS channel version"):
            channel_version_key("1.2.3-rc.1")
        self.assertEqual(
            classify(
                "os/images/manifests/beta/1.2.4-beta.1.json",
                self.release,
            ),
            ("keep", "beta version manifest"),
        )


class R2MaintenanceCatalogTests(unittest.TestCase):
    release = semver_tuple("1.2.3")

    def test_filters_dead_entries_but_keeps_current_release_and_future_beta(self) -> None:
        catalog = beta_catalog(
            "1.2.4-beta.2",
            "1.2.3",
            "1.2.3-beta.9",
            "1.2.2-beta.4",
        )

        filtered, removed = filter_beta_catalog(catalog, self.release)

        self.assertEqual(removed, ("1.2.3-beta.9", "1.2.2-beta.4"))
        self.assertIsNotNone(filtered)
        self.assertEqual(
            [release["version"] for release in filtered["releases"]],
            ["1.2.4-beta.2", "1.2.3"],
        )
        self.assertEqual(len(catalog["releases"]), 4, "input must not be mutated")

    def test_returns_no_catalog_when_every_entry_is_obsolete(self) -> None:
        filtered, removed = filter_beta_catalog(
            beta_catalog("1.2.3-beta.2", "1.2.2-beta.9"),
            self.release,
        )

        self.assertIsNone(filtered)
        self.assertEqual(removed, ("1.2.3-beta.2", "1.2.2-beta.9"))

    def test_rejects_non_beta_catalog(self) -> None:
        catalog = beta_catalog("1.2.4")
        catalog["channel"] = "release"
        catalog["releases"][0]["channel"] = "release"

        with self.assertRaisesRegex(ValueError, "beta channel"):
            filter_beta_catalog(catalog, self.release)

    def test_retains_current_pointer_when_filter_empties_catalog(self) -> None:
        current = beta_manifest("1.2.3")
        filtered, _ = filter_beta_catalog(
            beta_catalog("1.2.3-beta.2", "1.2.2-beta.9"),
            self.release,
        )

        retained = retain_beta_pointer(filtered, current)

        self.assertIsNotNone(retained)
        self.assertEqual(
            [release["version"] for release in retained["releases"]],
            ["1.2.3"],
        )

    def test_rejects_pointer_catalog_commit_conflict(self) -> None:
        catalog = beta_catalog("1.2.4-beta.1")
        current = beta_manifest("1.2.4-beta.1")
        current["commit"] = "d" * 40

        with self.assertRaisesRegex(ValueError, "disagree"):
            retain_beta_pointer(catalog, current)

    def test_no_pointer_leaves_absent_catalog_absent(self) -> None:
        self.assertIsNone(retain_beta_pointer(None, None))


class R2MaintenanceObjectTests(unittest.TestCase):
    def test_current_release_requires_every_board_image_and_bundle(self) -> None:
        keys = required_channel_artifacts("release", "1.2.3")

        self.assertEqual(len(keys), 21)
        self.assertIn("os/images/snapdog-os-pi3-1.2.3.img.gz", keys)
        self.assertIn("os/bundles/snapdog-os-zero2w-1.2.3.raucb", keys)
        self.assertIn("os/sbom/snapdog-os-pi4-1.2.3-sbom.csv", keys)
        self.assertIn("os/images/snapdog-os-pi5-release.img.gz", keys)
        self.assertIn("os/images/manifests/release/1.2.3.json", keys)

    def test_incomplete_channel_rejects_empty_and_mismatched_aliases(self) -> None:
        keys = required_channel_artifacts("release", "1.2.3")
        objects = {key: 100 for key in keys}
        objects["os/images/snapdog-os-pi3-release.img.gz"] = 99
        objects["os/images/snapdog-os-pi3-1.2.3.img.gz"] = 99
        objects["os/bundles/snapdog-os-pi4-1.2.3.raucb"] = 0

        manifest = release_manifest("1.2.3")
        incomplete = incomplete_channel_artifacts(objects, "release", manifest)

        self.assertIn("os/bundles/snapdog-os-pi4-1.2.3.raucb", incomplete)
        self.assertTrue(
            any(
                value.startswith("os/images/snapdog-os-pi3-release.img.gz")
                for value in incomplete
            )
        )
        self.assertTrue(
            any(
                value.startswith("os/images/snapdog-os-pi3-1.2.3.img.gz")
                for value in incomplete
            )
        )

    def test_beta_state_detection_fails_closed_on_orphaned_data(self) -> None:
        self.assertTrue(
            beta_state_exists(
                {"os/bundles/snapdog-os-pi4-1.2.4-beta.1.raucb": 100}
            )
        )
        self.assertTrue(beta_state_exists({CATALOG_KEY: 100}))
        self.assertFalse(
            beta_state_exists(
                {"os/bundles/snapdog-os-pi4-1.2.3-release.raucb": 100}
            )
        )
        self.assertFalse(
            beta_state_exists(
                {"os/bundles/backup/foo-beta-notes.raucb": 100}
            )
        )

    def test_optional_json_returns_none_only_for_explicit_not_found(self) -> None:
        s3 = FakeS3()
        s3.get_result = FakeS3Error("NoSuchKey", 404)

        self.assertIsNone(optional_json_object(s3, "bucket", CATALOG_KEY))

    def test_optional_json_propagates_auth_failure(self) -> None:
        s3 = FakeS3()
        s3.get_result = FakeS3Error("AccessDenied", 403)

        with self.assertRaises(FakeS3Error):
            optional_json_object(s3, "bucket", CATALOG_KEY)

    def test_optional_json_parses_an_object(self) -> None:
        s3 = FakeS3()
        s3.get_result = {"Body": FakeBody(b'{"channel":"beta"}')}

        self.assertEqual(
            optional_json_object(s3, "bucket", CATALOG_KEY),
            {"channel": "beta"},
        )

    def test_optional_json_state_requires_and_returns_last_modified(self) -> None:
        s3 = FakeS3()
        modified = datetime(2026, 7, 23, 12, 0, tzinfo=timezone.utc)
        s3.get_result = {
            "Body": FakeBody(b'{"channel":"beta"}'),
            "LastModified": modified,
        }
        self.assertEqual(
            optional_json_object_state(s3, "bucket", BETA_LATEST_KEY),
            ({"channel": "beta"}, modified),
        )

    def test_latest_release_requires_full_valid_manifest(self) -> None:
        s3 = FakeS3()
        modified = datetime(2026, 7, 23, 12, 0, tzinfo=timezone.utc)
        manifest = release_manifest("1.2.3")
        s3.get_result = {
            "Body": FakeBody(json.dumps(manifest).encode()),
            "LastModified": modified,
        }
        self.assertEqual(latest_release(s3, "bucket"), (manifest, modified))

        s3.get_result = {
            "Body": FakeBody(b'{"version":"999.0.0"}'),
            "LastModified": modified,
        }
        with self.assertRaises(ValueError):
            latest_release(s3, "bucket")

    def test_retirement_marker_honors_manifest_cache_and_download_grace(self) -> None:
        now = datetime(
            2026, 7, 23, 12, 0, 0, 500_000, tzinfo=timezone.utc
        )
        marker = new_retirement_marker(
            "1.2.3-beta.9",
            now,
            manifest_max_age_seconds=86_400,
            download_grace_seconds=3_600,
        )

        validate_retirement_marker(marker, "1.2.3-beta.9")
        self.assertEqual(
            marker["payload_not_before_epoch"],
            int(now.timestamp()) + 90_001,
        )
        self.assertEqual(marker["manifest_max_age_seconds"], 86_400)
        self.assertEqual(marker["download_grace_seconds"], 3_600)
        inconsistent = {
            **marker,
            "payload_not_before_epoch": marker["payload_not_before_epoch"] - 1,
        }
        with self.assertRaisesRegex(ValueError, "inconsistent deadline"):
            validate_retirement_marker(inconsistent, "1.2.3-beta.9")
        self.assertEqual(
            manifest_cache_seconds("public, max-age=86400, immutable"),
            86_400,
        )
        self.assertEqual(manifest_cache_seconds(None), 31_536_000)
        self.assertEqual(
            retirement_marker_version(
                "os/.retention/beta/1.2.3-beta.9.json"
            ),
            "1.2.3-beta.9",
        )
        self.assertIsNone(
            retirement_marker_version(
                "os/.retention/beta/archive/1.2.3-beta.9.json"
            )
        )

    def test_committed_manifest_must_equal_pointer(self) -> None:
        s3 = FakeS3()
        pointer = beta_manifest("1.2.4-beta.1")
        key = "os/images/manifests/beta/1.2.4-beta.1.json"
        s3.get_results[key] = {"Body": FakeBody(json.dumps(pointer).encode())}

        validate_committed_manifest(s3, "bucket", "beta", pointer)

        different = {**pointer, "commit": "d" * 40}
        s3.get_results[key] = {"Body": FakeBody(json.dumps(different).encode())}
        with self.assertRaisesRegex(ValueError, "does not exactly match"):
            validate_committed_manifest(s3, "bucket", "beta", pointer)

    def test_retained_catalog_requires_committed_complete_not_future_entries(self) -> None:
        s3 = FakeS3()
        latest = beta_manifest("1.2.4-beta.2")
        retained = beta_catalog("1.2.4-beta.2", "1.2.4-beta.1")
        objects: dict[str, int] = {}
        for release in retained["releases"]:
            key = f"os/images/manifests/beta/{release['version']}.json"
            s3.get_results[key] = {
                "Body": FakeBody(json.dumps(release).encode())
            }
            for artifact in required_version_artifacts("beta", release["version"]):
                objects[artifact] = 100

        validate_retained_beta_catalog(
            s3, "bucket", retained, latest, objects
        )

        future = beta_catalog("1.2.4-beta.3")
        with self.assertRaisesRegex(ValueError, "newer than latest-beta"):
            validate_retained_beta_catalog(
                s3, "bucket", future, latest, objects
            )

        missing_bundle = "os/bundles/snapdog-os-pi4-1.2.4-beta.1.raucb"
        del objects[missing_bundle]
        with self.assertRaisesRegex(ValueError, "is incomplete"):
            validate_retained_beta_catalog(
                s3, "bucket", retained, latest, objects
            )

    def test_delete_batch_errors_are_not_masked(self) -> None:
        s3 = FakeS3()
        s3.delete_result = {
            "Errors": [{"Key": "broken", "Code": "InternalError"}]
        }

        with self.assertRaisesRegex(RuntimeError, "R2 deletion failed"):
            delete_keys(s3, "bucket", ["broken"])

    def test_catalog_publish_uses_cacheable_json_or_deletes_empty_catalog(self) -> None:
        s3 = FakeS3()
        catalog = beta_catalog("1.2.4-beta.1")

        publish_beta_catalog(s3, "bucket", catalog)
        publish_beta_catalog(s3, "bucket", None)

        method, put = s3.calls[0]
        self.assertEqual(method, "put_object")
        self.assertEqual(put["Key"], CATALOG_KEY)
        self.assertEqual(put["ContentType"], "application/json")
        self.assertEqual(json.loads(put["Body"]), catalog)
        self.assertEqual(
            s3.calls[1],
            ("delete_object", {"Bucket": "bucket", "Key": CATALOG_KEY}),
        )


if __name__ == "__main__":
    unittest.main()
