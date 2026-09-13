//! Parses and validates a two-document schema file: the first document declares a flat
//! list of JSX node components — what props each accepts, how each is validated, and
//! whether it has children — the second is a minijinja template string (design record
//! §14, "a multi-part YAML file would take care of making this a single file"). Nothing
//! downstream of [`parse`] should ever see the raw YAML again — every `component` name has
//! already gone through [`Identifier::validate`], and the first document has already been
//! checked against the generic node-schema meta-schema.
//!
//! Deliberately config-format-independent (design record §17): there is no "selector"
//! concept, no rasi-specific value type, nothing here assumes the target format is
//! `.rasi`. A node is just a component with declared props, optional constant fields, and
//! an optional `children_as` name if it wraps other nodes — the *only* place any format's
//! own syntax is known is the schema author's own `pattern`/`error` strings and the
//! template text itself, both of which are data, not Rust code.

use crate::identifier::{Identifier, InvalidIdentifier};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

const META_SCHEMA: &str = include_str!("../schemas/config-schema.metaschema.json");

#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("schema is not valid YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("expected two YAML documents (schema, then template) but the {0} document is missing")]
    MissingDocument(&'static str),
    #[error("schema does not match the node-schema meta-schema: {0}")]
    SchemaShape(String),
    #[error(transparent)]
    Identifier(#[from] InvalidIdentifier),
    #[error(
        "component name \"{0}\" is used for more than one node in this schema — every node needs a distinct name"
    )]
    DuplicateComponent(String),
}

/// One prop's validation rule. `pattern`/`enum_values` are the same general mechanism
/// every node uses — there is no closed set of "named types": a format-specific shorthand
/// (rasi's Padding/Border, say) is just a `pattern` a schema author writes once, with
/// `error` naming the shape in a sentence a schema author wrote, so the thrown error is
/// never the regex source (design record §4.6's caveat about JSON Schema's worst part).
///
/// `items`, when set, means the prop's value must be a JS array, every element of which
/// is checked against the *nested* rule — rofi's list-of-keywords type (`man rofi-theme`,
/// "List of keywords": `children: [ prompt, entry ];`) is `items: { pattern: ... }` with
/// no `pattern`/`enum` of its own, not a new mechanism.
#[derive(Debug, Clone, Deserialize)]
pub struct PropRule {
    pub pattern: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub error: Option<String>,
    #[serde(default)]
    pub items: Option<Box<PropRule>>,
}

