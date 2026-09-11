//! Prototype: schema → readable JSX component source, for tauler's config-file feature.
//!
//! Design record: `~/Downloads/tauler-config-files-design-record.md`, §13 (the codegen
//! model, verified against real tauler source) and §14 (the tech stack and worked
//! example this crate makes real). Not yet wired into tauler's own QuickJS runtime — see
//! §14.10 and this crate's `README` in that same section for what's still missing.
//!
//! The pipeline: [`schema::parse`] reads a two-document YAML schema (structure, then a
//! minijinja template) and returns a [`schema::RasiSchema`] with every name already
//! validated; [`codegen::generate`] turns that into the actual `.jsx` source a layout file
//! would import; [`render::render_template`] is the Rust-side proof that the schema's own
//! template, run against data shaped like what the generated components produce, yields
//! the correct output text.

mod codegen;
mod identifier;
mod render;
mod schema;

pub use codegen::generate;
pub use identifier::{Identifier, InvalidIdentifier};
pub use render::{render_template, RenderError};
pub use schema::{parse, GenError, PropertySchema, RasiSchema, SelectorSchema};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use serde_json::json;

    const ROFI_SCHEMA: &str = include_str!("../examples/rofi.schema.yaml");

    /// The whole pipeline, end to end: parse the real example schema shipped with this
    /// crate, generate its JS, and confirm the generated source's embedded template
    /// renders — via the exact same `render_template` a `renderTemplate` builtin would
    /// call — to the `.rasi` text the design record's §14.8 traced by hand.
    #[test]
    fn the_rofi_example_schema_parses_generates_and_renders_correctly() {
        let schema = parse(ROFI_SCHEMA).expect("the shipped example schema must parse");

        let generated = generate(&schema);
        assert!(generated.contains("export function Rofi(props)"));
        assert!(generated.contains("export function BackgroundColor(props)"));
        assert!(generated.contains("export function WindowSelector(props)"));

        // The data shape the generated `BackgroundColor`/`TextColor`/`Selector`
        // components would actually produce, if this were run inside QuickJS against
        // `<Selector><BackgroundColor value="#221F2B"/><TextColor value="#E9E4DA"/></Selector>`.
        let data = json!({
            "selectors": [
                {
                    "name": "*",
                    "properties": [
                        { "css_name": "background-color", "value": "#221F2B" },
                        { "css_name": "text-color", "value": "#E9E4DA" },
                    ]
                }
            ]
        });

        let rasi = render_template(&schema.template, &data).unwrap();
        assert!(rasi.contains("* {"), "got: {rasi}");
        assert!(rasi.contains("background-color: #221F2B;"), "got: {rasi}");
        assert!(rasi.contains("text-color: #E9E4DA;"), "got: {rasi}");
    }

    /// A value that does not match its property's `pattern` must be impossible to end up
    /// in rendered output — enforced by the GENERATED code at prop-validation time, not by
    /// this crate at render time. This test documents that boundary: `render_template`
    /// itself does not validate; it trusts data has already passed the generated
    /// component's checks, the same way `ConfigFile`'s `render()` trusts its own tree.
    #[test]
    fn render_template_does_not_itself_validate_thats_the_generated_components_job() {
        let schema = parse(ROFI_SCHEMA).unwrap();
        let data = json!({
            "selectors": [
                { "name": "*", "properties": [{ "css_name": "background-color", "value": "not-a-color" }] }
            ]
        });
        // renders anyway — proving validation must have already happened upstream
        let rasi = render_template(&schema.template, &data).unwrap();
        assert!(rasi.contains("background-color: not-a-color;"));
    }
}
