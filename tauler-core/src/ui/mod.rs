use serde::{Deserialize, Serialize};
pub use tauler_ui_macro::component;
pub use tauler_ui_macro::rsx;

pub mod components;
pub mod cva;
pub mod registry;

/// A node as a Rust-backed component builds it.
///
/// Two shapes, because the layout vocabulary has two: an element, named by its HTML
/// tag, and a run of text. Text carries no styling of its own — style the element
/// around it, as in HTML (see `docs/adr/0016`).
///
/// Untagged, so the JSON is the layout file's own shape: an element serializes to
/// `{"type": "div", …}` and text serializes to a bare string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Node {
    Element(ElementNode),
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElementNode {
    /// The HTML tag. Serialized as `type`, which is what the layout tree calls it.
    #[serde(rename = "type")]
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<serde_json::Map<String, serde_json::Value>>,
    /// An array of intents, kept as raw JSON.
    ///
    /// Not typed as `Vec<Intent>`: components that take `children: Vec<Node>` round-trip
    /// their children through this struct, and serde drops what it cannot name — so a
    /// hand-written handler has to survive the trip malformed and reach the runtime,
    /// which is the only place that can warn about it (see `hit_test`).
    ///
    /// Only block-level elements can be hit — see `docs/adr/0018`.
    ///
    /// Boxed to keep an element the same size as before it could carry one: every
    /// node in the tree pays for this field on every tick, and almost none use it.
    /// `rsx!` boxes for you — write `on_click={some_option}`. Either shape is legal:
    /// an array of intents, or `{"$handler": n}` for a function (`docs/adr/0021`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_click: Option<Box<serde_json::Value>>,
    /// Fires on a press and on every motion until release, and captures the pointer
    /// while it does — see `docs/adr/0020`. Same two shapes as `on_click`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_drag: Option<Box<serde_json::Value>>,
    /// `<img>` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// Merge two Tailwind class strings by appending `extra` after `base`.
///
/// Correct because takumi applies declarations in order via plain field assignment
/// (`style.$longhand = value`), so later classes win. Appending `extra` last means
/// the caller's overrides take precedence over `base`.
pub fn tw_merge(base: &str, extra: &str) -> String {
    if extra.is_empty() {
        base.to_string()
    } else {
        format!("{base} {extra}")
    }
}

pub trait IntoNodes {
    fn into_nodes(self) -> Vec<Node>;
}

impl IntoNodes for Node {
    fn into_nodes(self) -> Vec<Node> {
        vec![self]
    }
}

impl IntoNodes for Vec<Node> {
    fn into_nodes(self) -> Vec<Node> {
        self
    }
}

/// So `<span>{some_string}</span>` works: interpolating a value into an element makes
/// a text node, exactly as writing the characters there would.
impl IntoNodes for String {
    fn into_nodes(self) -> Vec<Node> {
        vec![Node::Text(self)]
    }
}

impl IntoNodes for &str {
    fn into_nodes(self) -> Vec<Node> {
        vec![Node::Text(self.to_string())]
    }
}

pub trait UiComponent {
    type Props: for<'de> serde::Deserialize<'de> + Default;

    /// Usually [`Node`], but a component may return `Vec<Node>` to emit several
    /// siblings — the equivalent of a JSX fragment, which JS components can
    /// already do (see `jsx_fragment_shorthand_flattens_into_parent_children`).
    /// `rquickjs_serde` turns a `Vec` into a JS array, and the runtime splices
    /// arrays into the parent's children, so both arrive correctly shaped.
    type Output: serde::Serialize;

    fn render(props: Self::Props) -> Self::Output;

    fn render_from_value(v: serde_json::Value) -> Self::Output {
        Self::render(serde_json::from_value(v).unwrap_or_default())
    }

    #[cfg(feature = "quickjs")]
    fn js_fn<'js>(
        ctx: rquickjs::Ctx<'js>,
        props: rquickjs::Value<'js>,
    ) -> rquickjs::Result<rquickjs::Value<'js>> {
        let p = rquickjs_serde::from_value(props).map_err(|_| rquickjs::Error::Unknown)?;
        rquickjs_serde::to_value(ctx, Self::render(p)).map_err(|_| rquickjs::Error::Unknown)
    }
}

/// The wasm-bindgen counterpart of [`UiComponent::js_fn`], called by the binding the
/// `#[component]` macro generates.
///
/// Free rather than a trait method so the trait stays free of `wasm_bindgen` types on
/// every target — only this function and the generated bindings mention them.
#[cfg(target_arch = "wasm32")]
pub fn wasm_render<C: UiComponent>(
    props: wasm_bindgen::JsValue,
) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    let props: serde_json::Value = serde_wasm_bindgen::from_value(props)?;
    let out = C::render_from_value(props);
    let out =
        serde_json::to_value(out).map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))?;
    // Plain objects, not `Map`s — a component's output is read by the JavaScript shims,
    // and a `Map` answers every property access with `undefined`. See `web::to_js`.
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    Ok(serde::Serialize::serialize(&out, &serializer)?)
}

#[cfg(feature = "quickjs")]
pub trait IntoJsValue {
    fn into_js_value<'js>(self, ctx: rquickjs::Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>>;
}

#[cfg(feature = "quickjs")]
impl<T: Serialize> IntoJsValue for T {
    fn into_js_value<'js>(self, ctx: rquickjs::Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>> {
        rquickjs_serde::to_value(ctx, &self).map_err(|_| rquickjs::Error::Unknown)
    }
}

#[cfg(feature = "quickjs")]
pub trait FromJsValue: Sized {
    fn from_js_value<'js>(value: rquickjs::Value<'js>) -> rquickjs::Result<Self>;
}

#[cfg(feature = "quickjs")]
impl<T: for<'de> Deserialize<'de>> FromJsValue for T {
    fn from_js_value<'js>(value: rquickjs::Value<'js>) -> rquickjs::Result<Self> {
        rquickjs_serde::from_value(value).map_err(|_| rquickjs::Error::Unknown)
    }
}
