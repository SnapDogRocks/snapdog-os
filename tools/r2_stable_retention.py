#!/usr/bin/env python3
"""Two-phase, fail-closed retention for immutable stable OS artifacts in R2."""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
sys.path.insert(0, str(ROOT / "tools"))

from release_manifest import BOARDS, validate_catalog, validate_manifest  # noqa: E402
from r2_maintenance import (  # noqa: E402
    LEGACY_BETA_MANIFEST_MAX_AGE_SECONDS,
    SEMVER_RE,
    canonical_artifact_stem,
    delete_keys,
    env,
    incomplete_version_artifacts,
    latest_release,
    list_objects,
    make_client,
    manifest_cache_seconds,
    optional_json_object_state,
    semver_tuple,
    validate_committed_manifest,
)

CATALOG_KEY = "os/images/catalog-release.json"
MANIFEST_PREFIX = "os/images/manifests/release/"
MARKER_PREFIX = "os/.retention/release/"
DEFAULT_POLICY = ROOT / "config/r2-retention.json"
DEFAULT_DOWNLOAD_GRACE_SECONDS = 86_400


def load_policy(path: Path) -> tuple[int, set[str]]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read retention policy {path}: {error}") from error
    if set(value) != {"stable_releases", "pinned_versions"}:
        raise ValueError("retention policy has unexpected or missing fields")
    count = value["stable_releases"]
    pins = value["pinned_versions"]
    if not isinstance(count, int) or isinstance(count, bool) or count < 1:
        raise ValueError("stable_releases must be a positive integer")
    if not isinstance(pins, list) or any(
        not isinstance(version, str) or SEMVER_RE.fullmatch(version) is None
        for version in pins
    ):
        raise ValueError("pinned_versions must contain canonical stable versions")
    if len(set(pins)) != len(pins):
        raise ValueError("pinned_versions contains duplicates")
    return count, set(pins)


def manifest_version(key: str) -> str | None:
    if not key.startswith(MANIFEST_PREFIX) or not key.endswith(".json"):
        return None
    version = key[len(MANIFEST_PREFIX) : -5]
    return version if "/" not in version and SEMVER_RE.fullmatch(version) else None


def artifact_version(key: str) -> str | None:
    version = manifest_version(key)
    if version is not None:
        return version
    stem = canonical_artifact_stem(key)
    if stem is None:
        return None
    for board in BOARDS:
        prefix = f"snapdog-os-{board}-"
        if stem.startswith(prefix):
            candidate = stem[len(prefix) :]
            return candidate if SEMVER_RE.fullmatch(candidate) else None
    return None


def marker_key(version: str) -> str:
    if SEMVER_RE.fullmatch(version) is None:
        raise ValueError(f"invalid stable version: {version!r}")
    return f"{MARKER_PREFIX}{version}.json"


def marker_version(key: str) -> str | None:
    if not key.startswith(MARKER_PREFIX) or not key.endswith(".json"):
        return None
    version = key[len(MARKER_PREFIX) : -5]
    return version if "/" not in version and SEMVER_RE.fullmatch(version) else None


def new_marker(version: str, cache_seconds: int, grace_seconds: int) -> dict[str, Any]:
    now = datetime.now(timezone.utc)
    return {
        "schema_version": 1,
        "version": version,
        "manifest_retired_at": now.isoformat(),
        "manifest_max_age_seconds": cache_seconds,
        "download_grace_seconds": grace_seconds,
        "payload_not_before_epoch": int(now.timestamp() + 0.999999)
        + cache_seconds
        + grace_seconds,
    }


def validate_marker(value: dict[str, Any], version: str) -> None:
    expected = {
        "schema_version", "version", "manifest_retired_at",
        "manifest_max_age_seconds", "download_grace_seconds",
        "payload_not_before_epoch",
    }
    if set(value) != expected or value.get("schema_version") != 1 or value.get("version") != version:
        raise ValueError(f"invalid retirement marker identity for {version}")
    try:
        retired = datetime.fromisoformat(str(value["manifest_retired_at"]).replace("Z", "+00:00"))
    except ValueError as error:
        raise ValueError(f"invalid retirement time for {version}") from error
    cache = value["manifest_max_age_seconds"]
    grace = value["download_grace_seconds"]
    deadline = value["payload_not_before_epoch"]
    if retired.tzinfo is None or any(
        not isinstance(item, int) or isinstance(item, bool) or item < 0
        for item in (cache, grace, deadline)
    ):
        raise ValueError(f"invalid retirement timing for {version}")
    if deadline != int(retired.timestamp() + 0.999999) + cache + grace:
        raise ValueError(f"inconsistent retirement deadline for {version}")


