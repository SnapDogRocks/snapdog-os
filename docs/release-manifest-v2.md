# SnapDog OS Release Manifest v2

The channel manifests are the machine-readable source of truth for SnapDog OS
image downloads:

- `https://updates.snapdog.cc/os/images/latest-release.json`
- `https://updates.snapdog.cc/os/images/latest-beta.json`

Schema v2 lets installers download a versioned image, plan disk space, and
verify both the downloaded archive and the raw image written to a target.

## Compatibility

The top-level `channel`, `version`, `commit`, `date`, and `boards` fields are
unchanged. Every board also keeps the v1 fields with their original semantics:

- `image` is the rolling channel-alias filename.
- `sha256` is the SHA-256 digest of the compressed `.img.gz` bytes.

Consumers that ignore unknown JSON properties continue to work unchanged. New
consumers must require `schema_version == 2` before relying on v2 fields.

## Shape

```json
{
  "schema_version": 2,
  "channel": "release",
  "version": "1.2.3",
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "date": "2026-07-19T12:34:56Z",
  "boards": {
    "pi4": {
      "image": "snapdog-os-pi4-release.img.gz",
      "sha256": "<sha256-of-compressed-image>",
      "url": "https://updates.snapdog.cc/os/images/snapdog-os-pi4-1.2.3.img.gz",
      "bundle_url": "https://updates.snapdog.cc/os/bundles/snapdog-os-pi4-1.2.3.raucb",
      "compressed_size": 612345678,
      "uncompressed_size": 2550137344,
      "raw_sha256": "<sha256-of-uncompressed-image>"
    }
  }
}
```

Production manifests contain exactly the supported board keys: `pi3`, `pi4`,
`pi5`, and `zero2w`.

| Field | Meaning |
| --- | --- |
| `url` | Immutable HTTPS URL containing the concrete OS version, never a channel alias |
| `bundle_url` | Immutable HTTPS URL of the signed RAUC bundle for this exact board and version |
| `compressed_size` | Exact `.img.gz` size in bytes |
| `uncompressed_size` | Exact raw `.img` size in bytes and minimum image payload capacity |
| `raw_sha256` | SHA-256 digest of the uncompressed `.img` byte stream |

The beta pointer may be advanced to a stable release. In that case its
`channel` and rolling `image` alias change to `beta`, while `url` still points to
the same immutable, versioned release image.

Release-channel manifests require an exact stable `X.Y.Z` version. Explicitly
dispatched beta builds use `X.Y.Z-beta.N`; a beta-channel manifest may also carry
stable `X.Y.Z` when the stable-plus pointer mirrors the latest release.

## Installer Verification Order

An installer should:

1. Validate the manifest structure and select the requested board.
2. Require an HTTPS, versioned `url` and reject redirects to non-HTTPS URLs.
3. Check that the target can hold at least `uncompressed_size` bytes.
4. Stream the download with an upper bound derived from `compressed_size`.
5. Verify the compressed byte count and `sha256` before trusting the archive.
6. Decompress while hashing and counting the raw stream.
7. Require both `uncompressed_size` and `raw_sha256` to match before reporting
   success.

The OS image itself is not a substitute for a signed application update. RAUC
continues to authenticate OTA bundles separately with the device X.509 keyring.
OTA clients read `bundle_url` from the same manifest as `version`, preventing a
rolling bundle alias from changing between the update check and download. For
schema-v2 manifests published before `bundle_url` was added, SnapDog Ctrl derives
the identical versioned URL from the validated channel, board, and version; it
never falls back to a rolling alias.

Versioned beta manifests are cached for one day. Retention first removes a
superseded manifest, records its observed `max-age`, and keeps the referenced
payloads for that entire cache horizon plus a 24-hour download grace. Legacy
manifests without trustworthy cache metadata are conservatively treated as
cacheable for one year.

## Generation and Validation

`scripts/release_manifest.py` implements the contract with only Python standard
library dependencies. During image packaging, `board-metadata` hashes the raw
and compressed files and proves that the gzip expands to the supplied raw image.
The publish step then verifies each compressed artifact again before building
the public manifest.

Run the local contract tests with:

```bash
make check-release-manifest
```

Validate a downloaded manifest directly with:

```bash
python3 scripts/release_manifest.py validate --manifest latest-release.json
```

Reusing an already published version for different bytes is forbidden: the
versioned objects are served with immutable cache headers.
