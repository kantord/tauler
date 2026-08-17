/// Two minimal test components that share the same module path `@ui/test-multi`.
/// Used by `tests/ui_multi_module_test.rs` to verify that both exports are available
/// from a single JSX import of that module path.
use crate::ui::{component, rsx};

#[component("@ui/test-multi")]
pub fn foo_widget(_unused: Option<String>) -> Node {
    rsx! { <div class="foo-widget" /> }
}

#[component("@ui/test-multi")]
pub fn bar_widget(_unused: Option<String>) -> Node {
    rsx! { <div class="bar-widget" /> }
}

/// Returns siblings rather than a single node — a Rust-side JSX fragment.
/// Exercised by `tests/ui_multi_node_test.rs`.
#[component("@ui/test-multi")]
pub fn pair_widget(_unused: Option<String>) -> Vec<Node> {
    vec![
        rsx! { <div class="pair-first" /> },
        rsx! { <div class="pair-second" /> },
    ]
}
