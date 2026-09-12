# Dependency qualification

Dependency updates must pass the same checks as application changes. A neutral,
skipped, missing, or stale security check is not a successful security review.
Do not bypass a failing audit to merge a Dependabot update.

## Reproduce the checks

From `snapdog-ctrl/webui`:

```sh
npm ci --prefer-offline
npm audit --omit=dev --audit-level=high
npm audit --audit-level=high
npm run lint
npm run build
npm run typecheck
git diff --exit-code -- package.json package-lock.json
```

CI uses the Node version pinned in `.github/workflows/ci.yml`. Both CI and release
builds use `npm ci`, so an inconsistent lockfile fails instead of being silently
repaired by `npm install`. When intentionally updating dependencies, regenerate
the lockfile, inspect all changed package versions and integrity values, then
repeat the checks above. Platform-specific optional packages must remain in the
lockfile for Linux ARM64 and the other build hosts.

The WebUI is a static export embedded in Rust, with Next image optimization
disabled. Server-only advisories do not by themselves establish a deployed
exposure, but this does not justify leaving known-vulnerable build dependencies
or an inconsistent lockfile in the repository.

From the repository root:

```sh
cargo install cargo-audit --version 0.22.2 --locked
cargo audit --file snapdog-ctrl/Cargo.lock --deny warnings
cargo audit --file snapdog-update/Cargo.lock --deny warnings
make check-release-manifest
actionlint
```

Rust audits intentionally fail on unsound, unmaintained, and yanked warnings as
well as vulnerabilities. They run directly without a token or Check API writes,
including on Dependabot and fork PRs. Dependency fixes must also pass the Rust
MSRV job and the Linux ARM64 cross-build; local metadata resolution is not a
substitute for those builds.

## Overlapping Dependabot PRs

Inspect both manifests and lockfiles before declaring an update redundant. A
grouped Next.js update does not necessarily include a separate transitive Hono
update. Repair shared audit blockers first, then update the remaining PRs against
the merged base and qualify each resulting head again. Close redundant PRs only
after verifying that the merged dependency graph includes their fixes.

Disable an existing auto-merge request explicitly before repairing or investigating
a PR. The actor guard prevents re-enabling a hold; it does not cancel a request
that was already enabled. Never modify existing release tags or release assets
during dependency maintenance.

## CodeQL and merge policy

Default CodeQL setup can omit scans for Dependabot-triggered events. A maintainer
push can trigger the missing analyses; verify all five language configurations
(`actions`, `c-cpp`, `javascript-typescript`, `python`, `rust`) and the final CodeQL
result for the exact head before merging. Do not count a neutral result while a
language analysis is missing as qualification.

The repository currently requires linear history and squash merges. GitHub warns
that a Dependabot-authored squash commit can make default-branch CodeQL uploads
fail with read-only credentials. Use a maintainer-authored replacement PR when
necessary, verify its checks, then close the superseded bot PRs. See
[GitHub's CodeQL/Dependabot guidance](https://docs.github.com/en/code-security/reference/code-scanning/troubleshoot-analysis-errors/resource-not-accessible).

Automatic Dependabot merges are disabled by default. Do not set the repository
variable `DEPENDABOT_AUTOMERGE_ENABLED=true` until both the merge strategy and
required CodeQL checks have been explicitly configured and verified. The existing
four required CI checks alone do not enforce complete CodeQL coverage. Changing
that repository policy is separate from qualifying an individual update.
