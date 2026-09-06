#!/usr/bin/env python3
"""Lease-based cross-workflow lock for SnapDog's mutable R2 objects.

GitHub Actions concurrency groups retain only one pending run, so they are not
a reliable FIFO for release publications.  This lock uses R2 conditional writes
to serialize every workflow that mutates channel pointers or catalogs.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from datetime import datetime, timezone
from typing import Any, Callable


LOCK_KEY = "os/.locks/mutation.json"
PRECONDITION_CODES = {
    "409",
    "412",
    "ConditionalRequestConflict",
    "PreconditionFailed",
}
MISSING_CODES = {"404", "NoSuchKey", "NotFound"}


class LockTimeout(RuntimeError):
    """Raised when the R2 lease cannot be acquired within the wait window."""


class LockLost(RuntimeError):
    """Raised when a mutator no longer owns the lease it is trying to renew."""


def _error_code(error: BaseException) -> str:
    response = getattr(error, "response", {})
    return str(response.get("Error", {}).get("Code", ""))


def _lease_body(owner: str, now: float, ttl_seconds: int) -> bytes:
    return json.dumps(
        {
            "owner": owner,
            "acquired_at": datetime.fromtimestamp(now, timezone.utc).isoformat(),
            "expires_at_epoch": now + ttl_seconds,
        },
        sort_keys=True,
    ).encode("utf-8")


def acquire_lock(
    client: Any,
    bucket: str,
    key: str,
    owner: str,
    *,
    ttl_seconds: int,
    wait_seconds: int,
    poll_seconds: float,
    clock: Callable[[], float] = time.time,
    sleep: Callable[[float], None] = time.sleep,
) -> None:
    """Acquire a lease, atomically replacing it only after it has expired."""

    deadline = clock() + wait_seconds
    while True:
        now = clock()
        body = _lease_body(owner, now, ttl_seconds)
        try:
            client.put_object(
                Bucket=bucket,
                Key=key,
                Body=body,
                ContentType="application/json",
                CacheControl="no-store",
                IfNoneMatch="*",
            )
            return
        except Exception as error:  # botocore is loaded only in the CLI process
            if _error_code(error) not in PRECONDITION_CODES:
                raise

        try:
            current = client.get_object(Bucket=bucket, Key=key)
        except Exception as error:
            if _error_code(error) in MISSING_CODES:
                continue
            raise

        try:
            lease = json.loads(current["Body"].read())
            expires_at = float(lease["expires_at_epoch"])
            etag = current["ETag"]
        except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
            raise RuntimeError(f"R2 lock {key!r} is malformed") from error

        if expires_at <= now:
            try:
                client.put_object(
                    Bucket=bucket,
                    Key=key,
                    Body=body,
                    ContentType="application/json",
                    CacheControl="no-store",
                    IfMatch=etag,
                )
                return
            except Exception as error:
                if _error_code(error) in PRECONDITION_CODES:
                    continue
                raise

        if now >= deadline:
            held_by = lease.get("owner", "unknown")
            raise LockTimeout(
                f"timed out waiting for R2 mutation lock held by {held_by!r}"
            )
        sleep(min(poll_seconds, max(0.0, deadline - now)))


def release_lock(
    client: Any,
    bucket: str,
    key: str,
    owner: str,
    *,
    clock: Callable[[], float] = time.time,
) -> bool:
    """Expire only the caller's lease; never alter a successor's lock.

    Release is a conditional PutObject rather than DeleteObject because R2
    explicitly supports If-Match on PutObject. The expired marker is atomically
    replaced by the next acquirer.
    """

    try:
        current = client.get_object(Bucket=bucket, Key=key)
    except Exception as error:
        if _error_code(error) in MISSING_CODES:
            return False
        raise

    try:
        lease = json.loads(current["Body"].read())
        etag = current["ETag"]
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise RuntimeError(f"R2 lock {key!r} is malformed") from error
    if lease.get("owner") != owner:
        return False

    now = clock()
    try:
        client.put_object(
            Bucket=bucket,
            Key=key,
            Body=_lease_body(f"released:{owner}", now, 0),
            ContentType="application/json",
            CacheControl="no-store",
            IfMatch=etag,
        )
    except Exception as error:
        if _error_code(error) in PRECONDITION_CODES | MISSING_CODES:
            return False
        raise
    return True


def renew_lock(
    client: Any,
    bucket: str,
    key: str,
    owner: str,
    *,
    ttl_seconds: int,
    clock: Callable[[], float] = time.time,
) -> None:
    """Extend the caller's lease with an ETag-fenced conditional write."""

    try:
        current = client.get_object(Bucket=bucket, Key=key)
    except Exception as error:
        if _error_code(error) in MISSING_CODES:
            raise LockLost("R2 mutation lock disappeared before renewal") from error
        raise

    try:
        lease = json.loads(current["Body"].read())
        etag = current["ETag"]
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise RuntimeError(f"R2 lock {key!r} is malformed") from error
    if lease.get("owner") != owner:
        raise LockLost(
            f"R2 mutation lock belongs to {lease.get('owner', 'unknown')!r}, not {owner!r}"
        )

    try:
        client.put_object(
            Bucket=bucket,
            Key=key,
            Body=_lease_body(owner, clock(), ttl_seconds),
            ContentType="application/json",
            CacheControl="no-store",
            IfMatch=etag,
        )
    except Exception as error:
        if _error_code(error) in PRECONDITION_CODES:
            raise LockLost("R2 mutation lock changed during renewal") from error
        raise


def _client() -> Any:
    try:
        import boto3
    except ImportError as error:  # pragma: no cover - exercised by workflow setup
        raise SystemExit("boto3 is required; install it in the publish environment") from error
    endpoint = os.environ.get("AWS_ENDPOINT_URL")
    if not endpoint:
        raise SystemExit("AWS_ENDPOINT_URL is required")
    return boto3.client("s3", endpoint_url=endpoint, region_name="auto")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("acquire", "renew", "release"))
    parser.add_argument("--bucket", default=os.environ.get("R2_BUCKET"))
    parser.add_argument("--key", default=LOCK_KEY)
    parser.add_argument("--owner", required=True)
    parser.add_argument("--ttl-seconds", type=int, default=14_400)
    parser.add_argument("--wait-seconds", type=int, default=18_000)
    parser.add_argument("--poll-seconds", type=float, default=10.0)
    return parser


def main() -> int:
    args = _parser().parse_args()
    if not args.bucket:
        raise SystemExit("--bucket or R2_BUCKET is required")
    if args.ttl_seconds <= 0 or args.wait_seconds < 0 or args.poll_seconds <= 0:
        raise SystemExit("lock timing values must be positive")

    client = _client()
    if args.command == "acquire":
        acquire_lock(
            client,
            args.bucket,
            args.key,
            args.owner,
            ttl_seconds=args.ttl_seconds,
            wait_seconds=args.wait_seconds,
            poll_seconds=args.poll_seconds,
        )
        print(f"Acquired R2 mutation lock as {args.owner}")
    elif args.command == "renew":
        renew_lock(
            client,
            args.bucket,
            args.key,
            args.owner,
            ttl_seconds=args.ttl_seconds,
        )
        print(f"Renewed R2 mutation lock as {args.owner}")
    else:
        released = release_lock(client, args.bucket, args.key, args.owner)
        print("Released R2 mutation lock" if released else "R2 mutation lock already changed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
