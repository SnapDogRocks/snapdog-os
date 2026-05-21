#!/usr/bin/env bash
set -euo pipefail

BOARD_DIR="$(cd "$(dirname "$0")" && pwd)"
GENIMAGE_CFG="${BINARIES_DIR}/genimage-ab.cfg"
GENIMAGE_TMP="${BUILD_DIR}/genimage.tmp"

if [ ! -f "${BINARIES_DIR}/rootfs.ext4" ]; then
	echo "rootfs.ext4 is missing from ${BINARIES_DIR}" >&2
	exit 1
fi

FILES=()
shopt -s nullglob
for path in "${BINARIES_DIR}"/*.dtb "${BINARIES_DIR}"/rpi-firmware/*; do
	FILES+=("${path#${BINARIES_DIR}/}")
done
shopt -u nullglob

KERNEL="$(sed -n 's/^kernel=//p' "${BINARIES_DIR}/rpi-firmware/config.txt" | tail -n 1)"
if [ -n "${KERNEL}" ] && [ -f "${BINARIES_DIR}/${KERNEL}" ]; then
	FILES+=("${KERNEL}")
fi

if [ "${#FILES[@]}" -eq 0 ]; then
	echo "No boot files found in ${BINARIES_DIR}" >&2
	exit 1
fi

BOOT_FILES="$(printf '\t\t\t"%s",\n' "${FILES[@]}")"
sed "s|#BOOT_FILES#|${BOOT_FILES}|" "${BOARD_DIR}/genimage-ab.cfg.in" > "${GENIMAGE_CFG}"

cp -f "${BINARIES_DIR}/rootfs.ext4" "${BINARIES_DIR}/rootfs-b.ext4"

ROOTPATH_TMP="$(mktemp -d)"
trap 'rm -rf "${ROOTPATH_TMP}" "${GENIMAGE_TMP}"' EXIT
rm -rf "${GENIMAGE_TMP}"

genimage \
	--rootpath "${ROOTPATH_TMP}" \
	--tmppath "${GENIMAGE_TMP}" \
	--inputpath "${BINARIES_DIR}" \
	--outputpath "${BINARIES_DIR}" \
	--config "${GENIMAGE_CFG}"
