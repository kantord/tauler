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

/// Whether `dir` is a git checkout whose HEAD is exactly `rev`.
///
/// SPEC: a directory that merely *exists* is not a checkout. `git init` creates `.git` before the
/// fetch, so a run that died between the two leaves a repository with nothing in it, and trusting
/// the directory would hand every later run an empty scheme.
fn holds_commit(dir: &Path, rev: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == rev)
        .unwrap_or(false)
}

/// A checkout of `repo` at exactly `rev`, in the temp dir, reusing one that already holds `rev`.
fn checkout(repo: &str, rev: &str) -> PathBuf {
    assert_eq!(rev.len(), 40, "`rev` must be a full commit, got {rev:?}");
    let slug: String = repo
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = std::env::temp_dir()
        .join("tauler-themebuild")
        .join(format!("{slug}-{rev}"));
    if dir.exists() && !holds_commit(&dir, rev) {
        std::fs::remove_dir_all(&dir).expect("remove an incomplete checkout");
    }
    if !dir.exists() {
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

const LICENCE_NAMES: [&str; 4] = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"];

/// The upstream licence files in `repo_dir`, by the name that repository gave them.
///
/// SPEC: a scheme with none is refused — before anything of its theme is written, so a refusal
/// never leaves an unlicensed theme behind.
fn licence_files(repo_dir: &Path) -> Result<Vec<&'static str>, String> {
    let found: Vec<&'static str> = LICENCE_NAMES
        .into_iter()
        .filter(|n| repo_dir.join(n).exists())
        .collect();
    if found.is_empty() {
        Err("no licence file in the repository — refusing to ship an unlicensed theme".into())
    } else {
        Ok(found)
    }
}

/// Copy `files` (and a NOTICE, if there is one) verbatim from `repo_dir` into `out_dir`.
fn copy_licence_files(repo_dir: &Path, out_dir: &Path, files: &[&str]) {
    for file in files.iter().copied().chain(["NOTICE"]) {
        let src = repo_dir.join(file);
        if src.exists() {
            std::fs::copy(&src, out_dir.join(file)).expect("copy licence file");
        }
    }
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
        .get(&table.page())
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

    for w in &conf.themes {
        table
            .check_overrides(&w.queries)
            .unwrap_or_else(|e| panic!("{}: {e}", w.name));
    }

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

        let licences = match licence_files(&repo_dir) {
            Ok(files) => files,
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

        copy_licence_files(&repo_dir, &out_dir, &licences);
        println!("wrote {} ({})", out_dir.display(), label(mode));
    }
    assert!(!failed, "some workloads cannot ship; see above");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory unique to this test run.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("tauler-themebuild-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        run(&mut cmd)
    }

    #[test]
    fn a_repository_without_a_licence_is_refused() {
        let dir = scratch("no-licence");
        let err = licence_files(&dir).unwrap_err();
        assert!(err.contains("no licence file"), "{err}");
    }

    #[test]
    fn a_licence_is_found_under_the_name_the_repository_gave_it() {
        let dir = scratch("licence-md");
        std::fs::write(dir.join("LICENSE.md"), "MIT").unwrap();
        assert_eq!(licence_files(&dir), Ok(vec!["LICENSE.md"]));
    }

    #[test]
    fn a_licence_refusal_happens_before_anything_is_written() {
        // `licence_files` is called with only the repository directory; there is no output
        // directory for it to have touched.
        let repo = scratch("refusal-repo");
        let out = scratch("refusal-out");
        assert!(licence_files(&repo).is_err());
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
    }

    #[test]
    fn a_repository_with_nothing_fetched_does_not_hold_the_commit() {
        let dir = scratch("empty-init");
        git(&dir, &["init", "--quiet"]);
        assert!(
            dir.join(".git").exists(),
            "the half-made checkout looks present"
        );
        assert!(!holds_commit(&dir, &"a".repeat(40)));
    }

    #[test]
    fn a_checkout_holds_exactly_its_own_commit() {
        let dir = scratch("with-commit");
        git(&dir, &["init", "--quiet"]);
        std::fs::write(dir.join("f"), "x").unwrap();
        git(&dir, &["add", "f"]);
        git(&dir, &["commit", "--quiet", "-m", "c"]);
        let head = git(&dir, &["rev-parse", "HEAD"]).trim().to_string();
        assert!(holds_commit(&dir, &head));
        assert!(!holds_commit(&dir, &"b".repeat(40)));
    }
}
