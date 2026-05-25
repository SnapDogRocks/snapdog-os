#!/bin/sh
# post-build.sh — Runs after Buildroot target-finalize, before image creation.
# Creates persistent symlinks from /etc → /data for mutable config files.

set -eu

TARGET_DIR=$1

# Ensure mountpoints exist
mkdir -p "$TARGET_DIR/data"
mkdir -p "$TARGET_DIR/boot"

# Ensure parent directories exist
mkdir -p "$TARGET_DIR/etc/default"
mkdir -p "$TARGET_DIR/etc/snapdog"
mkdir -p "$TARGET_DIR/etc/wpa_supplicant"
mkdir -p "$TARGET_DIR/etc/hostapd"
mkdir -p "$TARGET_DIR/etc/dnsmasq.d"
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

rm -f "$TARGET_DIR/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"
ln -sf /data/wpa_supplicant/wpa_supplicant-wlan0.conf "$TARGET_DIR/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"

rm -f "$TARGET_DIR/etc/hostapd/hostapd.conf"
ln -sf /data/hostapd/hostapd.conf "$TARGET_DIR/etc/hostapd/hostapd.conf"

rm -f "$TARGET_DIR/etc/dnsmasq.d/snapdog-ap.conf"
ln -sf /data/dnsmasq.d/snapdog-ap.conf "$TARGET_DIR/etc/dnsmasq.d/snapdog-ap.conf"

rm -f "$TARGET_DIR/etc/systemd/resolved.conf.d/snapdog.conf"
ln -sf /data/systemd/resolved.conf.d/snapdog.conf "$TARGET_DIR/etc/systemd/resolved.conf.d/snapdog.conf"

rm -f "$TARGET_DIR/etc/systemd/system/updater.timer.d/schedule.conf"
ln -sf /data/systemd/system/updater.timer.d/schedule.conf "$TARGET_DIR/etc/systemd/system/updater.timer.d/schedule.conf"

rm -f "$TARGET_DIR/etc/snapdog-os.channel"
ln -sf /data/snapdog-os.channel "$TARGET_DIR/etc/snapdog-os.channel"

rm -f "$TARGET_DIR/etc/snapdog-os.auto-update"
ln -sf /data/snapdog-os.auto-update "$TARGET_DIR/etc/snapdog-os.auto-update"

rm -f "$TARGET_DIR/etc/hostname"
ln -sf /data/hostname "$TARGET_DIR/etc/hostname"

rm -rf "$TARGET_DIR/root/.ssh"
ln -sf /data/ssh "$TARGET_DIR/root/.ssh"

# First-boot marker: triggers partition resize + format
touch "$TARGET_DIR/resize-me"
