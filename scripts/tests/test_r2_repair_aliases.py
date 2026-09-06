from __future__ import annotations

import io
import json
import sys
import unittest
from contextlib import redirect_stdout
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from r2_repair_aliases import BOARDS, repair_channel  # noqa: E402


def manifest(channel: str = "release", version: str = "1.2.3") -> dict[str, Any]:
    return {
        "schema_version": 2,
        "channel": channel,
        "version": version,
        "commit": "a" * 40,
        "date": "2026-07-23T12:00:00Z",
        "boards": {
            board: {
                "image": f"snapdog-os-{board}-{channel}.img.gz",
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


class FakeBody:
    def __init__(self, value: bytes) -> None:
        self.value = value

    def read(self) -> bytes:
        return self.value


class MissingObject(Exception):
    def __init__(self) -> None:
        super().__init__("NoSuchKey")
        self.response = {"Error": {"Code": "NoSuchKey"}}


class FakeS3:
    def __init__(self, pointer: dict[str, Any]) -> None:
        channel = pointer["channel"]
        version = pointer["version"]
        self.json_objects = {
            f"os/images/latest-{channel}.json": pointer,
            f"os/images/manifests/{channel}/{version}.json": pointer,
        }
        self.heads: dict[str, dict[str, Any]] = {}
        self.copy_calls: list[dict[str, Any]] = []
        self.pointer_modified = datetime(2026, 7, 23, 12, 0, tzinfo=timezone.utc)
        for board in BOARDS:
            for directory, suffix, size in (
                ("images", ".img.gz", 100),
                ("bundles", ".raucb", 300),
            ):
                source = f"os/{directory}/snapdog-os-{board}-{version}{suffix}"
                alias = f"os/{directory}/snapdog-os-{board}-{channel}{suffix}"
                self.heads[source] = {
                    "ContentLength": size,
                    "ETag": f'"source-{board}-{directory}"',
                    "Metadata": {},
                }
                self.heads[alias] = {
                    "ContentLength": size,
                    "ETag": '"copy-etag-may-differ"',
                    "CacheControl": "no-store, max-age=0",
                    "Metadata": {"snapdog-source-key": source},
                }

    def get_object(self, *, Bucket: str, Key: str) -> dict[str, Any]:
        del Bucket
        if Key not in self.json_objects:
            raise MissingObject()
        return {
            "Body": FakeBody(json.dumps(self.json_objects[Key]).encode()),
            "LastModified": self.pointer_modified,
        }

    def head_object(self, *, Bucket: str, Key: str) -> dict[str, Any]:
        del Bucket
        if Key not in self.heads:
            raise MissingObject()
        return self.heads[Key]

    def copy_object(self, **kwargs: Any) -> None:
        self.copy_calls.append(kwargs)
        source_key = kwargs["CopySource"]["Key"]
        source = self.heads[source_key]
        self.heads[kwargs["Key"]] = {
            "ContentLength": source["ContentLength"],
            "ETag": '"new-copy-etag"',
            "CacheControl": kwargs["CacheControl"],
            "Metadata": kwargs["Metadata"],
        }


class R2AliasRepairTests(unittest.TestCase):
    def setUp(self) -> None:
        self.output = io.StringIO()
        self.redirect = redirect_stdout(self.output)
        self.redirect.__enter__()
        self.addCleanup(self.redirect.__exit__, None, None, None)

    def test_verified_aliases_need_no_copy(self) -> None:
        client = FakeS3(manifest())

        self.assertEqual(
            repair_channel(client, "bucket", "release", apply=True),
            0,
        )
        self.assertEqual(client.copy_calls, [])

    def test_dry_run_then_apply_repairs_transition_alias(self) -> None:
        current = manifest()
        client = FakeS3(current)
        alias = "os/images/snapdog-os-pi4-release.img.gz"
        client.heads[alias] = {
            "ContentLength": 70,
            "ETag": '"transition"',
            "CacheControl": "no-store, max-age=0",
            "Metadata": {},
        }

        self.assertEqual(repair_channel(client, "bucket", "release", apply=False), 1)
        self.assertEqual(client.copy_calls, [])
        self.assertEqual(
            repair_channel(
                client,
                "bucket",
                "release",
                apply=True,
                clock=lambda: client.pointer_modified.timestamp() + 70,
            ),
            1,
        )
        self.assertEqual(len(client.copy_calls), 1)
        self.assertEqual(
            client.copy_calls[0]["Metadata"]["snapdog-source-key"],
            "os/images/snapdog-os-pi4-1.2.3.img.gz",
        )
        self.assertEqual(repair_channel(client, "bucket", "release", apply=True), 0)

    def test_fresh_pointer_waits_out_cache_before_repair(self) -> None:
        client = FakeS3(manifest())
        alias = "os/bundles/snapdog-os-pi5-release.raucb"
        client.heads[alias] = {
            "ContentLength": 70,
            "ETag": '"transition"',
            "CacheControl": "no-store, max-age=0",
            "Metadata": {},
        }
        sleeps: list[float] = []

        repaired = repair_channel(
            client,
            "bucket",
            "release",
            apply=True,
            clock=lambda: client.pointer_modified.timestamp() + 10,
            sleep=sleeps.append,
        )

        self.assertEqual(repaired, 1)
        self.assertEqual(sleeps, [60])
        self.assertEqual(len(client.copy_calls), 1)

    def test_legacy_alias_with_exact_etag_is_accepted(self) -> None:
        client = FakeS3(manifest())
        source = "os/bundles/snapdog-os-pi3-1.2.3.raucb"
        alias = "os/bundles/snapdog-os-pi3-release.raucb"
        client.heads[alias] = {
            "ContentLength": client.heads[source]["ContentLength"],
            "ETag": client.heads[source]["ETag"],
            "CacheControl": "no-store",
            "Metadata": {},
        }

        self.assertEqual(repair_channel(client, "bucket", "release", apply=False), 0)

    def test_committed_manifest_must_exactly_match_pointer(self) -> None:
        current = manifest()
        client = FakeS3(current)
        committed_key = "os/images/manifests/release/1.2.3.json"
        client.json_objects[committed_key] = {**current, "commit": "d" * 40}

        with self.assertRaisesRegex(ValueError, "does not exactly match"):
            repair_channel(client, "bucket", "release", apply=True)
        self.assertEqual(client.copy_calls, [])

    def test_missing_optional_beta_pointer_is_a_noop(self) -> None:
        client = FakeS3(manifest())

        self.assertEqual(repair_channel(client, "bucket", "beta", apply=True), 0)

    def test_manifest_image_size_mismatch_fails_closed(self) -> None:
        client = FakeS3(manifest())
        client.heads["os/images/snapdog-os-pi5-1.2.3.img.gz"][
            "ContentLength"
        ] = 99

        with self.assertRaisesRegex(ValueError, "manifest 100"):
            repair_channel(client, "bucket", "release", apply=True)


if __name__ == "__main__":
    unittest.main()
