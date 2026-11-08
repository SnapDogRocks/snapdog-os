from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SERVER_CONFIG = ROOT / "snapdog-ctrl" / "src" / "server_config.rs"
CTRL_SERVICE = ROOT / "buildroot" / "package" / "snapdog-ctrl" / "snapdog-ctrl.service"
POST_BUILD = ROOT / "buildroot" / "board" / "raspberrypi" / "post-build.sh"
FSTAB = ROOT / "buildroot" / "board" / "raspberrypi" / "overlay" / "etc" / "fstab"


def rust_const(source: str, name: str) -> str:
    match = re.search(rf'const {name}: &str = "([^"]+)";', source)
    if match is None:
        raise AssertionError(f"{name} not found in server_config.rs")
    return match.group(1)


class ServerConfigPathTests(unittest.TestCase):
    """The server configuration is saved on the data partition, not below the read-only `/etc`.

    `/` is mounted read-only and snapdog-ctrl runs with `ProtectSystem=full`, so a write to
    `/etc/snapdog/...` fails with EROFS. An atomic write also creates its temporary file next to
    the target, which makes a write through the `/etc/snapdog/snapdog.toml` symlink fail as well.
    """

    def setUp(self) -> None:
        self.source = SERVER_CONFIG.read_text()

    def test_every_file_the_save_writes_is_on_the_data_partition(self) -> None:
        for name in ("CONFIG_PATH", "CONFIG_BACKUP", "CONFIG_CANDIDATE", "CONFIG_DIR"):
            with self.subTest(constant=name):
                self.assertTrue(
                    rust_const(self.source, name).startswith("/data/snapdog"),
                    f"{name} must be below /data/snapdog",
                )

    def test_the_save_never_names_etc(self) -> None:
        code = "\n".join(
            line for line in self.source.splitlines() if not line.lstrip().startswith("//")
        )
        self.assertNotIn('"/etc/', code)

    def test_the_config_path_is_what_the_etc_symlink_points_at(self) -> None:
        post_build = POST_BUILD.read_text()
        self.assertIn(
            f'ln -sf {rust_const(self.source, "CONFIG_PATH")} '
            '"$TARGET_DIR/etc/snapdog/snapdog.toml"',
            post_build,
        )

    def test_the_service_is_allowed_to_write_the_data_partition(self) -> None:
        service = CTRL_SERVICE.read_text()
        paths = re.search(r"^ReadWritePaths=(.*)$", service, re.MULTILINE)
        self.assertIsNotNone(paths)
        self.assertIn("/data", paths.group(1).split())

    def test_the_root_file_system_is_read_only(self) -> None:
        """If this changes, the paths above may go back to `/etc`; until then they may not."""
        self.assertRegex(FSTAB.read_text(), r"(?m)^/dev/root\s+/\s+ext4\s+ro[,\s]")


if __name__ == "__main__":
    unittest.main()
