#!/bin/sh
# Linux releases of the game app, built in Docker (Ubuntu 22.04, so they run on glibc 2.35 and newer):
#   dist/loveplus_VERSION_amd64.deb and dist/LovePlus+-VERSION-x86_64.AppImage
# The program carries the translation and the notices inside it (feature `embedded`). Nothing from the game is in
# either package: the app reads the player's ROM on first run. Run from macOS or Linux with Docker; ARCH=arm64 for ARM.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ARCH=${ARCH:-amd64}
[ -f "$ROOT/dist/translation-en.lpdelta" ] || { echo "no dist/translation-en.lpdelta: python3 tools/lp_delta.py build content patch dist/translation-en.lpdelta" >&2; exit 1; }
docker build --platform "linux/$ARCH" -t "lp-engine-linux-$ARCH" - <<'DOCKERFILE'
FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl build-essential pkg-config \
    libasound2-dev libudev-dev libgtk-3-dev file python3 && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH=/root/.cargo/bin:$PATH
DOCKERFILE
docker run --rm --platform "linux/$ARCH" -v "$ROOT:/src" -v "lp-engine-cargo-$ARCH:/root/.cargo/registry" \
    -e ARCH="$ARCH" "lp-engine-linux-$ARCH" sh /src/tools/package/linux_inner.sh
