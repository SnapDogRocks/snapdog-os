# snapdog-update Release Flow

`snapdog-update` ships as a standalone operator binary, aligned with the
existing SnapDog client binary and Homebrew tap pattern.

## Goals

- Publish reproducible release archives for macOS, Linux, and Windows.
- Keep release artifacts separate from OS image and RAUC bundle artifacts.
- Update the SnapDog Homebrew tap automatically after a stable release.
- Preserve checksums and provenance for automation and operator trust.

## Workflow Shape

1. Release Please manages the `snapdog-update` package version and changelog.
2. Binary releases are triggered only by a pushed strict SemVer tag in the form
   `snapdog-update-v<version>`; prerelease suffixes are supported. There is no
   manually supplied tag input: reruns use the original tag-push event and its
   immutable source revision.
3. Before building, the workflow resolves the tag to a commit and requires it
   to match the event SHA. The tag version must also match both the
   `snapdog-update` entry in `.release-please-manifest.json` and the package
   version in `snapdog-update/Cargo.toml`. Every matrix checkout is pinned to
   that validated commit, so the archives and GitHub provenance describe the
   same source.
4. `.github/workflows/release-snapdog-update.yml` builds a Rust-only matrix for:
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-pc-windows-msvc`
   - `aarch64-pc-windows-msvc`
5. Each build is packaged as:
   - macOS/Linux: `${TAG}-${TARGET}.tar.gz` and its `.sha256`
   - Windows: `${TAG}-${TARGET}.zip` and its `.sha256`
   - where `TAG` already includes the component name, for example
     `snapdog-update-v0.5.0-x86_64-apple-darwin.tar.gz`
6. Each archive contains the platform binary, `README.md`, and `LICENSE`.
7. Release Please creates a draft plus its protected tag. The workflow creates
   reproducible archives, refuses to replace an existing asset with different
   bytes, attaches six archives, six per-archive checksums, and aggregate
   `SHA256SUMS` (13 assets total), then re-downloads and byte-compares the exact
   13-asset set immediately before publishing the draft with `latest=false`.
   Publishing activates GitHub release immutability for its tag and assets.
8. GitHub artifact attestations are generated for the release assets.
9. For **stable** tags only, the workflow proposes `Formula/snapdog-update.rb`
   to `SnapDogRocks/homebrew-tap` as a pull request, waits for that tap's own
   `Formula qualification`, rechecks the head and the branch guard, and merges
   only then. Prerelease tags (a semver hyphen suffix, e.g.
   `snapdog-update-v0.1.0-beta.1`) still publish GitHub Release assets but skip
   the tap, so `brew install snapdogrocks/tap/snapdog-update` always resolves to
   the latest stable. The gate is the `meta.prerelease` output driving `if:` on
   the `update-homebrew` job.

Release jobs use the protected `updater-release` environment, which accepts only
`snapdog-update-v*` tags and owns the Homebrew credential. One repository
ruleset permits organization administrators and the Release Please app to create
release tags; a second ruleset prevents everyone, administrators included, from
moving or deleting one after creation. Metadata additionally
requires the tagged commit to be an ancestor of protected `main`, and the tag is
re-resolved immediately before draft mutation and publication.

The release job sets `SNAPDOG_UPDATE_VERSION=<version>` during the build so the
binary reports the package release version instead of the root OS image tag.

## Homebrew Formula

The formula should use the macOS archives from the GitHub Release:

```ruby
class SnapdogUpdate < Formula
  desc "Firmware update client for SnapDog OS"
  homepage "https://github.com/SnapDogRocks/snapdog-os"
  license "GPL-3.0-only"
  version_scheme 1

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/${TAG}/${TAG}-x86_64-apple-darwin.tar.gz"
      sha256 "${MACOS_X64_SHA}"
    else
      url "https://github.com/SnapDogRocks/snapdog-os/releases/download/${TAG}/${TAG}-aarch64-apple-darwin.tar.gz"
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

The historical formula used the OS release version `0.16.6`, although its
executable was updater `0.4.1`. Migration to updater `0.4.2` must not compare
those two version namespaces numerically. `version_scheme 1` makes the new
package version an upgrade in Homebrew and must remain in subsequent formulas.
Do not add an explicit `version` duplicating the version inferred from the URL.

Tap updates are serialized across stable tags. Check registration and completion
are separate: a pending check is registered, not missing. Retries must reuse an
existing matching PR or stop without deleting its branch. Before merging, check
the current formula again, the unchanged PR head, and effective PR/required-check
rules. A generic `protected: true` flag alone does not prove qualification is
required. The tap requires `Formula qualification`, including installs on Intel
and ARM macOS and `brew test` for the updater version.

## Recover a failed tap update without rebuilding

A failed Homebrew job does not imply that the GitHub release failed. First inspect
the published release and its complete asset set. Download the existing archives
and `SHA256SUMS` to a new directory, verify all checksums, and independently
measure both macOS archive SHA256 values. Propose only the formula migration
through a tap PR; require both macOS installation/version checks before merging.
Do not overwrite release assets, move tags, or rebuild binaries for this repair.

Rerunning an old tagged workflow uses that old workflow's code. A publisher fix
on `main` does not change an already published tag. For an old failed release,
use the verified-asset PR recovery above rather than repeatedly rerunning the
same broken job. The tag-only `updater-release` environment restriction remains
unchanged; there is no privileged arbitrary-ref recovery dispatch.

## Operator Install Path

```bash
brew install snapdogrocks/tap/snapdog-update
```

Linux and Windows users can download the matching release archive directly,
verify the checksum, and install the binary into their preferred tool path.

## Required Secrets

- `updater-release` environment variable `HOMEBREW_APP_CLIENT_ID` and secret
  `HOMEBREW_APP_PRIVATE_KEY`: the GitHub App installed on
  `SnapDogRocks/homebrew-tap` alone. The job mints a token scoped to that one
  repository, opens a pull request, waits for the tap's own `Formula
  qualification` and merges only then. Nothing pushes onto the tap's default
  branch, and no long-lived credential is stored for it.
