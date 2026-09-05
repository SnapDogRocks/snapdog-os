# GitHub release publication

GitHub releases with downloadable assets must be created as drafts. Publishing
an immutable release locks its assets immediately; uploading after publication
fails with HTTP 422, even if no assets were attached yet.

Release Please sets `draft: true` and `force-tag-creation: true` for the root OS
package and `snapdog-update`. The tags exist while the releases are drafts, so
version discovery and the standalone updater's tag-triggered build still work.
`snapdog-ctrl` releases remain changelog-only; its binary is embedded in the OS.

## OS releases

1. Release Please creates the tag and draft release.
2. The firmware and updater build matrices run.
3. The `publish` job uploads firmware to R2 and the draft GitHub release;
   `publish-update-tool` uploads the six updater archives and their checksums.
4. `finalize-release` waits for both upload jobs and verifies all 28 required
   assets by name, uploaded state, and nonzero size before publishing the draft.
5. The Homebrew update and website redeploy run after successful publication.

The 28 assets comprise images, RAUC bundles, SBOMs, and checksum files for four
boards, plus archives and checksums for six updater targets. When a target or
asset format changes, update `scripts/finalize_release.py` alongside the matrix.
Beta builds keep using R2 without creating or finalizing a GitHub OS release.

## Standalone updater releases

The tag-triggered workflow uploads to the Release Please draft, or creates a
draft for a manually pushed tag. It verifies four archives, four per-archive
checksums, and `SHA256SUMS` before publication. Prereleases retain their flag and
skip Homebrew; updater releases do not replace the OS release's `Latest` marker.

## Failures and retries

If a build or upload fails, the release stays a draft. Retry the failed jobs in
the original run so the Release Please outputs and successful artifacts are
reused. The finalization step is safe to repeat for an already complete release.

The empty immutable releases `v0.16.1` through `v0.16.4` cannot be repaired by
retrying uploads. This workflow change applies to future releases; it does not
delete, retag, or recreate existing releases. Their firmware was published to R2.

Run `make check-release-publication check-release-manifest` for the release guards.
