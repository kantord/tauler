//! Renders a schema's own template (its second YAML document) against structured data —
//! the step the design record's chat discussion placed inside a new `renderTemplate`
//! runtime builtin, called from the generated root component with the schema's template
//! text embedded as a JSON-escaped constant (see [`crate::codegen::generate`]).
//!
//! This module proves the Rust-side half works — given data shaped exactly like what the
//! generated JS components would produce, does the template produce the right `.rasi`
//! text — without yet wiring a QuickJS builtin (design record §14.10: that integration
//! step is explicitly not done in this prototype).
//!
//! `{{ prop.value }}` interpolates a runtime VALUE into OUTPUT TEXT, which is a different
//! injection surface than [`crate::codegen`]'s — not JS source, but the target config
//! format's own syntax. A schema whose `pattern`/`enum` is loose enough to allow the
//! target format's own special characters (`\n`, `{`, `}`, `;` for rasi) is not safe to
//! render even though its VALUES are perfectly valid JS strings — the schema's pattern is
//! the escaping mechanism for this surface, not a separate one.

use minijinja::Environment;

#[derive(Debug, thiserror::Error)]
#[error("template did not render: {0}")]
pub struct RenderError(#[from] minijinja::Error);

pub fn render_template(source: &str, data: &serde_json::Value) -> Result<String, RenderError> {
    let mut env = Environment::new();
    env.add_template("_", source)?;
    let rendered = env.get_template("_")?.render(data)?;
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RASI_TEMPLATE: &str = r#"{% for selector in selectors %}
{{ selector.name }} {
{% for prop in selector.properties %}
    {{ prop.css_name }}: {{ prop.value }};
{% endfor %}
}
{% endfor %}"#;

    /// The exact data shape §codegen's generated components would produce for the worked
    /// example (design record §14.7/§14.8), rendered against the schema's own template.
    #[test]
    fn renders_the_worked_example_to_the_expected_rasi_text() {
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

        let out = render_template(RASI_TEMPLATE, &data).unwrap();

        assert!(out.contains("* {"), "got: {out}");
        assert!(out.contains("background-color: #221F2B;"), "got: {out}");
        assert!(out.contains("text-color: #E9E4DA;"), "got: {out}");
    }

    #[test]
    fn multiple_selectors_each_get_their_own_block() {
        let data = json!({
            "selectors": [
                { "name": "*", "properties": [{ "css_name": "background-color", "value": "#221F2B" }] },
                { "name": "window", "properties": [{ "css_name": "width", "value": "480px" }] },
            ]
        });

        let out = render_template(RASI_TEMPLATE, &data).unwrap();

        assert!(out.contains("* {"), "got: {out}");
        assert!(out.contains("window {"), "got: {out}");
        assert!(out.contains("width: 480px;"), "got: {out}");
    }

    #[test]
    fn a_syntactically_broken_template_fails_at_load_not_at_first_use() {
        let broken = "{% for x in selectors %}{{ x.name }";
        let err = render_template(broken, &json!({ "selectors": [] }));
        assert!(err.is_err());
    }
}
