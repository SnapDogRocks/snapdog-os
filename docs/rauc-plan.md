# RAUC Integration Plan for SnapDog OS

## Overview

Replace the custom OTA system (snapdog-updater) with RAUC — an enterprise-grade
A/B update framework with D-Bus API, cryptographic bundle verification, and
automatic rollback via bootloader integration.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│ Cloudflare R2                                           │
│   /os/bundles/snapdog-os-pi4-0.2.0.raucb               │
│   /os/metadata/stable/pi4.json (+ .sig)                │
└──────────────────────────┬──────────────────────────────┘
                           │ HTTPS download
┌──────────────────────────▼──────────────────────────────┐
│ snapdog-ctrl (Rust)                                     │
│   - Checks metadata (signed JSON, as today)             │
│   - Downloads .raucb bundle                             │
│   - Calls RAUC D-Bus API: InstallBundle()               │
│   - Monitors progress via D-Bus signals                 │
│   - Reports status to Web UI                            │
└──────────────────────────┬──────────────────────────────┘
                           │ D-Bus
┌──────────────────────────▼──────────────────────────────┐
│ RAUC Service                                            │
│   - Verifies bundle signature (X.509)                   │
│   - Writes rootfs to inactive slot                      │
│   - Marks new slot as primary                           │
│   - Triggers reboot                                     │
└──────────────────────────┬──────────────────────────────┘
                           │ Custom bootloader backend
