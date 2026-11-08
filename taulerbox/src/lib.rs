//! taulerbox: a native window that shows a tauler layout's Panels as pixel
//! buffers, alongside a microVM compartment. See the crate README and issue
//! #582.
//!
//! This is the library half — `main.rs` is the CLI over it, and owns the window.
//! [`compose`] is the pure geometry that places each Panel's frame in that window.

pub mod compose;
