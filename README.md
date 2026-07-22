<p align="center">
  <img src="assets/snapdog-logo.svg" width="240" alt="SnapDog" />
</p>

<h1 align="center">SnapDog OS</h1>

<p align="center">
  Buildroot-based minimal Linux <a href="https://github.com/SnapDogRocks/snapdog">SnapDog</a> multiroom audio system.
</p>

<p align="center">
  <a href="https://github.com/SnapDogRocks/snapdog-os/actions"><img src="https://github.com/SnapDogRocks/snapdog-os/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/SnapDogRocks/snapdog-os/releases"><img src="https://img.shields.io/github/v/release/SnapDogRocks/snapdog-os" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/SnapDogRocks/snapdog-os" alt="License"></a>
</p>

---

> [!WARNING]
> **Alpha — not ready for production use.**
> SnapDog OS is under active development and has not been sufficiently tested on real hardware. Do not deploy it on devices you rely on.
>
> For a working multiroom audio setup today, use **Raspberry Pi OS** with the [SnapDog `.deb` package](https://github.com/SnapDogRocks/snapdog/releases) from [SnapDogRocks/snapdog](https://github.com/SnapDogRocks/snapdog).

---

SnapDog OS turns a Raspberry Pi with an I2S DAC into a dedicated network audio receiver for the [**SnapDog**](https://github.com/SnapDogRocks/snapdog) multiroom audio system. It boots in seconds, connects to your SnapDog server automatically via mDNS, and is fully configurable through a web browser — no SSH, no command line, no manual setup required.

## Features

- **Boots in seconds** — minimal embedded image, no desktop, no bloat
- **Synchronized audio** — Snapcast-compatible multiroom playback via [snapdog-client](https://github.com/SnapDogRocks/snapdog)
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
| Raspberry Pi Zero 2 W | ✅ |

All builds are 64-bit (aarch64). Kernel: Raspberry Pi Linux 6.6 LTS.

## Installation

### Raspberry Pi Imager (recommended)

1. Download the latest image for your board from [Releases](https://github.com/SnapDogRocks/snapdog-os/releases) or `https://updates.snapdog.cc/os/images/snapdog-os-<board>-beta.img.gz`
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
2. If no WiFi is configured and no functional wired network is available, the device creates a setup access point named **SnapDog-XXXX**, where `XXXX` is the final four hexadecimal characters of its WiFi MAC address. A unique password is generated on first use, persisted in `/data`, and printed to the device console.
3. Connect to the AP — your phone/laptop will automatically open the setup UI
4. Configure WiFi, select your SnapDog server, choose your DAC — done

## Architecture

```
┌─────────────────────────────────────┐
│  snapdog-ctrl (port 80)             │
│  Device configuration web UI        │
│  Service manager, OTA orchestrator  │
├─────────────────────────────────────┤
│  snapdog-client / snapdog-server    │
│  Snapcast audio (managed by ctrl)   │
├─────────────────────────────────────┤
│  RAUC │ A/B OTA with auto-rollback  │
├─────────────────────────────────────┤
│  ALSA → I2S DAC/AMP                 │
├─────────────────────────────────────┤
│  Linux 6.6 LTS (aarch64) + systemd  │
├─────────────────────────────────────┤
│  Buildroot 2025.02                   │
└─────────────────────────────────────┘
```

### Partition Layout

| # | Name | Size | Type | Purpose |
|---|------|------|------|---------|
| 1 | boot | 256MB | vfat (ro) | Firmware, kernel, DTBs, config.txt |
| 2 | rootfsA | 1GB | ext4 (ro) | Root filesystem (slot A) |
| 3 | rootfsB | 1GB | ext4 (ro) | Root filesystem (slot B) |
| 4 | data | 128MB+ | ext4 (rw) | Persistent config (`/data`) |

### Read-Only Rootfs

The root filesystem is mounted read-only. Mutable state lives on `/data`:

- `/etc/hostname` → `/data/hostname`
- `/etc/snapdog/snapdog.toml` → `/data/snapdog/snapdog.toml`
- `/etc/systemd/network` → `/data/systemd/network`
- `/var/lib` → tmpfs (ephemeral)

### Service Management

`snapdog-ctrl` is the **sole orchestrator** for optional services. No services are enabled in systemd directly — `snapdog-ctrl` starts/stops them at boot based on `/data/snapdog/ctrl.toml`:

```toml
[services]
ssh = false       # default: off
client = true     # default: on
server = false    # default: off

[auto-update]
enabled = true
channel = "release"
interval = "daily"
time = "04:00"

[auth]
# password_hash = "$2b$12$..."  # optional web UI password
```

### Boot Flow

1. Bootloader loads kernel from `/boot`
2. systemd starts `snapdog-data-init` (creates `/data` defaults)
3. systemd starts `snapdog-ctrl`
4. `snapdog-ctrl` reads `ctrl.toml` and starts configured services
5. `snapdog-ctrl` starts the derived `SnapDog-XXXX` setup AP if WiFi is unconfigured and the wired connectivity check fails
6. `rauc-mark-good` marks the current slot as bootable

### OTA Updates (RAUC)

- **Bundle format**: `.raucb` (verity, X.509 signed)
- **A/B switching**: Custom bootloader backend using RPi `cmdline.txt`
- **Auto-rollback**: If `snapdog-ctrl` fails to start 3 times → previous slot
- **Auto-update**: Daily check at configured time, install + reboot
- **Manual**: Upload `.raucb` via web UI or install from URL
- **Channels**: `release` (`snapdog-os-<board>-release.raucb`) / `beta` (`snapdog-os-<board>-beta.raucb`) for `pi3`, `pi4`, `pi5`, and `zero2w`
- **Installer metadata**: Backward-compatible [release manifest v2](docs/release-manifest-v2.md) with immutable image URLs, byte sizes, and hashes for compressed and raw images

### SoftAP

Started automatically when no WiFi is configured and no functional wired network is available after the startup connectivity check. It stops automatically when a network connection is established. The SSID is derived from the WiFi MAC address as `SnapDog-XXXX` (for example, `SnapDog-2B3C`); `SnapDog-Setup` is used only as a fallback when a valid MAC address cannot be read. A unique WPA2 password is generated on first use, persisted in `/data`, printed to the device console, and configurable in the web UI.

## snapdog-ctrl

The device configuration service is a single Rust binary with an embedded web UI:

| Feature | Implementation |
|---------|---------------|
| Web framework | axum 0.8 |
| Frontend | Next.js 16, React 19, Tailwind v4, static export |
| Embedded assets | rust-embed |
| Network management | wpa_supplicant, hostapd, systemd-networkd |
| mDNS discovery | mdns-sd crate |
| Config parsing | Lossless TOML editing plus guarded, atomic activation |
| Logging | tracing + journald (Linux) |
| Accessibility | WCAG AAA, 6 languages (EN/DE/ES/FR/IT/NL) |
| Linting | clippy pedantic + nursery, zero exceptions |

### Web UI tabs

- **Dashboard** — hostname, version, network status, uptime
- **Network** — WiFi (scan, connect, static IP), Ethernet (DHCP/static)
- **Audio** — DAC overlay selection from detected boards
- **Client** — server discovery (mDNS), soundcard, volume control, latency
- **Server** — SnapDog zones, clients, sources and integrations, plus a complete advanced TOML editor
- **SSH** — enable/disable, pubkey management
- **Update** — OTA check/install, channel (stable/beta), auto-update schedule
- **System** — timezone, logs, reboot, factory reset

Server configuration changes are revision-guarded, validated by the installed
SnapDog binary, written atomically, and activated only through a monitored
service restart. If the new service does not remain healthy, `snapdog-ctrl`
restores the previous configuration and restarts it automatically. Fields not
yet represented by the graphical editor remain untouched and can be edited in
the Advanced TOML view.

### Local development

```bash
cd snapdog-ctrl
SNAPDOG_SETUP_PORT=8080 cargo run
# → http://localhost:80 (mock mode, all APIs functional)
```

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
make BOARD=pi4 config               # Configure for Raspberry Pi 4
make BOARD=pi4 build                # Build SD card image
make all                            # Build pi3, pi4, pi5, and zero2w variants
```

Output: `../buildroot-<board>/images/sdcard.img`

## Security

| Aspect | Default |
|--------|---------|
| Web UI | Optional password (protects UI + console login) |
| SSH | Disabled, key-only when enabled via UI |
| Root password | `snapdog` (changeable via Web UI) |
| OTA bundles | X.509 signed RAUC verity bundles, auto-rollback |
| SoftAP | WPA2; device-specific `SnapDog-XXXX` SSID; generated per-device password, configurable in the web UI |
| Filesystem | Read-only rootfs; mutable config on `/data` partition |

## Related

- [**snapdog**](https://github.com/SnapDogRocks/snapdog) — SnapDog multiroom audio server and client

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

RAUC signing key (X.509):

```bash
# Generate CA keypair (once)
openssl req -x509 -newkey rsa:4096 -nodes -days 3650 \
  -keyout secrets/rauc-ca.key.pem \
  -out buildroot/keys/rauc-ca.cert.pem \
  -subj "/O=SnapDog/CN=SnapDog OS Update CA"

# Add to GitHub secrets
gh secret set RAUC_CA_KEY_PEM < secrets/rauc-ca.key.pem
```

The CA certificate is baked into the OS image at `/etc/rauc/ca.cert.pem`. Only bundles signed with the matching private key can be installed.

## Roadmap

- [ ] Per-board configuration structure (`buildroot/boards/{pi3,pi4,pi5,cm4}/`)
- [ ] WebUI i18n completion (all new strings in de/fr/es/nl)
- [ ] Zone presence/scheduling UI
- [ ] Custom Docker build image for CI (faster builds)
- [ ] Hardware test suite (automated boot + API verification)

## License

[GPL-3.0](LICENSE)