#[derive(Debug, Deserialize)]
struct RawNode {
    component: String,
    #[serde(default)]
    root: bool,
    #[serde(default)]
    container: bool,
    children_as: Option<String>,
    #[serde(default)]
    props: BTreeMap<String, PropRule>,
    #[serde(default)]
    constants: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawSchema {
    nodes: Vec<RawNode>,
}

#[derive(Debug, Deserialize)]
struct RawTemplateDoc {
    template: String,
}

/// One declared JSX component: a leaf if `children_as` is `None`, a container wrapping its
/// children under that field name otherwise. Exactly one node in a [`Schema`] has
/// `is_root: true` — its generated function additionally embeds the template and calls
/// `renderTemplate`, instead of returning its data plainly.
#[derive(Debug, Clone)]
pub struct NodeSchema {
    pub component: Identifier,
    pub is_root: bool,
    pub children_as: Option<String>,
    pub props: BTreeMap<String, PropRule>,
    pub constants: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub nodes: Vec<NodeSchema>,
    pub template: String,
}

impl Schema {
    pub fn root(&self) -> &NodeSchema {
        self.nodes
            .iter()
            .find(|n| n.is_root)
            .expect("the meta-schema requires exactly one root node")
    }
}

/// Parses the two-document schema file, validates the first document's shape against the
/// generic node-schema meta-schema (exactly one root, every container names its
/// `children_as` field — both enforced structurally, not just by convention), validates
/// every `component` name as a safe JS identifier, and rejects two nodes sharing one name
/// — the bug the design record's §14.6 found by hand-tracing a schema that reused
/// `component: Selector` for two different selectors.
pub fn parse(source: &str) -> Result<Schema, GenError> {
    let mut docs = serde_yaml_ng::Deserializer::from_str(source);

    let schema_doc = docs.next().ok_or(GenError::MissingDocument("schema"))?;
    let schema_value: serde_json::Value = serde::Deserialize::deserialize(schema_doc)?;
    validate_shape(&schema_value)?;
    let raw: RawSchema = serde_json::from_value(schema_value)
        .expect("already validated against the meta-schema, so this must deserialize");

    let template_doc = docs.next().ok_or(GenError::MissingDocument("template"))?;
    let template_value: serde_json::Value = serde::Deserialize::deserialize(template_doc)?;
    let raw_template: RawTemplateDoc = serde_json::from_value(template_value)
        .map_err(|e| GenError::SchemaShape(format!("template document: {e}")))?;

    let mut seen = HashSet::new();
    let nodes = raw
        .nodes
        .into_iter()
        .map(|n| {
            Ok(NodeSchema {
                component: checked_component(&n.component, &mut seen)?,
                is_root: n.root,
                children_as: if n.container { n.children_as } else { None },
                props: n.props,
                constants: n.constants,
            })
        })
        .collect::<Result<Vec<_>, GenError>>()?;

    Ok(Schema {
        nodes,
        template: raw_template.template,
    })
}

fn checked_component(name: &str, seen: &mut HashSet<String>) -> Result<Identifier, GenError> {
    let id = Identifier::validate(name)?;
    if !seen.insert(id.as_str().to_string()) {
        return Err(GenError::DuplicateComponent(id.as_str().to_string()));
    }
    Ok(id)
}

fn validate_shape(schema: &serde_json::Value) -> Result<(), GenError> {
    let meta: serde_json::Value = serde_json::from_str(META_SCHEMA)
        .expect("meta-schema is checked-in and must be valid JSON");
    let validator = jsonschema::validator_for(&meta)
        .expect("meta-schema is checked-in and must be valid JSON Schema");
    validator
        .validate(schema)
        .map_err(|err| GenError::SchemaShape(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROFI_SCHEMA: &str = r#"
nodes:
  - component: Rofi
    root: true
    container: true
    children_as: selectors
  - component: Selector
    container: true
    children_as: properties
  - component: BackgroundColor
    props:
      value:
        pattern: "^#[0-9a-fA-F]{6}$"
    constants:
      css_name: background-color
  - component: TextColor
    props:
      value:
        pattern: "^#[0-9a-fA-F]{6}$"
    constants:
      css_name: text-color
  - component: WindowWidth
    props:
      value:
        pattern: "^[0-9]+(px|%)$"
    constants:
      css_name: width
  - component: Location
    props:
      value:
        enum: ["center", "north", "south", "east", "west"]
    constants:
      css_name: location
---
template: |
  {% for selector in selectors %}
  {{ selector.name }} {
  {% for prop in selector.properties %}
      {{ prop.css_name }}: {{ prop.value }};
  {% endfor %}
  }
  {% endfor %}
"#;

    #[test]
    fn parses_the_rofi_schema_end_to_end() {
        let schema = parse(ROFI_SCHEMA).unwrap();
        assert_eq!(schema.root().component.as_str(), "Rofi");
        assert_eq!(schema.nodes.len(), 6);
        let selector = schema
            .nodes
            .iter()
            .find(|n| n.component.as_str() == "Selector")
            .unwrap();
        assert_eq!(selector.children_as.as_deref(), Some("properties"));
        assert!(schema.template.contains("{{ selector.name }}"));
    }

    #[test]
    fn rejects_a_schema_missing_the_template_document() {
        let one_doc_only = ROFI_SCHEMA.split("---").next().unwrap();
        let err = parse(one_doc_only).unwrap_err();
        assert!(matches!(err, GenError::MissingDocument("template")));
    }

    #[test]
    fn rejects_a_schema_with_no_root_node() {
        let broken = r#"
nodes:
  - component: BackgroundColor
    props:
      value:
        pattern: "^#[0-9a-fA-F]{6}$"
---
template: "irrelevant"
"#;
        let err = parse(broken).unwrap_err();
        assert!(matches!(err, GenError::SchemaShape(_)), "got {err:?}");
    }

    #[test]
    fn rejects_a_container_with_no_children_as() {
        let broken = r#"
nodes:
  - component: Rofi
    root: true
    container: true
    children_as: selectors
  - component: Selector
    container: true
---
template: "irrelevant"
"#;
        let err = parse(broken).unwrap_err();
        assert!(matches!(err, GenError::SchemaShape(_)), "got {err:?}");
    }

    /// The exact bug the design record's §14.6 caught by hand-tracing generated output:
    /// two nodes reusing one `component` name silently produces one generated function
    /// that the second definition would shadow. It must be a rejected schema, not a
    /// silently-broken generated file.
    #[test]
    fn rejects_two_nodes_sharing_one_component_name() {
        let broken = r#"
nodes:
  - component: Rofi
    root: true
    container: true
    children_as: selectors
  - component: Rofi
    container: true
    children_as: properties
---
template: "irrelevant"
"#;
        let err = parse(broken).unwrap_err();
        assert!(matches!(err, GenError::DuplicateComponent(name) if name == "Rofi"));
    }

    #[test]
    fn rejects_an_unsafe_component_name_before_it_ever_reaches_codegen() {
        let malicious = r#"
nodes:
  - component: Rofi
    root: true
    container: true
    children_as: selectors
  - component: "}); evil(); (({ "
    props:
      value:
        pattern: "^#[0-9a-fA-F]{6}$"
---
template: "irrelevant"
"#;
        let err = parse(malicious).unwrap_err();
        // The meta-schema's own `pattern` on `component` fields happens to use the same
        // regex as `Identifier::validate`, so it catches this payload first — a
        // `SchemaShape` error, not `GenError::Identifier`. That's defense in depth working
        // as intended (§14.4), not a bug: the claim this test makes is "never reaches
        // codegen," not "caught by this specific layer." `Identifier::validate`'s own
        // rejection of the identical payload is proven independently, in isolation, by
        // `identifier::tests::rejects_the_identifier_breakout_payload` — the layer that
        // matters if the meta-schema's pattern is ever loosened.
        assert!(
            matches!(err, GenError::SchemaShape(_) | GenError::Identifier(_)),
            "got {err:?}"
        );
    }

    /// A grammar-specific shorthand type (rasi's Border: 1-4 groups of a distance
    /// optionally followed by a line style) is expressed with a plain `pattern` + a
    /// human-written `error` — no Rust enum, no `tauler-configgen` code change, proving
    /// the generator really is format-independent (design record §17).
    #[test]
    fn a_custom_error_message_travels_through_as_plain_data() {
        let schema_with_border = r#"
nodes:
  - component: Rofi
    root: true
    container: true
    children_as: selectors
  - component: Border
    props:
      value:
        pattern: "^(?:[+-]?\\d+(?:px)?)(?:\\s+(?:dash|solid))?$"
        error: "must be a distance optionally followed by \"dash\" or \"solid\""
    constants:
      css_name: border
---
template: "irrelevant"
"#;
        let schema = parse(schema_with_border).unwrap();
        let border = schema
            .nodes
            .iter()
            .find(|n| n.component.as_str() == "Border")
            .unwrap();
        let rule = &border.props["value"];
        assert_eq!(
            rule.error.as_deref(),
            Some("must be a distance optionally followed by \"dash\" or \"solid\"")
        );
    }
}
