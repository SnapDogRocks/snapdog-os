# snapdog-update Release Flow

This is the proposed release flow for shipping `snapdog-update` as a standalone
operator binary, aligned with the existing SnapDog client binary and Homebrew tap
pattern.

## Goals

- Publish reproducible release archives for macOS and Linux.
- Keep release artifacts separate from OS image and RAUC bundle artifacts.
- Update the SnapDog Homebrew tap automatically after a stable release.
- Preserve checksums and provenance for automation and operator trust.

## Workflow Shape

1. Use the existing `snapdog-update` release-please package to create versioned
   releases and tags.
2. Add a Rust-only build matrix for:
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
3. Package each build as:
   - `snapdog-update-${TAG}-${TARGET}.tar.gz`
   - `snapdog-update-${TAG}-${TARGET}.tar.gz.sha256`
4. Include `snapdog-update`, `README.md`, and `LICENSE` in each archive.
5. Attach archives, per-archive checksums, and aggregate `SHA256SUMS` to the
   GitHub Release.
6. Generate GitHub artifact attestations for the release assets.
7. Update `SnapDogRocks/homebrew-tap` with `Formula/snapdog-update.rb`.

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
