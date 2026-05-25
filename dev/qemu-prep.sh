#!/bin/bash
# Prepare a SnapDog OS image for QEMU testing.
# Creates a copy, resizes to 4G, patches fstab for virtio devices.
#
# Usage: ./dev/qemu-prep.sh [input.img] [output.img]

set -euo pipefail

export DOCKER_HOST="${DOCKER_HOST:-unix://$HOME/.colima/default/docker.sock}"

INPUT="${1:-$HOME/Desktop/snapdog-os-pi4.img}"
OUTPUT="${2:-$HOME/Desktop/snapdog-os-pi4-qemu.img}"

[ -f "$INPUT" ] || { echo "Input not found: $INPUT" >&2; exit 1; }

echo "Copying $INPUT → $OUTPUT"
cp "$INPUT" "$OUTPUT"
qemu-img resize -f raw "$OUTPUT" 4G

echo "Patching fstab for QEMU (mmcblk0p1 → vda1)..."
docker run --rm -v "$(dirname "$OUTPUT"):/img" ubuntu:24.04 bash -c "
  apt-get update -qq && apt-get install -y -qq e2fsprogs fdisk > /dev/null 2>&1
  IMG=/img/$(basename "$OUTPUT")
  # Get partition 2 offset in bytes
  START=\$(fdisk -l \"\$IMG\" | awk '/img2/{print \$2}')
  OFFSET=\$((START * 512))
  SIZE=\$(fdisk -l \"\$IMG\" | awk '/img2/{print \$4}')
  BYTES=\$((SIZE * 512))
  # Extract partition 2
  dd if=\"\$IMG\" of=/tmp/rootfs.ext4 bs=512 skip=\$START count=\$SIZE status=none
  # Patch fstab
  debugfs -w /tmp/rootfs.ext4 -R 'cat /etc/fstab' 2>/dev/null > /tmp/fstab
  sed -i 's|/dev/mmcblk0p1|/dev/vda1|g' /tmp/fstab
  echo '=== patched fstab ==='
  cat /tmp/fstab
  # Write back
  debugfs -w /tmp/rootfs.ext4 -R 'rm /etc/fstab' 2>/dev/null
  debugfs -w /tmp/rootfs.ext4 -R 'write /tmp/fstab /etc/fstab' 2>/dev/null
  # Put partition back
  dd if=/tmp/rootfs.ext4 of=\"\$IMG\" bs=512 seek=\$START count=\$SIZE conv=notrunc status=none
"

echo "Done: $OUTPUT (4GB, QEMU-ready)"
echo ""
echo "Boot with:"
echo "  qemu-system-aarch64 \\"
echo "    -machine virt -cpu cortex-a72 -m 1G \\"
echo "    -kernel /Volumes/Dev/Source/snapdog-os/dev/Image \\"
echo "    -drive file=$OUTPUT,format=raw,if=none,id=hd0 \\"
echo "    -device virtio-blk-device,drive=hd0 \\"
echo "    -append \"root=/dev/vda2 rootwait console=tty0 console=ttyAMA0,115200 rauc.slot=A\" \\"
echo "    -device virtio-gpu-pci -display cocoa -serial mon:stdio"
