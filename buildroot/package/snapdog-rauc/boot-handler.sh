#!/bin/sh
# RAUC custom bootloader backend for Raspberry Pi.
# Uses config.txt (primary) and tryboot.txt (one-shot trial boot).
# Slot state persisted in /data/rauc-slot-state.

set -eu

BOOT_MNT=/boot
STATE_FILE=/data/rauc-slot-state
CMDLINE="$BOOT_MNT/cmdline.txt"
TRYBOOT_CMDLINE="$BOOT_MNT/tryboot.txt"

# Map bootname (A/B) to partition device
slot_to_dev() {
  case "$1" in
    A) echo "/dev/mmcblk0p2" ;;
    B) echo "/dev/mmcblk0p3" ;;
    *) echo "unknown" ;;
  esac
}

# Parse root= from a cmdline file to get current bootname
cmdline_to_bootname() {
  ROOT=$(sed -n 's/.*root=\([^ ]*\).*/\1/p' "$1" 2>/dev/null)
  case "$ROOT" in
    */mmcblk0p2) echo "A" ;;
    */mmcblk0p3) echo "B" ;;
    *) echo "" ;;
  esac
}

# Remount boot rw, execute, remount ro
with_boot_rw() {
  mount -o remount,rw "$BOOT_MNT"
  "$@"
  sync
  mount -o remount,ro "$BOOT_MNT"
}

# Write cmdline pointing to given slot
write_cmdline() {
  local FILE="$1" SLOT="$2"
  local DEV
  DEV=$(slot_to_dev "$SLOT")
  sed "s|root=[^ ]*|root=$DEV|" "$CMDLINE" > "$FILE.tmp"
  mv "$FILE.tmp" "$FILE"
}

get_primary() {
  cmdline_to_bootname "$CMDLINE"
}

set_primary() {
  local SLOT="$1"
  with_boot_rw write_cmdline "$CMDLINE" "$SLOT"
  # Remove tryboot if it exists (promotion)
  [ -f "$TRYBOOT_CMDLINE" ] && with_boot_rw rm -f "$TRYBOOT_CMDLINE"
}

get_state() {
  local SLOT="$1"
  if [ -f "$STATE_FILE" ]; then
    grep "^${SLOT}=" "$STATE_FILE" 2>/dev/null | cut -d= -f2 || echo "good"
  else
    echo "good"
  fi
}

set_state() {
  local SLOT="$1" STATE="$2"
  mkdir -p "$(dirname "$STATE_FILE")"
  if [ -f "$STATE_FILE" ]; then
    sed -i "/^${SLOT}=/d" "$STATE_FILE"
  fi
  echo "${SLOT}=${STATE}" >> "$STATE_FILE"
}

get_current() {
  # Read from /proc/cmdline what we actually booted
  ROOT=$(sed -n 's/.*root=\([^ ]*\).*/\1/p' /proc/cmdline)
  case "$ROOT" in
    */mmcblk0p2|*/vda2) echo "A" ;;
    */mmcblk0p3|*/vda3) echo "B" ;;
    *) echo "" ;;
  esac
}

case "${1:-}" in
  get-primary)    get_primary ;;
  set-primary)    set_primary "$2" ;;
  get-state)      get_state "$2" ;;
  set-state)      set_state "$2" "$3" ;;
  get-current)    get_current ;;
  *)              echo "Unknown command: $1" >&2; exit 1 ;;
esac
