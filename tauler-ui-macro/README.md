# tauler-ui-macro

The `#[component]` proc macro behind [tauler](https://crates.io/crates/tauler)'s built-in
UI components.

It turns a plain Rust function into a component that a `.jsx` layout can import and use
as a tag. The function's arguments become the component's props, deserialized from the
JSX call site, so the prop list stays a single declaration rather than a struct plus a
registration plus a deserializer.

This crate exists to support tauler's own component library. It is published because
tauler depends on it, and it is unlikely to be useful on its own.

## Use

```rust
use crate::ui::{component, rsx, Node};

/// A small inline label for status, category, or count.
#[component("@ui/badge")]
pub fn badge(children: Vec<Node>, variant: Option<String>, tw: Option<String>) -> Node {
    rsx! { <container tw={resolved}>{children}</container> }
}
```

The string argument is the module path the component is imported from in a layout:

```jsx
import { Badge } from '@ui/badge';

<Badge variant="secondary"><text>Secondary</text></Badge>
```

The function name is converted to PascalCase for the JSX tag (`badge` → `Badge`), and
`Option<T>` and `Vec<T>` props default rather than being required.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
