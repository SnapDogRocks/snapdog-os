#!/bin/bash
# Generate a signed RAUC bundle from a rootfs image.
# Usage: ./create-bundle.sh <pi3|pi4|pi5> <version> <rootfs.ext4> <output.raucb>
#
# Requires: rauc, openssl, signing key + cert

set -euo pipefail

PI="${1:?Usage: $0 <pi3|pi4|pi5> <version> <rootfs.ext4> <output.raucb>}"
VERSION="${2:?}"
ROOTFS="${3:?}"
OUTPUT="${4:?}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CERT="${SCRIPT_DIR}/../keys/rauc-ca.cert.pem"
KEY="${RAUC_KEY_FILE:-${SCRIPT_DIR}/../../secrets/rauc-ca.key.pem}"

[ -f "$ROOTFS" ] || { echo "rootfs not found: $ROOTFS" >&2; exit 1; }
[ -f "$CERT" ] || { echo "cert not found: $CERT" >&2; exit 1; }
[ -f "$KEY" ] || { echo "signing key not found: $KEY" >&2; exit 1; }

WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

# Create manifest
sed -e "s/@PI@/$PI/" -e "s/@VERSION@/$VERSION/" \
  "$SCRIPT_DIR/manifest.raucm.in" > "$WORKDIR/manifest.raucm"

# Copy rootfs image
cp "$ROOTFS" "$WORKDIR/rootfs.img"

# Create signed bundle
rauc bundle \
  --cert="$CERT" \
  --key="$KEY" \
  "$WORKDIR" \
  "$OUTPUT"

echo "Bundle created: $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"
