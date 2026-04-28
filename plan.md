# Issue 40 + 28: Rust-based `@ui/*` component system + Card

## Context

Implements issue #40 (`@ui/*` module loader infrastructure) using issue #28 (Card) as the
first concrete component.

**Approach**: build a working prototype end-to-end — Card fully implemented — to discover
real implementation problems and let those drive the trait design. No phase split.
The prototype covers: `costae::ui::Node`, `IntoJsValue`/`FromJsValue`, `Callback<P, O>`,
`UiRegistry`, `register_ui_components`, `@ui/*` loader wiring, and the Card component itself.
The `ui!` proc-macro (rstml syntax) is out of scope for the prototype but the function signatures
and node construction must be written so it could eventually generate them.

---

## Implementation Steps

### 1. Define `costae::ui::Node`

New file `src/ui/node.rs`, exposed as `pub mod ui` in `lib.rs`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Node {
    Container(ContainerNode),
    Text(TextNode),
    Image(ImageNode),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tw: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tw: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tw: Option<String>,
    pub src: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}
```

**Why a separate type and not `takumi::Node`**: takumi::Node is Deserialize-only (no Serialize
impl, and ImageSourceInput has a skip_deserializing variant). More importantly, components should
not be coupled to takumi — if the renderer is swapped out, all built-in components would need
rewriting. This type is the component author's API; takumi is the renderer's API. They stay
connected only via the JSON wire format.

**Serde tag compatibility**: `tag = "type", rename_all = "camelCase"` produces `"container"`,
`"text"`, `"image"` — exactly what takumi's NodeKind expects. The compatibility contract is
enforced by a round-trip test: `parse_layout(serde_json::to_value(node)?)` must succeed.

**`tw` is `Option<String>`**: takumi's `TailwindValues` deserializes from a plain string (its
`Deserialize` impl calls `from_str`). So serializing as a string and letting takumi deserialize
is correct. Theme token resolution (`resolve_tw_in_json`) still runs on the JSON before it
reaches `parse_layout` — same as today.

### 2. Implement `Card` component function

```rust
// src/ui/components/card.rs
pub fn card<'js>(ctx: rquickjs::Ctx<'js>, props: rquickjs::Value<'js>)
    -> rquickjs::Result<rquickjs::Value<'js>>
{
    let children: Vec<Node> = props
        .get::<_, rquickjs::Value>("children")
        .ok()
        .and_then(|v| rquickjs_serde::from_value(v).ok())
        .unwrap_or_default();

    let node = Node::Container(ContainerNode {
        tw: Some("rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]".into()),
        children,
    });

    let json = serde_json::to_value(&node).map_err(|_| rquickjs::Error::Unknown)?;
    rquickjs_serde::to_value(ctx, &json).map_err(|_| rquickjs::Error::Unknown)
}
```

**tw string rationale**: matches what existing chezmoi components consistently use. Deviations
from shadcn Card are intentional — see Shadcn Notes below.

### 3. Trait-based component output

The registry and component system must not be hardcoded to `costae::ui::Node`. The same
infrastructure should be reusable for non-UI reconciler systems in the future (different node
shapes, different output domains).

The boundary trait is minimal — just "can be returned to JS":

```rust
pub trait IntoJsValue {
    fn into_js_value<'js>(self, ctx: rquickjs::Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>>;
}
```

`costae::ui::Node` implements this via the serde round-trip. Any future node type does too, as
long as it implements `Serialize`. A blanket impl covers the common case:

```rust
impl<T: serde::Serialize> IntoJsValue for T {
    fn into_js_value<'js>(self, ctx: rquickjs::Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>> {
        let json = serde_json::to_value(&self).map_err(|_| rquickjs::Error::Unknown)?;
        rquickjs_serde::to_value(ctx, &json).map_err(|_| rquickjs::Error::Unknown)
    }
}
```

Component functions are registered generically — the Rust component returns any `O: IntoJsValue`,
and the registration helper wraps it into the `fn(Ctx, Value) -> Result<Value>` shape that
rquickjs expects:

```rust
pub fn make_component_fn<P, O, F>(f: F)
    -> impl Fn(rquickjs::Ctx, rquickjs::Value) -> rquickjs::Result<rquickjs::Value>
where
    P: for<'de> serde::Deserialize<'de>,
    O: IntoJsValue,
    F: Fn(P) -> O,
{ ... }
```

This way:
- Rust component functions are written against typed props and typed output, not raw rquickjs values
- The JS boundary plumbing (deserialize props, serialize output) lives in one place
- The same pattern works for any future output domain

### 4. Add `UiRegistry` and `register_ui_components`

```rust
// src/ui/registry.rs
pub struct UiEntry {
    pub module_path: &'static str,   // e.g. "@ui/card"
    pub export_name: &'static str,   // e.g. "Card"
    pub global_name: &'static str,   // e.g. "__ui_card"
}

