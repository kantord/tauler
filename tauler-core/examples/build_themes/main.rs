//! Regenerates the built-in themes listed in `src/themes/themebuild.yaml`.
//!
//!     cargo run -p tauler-core --example build_themes
//!
//! For each workload: check the scheme out at its pinned commit into a temp dir, load it in a
//! headless `nvim --clean`, ask that Neovim the questions in `generate/queries.yaml` (one per
//! token, see `queries.rs`), and write `src/themes/<name>/theme.yaml` from the answers plus the
//! upstream licence verbatim. The output is committed; nothing here runs at build or run time of
//! tauler.
//!
//! DECISION: a theme is exactly what the scheme answers. Nothing is blended, nudged or guessed: a
//! token with no answer, or a text/surface pair under the contrast floor, stops the workload with
//! the names of the offenders, and the fix is a different query for that token (`queries:` on the
//! workload) or a different scheme. Rejected: deriving the tokens from the background, foreground
//! and ANSI colours and repairing what came out, which could ship any scheme but ~400 lines of
//! colour code later and with colours that were no longer the scheme's own.
//!
//! DECISION: a Cargo example, not a script. The repo's tooling is Rust and shell, and an example
//! reuses `serde_yaml` and `tauler_core::theme::contrast` with no new crate in the workspace.
//!
//! DECISION: the temp checkout is reused when it already exists at the pinned commit. Two
//! workloads on the same repo and commit share one clone.

mod queries;

use queries::{ask, mode_of, run, Overrides, Table};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauler_core::theme::{contrast, Theme, ThemeMode};

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
    /// A hint for schemes that branch on `&background`.
    #[serde(default = "default_background")]
    background: String,
    /// Lua run before the scheme loads, for a scheme that must be configured to answer (a
    /// terminal-colour switch, an opaque background).
    setup: Option<String>,
    /// Token → query, replacing the table's for this scheme.
    #[serde(default)]
    queries: Overrides,
}

fn default_background() -> String {
    "dark".to_string()
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

/// `w`'s theme as `(mode, colour per token in table order)`, or why the scheme cannot ship.
fn extract(
    themes_dir: &Path,
    table: &Table,
    w: &Workload,
    repo_dir: &Path,
) -> Result<(ThemeMode, Vec<(String, String)>), String> {
    let answers = ask(
        &themes_dir.join("generate/query.lua"),
        &repo_dir.to_string_lossy(),
        &w.colorscheme,
        &w.background,
        w.setup.as_deref(),
        &table.questions(&w.queries),
    )?;
    let page = answers
        .get(table.page())
        .ok_or("the scheme answers no page colour (a transparent background?)")?;
    let mode = mode_of(page);
    let resolved = table.resolve(mode, &w.queries, &answers);

    let missing: Vec<&str> = table.tokens().filter(|t| resolved[*t].is_none()).collect();
    if !missing.is_empty() {
        return Err(format!("no answer for: {}", missing.join(", ")));
    }
    let colours: Vec<(String, String)> = table
        .tokens()
        .map(|t| (t.to_string(), resolved[t].clone().expect("checked above")))
        .collect();

    let yaml: String = colours
        .iter()
        .map(|(t, c)| format!("  {t}: \"{c}\"\n"))
        .collect();
    let theme = Theme::from_yaml(&format!(
        "name: {}\nmode: {}\ncolors:\n{yaml}",
        w.name,
        label(mode)
    ))
    .map_err(|e| format!("the answers do not form a theme: {e}"))?;
    let under: Vec<String> = contrast::failures(&theme, contrast::HARD_FLOOR)
        .iter()
        .map(|f| {
            format!(
                "{} on {} = {:.2}",
                f.foreground,
                f.surface,
                f.ratio.unwrap_or(0.0)
            )
        })
        .collect();
    if !under.is_empty() {
        return Err(format!(
            "pairs under {}:1: {} — give one of those tokens a different query under `queries:`",
            contrast::HARD_FLOOR,
            under.join("; ")
        ));
    }
    Ok((mode, colours))
}

fn label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
    }
}

fn main() {
    let themes_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/themes");
    let conf: Conf = serde_yaml::from_str(
        &std::fs::read_to_string(themes_dir.join("themebuild.yaml")).expect("read themebuild.yaml"),
    )
    .expect("parse themebuild.yaml");
    let table = Table::parse(
        &std::fs::read_to_string(themes_dir.join("generate/queries.yaml"))
            .expect("read queries.yaml"),
    )
    .unwrap_or_else(|e| panic!("{e}"));

    let mut failed = false;
    for w in &conf.themes {
        let repo_dir = checkout(&w.repo, &w.rev);
        let (mode, colours) = match extract(&themes_dir, &table, w, &repo_dir) {
            Ok(ok) => ok,
            Err(why) => {
                eprintln!("{}: {why}", w.name);
                failed = true;
                continue;
            }
        };

        let out_dir = themes_dir.join(&w.name);
        std::fs::create_dir_all(&out_dir).expect("create theme dir");
        let header = format!(
            "# GENERATED by `cargo run -p tauler-core --example build_themes` — do not edit.\n\
             # Source: {} at {} ({}), colour scheme `{}`.\n\
             # Every colour is the scheme's own answer to a question in generate/queries.yaml.\n",
            w.repo, w.rev, w.license, w.colorscheme
        );
        let body: String = colours
            .iter()
            .map(|(t, c)| format!("  {t}: \"{c}\"\n"))
            .collect();
        std::fs::write(
            out_dir.join("theme.yaml"),
            format!(
                "{header}name: {}\nmode: {}\ncolors:\n{body}",
                w.name,
                label(mode)
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
        println!("wrote {} ({})", out_dir.display(), label(mode));
    }
    assert!(!failed, "some workloads cannot ship; see above");
}
