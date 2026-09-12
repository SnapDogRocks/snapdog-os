#!/usr/bin/env python3
"""Small, dependency-free checks used by the snapdog-update tap workflow.

Homebrew derives a formula version from its URLs.  The tap used to publish the
updater below the OS release tag (``.../download/v0.16.6/...``), while the
updater has its own release stream now (``.../download/snapdog-update-v0.4.2``).
This module keeps that migration explicit instead of comparing the two
unrelated numbers as if they were one release stream.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any


SEMVER_RE = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)
URL_SHA_RE = re.compile(
    r'\burl\s+"(?P<url>[^"]+)"\s*\n\s*'
    r'sha256\s+"(?P<sha>[0-9a-fA-F]{64})"'
)
URL_RE = re.compile(
    r"/releases/download/(?P<release>[^/]+)/"
    r"snapdog-update-v(?P<version>[^/]+)-(?P<arch>"
    r"x86_64-apple-darwin|aarch64-apple-darwin)\.tar\.gz$"
)
SCHEME_RE = re.compile(r"^\s*version_scheme\s+(?P<scheme>[0-9]+)\s*$", re.MULTILINE)


class FormulaError(ValueError):
    """Raised when the tap formula is not a formula this workflow owns."""


@dataclass(frozen=True)
class FormulaState:
    version: str
    scheme: int
    legacy: bool
    urls: dict[str, str]
    hashes: dict[str, str]

    def json(self) -> dict[str, Any]:
        return asdict(self)


def semver_key(version: str) -> tuple[Any, ...]:
    """Return a SemVer ordering key (build metadata is intentionally ignored)."""

    match = SEMVER_RE.fullmatch(version)
    if not match:
        raise FormulaError(f"invalid SemVer: {version}")
    major, minor, patch, prerelease, _build = match.groups()
    core = (int(major), int(minor), int(patch))
    if prerelease is None:
        return (*core, (1,))
    identifiers: list[tuple[int, Any]] = []
    for identifier in prerelease.split("."):
        if identifier.isdigit():
            # Numeric prerelease identifiers sort before non-numeric ones.
            identifiers.append((0, int(identifier)))
        else:
            identifiers.append((1, identifier))
    return (*core, (0, *identifiers))


def parse_formula(text: str) -> FormulaState:
    """Parse both the old OS-tag URL scheme and the current updater scheme."""

    scheme_matches = list(SCHEME_RE.finditer(text))
    if len(scheme_matches) > 1:
        raise FormulaError("formula contains more than one version_scheme")
    explicit_scheme = int(scheme_matches[0].group("scheme")) if scheme_matches else None

    entries = list(URL_SHA_RE.finditer(text))
    if len(entries) != 2:
        raise FormulaError("formula must contain exactly two URL/SHA-256 pairs")

    urls: dict[str, str] = {}
    hashes: dict[str, str] = {}
    versions: set[str] = set()
    schemes: set[int] = set()
    for entry in entries:
        url = entry.group("url")
        digest = entry.group("sha").lower()
        match = URL_RE.search(url)
        if not match:
            raise FormulaError(f"unrecognised updater archive URL: {url}")
        arch = match.group("arch")
        version = match.group("version")
        release = match.group("release")
        semver_key(version)
        if arch in urls:
            raise FormulaError(f"duplicate archive URL for {arch}")
        if release == f"snapdog-update-v{version}":
            url_scheme = 1
        elif release == f"v{version}":
            # This was the OS release URL scheme.  Its number is not the
            # updater version, even though the old archive was named similarly.
            url_scheme = 0
        else:
            raise FormulaError(f"release tag and archive version disagree: {url}")
        urls[arch] = url
        hashes[arch] = digest
        versions.add(version)
        schemes.add(url_scheme)

    if len(versions) != 1:
        raise FormulaError("formula URL versions disagree")
    if len(schemes) != 1:
        raise FormulaError("formula mixes URL version schemes")
    url_scheme = schemes.pop()
    # Homebrew treats an omitted directive as scheme 0.  Do not infer scheme 1
    # merely because a formula already has updater-shaped URLs: the migration
    # must explicitly write ``version_scheme 1``.
    scheme = explicit_scheme if explicit_scheme is not None else 0
    if explicit_scheme is not None and explicit_scheme != url_scheme:
        raise FormulaError(
            f"version_scheme {explicit_scheme} does not match URL scheme {url_scheme}"
        )
    return FormulaState(
        version=versions.pop(),
        scheme=scheme,
        legacy=url_scheme == 0,
        urls=urls,
        hashes=hashes,
    )


def decide(text: str | None, target_version: str) -> dict[str, Any]:
    """Classify a tap update without ever ordering legacy and updater versions."""

    semver_key(target_version)
    if text is None:
        return {
            "action": "update",
            "current_version": None,
            "current_scheme": None,
            "legacy": False,
            "validation_version": target_version,
            "validation_tag": f"snapdog-update-v{target_version}",
        }

    state = parse_formula(text)
    if state.legacy or state.scheme != 1:
        # A legacy 0.16.6 OS tag must be migrated to updater 0.4.2 even though
        # a plain numeric comparison would incorrectly call it newer.
        action = "migrate"
    elif semver_key(state.version) == semver_key(target_version):
        action = "noop"
    elif semver_key(state.version) > semver_key(target_version):
        action = "stale"
    else:
        action = "update"
    validation_version = target_version if action in {"update", "migrate", "noop"} else state.version
    return {
        "action": action,
        "current_version": state.version,
        "current_scheme": state.scheme,
        "legacy": state.legacy,
        "validation_version": validation_version,
        "validation_tag": f"snapdog-update-v{validation_version}",
    }


def validate_formula(
    text: str,
    *,
    tag: str,
    version: str,
    x86_sha: str | None = None,
    arm_sha: str | None = None,
    require_scheme: int = 1,
) -> FormulaState:
    """Validate URLs, hashes, architecture coverage, and version scheme."""

    state = parse_formula(text)
    if state.version != version:
        raise FormulaError(
            f"formula version {state.version} does not match expected {version}"
        )
    if state.scheme != require_scheme:
        raise FormulaError(
            f"formula version_scheme {state.scheme} does not match expected {require_scheme}"
        )
    expected_urls = {
        "x86_64-apple-darwin": (
            f"https://github.com/SnapDogRocks/snapdog-os/releases/download/{tag}/"
            f"{tag}-x86_64-apple-darwin.tar.gz"
        ),
        "aarch64-apple-darwin": (
            f"https://github.com/SnapDogRocks/snapdog-os/releases/download/{tag}/"
            f"{tag}-aarch64-apple-darwin.tar.gz"
        ),
    }
    for arch, expected_url in expected_urls.items():
        if state.urls.get(arch) != expected_url:
            raise FormulaError(f"formula URL for {arch} is not {expected_url}")
    for arch, expected_sha in (
        ("x86_64-apple-darwin", x86_sha),
        ("aarch64-apple-darwin", arm_sha),
    ):
        if expected_sha is not None and state.hashes.get(arch) != expected_sha.lower():
            raise FormulaError(f"formula SHA-256 for {arch} does not match release asset")
    if state.legacy:
        raise FormulaError("legacy URL scheme is not valid after migration")
    return state


def checks_registered(payload: str) -> bool:
    """Return whether ``gh pr checks --json`` reported at least one check.

    The command exits 8 while checks are still pending.  Its JSON output is the
    source of truth; the process status must not be used as a registration test.
    """

    try:
        value = json.loads(payload)
    except json.JSONDecodeError:
        return False
    if isinstance(value, list):
        return bool(value)
    if isinstance(value, dict):
        # Also accept the shape returned by the REST check-runs endpoint for
        # callers/tests that use that endpoint directly.
        return bool(value.get("check_runs") or value.get("statuses"))
    return False


def orphan_branch_is_deletable(open_pr_count: int) -> bool:
    """A branch may be cleaned up only after proving no open PR uses it."""

    return open_pr_count == 0


def read_formula(path: str) -> str | None:
    formula_path = Path(path)
    if not formula_path.exists():
        return None
    return formula_path.read_text(encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    decision_parser = subparsers.add_parser("decision")
    decision_parser.add_argument("--formula", required=True)
    decision_parser.add_argument("--target-version", required=True)

    state_parser = subparsers.add_parser("state")
    state_parser.add_argument("--formula", required=True)

    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("--formula", required=True)
    validate_parser.add_argument("--tag", required=True)
    validate_parser.add_argument("--version", required=True)
    validate_parser.add_argument("--x86-sha")
    validate_parser.add_argument("--arm-sha")
    validate_parser.add_argument("--require-scheme", type=int, default=1)

    checks_parser = subparsers.add_parser("checks-registered")
    checks_parser.add_argument("--payload", help="JSON payload; stdin when omitted")

    cleanup_parser = subparsers.add_parser("branch-cleanup-decision")
    cleanup_parser.add_argument("--open-pr-count", type=int, required=True)

    args = parser.parse_args(argv)
    try:
        if args.command == "decision":
            print(json.dumps(decide(read_formula(args.formula), args.target_version)))
        elif args.command == "state":
            text = read_formula(args.formula)
            if text is None:
                print(json.dumps({"exists": False}))
            else:
                print(json.dumps({"exists": True, **parse_formula(text).json()}))
        elif args.command == "validate":
            state = parse_formula(read_formula(args.formula) or "")
            validate_formula(
                Path(args.formula).read_text(encoding="utf-8"),
                tag=args.tag,
                version=args.version,
                x86_sha=args.x86_sha,
                arm_sha=args.arm_sha,
                require_scheme=args.require_scheme,
            )
            print(json.dumps(state.json()))
        elif args.command == "checks-registered":
            payload = args.payload if args.payload is not None else sys.stdin.read()
            return 0 if checks_registered(payload) else 1
        elif args.command == "branch-cleanup-decision":
            return 0 if orphan_branch_is_deletable(args.open_pr_count) else 1
    except (FormulaError, OSError) as error:
        print(f"homebrew formula validation failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