pub const UI_COMPONENTS: &[UiEntry] = &[
    UiEntry { module_path: "@ui/card", export_name: "Card", global_name: "__ui_card" },
];
```

`register_ui_components(ctx: &Ctx)` iterates `UI_COMPONENTS`, registers each Rust function as
a JS global by `global_name`. Called from `JsxEvaluator::new`.

The synthetic JS module source for each entry is generated from the registry:
```js
const Card = __ui_card;
export { Card };
```

This way adding a new component is one registry entry + one Rust function. No other files change.

### 4. Wire `@ui/*` into the loader/resolver

rquickjs supports tuple-chained resolvers and loaders. Use `BuiltinResolver` and `BuiltinLoader`
from rquickjs (already available, no custom impl needed) for the `@ui/*` names. Chain them with
the existing `CostaeResolver`/`CostaeLoader` for file-based imports.

**Key change**: the loader is currently only set when `base_dir` is `Some`. It must always be
set so `@ui/*` imports work even without a base_dir (which is the case in all unit tests):

```rust
// Always build the builtin pair from the registry
let builtin_resolver = UI_COMPONENTS.iter().fold(
    BuiltinResolver::default(),
    |r, e| r.with_module(e.module_path)
);
let builtin_loader = /* BuiltinLoader with synthetic source per entry */;

