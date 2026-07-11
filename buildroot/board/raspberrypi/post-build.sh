#!/bin/sh
# post-build.sh — Runs after Buildroot target-finalize, before image creation.
# Creates persistent symlinks from /etc → /data for mutable config files.

set -eu

TARGET_DIR=$1

# Ensure mountpoints exist
mkdir -p "$TARGET_DIR/data"
mkdir -p "$TARGET_DIR/boot"
mkdir -p "$TARGET_DIR/var/empty"

# Ensure parent directories exist
mkdir -p "$TARGET_DIR/etc/default"
mkdir -p "$TARGET_DIR/etc/snapdog"
# NB: /etc/wpa_supplicant is intentionally not created here — it is replaced by a
# symlink to /data below. Creating it would break idempotent (incremental)
# rebuilds: `mkdir -p` fails on the already-existing dangling symlink under set -e.
mkdir -p "$TARGET_DIR/etc/hostapd"
mkdir -p "$TARGET_DIR/etc/systemd/resolved.conf.d"
mkdir -p "$TARGET_DIR/etc/systemd/system/updater.timer.d"
mkdir -p "$TARGET_DIR/root"

# Replace files/dirs with symlinks to /data
rm -rf "$TARGET_DIR/etc/systemd/network"
ln -sf /data/systemd/network "$TARGET_DIR/etc/systemd/network"

rm -f "$TARGET_DIR/etc/default/snapdog-client"
ln -sf /data/default/snapdog-client "$TARGET_DIR/etc/default/snapdog-client"

rm -f "$TARGET_DIR/etc/snapdog/snapdog.toml"
ln -sf /data/snapdog/snapdog.toml "$TARGET_DIR/etc/snapdog/snapdog.toml"

rm -rf "$TARGET_DIR/etc/wpa_supplicant"
ln -sf /data/wpa_supplicant "$TARGET_DIR/etc/wpa_supplicant"

rm -f "$TARGET_DIR/etc/hostapd/hostapd.conf"
ln -sf /data/hostapd/hostapd.conf "$TARGET_DIR/etc/hostapd/hostapd.conf"

rm -f "$TARGET_DIR/etc/systemd/resolved.conf.d/snapdog.conf"
ln -sf /data/systemd/resolved.conf.d/snapdog.conf "$TARGET_DIR/etc/systemd/resolved.conf.d/snapdog.conf"

# The "Exclusive Audio Core" tuning writes a CPUAffinity drop-in here at runtime;
# the rootfs is read-only, so back the whole drop-in dir with writable /data (also
# survives OS updates that replace the rootfs slot). snapdog-data-init seeds the dir.
rm -rf "$TARGET_DIR/etc/systemd/system/snapdog-client.service.d"
ln -sf /data/systemd/system/snapdog-client.service.d "$TARGET_DIR/etc/systemd/system/snapdog-client.service.d"

rm -f "$TARGET_DIR/etc/snapdog-os.channel"
ln -sf /data/snapdog-os.channel "$TARGET_DIR/etc/snapdog-os.channel"

rm -f "$TARGET_DIR/etc/snapdog-os.auto-update"
ln -sf /data/snapdog-os.auto-update "$TARGET_DIR/etc/snapdog-os.auto-update"

rm -f "$TARGET_DIR/etc/localtime"
ln -sf /data/localtime "$TARGET_DIR/etc/localtime"

rm -f "$TARGET_DIR/etc/hostname"
ln -sf /data/hostname "$TARGET_DIR/etc/hostname"

rm -rf "$TARGET_DIR/root/.ssh"
ln -sf /data/ssh "$TARGET_DIR/root/.ssh"

# Substitute boot partition device in fstab
if [ -n "${SNAPDOG_ROOT_DEV:-}" ]; then
  sed -i "s|/dev/mmcblk0p1|${SNAPDOG_ROOT_DEV}1|" "$TARGET_DIR/etc/fstab"
fi

# First-boot marker: triggers partition resize + format
touch "$TARGET_DIR/resize-me"

# Write OS version (baked into image, read-only)
VERSION_FILE="${BR2_EXTERNAL_SNAPDOG_PATH:-.}/VERSION"
if [ -f "$VERSION_FILE" ]; then
  cp "$VERSION_FILE" "$TARGET_DIR/etc/snapdog-os.version"
else
  echo "unknown" > "$TARGET_DIR/etc/snapdog-os.version"
fi

# Enable serial console on USB gadget
mkdir -p "$TARGET_DIR/etc/systemd/system/getty.target.wants"
ln -sf /usr/lib/systemd/system/serial-getty@.service \
  "$TARGET_DIR/etc/systemd/system/getty.target.wants/serial-getty@ttyGS0.service"

# Mask wait-online (no service needs network-online.target; AP mode is intentionally offline)
ln -sf /dev/null "$TARGET_DIR/etc/systemd/system/systemd-networkd-wait-online.service"

# SSH on a read-only rootfs: it cannot be mask/unmasked at runtime, and sshd's
# ExecStartPre `ssh-keygen -A` cannot write host keys to read-only /etc/ssh.
# So gate sshd on a writable flag (/data/ssh.enabled) that snapdog-ctrl toggles,
# and keep host keys on writable /data (persisted across reflash). Off by
# default: with no flag, ConditionPathExists skips the unit even if it is pulled
# in at boot.
mkdir -p "$TARGET_DIR/etc/systemd/system/sshd.service.d"
cat > "$TARGET_DIR/etc/systemd/system/sshd.service.d/10-snapdog.conf" <<'SSHD_DROPIN'
[Unit]
ConditionPathExists=/data/ssh.enabled

[Service]
# Generate and load host keys from writable /data (vendor ExecStartPre writes to
# read-only /etc/ssh and fails). `ssh-keygen -A -f /data` emits /data/etc/ssh/*.
ExecStartPre=
ExecStartPre=/bin/mkdir -p /data/etc/ssh
ExecStartPre=/usr/bin/ssh-keygen -A -f /data
ExecStart=
ExecStart=/usr/sbin/sshd -D -e -h /data/etc/ssh/ssh_host_rsa_key -h /data/etc/ssh/ssh_host_ecdsa_key -h /data/etc/ssh/ssh_host_ed25519_key
SSHD_DROPIN
