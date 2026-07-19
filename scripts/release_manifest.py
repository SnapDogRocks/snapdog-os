#!/usr/bin/env python3
"""Build and validate SnapDog OS image manifests.

The public channel manifest is intentionally generated from per-board metadata
created while both the raw and compressed images are still available. This
keeps the release manifest small while allowing download and decompression to
be verified independently.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import re
import sys
import tempfile
import zlib
from datetime import datetime
from pathlib import Path
from typing import Any, BinaryIO
from urllib.parse import quote, unquote, urlparse

BOARDS = ("pi3", "pi4", "pi5", "zero2w")
CHANNELS = ("release", "beta")
SCHEMA_VERSION = 2
CHUNK_SIZE = 1024 * 1024

SEMVER_RE = re.compile(
    r"^(?:0|[1-9][0-9]*)\."
    r"(?:0|[1-9][0-9]*)\."
    r"(?:0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")


class ManifestError(ValueError):
    """Raised when image metadata or a public manifest violates the contract."""


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ManifestError(message)


def _read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ManifestError(f"cannot read JSON from {path}: {error}") from error
    _require(isinstance(value, dict), f"{path}: top-level value must be an object")
    return value


def _write_json(path: Path, value: dict[str, Any]) -> None:
    payload = json.dumps(value, indent=2, sort_keys=False) + "\n"
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            delete=False,
        ) as output:
            output.write(payload)
            temporary_path = Path(output.name)
        os.replace(temporary_path, path)
    except OSError as error:
        raise ManifestError(f"cannot write JSON to {path}: {error}") from error


def _digest(stream: BinaryIO) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    while chunk := stream.read(CHUNK_SIZE):
        digest.update(chunk)
        size += len(chunk)
    return digest.hexdigest(), size


def _digest_file(path: Path) -> tuple[str, int]:
    try:
        with path.open("rb") as stream:
            return _digest(stream)
    except OSError as error:
        raise ManifestError(f"cannot read image {path}: {error}") from error


def _digest_gzip_payload(path: Path) -> tuple[str, int]:
    try:
        with gzip.open(path, "rb") as stream:
            return _digest(stream)
    except (OSError, EOFError, zlib.error) as error:
        raise ManifestError(f"cannot decompress image {path}: {error}") from error


def _validate_version(version: Any, context: str) -> str:
    _require(isinstance(version, str), f"{context}: version must be a string")
    _require(
        SEMVER_RE.fullmatch(version) is not None,
        f"{context}: invalid version {version!r}",
    )
    return version


def _validate_sha256(value: Any, context: str) -> str:
    _require(
        isinstance(value, str) and SHA256_RE.fullmatch(value) is not None,
        f"{context}: expected a lowercase SHA-256 digest",
    )
    return value


def _validate_size(value: Any, context: str) -> int:
    _require(
        isinstance(value, int) and not isinstance(value, bool) and value > 0,
        f"{context}: expected a positive byte count",
    )
    return value


def _validate_date(value: Any) -> str:
    _require(isinstance(value, str), "manifest: date must be a string")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise ManifestError(f"manifest: invalid ISO-8601 date {value!r}") from error
    _require(parsed.tzinfo is not None, "manifest: date must include a timezone")
    return value


def _validate_base_url(value: str) -> str:
    parsed = urlparse(value)
    _require(parsed.scheme == "https", "base URL must use HTTPS")
    _require(bool(parsed.netloc), "base URL must include a host")
    _require(not parsed.params, "base URL must not include parameters")
    _require(not parsed.query, "base URL must not include a query")
    _require(not parsed.fragment, "base URL must not include a fragment")
    return value.rstrip("/")


def create_board_metadata(
    *, board: str, version: str, raw_image: Path, compressed_image: Path
) -> dict[str, Any]:
    """Create private build metadata and verify the gzip represents the raw image."""
    _require(board in BOARDS, f"unsupported board {board!r}")
    _validate_version(version, "board metadata")
    expected_name = f"snapdog-os-{board}-{version}.img.gz"
    _require(
        compressed_image.name == expected_name,
        f"compressed image must be named {expected_name!r}, got {compressed_image.name!r}",
    )

    raw_sha256, uncompressed_size = _digest_file(raw_image)
    compressed_sha256, compressed_size = _digest_file(compressed_image)
    payload_sha256, payload_size = _digest_gzip_payload(compressed_image)
    _require(
        (payload_sha256, payload_size) == (raw_sha256, uncompressed_size),
        "compressed image does not expand to the supplied raw image",
    )

    return {
        "board": board,
        "version": version,
        "image": compressed_image.name,
        "sha256": compressed_sha256,
        "raw_sha256": raw_sha256,
        "compressed_size": compressed_size,
        "uncompressed_size": uncompressed_size,
    }


def _validate_board_metadata(
    metadata: dict[str, Any], source: Path, *, verify_compressed_image: bool
) -> dict[str, Any]:
    expected_keys = {
        "board",
        "version",
        "image",
        "sha256",
        "raw_sha256",
        "compressed_size",
        "uncompressed_size",
    }
    _require(
        set(metadata) == expected_keys,
        f"{source}: metadata keys must be exactly {sorted(expected_keys)}",
    )

    board = metadata["board"]
    _require(
        isinstance(board, str) and board in BOARDS,
        f"{source}: unsupported board {board!r}",
    )
    version = _validate_version(metadata["version"], str(source))
    image = metadata["image"]
    expected_image = f"snapdog-os-{board}-{version}.img.gz"
    _require(image == expected_image, f"{source}: image must be {expected_image!r}")
    sha256 = _validate_sha256(metadata["sha256"], f"{source}: sha256")
    _validate_sha256(metadata["raw_sha256"], f"{source}: raw_sha256")
    compressed_size = _validate_size(
        metadata["compressed_size"], f"{source}: compressed_size"
    )
    _validate_size(metadata["uncompressed_size"], f"{source}: uncompressed_size")

    if verify_compressed_image:
        artifact = source.parent / image
        actual_sha256, actual_size = _digest_file(artifact)
        _require(
            actual_sha256 == sha256, f"{source}: compressed image SHA-256 mismatch"
        )
        _require(
            actual_size == compressed_size, f"{source}: compressed image size mismatch"
        )
    return metadata


def _public_board_entry(
    metadata: dict[str, Any], *, channel: str, base_url: str
) -> dict[str, Any]:
    board = metadata["board"]
    return {
        # v1 fields: keep their names and semantics for existing consumers.
        "image": f"snapdog-os-{board}-{channel}.img.gz",
        "sha256": metadata["sha256"],
        # v2 fields: immutable download plus pre/decompression verification data.
        "url": f"{base_url}/{quote(metadata['image'])}",
        "compressed_size": metadata["compressed_size"],
        "uncompressed_size": metadata["uncompressed_size"],
        "raw_sha256": metadata["raw_sha256"],
    }


def create_manifest(
    *,
    channel: str,
    version: str,
    commit: str,
    date: str,
    base_url: str,
    metadata_paths: list[Path],
) -> dict[str, Any]:
    """Create a public v2 channel manifest from verified build metadata."""
    _require(channel in CHANNELS, f"unsupported channel {channel!r}")
    _validate_version(version, "manifest")
    _require(
        COMMIT_RE.fullmatch(commit) is not None,
        "manifest: commit must be a 40-digit SHA",
    )
    _validate_date(date)
    base_url = _validate_base_url(base_url)

    metadata_by_board: dict[str, dict[str, Any]] = {}
    for path in metadata_paths:
        metadata = _validate_board_metadata(
            _read_json(path), path, verify_compressed_image=True
        )
        board = metadata["board"]
        _require(
            board not in metadata_by_board, f"duplicate metadata for board {board}"
        )
        _require(
            metadata["version"] == version,
            f"{path}: version {metadata['version']!r} does not match manifest {version!r}",
        )
        metadata_by_board[board] = metadata

    _require(
        set(metadata_by_board) == set(BOARDS),
        f"manifest metadata must cover exactly {list(BOARDS)}",
    )
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "channel": channel,
        "version": version,
        "commit": commit,
        "date": date,
        "boards": {
            board: _public_board_entry(
                metadata_by_board[board], channel=channel, base_url=base_url
            )
            for board in BOARDS
        },
    }
    validate_manifest(manifest)
    return manifest


def validate_manifest(manifest: dict[str, Any]) -> None:
    """Validate the public v2 contract without rejecting future extra fields."""
    _require(
        manifest.get("schema_version") == SCHEMA_VERSION,
        "manifest: schema_version must be 2",
    )
    channel = manifest.get("channel")
    _require(channel in CHANNELS, f"manifest: unsupported channel {channel!r}")
    version = _validate_version(manifest.get("version"), "manifest")
    commit = manifest.get("commit")
    _require(
        isinstance(commit, str) and COMMIT_RE.fullmatch(commit) is not None,
        "manifest: commit must be a lowercase 40-digit SHA",
    )
    _validate_date(manifest.get("date"))

    boards = manifest.get("boards")
    _require(isinstance(boards, dict), "manifest: boards must be an object")
    _require(
        set(boards) == set(BOARDS), f"manifest: boards must be exactly {list(BOARDS)}"
    )
    for board in BOARDS:
        entry = boards[board]
        _require(isinstance(entry, dict), f"manifest: board {board} must be an object")
        expected_alias = f"snapdog-os-{board}-{channel}.img.gz"
        _require(
            entry.get("image") == expected_alias,
            f"manifest: {board}.image must be {expected_alias!r}",
        )
        _validate_sha256(entry.get("sha256"), f"manifest: {board}.sha256")
        _validate_sha256(entry.get("raw_sha256"), f"manifest: {board}.raw_sha256")
        _validate_size(
            entry.get("compressed_size"), f"manifest: {board}.compressed_size"
        )
        _validate_size(
            entry.get("uncompressed_size"), f"manifest: {board}.uncompressed_size"
        )

        url = entry.get("url")
        _require(isinstance(url, str), f"manifest: {board}.url must be a string")
        parsed = urlparse(url)
        _require(
            parsed.scheme == "https" and bool(parsed.netloc),
            f"manifest: {board}.url must use HTTPS",
        )
        _require(
            not parsed.params and not parsed.query and not parsed.fragment,
            f"manifest: {board}.url must not contain parameters, a query, or a fragment",
        )
        expected_versioned_image = f"snapdog-os-{board}-{version}.img.gz"
        _require(
            unquote(Path(parsed.path).name) == expected_versioned_image,
            f"manifest: {board}.url must reference immutable image {expected_versioned_image!r}",
        )


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    metadata = commands.add_parser(
        "board-metadata", help="hash and verify one raw/compressed image pair"
    )
    metadata.add_argument("--board", required=True, choices=BOARDS)
    metadata.add_argument("--version", required=True)
    metadata.add_argument("--raw-image", required=True, type=Path)
    metadata.add_argument("--compressed-image", required=True, type=Path)
    metadata.add_argument("--output", required=True, type=Path)

    generate = commands.add_parser(
        "manifest", help="generate a public channel manifest"
    )
    generate.add_argument("--channel", required=True, choices=CHANNELS)
    generate.add_argument("--version", required=True)
    generate.add_argument("--commit", required=True)
    generate.add_argument("--date", required=True)
    generate.add_argument("--base-url", required=True)
    generate.add_argument("--metadata", required=True, nargs="+", type=Path)
    generate.add_argument("--output", required=True, type=Path)

    validate = commands.add_parser(
        "validate", help="validate an existing public manifest"
    )
    validate.add_argument("--manifest", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv if argv is not None else sys.argv[1:])
    try:
        if args.command == "board-metadata":
            value = create_board_metadata(
                board=args.board,
                version=args.version,
                raw_image=args.raw_image,
                compressed_image=args.compressed_image,
            )
            _write_json(args.output, value)
        elif args.command == "manifest":
            value = create_manifest(
                channel=args.channel,
                version=args.version,
                commit=args.commit,
                date=args.date,
                base_url=args.base_url,
                metadata_paths=args.metadata,
            )
            _write_json(args.output, value)
        else:
            validate_manifest(_read_json(args.manifest))
    except ManifestError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
