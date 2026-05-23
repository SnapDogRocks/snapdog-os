# Local Development Build

Buildroot requires Linux. On macOS, use Docker (via Colima) to build SD card images locally.

## Prerequisites

```bash
brew install docker docker-compose
colima start --cpu 4 --memory 8 --disk 30 --mount /Volumes/Dev/Source:/Volumes/Dev/Source:w
```

## Build

```bash
cd dev
docker-compose up
```

This builds a Pi 4 image. Output: `/build/buildroot-pi4/images/sdcard.img` inside the container.

To copy the image out:

```bash
docker cp $(docker ps -aq -f name=dev-build):/build/buildroot-pi4/images/sdcard.img ../images/snapdog-os-pi4.img
```

## Why Docker?

Buildroot cross-compiles an entire Linux system (toolchain, kernel, packages, filesystem image). This only works on a Linux host. Docker provides that Linux environment on macOS without a full VM.

The CI builds natively on Ubuntu — these files are not used in production.

## With snapdog-ctrl

To include `snapdog-ctrl` in the image, cross-compile it first:

```bash
cd ../snapdog-ctrl
cross build --release --target aarch64-unknown-linux-gnu
cp target/aarch64-unknown-linux-gnu/release/snapdog-ctrl ../snapdog-ctrl-binary
```

The `docker-compose.yml` will detect and inject the binary into the image.
