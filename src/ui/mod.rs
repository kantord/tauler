use serde::{Deserialize, Serialize};

pub mod components;
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
