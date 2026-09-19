//! taulerbox: a native window that shows a tauler layout's Panels as pixel
//! buffers, alongside a microVM compartment. See the crate README and issue
//! #582.
//!
//! This is the library half — `main.rs` is the thin CLI over it. Nothing here
//! talks to a display server yet; [`compose`] is the first real piece, the
//! pure geometry taulerbox needs before it can blit anything into a window.

pub mod compose;
