FROM ubuntu:24.04

ARG NODE_VERSION=22.13.0
ARG RUST_VERSION=1.85.0

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential git wget curl xz-utils cpio unzip rsync bc \
    python3 file ca-certificates \
    libncurses-dev libssl-dev pkg-config \
    gcc-aarch64-linux-gnu \
    dosfstools mtools e2fsprogs fdisk jq openssl \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
      amd64) node_arch="x64"; rust_arch="x86_64-unknown-linux-gnu" ;; \
      arm64) node_arch="arm64"; rust_arch="aarch64-unknown-linux-gnu" ;; \
      *) echo "Unsupported architecture: $arch" >&2; exit 1 ;; \
    esac; \
    wget -qO- "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" \
      | tar -xJ -C /usr/local --strip-components=1; \
    wget -qO /tmp/rustup-init "https://static.rust-lang.org/rustup/archive/1.27.1/${rust_arch}/rustup-init"; \
    chmod +x /tmp/rustup-init; \
    /tmp/rustup-init -y --profile minimal --default-toolchain "${RUST_VERSION}" --target aarch64-unknown-linux-gnu; \
    rm -f /tmp/rustup-init

ENV PATH="/root/.cargo/bin:${PATH}" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

WORKDIR /build
