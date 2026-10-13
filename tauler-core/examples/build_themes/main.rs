//! Regenerates the built-in themes listed in `src/themes/themebuild.yaml`.
//!
//!     cargo run -p tauler-core --example build_themes
//!
//! For each workload: check the scheme out at its pinned commit into a temp dir, read its seed
//! (background, foreground, 16 ANSI colours) in a headless `nvim --clean` for each mode, derive
//! every token from the seed (`derive.rs`, the same way for every scheme), and write
//! `src/themes/<name>/theme.yaml` plus the upstream LICENSE verbatim. The output is committed;
//! nothing here runs at build or run time of tauler.
//!
//! DECISION: a Cargo example, not a script. The repo's tooling is Rust and shell, and an
//! example reuses `serde_yaml` and `tauler_core::theme::contrast` with no new crate in the
//! workspace. Rejected: Python with PyYAML (a second language for one tool), a new workspace
//! member (every member is a release-plz and manifest-test concern).
//!
//! DECISION: the temp checkout is reused when it already exists at the pinned commit. Two
//! workloads on the same repo and commit share one clone.

mod derive;

use derive::{derive, token_order, Seed};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauler_core::theme::ThemeMode;

#[derive(Deserialize)]
struct Conf {
    themes: Vec<Workload>,
}

#[derive(Deserialize)]
struct Workload {
    name: String,
    repo: String,
    rev: String,
    license: String,
    colorscheme: String,
    /// A hint for schemes that branch on `&background`; see `dump.lua`.
    #[serde(default = "default_background")]
    background: String,
    /// The ANSI colour that is the accent. See `derive.rs` for why this is a parameter.
    #[serde(default = "default_primary")]
    primary: String,
}

fn default_background() -> String {
    "dark".to_string()
}

fn default_primary() -> String {
    "blue".to_string()
}

#[derive(Deserialize)]
struct Dump {
    bg: String,
    fg: String,
    ansi: Vec<String>,
    mode: String,
    #[serde(default)]
    #[allow(dead_code)]
    fallback: bool,
}

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// A checkout of `repo` at exactly `rev`, in the temp dir.
fn checkout(repo: &str, rev: &str) -> PathBuf {
    assert_eq!(rev.len(), 40, "`rev` must be a full commit, got {rev:?}");
    let slug: String = repo
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = std::env::temp_dir()
        .join("tauler-themebuild")
        .join(format!("{slug}-{rev}"));
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(&dir).expect("create checkout dir");
        run(Command::new("git").args(["init", "--quiet"]).arg(&dir));
        run(Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["fetch", "--quiet", "--depth", "1", repo, rev]));
        run(Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["checkout", "--quiet", "FETCH_HEAD"]));
    }
    dir
}

/// The seed (and the mode it turned out to be) `w`'s colour scheme gives in a pristine Neovim with
/// only `repo_dir` on its runtimepath.
fn seed(dump_lua: &Path, repo_dir: &Path, w: &Workload) -> (Seed, ThemeMode) {
    let json = run(Command::new("nvim")
        .args(["--headless", "--clean", "-l"])
        .arg(dump_lua)
        .env("RTP", repo_dir)
        .env("CS", &w.colorscheme)
        .env("BGMODE", &w.background));
    let dump: Dump = serde_json::from_str(&json).expect("dump.lua prints JSON");
    let mode = match dump.mode.as_str() {
        "dark" => ThemeMode::Dark,
        "light" => ThemeMode::Light,
        other => panic!("{}: unknown &background `{other}`", w.name),
    };
    let seed = Seed {
        bg: dump.bg,
        fg: dump.fg,
        ansi: dump
            .ansi
            .try_into()
            .unwrap_or_else(|v: Vec<String>| panic!("expected 16 ANSI colours, got {}", v.len())),
    };
    // The label and the colours must agree, or the theme would be shown on the wrong background.
    assert_eq!(
        derive::is_dark(&seed.bg),
        mode == ThemeMode::Dark,
        "{}: &background is `{}` but its background {} is {}",
        w.name,
        dump.mode,
        seed.bg,
        if derive::is_dark(&seed.bg) {
            "dark"
        } else {
            "light"
        }
    );
    (seed, mode)
}

fn block(tokens: &std::collections::BTreeMap<String, String>) -> String {
    token_order()
        .iter()
        .map(|k| format!("  {k}: \"{}\"\n", tokens[k]))
        .collect()
}

fn main() {
    let themes_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/themes");
    let conf: Conf = serde_yaml::from_str(
        &std::fs::read_to_string(themes_dir.join("themebuild.yaml")).expect("read themebuild.yaml"),
    )
    .expect("parse themebuild.yaml");
    let dump_lua = themes_dir.join("generate/dump.lua");

    for w in conf.themes {
        let repo_dir = checkout(&w.repo, &w.rev);
        let (seed, mode) = seed(&dump_lua, &repo_dir, &w);
        let tokens = derive(&seed, &w.primary);
        let (donor, ui) = derive::accent_separation(&seed);
        assert!(
            ui >= donor.min(derive::MIN_SEPARATION) - 1e-9,
            "{}: making its accents readable pulled two of them together (distance {donor:.3} -> {ui:.3}); \
             give this workload a different `primary`, or drop the scheme",
            w.name
        );

        let out_dir = themes_dir.join(&w.name);
        std::fs::create_dir_all(&out_dir).expect("create theme dir");
        let header = format!(
            "# GENERATED by `cargo run -p tauler-core --example build_themes` — do not edit.\n\
             # Source: {} at {} ({}), colour scheme `{}`.\n\
             # Derived from the scheme's background, foreground and 16 terminal colours by\n\
             # examples/build_themes/derive.rs; the ANSI colours are the scheme's own, unchanged.\n",
            w.repo, w.rev, w.license, w.colorscheme
        );
        let mode_label = match mode {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        };
        std::fs::write(
            out_dir.join("theme.yaml"),
            format!(
                "{header}name: {}\nmode: {mode_label}\ncolors:\n{}",
                w.name,
                block(&tokens)
            ),
        )
        .expect("write theme.yaml");

        // The upstream licence text, copied verbatim under its own file name. Repositories name
        // it differently (`LICENSE`, `LICENSE.md`, `COPYING`...); one with none is refused.
        let licence_names = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"];
        let found: Vec<&str> = licence_names
            .iter()
            .copied()
            .filter(|n| repo_dir.join(n).exists())
            .collect();
        assert!(
            !found.is_empty(),
            "{} has no licence file at {} — refusing to ship an unlicensed theme",
            w.repo,
            w.rev
        );
        for file in found.iter().copied().chain(["NOTICE"]) {
            let src = repo_dir.join(file);
            if src.exists() {
                std::fs::copy(&src, out_dir.join(file)).expect("copy licence file");
            }
        }
        println!("wrote {} ({mode_label})", out_dir.display());
    }
}
