from __future__ import annotations

import hashlib
import os
import stat
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

from scripts.package_updater_release import create_tar, create_zip


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class ReproducibleUpdaterArchiveTests(unittest.TestCase):
    def fixture(self, directory: str) -> Path:
        source = Path(directory) / "snapdog-update-v1.2.3-target"
        source.mkdir()
        binary = source / "snapdog-update"
        binary.write_bytes(b"binary\x00payload")
        binary.chmod(0o755)
        (source / "README.md").write_text("hello\n", encoding="utf-8")
        return source

    def test_tar_is_reproducible_and_preserves_executable_mode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = self.fixture(directory)
            first = Path(directory) / "first.tar.gz"
            second = Path(directory) / "second.tar.gz"
            create_tar(source, first, 1_700_000_000)
            os.utime(source / "snapdog-update", (1_800_000_000, 1_800_000_000))
            create_tar(source, second, 1_700_000_000)

            self.assertEqual(digest(first), digest(second))
            with tarfile.open(first, "r:gz") as archive:
                member = archive.getmember(f"{source.name}/snapdog-update")
                self.assertTrue(member.mode & stat.S_IXUSR)

    def test_zip_is_reproducible_and_preserves_executable_mode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = self.fixture(directory)
            first = Path(directory) / "first.zip"
            second = Path(directory) / "second.zip"
            create_zip(source, first, 1_700_000_000)
            os.utime(source / "snapdog-update", (1_800_000_000, 1_800_000_000))
            create_zip(source, second, 1_700_000_000)

            self.assertEqual(digest(first), digest(second))
            with zipfile.ZipFile(first) as archive:
                info = archive.getinfo(f"{source.name}/snapdog-update")
                mode = info.external_attr >> 16
                self.assertTrue(mode & stat.S_IXUSR)


if __name__ == "__main__":
    unittest.main()
