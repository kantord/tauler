// tauler-screenshot: the CLI over the library in lib.rs — arg parsing and file
// I/O around one `tauler_screenshot::render` call.

use clap::Parser;
use tauler_screenshot::{render, Options, Screen, ThemeMode};

fn parse_theme_mode(s: &str) -> Result<ThemeMode, String> {
    match s {
        "dark" => Ok(ThemeMode::Dark),
        "light" => Ok(ThemeMode::Light),
        other => Err(format!("unknown theme '{other}'; expected dark or light")),
    }
}

fn parse_screen(s: &str) -> Result<Screen, String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got '{s}'"))?;
    let parse = |v: &str| v.parse::<u32>().map_err(|e| format!("'{v}': {e}"));
    Ok(Screen {
        width: parse(w)?,
        height: parse(h)?,
    })
}

#[derive(serde::Deserialize)]
struct StreamValue {
    bin: String,
    script: Option<String>,
    line: String,
}

#[derive(Parser)]
#[command(name = "tauler-screenshot")]
struct Args {
    #[arg(long)]
    input: String,

    /// Where to write the SVG. Optional, so a layout wanted only for its
    /// `--html-out` need not be painted to a file as well.
    #[arg(long)]
    output: Option<String>,

    /// Also write the layout's `<dom>` surface as markup — the same tree the
    /// SVG paints, walked the way the browser runtime walks it (ADR 0026). An
    /// error when the layout declares no `<dom>`.
    #[arg(long)]
    html_out: Option<std::path::PathBuf>,

    /// A JSON file of stream values: an array of `{"bin", "script", "line"}`
    /// objects (`script` may be omitted), keyed the way the layout declares
    /// each stream. Streams not listed resolve to empty values.
    #[arg(long)]
    stream_values: Option<std::path::PathBuf>,

    /// The screen `ctx` describes, as WIDTHxHEIGHT in logical pixels.
    #[arg(long, default_value = "1920x1080", value_parser = parse_screen)]
    screen: Screen,

    #[arg(long, default_value = "dark", value_parser = parse_theme_mode)]
    theme: ThemeMode,

    /// Render width in CSS pixels. The document is this wide (including 16px margins).
    #[arg(long, default_value_t = tauler::preview::WIDTH)]
    width: u32,

    /// Path to a TTF/OTF font file to use as the primary (sans-serif) font.
    #[arg(long)]
    font_path: Option<std::path::PathBuf>,

    /// Path to the symbol font file for `<Icon>` glyphs. Without it fontconfig picks
    /// one, which varies by host — pass the vendored file for a reproducible render.
    #[arg(long)]
    symbol_font_path: Option<std::path::PathBuf>,

    /// Register only `--font-path` and `--symbol-font-path` and never consult the
    /// host's fonts, so the output is the same on every machine.
    #[arg(long)]
    files_only: bool,

    /// Also write every Tailwind class the resolved tree carries, one per line.
    ///
    /// The web renderer hands `class` to the browser verbatim, so Tailwind has to be told
    /// which utilities to compile — and a text scan of the sources cannot say, because
    /// `theme/resolver.rs` has already rewritten `bg-background` into `bg-[#hex]` by the
    /// time anything renders. Harvesting from the tree that actually rendered is the only
    /// list that is guaranteed complete (ADR 0026).
    #[arg(long)]
    classes_out: Option<std::path::PathBuf>,

    /// Also write every painted node's box, keyed by render path, as JSON.
    ///
    /// The other half of the geometry gate: the browser reports the same paths from
    /// `getBoundingClientRect()`, and the two are compared node by node (ADR 0028).
    /// Boxes are in CSS pixels, same as the SVG itself.
    #[arg(long)]
    geometry_out: Option<std::path::PathBuf>,
}

fn main() {
    let args = Args::parse();

    let source = std::fs::read_to_string(&args.input)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", args.input, e));

    let stream_values = match &args.stream_values {
        Some(path) => {
            let body = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            let values: Vec<StreamValue> = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("{} is not a stream-values file: {e}", path.display()));
            values
                .into_iter()
                .map(|v| ((v.bin, v.script), v.line))
                .collect()
        }
        None => Default::default(),
    };

    let shot = render(
        &source,
        &Options {
            theme: args.theme,
            width: args.width,
            font_path: args.font_path,
            symbol_font_path: args.symbol_font_path,
            files_only: args.files_only,
            screen: args.screen,
            stream_values,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));

    if let Some(path) = &args.html_out {
        let dom = shot.dom.as_deref().unwrap_or_else(|| {
            panic!("--html-out asked for markup, but the layout declares no <dom>")
        });
        std::fs::write(path, dom).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        eprintln!("wrote {} ({} bytes)", path.display(), dom.len());
    }

    if let Some(path) = &args.classes_out {
        let body = shot.classes.iter().cloned().collect::<Vec<_>>().join("\n");
        std::fs::write(path, body).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    if let Some(path) = &args.geometry_out {
        let boxes: Vec<serde_json::Value> = shot
            .geometry()
            .into_iter()
            .map(|b| {
                serde_json::json!({
                    "path": b.path,
                    "x": b.x,
                    "y": b.y,
                    "width": b.width,
                    "height": b.height,
                })
            })
            .collect();
        std::fs::write(
            path,
            serde_json::to_string_pretty(&boxes).expect("serialise boxes"),
        )
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    if let Some(output) = &args.output {
        std::fs::write(output, &shot.svg).unwrap_or_else(|e| panic!("write {output}: {e}"));
        eprintln!("wrote {output} ({} bytes)", shot.svg.len());
    }
}
