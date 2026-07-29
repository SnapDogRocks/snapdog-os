import grp
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
PACKAGE = ROOT / "buildroot/package/snapdog-server"
UNIT = PACKAGE / "snapdog.service"
CTRL_UNIT = ROOT / "buildroot/package/snapdog-ctrl/snapdog-ctrl.service"
DATA_INIT = PACKAGE / "snapdog-data-init"
RUN_GUARD = PACKAGE / "snapdog-run"
PACKAGE_MK = PACKAGE / "snapdog-server.mk"
DEFAULT_CONFIG = PACKAGE / "snapdog.toml"


def portable_data_init(data: Path) -> str:
    """Adapt the root/group operations for an unprivileged host test run."""
    group = grp.getgrgid(os.getgid()).gr_name
    script = (
        DATA_INIT.read_text()
        .replace("DATA=/data", f"DATA={shlex.quote(str(data))}", 1)
        .replace("VALIDATION_OWNER_UID=0", f"VALIDATION_OWNER_UID={os.getuid()}", 1)
        .replace("snapdog-state", group)
        .replace(
            'chown "$managed_owner" "$managed_file"',
            f'chgrp {shlex.quote(group)} "$managed_file"',
        )
        .replace(f"chown root:{group}", f"chgrp {shlex.quote(group)}")
    )
    if sys.platform == "darwin":
        script = script.replace("stat -c %u", "stat -f %u")
        script = script.replace("stat -c %h", "stat -f %l")
    return script


