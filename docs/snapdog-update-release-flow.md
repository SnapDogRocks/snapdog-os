# snapdog-update Release Flow

`snapdog-update` ships as a standalone operator binary, aligned with the
existing SnapDog client binary and Homebrew tap pattern.

## Goals

- Publish reproducible release archives for macOS and Linux.
- Keep release artifacts separate from OS image and RAUC bundle artifacts.
- Update the SnapDog Homebrew tap automatically after a stable release.
- Preserve checksums and provenance for automation and operator trust.

## Workflow Shape

1. Release Please manages the `snapdog-update` package version and changelog.
2. Stable binary releases are triggered by tags in the form
   `snapdog-update-v<version>`.
3. `.github/workflows/release-snapdog-update.yml` builds a Rust-only matrix for:
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
4. Each build is packaged as:
   - `snapdog-update-${TAG}-${TARGET}.tar.gz`
   - `snapdog-update-${TAG}-${TARGET}.tar.gz.sha256`
5. Each archive contains `snapdog-update`, `README.md`, and `LICENSE`.
6. The workflow attaches archives, per-archive checksums, and aggregate `SHA256SUMS` to the
   GitHub Release.
7. GitHub artifact attestations are generated for the release assets.
8. For **stable** tags only, the workflow updates `SnapDogRocks/homebrew-tap`
   with `Formula/snapdog-update.rb`. Prerelease tags (a semver hyphen suffix,
   e.g. `snapdog-update-v0.1.0-beta.1` or `-rc.1`) still publish GitHub Release
   assets but skip the tap, so `brew install snapdogrocks/tap/snapdog-update`
   always resolves to the latest stable. The gate is the `meta.prerelease`
   output driving `if:` on the `update-homebrew` job.

The release job sets `SNAPDOG_UPDATE_VERSION=<version>` during the build so the
binary reports the package release version instead of the root OS image tag.

## Homebrew Formula

The formula should use the macOS archives from the GitHub Release:

```ruby
class SnapdogUpdate < Formula
  desc "Firmware update client for SnapDog OS"
  homepage "https://github.com/SnapDogRocks/snapdog-os"
  license "GPL-3.0-only"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/${TAG}/snapdog-update-${TAG}-x86_64-apple-darwin.tar.gz"
      sha256 "${MACOS_X64_SHA}"
    else
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/${TAG}/snapdog-update-${TAG}-aarch64-apple-darwin.tar.gz"
      sha256 "${MACOS_ARM64_SHA}"
    end
  end

  def install
    bin.install "snapdog-update"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/snapdog-update --version")
  end
end
```

## Operator Install Path

```bash
brew install snapdogrocks/tap/snapdog-update
```

Linux users can download the matching release archive directly, verify the
checksum, and install the binary into their preferred tool path.

## Required Secrets

- `HOMEBREW_TAP_TOKEN`: token with write access to
  `SnapDogRocks/homebrew-tap`.
