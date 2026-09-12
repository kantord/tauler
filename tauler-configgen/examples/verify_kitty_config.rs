/// Generates the kitty settings JSX from the shipped schema, then renders it
/// against data that faithfully mirrors every setting in the real,
/// currently-deployed ~/.config/kitty/tauler-kitty-settings.conf — checked
/// here, not deployed.
fn main() {
    let src = std::fs::read_to_string("examples/kitty-config.schema.yaml").unwrap();
    let schema = tauler_configgen::parse(&src).expect("schema must parse");
    let generated = tauler_configgen::annotate_with_source_path(
        &tauler_configgen::generate(&schema),
        "tauler-configgen/examples/kitty-config.schema.yaml",
    );
    std::fs::write("/tmp/kitty-config.gen.jsx", &generated).unwrap();
    println!(
        "wrote generated component source to /tmp/kitty-config.gen.jsx ({} bytes)",
        generated.len()
    );

    let data = serde_json::json!({
        "settings": [
            { "directive": "font_family", "value": "JetBrains Mono" },
            { "directive": "bold_font", "value": "auto" },
            { "directive": "italic_font", "value": "auto" },
            { "directive": "bold_italic_font", "value": "auto" },
            { "directive": "adjust_line_height", "value": "100%" },
            { "directive": "font_size", "value": "14" },
            { "directive": "auto_reload_config", "value": "0.5" },
            { "directive": "progress_bar", "value": "top" },
            { "directive": "scrollback_lines", "value": "10000" },
            { "directive": "enable_audio_bell", "value": "no" },
            { "directive": "shell_integration", "value": "enabled" },
            { "directive": "shell", "value": "/home/kantord/.cargo/bin/enw shell" },
            { "directive": "remember_window_size", "value": "no" },
            { "directive": "initial_window_width", "value": "830" },
            { "directive": "initial_window_height", "value": "700" },
            { "directive": "inactive_text_alpha", "value": "0.6" },
            { "directive": "repaint_delay", "value": "8" },
            { "directive": "input_delay", "value": "2" },
            { "directive": "allow_remote_control", "value": "yes" },
        ]
    });

    let conf = tauler_configgen::render_template(&schema.template, &data)
        .expect("template must render against the faithful port");
    std::fs::write("/tmp/tauler-kitty-settings.conf", &conf).unwrap();
    println!(
        "wrote rendered .conf to /tmp/tauler-kitty-settings.conf ({} bytes)",
        conf.len()
    );
    println!("\n{}", conf);
}
