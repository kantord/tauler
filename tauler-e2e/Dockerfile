# syntax=docker/dockerfile:1@sha256:ecfaec9ed6d810b56388c508f4121597bfbba70d41a6dfeee4d8cad5f295fc32
#
# A minimal X11 + i3 desktop with tauler running in it.
#
# tauler is compiled here rather than copied in from the host: a binary built on
# a rolling distro links a glibc newer than any stable base image ships, and
# glibc is forward-compatible only. See docs/adr/0004.
#
# Build context is the repo root, not this directory:
#   docker build -f tauler-e2e/Dockerfile .

FROM rust:1.97.1-bookworm@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97 AS builder

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

# dunst, rofi and kitty are here so that a rice can ship a real
# `~/.config/dunst/dunstrc`, a real `.rasi` and a real `kitty.conf`, and so that
# what each of them can and cannot paint is answered from evidence rather than
# from its documentation. Nothing outside a fixture's own `startup` runs them.
#
# xdotool and ffmpeg are the motion half of the harness. A settled screenshot is
# structurally incapable of answering anything about latency or flicker — it is
# defined as the frame after the screen stopped changing — so the questions round
# one could not touch need a desktop that moves and a camera pointed at it.
# ffmpeg writes PNG frames rather than a video: no encode, no yuv420p colour
# conversion, and the analysis compares exact pixels.
#
# kitty needs OpenGL, hence `libgl1-mesa-dri`: under Xvfb that resolves to
# llvmpipe. It was measured before being added — kitty maps in ~310ms on
# llvmpipe, which is well inside the harness's stability budget. It is the only
# terminal here that can be told to change its own background at runtime, which
# is the whole reason it is worth ~200MB of image.
RUN apt-get update && apt-get install -y --no-install-recommends \
        i3-wm \
        xvfb \
        x11-utils \
        x11-xserver-utils \
        xterm \
        rxvt-unicode \
        kitty \
        libgl1-mesa-dri \
        dunst \
        rofi \
        scrot \
        xdotool \
        ffmpeg \
        fontconfig \
        fonts-jetbrains-mono \
        libwayland-client0 libxkbcommon0 libfontconfig1 libxcb1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /artifacts/tauler /artifacts/tauler-i3 /usr/local/bin/

# /usr/local/share/fonts, not /usr/local/share/tauler/fonts: nothing reads the
# latter. tauler resolves fonts through fontconfig, and falls back to walking
# the platform font directories only when fontconfig answers nothing — neither
# path has ever looked under a `tauler/` subdirectory, so these fonts were
# invisible to the very binary they were shipped for.
#
# This is also what puts Symbols Nerd Font Mono where `append_symbol_fallback`
# probes for it. Without it every <Icon> in this image renders as tofu.
COPY assets/fonts /usr/local/share/fonts
COPY tauler-e2e/docker/fonts.conf /etc/fonts/conf.d/99-tauler-e2e.conf
RUN fc-cache -f
COPY tauler-e2e/docker/ /usr/local/share/e2e/
RUN chmod +x /usr/local/share/e2e/bin/*

# `~/.local/bin` is on PATH because a rice fixture installs its own scripts
# there, the way a real dotfiles repo does — a `startup` then says
# `monolith-pane 1` rather than naming a path under /fixtures.
ENV PATH="/usr/local/share/e2e/bin:/root/.local/bin:${PATH}" \
    HOME=/root \
    DISPLAY=:99 \
    SCREEN=1920x1080x24 \
    DPI=96

ENTRYPOINT ["e2e-entrypoint"]
