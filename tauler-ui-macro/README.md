# tauler-ui-macro

The `#[component]` proc macro behind [tauler](https://crates.io/crates/tauler)'s built-in
UI components.

It turns a plain Rust function into a component that a layout file can import and use
as a tag. The function's arguments become the component's props, deserialized from the
JSX call site, so the prop list stays a single declaration.

This crate exists to support tauler's own component library. It is published because
tauler depends on it, and it is unlikely to be useful on its own.

## Use

```rust
use crate::ui::{component, rsx, Node};

#[component("@ui/badge")]
pub fn badge(children: Vec<Node>, variant: Option<String>, class: Option<String>) -> Node {
    rsx! { <div class={class}>{children}</div> }
}
```

```jsx
import { Badge } from '@ui/badge';

<Badge variant="secondary"><span>Secondary</span></Badge>
```

- The string argument is the module path the layout imports from. Omit it for a helper
  that only other Rust components call (`fill` in `progress.rs`).
- The function name becomes the PascalCase tag (`badge` -> `Badge`).
- `Option<T>` and `Vec<T>` props default when absent. If the props fail to deserialize,
  the whole props struct falls back to `Default`.
- The return type is `Node`, `Vec<Node>` (sibling nodes), or any serializable type.
- The macro generates both engine bindings: QuickJS (feature `quickjs`) and wasm-bindgen
  (`wasm32`).

The macro emits paths like `crate::ui::Node`, so it only works inside `tauler-core`.
For real usage see `tauler-core/src/ui/`, for example `components/badge.rs`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
