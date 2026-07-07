#!/bin/sh
# RAUC custom bootloader backend for Raspberry Pi, with fail-safe tryboot rollback.
#
# HOW ROLLBACK WORKS (fail-safe by construction):
#   * /boot/cmdline.txt always selects the COMMITTED (last-known-good) slot. A
#     NORMAL boot reads config.txt -> cmdline.txt, so it ALWAYS lands on the
#     committed slot. Nothing in the trial path can change that.
#   * Installing a bundle arms a TRIAL: set-primary writes a trial cmdline
#     (/boot/tryboot-cmdline.txt for the new slot) and a /boot/tryboot.txt (a copy
#     of the live config.txt whose only change is `cmdline=tryboot-cmdline.txt`).
#     The RPi firmware reads tryboot.txt INSTEAD of config.txt for exactly ONE boot
#     when the one-shot tryboot flag is set (RESTART2 "0 tryboot", see the reboot
#     helper). cmdline.txt is left untouched.
#   * If the trial boots healthily, snapdog-ctrl comes up and rauc-mark-good runs
#     `boot-handler.sh commit`, which promotes the trial (cmdline.txt <-
#     tryboot-cmdline.txt) and removes the trial files. If the trial does NOT come
#     up, nothing commits: the next (normal) boot reads cmdline.txt and lands back
#     on the OLD committed slot automatically; commit then cleans up the stale
#     trial files and marks the failed slot bad. The systemd watchdog (30s) turns
#     a hang into that next normal boot.
#   * No boot counter, no separate fallback file: cmdline.txt IS the committed
#     state, only ever rewritten on a proven-good commit.
#
# Slot good/bad state persisted in /data/rauc-slot-state.
set -eu

BOOT_MNT=/boot
STATE_FILE=/data/rauc-slot-state
CMDLINE="$BOOT_MNT/cmdline.txt"
CONFIG_TXT="$BOOT_MNT/config.txt"
TRYBOOT_CMDLINE="$BOOT_MNT/tryboot-cmdline.txt"
TRYBOOT_TXT="$BOOT_MNT/tryboot.txt"

# Detect root device base from the committed cmdline (e.g. /dev/mmcblk0p).
detect_root_base() {
  ROOT=$(sed -n 's/.*root=\([^ ]*\).*/\1/p' "$CMDLINE" 2>/dev/null || true)
  [ -n "$ROOT" ] || ROOT=$(sed -n 's/.*root=\([^ ]*\).*/\1/p' /proc/cmdline)
  echo "${ROOT%[0-9]}"
}

# Map bootname (A/B) to partition device, or "unknown".
slot_to_dev() {
  BASE=$(detect_root_base)
  case "$1" in
    A) echo "${BASE}2" ;;
    B) echo "${BASE}3" ;;
    *) echo "unknown" ;;
  esac
}

# Parse root= from a cmdline file to get its bootname (A/B), or "".
cmdline_to_bootname() {
  ROOT=$(sed -n 's/.*root=\([^ ]*\).*/\1/p' "$1" 2>/dev/null || true)
  case "$ROOT" in
    *2) echo "A" ;;
    *3) echo "B" ;;
    *) echo "" ;;
  esac
}

# Run a command with /boot mounted read-write, then ALWAYS restore read-only —
# even on error — so a failure never leaves the shared FAT writable. Refuses if
# /boot is not actually a mountpoint (guards against writing into the rootfs).
with_boot_rw() {
  if ! mountpoint -q "$BOOT_MNT"; then
    echo "boot-handler: $BOOT_MNT is not mounted; refusing to write" >&2
    return 1
  fi
  mount -o remount,rw "$BOOT_MNT"
  trap 'sync; mount -o remount,ro "$BOOT_MNT" 2>/dev/null || true; trap - EXIT INT TERM' EXIT INT TERM
  "$@"
  rc=$?
  sync
  mount -o remount,ro "$BOOT_MNT" 2>/dev/null || true
  trap - EXIT INT TERM
  return $rc
}

