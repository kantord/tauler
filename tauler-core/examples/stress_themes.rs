//! Runs the theme derivation over every colour scheme in a manifest and reports what happened.
//!
//!     cargo run -p tauler-core --example stress_themes -- manifest.json > report.tsv
//!
//! The manifest is a JSON array of `{ repo, rtp, colorscheme, background, license }`. Each entry
//! is run through the same `dump.lua` and `derive.rs` as `build_themes`, so what this reports is
//! what a workload for that scheme would do. It writes nothing but the report.
//!
//! One line per entry, tab separated: repo, colorscheme, background hint, then `ok` with the
//! measurements or `FAIL` with the first line of the reason.

#![allow(dead_code)]

#[path = "build_themes/derive.rs"]
mod derive;

use derive::{
    accent_separation, ansi_hue_misses, derive, distance, is_dark, token_order, ui_accents, Seed,
    MIN_SEPARATION,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauler_core::theme::{contrast, Theme};

#[derive(Deserialize, Clone)]
struct Entry {
    repo: String,
    rtp: String,
    colorscheme: String,
    background: String,
    license: String,
}

#[derive(Deserialize)]
struct Dump {
    bg: String,
    fg: String,
    ansi: Vec<String>,
    mode: String,
    #[serde(default)]
    fallback: bool,
}

fn run_one(dump_lua: &PathBuf, e: &Entry) -> Result<String, String> {
    let out = Command::new("nvim")
        .args(["--headless", "--clean", "-l"])
        .arg(dump_lua)
        .env("RTP", &e.rtp)
        .env("CS", &e.colorscheme)
        .env("BGMODE", &e.background)
        .output()
        .map_err(|err| format!("spawn nvim: {err}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(msg
            .lines()
            .next()
            .unwrap_or("nvim failed")
            .trim()
            .to_string());
    }
    let dump: Dump =
        serde_json::from_slice(&out.stdout).map_err(|err| format!("dump is not JSON: {err}"))?;
    let ansi: [String; 16] = dump
        .ansi
        .clone()
        .try_into()
        .map_err(|v: Vec<String>| format!("{} ANSI colours", v.len()))?;
    for c in [&dump.bg, &dump.fg].into_iter().chain(ansi.iter()) {
        if color::parse_color(c).is_err() {
            return Err(format!("`{c}` is not a colour"));
        }
    }
    let seed = Seed {
        bg: dump.bg.clone(),
        fg: dump.fg.clone(),
        ansi,
    };
    let mode_agrees = is_dark(&seed.bg) == (dump.mode == "dark");

    // Derive with the default accent; a panic is a failure of the derivation, not of the scheme.
    let tokens = std::panic::catch_unwind(|| derive(&seed, "blue"))
        .map_err(|_| "derive panicked".to_string())?;
    let yaml_colors: String = token_order()
        .iter()
        .map(|k| format!("  {k}: \"{}\"\n", tokens[k]))
        .collect();
    let theme = Theme::from_yaml(&format!(
        "name: t\nmode: {}\ncolors:\n{yaml_colors}",
        if is_dark(&seed.bg) { "dark" } else { "light" }
    ))
    .map_err(|err| format!("derived theme does not parse: {err}"))?;
    let missing = theme.missing_tokens().len();
    let floor_fail = contrast::failures(&theme, contrast::HARD_FLOOR).len();

    let (donor, ui) = ui_accents(&seed);
    let nudged = donor.iter().zip(&ui).filter(|(a, b)| a != b).count();
    let (donor_sep, ui_sep) = accent_separation(&seed);
    let guard_ok = ui_sep >= donor_sep.min(MIN_SEPARATION) - 1e-9;
    let raw_fg_ratio = contrast::ratio(&seed.fg, &seed.bg).unwrap_or(0.0);
    let fg_nudged = tokens["foreground"] != seed.fg;

    // The derived theme itself, for looking at.
    if let Ok(dir) = std::env::var("STRESS_OUT") {
        let name = format!(
            "{}__{}__{}",
            e.repo.replace('/', "_"),
            e.colorscheme,
            dump.mode
        );
        let _ = std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.yaml")),
            format!(
                "name: {name}\nmode: {}\ncolors:\n{yaml_colors}",
                if is_dark(&seed.bg) { "dark" } else { "light" }
            ),
        );
    }

    // How far each surface and line sits from the page: a panel that is the page's own colour is
    // an invisible panel.
    let sep = |token: &str| distance(&tokens[token], &tokens["background"]);
    let surfaces = [
        ("card", sep("card")),
        ("muted", sep("muted")),
        ("border", sep("border")),
        ("accent", sep("accent")),
        ("input", sep("input")),
    ];
    let (weakest, weakest_sep) = surfaces
        .iter()
        .copied()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();

    let misses = ansi_hue_misses(&seed)
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join("+");

    // A key to spot two entries that are the same scheme.
    let key = format!("{}|{}|{}", seed.bg, seed.fg, seed.ansi.join(","));
    Ok(format!(
        "ok\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{}\t{:.3}\t{:.3}\t{}\t{}\t{}:{:.3}\t{:.3}\t{}",
        dump.mode,
        u8::from(mode_agrees),
        u8::from(dump.fallback),
        missing,
        floor_fail,
        seed.bg,
        raw_fg_ratio,
        u8::from(fg_nudged),
        nudged,
        donor_sep,
        ui_sep,
        u8::from(guard_ok),
        key,
        weakest,
        weakest_sep,
        sep("card"),
        if misses.is_empty() {
            "-".to_string()
        } else {
            misses
        }
    ))
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: stress_themes <manifest.json>");
    let entries: Vec<Entry> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read manifest"))
            .expect("parse manifest");
    let dump_lua = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/themes/generate/dump.lua");

    println!("repo\tcolorscheme\tbg_hint\tresult\tmode\tmode_agrees\tfallback\tmissing\tfloor_fail\tbg\tfg_ratio\tfg_nudged\taccents_nudged\tdonor_sep\tui_sep\tguard_ok\tseed_key\tweakest_surface\tcard_sep\thue_misses\tlicense");
    let queue = Arc::new(Mutex::new(entries.into_iter().collect::<Vec<_>>()));
    let lines: Arc<Mutex<BTreeMap<usize, String>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let total = queue.lock().unwrap().len();
    let next = Arc::new(Mutex::new(0usize));
    let work: Vec<Entry> = queue.lock().unwrap().clone();
    let work = Arc::new(work);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (work, next, lines, dump_lua) =
                (work.clone(), next.clone(), lines.clone(), dump_lua.clone());
            std::thread::spawn(move || loop {
                let i = {
                    let mut n = next.lock().unwrap();
                    let i = *n;
                    *n += 1;
                    i
                };
                if i >= total {
                    break;
                }
                let e = &work[i];
                let body = match run_one(&dump_lua, e) {
                    Ok(ok) => ok,
                    Err(why) => format!("FAIL\t{}", why.replace(['\t', '\n'], " ")),
                };
                lines.lock().unwrap().insert(
                    i,
                    format!(
                        "{}\t{}\t{}\t{}\t{}",
                        e.repo, e.colorscheme, e.background, body, e.license
                    ),
                );
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    for l in lines.lock().unwrap().values() {
        println!("{l}");
    }
}
