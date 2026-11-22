//! `tauler-core` cannot depend on the root crate (wasm boundary, ADR 0010), so it keeps
//! its own copies of the `tauler:` names it needs. They must not drift.

use tauler::{dom, scheme};

#[test]
fn tauler_core_names_equal_the_root_crates() {
    assert_eq!(dom::TAULER_SCHEME, scheme::SCHEME);
    assert_eq!(dom::PICTURE_PREFIX, scheme::PICTURE_PREFIX);
}
