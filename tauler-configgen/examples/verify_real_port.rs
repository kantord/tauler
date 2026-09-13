/// Generates the real theme JSX from the shipped schema, then renders it against data
/// that faithfully mirrors every selector/property in the user's actual
/// ~/.local/share/chezmoi/dot_config/rofi/theme.rasi.tmpl — checked here, not deployed.
fn main() {
    let src = std::fs::read_to_string("examples/rofi-full-theme.schema.yaml").unwrap();
    let schema = tauler_configgen::parse(&src).expect("schema must parse");
    let generated = tauler_configgen::generate(&schema);
    std::fs::write("/tmp/rofi-theme.gen.jsx", &generated).unwrap();
    println!("wrote generated component source to /tmp/rofi-theme.gen.jsx ({} bytes)", generated.len());

    // Faithful port of every selector/property in the real theme.rasi.tmpl,
    // using literal hex values, matching how RofiTheme.jsx now reads them from
    // rofi-colors.json rather than an @-reference into colors.rasi.
    let data = serde_json::json!({
        "selectors": [
            { "name": "*", "properties": [
                { "css_name": "font", "value": "JetBrains Mono 20", "quoted": true },
                { "css_name": "background-color", "value": "transparent" },
                { "css_name": "text-color", "value": "#e7dcc4" },
            ]},
            { "name": "window", "properties": [
                { "css_name": "location", "value": "south" },
                { "css_name": "anchor", "value": "south" },
                { "css_name": "y-offset", "value": "-5%" },
                { "css_name": "width", "value": "70%" },
                { "css_name": "height", "value": "70%" },
                { "css_name": "padding", "value": "0" },
                { "css_name": "border", "value": "2px solid" },
                { "css_name": "border-color", "value": "#EDC77A" },
                { "css_name": "background-color", "value": "#3a4145" },
            ]},
            { "name": "mainbox", "properties": [
                { "css_name": "padding", "value": "16px" },
                { "css_name": "spacing", "value": "12px" },
                { "css_name": "children", "value": ["inputbar", "message", "listview"] },
            ]},
            { "name": "inputbar", "properties": [
                { "css_name": "padding", "value": "10px 12px" },
                { "css_name": "spacing", "value": "10px" },
                { "css_name": "background-color", "value": "#2c343a" },
                { "css_name": "children", "value": ["prompt", "entry"] },
            ]},
            { "name": "prompt", "properties": [
                { "css_name": "text-color", "value": "#EDC77A" },
            ]},
            { "name": "entry", "properties": [
                { "css_name": "placeholder", "value": "search…", "quoted": true },
                { "css_name": "placeholder-color", "value": "#e7dcc4" },
            ]},
            { "name": "listview", "properties": [
                { "css_name": "lines", "value": "24" },
                { "css_name": "columns", "value": "1" },
                { "css_name": "spacing", "value": "2px" },
                { "css_name": "cycle", "value": "true" },
                { "css_name": "dynamic", "value": "true" },
                { "css_name": "scrollbar", "value": "false" },
            ]},
            { "name": "element", "properties": [
                { "css_name": "padding", "value": "8px 10px" },
                { "css_name": "spacing", "value": "10px" },
            ]},
            { "name": "element selected", "properties": [
                { "css_name": "background-color", "value": "#EDC77A" },
                { "css_name": "text-color", "value": "#2c343a" },
            ]},
            { "name": "element-icon", "properties": [
                { "css_name": "size", "value": "1.3em" },
                { "css_name": "background-color", "value": "transparent" },
            ]},
            { "name": "element-text", "properties": [
                { "css_name": "background-color", "value": "transparent" },
                { "css_name": "text-color", "value": "inherit" },
                { "css_name": "vertical-align", "value": "0.5" },
            ]},
            { "name": "mode-switcher", "properties": [
                { "css_name": "spacing", "value": "6px" },
            ]},
            { "name": "button", "properties": [
                { "css_name": "padding", "value": "4px 8px" },
                { "css_name": "background-color", "value": "#2c343a" },
                { "css_name": "text-color", "value": "#e7dcc4" },
            ]},
            { "name": "button selected", "properties": [
                { "css_name": "background-color", "value": "#EDC77A" },
                { "css_name": "text-color", "value": "#2c343a" },
            ]},
            { "name": "message", "properties": [
                { "css_name": "padding", "value": "6px 8px" },
                { "css_name": "background-color", "value": "#2c343a" },
            ]},
            { "name": "textbox", "properties": [
                { "css_name": "text-color", "value": "#e7dcc4" },
            ]},
            { "name": "error-message", "properties": [
                { "css_name": "background-color", "value": "#ED9294" },
                { "css_name": "text-color", "value": "#2c343a" },
                { "css_name": "padding", "value": "8px" },
            ]},
        ]
    });

    let rasi = tauler_configgen::render_template(&schema.template, &data)
        .expect("template must render against the faithful port");
    std::fs::write("/tmp/tauler-theme.rasi", &rasi).unwrap();
    println!("wrote rendered .rasi to /tmp/tauler-theme.rasi ({} bytes)", rasi.len());
    println!("\n{}", rasi);
}
