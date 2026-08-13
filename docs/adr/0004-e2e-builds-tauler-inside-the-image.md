# 0004 — The e2e image compiles tauler; it does not copy it in

## Status

Accepted.

## Context

`tauler-e2e` runs tauler against a real X server and a real i3, in a container.
The binaries have to get in there somehow. Bind-mounting the ones already sitting
in `target/release` is the obvious move: no compile step, no image rebuild when
the code changes, and the container image stays a stable, cacheable thing.

It does not work here. The development machine is Arch, whose glibc is 2.44;
Debian bookworm ships 2.36 and Ubuntu 24.04 ships 2.39. glibc is
forward-compatible only — a binary linked against a newer one does not start on
an older one, and the failure is a loader error before `main`, not something the
harness can diagnose. The same applies in CI, where the runner's glibc and the
base image's are unrelated.

Two ways out. Use a base image whose glibc is at least as new as every machine
that might build a binary for it — in practice an Arch base, which means a
rolling image under a suite that wants to be reproducible. Or compile inside the
image and stop caring what the host is.

## Decision

The image builds tauler from source in a builder stage, with the runtime stage
carrying only the two binaries and the desktop packages.

The build uses BuildKit cache mounts for the cargo registry and `target/`, which
is what keeps an incremental rebuild in the seconds. That has a consequence:
`just e2e-image` runs `docker buildx build`, and testcontainers never builds the
image. testcontainers builds through the Docker Engine API's classic builder,
which ignores `RUN --mount=type=cache`, so letting it build would silently turn
every run into a full rebuild of the dependency tree.

## Consequences

Running the suite is two steps, not one: build the image, then run the tests.
`just e2e` does both.

`docker buildx` is required. It ships as a plugin, and is not always installed
alongside Docker — `just e2e-image` checks for it and says so rather than
failing inside the build with a message about BuildKit.

Because `target/` lives in a cache mount, nothing under it survives the layer.
The binaries are copied out inside the same `RUN` that produces them.
