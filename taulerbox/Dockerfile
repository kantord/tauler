# syntax=docker/dockerfile:1
#
# The compartment's desktop image: headless Sway + wayvnc baked in, so
# `msb create` boots straight into a ready-to-start desktop instead of
# installing them at runtime.
#
# Runtime install was the first design (issue #582) and it broke live:
# `apt-get install sway wayvnc` pulls in every Recommends package too —
# measured at 253 packages on a real run, including a full ffmpeg codec
# set, PulseAudio, GPU drivers (mesa/intel/vdpau/va) and a terminal
# emulator, none of which a headless VNC-only compositor touches. That
# reliably outran the `msb exec --timeout` window on real network
# conditions. `--no-install-recommends` here cuts it back to just what
# sway/wayvnc actually need, baked in at build time instead of gambled on
# at every compartment's first boot.
FROM debian:trixie

RUN apt-get update && \
    apt-get install -y --no-install-recommends sway wayvnc && \
    rm -rf /var/lib/apt/lists/*
