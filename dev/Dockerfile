FROM ubuntu:24.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential git wget curl xz-utils cpio unzip rsync bc \
    python3 file ca-certificates \
    libncurses-dev libssl-dev pkg-config \
    gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
    libavahi-compat-libdnssd-dev \
    dosfstools mtools e2fsprogs fdisk jq openssl \
    && rm -rf /var/lib/apt/lists/*

# Node.js
RUN wget -qO- "https://nodejs.org/dist/v22.13.0/node-v22.13.0-linux-$(dpkg --print-architecture | sed 's/amd64/x64/;s/arm64/arm64/').tar.xz" \
    | tar -xJ -C /usr/local --strip-components=1

# Rust (stable)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --profile minimal --target aarch64-unknown-linux-gnu

ENV PATH="/root/.cargo/bin:${PATH}" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /build