┌──────────────────────────▼──────────────────────────────┐
│ Boot Selection (config.txt / tryboot.txt)                │
│   - RPi bootloader reads tryboot.txt on next boot       │
│   - Falls back to config.txt if boot fails              │
│   - RAUC marks slot good after successful boot          │
└─────────────────────────────────────────────────────────┘
```

## Partition Layout (unchanged)

| # | Name    | Size   | Type | Purpose                    |
|---|---------|--------|------|----------------------------|
| 1 | boot    | 256MB  | vfat | Firmware, kernel, DTBs     |
| 2 | rootfsA | 1024MB | ext4 | Root filesystem (slot A)   |
| 3 | rootfsB | 1024MB | ext4 | Root filesystem (slot B)   |
| 4 | data    | 128MB+ | ext4 | Persistent config (/data)  |

## Bootloader Backend: Custom (RPi-native)

RAUC supports a `custom` bootloader backend via shell scripts. For RPi we use
`tryboot.txt` (Pi4/5 firmware feature):

- `config.txt` → always points to the "known good" slot
- `tryboot.txt` → written by RAUC before reboot, points to new slot
- Pi firmware boots from `tryboot.txt` once, then falls back to `config.txt`
- After successful boot: RAUC `mark-good` promotes tryboot → config.txt

Custom backend script implements:
- `get-primary` → parse config.txt for root= partition
- `set-primary` → update config.txt
- `get-state` / `set-state` → manage slot state in /data/rauc-state
- `get-current` → parse /proc/cmdline

## Implementation Steps

### Phase 1: RAUC on Target

1. Enable `BR2_PACKAGE_RAUC` in Buildroot config
2. Create `/etc/rauc/system.conf`:
   ```ini
   [system]
   compatible=snapdog-os-pi4
   bootloader=custom
   statusfile=/data/rauc.status

   [handlers]
   bootloader-custom-backend=/usr/lib/rauc/boot-handler.sh

   [keyring]
   path=/etc/rauc/ca.cert.pem

   [slot.rootfs.0]
   device=/dev/mmcblk0p2
   type=ext4
   bootname=A

   [slot.rootfs.1]
   device=/dev/mmcblk0p3
   type=ext4
   bootname=B
   ```
3. Create custom boot handler script (`boot-handler.sh`)
4. Create RAUC keyring (X.509 CA cert for bundle signing)
5. Add `rauc.slot=A` to kernel cmdline (explicit slot identification)
6. Add `rauc-mark-good.service` (runs after snapdog-ctrl starts)

### Phase 2: Bundle Generation in CI

7. Install `rauc` as host tool in CI
8. Create bundle manifest template
9. Generate `.raucb` bundles (signed with X.509 key)
10. Upload bundles to R2 alongside metadata JSON
11. Update metadata JSON format to include bundle URL + hash

### Phase 3: snapdog-ctrl Integration

12. Add `zbus` crate for D-Bus communication with RAUC
13. Replace shell-based update logic with RAUC D-Bus calls:
    - `org.rauc.Installer.InstallBundle()`
    - `org.rauc.Installer.Info()` for progress
    - `org.rauc.Installer.GetSlotStatus()`
14. Update Web UI progress reporting (D-Bus signals → WebSocket)

### Phase 4: Remove Old System

15. Remove `snapdog-updater` package (update script, boot-guard, extract-update)
16. Remove custom signing logic from CI publish step (RAUC handles it)
17. Update release workflow to build .raucb instead of .tar.gz

## Key Decisions

- **Bundle format**: `verity` (recommended, integrity-checked)
- **Signing**: X.509 (RAUC native), replaces our Ed25519 metadata signing
- **Slot detection**: Explicit via `rauc.slot=A` in cmdline
- **Status file**: `/data/rauc.status` (persistent across updates)
- **Mark-good trigger**: After snapdog-ctrl HTTP server responds on port 80
- **Manual upload**: Web UI file upload → `/tmp/update.raucb` → RAUC install
- **Streaming install**: RAUC can install directly from URL (saves RAM on 1GB Pi3)
- **Per-board compatible**: `snapdog-os-pi3`, `snapdog-os-pi4`, `snapdog-os-pi5`

## Kernel Requirements

Add to `kernel.fragment`:
```
CONFIG_MD=y
CONFIG_BLK_DEV_DM=y
CONFIG_DM_VERITY=y
CONFIG_CRYPTO_SHA256=y
```

## Files to Create/Modify

### New files:
- `buildroot/package/snapdog-rauc/` — RAUC config package
  - `system.conf` — slot definitions
  - `boot-handler.sh` — custom bootloader backend
  - `rauc-mark-good.service` — systemd service
- `buildroot/keys/rauc-ca.cert.pem` — signing CA certificate
- `secrets/rauc-ca.key.pem` — signing CA private key (gitignored)

### Modified files:
- `buildroot/configs/override.conf` — enable BR2_PACKAGE_RAUC
- `buildroot/package/snapdog-base/Config.in` — add RAUC dependency
- `buildroot/package/configtxt/cmdline.quiet` — add rauc.slot=A
- `.github/workflows/release.yml` — build .raucb bundles
- `snapdog-ctrl/Cargo.toml` — add zbus for D-Bus
- `snapdog-ctrl/src/system.rs` — RAUC D-Bus update logic

### Removed files:
- `buildroot/package/snapdog-updater/` — entire package
- Custom boot-guard, extract-update, reactivate-previous-release scripts

## GitHub Secrets Required

| Secret | Purpose |
|--------|---------|
| `RAUC_CA_KEY_PEM` | X.509 private key (PEM) for signing RAUC bundles |
| `R2_ACCESS_KEY_ID` | Cloudflare R2 access (existing) |
| `R2_SECRET_ACCESS_KEY` | Cloudflare R2 secret (existing) |
| `R2_ENDPOINT_URL` | Cloudflare R2 endpoint (existing) |
| `SNAPDOG_UPDATE_SIGNING_KEY_PEM` | Ed25519 key for metadata JSON signing (existing) |

## Migration Path

1. First release with RAUC: include both old updater AND RAUC
2. Old updater installs the RAUC-enabled image
3. After that, RAUC handles all future updates
4. Remove old updater in subsequent release

## Raw Image Flash (Escape Hatch)

For emergencies (downgrade, recovery, custom images), a raw rootfs flash
bypassing RAUC is available:

- **UI**: System tab → Update → Advanced → "Flash Raw Image"
- **Guard**: Challenge-response (cannot be automated via API alone)
- **No signature verification** — user accepts full responsibility
- **No mark-good** — RAUC slot state set to "good" manually after reboot

### Challenge-Response Protocol

```
1. POST /api/system/update/flash-raw
   Body: multipart file upload (.img or .img.gz)
   Response: { "challenge": "X7k9Qm", "expires_at": "2026-...", "size": 1234567 }
   → Image saved to /tmp/pending-flash.img.gz
   → Challenge valid for 60 seconds, single-use

2. UI displays: "To confirm flashing, type: X7k9Qm"
   User must manually type the 6-character code

3. POST /api/system/update/flash-raw/confirm
   Body: { "challenge": "X7k9Qm" }
   Response: 202 Accepted (flash starts)
   → On wrong/expired challenge: 403 Forbidden
   → Challenge deleted after use or timeout

4. Progress via WebSocket (same as RAUC updates)
```

### Security Properties

- **Not automatable**: Challenge is random, short-lived, must be typed
- **No replay**: Single-use, deleted after confirm or 60s timeout
- **No brute-force**: Only 1 active challenge at a time
- **Requires physical presence**: User must see the UI to read the code
- **Works without auth password**: The challenge IS the auth for this action
