//! Every name that starts `tauler:`, in one place.
//!
//! The prefix marks a name tauler answers itself, never a file to read or a program
//! to run. The constants live here so the names can be found together; each is
//! implemented where the table says.
//!
//! | Name | Kind | Written by | Implemented in |
//! |---|---|---|---|
//! | `tauler:outputs` | stream | layout author | [`crate::x11::outputs`], behind the display-server seam (ADR 0010) |
//! | `tauler:root-bg` | image | layout author | [`crate::backdrop`] |
//! | `tauler:picture/<key>` | image | internal (`<D3Figure>` emits it) | [`crate::d3_figure`], [`crate::render::picture`] |
//! | `tauler:d3` | import | internal (the `"d3"` specifier resolves to it) | [`crate::jsx`] |
//! | `tauler:linkedom` | import | internal (draw runtime only) | [`crate::jsx`] |
//! | `tauler:draw-bootstrap-<n>` | module name | internal (the draw runtime's bootstrap) | [`crate::jsx`] |
//!
//! `tauler-core` cannot depend on this crate (it is compiled to wasm, ADR 0010), so it
//! keeps its own copies of the scheme and the picture prefix; a test in
//! `tests/scheme_test.rs` holds them equal. `tauler-web/js/runtime.js` hardcodes no
//! `tauler:` name.

/// The prefix every name here starts with.
pub const SCHEME: &str = "tauler:";

/// The built-in stream of connected Screens, read with `useJSONStream("tauler:outputs")`.
pub const OUTPUTS: &str = "tauler:outputs";

/// The image key a layout uses to reach its backdrop: `<img src="tauler:root-bg">`.
pub const ROOT_BG: &str = "tauler:root-bg";

/// The `src` prefix an `<img>` names a Picture by; the rest is `<key>@<w>x<h>`.
pub const PICTURE_PREFIX: &str = "tauler:picture/";

/// The module name the `"d3"` specifier resolves to.
pub const D3_MODULE: &str = "tauler:d3";

/// The module name (and specifier) the draw runtime imports linkedom by.
pub const LINKEDOM_MODULE: &str = "tauler:linkedom";

/// The module name of the draw runtime's bootstrap on attempt `attempt`.
pub fn draw_bootstrap_module(attempt: u32) -> String {
    format!("tauler:draw-bootstrap-{attempt}")
}
