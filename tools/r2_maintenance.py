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
import os
import re
import sys
from collections import defaultdict

import boto3

PREFIXES = ("os/bundles/", "os/images/", "os/sbom/")
ARTIFACT_SUFFIXES = (".raucb", ".img.gz", "-sbom.csv", ".sha256")
# snapdog-os-<board>-<rest>  (board = a single lowercase-alnum token, e.g. pi4,
# zero2w); <rest> is a version, a "-beta.N" version, or a channel alias.
STEM_RE = re.compile(r"^snapdog-os-([a-z0-9]+)(?:-(.+))?$")
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+$")


def env(*names: str) -> str | None:
    for n in names:
        v = os.environ.get(n)
        if v:
            return v
    return None


def semver_tuple(v: str) -> tuple[int, int, int]:
    return tuple(int(x) for x in v.split(".")[:3])  # type: ignore[return-value]


def artifact_stem(name: str) -> str | None:
    """Strip a known artifact suffix; return None for anything else (e.g. .json)."""
    for suf in ARTIFACT_SUFFIXES:
        if name.endswith(suf):
            return name[: -len(suf)]
    return None


def classify(key: str, release_t: tuple[int, int, int]) -> tuple[str, str]:
    """Return (action, reason) where action is 'keep' or 'delete'."""
    name = key.rsplit("/", 1)[-1]
    stem = artifact_stem(name)
    if stem is None:
        return ("keep", "manifest/other")  # latest-*.json, index.json, …
    m = STEM_RE.match(stem)
    if not m:
        return ("keep", "unrecognized (safe)")
    rest = m.group(2)
    if rest is None:
        return ("delete", "legacy board-only")
    if rest in ("release", "beta"):
        return ("keep", "channel alias")
    if "-beta." in rest:
        base = rest.split("-beta.", 1)[0]
        if SEMVER_RE.match(base) and semver_tuple(base) > release_t:
            return ("keep", "beta for upcoming release")
        return ("delete", "superseded beta")
    if SEMVER_RE.match(rest):
        return ("keep", "release version")
    return ("keep", "unrecognized (safe)")


def make_client(endpoint: str):
    return boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=env("AWS_ACCESS_KEY_ID", "R2_ACCESS_KEY_ID"),
        aws_secret_access_key=env("AWS_SECRET_ACCESS_KEY", "R2_SECRET_ACCESS_KEY"),
        region_name="auto",
    )


def latest_release(s3, bucket: str) -> str:
    body = s3.get_object(Bucket=bucket, Key="os/images/latest-release.json")["Body"].read()
    version = json.loads(body).get("version", "").strip()
    if not SEMVER_RE.match(version):
        raise ValueError(f"latest-release.json version is not a semver: {version!r}")
    return version


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
    args = ap.parse_args()

    if not args.endpoint:
        print("ERROR: no R2 endpoint (set AWS_ENDPOINT_URL or R2_ENDPOINT_URL)", file=sys.stderr)
        return 2

    s3 = make_client(args.endpoint)
    rel = latest_release(s3, args.bucket)
    rel_t = semver_tuple(rel)
    objs = list_objects(s3, args.bucket)

    delete: list[str] = []
    keep_reason: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    del_reason: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    release_versions = 0
    for key, size in objs.items():
        action, reason = classify(key, rel_t)
        if action == "delete":
            delete.append(key)
            del_reason[reason][0] += 1
            del_reason[reason][1] += size
        else:
            keep_reason[reason][0] += 1
            keep_reason[reason][1] += size
            if reason == "release version" and key.endswith(".raucb"):
                release_versions += 1

    # Safety guards: never proceed if the bucket looks wrong, and never let a
    # keep-classified key slip into the delete set.
    if release_versions == 0:
        print("ERROR: no release .raucb found — refusing to prune (wrong bucket?)", file=sys.stderr)
        return 2
    for k in delete:
        assert classify(k, rel_t)[0] == "delete", f"guard: {k} is not deletable"

    print(f"bucket={args.bucket}  latest_release={rel}  scanned={len(objs)}")
    print("KEEP:")
    for r, (c, b) in sorted(keep_reason.items()):
        print(f"  {c:5d}  {b/1e6:9.0f} MB  {r}")
    total_mb = sum(objs[k] for k in delete) / 1e6
    print(f"DELETE ({len(delete)} objects, {total_mb:.0f} MB):")
    for r, (c, b) in sorted(del_reason.items()):
        print(f"  {c:5d}  {b/1e6:9.0f} MB  {r}")
    for k in sorted(delete):
        print(f"  DEL {k}")

    if not delete:
        print("Nothing to prune.")
        return 0
    if not args.apply:
        print("\n[DRY RUN] pass --apply to delete.")
        return 0

    print("\n[APPLY] deleting…")
    for i in range(0, len(delete), 1000):
        batch = [{"Key": k} for k in delete[i : i + 1000]]
        resp = s3.delete_objects(Bucket=args.bucket, Delete={"Objects": batch, "Quiet": True})
        errors = resp.get("Errors", [])
        if errors:
            print(f"ERROR deleting: {errors}", file=sys.stderr)
            return 1
    print(f"Deleted {len(delete)} objects.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
