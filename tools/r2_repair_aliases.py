#!/usr/bin/env python3
"""Inspect or repair rolling R2 aliases from committed channel pointers.

This is the out-of-process recovery path for a runner that disappears during
the release cutover while aliases contain the deliberately invalid transition
object. Versioned objects and their exact manifest are authoritative; aliases
are never used as a repair source.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Callable

SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

from release_manifest import BOARDS, validate_manifest  # noqa: E402


POINTER_CACHE_BARRIER_SECONDS = 70


def env(*names: str) -> str | None:
    for name in names:
        value = os.environ.get(name)
        if value:
            return value
    return None


def error_code(error: BaseException) -> str:
    response = getattr(error, "response", {})
    return str(response.get("Error", {}).get("Code", ""))


def missing(error: BaseException) -> bool:
    return error_code(error) in {"404", "NoSuchKey", "NotFound"}


def json_object_state(
    client: Any, bucket: str, key: str, *, optional: bool
) -> tuple[dict[str, Any], datetime] | None:
    try:
        response = client.get_object(Bucket=bucket, Key=key)
    except Exception as error:
        if optional and missing(error):
            return None
        raise
    try:
        value = json.loads(response["Body"].read())
    except (KeyError, TypeError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{key} is not valid JSON") from error
    if not isinstance(value, dict):
        raise ValueError(f"{key} must contain a JSON object")
    modified = response.get("LastModified")
    if not isinstance(modified, datetime) or modified.tzinfo is None:
        raise ValueError(f"{key} has no timezone-aware LastModified")
    return value, modified


def head_optional(client: Any, bucket: str, key: str) -> dict[str, Any] | None:
    try:
        return client.head_object(Bucket=bucket, Key=key)
    except Exception as error:
        if missing(error):
            return None
        raise


def alias_current(
    alias: dict[str, Any] | None,
    source: dict[str, Any],
    source_key: str,
) -> bool:
    if alias is None:
        return False
    cache_control = str(alias.get("CacheControl", "")).lower()
    metadata = alias.get("Metadata", {})
    source_marker = (
        str(metadata.get("snapdog-source-key", ""))
        if isinstance(metadata, dict)
        else ""
    )
    identity_matches = source_marker == source_key or (
        not source_marker and alias.get("ETag") == source.get("ETag")
    )
    return (
        alias.get("ContentLength") == source.get("ContentLength")
        and identity_matches
        and "no-store" in cache_control
    )


def repair_channel(
    client: Any,
    bucket: str,
    channel: str,
    *,
    apply: bool,
    pointer_cache_barrier_seconds: int = POINTER_CACHE_BARRIER_SECONDS,
    clock: Callable[[], float] = time.time,
    sleep: Callable[[float], None] = time.sleep,
) -> int:
    if pointer_cache_barrier_seconds < 0:
        raise ValueError("pointer cache barrier must be non-negative")
    pointer_key = f"os/images/latest-{channel}.json"
    pointer_state = json_object_state(client, bucket, pointer_key, optional=True)
    if pointer_state is None:
        print(f"{channel} channel has not been initialized; nothing to repair")
        return 0
    pointer, pointer_modified = pointer_state
    validate_manifest(pointer)
    if pointer.get("channel") != channel:
        raise ValueError(f"{pointer_key} does not declare channel {channel!r}")

    version = pointer["version"]
    committed_key = f"os/images/manifests/{channel}/{version}.json"
    committed_state = json_object_state(
        client, bucket, committed_key, optional=False
    )
    if committed_state is None:
        # `json_object_state(optional=False)` contracts never to return None. An
        # assert would state that too, and vanish under `python -O`, which is
        # exactly the run where a silent None would delete the wrong objects.
        raise RuntimeError(f"{committed_key} could not be read")
    committed, _ = committed_state
    validate_manifest(committed)
    if committed != pointer:
        raise ValueError(f"{committed_key} does not exactly match {pointer_key}")

    repairs: list[tuple[str, str, dict[str, Any], str]] = []
    for board in BOARDS:
        for directory, suffix, content_type in (
            ("images", ".img.gz", "application/gzip"),
            ("bundles", ".raucb", "application/octet-stream"),
        ):
            source_key = f"os/{directory}/snapdog-os-{board}-{version}{suffix}"
            alias_key = f"os/{directory}/snapdog-os-{board}-{channel}{suffix}"
            source = head_optional(client, bucket, source_key)
            if source is None or int(source.get("ContentLength", 0)) <= 0:
                raise ValueError(f"authoritative object is missing or empty: {source_key}")
            if directory == "images":
                expected = pointer["boards"][board]["compressed_size"]
                if source["ContentLength"] != expected:
                    raise ValueError(
                        f"{source_key} size {source['ContentLength']} != manifest {expected}"
                    )

            alias = head_optional(client, bucket, alias_key)
            if alias_current(alias, source, source_key):
                continue
            action = "REPAIR" if apply else "WOULD REPAIR"
            print(f"{action} {alias_key} <- {source_key}")
            repairs.append((alias_key, source_key, source, content_type))

    if not apply or not repairs:
        return len(repairs)

    pointer_age = clock() - pointer_modified.timestamp()
    remaining = pointer_cache_barrier_seconds - pointer_age
    if remaining > 0:
        print(
            f"Waiting {remaining:.0f}s for cached {channel} pointers to expire "
            "before repairing aliases."
        )
        sleep(remaining)

        # The workflow holds the shared mutation lease while this tool runs.
        # Still re-read the authoritative pointer so standalone use fails closed
        # if another writer ignored that protocol during the barrier.
        refreshed_state = json_object_state(
            client, bucket, pointer_key, optional=False
        )
        if refreshed_state is None:
            raise RuntimeError(f"{pointer_key} could not be re-read")
        refreshed, refreshed_modified = refreshed_state
        if refreshed != pointer or refreshed_modified != pointer_modified:
            raise RuntimeError(f"{pointer_key} changed during its cache barrier")

    for alias_key, source_key, source, content_type in repairs:
        client.copy_object(
            Bucket=bucket,
            Key=alias_key,
            CopySource={"Bucket": bucket, "Key": source_key},
            MetadataDirective="REPLACE",
            Metadata={"snapdog-source-key": source_key},
            CacheControl="no-store, max-age=0",
            ContentType=content_type,
        )
        updated = head_optional(client, bucket, alias_key)
        if not alias_current(updated, source, source_key):
            raise RuntimeError(f"alias repair did not commit correctly: {alias_key}")
    return len(repairs)


def make_client(endpoint: str) -> Any:
    import boto3

    return boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=env("AWS_ACCESS_KEY_ID", "R2_ACCESS_KEY_ID"),
        aws_secret_access_key=env("AWS_SECRET_ACCESS_KEY", "R2_SECRET_ACCESS_KEY"),
        region_name="auto",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument(
        "--channel",
        action="append",
        choices=("release", "beta"),
        dest="channels",
        help="repair only this channel (repeatable; default: both)",
    )
    parser.add_argument("--bucket", default=env("R2_BUCKET") or "snapdog-updates")
    parser.add_argument("--endpoint", default=env("AWS_ENDPOINT_URL", "R2_ENDPOINT_URL"))
    args = parser.parse_args()
    if not args.endpoint:
        raise SystemExit("AWS_ENDPOINT_URL or R2_ENDPOINT_URL is required")

    client = make_client(args.endpoint)
    channels = args.channels or ["release", "beta"]
    repaired = sum(
        repair_channel(client, args.bucket, channel, apply=args.apply)
        for channel in channels
    )
    verb = "repaired" if args.apply else "would repair"
    print(f"Alias check complete: {repaired} object(s) {verb}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
