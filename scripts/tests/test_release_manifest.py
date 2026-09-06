from __future__ import annotations

import gzip
import hashlib
import json
import sys
import tempfile
import unittest
from copy import deepcopy
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

from release_manifest import (  # noqa: E402
    BOARDS,
    ManifestError,
    create_board_metadata,
    create_catalog,
    create_manifest,
    main,
    validate_catalog,
    validate_manifest,
)

RELEASE_VERSION = "1.2.3"
BETA_VERSION = "1.2.4-beta.45"
COMMIT = "0123456789abcdef0123456789abcdef01234567"
DATE = "2026-07-19T12:34:56Z"
BASE_URL = "https://updates.snapdog.cc/os/images"
BUNDLE_BASE_URL = "https://updates.snapdog.cc/os/bundles"


class ReleaseManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def _write_image_pair(
        self,
        board: str,
        payload: bytes | None = None,
        *,
        version: str = RELEASE_VERSION,
    ) -> tuple[Path, Path]:
        board_directory = self.root / board
        board_directory.mkdir(parents=True, exist_ok=True)
        raw_image = board_directory / f"snapdog-os-{board}-{version}.img"
        compressed_image = board_directory / f"snapdog-os-{board}-{version}.img.gz"
        raw_payload = (
            payload if payload is not None else (f"{board}-image\n".encode() * 128)
        )
        raw_image.write_bytes(raw_payload)
        with compressed_image.open("wb") as output:
            with gzip.GzipFile(
                fileobj=output, mode="wb", filename="", mtime=0
            ) as archive:
                archive.write(raw_payload)
        return raw_image, compressed_image

    def _metadata_paths(self, *, version: str = RELEASE_VERSION) -> list[Path]:
        paths = []
        for board in BOARDS:
            raw_image, compressed_image = self._write_image_pair(
                board, version=version
            )
            metadata = create_board_metadata(
                board=board,
                version=version,
                raw_image=raw_image,
                compressed_image=compressed_image,
            )
            metadata_path = compressed_image.parent / f"{board}.metadata.json"
            metadata_path.write_text(json.dumps(metadata), encoding="utf-8")
            paths.append(metadata_path)
        return paths

    def _manifest(
        self, channel: str = "release", *, version: str = RELEASE_VERSION
    ) -> dict[str, object]:
        return create_manifest(
            channel=channel,
            version=version,
            commit=COMMIT,
            date=DATE,
            base_url=BASE_URL,
            bundle_base_url=BUNDLE_BASE_URL,
            metadata_paths=self._metadata_paths(version=version),
        )

    def test_board_metadata_describes_and_verifies_both_representations(self) -> None:
        payload = b"raw image payload" * 32
        raw_image, compressed_image = self._write_image_pair("pi4", payload)

        metadata = create_board_metadata(
            board="pi4",
            version=RELEASE_VERSION,
            raw_image=raw_image,
            compressed_image=compressed_image,
        )

        self.assertEqual(metadata["board"], "pi4")
        self.assertEqual(metadata["image"], compressed_image.name)
        self.assertEqual(metadata["uncompressed_size"], len(payload))
        self.assertEqual(metadata["compressed_size"], compressed_image.stat().st_size)
        self.assertEqual(
            metadata["sha256"],
            hashlib.sha256(compressed_image.read_bytes()).hexdigest(),
        )
        self.assertEqual(metadata["raw_sha256"], hashlib.sha256(payload).hexdigest())
        self.assertNotEqual(metadata["sha256"], metadata["raw_sha256"])

    def test_board_metadata_rejects_a_gzip_with_different_payload(self) -> None:
        raw_image, compressed_image = self._write_image_pair("pi4", b"expected")
        with compressed_image.open("wb") as output:
            with gzip.GzipFile(
                fileobj=output, mode="wb", filename="", mtime=0
            ) as archive:
                archive.write(b"different")

        with self.assertRaisesRegex(ManifestError, "does not expand"):
            create_board_metadata(
                board="pi4",
                version=RELEASE_VERSION,
                raw_image=raw_image,
                compressed_image=compressed_image,
            )

    def test_board_metadata_rejects_a_corrupt_gzip(self) -> None:
        raw_image, compressed_image = self._write_image_pair("pi4", b"expected")
        compressed_image.write_bytes(b"not a gzip archive")

        with self.assertRaisesRegex(ManifestError, "cannot decompress"):
            create_board_metadata(
                board="pi4",
                version=RELEASE_VERSION,
                raw_image=raw_image,
                compressed_image=compressed_image,
            )

    def test_v2_manifest_keeps_v1_fields_and_adds_immutable_metadata(self) -> None:
        manifest = self._manifest()

        self.assertEqual(manifest["schema_version"], 2)
        self.assertEqual(manifest["channel"], "release")
        self.assertEqual(list(manifest["boards"]), list(BOARDS))
        for board, entry in manifest["boards"].items():
            self.assertEqual(entry["image"], f"snapdog-os-{board}-release.img.gz")
            self.assertRegex(entry["sha256"], r"^[0-9a-f]{64}$")
            self.assertEqual(
                entry["url"],
                f"{BASE_URL}/snapdog-os-{board}-{RELEASE_VERSION}.img.gz",
            )
            self.assertEqual(
                entry["bundle_url"],
                f"{BUNDLE_BASE_URL}/snapdog-os-{board}-{RELEASE_VERSION}.raucb",
            )
            self.assertGreater(entry["compressed_size"], 0)
            self.assertGreater(entry["uncompressed_size"], 0)
            self.assertRegex(entry["raw_sha256"], r"^[0-9a-f]{64}$")

    def test_beta_manifest_allows_mirrored_stable_release(self) -> None:
        release = self._manifest("release")
        beta = self._manifest("beta")

        validate_manifest(beta)
        for board in BOARDS:
            self.assertEqual(
                beta["boards"][board]["url"],
                release["boards"][board]["url"],
            )
            self.assertEqual(
                beta["boards"][board]["bundle_url"],
                release["boards"][board]["bundle_url"],
            )

    def test_release_manifest_rejects_prerelease_and_build_versions(self) -> None:
        for version in (BETA_VERSION, "1.2.3+build.1"):
            with self.subTest(version=version):
                with self.assertRaisesRegex(
                    ManifestError, "release channel requires stable X.Y.Z"
                ):
                    self._manifest("release", version=version)

    def test_beta_manifest_accepts_only_beta_prerelease_or_stable_mirror(self) -> None:
        beta = self._manifest("beta", version=BETA_VERSION)

        validate_manifest(beta)
        self.assertEqual(beta["version"], BETA_VERSION)
        for board in BOARDS:
            self.assertEqual(
                beta["boards"][board]["url"],
                f"{BASE_URL}/snapdog-os-{board}-{BETA_VERSION}.img.gz",
            )

        for version in ("1.2.4-rc.1", "1.2.4-beta.01", "1.2.4+build.1"):
            with self.subTest(version=version):
                with self.assertRaisesRegex(
                    ManifestError, "beta channel requires X.Y.Z-beta.N"
                ):
                    self._manifest("beta", version=version)

    def test_validator_enforces_the_channel_version_contract(self) -> None:
        manifest = self._manifest("release")
        manifest["version"] = BETA_VERSION

        with self.assertRaisesRegex(
            ManifestError, "release channel requires stable X.Y.Z"
        ):
            validate_manifest(manifest)

    def test_manifest_rejects_missing_or_duplicate_board_metadata(self) -> None:
        metadata_paths = self._metadata_paths()
        common = {
            "channel": "release",
            "version": RELEASE_VERSION,
            "commit": COMMIT,
            "date": DATE,
            "base_url": BASE_URL,
            "bundle_base_url": BUNDLE_BASE_URL,
        }
        with self.assertRaisesRegex(ManifestError, "cover exactly"):
            create_manifest(metadata_paths=metadata_paths[:-1], **common)
        with self.assertRaisesRegex(ManifestError, "duplicate metadata"):
            create_manifest(
                metadata_paths=[*metadata_paths, metadata_paths[0]], **common
            )

    def test_manifest_rejects_tampered_compressed_artifact(self) -> None:
        metadata_paths = self._metadata_paths()
        metadata = json.loads(metadata_paths[0].read_text(encoding="utf-8"))
        (metadata_paths[0].parent / metadata["image"]).write_bytes(b"tampered")

        with self.assertRaisesRegex(ManifestError, "SHA-256 mismatch"):
            create_manifest(
                channel="release",
                version=RELEASE_VERSION,
                commit=COMMIT,
                date=DATE,
                base_url=BASE_URL,
                bundle_base_url=BUNDLE_BASE_URL,
                metadata_paths=metadata_paths,
            )

    def test_manifest_rejects_channel_alias_as_the_immutable_url(self) -> None:
        manifest = self._manifest()
        manifest["boards"]["pi4"]["url"] = (
            "https://updates.snapdog.cc/os/images/snapdog-os-pi4-release.img.gz"
        )

        with self.assertRaisesRegex(ManifestError, "immutable image"):
            validate_manifest(manifest)

    def test_manifest_requires_https_urls_and_timezone_aware_dates(self) -> None:
        manifest = self._manifest()
        manifest["boards"]["pi3"]["url"] = manifest["boards"]["pi3"]["url"].replace(
            "https://", "http://"
        )
        with self.assertRaisesRegex(ManifestError, "must use HTTPS"):
            validate_manifest(manifest)

        manifest = self._manifest()
        manifest["boards"]["pi3"]["bundle_url"] = manifest["boards"]["pi3"][
            "bundle_url"
        ].replace("https://", "http://")
        with self.assertRaisesRegex(ManifestError, "bundle_url must use HTTPS"):
            validate_manifest(manifest)

        manifest = self._manifest()
        manifest["date"] = "2026-07-19T12:34:56"
        with self.assertRaisesRegex(ManifestError, "include a timezone"):
            validate_manifest(manifest)

    def test_validator_allows_future_additive_fields(self) -> None:
        manifest = self._manifest()
        manifest["future_top_level_field"] = {"enabled": True}
        manifest["boards"]["pi5"]["future_board_field"] = "value"

        validate_manifest(manifest)

    def test_validator_accepts_legacy_v2_without_bundle_url(self) -> None:
        manifest = self._manifest()
        for entry in manifest["boards"].values():
            del entry["bundle_url"]

        validate_manifest(manifest)

    def test_manifest_rejects_rolling_bundle_alias(self) -> None:
        manifest = self._manifest()
        manifest["boards"]["pi4"]["bundle_url"] = (
            f"{BUNDLE_BASE_URL}/snapdog-os-pi4-release.raucb"
        )

        with self.assertRaisesRegex(ManifestError, "immutable bundle"):
            validate_manifest(manifest)

        manifest = self._manifest()
        manifest["boards"]["pi4"]["bundle_url"] = None
        with self.assertRaisesRegex(ManifestError, "bundle_url must be a string"):
            validate_manifest(manifest)

    def test_manifest_rejects_foreign_or_wrong_path_bundle_urls(self) -> None:
        for url in (
            "https://evil.invalid/os/bundles/snapdog-os-pi4-1.2.3.raucb",
            "https://updates.snapdog.cc/wrong/snapdog-os-pi4-1.2.3.raucb",
        ):
            with self.subTest(url=url):
                manifest = self._manifest()
                manifest["boards"]["pi4"]["bundle_url"] = url
                with self.assertRaisesRegex(ManifestError, "canonical URL"):
                    validate_manifest(manifest)

    def test_generator_requires_canonical_bundle_origin(self) -> None:
        with self.assertRaisesRegex(ManifestError, "bundle base URL"):
            create_manifest(
                channel="release",
                version=RELEASE_VERSION,
                commit=COMMIT,
                date=DATE,
                base_url=BASE_URL,
                bundle_base_url="https://evil.invalid/os/bundles",
                metadata_paths=self._metadata_paths(),
            )

    def test_v1_manifest_is_not_misidentified_as_v2(self) -> None:
        manifest = self._manifest()
        del manifest["schema_version"]
        for entry in manifest["boards"].values():
            for key in ("url", "compressed_size", "uncompressed_size", "raw_sha256"):
                del entry[key]

        with self.assertRaisesRegex(ManifestError, "schema_version"):
            validate_manifest(manifest)

    def test_command_line_round_trip_writes_and_validates_manifest(self) -> None:
        metadata_paths = self._metadata_paths()
        output = self.root / "latest-release.json"
        result = main(
            [
                "manifest",
                "--channel",
                "release",
                "--version",
                RELEASE_VERSION,
                "--commit",
                COMMIT,
                "--date",
                DATE,
                "--base-url",
                BASE_URL,
                "--bundle-base-url",
                BUNDLE_BASE_URL,
                "--metadata",
                *(str(path) for path in metadata_paths),
                "--output",
                str(output),
            ]
        )

        self.assertEqual(result, 0)
        self.assertEqual(main(["validate", "--manifest", str(output)]), 0)
        self.assertEqual(
            json.loads(output.read_text(encoding="utf-8"))["schema_version"], 2
        )

    def test_catalog_is_newest_first_and_replaces_matching_version(self) -> None:
        current = self._manifest()
        older = deepcopy(current)
        older["version"] = "1.2.2"
        older["date"] = "2026-07-18T12:34:56Z"
        for board, entry in older["boards"].items():
            entry["url"] = f"{BASE_URL}/snapdog-os-{board}-1.2.2.img.gz"
            entry["bundle_url"] = (
                f"{BUNDLE_BASE_URL}/snapdog-os-{board}-1.2.2.raucb"
            )

        catalog = create_catalog(channel="release", manifest=older)
        catalog = create_catalog(
            channel="release", manifest=current, previous=catalog
        )
        self.assertEqual(
            [release["version"] for release in catalog["releases"]],
            [RELEASE_VERSION, "1.2.2"],
        )

        replacement = deepcopy(current)
        replacement["date"] = "2026-07-20T12:34:56Z"
        catalog = create_catalog(
            channel="release", manifest=replacement, previous=catalog
        )
        self.assertEqual(len(catalog["releases"]), 2)
        self.assertEqual(catalog["releases"][0]["date"], replacement["date"])
        validate_catalog(catalog)

    def test_catalog_rejects_wrong_channel_duplicates_and_order(self) -> None:
        manifest = self._manifest()
        catalog = create_catalog(channel="release", manifest=manifest)

        wrong_channel = deepcopy(catalog)
        wrong_channel["channel"] = "beta"
        with self.assertRaisesRegex(ManifestError, "channel"):
            validate_catalog(wrong_channel)

        duplicate = deepcopy(catalog)
        duplicate["releases"].append(deepcopy(manifest))
        with self.assertRaisesRegex(ManifestError, "duplicate"):
            validate_catalog(duplicate)

        newer = deepcopy(manifest)
        newer["version"] = "1.2.4"
        for board, entry in newer["boards"].items():
            entry["url"] = f"{BASE_URL}/snapdog-os-{board}-1.2.4.img.gz"
            entry["bundle_url"] = (
                f"{BUNDLE_BASE_URL}/snapdog-os-{board}-1.2.4.raucb"
            )
        unsorted = deepcopy(catalog)
        unsorted["releases"].append(newer)
        with self.assertRaisesRegex(ManifestError, "descending SemVer"):
            validate_catalog(unsorted)

    def test_catalog_command_line_round_trip(self) -> None:
        manifest_path = self.root / "latest-release.json"
        manifest_path.write_text(json.dumps(self._manifest()), encoding="utf-8")
        catalog_path = self.root / "catalog-release.json"

        self.assertEqual(
            main(
                [
                    "catalog",
                    "--channel",
                    "release",
                    "--manifest",
                    str(manifest_path),
                    "--output",
                    str(catalog_path),
                ]
            ),
            0,
        )
        self.assertEqual(
            main(["validate-catalog", "--catalog", str(catalog_path)]), 0
        )


if __name__ == "__main__":
    unittest.main()
