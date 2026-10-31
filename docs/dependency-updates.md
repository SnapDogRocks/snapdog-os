# Dependency qualification

Dependency updates must pass the same checks as application changes. A neutral,
skipped, missing, or stale security check is not a successful security review.
Do not bypass a failing audit to merge a Dependabot update.

## Reproduce the checks

From `snapdog-ctrl/webui`:

```sh
pnpm install --frozen-lockfile --prefer-offline
pnpm audit --prod --audit-level high
pnpm audit --audit-level high
pnpm run lint
pnpm run build
pnpm run typecheck
git diff --exit-code -- package.json pnpm-lock.yaml pnpm-workspace.yaml
```

CI uses the Node version pinned in `.github/workflows/ci.yml` and the pnpm version
from `packageManager` in `package.json`. Both CI and release builds install with
`--frozen-lockfile`, so an inconsistent lockfile fails instead of being silently
repaired. When intentionally updating dependencies, regenerate the lockfile,
inspect all changed package versions and integrity values, then repeat the checks
above. Platform-specific optional packages must remain in the lockfile for Linux
ARM64 and the other build hosts.

Dependency overrides and the packages allowed to run install scripts live in
`pnpm-workspace.yaml`. pnpm ignores an npm-style `overrides` block in
`package.json`, and it refuses to install while a package with an unapproved
install script is present.

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
including on Dependabot and fork PRs. Dependency fixes must also pass the Linux
ARM64 cross-build; local metadata resolution is not a substitute for that build.

The Rust version lives in `rust-toolchain.toml` only. CI, the release builds and
the dev image use it, and Dependabot proposes new versions in a pull request of
their own. No crate is published to crates.io, so there is no separate minimum
version and no `rust-version` in the manifests.

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

CodeQL runs from `.github/workflows/codeql.yml` (advanced setup), not from
GitHub's managed default setup. The managed workflow never ran for pushes by
`dependabot[bot]`, so no Dependabot PR could pass `CodeQL Coverage` without a
maintainer push. The workflow analyses the same five languages (`actions`,
`c-cpp`, `javascript-typescript`, `python`, `rust`) with `build-mode: none`, the
default query suite and the categories `/language:<language>`, so alerts from the
former default setup continue. It runs on pull requests, on pushes to `main`,
weekly and on demand.

GitHub rejects uploads from this workflow while default setup is enabled.
Default setup must stay off (Settings, Code security, CodeQL analysis). Runs that
Dependabot triggers start with a read-only token. The job requests
`security-events: write`, which GitHub honours for these runs, and code scanning
accepts uploads from `pull_request` runs in any case. Do not count a neutral
result while a language analysis is missing as qualification.

The repository currently requires linear history and squash merges. GitHub warns
that a Dependabot-authored squash commit can make default-branch CodeQL uploads
fail with read-only credentials. Use a maintainer-authored replacement PR when
necessary, verify its checks, then close the superseded bot PRs. See
[GitHub's CodeQL/Dependabot guidance](https://docs.github.com/en/code-security/reference/code-scanning/troubleshoot-analysis-errors/resource-not-accessible).

The default-branch ruleset requires `Lint & Test`, `Security Audit`,
`Cross-compile (aarch64)`, `Release Sanity`, `CodeQL Coverage`, and
the final `CodeQL` check. This also applies to Release Please auto-merges.
The new checks are bound to their GitHub integration IDs, not merely their names.

`CodeQL Coverage` reads the `codeql.yml` workflow for the exact PR head. It
requires the latest run attempt and all five language jobs to finish
successfully, followed by a successful CodeQL result from integration `57789`.
Missing, skipped, neutral, stale, and failed results fail closed after a bounded
wait. Code Quality's similarly named jobs cannot substitute for security
analysis. `scripts/tests/test_codeql_coverage.py` checks that the workflow's
language matrix, job names and categories match the gate. Change both together.

Automatic Dependabot merges remain disabled. Do not set the repository variable
`DEPENDABOT_AUTOMERGE_ENABLED=true` until the squash-author limitation and full
PR/default-branch coverage have been verified for bot-authored updates. Required
checks are not a reason to bypass an absent scan.
