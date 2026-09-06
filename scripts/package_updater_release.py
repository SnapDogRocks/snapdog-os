#!/usr/bin/env python3
"""Create reproducible snapdog-update release archives on every runner OS."""

from __future__ import annotations

import argparse
import gzip
import stat
import tarfile
import time
import zipfile
from pathlib import Path


ZIP_EPOCH = 315_532_800  # ZIP timestamps cannot predate 1980-01-01 UTC.


def _paths(source: Path) -> list[Path]:
    return [source, *sorted(source.rglob("*"), key=lambda path: path.as_posix())]


def create_tar(source: Path, output: Path, epoch: int) -> None:
    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=epoch) as compressed:
            with tarfile.open(
                fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT
            ) as archive:
                for path in _paths(source):
                    name = path.relative_to(source.parent).as_posix()
                    info = archive.gettarinfo(str(path), arcname=name)
                    info.uid = 0
                    info.gid = 0
                    info.uname = "root"
                    info.gname = "root"
                    info.mtime = epoch
                    if path.is_file():
                        with path.open("rb") as handle:
                            archive.addfile(info, handle)
                    else:
                        archive.addfile(info)


def create_zip(source: Path, output: Path, epoch: int) -> None:
    timestamp = tuple(time.gmtime(max(epoch, ZIP_EPOCH))[:6])
    with zipfile.ZipFile(
        output, mode="w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as archive:
        for path in _paths(source):
            relative = path.relative_to(source.parent).as_posix()
            is_directory = path.is_dir()
            name = f"{relative}/" if is_directory else relative
            info = zipfile.ZipInfo(name, date_time=timestamp)
            info.create_system = 3
            mode = stat.S_IMODE(path.stat().st_mode)
            kind = stat.S_IFDIR if is_directory else stat.S_IFREG
            info.external_attr = (kind | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, b"" if is_directory else path.read_bytes())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--format", choices=("tar", "zip"), required=True)
    parser.add_argument("--epoch", type=int, required=True)
    args = parser.parse_args()

    source = args.source.resolve()
    if not source.is_dir() or source.parent == source:
        parser.error("--source must be an archive directory")
    if args.epoch < 0:
        parser.error("--epoch must be non-negative")
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    if args.format == "tar":
        create_tar(source, output, args.epoch)
    else:
        create_zip(source, output, args.epoch)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
