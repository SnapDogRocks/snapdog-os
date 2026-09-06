#!/usr/bin/env python3
"""R2 retention for the snapdog-os update bucket (updates.snapdog.cc).

Keeps the `snapdog-updates` bucket tidy by removing OS artifacts that are no
longer useful, applying a self-cleaning retention policy:

  * Release versions (``X.Y.Z``)     -> kept forever.
  * Beta versions (``X.Y.Z-beta.N``) -> kept only while the release is still
    unreleased, i.e. iff ``base_semver(beta) > latest_release``. When version X
    ships, every ``X-beta.*`` becomes ``<= X`` and ages out automatically.
  * Channel aliases (``…-release`` / ``…-beta``) and channel manifests
    (``latest-*.json``)             -> never touched (they are the pointers).
  * Beta catalog + version manifests -> superseded entries/manifests are removed
    before their payloads. A durable retirement marker then waits out the
    manifest's advertised cache lifetime plus the active-download grace.
  * Legacy / non-conforming keys (e.g. board-only ``snapdog-os-pi4.raucb``)
                                     -> removed.

Applies across ``os/bundles/``, ``os/images/`` and ``os/sbom/`` so a pruned beta
takes its whole footprint with it.

Runs dry by default; pass ``--apply`` to delete. Reads credentials from
``AWS_ACCESS_KEY_ID`` / ``AWS_SECRET_ACCESS_KEY`` / ``AWS_ENDPOINT_URL`` (the
names the Publish job already exports), falling back to the ``R2_*`` equivalents
for local runs. Bucket defaults to ``$R2_BUCKET`` or ``snapdog-updates``.

    # CI (creds already in env):     python tools/r2_maintenance.py --apply
    # Local dry-run:                 R2_ENDPOINT_URL=… uv run --with boto3 \
    #                                  python tools/r2_maintenance.py
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

from release_manifest import (  # noqa: E402
    BOARDS,
    create_catalog,
    validate_catalog,
    validate_manifest,
)

PREFIXES = ("os/bundles/", "os/images/", "os/sbom/", "os/.retention/")
ARTIFACT_SUFFIX_BY_PREFIX = {
    "os/bundles/": ".raucb",
    "os/images/": ".img.gz",
    "os/sbom/": "-sbom.csv",
}
CATALOG_KEY = "os/images/catalog-beta.json"
BETA_LATEST_KEY = "os/images/latest-beta.json"
BETA_MANIFEST_PREFIX = "os/images/manifests/beta/"
RETIREMENT_PREFIX = "os/.retention/beta/"
# snapdog-os-<board>-<rest>  (board = a single lowercase-alnum token, e.g. pi4,
# zero2w); <rest> is a version, a "-beta.N" version, or a channel alias.
STEM_RE = re.compile(r"^snapdog-os-([a-z0-9]+)(?:-(.+))?$")
SEMVER_PATTERN = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
SEMVER_RE = re.compile(rf"^{SEMVER_PATTERN}$")
BETA_RE = re.compile(rf"^({SEMVER_PATTERN})-beta\.(?:0|[1-9][0-9]*)$")
DEFAULT_DELETE_GRACE_SECONDS = 86_400
LEGACY_BETA_MANIFEST_MAX_AGE_SECONDS = 31_536_000
MAX_AGE_RE = re.compile(r"(?:^|,)\s*max-age=(\d+)(?:\s*,|$)", re.IGNORECASE)


def env(*names: str) -> str | None:
    for n in names:
        v = os.environ.get(n)
        if v:
            return v
    return None


def semver_tuple(v: str) -> tuple[int, int, int]:
    return tuple(int(x) for x in v.split(".")[:3])  # type: ignore[return-value]


def canonical_artifact_stem(key: str) -> str | None:
    """Return a top-level artifact stem, never one from a backup subdirectory."""
    for prefix, suffix in ARTIFACT_SUFFIX_BY_PREFIX.items():
        if not key.startswith(prefix):
            continue
        name = key[len(prefix) :]
        if not name or "/" in name or not name.endswith(suffix):
            return None
        return name[: -len(suffix)]
    return None


def superseded_beta(version: str, release_t: tuple[int, int, int]) -> bool:
    """Whether ``version`` is a well-formed beta made obsolete by release_t."""
    match = BETA_RE.fullmatch(version)
    return bool(match and semver_tuple(match.group(1)) <= release_t)


def channel_version_key(version: str) -> tuple[int, int, int, int, int]:
    """Order the two OS channel forms: X.Y.Z-beta.N < X.Y.Z."""
    if SEMVER_RE.fullmatch(version):
        return (*semver_tuple(version), 1, 0)
    match = BETA_RE.fullmatch(version)
    if match:
        sequence = int(version.rsplit(".", 1)[1])
        return (*semver_tuple(match.group(1)), 0, sequence)
    raise ValueError(f"invalid OS channel version: {version!r}")


def beta_manifest_version(key: str) -> str | None:
    """Return a version for a versioned beta manifest key, if recognized."""
    if not key.startswith(BETA_MANIFEST_PREFIX) or not key.endswith(".json"):
        return None
    version = key[len(BETA_MANIFEST_PREFIX) : -len(".json")]
    if "/" in version:
        return None
    if SEMVER_RE.fullmatch(version) or BETA_RE.fullmatch(version):
        return version
    return None


def beta_artifact_version(key: str) -> str | None:
    """Return the exact prerelease version represented by a deletable key."""
    manifest_version = beta_manifest_version(key)
    if manifest_version is not None and BETA_RE.fullmatch(manifest_version):
        return manifest_version
    stem = canonical_artifact_stem(key)
    match = None if stem is None else STEM_RE.fullmatch(stem)
    if match is None or match.group(1) not in BOARDS:
        return None
    rest = match.group(2)
    return rest if rest is not None and BETA_RE.fullmatch(rest) else None


def retirement_marker_key(version: str) -> str:
    if BETA_RE.fullmatch(version) is None:
        raise ValueError(f"invalid beta retirement version: {version!r}")
    return f"{RETIREMENT_PREFIX}{version}.json"


def retirement_marker_version(key: str) -> str | None:
    """Return the prerelease version encoded by a canonical marker key."""
    if not key.startswith(RETIREMENT_PREFIX) or not key.endswith(".json"):
        return None
    version = key[len(RETIREMENT_PREFIX) : -len(".json")]
    if "/" in version or BETA_RE.fullmatch(version) is None:
        return None
    return version


def manifest_cache_seconds(cache_control: Any) -> int:
    """Read max-age conservatively; unknown legacy metadata means one year."""
    if isinstance(cache_control, str):
        match = MAX_AGE_RE.search(cache_control)
        if match is not None:
            return int(match.group(1))
    return LEGACY_BETA_MANIFEST_MAX_AGE_SECONDS


def new_retirement_marker(
    version: str,
    now: datetime,
    *,
    manifest_max_age_seconds: int,
    download_grace_seconds: int,
) -> dict[str, Any]:
    """Create the durable second-phase payload deletion barrier."""
    if now.tzinfo is None:
        raise ValueError("retirement time must be timezone-aware")
    if manifest_max_age_seconds < 0 or download_grace_seconds < 0:
        raise ValueError("retirement grace values must be non-negative")
    return {
        "schema_version": 1,
        "version": version,
        "manifest_retired_at": now.astimezone(timezone.utc).isoformat(),
        "manifest_max_age_seconds": manifest_max_age_seconds,
        "download_grace_seconds": download_grace_seconds,
        "payload_not_before_epoch": math.ceil(now.timestamp())
        + manifest_max_age_seconds
        + download_grace_seconds,
    }


def validate_retirement_marker(marker: dict[str, Any], version: str) -> None:
    expected_keys = {
        "schema_version",
        "version",
        "manifest_retired_at",
        "manifest_max_age_seconds",
        "download_grace_seconds",
        "payload_not_before_epoch",
    }
    if set(marker) != expected_keys:
        raise ValueError(f"retirement marker for {version} has unexpected fields")
    if marker.get("schema_version") != 1 or marker.get("version") != version:
        raise ValueError(f"retirement marker for {version} has invalid identity")
    retired_at = marker.get("manifest_retired_at")
    try:
        parsed = datetime.fromisoformat(str(retired_at).replace("Z", "+00:00"))
    except ValueError as error:
        raise ValueError(f"retirement marker for {version} has invalid time") from error
    if parsed.tzinfo is None:
        raise ValueError(f"retirement marker for {version} has a naive time")
    manifest_max_age = marker.get("manifest_max_age_seconds")
    download_grace = marker.get("download_grace_seconds")
    for name, value in (
        ("manifest cache age", manifest_max_age),
        ("download grace", download_grace),
    ):
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise ValueError(f"retirement marker for {version} has invalid {name}")
    deadline = marker.get("payload_not_before_epoch")
    if not isinstance(deadline, int) or isinstance(deadline, bool) or deadline < 0:
        raise ValueError(f"retirement marker for {version} has invalid deadline")
    expected_deadline = (
        math.ceil(parsed.timestamp()) + manifest_max_age + download_grace
    )
    if deadline != expected_deadline:
        raise ValueError(f"retirement marker for {version} has inconsistent deadline")


def classify(key: str, release_t: tuple[int, int, int]) -> tuple[str, str]:
    """Return (action, reason) where action is 'keep' or 'delete'."""
    manifest_version = beta_manifest_version(key)
    if manifest_version is not None:
        if superseded_beta(manifest_version, release_t):
            return ("delete", "superseded beta manifest")
        return ("keep", "beta version manifest")

    stem = canonical_artifact_stem(key)
    if stem is None:
        return ("keep", "manifest/other")  # latest-*.json, index.json, …
    m = STEM_RE.fullmatch(stem)
    if not m or m.group(1) not in BOARDS:
        return ("keep", "unrecognized (safe)")
    rest = m.group(2)
    if rest is None:
        return ("delete", "legacy board-only")
    if rest in ("release", "beta"):
        return ("keep", "channel alias")
    beta_match = BETA_RE.fullmatch(rest)
    if beta_match:
        if semver_tuple(beta_match.group(1)) > release_t:
            return ("keep", "beta for upcoming release")
        return ("delete", "superseded beta")
    if SEMVER_RE.fullmatch(rest):
        return ("keep", "release version")
    return ("keep", "unrecognized (safe)")


def filter_beta_catalog(
    catalog: dict[str, Any], release_t: tuple[int, int, int]
) -> tuple[dict[str, Any] | None, tuple[str, ...]]:
    """Remove catalog entries for beta payloads that retention will delete.

    ``None`` represents an absent catalog. The public catalog schema requires at
    least one release, so a catalog whose every entry is obsolete must be
    removed rather than replaced with an invalid empty document.
    """
    if catalog.get("channel") != "beta":
        raise ValueError("catalog-beta.json must declare the beta channel")
    validate_catalog(catalog)

    removed = tuple(
        release["version"]
        for release in catalog["releases"]
        if superseded_beta(release["version"], release_t)
    )
    if not removed:
        return catalog, ()

    kept = [
        release
        for release in catalog["releases"]
        if release["version"] not in removed
    ]
    if not kept:
        return None, removed

    filtered = {**catalog, "releases": kept}
    validate_catalog(filtered)
    return filtered, removed


def retain_beta_pointer(
    catalog: dict[str, Any] | None,
    latest: dict[str, Any] | None,
) -> dict[str, Any] | None:
    """Keep the public beta pointer represented in its release catalog.

    Besides keeping the two discovery surfaces consistent, retaining this entry
    gives catalog cleanup a persistent LastModified timestamp. A second
    retention run therefore cannot delete payloads immediately after an empty
    catalog was removed while clients may still hold its cached contents.
    """
    if latest is None:
        return catalog
    validate_manifest(latest)
    if latest.get("channel") != "beta":
        raise ValueError("latest-beta.json must declare the beta channel")
    if catalog is not None:
        validate_catalog(catalog)
        for release in catalog["releases"]:
            if release["version"] != latest["version"]:
                continue
            if release.get("commit") != latest.get("commit"):
                raise ValueError(
                    "catalog-beta.json and latest-beta.json disagree for "
                    f"version {latest['version']}"
                )
            break
    return create_catalog(channel="beta", manifest=latest, previous=catalog)


def required_version_artifacts(channel: str, version: str) -> set[str]:
    """Return immutable objects required by one committed manifest."""
    if channel not in {"release", "beta"}:
        raise ValueError(f"unsupported channel: {channel!r}")
    return {
        key
        for board in BOARDS
        for key in (
            f"os/images/snapdog-os-{board}-{version}.img.gz",
            f"os/bundles/snapdog-os-{board}-{version}.raucb",
            f"os/sbom/snapdog-os-{board}-{version}-sbom.csv",
        )
    } | {f"os/images/manifests/{channel}/{version}.json"}


def required_channel_artifacts(channel: str, version: str) -> set[str]:
    """Return immutable and compatibility objects required by a pointer."""
    aliases = {
        key
        for board in BOARDS
        for key in (
            f"os/images/snapdog-os-{board}-{channel}.img.gz",
            f"os/bundles/snapdog-os-{board}-{channel}.raucb",
        )
    }
    return required_version_artifacts(channel, version) | aliases


def beta_state_exists(objects: dict[str, int]) -> bool:
    """Whether bucket contents prove that the beta channel was initialized."""
    for key in objects:
        if key == CATALOG_KEY or beta_manifest_version(key) is not None:
            return True
        stem = canonical_artifact_stem(key)
        match = None if stem is None else STEM_RE.fullmatch(stem)
        if match is None or match.group(1) not in BOARDS:
            continue
        rest = match.group(2)
        if rest == "beta" or (rest is not None and BETA_RE.fullmatch(rest)):
            return True
    return False


def incomplete_version_artifacts(
    objects: dict[str, int], channel: str, manifest: dict[str, Any]
) -> list[str]:
    """Return immutable keys absent or inconsistent with a manifest."""
    validate_manifest(manifest)
    if manifest.get("channel") != channel:
        raise ValueError(f"manifest must declare the {channel} channel")
    version = manifest["version"]
    incomplete = {
        key
        for key in required_version_artifacts(channel, version)
        if objects.get(key, 0) <= 0
    }
    for board in BOARDS:
        expected_image_size = manifest["boards"][board]["compressed_size"]
        versioned_image = f"os/images/snapdog-os-{board}-{version}.img.gz"
        if (
            objects.get(versioned_image, 0) > 0
            and objects[versioned_image] != expected_image_size
        ):
            incomplete.add(
                f"{versioned_image} (size {objects[versioned_image]} "
                f"!= manifest {expected_image_size})"
            )
    return sorted(incomplete)


def incomplete_channel_artifacts(
    objects: dict[str, int], channel: str, manifest: dict[str, Any]
) -> list[str]:
    """Return immutable or compatibility keys inconsistent with a pointer."""
    version = manifest["version"]
    incomplete = set(incomplete_version_artifacts(objects, channel, manifest))
    for board in BOARDS:
        expected_image_size = manifest["boards"][board]["compressed_size"]
        image_alias = f"os/images/snapdog-os-{board}-{channel}.img.gz"
        if objects.get(image_alias, 0) <= 0:
            incomplete.add(image_alias)
        elif objects[image_alias] != expected_image_size:
            incomplete.add(
                f"{image_alias} (size {objects[image_alias]} "
                f"!= manifest {expected_image_size})"
            )
        versioned_bundle = f"os/bundles/snapdog-os-{board}-{version}.raucb"
        bundle_alias = f"os/bundles/snapdog-os-{board}-{channel}.raucb"
        if objects.get(bundle_alias, 0) <= 0:
            incomplete.add(bundle_alias)
        elif (
            objects.get(versioned_bundle, 0) > 0
            and objects[versioned_bundle] != objects[bundle_alias]
        ):
            incomplete.add(f"{bundle_alias} (size differs from {versioned_bundle})")
    return sorted(incomplete)


def _missing_object(error: Exception) -> bool:
    response = getattr(error, "response", None)
    if not isinstance(response, dict):
        return False
    details = response.get("Error", {})
    code = str(details.get("Code", "")) if isinstance(details, dict) else ""
    status = response.get("ResponseMetadata", {}).get("HTTPStatusCode")
    return code in {"404", "NoSuchKey", "NotFound"} or status == 404


def _optional_object_response(s3, bucket: str, key: str) -> dict[str, Any] | None:
    try:
        return s3.get_object(Bucket=bucket, Key=key)
    except Exception as error:
        if _missing_object(error):
            return None
        raise


def _json_from_response(response: dict[str, Any], key: str) -> dict[str, Any]:
    try:
        value = json.loads(response["Body"].read())
    except (TypeError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{key} is not valid JSON: {error}") from error
    except KeyError as error:
        raise ValueError(f"{key} response has no body") from error
    if not isinstance(value, dict):
        raise ValueError(f"{key} must contain a JSON object")
    return value


def optional_json_object(s3, bucket: str, key: str) -> dict[str, Any] | None:
    """Read an optional JSON object, swallowing only an explicit not-found."""
    response = _optional_object_response(s3, bucket, key)
    return None if response is None else _json_from_response(response, key)


def optional_json_object_state(
    s3, bucket: str, key: str
) -> tuple[dict[str, Any], datetime] | None:
    """Read optional JSON plus authoritative S3 modification time."""
    response = _optional_object_response(s3, bucket, key)
    if response is None:
        return None
    value = _json_from_response(response, key)
    modified = response.get("LastModified")
    if not isinstance(modified, datetime) or modified.tzinfo is None:
        raise ValueError(f"{key} has no timezone-aware LastModified")
    return value, modified


def validate_committed_manifest(
    s3,
    bucket: str,
    channel: str,
    pointer: dict[str, Any],
) -> None:
    """Require the immutable commit marker to equal its public pointer."""
    version = pointer["version"]
    key = f"os/images/manifests/{channel}/{version}.json"
    committed = optional_json_object(s3, bucket, key)
    if committed is None:
        raise ValueError(f"{key} is missing")
    validate_manifest(committed)
    if committed != pointer:
        raise ValueError(f"{key} does not exactly match latest-{channel}.json")


def validate_retained_beta_catalog(
    s3,
    bucket: str,
    catalog: dict[str, Any] | None,
    latest: dict[str, Any] | None,
    objects: dict[str, int],
) -> None:
    """Require every retained catalog entry to be committed and downloadable."""
    if catalog is None:
        if latest is not None:
            raise ValueError("latest-beta.json exists without catalog-beta.json")
        return
    validate_catalog(catalog)
    if latest is None:
        raise ValueError("catalog-beta.json exists without latest-beta.json")
    latest_key = channel_version_key(latest["version"])
    for release in catalog["releases"]:
        if channel_version_key(release["version"]) > latest_key:
            raise ValueError(
                "catalog-beta.json advertises a version newer than latest-beta.json: "
                f"{release['version']} > {latest['version']}"
            )
        validate_committed_manifest(s3, bucket, "beta", release)
        incomplete = incomplete_version_artifacts(objects, "beta", release)
        if incomplete:
            raise ValueError(
                f"catalog beta {release['version']} is incomplete: "
                + ", ".join(incomplete)
            )


def retirement_marker(
    s3, bucket: str, version: str
) -> dict[str, Any] | None:
    marker = optional_json_object(s3, bucket, retirement_marker_key(version))
    if marker is not None:
        validate_retirement_marker(marker, version)
    return marker


def publish_retirement_marker(
    s3, bucket: str, marker: dict[str, Any]
) -> None:
    version = str(marker.get("version", ""))
    validate_retirement_marker(marker, version)
    payload = json.dumps(marker, indent=2, sort_keys=True) + "\n"
    s3.put_object(
        Bucket=bucket,
        Key=retirement_marker_key(version),
        Body=payload.encode("utf-8"),
        CacheControl="no-store",
        ContentType="application/json",
        IfNoneMatch="*",
    )


def delete_keys(s3, bucket: str, keys: list[str]) -> None:
    """Delete a complete phase or fail before a later phase can start."""
    for index in range(0, len(keys), 1000):
        batch = [{"Key": key} for key in keys[index : index + 1000]]
        response = s3.delete_objects(
            Bucket=bucket, Delete={"Objects": batch, "Quiet": True}
        )
        errors = response.get("Errors", [])
        if errors:
            raise RuntimeError(f"R2 deletion failed: {errors}")


def publish_beta_catalog(
    s3, bucket: str, catalog: dict[str, Any] | None
) -> None:
    """Publish a valid filtered beta catalog, or remove an empty one."""
    if catalog is None:
        s3.delete_object(Bucket=bucket, Key=CATALOG_KEY)
        return
    if catalog.get("channel") != "beta":
        raise ValueError("refusing to publish a non-beta catalog as catalog-beta.json")
    validate_catalog(catalog)
    payload = json.dumps(catalog, indent=2, sort_keys=False) + "\n"
    s3.put_object(
        Bucket=bucket,
        Key=CATALOG_KEY,
        Body=payload.encode("utf-8"),
        CacheControl="public, max-age=60",
        ContentType="application/json",
    )


def make_client(endpoint: str):
    import boto3

    return boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=env("AWS_ACCESS_KEY_ID", "R2_ACCESS_KEY_ID"),
        aws_secret_access_key=env("AWS_SECRET_ACCESS_KEY", "R2_SECRET_ACCESS_KEY"),
        region_name="auto",
    )


def latest_release(s3, bucket: str) -> tuple[dict[str, Any], datetime]:
    response = s3.get_object(Bucket=bucket, Key="os/images/latest-release.json")
    try:
        manifest = json.loads(response["Body"].read())
        last_modified = response["LastModified"]
    except (KeyError, TypeError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("latest-release.json response is incomplete") from error
    if not isinstance(manifest, dict):
        raise ValueError("latest-release.json must contain an object")
    validate_manifest(manifest)
    if manifest.get("channel") != "release":
        raise ValueError("latest-release.json must declare the release channel")
    if not isinstance(last_modified, datetime) or last_modified.tzinfo is None:
        raise ValueError("latest-release.json has no timezone-aware LastModified")
    return manifest, last_modified


def list_objects(s3, bucket: str) -> dict[str, int]:
    out: dict[str, int] = {}
    for prefix in PREFIXES:
        token = None
        while True:
            kw = dict(Bucket=bucket, Prefix=prefix)
            if token:
                kw["ContinuationToken"] = token
            resp = s3.list_objects_v2(**kw)
            for o in resp.get("Contents", []):
                out[o["Key"]] = o["Size"]
            if resp.get("IsTruncated"):
                token = resp.get("NextContinuationToken")
            else:
                break
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Prune superseded betas + legacy artifacts from R2.")
    ap.add_argument("--apply", action="store_true", help="actually delete (default: dry-run)")
    ap.add_argument("--bucket", default=env("R2_BUCKET") or "snapdog-updates")
    ap.add_argument("--endpoint", default=env("AWS_ENDPOINT_URL", "R2_ENDPOINT_URL"))
    ap.add_argument(
        "--delete-grace-seconds",
        type=int,
        default=DEFAULT_DELETE_GRACE_SECONDS,
        help="retain superseded payloads this long after a stable pointer update",
    )
    args = ap.parse_args()

    if not args.endpoint:
        print("ERROR: no R2 endpoint (set AWS_ENDPOINT_URL or R2_ENDPOINT_URL)", file=sys.stderr)
        return 2

    s3 = make_client(args.endpoint)
    if args.delete_grace_seconds < 0:
        print("ERROR: --delete-grace-seconds must be non-negative", file=sys.stderr)
        return 2

    release_manifest, release_modified = latest_release(s3, args.bucket)
    rel = release_manifest["version"]
    rel_t = semver_tuple(rel)
    objs = list_objects(s3, args.bucket)

    validate_committed_manifest(s3, args.bucket, "release", release_manifest)
    incomplete_release = incomplete_channel_artifacts(
        objs, "release", release_manifest
    )
    if incomplete_release:
        print(
            "ERROR: latest release is incomplete — refusing to prune:",
            file=sys.stderr,
        )
        for key in incomplete_release:
            print(f"  {key}", file=sys.stderr)
        return 2

    beta_latest_state = optional_json_object_state(s3, args.bucket, BETA_LATEST_KEY)
    beta_latest: dict[str, Any] | None = None
    beta_modified = release_modified
    if beta_latest_state is not None:
        beta_latest, beta_modified = beta_latest_state
        validate_manifest(beta_latest)
        if beta_latest.get("channel") != "beta":
            print("ERROR: latest-beta.json is not a beta manifest", file=sys.stderr)
            return 2
        beta_latest_version = beta_latest["version"]
        if channel_version_key(beta_latest_version) < channel_version_key(rel):
            print(
                "ERROR: latest-beta.json is behind the stable release: "
                f"{beta_latest_version}; repair the beta channel pointer before pruning",
                file=sys.stderr,
            )
            return 2
        incomplete_beta = incomplete_channel_artifacts(
            objs, "beta", beta_latest
        )
        if incomplete_beta:
            print(
                "ERROR: latest beta is incomplete — refusing to prune:",
                file=sys.stderr,
            )
            for key in incomplete_beta:
                print(f"  {key}", file=sys.stderr)
            return 2
        validate_committed_manifest(s3, args.bucket, "beta", beta_latest)
    elif beta_state_exists(objs):
        print(
            "ERROR: beta artifacts exist but latest-beta.json is missing — "
            "refusing to prune",
            file=sys.stderr,
        )
        return 2

    beta_catalog_state = optional_json_object_state(s3, args.bucket, CATALOG_KEY)
    beta_catalog = None if beta_catalog_state is None else beta_catalog_state[0]
    catalog_modified = (
        release_modified if beta_catalog_state is None else beta_catalog_state[1]
    )
    filtered_catalog = beta_catalog
    removed_catalog_versions: tuple[str, ...] = ()
    if beta_catalog is not None:
        filtered_catalog, removed_catalog_versions = filter_beta_catalog(
            beta_catalog, rel_t
        )
    filtered_catalog = retain_beta_pointer(filtered_catalog, beta_latest)
    validate_retained_beta_catalog(
        s3, args.bucket, filtered_catalog, beta_latest, objs
    )
    catalog_changed = filtered_catalog != beta_catalog

    delete: list[str] = []
    keep_reason: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    del_reason: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    for key, size in objs.items():
        action, reason = classify(key, rel_t)
        if action == "delete":
            delete.append(key)
            del_reason[reason][0] += 1
            del_reason[reason][1] += size
        else:
            keep_reason[reason][0] += 1
            keep_reason[reason][1] += size

    # Safety guard: never let a keep-classified key slip into the delete set.
    # Not an assert. `python -O` removes those, and this is the one line that
    # stands between a classification bug and deleted release artifacts.
    for k in delete:
        if classify(k, rel_t)[0] != "delete":
            raise RuntimeError(f"guard: {k} is not deletable")

    marker_versions = {
        version
        for key in objs
        if (version := retirement_marker_version(key)) is not None
    }
    unexpected_markers = sorted(
        version
        for version in marker_versions
        if not superseded_beta(version, rel_t)
    )
    if unexpected_markers:
        print(
            "ERROR: retirement markers exist for non-superseded betas: "
            + ", ".join(unexpected_markers),
            file=sys.stderr,
        )
        return 2

    print(f"bucket={args.bucket}  latest_release={rel}  scanned={len(objs)}")
    if catalog_changed:
        if removed_catalog_versions:
            print(
                "CATALOG: remove superseded beta versions "
                + ", ".join(removed_catalog_versions)
            )
        else:
            print("CATALOG: synchronize the current beta pointer")
        if filtered_catalog is None:
            print("  DEL os/images/catalog-beta.json (no retained entries)")
        else:
            print(
                "  PUT os/images/catalog-beta.json "
                f"({len(filtered_catalog['releases'])} retained entries)"
            )
    print("KEEP:")
    for r, (c, b) in sorted(keep_reason.items()):
        print(f"  {c:5d}  {b/1e6:9.0f} MB  {r}")
    total_mb = sum(objs[k] for k in delete) / 1e6
    print(f"DELETE ({len(delete)} objects, {total_mb:.0f} MB):")
    for r, (c, b) in sorted(del_reason.items()):
        print(f"  {c:5d}  {b/1e6:9.0f} MB  {r}")
    for k in sorted(delete):
        print(f"  DEL {k}")
    for version in sorted(marker_versions, key=channel_version_key):
        print(f"  RETIREMENT {version}")

    pointer_modified = max(release_modified, beta_modified, catalog_modified)
    pointer_age = (datetime.now(timezone.utc) - pointer_modified).total_seconds()
    delete_deferred = bool(delete) and (
        catalog_changed or pointer_age < args.delete_grace_seconds
    )
    if delete_deferred:
        if catalog_changed:
            print(
                "DELETE DEFERRED: beta catalog changed in this run; retry after "
                "cached catalog responses and active downloads can expire."
            )
        else:
            remaining = max(
                0, int(args.delete_grace_seconds - pointer_age + 0.999)
            )
            print(
                f"DELETE DEFERRED: newest channel pointer is only {pointer_age:.0f}s old; "
                f"retry in at least {remaining}s so cached pointers can expire."
            )

    if not delete and not catalog_changed and not marker_versions:
        print("Nothing to prune.")
        return 0
    if not args.apply:
        print("\n[DRY RUN] pass --apply to apply these retention changes.")
        return 0

    print("\n[APPLY] publishing catalog cleanup before deleting objects…")
    if catalog_changed:
        publish_beta_catalog(s3, args.bucket, filtered_catalog)

    if delete_deferred:
        print("Deleted 0 objects during the cache-safety grace period.")
    else:
        beta_groups: dict[str, list[str]] = defaultdict(list)
        legacy_delete: list[str] = []
        for key in delete:
            version = beta_artifact_version(key)
            if version is None:
                legacy_delete.append(key)
            else:
                beta_groups[version].append(key)
        for version in marker_versions:
            beta_groups[version]

        # Phase one removes each immutable beta manifest and starts a durable
        # timer. Payloads cannot be deleted until every cache allowed by that
        # manifest has expired, plus the active-download grace.
        markers: dict[str, dict[str, Any]] = {}
        newly_retired: set[str] = set()
        for version in sorted(beta_groups, key=channel_version_key):
            manifest_key = f"{BETA_MANIFEST_PREFIX}{version}.json"
            existing_marker = retirement_marker(s3, args.bucket, version)
            if manifest_key in objs:
                if existing_marker is not None:
                    raise ValueError(
                        f"{manifest_key} exists after its retirement timer started"
                    )
                response = _optional_object_response(s3, args.bucket, manifest_key)
                if response is None:
                    cache_seconds = LEGACY_BETA_MANIFEST_MAX_AGE_SECONDS
                else:
                    cache_seconds = manifest_cache_seconds(
                        response.get("CacheControl")
                    )
                    s3.delete_object(Bucket=args.bucket, Key=manifest_key)
                    if _optional_object_response(
                        s3, args.bucket, manifest_key
                    ) is not None:
                        raise RuntimeError(f"failed to retire {manifest_key}")
                retired_at = datetime.now(timezone.utc)
                marker = new_retirement_marker(
                    version,
                    retired_at,
                    manifest_max_age_seconds=cache_seconds,
                    download_grace_seconds=args.delete_grace_seconds,
                )
                publish_retirement_marker(s3, args.bucket, marker)
                markers[version] = marker
                newly_retired.add(version)
                print(
                    f"Retired {manifest_key}; payloads remain until "
                    f"{marker['manifest_retired_at']} + {cache_seconds}s cache "
                    f"+ {args.delete_grace_seconds}s download grace."
                )
            elif existing_marker is None:
                # Crash recovery: if a prior run removed the manifest but died
                # before writing its marker, restart the conservative legacy
                # cache clock now rather than guessing an earlier timestamp.
                retired_at = datetime.now(timezone.utc)
                marker = new_retirement_marker(
                    version,
                    retired_at,
                    manifest_max_age_seconds=LEGACY_BETA_MANIFEST_MAX_AGE_SECONDS,
                    download_grace_seconds=args.delete_grace_seconds,
                )
                publish_retirement_marker(s3, args.bucket, marker)
                markers[version] = marker
                newly_retired.add(version)
                print(
                    f"Started conservative crash-recovery retirement for {version}."
                )
            else:
                markers[version] = existing_marker

        # Phase two is per-version. A failure keeps the marker so a later run
        # retries only orphan payloads; it can never recreate public metadata.
        payload_deleted = 0
        markers_cleaned = 0
        phase_two_now = datetime.now(timezone.utc).timestamp()
        for version in sorted(beta_groups, key=channel_version_key):
            payloads = sorted(
                key
                for key in beta_groups[version]
                if beta_manifest_version(key) is None
            )
            marker = markers[version]
            if not payloads:
                s3.delete_object(
                    Bucket=args.bucket, Key=retirement_marker_key(version)
                )
                markers_cleaned += 1
                continue
            if version in newly_retired or phase_two_now < marker[
                "payload_not_before_epoch"
            ]:
                remaining = max(
                    0, int(marker["payload_not_before_epoch"] - phase_two_now)
                )
                print(
                    f"Deferred {len(payloads)} payloads for {version} "
                    f"for another {remaining}s."
                )
                continue
            delete_keys(s3, args.bucket, payloads)
            payload_deleted += len(payloads)
            s3.delete_object(
                Bucket=args.bucket, Key=retirement_marker_key(version)
            )

        # Canonical board-only legacy keys were never advertised by a version
        # manifest, but still observe the channel-pointer grace above.
        delete_keys(s3, args.bucket, sorted(legacy_delete))
        print(
            f"Deleted {payload_deleted} retired beta payloads and "
            f"{len(legacy_delete)} legacy objects; cleaned "
            f"{markers_cleaned} orphan retirement markers."
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
