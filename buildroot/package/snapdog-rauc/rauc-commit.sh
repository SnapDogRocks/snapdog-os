#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Fabian Schmieder
#
# Runs from rauc-mark-good.service after snapdog-ctrl has been started. Commits a
# pending tryboot trial ONLY once the ctrl proves it is stably serving, so a
# crash-looping or hung trial is never committed — it is left for OnFailure /
# the next normal boot to revert. Always exits 0 so the oneshot unit is happy.
set -u

HEALTH_URL="http://127.0.0.1/api/system/health"
NEED_OK=3        # consecutive healthy probes required (stability, not a blip)
INTERVAL=2       # seconds between probes
MAX_PROBES=25    # ~50s budget; must exceed the ctrl start-limit window (~20s)

# One health probe: 0 if the ctrl answered on its (unauthenticated) liveness
# endpoint, which stays reachable even in critical mode.
probe() {
  if command -v wget >/dev/null 2>&1; then
    wget -q -T 3 -O /dev/null "$HEALTH_URL" 2>/dev/null
  elif command -v curl >/dev/null 2>&1; then
    curl -fsS -m 3 -o /dev/null "$HEALTH_URL" 2>/dev/null
  else
    # No HTTP client: fall back to "is something listening on :80".
    ss -ltnH 2>/dev/null | grep -q ':80 '
  fi
}

# If no trial is pending, just mark the booted slot good and exit.
if [ ! -f /boot/tryboot.txt ] && [ ! -f /boot/tryboot-cmdline.txt ]; then
  rauc status mark-good 2>/dev/null || true
  exit 0
fi

echo "rauc-commit: tryboot trial pending — waiting for snapdog-ctrl to be stably healthy"
ok=0
i=0
while [ "$i" -lt "$MAX_PROBES" ]; do
  if probe; then
    ok=$((ok + 1))
    [ "$ok" -ge "$NEED_OK" ] && break
  else
    ok=0
  fi
  i=$((i + 1))
  sleep "$INTERVAL"
done

if [ "$ok" -ge "$NEED_OK" ]; then
  echo "rauc-commit: snapdog-ctrl healthy — marking good + committing trial"
  rauc status mark-good 2>/dev/null || true
  /usr/lib/rauc/boot-handler.sh commit || true
else
  echo "rauc-commit: snapdog-ctrl never became stably healthy — NOT committing (trial will revert)"
fi
exit 0
