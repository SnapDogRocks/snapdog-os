// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder
//
// Reboots the Raspberry Pi into one-shot tryboot mode.
//
// systemd on this image exposes no reboot-parameter mechanism (Manager.Reboot
// takes no argument, there is no SetRebootParameter), so the RPi tryboot flag
// cannot be set via systemctl. This helper issues the reboot(2) RESTART2 syscall
// directly with the "0 tryboot" magic string, which the firmware consumes to boot
// tryboot.txt for exactly one boot. Requires CAP_SYS_BOOT.
#include <sys/syscall.h>
#include <linux/reboot.h>
#include <unistd.h>

int main(void) {
    sync();
    syscall(SYS_reboot, LINUX_REBOOT_MAGIC1, LINUX_REBOOT_MAGIC2,
            LINUX_REBOOT_CMD_RESTART2, "0 tryboot");
    return 1; // only reached if the syscall fails (e.g. missing CAP_SYS_BOOT)
}
