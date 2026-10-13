// tauler-screenshot: the CLI over the library in lib.rs — arg parsing and file
// I/O around one `tauler_screenshot::render` call.

use clap::Parser;
use tauler_screenshot::{render, Options, ThemeMode};

fn parse_theme_mode(s: &str) -> Result<ThemeMode, String> {
    match s {
        "dark" => Ok(ThemeMode::Dark),
        "light" => Ok(ThemeMode::Light),
        other => Err(format!("unknown theme '{other}'; expected dark or light")),
    }
}

#[derive(Parser)]
#[command(name = "tauler-screenshot")]
struct Args {
    /// The JSX layout to render. Not needed with `--swatch`.
    #[arg(long, required_unless_present = "swatch")]
    input: Option<String>,

    /// Resolve theme tokens against this built-in theme (`theme.name`) instead of the default.
    #[arg(long)]
    theme_name: Option<String>,

    /// Resolve theme tokens against this theme YAML file instead of the default. Takes
    /// precedence over `--theme-name`; the file must define every core and ANSI token.
    #[arg(long)]
    theme_file: Option<std::path::PathBuf>,

    /// Render the swatch scene — every token, every pair, the ANSI strip and a few
    /// components — for the theme, instead of `--input`.
    #[arg(long)]
    swatch: bool,

    #[arg(long)]
    output: String,

    /// Which shipped default to render with. With `--theme-name` or `--theme-file` it overrides
    /// the mode that theme carries, as `theme.mode` does in a config.
    #[arg(long, value_parser = parse_theme_mode)]
    theme: Option<ThemeMode>,

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

    let palette = match (&args.theme_file, args.theme_name.as_deref()) {
        (Some(path), _) => {
            let yaml = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            Some(
                tauler::theme::Theme::from_yaml(&yaml)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                    .validated()
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            )
        }
        (None, Some(name)) => {
            Some(tauler::theme::builtin::by_name(name).unwrap_or_else(|e| panic!("{e}")))
        }
        (None, None) => None,
    };
    // `--theme` overrides the mode the theme carries, as `theme.mode` does in a config.
    let palette = palette.map(|p| tauler::theme::Theme {
        mode: args.theme.unwrap_or(p.mode),
        ..p
    });

    let source = if args.swatch {
        let palette = palette.clone().unwrap_or_else(|| {
            tauler::theme::builtin::default_for(args.theme.unwrap_or(ThemeMode::Dark))
        });
        tauler::theme::swatch::jsx(&palette)
    } else {
        let input = args
            .input
            .as_deref()
            .expect("clap requires --input without --swatch");
        std::fs::read_to_string(input).unwrap_or_else(|e| panic!("failed to read {input}: {e}"))
    };

    let shot = render(
        &source,
        &Options {
            theme: args.theme.unwrap_or(ThemeMode::Dark),
            width: args.width,
            font_path: args.font_path,
            symbol_font_path: args.symbol_font_path,
            files_only: args.files_only,
            palette,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));

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

    std::fs::write(&args.output, &shot.svg)
        .unwrap_or_else(|e| panic!("write {}: {e}", args.output));

    eprintln!("wrote {} ({} bytes)", args.output, shot.svg.len());
}
