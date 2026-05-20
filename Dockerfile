FROM ubuntu:24.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential git wget cpio unzip rsync bc \
    python3 file ca-certificates \
    libncurses-dev libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
