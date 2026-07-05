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
mkdir -p "$TARGET_DIR/etc/wpa_supplicant"
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

rm -f "$TARGET_DIR/etc/systemd/system/updater.timer.d/schedule.conf"
ln -sf /data/systemd/system/updater.timer.d/schedule.conf "$TARGET_DIR/etc/systemd/system/updater.timer.d/schedule.conf"

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

# Disable services managed by snapdog-ctrl (it starts them based on config)
ln -sf /dev/null "$TARGET_DIR/etc/systemd/system/sshd.service"
ln -sf /dev/null "$TARGET_DIR/etc/systemd/system/ssh-access.target"