class SnapdogServerContractTests(unittest.TestCase):
    def test_ctrl_cannot_manage_server_data_before_data_init_succeeds(self):
        unit = CTRL_UNIT.read_text()

        self.assertIn("Requires=snapdog-data-init.service", unit)
        self.assertRegex(
            unit,
            r"(?m)^After=.*\bsnapdog-data-init\.service\b",
        )

    def test_unit_has_bounded_and_diagnosable_lifecycle(self):
        unit = UNIT.read_text()

        self.assertIn("Requires=snapdog-data-init.service", unit)
        self.assertRegex(
            unit,
            r"(?m)^After=.*\bsnapdog-data-init\.service\b",
        )
        self.assertIn("ExecStart=/usr/libexec/snapdog-run", unit)
        self.assertIn("Restart=on-failure", unit)
        self.assertIn("RestartPreventExitStatus=78", unit)
        self.assertIn("StartLimitIntervalSec=120", unit)
        self.assertIn("StartLimitBurst=4", unit)

    def test_unit_preserves_dynamic_user_and_read_only_rootfs_contract(self):
        unit = UNIT.read_text()

        self.assertIn("DynamicUser=yes", unit)
        self.assertIn("SupplementaryGroups=audio snapdog-state", unit)
        self.assertIn("UMask=0007", unit)
        self.assertIn("ProtectSystem=strict", unit)
        self.assertIn("NoNewPrivileges=yes", unit)
        self.assertIn("ReadWritePaths=/data/snapdog/state", unit)
        self.assertNotIn("StateDirectory=", unit)

    def test_persistent_state_uses_a_stable_private_group(self):
        package_mk = PACKAGE_MK.read_text()
        data_init = DATA_INIT.read_text()

        self.assertRegex(
            package_mk,
            r"(?m)^\s*-\s+-\s+snapdog-state\s+2002\s+",
        )
        self.assertRegex(
            package_mk,
            r"(?m)^\s*\$\(INSTALL\) -D -m 0755 .*snapdog-run",
        )
        self.assertIn("$(TARGET_DIR)/usr/libexec/snapdog-run", package_mk)
        self.assertNotRegex(data_init, r"(?m)^\s*chmod\s+0?777\b")
        self.assertIn('chown root:snapdog-state "$SNAPDOG_DIR"', data_init)
        self.assertIn('chmod 2750 "$SNAPDOG_DIR"', data_init)
        self.assertIn('chown root:snapdog-state "$STATE_DIR"', data_init)
        self.assertIn(
            'find "$STATE_DIR" -type d -exec chmod 2770 {} \\;',
            data_init,
        )

        gid_definitions = []
        gid_pattern = re.compile(r"(?m)^\s*\S+\s+\S+\s+\S+\s+2002\s+")
        for makefile in (ROOT / "buildroot/package").glob("*/*.mk"):
            if gid_pattern.search(makefile.read_text()):
                gid_definitions.append(makefile)
        self.assertEqual(gid_definitions, [PACKAGE_MK])
        self.assertIn(
            'find "$STATE_DIR" -type f -exec chmod 0660 {} \\;',
            data_init,
        )

    def test_data_init_normalizes_managed_files_and_rejects_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            data = temp / "data"
            snapdog = data / "snapdog"
            snapdog.mkdir(parents=True)
            ctrl = snapdog / "ctrl.toml"
            active = snapdog / "snapdog.toml"
            candidate = snapdog / ".snapdog.toml.candidate"
            previous = snapdog / ".snapdog.toml.previous"
            last_issue = snapdog / "server-last-issue.json"
            ctrl.write_text("[services]\nserver = true\n")
            active.write_text("[[zone]]\nname = 'Living'\n")
            candidate.write_text(active.read_text())
            previous.write_text(active.read_text())
            last_issue.write_text('{"issue":{}}')
            for managed_file in (ctrl, active, candidate, previous, last_issue):
                managed_file.chmod(0o666)

            test_script = temp / "snapdog-data-init"
            test_script.write_text(portable_data_init(data))
            result = subprocess.run(
                ["sh", test_script],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(ctrl.stat().st_mode & 0o777, 0o600)
            self.assertEqual(active.stat().st_mode & 0o777, 0o640)
            self.assertEqual(candidate.stat().st_mode & 0o777, 0o640)
            self.assertEqual(previous.stat().st_mode & 0o777, 0o600)
            self.assertEqual(last_issue.stat().st_mode & 0o777, 0o600)

            active.unlink()
            outside = temp / "outside"
            outside.write_text("do not follow")
            active.symlink_to(outside)
            result = subprocess.run(
                ["sh", test_script],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing symlinked managed file", result.stderr)
            self.assertEqual(outside.read_text(), "do not follow")

    def test_data_init_removes_only_safe_import_validation_crash_files(self):
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            data = temp / "data"
            snapdog = data / "snapdog"
            snapdog.mkdir(parents=True)
            stale = snapdog / ".settings-import-validation.123.456.toml"
            malformed = snapdog / ".settings-import-validation.pid.456.toml"
            outside = temp / "outside-secret"
            linked = snapdog / ".settings-import-validation.789.1.toml"
            stale.write_text("secret")
            malformed.write_text("keep")
            outside.write_text("outside")
            linked.symlink_to(outside)

            test_script = temp / "snapdog-data-init"
            test_script.write_text(portable_data_init(data))
            result = subprocess.run(
                ["sh", test_script],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(stale.exists())
            self.assertTrue(malformed.exists())
            self.assertTrue(linked.is_symlink())
            self.assertEqual(outside.read_text(), "outside")

    def test_state_path_symlink_is_rejected_before_creation_or_chown(self):
        data_init = DATA_INIT.read_text()
        symlink_check = 'if [ -L "$STATE_DIR" ]; then'

        self.assertIn(symlink_check, data_init)
        self.assertLess(data_init.index(symlink_check), data_init.index('mkdir -p "$STATE_DIR"'))
        self.assertLess(
            data_init.index(symlink_check),
            data_init.index('chown root:snapdog-state "$STATE_DIR"'),
        )

        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            data = temp / "data"
            snapdog = data / "snapdog"
            outside = temp / "outside"
            snapdog.mkdir(parents=True)
            outside.mkdir()
            (snapdog / "state").symlink_to(outside, target_is_directory=True)

            test_script = temp / "snapdog-data-init"
            test_script.write_text(portable_data_init(data))
            result = subprocess.run(
                ["sh", test_script],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing symlinked state directory", result.stderr)

    def test_snapdog_parent_symlink_is_rejected_before_mkdir(self):
        data_init = DATA_INIT.read_text()
        symlink_check = 'if [ -L "$SNAPDOG_DIR" ]; then'

        self.assertIn(symlink_check, data_init)
        self.assertLess(
            data_init.index(symlink_check),
            data_init.index('mkdir -p "$SNAPDOG_DIR"'),
        )

        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            data = temp / "data"
            outside = temp / "outside"
            data.mkdir()
            outside.mkdir()
            (data / "snapdog").symlink_to(outside, target_is_directory=True)

            test_script = temp / "snapdog-data-init"
            test_script.write_text(portable_data_init(data))
            result = subprocess.run(
                ["sh", test_script],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing symlinked data directory", result.stderr)

    def test_data_init_does_not_follow_nested_state_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            data = temp / "data"
            state = data / "snapdog/state"
            inside_file = state / "zones.json"
            outside = temp / "outside"
            outside_file = outside / "must-not-change"
            state.mkdir(parents=True)
            outside.mkdir()
            inside_file.write_text("{}")
            outside_file.write_text("outside")
            outside.chmod(0o711)
            outside_file.chmod(0o604)
            (state / "external").symlink_to(outside, target_is_directory=True)

            test_script_source = portable_data_init(data)
            test_script = temp / "snapdog-data-init"
            test_script.write_text(test_script_source)

            result = subprocess.run(
                ["sh", test_script],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((data / "snapdog").stat().st_mode & 0o7777, 0o2750)
            self.assertEqual((data / "snapdog").stat().st_gid, os.getgid())
            self.assertEqual(inside_file.stat().st_mode & 0o7777, 0o660)
            self.assertTrue((state / "external").is_symlink())
            self.assertEqual(outside.stat().st_mode & 0o7777, 0o711)
            self.assertEqual(outside_file.stat().st_mode & 0o7777, 0o604)

    def test_default_config_matches_the_server_runtime_contract(self):
        config = tomllib.loads(DEFAULT_CONFIG.read_text())
        self.assertEqual(config["system"]["state_dir"], "/data/snapdog/state")
        self.assertEqual(config["airplay"]["mode"], "airplay2")

    def test_server_scripts_are_posix_shell_syntax_clean(self):
        for script in (DATA_INIT, RUN_GUARD):
            with self.subTest(script=script.name):
                subprocess.run(["sh", "-n", script], check=True)

    def test_guard_maps_only_snapdog_config_rejection_to_exit_78(self):
        self.assertIn("SNAPDOG_SERVER_VERSION = 0.27.0", PACKAGE_MK.read_text())

        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            fake_snapdog = temp / "snapdog"
            config = temp / "snapdog.toml"
            marker = temp / "runtime-started"
            guard = temp / "snapdog-run"

            fake_snapdog.write_text(
                "#!/bin/sh\n"
                'if [ "${3:-}" = "--check-config" ]; then\n'
                '  exit "${CHECK_STATUS:-0}"\n'
                "fi\n"
                'printf "%s\\n" "$$" > "$RUN_MARKER"\n'
                'exit "${RUN_STATUS:-0}"\n'
            )
            fake_snapdog.chmod(0o755)
            config.write_text("[system]\n")

            guard_source = RUN_GUARD.read_text()
            guard_source = guard_source.replace(
                "SNAPDOG=/usr/bin/snapdog",
                f"SNAPDOG={shlex.quote(str(fake_snapdog))}",
            ).replace(
                "CONFIG=/etc/snapdog/snapdog.toml",
                f"CONFIG={shlex.quote(str(config))}",
            )
            guard.write_text(guard_source)

            environment = {**os.environ, "RUN_MARKER": str(marker)}

            rejected = subprocess.run(
                ["sh", guard],
                env={**environment, "CHECK_STATUS": "1"},
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(rejected.returncode, 78)
            self.assertIn("configuration rejected", rejected.stderr)
            self.assertFalse(marker.exists())

            infrastructure_error = subprocess.run(
                ["sh", guard],
                env={**environment, "CHECK_STATUS": "42"},
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(infrastructure_error.returncode, 42)
            self.assertFalse(marker.exists())

            runtime_failure = subprocess.run(
                ["sh", guard],
                env={**environment, "CHECK_STATUS": "0", "RUN_STATUS": "23"},
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(runtime_failure.returncode, 23)
            self.assertTrue(marker.exists())


if __name__ == "__main__":
    unittest.main()
