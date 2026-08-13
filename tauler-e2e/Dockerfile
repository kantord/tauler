# syntax=docker/dockerfile:1
#
# A minimal X11 + i3 desktop with tauler running in it.
#
# tauler is compiled here rather than copied in from the host: a binary built on
# a rolling distro links a glibc newer than any stable base image ships, and
# glibc is forward-compatible only. See docs/adr/0004.
#
# Build context is the repo root, not this directory:
#   docker build -f tauler-e2e/Dockerfile .

FROM rust:1.97.1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        libwayland-dev libxkbcommon-dev libfontconfig-dev libxcb1-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .

# The cache mounts are what make an in-container build tolerable, and they are
# BuildKit-only — which is why the image is built by `just e2e-image` rather
# than by testcontainers, whose builder does not support them.
#
# `target/` lives in a cache mount, so the binaries must be copied out inside
# the same RUN: nothing under it survives the layer.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release -p tauler -p tauler-i3 \
    && mkdir -p /artifacts \
    && cp target/release/tauler target/release/tauler-i3 /artifacts/

FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241

RUN apt-get update && apt-get install -y --no-install-recommends \
        i3-wm \
        xvfb \
        x11-utils \
        x11-xserver-utils \
        xterm \
        scrot \
        fonts-jetbrains-mono \
        libwayland-client0 libxkbcommon0 libfontconfig1 libxcb1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /artifacts/tauler /artifacts/tauler-i3 /usr/local/bin/
COPY assets/fonts /usr/local/share/tauler/fonts
COPY tauler-e2e/docker/ /usr/local/share/e2e/
RUN chmod +x /usr/local/share/e2e/bin/*

ENV PATH="/usr/local/share/e2e/bin:${PATH}" \
    HOME=/root \
    DISPLAY=:99 \
    SCREEN=1920x1080x24 \
    DPI=96

ENTRYPOINT ["e2e-entrypoint"]
