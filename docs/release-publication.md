# GitHub release publication

The trigger model, the publication order and the R2 lease are described in
[SnapDog OS release flow](os-release-flow.md) and
[snapdog-update release flow](snapdog-update-release-flow.md). This page keeps
the two rules that hold for every release and the record of one incident.

## Drafts are not optional

A release with downloadable assets must be created as a draft. Publishing an
immutable release locks its assets immediately, and a later upload fails with
HTTP 422 even when no asset was attached yet. Release Please therefore sets
`draft: true` and `force-tag-creation: true` for the root package and
`snapdog-update`; the tags exist while the releases are drafts, so version
discovery and the tag-driven builds still work. `snapdog-ctrl` stays
changelog-only, because its binary is embedded in the OS image.

## Failures and retries

A failed build or upload leaves the release a draft. Retry the failed jobs in
the original run, so the Release Please outputs and the artifacts that did
succeed are reused. Publication is idempotent: on a rerun the bytes already
committed to R2 are adopted as canonical before attestation or upload, and only
an incomplete catalog or channel pointer is repaired. A different build has to
receive a new version and a new tag.

## The empty releases v0.16.1 to v0.16.4

Those four releases were published immutable while still empty, so their assets
can never be attached. Retrying uploads does not repair them, and no workflow
change does either; the current model applies to future releases and deletes,
retags or recreates nothing. Their firmware reached R2 and is served from there.

## Local guards

```bash
make check-release-manifest
```

It runs every test under `scripts/tests`, which is what CI runs as well.
