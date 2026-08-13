# The e2e image compiles tauler; it does not copy it in

`tauler-e2e` runs tauler against a real X server and a real i3 in a container, so the
binaries have to get in there somehow. Bind-mounting the ones already sitting in
`target/release` is the obvious move — no compile step, no image rebuild when the code
changes. The image builds them from source instead.

## Why not bind-mount

glibc is forward-compatible only. The development machine is Arch, at glibc 2.44; Debian
bookworm ships 2.36 and Ubuntu 24.04 ships 2.39, so a locally built binary does not start
in the container at all — and the failure is a loader error before `main`, which the
harness cannot diagnose. CI has the same problem from the other direction, with a runner
glibc unrelated to the base image's.

The alternative was a base image at least as new as any machine that might build for it —
in practice Arch, which means a rolling base under a suite that wants to be reproducible.
Compiling inside the image stops the host mattering at all.

## Consequences

The build uses BuildKit cache mounts for the cargo registry and `target/`, which is what
keeps an incremental rebuild in the seconds. That is why `just e2e-image` runs `docker
buildx build` and testcontainers never builds the image: testcontainers goes through the
Docker Engine API's classic builder, which ignores `RUN --mount=type=cache`, so letting it
build would silently make every run a full rebuild of the dependency tree.

Two consequences fall out of that. `docker buildx` is required, and it is a plugin that is
not always installed alongside Docker — `just e2e-image` checks for it and says so. And
because `target/` lives in a cache mount, nothing under it survives the layer, so the
binaries are copied out inside the same `RUN` that produces them.
