"""Validate release JSON from stdin; emit its state only when assets are complete."""

from __future__ import annotations

import argparse
import json
import re
import sys

BOARDS = ("pi3", "pi4", "pi5", "zero2w")
UNIX_TARGETS = (
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
)


def expected_assets(kind: str, tag: str) -> set[str]:
    prefix = "v" if kind == "os" else "snapdog-update-v"
    if kind not in ("os", "updater") or not re.fullmatch(
        re.escape(prefix) + r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", tag
    ):
        raise ValueError(f"Invalid {kind} release tag: {tag}")
    names = set()
    if kind == "os":
        # The OS release carries images, bundles and SBOMs. snapdog-update is a
        # package of its own with its own version and its own release; it used to
        # be built a second time here under the OS tag, which put a binary
        # reporting one version behind an asset name claiming another.
        version = tag.removeprefix("v")
        for board in BOARDS:
            base = f"snapdog-os-{board}-{version}"
            names.update(f"{base}{suffix}" for suffix in (".img.gz", ".raucb", "-sbom.csv", ".sha256"))
    else:
        # The tag already reads `snapdog-update-v<version>`, so it is the whole
        # archive prefix. Prefixing it again produced
        # `snapdog-update-snapdog-update-v0.4.1-…`.
        for target in UNIX_TARGETS:
            archive = f"{tag}-{target}.tar.gz"
            names.update((archive, f"{archive}.sha256"))
        names.add("SHA256SUMS")
    return names


def validate_release(release: dict, expected: set[str], tag: str) -> None:
    if release.get("tagName") != tag:
        raise ValueError(f"Release tag does not match {tag}")
    assets = release.get("assets", [])
    names = [asset["name"] for asset in assets]
    if len(names) != len(set(names)):
        raise ValueError("Release contains duplicate asset names")
    missing = expected - set(names)
    if missing:
        raise ValueError("Missing release assets: " + ", ".join(sorted(missing)))
    invalid = [
        asset["name"]
        for asset in assets
        if asset.get("state") != "uploaded" or asset.get("size", 0) <= 0
    ]
    if invalid:
        raise ValueError("Empty or incomplete release assets: " + ", ".join(sorted(invalid)))


def publication_state(kind: str, tag: str, release: dict) -> str:
    expected = expected_assets(kind, tag)
    validate_release(release, expected, tag)
    if not isinstance(release.get("isDraft"), bool):
        raise ValueError("Missing or invalid release draft state")
    return "draft" if release["isDraft"] else "published"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=("os", "updater"), required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    try:
        print(publication_state(args.kind, args.tag, json.load(sys.stdin)))
    except (ValueError, KeyError, TypeError, AttributeError) as error:
        print(f"Release publication refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
