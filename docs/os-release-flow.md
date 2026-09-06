# SnapDog OS Release Flow

SnapDog OS uses tag-driven artifact builds. Release orchestration and artifact
publication are deliberately separate so one source commit cannot start an
automatic beta build immediately followed by the same stable build.

## Trigger Model

1. A push to `main` runs `.github/workflows/release-please.yml` only.
2. Release Please updates its shared release PR and enables squash auto-merge.
3. Merging that PR creates protected component tags and draft releases with
   `TAP_TOKEN`. The PAT is intentional: tags created by `GITHUB_TOKEN` do not
   start another workflow. Drafts keep incomplete releases out of the public
   feed while their artifacts build.
4. An exact root tag `vX.Y.Z` starts `.github/workflows/release.yml` and publishes
   the stable OS artifacts once. Component tags such as `snapdog-update-vX.Y.Z`
   do not match this trigger.
5. A beta is built only through `workflow_dispatch` from `main`. The immutable
   commit SHA carried by that dispatch event is used by every build job and by
   the published manifest. A delayed job never silently switches to a newer
   `main`, and arbitrary refs are rejected before jobs can use the RAUC signing
   key.

The metadata job rejects malformed root tags and requires the tag version to
match the root entry in `.release-please-manifest.json` before an image runner
is allocated.

## Beta Versioning

An explicit beta uses the pending root version from the open Release Please
branch. If there is no pending release, it previews the next patch version. Its
full version is `X.Y.Z-beta.<workflow-run-number>` and its base version must be
strictly newer than the latest stable version.

Beta builds may run concurrently, so GitHub cannot discard an older pending
dispatch. Their R2 publication is serialized by the conditional-write lease.
Inside that lease the candidate is compared with both `latest-release.json` and
`latest-beta.json`. An obsolete beta therefore exits successfully without
changing any rolling pointer, even if a stable release completed while its
images were building.

## Build and Publication Graph

```text
release metadata
  -> build snapdog-ctrl
    -> build image [pi3, pi4, pi5, zero2w]
      -> publish to R2 and GitHub
        -> redeploy snapdog-web (stable only)
```

The publish job validates all four board images, bundles, checksums, SBOMs, and
metadata before the first remote mutation. Every manifest contains the exact
versioned RAUC `bundle_url`, so OTA clients never combine one manifest version
with a newer rolling bundle alias. Publication then writes in this order:

1. Stage versioned image, RAUC bundle, SBOM, and manifest objects without moving
   a public channel pointer.
2. Attest the adopted canonical bytes and upload/byte-verify every stable asset
   on the GitHub draft.
3. Publish the bounded catalog, replace affected legacy aliases with a
   non-cacheable invalid transition object, and wait out their previous
   300-second cache lifetime. A legacy updater can retry here but cannot install
   a bundle for the wrong manifest version.
4. For stable releases, publish `latest-beta.json` first, then publish
   `latest-release.json`; beta therefore remains stable-or-newer at every
   externally visible instant.
5. After the old 60-second manifest caches expire, restore each compatibility
   alias directly from its immutable versioned object with `no-store`. A failed
   cutover restores aliases from the still-authoritative channel pointers before
   releasing the lease.
6. Publish the already verified root and snapdog-ctrl drafts. Only a new/current
   root release may claim the repository's `Latest` pointer; historical repairs
   explicitly leave it unchanged.
7. Remove superseded entries from the beta catalog. Retention removes each
   superseded immutable manifest first and records a durable retirement marker;
   its payloads remain until the manifest's full cache lifetime plus a 24-hour
   active-download grace has elapsed.

All release, retention, and catalog mutations acquire the same lease object with
atomic R2 `If-None-Match`/`If-Match` writes. This is a real queue: unlike GitHub
Actions concurrency groups, it cannot discard an older pending release when a
third run arrives. A version manifest commits the immutable object set. On a
rerun, R2's committed bytes are adopted as the canonical local artifacts before
attestation or GitHub upload; only incomplete catalogs or channel pointers are
repaired. Markerless partial uploads may be replaced under the lease. A
different build must receive a new version and tag.

An hourly retention backstop repairs every rolling alias directly from the
exact version manifest committed by its channel pointer before pruning. This is
the independent recovery path for a hard runner loss that prevents the release
job's `always()` recovery step from running; it never treats an alias as a
source of truth.

GitHub uses separate release-tag rulesets: organization administrators (including
the Release Please PAT owner) may create `v*`, `snapdog-ctrl-v*`, and
`snapdog-update-v*`, but nobody—including administrators—may move or delete an
existing release tag. Repository release immutability additionally locks every
future release and its assets when its draft is published. Stable, beta, and
updater release environments accept only their exact branch/tag patterns. The
R2 maintenance environment accepts only `main`, so a manually dispatched
workflow from an unreviewed branch cannot receive update-bucket credentials.

## Component Boundaries

The root Release Please package excludes `snapdog-update`-only changes. The
standalone updater is released exclusively by
`.github/workflows/release-snapdog-update.yml` from `snapdog-update-vX.Y.Z`
tags. `snapdog-ctrl` remains part of the root package because its binary is
embedded in every OS image.
