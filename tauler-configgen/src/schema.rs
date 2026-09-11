//! Parses and validates a two-document schema file: the first document declares structure
//! (selectors, properties, which JS component names them), the second is a minijinja
//! template string (design record §14, "a multi-part YAML file would take care of making
//! this a single file"). Nothing downstream of [`parse`] should ever see the raw YAML
//! again — every `component` name has already gone through [`Identifier::validate`], and
//! the first document has already been checked against the rasi grammar's meta-schema.

use crate::identifier::{Identifier, InvalidIdentifier};
use serde::Deserialize;
use std::collections::HashSet;

const META_SCHEMA: &str = include_str!("../schemas/rasi-grammar.metaschema.json");

#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("schema is not valid YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("expected two YAML documents (schema, then template) but the {0} document is missing")]
    MissingDocument(&'static str),
    #[error("schema does not match the rasi grammar's meta-schema: {0}")]
    SchemaShape(String),
    #[error(transparent)]
    Identifier(#[from] InvalidIdentifier),
    #[error(
        "component name \"{0}\" is used for more than one node in this schema — every node needs a distinct name"
    )]
    DuplicateComponent(String),
}

#[derive(Debug, Deserialize)]
struct RawProperty {
    name: String,
    component: String,
    pattern: Option<String>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawSelector {
    name: String,
    component: String,
    properties: Vec<RawProperty>,
}

#[derive(Debug, Deserialize)]
struct RawSchema {
    component: String,
    selectors: Vec<RawSelector>,
}

#[derive(Debug, Deserialize)]
struct RawTemplateDoc {
    template: String,
}

#[derive(Debug, Clone)]
pub struct PropertySchema {
    pub css_name: String,
    pub component: Identifier,
    pub pattern: Option<String>,
    pub enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct SelectorSchema {
    pub name: String,
    pub component: Identifier,
    pub properties: Vec<PropertySchema>,
}

#[derive(Debug, Clone)]
pub struct RasiSchema {
    pub root_component: Identifier,
    pub selectors: Vec<SelectorSchema>,
    pub template: String,
}

/// Parses the two-document schema file, validates the first document's shape against the
/// rasi grammar's meta-schema, validates every `component` name as a safe JS identifier,
/// and rejects two nodes sharing one name — the bug the design record's §14.6 found by
/// hand-tracing a schema that reused `component: Selector` for two different selectors.
pub fn parse(source: &str) -> Result<RasiSchema, GenError> {
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
    let root_component = checked_component(&raw.component, &mut seen)?;

    let selectors = raw
        .selectors
        .into_iter()
        .map(|s| {
            let component = checked_component(&s.component, &mut seen)?;
            let properties = s
                .properties
                .into_iter()
                .map(|p| {
                    Ok(PropertySchema {
                        css_name: p.name,
                        component: checked_component(&p.component, &mut seen)?,
                        pattern: p.pattern,
                        enum_values: p.enum_values,
                    })
                })
                .collect::<Result<Vec<_>, GenError>>()?;
            Ok(SelectorSchema {
                name: s.name,
                component,
                properties,
            })
        })
        .collect::<Result<Vec<_>, GenError>>()?;

    Ok(RasiSchema {
        root_component,
        selectors,
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
grammar: rasi
component: Rofi
selectors:
  - name: "*"
    component: Selector
    properties:
      - name: background-color
        component: BackgroundColor
        pattern: "^#[0-9a-fA-F]{6}$"
      - name: text-color
        component: TextColor
        pattern: "^#[0-9a-fA-F]{6}$"
  - name: window
    component: WindowSelector
    properties:
      - name: width
        component: Width
        pattern: "^[0-9]+(px|%)$"
      - name: location
        component: Location
        enum: ["center", "north", "south", "east", "west"]
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
        assert_eq!(schema.root_component.as_str(), "Rofi");
        assert_eq!(schema.selectors.len(), 2);
        assert_eq!(schema.selectors[0].name, "*");
        assert_eq!(schema.selectors[0].properties.len(), 2);
        assert_eq!(schema.selectors[1].component.as_str(), "WindowSelector");
        assert!(schema.template.contains("{{ selector.name }}"));
    }

    #[test]
    fn rejects_a_schema_missing_the_template_document() {
        let one_doc_only = ROFI_SCHEMA.split("---").next().unwrap();
        let err = parse(one_doc_only).unwrap_err();
        assert!(matches!(err, GenError::MissingDocument("template")));
    }

    #[test]
    fn rejects_a_schema_missing_a_required_component_field() {
        let broken = r#"
grammar: rasi
component: Rofi
selectors:
  - name: "*"
    properties:
      - name: background-color
        component: BackgroundColor
        pattern: "^#[0-9a-fA-F]{6}$"
---
template: "irrelevant"
"#;
        let err = parse(broken).unwrap_err();
        assert!(matches!(err, GenError::SchemaShape(_)), "got {err:?}");
    }

    /// The exact bug the design record's §14.6 caught by hand-tracing generated output:
    /// two different selectors reusing `component: Selector` silently produces one
    /// generated function that the second definition would shadow. It must be a rejected
    /// schema, not a silently-broken generated file.
    #[test]
    fn rejects_two_selectors_sharing_one_component_name() {
        let broken = r#"
grammar: rasi
component: Rofi
selectors:
  - name: "*"
    component: Selector
    properties:
      - name: background-color
        component: BackgroundColor
        pattern: "^#[0-9a-fA-F]{6}$"
  - name: window
    component: Selector
    properties:
      - name: width
        component: Width
        pattern: "^[0-9]+(px|%)$"
---
template: "irrelevant"
"#;
        let err = parse(broken).unwrap_err();
        assert!(matches!(err, GenError::DuplicateComponent(name) if name == "Selector"));
    }

    #[test]
    fn rejects_an_unsafe_component_name_before_it_ever_reaches_codegen() {
        let malicious = r#"
grammar: rasi
component: Rofi
selectors:
  - name: "*"
    component: "}); evil(); (({ "
    properties:
      - name: background-color
        component: BackgroundColor
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
}
