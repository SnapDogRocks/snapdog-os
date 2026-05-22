<p align="center">
  <img src="assets/snapdog-logo.svg" width="240" alt="SnapDog" />
</p>

<h1 align="center">SnapDog OS</h1>

<p align="center">
  Buildroot-based minimal Linux <a href="https://github.com/metaneutrons/snapdog">SnapDog</a> multiroom audio system.
</p>

<p align="center">
  <a href="https://github.com/metaneutrons/snapdog-os/actions"><img src="https://github.com/metaneutrons/snapdog-os/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/metaneutrons/snapdog-os/releases"><img src="https://img.shields.io/github/v/release/metaneutrons/snapdog-os" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/metaneutrons/snapdog-os" alt="License"></a>
</p>

---

SnapDog OS turns a Raspberry Pi with an I2S DAC into a dedicated network audio receiver for the [**SnapDog**](https://github.com/metaneutrons/snapdog) multiroom audio system. It boots in seconds, connects to your SnapDog server automatically via mDNS, and is fully configurable through a web browser — no SSH, no command line, no manual setup required.

## Features

- **Boots in seconds** — minimal embedded image, no desktop, no bloat
- **Synchronized audio** — Snapcast-compatible multiroom playback via [snapdog-client](https://github.com/metaneutrons/snapdog)
- **Any I2S DAC** — HiFiBerry, Allo, IQAudio, JustBoom, MAX98357A, and more
- **Zero-config setup** — captive portal WiFi configuration from your phone
- **Web UI** — [SnapDog Control](#snapdog-ctrl) for network, audio, and system settings
- **OTA updates** — dual-partition A/B (1GB per partition for future-proof updates) with signed metadata, SHA256 verification, and automatic rollback
- **Zero Rootfs Wear & Persistence** — symbolic link mapping of all mutable files (WiFi setups, client configurations, update schedules, and SSH keys) to a dedicated writeable `/data` partition, ensuring robust flash memory protection and full configuration preservation across system upgrades
- **Secure by default** — SSH disabled, pubkey-only when enabled, no default network access

## Supported Hardware

| Board | Status |
|-------|--------|
| Raspberry Pi 5 | ✅ |
| Raspberry Pi 4 | ✅ |
| Raspberry Pi 3 | ✅ |

All builds are 64-bit (aarch64). Kernel: Raspberry Pi Linux 6.6 LTS.

## Installation

### Raspberry Pi Imager (recommended)

1. Download the latest image from [Releases](https://github.com/metaneutrons/snapdog-os/releases)
2. Open [Raspberry Pi Imager](https://www.raspberrypi.com/software/)
3. Choose **"Use custom"** and select the downloaded `.img.gz` file
4. Select your SD card and write

### Command line

```bash
# macOS
gunzip -k snapdog-os-pi4-0.1.0.img.gz
sudo dd if=snapdog-os-pi4-0.1.0.img of=/dev/rdiskN bs=4m status=progress

# Linux
gunzip -k snapdog-os-pi4-0.1.0.img.gz
sudo dd if=snapdog-os-pi4-0.1.0.img of=/dev/sdX bs=4M status=progress conv=fsync
```

### First boot

1. Insert SD card and power on the Pi
2. If no Ethernet is connected, the device creates a WiFi access point: **SnapDog-Setup** (password: `snapdog123`)
3. Connect to the AP — your phone/laptop will automatically open the setup UI
4. Configure WiFi, select your SnapDog server, choose your DAC — done

## Architecture

```
┌─────────────────────────────────────┐
│  snapdog-ctrl (port 80)             │
│  Device configuration web UI        │
├─────────────────────────────────────┤
│  snapdog-client                     │
│  Snapcast audio receiver            │
├─────────────────────────────────────┤
│  ALSA → I2S DAC/AMP                 │
│  Hardware audio output              │
├─────────────────────────────────────┤
│  Linux 6.6 LTS (aarch64) + systemd  │
│  Raspberry Pi kernel                │
├─────────────────────────────────────┤
│  Buildroot 2025.02 LTS              │
│  Minimal userspace (~21 packages)   │
└─────────────────────────────────────┘
```

## snapdog-ctrl

The device configuration service is a single Rust binary with an embedded web UI:

| Feature | Implementation |
|---------|---------------|
| Web framework | axum 0.8 |
| Frontend | Next.js 16, React 19, Tailwind v4, static export |
| Embedded assets | rust-embed |
| Network management | wpa_supplicant, hostapd, systemd-networkd |
| mDNS discovery | mdns-sd crate |
| Config parsing | Custom config.txt parser with backup |
| Logging | tracing + journald (Linux) |
| Accessibility | WCAG AAA, 5 languages (EN/DE/FR/ES/NL) |
| Linting | clippy pedantic + nursery, zero exceptions |

### Web UI tabs

- **Dashboard** — hostname, version, network status, uptime
- **Network** — WiFi (scan, connect, static IP), Ethernet (DHCP/static)
- **Audio** — DAC overlay selection from detected boards
- **Client** — server discovery (mDNS), soundcard, volume control, latency
- **SSH** — enable/disable, pubkey management
- **Update** — OTA check/install, channel (stable/beta), auto-update schedule
- **System** — timezone, logs, reboot, factory reset

### Local development

```bash
cd snapdog-ctrl
SNAPDOG_SETUP_PORT=8080 cargo run
# → http://localhost:8080 (mock mode, all APIs functional)
```

## OTA Updates

| Feature | Detail |
|---------|--------|
| Mechanism | Dual-partition A/B |
| Integrity | OpenSSL-signed metadata + SHA256 archive verification |
| Rollback | Automatic after 3 failed boot attempts |
| Channels | `stable` (tagged releases), `beta` (every push) |
| Schedule | Configurable: daily/weekly/monthly at chosen time |
| Server | `update.snapdog.cc/os` (Cloudflare R2) |

## DAC Support

Set via the web UI or `BR2_PACKAGE_CONFIGTXT_DAC_OVERLAY` at build time. Leave empty for HAT EEPROM auto-detection.

| Board | Overlay |
|-------|---------|
| HiFiBerry DAC+/DAC2 Pro | `hifiberry-dacplus` |
| HiFiBerry Amp2/3 | `hifiberry-amp3` |
| Allo Boss DAC | `allo-boss-dac-pcm512x-audio` |
| IQAudio DAC+ | `iqaudio-dacplus` |
| JustBoom DAC | `justboom-dac` |
| Adafruit MAX98357A | `max98357a` |
| Google AIY Voice HAT | `googlevoicehat-soundcard` |

## Building from source

Requires a Linux host with standard buildroot dependencies (`build-essential`, `git`, `wget`, `cpio`, `unzip`, `rsync`, `bc`).

```bash
make setup                         # Download buildroot 2025.02
# provide an aarch64 snapdog-ctrl binary at ./snapdog-ctrl-binary
make PI=pi4 config                 # Configure for Raspberry Pi 4
make PI=pi4 build                  # Build SD card image
make all                           # Build for all Pi variants
```

Output: `../buildroot-pi4/images/sdcard.img`

## Security

| Aspect | Default |
|--------|---------|
| SSH | disabled by default |
| SSH auth | pubkey only (password auth forbidden) |
| Root password | `snapdog` (local console only) |
| Web UI | No authentication (local network only) |
| OTA | Signed metadata, SHA256 verified payloads, auto-rollback on failure |
| Watchdog | Hardware, 30s timeout via systemd |
| Filesystem | Completely read-only system partition; all mutable configurations are symlinked to a dedicated `/data` partition |

## Related

- [**snapdog**](https://github.com/metaneutrons/snapdog) — SnapDog multiroom audio server and client

## Repository Setup

Branch protection on `main`:
- Required status checks: Lint & Test, Cross-compile, Security Audit, Release Sanity
- Required pull request review and CODEOWNERS review
- Require conversation resolution and signed commits
- No force pushes, no deletions
- All changes via PR (CI must pass before merge)

Actions permissions:
- Default workflow permissions: read-only
- Allowlist pinned actions/reusable workflows only where supported
- Release workflow grants write permissions only to release-please/publish jobs

Required secrets for releases:
- `R2_ACCESS_KEY_ID` — Cloudflare R2 access key
- `R2_SECRET_ACCESS_KEY` — Cloudflare R2 secret
- `R2_ENDPOINT_URL` — Cloudflare R2 endpoint
- `SNAPDOG_UPDATE_SIGNING_KEY_PEM` — private RSA update metadata signing key

Signing key bootstrap:

```bash
scripts/generate-update-signing-key.sh
gh secret set SNAPDOG_UPDATE_SIGNING_KEY_PEM < secrets/update-signing.private.pem
```

The public key is baked into the OS image at `/etc/snapdog-os-update.pub.pem`. The release workflow refuses to publish if the private signing key does not match the committed public key.

## License

[GPL-3.0](LICENSE)
