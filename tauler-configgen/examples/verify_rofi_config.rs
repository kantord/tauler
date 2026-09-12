/// Generates the rofi `configuration{}` JSX from the shipped schema, then
/// renders it against data mirroring the real deployed
/// ~/.config/rofi/config.rasi — checked here, not deployed.
fn main() {
    let src = std::fs::read_to_string("examples/rofi-config.schema.yaml").unwrap();
    let schema = tauler_configgen::parse(&src).expect("schema must parse");
    let generated = tauler_configgen::annotate_with_source_path(
        &tauler_configgen::generate(&schema),
        "tauler-configgen/examples/rofi-config.schema.yaml",
    );
    std::fs::write("/tmp/rofi-config.gen.jsx", &generated).unwrap();
    println!(
        "wrote generated component source to /tmp/rofi-config.gen.jsx ({} bytes)",
        generated.len()
    );

    let data = serde_json::json!({
        "settings": [
            { "css_name": "modes", "value": "combi,drun,run,window", "quoted": true },
            { "css_name": "combi-modes", "value": "window,drun", "quoted": true },
            { "css_name": "matching", "value": "fuzzy", "quoted": true },
            { "css_name": "sort", "value": "false" },
            { "css_name": "show-icons", "value": "true" },
            { "css_name": "terminal", "value": "kitty", "quoted": true },
            { "css_name": "window-format", "value": "{c}", "quoted": true },
            { "css_name": "drun-display-format", "value": "{name}", "quoted": true },
            { "css_name": "drun-show-actions", "value": "false" },
            { "css_name": "drun-match-fields", "value": "name,generic,keywords", "quoted": true },
            { "css_name": "drun-exclude-categories", "value": "Settings;System;Building;Debugger;IDE;Profiling;RevisionControl;Translation", "quoted": true },
            { "css_name": "display-drun", "value": "❯", "quoted": true },
            { "css_name": "display-run", "value": "❯", "quoted": true },
            { "css_name": "display-window", "value": "❯", "quoted": true },
            { "css_name": "display-combi", "value": "❯", "quoted": true },
            { "css_name": "click-to-exit", "value": "true" },
        ]
    });

    let conf = tauler_configgen::render_template(&schema.template, &data)
        .expect("template must render against the faithful port");
    std::fs::write("/tmp/config.rasi", &conf).unwrap();
    println!("wrote rendered .rasi to /tmp/config.rasi ({} bytes)", conf.len());
    println!("\n{}", conf);
}
