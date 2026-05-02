pub use costae_ui_macro::component;
pub use costae_ui_macro::rsx;
use serde::{Deserialize, Serialize};

pub mod components;
pub mod cva;
pub mod registry;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<serde_json::Map<String, serde_json::Value>>,
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

pub trait UiComponent {
    type Props: for<'de> serde::Deserialize<'de> + Default;

    fn render(props: Self::Props) -> Node;

    fn render_from_value(v: serde_json::Value) -> Node {
        Self::render(serde_json::from_value(v).unwrap_or_default())
    }

    fn js_fn<'js>(
        ctx: rquickjs::Ctx<'js>,
        props: rquickjs::Value<'js>,
    ) -> rquickjs::Result<rquickjs::Value<'js>> {
        let p = rquickjs_serde::from_value(props).map_err(|_| rquickjs::Error::Unknown)?;
        rquickjs_serde::to_value(ctx, Self::render(p)).map_err(|_| rquickjs::Error::Unknown)
    }
}

pub trait IntoJsValue {
    fn into_js_value<'js>(self, ctx: rquickjs::Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>>;
}

impl<T: Serialize> IntoJsValue for T {
    fn into_js_value<'js>(self, ctx: rquickjs::Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>> {
        rquickjs_serde::to_value(ctx, &self).map_err(|_| rquickjs::Error::Unknown)
    }
}

pub trait FromJsValue: Sized {
    fn from_js_value<'js>(value: rquickjs::Value<'js>) -> rquickjs::Result<Self>;
}

impl<T: for<'de> Deserialize<'de>> FromJsValue for T {
    fn from_js_value<'js>(value: rquickjs::Value<'js>) -> rquickjs::Result<Self> {
        rquickjs_serde::from_value(value).map_err(|_| rquickjs::Error::Unknown)
    }
}