# Write a cmdline file for SLOT, based on the committed cmdline as a template so
# console/serial args are preserved. Validates the result before replacing it
# atomically (tmp on the same FAT + rename); fails closed on a bad slot/token.
write_cmdline() {
  FILE="$1"; SLOT="$2"
  DEV=$(slot_to_dev "$SLOT")
  case "$DEV" in
    /dev/*) : ;;
    *) echo "boot-handler: refusing to write cmdline for slot '$SLOT' (dev='$DEV')" >&2; return 1 ;;
  esac
  sed -e "s|root=[^ ]*|root=$DEV|" -e "s|rauc\.slot=[^ ]*|rauc.slot=$SLOT|" \
    "$CMDLINE" > "$FILE.tmp"
  if [ "$(grep -c "root=$DEV" "$FILE.tmp")" != "1" ] || \
     [ "$(grep -c "rauc.slot=$SLOT" "$FILE.tmp")" != "1" ]; then
    echo "boot-handler: refusing malformed cmdline for slot $SLOT" >&2
    rm -f "$FILE.tmp"; return 1
  fi
  mv "$FILE.tmp" "$FILE"
}

# Generate tryboot.txt = the live config.txt with its kernel command line pointed
# at the trial cmdline. The firmware reads THIS instead of config.txt for the
# one-shot tryboot; config.txt/cmdline.txt (the committed slot) stay untouched.
gen_tryboot_txt() {
  grep -v '^[[:space:]]*cmdline=' "$CONFIG_TXT" > "$TRYBOOT_TXT.tmp"
  printf 'cmdline=%s\n' "$(basename "$TRYBOOT_CMDLINE")" >> "$TRYBOOT_TXT.tmp"
  mv "$TRYBOOT_TXT.tmp" "$TRYBOOT_TXT"
}

# Arm a trial for SLOT (runs under with_boot_rw). cmdline.txt is left untouched.
arm_trial() {
  write_cmdline "$TRYBOOT_CMDLINE" "$1" || return 1
  gen_tryboot_txt
}

# RAUC's notion of the slot that boots next: a pending trial if one is armed.
get_primary() {
  if [ -f "$TRYBOOT_TXT" ]; then
    cmdline_to_bootname "$TRYBOOT_CMDLINE"
  else
    cmdline_to_bootname "$CMDLINE"
  fi
}

# RAUC calls this to make the freshly-installed inactive slot bootable -> arm it
# as a tryboot trial. Fails the install if arming can't be done safely.
set_primary() {
  with_boot_rw arm_trial "$1"
}

get_state() {
  if [ -f "$STATE_FILE" ]; then
    VAL=$(sed -n "s/^$1=//p" "$STATE_FILE" 2>/dev/null | head -n1)
    [ -n "$VAL" ] && echo "$VAL" || echo "good"
  else
    echo "good"
  fi
}

set_state() {
  mkdir -p "$(dirname "$STATE_FILE")"
  [ -f "$STATE_FILE" ] && sed -i "/^$1=/d" "$STATE_FILE"
  echo "$1=$2" >> "$STATE_FILE"
}

get_current() {
  ROOT=$(sed -n 's/.*root=\([^ ]*\).*/\1/p' /proc/cmdline)
  case "$ROOT" in
    *2) echo "A" ;;
    *3) echo "B" ;;
    *) echo "" ;;
  esac
}

# Reconcile a pending trial against the slot we actually booted. Runs on a good
# boot (rauc-mark-good, only after snapdog-ctrl is up):
#   - booted == trial -> trial came up healthy: COMMIT (cmdline.txt <-
#     tryboot-cmdline.txt) and drop the trial files.
#   - booted != trial -> trial failed and we auto-reverted to the committed slot:
#     drop the stale trial files and mark the failed slot bad.
commit() {
  [ -f "$TRYBOOT_TXT" ] || [ -f "$TRYBOOT_CMDLINE" ] || return 0
  BOOTED=$(get_current)
  TRIAL=$(cmdline_to_bootname "$TRYBOOT_CMDLINE")
  if [ "$BOOTED" = "$TRIAL" ] && [ -n "$BOOTED" ]; then
    echo "boot-handler: trial slot $TRIAL booted healthy — committing"
    with_boot_rw sh -c "cp '$TRYBOOT_CMDLINE' '$CMDLINE' && rm -f '$TRYBOOT_TXT' '$TRYBOOT_CMDLINE'"
  else
    echo "boot-handler: trial slot $TRIAL failed; reverted to $BOOTED — cleaning up + marking $TRIAL bad"
    [ -n "$TRIAL" ] && set_state "$TRIAL" "bad"
    with_boot_rw rm -f "$TRYBOOT_TXT" "$TRYBOOT_CMDLINE"
  fi
}

case "${1:-}" in
  get-primary)    get_primary ;;
  set-primary)    set_primary "$2" ;;
  get-state)      get_state "$2" ;;
  set-state)      set_state "$2" "$3" ;;
  get-current)    get_current ;;
  commit)         commit ;;
  *)              echo "Unknown command: ${1:-}" >&2; exit 1 ;;
esac
