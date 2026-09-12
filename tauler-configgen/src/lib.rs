//! Prototype: schema → readable JSX component source, for tauler's config-file feature.
//!
//! Design record: `~/Downloads/tauler-config-files-design-record.md`, §13 (the codegen
//! model, verified against real tauler source), §14 (the tech stack and worked example
//! this crate makes real), and §17 (making the schema/generator config-format-independent
//! — there is no rasi-specific concept anywhere in this crate's Rust code; the only place
//! any format's syntax is known is a schema file's own data and its template text). Not
//! yet wired into tauler's own QuickJS runtime — see this crate's `README` for what's
//! still missing.
//!
//! The pipeline: [`schema::parse`] reads a two-document YAML schema (a flat list of node
//! declarations, then a minijinja template) and returns a [`schema::Schema`] with every
//! name already validated; [`codegen::generate`] turns that into the actual `.jsx` source
//! a layout file would import; [`render::render_template`] is the Rust-side proof that the
//! schema's own template, run against data shaped like what the generated components
//! produce, yields the correct output text.

mod codegen;
mod identifier;
mod render;
mod schema;

// The core pipeline: parse a schema, generate its JSX, prove its template renders.
pub use codegen::generate;
pub use identifier::{Identifier, InvalidIdentifier};
pub use render::{render_template, RenderError};
pub use schema::{parse, GenError, NodeSchema, PropRule, Schema};

// Deployment helper, not a schema-processing primitive: stamps a source-schema comment
// into `generate`'s output. Exists because every real deployment so far (rofi's theme,
// rofi's `configuration{}`, kitty's settings) needed this line, and having each of this
// crate's own `examples/verify_*.rs` scripts call it is how that stopped being manual
// copy-paste — not something a schema-format-agnostic consumer would otherwise expect
// from this crate's core API.
pub use codegen::annotate_with_source_path;

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
        assert!(generated.contains("export function Selector(props)"));

        // The data shape the generated `BackgroundColor`/`TextColor`/`Selector`
        // components would actually produce, if this were run inside QuickJS against
        // `<Selector name="*"><BackgroundColor value="#221F2B"/><TextColor value="#E9E4DA"/></Selector>`.
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

    /// Verifies the ACTUAL shipped schema file's `Padding`/`Border` patterns — not a
    /// hand-copied Rust constant that could silently drift from the real YAML through an
    /// escaping mistake. `regex` here is a dev-dependency only; the pattern itself reaches
    /// generated JS as a `new RegExp(...)` argument, per `codegen`'s injection-safety rule.
    #[test]
    fn the_shipped_schemas_padding_and_border_patterns_match_real_gruvbox_values() {
        let schema = parse(ROFI_SCHEMA).unwrap();
        let padding = &schema
            .nodes
            .iter()
            .find(|n| n.component.as_str() == "Padding")
            .unwrap()
            .props["value"];
        let border = &schema
            .nodes
            .iter()
            .find(|n| n.component.as_str() == "Border")
            .unwrap()
            .props["value"];

        let padding_re = regex::Regex::new(padding.pattern.as_ref().unwrap()).unwrap();
        let border_re = regex::Regex::new(border.pattern.as_ref().unwrap()).unwrap();

        // from /usr/share/rofi/themes/gruvbox-common.rasinc, cited in the rasi format research
        assert!(padding_re.is_match("0 0.3em 0 0"));
        assert!(border_re.is_match("2px solid 0 0"));

        // and the wrong shapes are still rejected
        assert!(
            !padding_re.is_match("2px 0 0 4px 1px"),
            "five fields must be rejected"
        );
        assert!(
            !border_re.is_match("2px 0 0 0 0"),
            "five groups must be rejected"
        );
        assert!(
            !padding_re.is_match("1.5px"),
            "px is Integer-only, per man rofi-theme"
        );
    }
}
