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

rust_msrv=$(sed -n 's/^  RUST_MSRV: "\([^"]*\)"$/\1/p' .github/workflows/ci.yml)
[ -n "$rust_msrv" ] || fail "could not read RUST_MSRV from CI"
rust_manifest_version=${rust_msrv%.*}
require_line dev/Dockerfile "ARG RUST_VERSION=$rust_msrv"
require_line snapdog-ctrl/Cargo.toml "rust-version = \"$rust_manifest_version\""
require_line snapdog-update/Cargo.toml "rust-version = \"$rust_manifest_version\""

buildroot_version=$(sed -n 's/^  BUILDROOT_VERSION: "\([^"]*\)"$/\1/p' .github/workflows/release.yml)
[ -n "$buildroot_version" ] || fail "could not read BUILDROOT_VERSION from release workflow"
require_line Makefile "--branch $buildroot_version"
require_line Makefile "tag $buildroot_version"
require_line dev/docker-compose.yml "--branch $buildroot_version"
require_line dev/docker-compose.yml "tag $buildroot_version"

echo "Version pins consistent: Node $node_version, Rust $rust_msrv, Buildroot $buildroot_version"
