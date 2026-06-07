# snapdog-update

`snapdog-update` is the operator CLI for updating a SnapDog OS device from a
workstation or automation runner. It supports normal RAUC bundle updates and a
guarded raw image flash path for recovery or first-install workflows.

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

## Raw Flash

Raw image flashing is intentionally two-step. Uploading a raw image prints a
short-lived challenge and exits with code `2` unless the user confirms
interactively.

```bash
snapdog-update --url http://snapdog.local --file snapdog-os-pi4-0.3.0.img.gz --raw
snapdog-update --url http://snapdog.local --raw --confirm-raw-flash CHALLENGE
```

This prevents unattended destructive flashes while still allowing automation to
pause, display the challenge to an operator, and resume after explicit approval.

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Update completed successfully |
| `1` | Update failed |
| `2` | Raw flash upload is waiting for explicit challenge confirmation |

## Safety Checks

- Validates the target URL and accepts only HTTP or HTTPS endpoints.
- Verifies the local image path before opening network connections.
- Rejects raw image files unless `--raw` is set.
- Checks target health warnings before installation.
- Checks image filename board markers against the target board model when both
  can be identified.
- Preserves HTTP status and response body snippets in errors.
- Enforces the configured upgrade timeout across upload, install, confirm, and
  reboot monitoring.
