#!/bin/sh
set -eu

fail() {
  echo "Version pin mismatch: $*" >&2
  exit 1
}

require_line() {
  file=$1
  line=$2
  grep -Fq -- "$line" "$file" || fail "$file does not contain: $line"
}

node_version=$(tr -d '[:space:]' < snapdog-ctrl/webui/.nvmrc)
require_line dev/Dockerfile "ARG NODE_VERSION=$node_version"
require_line .github/workflows/ci.yml "NODE_VERSION: \"$node_version\""
require_line .github/workflows/release.yml "NODE_VERSION: \"$node_version\""

# packageManager in the WebUI's package.json is the only source of the pnpm
# version: pnpm/action-setup reads it in CI and the release, the dev image pins it.
pnpm_version=$(sed -n 's/^  "packageManager": "pnpm@\([^"]*\)",$/\1/p' snapdog-ctrl/webui/package.json)
[ -n "$pnpm_version" ] || fail "could not read packageManager from snapdog-ctrl/webui/package.json"
require_line dev/Dockerfile "ARG PNPM_VERSION=$pnpm_version"

# rust-toolchain.toml is the only source of the Rust version: CI and the release
# builds install it, the dev image pins the same one, and nothing else names one.
rust_version=$(sed -n 's/^channel = "\([^"]*\)"$/\1/p' rust-toolchain.toml)
[ -n "$rust_version" ] || fail "could not read the channel from rust-toolchain.toml"
require_line dev/Dockerfile "ARG RUST_VERSION=$rust_version"
if grep -nE 'RUST_TOOLCHAIN|RUST_MSRV|rustup default|rustup toolchain install [^ ]' .github/workflows/*.yml; then
  fail "a workflow chooses its own Rust toolchain instead of rust-toolchain.toml"
fi
if grep -n '^rust-version' snapdog-ctrl/Cargo.toml snapdog-update/Cargo.toml; then
  fail "rust-version is set although no crate is published to crates.io"
fi

buildroot_version=$(sed -n 's/^  BUILDROOT_VERSION: "\([^"]*\)"$/\1/p' .github/workflows/release.yml)
[ -n "$buildroot_version" ] || fail "could not read BUILDROOT_VERSION from release workflow"
require_line Makefile "--branch $buildroot_version"
require_line Makefile "tag $buildroot_version"
require_line dev/docker-compose.yml "--branch $buildroot_version"
require_line dev/docker-compose.yml "tag $buildroot_version"

# The dev image reproduces the host that builds the release images, so both use
# the same Ubuntu release. Jobs that only publish may run on ubuntu-latest.
ubuntu_release=$(sed -n 's/^    runs-on: ubuntu-\([0-9][0-9]\.[0-9][0-9]\)$/\1/p' .github/workflows/release.yml | sort -u)
[ -n "$ubuntu_release" ] || fail "could not read a pinned Ubuntu runner from release workflow"
[ "$(printf '%s\n' "$ubuntu_release" | grep -c .)" = 1 ] || fail "release workflow pins more than one Ubuntu release: $(printf '%s' "$ubuntu_release" | tr '\n' ' ')"
require_line dev/Dockerfile "FROM ubuntu:$ubuntu_release"

echo "Version pins consistent: Node $node_version, pnpm $pnpm_version, Rust $rust_version, Buildroot $buildroot_version, Ubuntu $ubuntu_release"
