"""Publish an immutable GitHub release only after every expected asset exists."""

from __future__ import annotations

import argparse
import json
import re
import shutil
# Invoke the trusted GitHub CLI without a shell.
import subprocess  # nosec B404
import sys

BOARDS = ("pi3", "pi4", "pi5", "zero2w")
UNIX_TARGETS = (
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
)
WINDOWS_TARGETS = ("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")


def expected_assets(kind: str, tag: str) -> set[str]:
    prefix = "v" if kind == "os" else "snapdog-update-v"
    if kind not in ("os", "updater") or not re.fullmatch(
        re.escape(prefix) + r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", tag
    ):
        raise ValueError(f"Invalid {kind} release tag: {tag}")
    names = set()
    for target in UNIX_TARGETS:
        archive = f"snapdog-update-{tag}-{target}.tar.gz"
        names.update((archive, f"{archive}.sha256"))
    if kind == "os":
        version = tag.removeprefix("v")
        for board in BOARDS:
            base = f"snapdog-os-{board}-{version}"
            names.update(f"{base}{suffix}" for suffix in (".img.gz", ".raucb", "-sbom.csv", ".sha256"))
        for target in WINDOWS_TARGETS:
            archive = f"snapdog-update-{tag}-{target}.zip"
            names.update((archive, f"{archive}.sha256"))
    else:
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


def finalize_release(kind: str, tag: str, repo: str) -> None:
    expected = expected_assets(kind, tag)
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise ValueError(f"Invalid GitHub repository: {repo}")
    gh_path = shutil.which("gh")
    if gh_path is None:
        raise ValueError("GitHub CLI (gh) is not installed")
    # The executable is resolved from the runner's PATH; tag and repo are
    # validated above and passed as separate arguments, never shell commands.
    result = subprocess.run(  # nosec B603
        [gh_path, "release", "view", tag, "--repo", repo, "--json", "tagName,isDraft,assets"],
        check=True, capture_output=True, text=True,
    )
    release = json.loads(result.stdout)
    validate_release(release, expected, tag)
    if not release["isDraft"]:
        print(f"{tag} is already published with all {len(expected)} required assets")
        return
    prerelease = "-" in tag.removeprefix("snapdog-update-v").removeprefix("v").split("+", 1)[0]
    # Use the same trusted executable and validated arguments for publication.
    subprocess.run(  # nosec B603
        [
            gh_path, "release", "edit", tag, "--repo", repo, "--draft=false",
            "--verify-tag", f"--prerelease={str(prerelease).lower()}",
            f"--latest={str(kind == 'os' and not prerelease).lower()}",
        ],
        check=True,
    )
    print(f"Published {tag} with all {len(expected)} required assets")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=("os", "updater"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repo", required=True)
    args = parser.parse_args()
    try:
        finalize_release(args.kind, args.tag, args.repo)
    except (ValueError, subprocess.CalledProcessError) as error:
        print(f"Release publication refused: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
