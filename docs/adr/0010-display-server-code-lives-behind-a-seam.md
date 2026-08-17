# The core is independent of any display server

Nothing outside the windowing layer should know what a display server is. JSX evaluation,
layout, rasterization and the main loop produce and move a BGRX buffer; what a window is,
how that buffer reaches a screen, and where clicks come from are the backend's business.
This is the target, and it is not fully true today.

## Why

There are at least three answers to "what is a bar" — an override-redirect X11 window, a
wlr-layer-shell surface, a macOS window — and nothing above the buffer cares which. Every
x11rb call that leaks upward is code the next backend has to rewrite for reasons that have
nothing to do with it.

## Where it holds, and where it does not

Adding Wayland (`b0fb177`) is the one real measurement of this. It went half well.

**Held:** `src/jsx.rs` and `src/render/` were not touched at all — a 924-line second
backend arrived without moving a line of evaluation or rasterization.

**Did not hold:** the same commit rewrote 531 lines of `src/main.rs`, and changed
`src/managed_set/` (143 lines), the data layer (44) and `src/layout/`. It also grew the
*existing* X11 backend by 576 lines. Surface lifecycle is where the leak is: what a surface
is, when it is created and destroyed, and who owns reconciliation were all entangled with
X11 specifics.

## Consequences

Treat the lifecycle entanglement as debt, not as the design. `display_manager.rs`,
`surface/`, `presentation/` and `windowing/` exist because of that commit — each one is a
step toward the target, and the fact that they were extracted *after* the second backend
landed is why the seam is cleaner now than it was then. A third backend is the next
measurement.

The rule for new code is unchanged and unconditional: display-server calls belong in a
backend module. Anything in the core loop that needs to branch on which display server is
running is a bug to file, not a pattern to copy.

## The third measurement

The web renderer ([0024](0024-the-web-renderer-emits-dom.md)) is the third backend this
asked for, and it does not merely restate the rule — it compiles it. The wasm-clean subset
moves into `tauler-core`, which cannot reach x11rb, smithay, takumi or rquickjs because it
does not depend on `tauler` at all. Where the Wayland commit relied on discipline and lost
531 lines of `main.rs` to the leak, a crate boundary fails at build time on the first
stray `use`.

