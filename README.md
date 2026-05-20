# SnapDog OS

Minimal Linux distribution for Raspberry Pi, purpose-built as a multiroom audio receiver.

Based on [Buildroot 2025.02 LTS](https://buildroot.org/), SnapDog OS provides a headless appliance that runs [snapdog-client](https://github.com/metaneutrons/snapdog) — a Snapcast-compatible audio receiver with mDNS discovery, hardware mixer support, and parametric EQ.

## Features

- **Minimal footprint** — boots in seconds, ~512MB image, no desktop environment
- **Snapcast client** — synchronized multiroom audio playback
- **Generic I2S DAC/AMP support** — works with HiFiBerry, Allo, IQAudio, JustBoom, and other I2S boards
- **WiFi setup** — temporary AP mode for initial configuration
- **Web-based setup** — configure network, audio device, and server via [snapdog-ctrl](https://github.com/metaneutrons/snapdog-ctrl)
- **OTA updates** — dual-partition A/B update mechanism via `update.snapdog.cc/os`
- **Supported hardware** — Raspberry Pi 3, 4, and 5 (64-bit only)
- **Kernel** — Raspberry Pi Linux 6.6 LTS

## Building

Requires a Linux host (or Docker container) with standard buildroot dependencies.

```bash
# 1. Get buildroot
./get-buildroot

# 2. Configure for your Pi version (3, 4, or 5)
./build-config 4

# 3. Compile
./compile 4
```

The SD card image will be at `../buildroot-2025.02/output-pi4/images/sdcard.img`.

## DAC Configuration

Set `BR2_PACKAGE_CONFIGTXT_DAC_OVERLAY` in buildroot menuconfig (`./config 4`), or leave empty for HAT EEPROM auto-detection.

Common overlays:

| Board | Overlay |
|-------|---------|
| HiFiBerry DAC+ / DAC2 | `hifiberry-dacplus` |
| HiFiBerry Amp2/3 | `hifiberry-amp3` |
| Allo Boss DAC | `allo-boss-dac-pcm512x-audio` |
| IQAudio DAC+ | `iqaudio-dacplus` |
| JustBoom DAC | `justboom-dac` |
| Adafruit MAX98357A | `max98357a` |
| Google AIY Voice HAT | `googlevoicehat-soundcard` |

At runtime, the DAC overlay can be changed via the snapdog-ctrl web UI or by editing `/boot/config.txt`.

## Runtime Configuration

Edit `/etc/default/snapdog-client`, then `systemctl restart snapdog-client`:

```bash
# Auto-discover server via mDNS (default)
SNAPDOG_CLIENT_ARGS=""

# Specify server explicitly
SNAPDOG_CLIENT_ARGS="tcp://192.168.1.10:1704 --hostID kitchen --soundcard hw:0"
```

## OTA Updates

Updates are fetched from `https://update.snapdog.cc/os` and applied to the inactive partition. On reboot, the system switches to the updated partition.

```bash
# Check for updates
/opt/snapdog/bin/update --check

# Apply update and reboot
/opt/snapdog/bin/update --reboot
```

Release channel is configured in `/etc/snapdog-os.channel` (`stable` or `beta`).

## Architecture

```
┌─────────────────────────────────────────┐
│  snapdog-ctrl (port 80)                │  WiFi/network/DAC config web UI
├─────────────────────────────────────────┤
│  snapdog-client                         │  Snapcast audio receiver
├─────────────────────────────────────────┤
│  ALSA → I2S DAC/AMP                    │  Hardware audio output
├─────────────────────────────────────────┤
│  Linux 6.6 LTS (aarch64)               │  Raspberry Pi kernel
├─────────────────────────────────────────┤
│  Buildroot 2025.02 + systemd           │  Minimal userspace
└─────────────────────────────────────────┘
```

## Default Credentials

- **SSH**: disabled by default, enable via snapdog-ctrl web UI
- **Root password**: `snapdog`
- **Hostname**: defaults to snapdog-client's host ID (e.g. `kitchen`)

## License

MIT