def publish_json(s3, bucket: str, key: str, value: dict[str, Any], cache: str) -> None:
    s3.put_object(
        Bucket=bucket,
        Key=key,
        Body=(json.dumps(value, indent=2) + "\n").encode(),
        CacheControl=cache,
        ContentType="application/json",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Retain the newest stable R2 releases")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--bucket", default=env("R2_BUCKET") or "snapdog-updates")
    parser.add_argument("--endpoint", default=env("AWS_ENDPOINT_URL", "R2_ENDPOINT_URL"))
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--download-grace-seconds", type=int, default=DEFAULT_DOWNLOAD_GRACE_SECONDS)
    args = parser.parse_args()
    if not args.endpoint or args.download_grace_seconds < 0:
        print("ERROR: endpoint missing or grace is negative", file=sys.stderr)
        return 2

    keep_count, pins = load_policy(args.policy)
    s3 = make_client(args.endpoint)
    latest, _ = latest_release(s3, args.bucket)
    objects = list_objects(s3, args.bucket)
    validate_committed_manifest(s3, args.bucket, "release", latest)

    catalog_state = optional_json_object_state(s3, args.bucket, CATALOG_KEY)
    if catalog_state is None:
        raise ValueError("catalog-release.json is missing")
    catalog, _ = catalog_state
    validate_catalog(catalog)
    if catalog.get("channel") != "release":
        raise ValueError("catalog-release.json is not a release catalog")

    versions = {version for key in objects if (version := artifact_version(key))}
    if latest["version"] not in versions:
        raise ValueError("latest stable version is absent from versioned objects")
    unknown_pins = pins - versions
    if unknown_pins:
        raise ValueError("pinned versions are absent: " + ", ".join(sorted(unknown_pins)))
    newest = set(sorted(versions, key=semver_tuple, reverse=True)[:keep_count])
    retained = newest | pins | {latest["version"]}

    marker_versions = {version for key in objects if (version := marker_version(key))}
    revived = marker_versions & retained
    if revived:
        raise ValueError("retained versions already have retirement markers: " + ", ".join(sorted(revived)))

    filtered = {**catalog, "releases": [entry for entry in catalog["releases"] if entry["version"] in retained]}
    validate_catalog(filtered)
    if latest["version"] not in {entry["version"] for entry in filtered["releases"]}:
        raise ValueError("filtered catalog would omit latest-release.json")
    catalog_by_version = {entry["version"]: entry for entry in catalog["releases"]}
    for version in retained:
        manifest_state = optional_json_object_state(
            s3, args.bucket, f"{MANIFEST_PREFIX}{version}.json"
        )
        if manifest_state is None:
            raise ValueError(f"retained release manifest is missing: {version}")
        manifest = manifest_state[0]
        validate_manifest(manifest)
        if manifest.get("channel") != "release" or manifest.get("version") != version:
            raise ValueError(f"retained release manifest has wrong identity: {version}")
        if version in catalog_by_version and catalog_by_version[version] != manifest:
            raise ValueError(f"release catalog disagrees with manifest: {version}")
        missing = incomplete_version_artifacts(objects, "release", manifest)
        if missing:
            raise ValueError(f"retained release {version} is incomplete: {missing}")

    candidates = versions - retained
    grouped: dict[str, list[str]] = defaultdict(list)
    for key in objects:
        version = artifact_version(key)
        if version in candidates:
            grouped[version].append(key)
    for version in marker_versions:
        grouped[version]

    bytes_pending = sum(objects.get(key, 0) for keys in grouped.values() for key in keys if manifest_version(key) is None)
    print(f"bucket={args.bucket} stable_versions={len(versions)} keep={','.join(sorted(retained, key=semver_tuple))}")
    print(f"retire={len(grouped)} versions payload={bytes_pending / 1e9:.2f} GB")
    if not args.apply:
        print("[DRY RUN] no R2 objects changed")
        return 0

    if filtered != catalog:
        publish_json(s3, args.bucket, CATALOG_KEY, filtered, "public, max-age=60")

    now = datetime.now(timezone.utc).timestamp()
    deleted = 0
    for version in sorted(grouped, key=semver_tuple):
        manifest = f"{MANIFEST_PREFIX}{version}.json"
        marker_state = optional_json_object_state(s3, args.bucket, marker_key(version))
        marker = None if marker_state is None else marker_state[0]
        if marker is not None:
            validate_marker(marker, version)
        if manifest in objects:
            if marker is not None:
                raise ValueError(f"manifest and retirement marker coexist for {version}")
            response = s3.get_object(Bucket=args.bucket, Key=manifest)
            cache = manifest_cache_seconds(response.get("CacheControl"))
            s3.delete_object(Bucket=args.bucket, Key=manifest)
            if optional_json_object_state(s3, args.bucket, manifest) is not None:
                raise RuntimeError(f"failed to retire {manifest}")
            marker = new_marker(version, cache, args.download_grace_seconds)
            publish_json(s3, args.bucket, marker_key(version), marker, "no-store")
            print(f"retired {version}; payload deletion after epoch {marker['payload_not_before_epoch']}")
            continue
        if marker is None:
            marker = new_marker(version, LEGACY_BETA_MANIFEST_MAX_AGE_SECONDS, args.download_grace_seconds)
            publish_json(s3, args.bucket, marker_key(version), marker, "no-store")
            print(f"started conservative retirement for manifest-less {version}")
            continue
        if now < marker["payload_not_before_epoch"]:
            continue
        payloads = [key for key in grouped[version] if manifest_version(key) is None]
        delete_keys(s3, args.bucket, sorted(payloads))
        s3.delete_object(Bucket=args.bucket, Key=marker_key(version))
        deleted += len(payloads)
    print(f"deleted={deleted} payload objects")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
