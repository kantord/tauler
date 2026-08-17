//! The part of tauler that draws nothing.
//!
//! Everything here runs identically on a desktop and in a browser: the UI components, the
//! theme layer that rewrites theme tokens into ordinary Tailwind, and the walk that turns
//! a layout tree into markup. Nothing here rasterizes, opens a window, spawns a process or
//! knows what a display server is.
//!
//! That is a compiler-enforced claim, not a convention. This crate does not depend on
//! `tauler`, so it cannot reach x11rb, smithay or takumi however carelessly someone writes
//! a `use`; `cargo check -p tauler-core --target wasm32-unknown-unknown` is the check that
//! says so. See `docs/adr/0010`, "The third measurement".
//!
//! The one seam back to the desktop is the `quickjs` feature, which adds the registration
//! that puts these components into a QuickJS realm. The browser needs none of it — it has
//! a JavaScript engine already (ADR 0025).

pub mod dom;
pub mod flatten;
pub mod globals;
pub mod preview;
pub mod theme;
pub mod ui;

#[cfg(target_arch = "wasm32")]
pub mod web;
