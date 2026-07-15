FROM ubuntu:24.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential git wget curl xz-utils cpio unzip rsync bc \
    python3 file ca-certificates \
    libncurses-dev libssl-dev pkg-config \
    gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
    libavahi-compat-libdnssd-dev \
    dosfstools mtools e2fsprogs fdisk jq openssl \
    && rm -rf /var/lib/apt/lists/*

# Node.js — pinned + checksum-verified (kept in lockstep with .nvmrc / CI NODE_VERSION
# by scripts/check-version-pins.sh).
ARG NODE_VERSION=26.5.0
RUN arch="$(dpkg --print-architecture | sed 's/amd64/x64/;s/arm64/arm64/')" \
    && archive="node-v${NODE_VERSION}-linux-${arch}.tar.xz" \
    && wget -q "https://nodejs.org/dist/v${NODE_VERSION}/${archive}" \
    && wget -q "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" \
    && grep "  ${archive}$" SHASUMS256.txt | sha256sum -c - \
    && tar -xJf "${archive}" -C /usr/local --strip-components=1 \
    && rm -f "${archive}" SHASUMS256.txt

# Rust — pinned to the project MSRV + checksum-verified (kept in lockstep with
# CI RUST_MSRV / Cargo.toml rust-version by scripts/check-version-pins.sh).
ARG RUST_VERSION=1.88.0
RUN case "$(dpkg --print-architecture)" in \
      amd64) rust_target=x86_64-unknown-linux-gnu ;; \
      arm64) rust_target=aarch64-unknown-linux-gnu ;; \
      *) echo "Unsupported architecture" >&2; exit 1 ;; \
    esac \
    && rustup_url="https://static.rust-lang.org/rustup/dist/${rust_target}" \
    && wget -q "${rustup_url}/rustup-init" \
    && wget -q "${rustup_url}/rustup-init.sha256" \
    && sha256sum -c rustup-init.sha256 \
    && chmod +x rustup-init \
    && ./rustup-init -y --profile minimal --default-toolchain "${RUST_VERSION}" \
      --target aarch64-unknown-linux-gnu \
    && rm -f rustup-init rustup-init.sha256

ENV PATH="/root/.cargo/bin:${PATH}" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /build
