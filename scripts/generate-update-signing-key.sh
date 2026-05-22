#!/usr/bin/env bash
set -euo pipefail

PRIVATE_KEY=${1:-secrets/update-signing.private.pem}
PUBLIC_KEY=${2:-buildroot/keys/update-signing.pub.pem}

mkdir -p "$(dirname "$PRIVATE_KEY")" "$(dirname "$PUBLIC_KEY")"
umask 077

if [ -e "$PRIVATE_KEY" ]; then
	echo "$PRIVATE_KEY already exists; refusing to overwrite" >&2
	exit 1
fi

openssl genpkey -algorithm ED25519 -out "$PRIVATE_KEY"
openssl pkey -in "$PRIVATE_KEY" -pubout -out "$PUBLIC_KEY"

echo "Wrote private key: $PRIVATE_KEY"
echo "Wrote public key:  $PUBLIC_KEY"
echo "Store the private key in the SNAPDOG_UPDATE_SIGNING_KEY_PEM GitHub environment secret."
