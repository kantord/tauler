//! Asks every colour scheme in a manifest the questions in `queries.yaml` and reports how they
//! answered.
//!
//!     cargo run -p tauler-core --example stress_themes -- manifest.json > report.tsv
//!
//! The manifest is a JSON array of `{ repo, rtp, colorscheme, background, license, setup? }`.
//! Each entry goes through the same `query.lua` and table as `build_themes`, with no overrides, so
//! what this reports is what a plain workload for that scheme would do. It writes nothing but the
//! report: one line per entry, then `#` lines that summarise.
//!
//! SPEC: the summary names every token whose answer is the same colour in more than 12% of one
//! mode's themes. A query that does that is answering from a default every scheme inherits rather
//! than from the scheme, and belongs out of the table. (Colours that are legitimately shared, such
//! as a pure-white page, show up here too; the reader decides.)

#![allow(dead_code)]

#[path = "build_themes/queries.rs"]
mod queries;

use queries::{ask, mode_of, Overrides, Table};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauler_core::theme::{contrast, Theme, ThemeMode};

/// A mode's themes sharing one answer to this share of them or more is suspicious.
const DEGENERATE_SHARE: f64 = 0.12;

#[derive(Deserialize, Clone)]
struct Entry {
    repo: String,
    rtp: String,
    colorscheme: String,
    background: String,
    license: String,
    setup: Option<String>,
}

struct Outcome {
    mode: ThemeMode,
    colours: BTreeMap<String, Option<String>>,
    pairs_under: Vec<String>,
}

fn run_one(root: &Path, table: &Table, e: &Entry) -> Result<Outcome, String> {
    let none = Overrides::new();
    let answers = ask(
        &root.join("src/themes/generate/query.lua"),
        &e.rtp,
        &e.colorscheme,
        &e.background,
        e.setup.as_deref(),
        &table.questions(&none),
    )?;
    let page = answers.get(table.page()).ok_or("no page colour")?;
    let mode = mode_of(page);
    let colours = table.resolve(mode, &none, &answers);

    // Pairs can only be judged when every token they touch has an answer.
    let present: String = colours
        .iter()
        .filter_map(|(t, c)| c.as_ref().map(|c| format!("  {t}: \"{c}\"\n")))
        .collect();
    let label = if mode == ThemeMode::Dark {
        "dark"
    } else {
        "light"
    };
    let theme = Theme::from_yaml(&format!("name: t\nmode: {label}\ncolors:\n{present}"))
        .map_err(|e| format!("answers do not form a theme: {e}"))?;
    let pairs_under = contrast::failures(&theme, contrast::HARD_FLOOR)
        .iter()
        .map(|f| format!("{} on {}", f.foreground, f.surface))
        .collect();
    Ok(Outcome {
        mode,
        colours,
        pairs_under,
    })
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: stress_themes <manifest.json>");
    let entries: Vec<Entry> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read manifest"))
            .expect("parse manifest");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let table = Arc::new(
        Table::parse(
            &std::fs::read_to_string(root.join("src/themes/generate/queries.yaml"))
                .expect("read queries.yaml"),
        )
        .unwrap_or_else(|e| panic!("{e}")),
    );

    let entries = Arc::new(entries);
    let next = Arc::new(Mutex::new(0usize));
    let results: Arc<Mutex<BTreeMap<usize, Result<Outcome, String>>>> = Arc::default();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (entries, next, results, table, root) = (
                entries.clone(),
                next.clone(),
                results.clone(),
                table.clone(),
                root.clone(),
            );
            std::thread::spawn(move || loop {
                let i = {
                    let mut n = next.lock().unwrap();
                    *n += 1;
                    *n - 1
                };
                let Some(e) = entries.get(i) else { break };
                let r = run_one(&root, &table, e);
                results.lock().unwrap().insert(i, r);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    println!("repo\tcolorscheme\tbg_hint\tresult\tmode\tunanswered\tpairs_under\tlicense");
    let results = results.lock().unwrap();
    let mut per_mode: BTreeMap<&str, Vec<&Outcome>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        let head = format!("{}\t{}\t{}", e.repo, e.colorscheme, e.background);
        match &results[&i] {
            Err(why) => println!(
                "{head}\tFAIL\t-\t{}\t-\t{}",
                why.replace(['\t', '\n'], " "),
                e.license
            ),
            Ok(o) => {
                let missing: Vec<&str> = o
                    .colours
                    .iter()
                    .filter(|(_, c)| c.is_none())
                    .map(|(t, _)| t.as_str())
                    .collect();
                let mode = if o.mode == ThemeMode::Dark {
                    "dark"
                } else {
                    "light"
                };
                let dash = |v: Vec<&str>| {
                    if v.is_empty() {
                        "-".to_string()
                    } else {
                        v.join("+")
                    }
                };
                println!(
                    "{head}\tok\t{mode}\t{}\t{}\t{}",
                    dash(missing),
                    dash(o.pairs_under.iter().map(String::as_str).collect()),
                    e.license
                );
                per_mode.entry(mode).or_default().push(o);
            }
        }
    }

    let failed = results.values().filter(|r| r.is_err()).count();
    println!("# {} entries, {} could not be asked", entries.len(), failed);
    for (mode, outcomes) in &per_mode {
        let n = outcomes.len() as f64;
        let complete = outcomes
            .iter()
            .filter(|o| o.colours.values().all(Option::is_some))
            .count();
        let clean = outcomes
            .iter()
            .filter(|o| o.colours.values().all(Option::is_some) && o.pairs_under.is_empty())
            .count();
        println!(
            "# {mode}: {} answered, {complete} with every token answered, {clean} of those with every pair at the floor",
            outcomes.len()
        );
        let mut shared = Vec::new();
        for token in table.tokens() {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for o in outcomes {
                if let Some(Some(c)) = o.colours.get(token) {
                    *counts.entry(c.as_str()).or_default() += 1;
                }
            }
            if let Some((colour, c)) = counts.into_iter().max_by_key(|(_, c)| *c) {
                if c as f64 / n > DEGENERATE_SHARE {
                    shared.push(format!("{token}={colour} in {:.0}%", 100.0 * c as f64 / n));
                }
            }
        }
        println!(
            "# {mode}: tokens answered identically in many themes: {}",
            if shared.is_empty() {
                "none".into()
            } else {
                shared.join(", ")
            }
        );
    }
}
