# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
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
FROM debian:trixie@sha256:9cc080028c43b27d2074d63a5f9caf7166d731494965616c1a6d2827a004585c

RUN apt-get update && \
    apt-get install -y --no-install-recommends sway wayvnc && \
    rm -rf /var/lib/apt/lists/*