if let Some(dir) = base_dir {
    runtime.set_loader(
        (builtin_resolver, CostaeResolver::new(dir)),
        (builtin_loader, CostaeLoader::new(Arc::clone(&loaded_paths))),
    );
} else {
    runtime.set_loader(builtin_resolver, builtin_loader);
}
```

**Ordering matters**: builtins must be first in the tuple. If `CostaeResolver` runs first it
rejects `@ui/*` (it only allows `./` and `../` prefixes) before `BuiltinResolver` gets a chance.

**`BuiltinLoader.load()` calls `remove()`**: QuickJS caches compiled modules after first load, so
the loader is never called twice for the same module name in the same runtime. Since each
`JsxEvaluator` creates a fresh runtime, this is safe. The `BuiltinLoader` is consumed into the
runtime and a new one is created for each evaluator instance.

---

## Acceptance Criteria & Tests

### Testable in unit tests (within `jsx.rs` using the existing `eval()` helper)

| Criterion | Test approach |
|-----------|--------------|
| `costae::ui::Node` serializes to the JSON shape `parse_layout` accepts | `parse_layout(&serde_json::to_value(Node::Container(...))?)` succeeds |
| `import { Card } from '@ui/card'` resolves without error | `eval(r#"import { Card } from '@ui/card'; export default function render() { return <Card />; }"#)` does not panic |
| Card returns `type: "container"` | `result["type"] == "container"` |
| Card tw contains required tokens | `tw.contains("bg-card")`, `"border-border"`, `"rounded-lg"`, `"text-card-foreground"` |
| Children pass through | `<Card><text tw="text-white">hi</text></Card>` → `result["children"][0]["type"] == "text"` |
| `@ui/*` works without a base_dir | all above tests use `eval()` which passes `None` for base_dir |
| Unknown `@ui/` module fails gracefully | `JsxEvaluator::new` with `import {} from '@ui/nonexistent'` returns `Err`, not a panic |

### Not unit-testable, verify manually

- Rendered pixels look correct with a real theme
- `text-card-foreground` token resolves in your active theme (check theme YAML for `card-foreground` key)

---

## Shadcn Card Notes

Shadcn Card default classes: `"bg-card text-card-foreground flex flex-col gap-6 rounded-xl border py-6 shadow-sm"`

| Shadcn choice | Our choice | Reason |
|---------------|-----------|--------|
| `rounded-xl` | `rounded-lg` | Matches existing chezmoi components |
| `border` (plain) | `border border-border` | Our theme system requires explicit color token; plain `border` doesn't resolve |
| `flex flex-col gap-6` baked in | No layout classes | Card is a visual wrapper; layout belongs at the call site |
| `py-6` | `px-3 py-[10px]` | Matches chezmoi component conventions |
| `shadow-sm` | omit | Existing chezmoi components don't use it |
| CardHeader / CardContent / CardFooter sub-components | Not implemented | No need yet; add when a real use case requires them |

---

## Component Authoring Notes (seed for future AI skill)

These notes describe how to add a new built-in Rust component to the `@ui/*` system.

### Step-by-step

1. **Check shadcn** for the component. Read the default className string. Scan for: baked-in
   layout classes (flex, gap, grid — usually omit), shadow defaults, border-radius values, and
   sub-component structure. Do not copy complexity that has no parallel in our system.

2. **Map to our token system**. shadcn uses CSS variables resolved at runtime; we use explicit
   token names (`bg-card`, `border-border`, `text-foreground`, etc.) that our theme resolver
   substitutes. Replace shadcn's plain `border` with `border border-border`. Replace `rounded-xl`
   with the actual value from your theme's radius scale (`rounded-lg` = `lg` key in theme YAML).

3. **Check the theme YAML** for any tokens you plan to use. If a token isn't defined, it passes
   through to takumi as a literal class name and silently does nothing. Tokens are defined under
   `colors.light`, `colors.dark`, and `radius` in the theme config.

4. **Write the Rust function** in `src/ui/components/<name>.rs`:
   - Signature: `pub fn component_name<'js>(ctx: Ctx<'js>, props: Value<'js>) -> Result<Value<'js>>`
   - Extract children: `rquickjs_serde::from_value::<Vec<Node>>(props.get("children")?).unwrap_or_default()`
   - Extract other props: `rquickjs_serde::from_value::<PropType>(props.get("field_name")?)`
   - Build: construct `costae::ui::Node` using the `ContainerNode`/`TextNode`/`ImageNode` structs
   - Return: `serde_json::to_value(&node)` → `rquickjs_serde::to_value(ctx, &json)`

5. **Register in `UI_COMPONENTS`** (one line in `src/ui/registry.rs`):
   ```rust
   UiEntry { module_path: "@ui/mycomp", export_name: "MyComp", global_name: "__ui_mycomp" },
   ```

6. **No other files need changing.** The registry drives the resolver, loader, and global
   registration automatically.

7. **Write tests** asserting:
   - The output `type` field is correct
   - The `tw` string contains every token you intended
   - Children are present in the output when passed in

### Design constraints

- `tw` is always a plain `String` — never pre-resolve tokens in Rust component code. Resolution
  happens in the `resolve_tw_in_json` pass after the component returns.
- Components must not access the rquickjs runtime directly beyond extracting props and returning
  a value. Side effects (streams, modules) belong in the JS layer.
- **Return `costae::ui::Node`, not `takumi::Node`**. Never construct `takumi::Node` in a
  component — it cannot be serialized and couples the component to the renderer.
- **Return any `IntoJsValue`, not just `costae::ui::Node`**. The system is trait-based; a
  component's output type only needs to implement `IntoJsValue` (blanket-implemented for anything
  `Serialize`). This keeps the component infrastructure reusable for non-UI domains.
- Props deserialization can fail silently (use `unwrap_or_default`) for optional fields; children
  in particular should default to an empty vec rather than erroring.
- No layout classes on pure visual container components. The component provides visual identity
  (background, border, radius, text color); the caller provides layout (flex, grid, gap, width).

### Phase 1 vs Phase 2 authoring syntax

Phase 1 constructs nodes manually:
```rust
fn card(props: CardProps) -> Node {
    Node::Container(ContainerNode {
        tw: Some("rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]".into()),
        children: props.children,
    })
}
```

Phase 2 replaces this with `ui!` (rstml-based proc-macro) — same output, ergonomic syntax:
```rust
fn card(props: CardProps) -> Node {
    ui! {
        <container tw="rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]">
            {props.children}
        </container>
    }
}
```

The macro rules:
- Lowercase tag (`<container>`, `<text>`, `<image>`) → `Node::Container(...)` / `Node::Text(...)` / `Node::Image(...)`
- PascalCase tag (`<Button />`) → direct Rust call to `button(ButtonProps { ... })` (snake_case of tag name)

**Naming convention locked in Phase 1**: component functions must be named as the snake_case of
their PascalCase export name so the Phase 2 macro can derive the call correctly. `Card` export →
`card` function, `Button` export → `button` function. Do not use other naming patterns.

### Callable props

Rust components can call each other directly (no JS involved):
```rust
fn card(props: CardProps) -> Node {
    // Phase 2: ui! { <container tw="..."><Button label="ok" />{props.children}</container> }
    Node::Container(ContainerNode {
        tw: Some("...".into()),
        children: vec![button(ButtonProps { label: "ok".into() }), props.children],
    })
}
```

For callable props (a component or function passed as a prop value), use `Callback<P, O>` — an
enum that holds either a Rust closure or a JS function, with a uniform `.call(ctx, props)`
interface. The receiving component never knows which branch it is:

```rust
pub enum Callback<P, O> {
    Rust(Box<dyn Fn(P) -> rquickjs::Result<O>>),
    Js { func: Persistent<Function<'static>>, _phantom: PhantomData<fn(P) -> O> },
}
impl<P: IntoJsValue, O: FromJsValue> Callback<P, O> {
    pub fn call<'js>(&self, ctx: Ctx<'js>, props: P) -> rquickjs::Result<O> { ... }
}
// Rust side: From<fn(P) -> O>
// JS side:   FromJsValue (receives rquickjs::Function from props)
```

- Rust-to-Rust: `Callback::Rust` branch — no serialization, no JS involved
- From JS props: both pure JS functions and registered Rust globals arrive as `rquickjs::Function`
  and are stored in `Callback::Js` — opaque to the receiving component
- Components with callable props use `make_component_fn_with_ctx` (receives `Ctx`) instead of
  `make_component_fn`
- Deferred: `IntoJsValue for Callback<P, O>` (passing a Rust-created callback to JS) — requires
  registering the closure as a new QuickJS function; not needed for Phase 1
