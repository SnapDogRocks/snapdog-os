# snapdog-update

`snapdog-update` is the operator CLI for updating a SnapDog OS device from a
workstation or automation runner. It accepts signed RAUC firmware bundles and
installs them through the device's atomic A/B update system.

## Usage

Install on macOS with Homebrew:

```bash
brew install snapdogrocks/tap/snapdog-update
```

```bash
snapdog-update --url http://snapdog.local --file snapdog-os-pi4-0.3.0.raucb
```

Authentication can be supplied with `--password` or `SNAPDOG_PASSWORD`. If the
target does not require authentication, no password is needed.

For CI, scripts, and other non-interactive callers:

```bash
snapdog-update \
  --url http://snapdog.local \
  --file snapdog-os-pi4-0.3.0.raucb \
  --password "$SNAPDOG_PASSWORD" \
  --non-interactive \
  --no-progress \
  --output json
```

JSON mode writes newline-delimited events to stdout. Human status, prompts, and
progress bars use stderr, so stdout remains machine-readable.

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Update completed successfully |
| `1` | Update failed |

## Safety Checks

- Validates the target URL and accepts only HTTP or HTTPS endpoints.
- Verifies the local bundle path before opening network connections.
- Accepts only `.raucb` firmware bundles; RAUC verifies the bundle signature and
  target compatibility before installation.
- Checks target health warnings before installation.
- Checks bundle filename board markers against the target board model when both
  can be identified.
- Preserves HTTP status and response body snippets in errors.
- Enforces the configured upgrade timeout across upload, install, and reboot
  monitoring.
