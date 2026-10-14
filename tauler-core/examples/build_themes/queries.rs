//! The table in `queries.yaml` and what is done with a colour scheme's answers to it.
//!
//! TERM: **Query table** — for every theme token, the one question (per mode) whose answer is that
//! token's colour. It is the whole of the extraction policy: this module reads it, asks the
//! questions, follows `=token` aliases and hands back colours. It does not choose, fall back or
//! repair; a token whose question got no answer stays empty.
//! Avoid: mapping, preset.
//!
//! TERM: **Override** — a query a single workload gives for one token, replacing the table's for
//! that scheme. It is how a scheme whose own answer is unusable (a comment colour under 3:1) ships
//! anyway, and the only way: nothing is adjusted after the fact.
//! Avoid: patch, fix-up.
//!
//! SPEC: `Table::parse` accepts every core and ANSI token and the optional chart and sidebar ones,
//! in the order the file lists them, and nothing else; every alias names a token.
//!
//! SPEC: `background` has one query for both modes and is not an alias. The mode picks which column
//! to read and the mode is decided from the page colour, so the page colour cannot itself depend on
//! the mode. [`Table::page`] is that query.
//!
//! SPEC: `resolve` reads the dark column for a dark theme and the light one for a light theme, and
//! gives an alias the colour its target has *in that mode* and with that workload's overrides.
//!
//! SPEC: a query that is not in the grammar, and an override for a token that does not exist, are
//! refused where the file is read — never carried as a string until a scheme "fails to answer".
//!
//! SPEC: an answer that is not a colour is an error at the point it is received, naming the query
//! and the value; it never reaches a theme.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use tauler_core::theme::{contrast, ThemeMode, ANSI_TOKENS, CORE_TOKENS};

/// Tokens a theme may define beyond the required ones; the table lists them too.
const OPTIONAL_TOKENS: &[&str] = &[
    "chart-1",
    "chart-2",
    "chart-3",
    "chart-4",
    "chart-5",
    "sidebar",
    "sidebar-foreground",
    "sidebar-primary",
    "sidebar-primary-foreground",
    "sidebar-accent",
    "sidebar-accent-foreground",
    "sidebar-border",
    "sidebar-ring",
];

/// Which colour of a highlight group is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Attr {
    Fg,
    Bg,
    Sp,
}

impl Attr {
    fn as_str(self) -> &'static str {
        match self {
            Attr::Fg => "fg",
            Attr::Bg => "bg",
            Attr::Sp => "sp",
        }
    }
}

/// TERM: **Query** — a question put to a running Neovim that has loaded a colour scheme, answered
/// with one colour. `Group.attr` is a highlight group's `fg`, `bg` or `sp` (links followed, so it
/// is the effective value); `g:name` is a global variable (`g:terminal_color_4`); `=token` means
/// "the same colour as that other token". Nothing else: no fallbacks, no blending, no nudging.
/// Avoid: lookup, selector, mapping (a mapping says token→group; this says token→colour).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub enum Query {
    Highlight { group: String, attr: Attr },
    Global(String),
    Alias(String),
}

impl FromStr for Query {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        if let Some(token) = s.strip_prefix('=') {
            return if token.is_empty() {
                Err("`=` names no token".into())
            } else {
                Ok(Query::Alias(token.to_string()))
            };
        }
        if let Some(name) = s.strip_prefix("g:") {
            return if name.is_empty() {
                Err("`g:` names no variable".into())
            } else {
                Ok(Query::Global(name.to_string()))
            };
        }
        // A group name may itself contain dots (`@function.call`), so the attribute is whatever
        // follows the last one.
        let (group, attr) = s.rsplit_once('.').ok_or_else(|| {
            format!("`{s}` is not a query: expected Group.attr, g:name or =token")
        })?;
        let attr = match attr {
            "fg" => Attr::Fg,
            "bg" => Attr::Bg,
            "sp" => Attr::Sp,
            other => {
                return Err(format!(
                    "`{s}`: the attribute `{other}` must be fg, bg or sp"
                ))
            }
        };
        if group.is_empty() {
            return Err(format!("`{s}` names no highlight group"));
        }
        Ok(Query::Highlight {
            group: group.to_string(),
            attr,
        })
    }
}

impl TryFrom<String> for Query {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Query::Highlight { group, attr } => write!(f, "{group}.{}", attr.as_str()),
            Query::Global(name) => write!(f, "g:{name}"),
            Query::Alias(token) => write!(f, "={token}"),
        }
    }
}

#[derive(Clone)]
enum Entry {
    Same(Query),
    PerMode { dark: Query, light: Query },
}

impl Entry {
    /// One query, or a `{ dark, light }` pair. Parsed by hand, not through an untagged serde
    /// enum, because that would swallow the grammar error (`the attribute `bgg` must be fg, bg
    /// or sp`) behind "data did not match any variant".
    fn from_yaml(value: &serde_yaml::Value) -> Result<Self, String> {
        let query = |v: Option<&serde_yaml::Value>, what: &str| -> Result<Query, String> {
            v.and_then(serde_yaml::Value::as_str)
                .ok_or_else(|| format!("{what} is not a query string"))?
                .parse()
        };
        match value {
            serde_yaml::Value::String(s) => Ok(Entry::Same(s.parse()?)),
            serde_yaml::Value::Mapping(m) => Ok(Entry::PerMode {
                dark: query(m.get("dark"), "`dark`")?,
                light: query(m.get("light"), "`light`")?,
            }),
            _ => Err("expected a query or a { dark, light } pair".into()),
        }
    }
}

impl Entry {
    fn query(&self, mode: ThemeMode) -> &Query {
        match (self, mode) {
            (Entry::Same(q), _) => q,
            (Entry::PerMode { dark, .. }, ThemeMode::Dark) => dark,
            (Entry::PerMode { light, .. }, ThemeMode::Light) => light,
        }
    }
}

/// Token → query for one workload's overrides.
pub type Overrides = BTreeMap<String, Query>;

pub struct Table {
    /// In the order `queries.yaml` lists them, which is the order a generated theme lists them.
    entries: Vec<(String, Entry)>,
}

/// The mode a page colour is for: dark if it is closer in contrast to black than to white.
pub fn mode_of(page: &str) -> ThemeMode {
    let to = |other: &str| contrast::ratio(page, other).expect("a page colour parses");
    if to("#000000") < to("#ffffff") {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

impl Table {
    pub fn parse(yaml: &str) -> Result<Self, String> {
        let raw: serde_yaml::Mapping =
            serde_yaml::from_str(yaml).map_err(|e| format!("queries.yaml: {e}"))?;
        let mut entries = Vec::new();
        for (key, value) in raw {
            let token = key
                .as_str()
                .ok_or("queries.yaml: a token name is not a string")?
                .to_string();
            let entry =
                Entry::from_yaml(&value).map_err(|e| format!("queries.yaml: `{token}`: {e}"))?;
            entries.push((token, entry));
        }
        let table = Self { entries };
        let known: BTreeSet<&str> = CORE_TOKENS
            .iter()
            .chain(ANSI_TOKENS)
            .chain(OPTIONAL_TOKENS)
            .copied()
            .collect();
        for required in CORE_TOKENS.iter().chain(ANSI_TOKENS) {
            if table.entry(required).is_none() {
                return Err(format!("queries.yaml has no entry for `{required}`"));
            }
        }
        for (token, _) in &table.entries {
            if !known.contains(token.as_str()) {
                return Err(format!(
                    "queries.yaml names `{token}`, which is not a theme token"
                ));
            }
        }
        for (_, entry) in &table.entries {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                if let Query::Alias(target) = entry.query(mode) {
                    if table.entry(target).is_none() {
                        return Err(format!("`={target}` names no token"));
                    }
                }
            }
        }
        match table.entry("background") {
            Some(Entry::Same(q)) if !matches!(q, Query::Alias(_)) => {}
            _ => {
                return Err(
                    "`background` must be one query, not an alias or a per-mode pair".into(),
                )
            }
        }
        Ok(table)
    }

    fn entry(&self, token: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|(t, _)| t == token)
            .map(|(_, e)| e)
    }

    /// Every token, in file order.
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(t, _)| t.as_str())
    }

    /// The query whose answer is the page colour, from which the mode is decided.
    pub fn page(&self) -> String {
        self.entry("background")
            .expect("parse checked it")
            .query(ThemeMode::Dark)
            .to_string()
    }

    /// Refuse overrides for tokens the table does not have, and aliases to nowhere. Without this a
    /// typo'd token name changes nothing and the scheme just seems to fall short.
    pub fn check_overrides(&self, overrides: &Overrides) -> Result<(), String> {
        for (token, query) in overrides {
            if self.entry(token).is_none() {
                return Err(format!(
                    "override for `{token}`, which is not a token in the table"
                ));
            }
            if let Query::Alias(target) = query {
                if self.entry(target).is_none() {
                    return Err(format!("override `{token}: ={target}` names no token"));
                }
            }
        }
        Ok(())
    }

    /// Every question worth asking for a workload with these overrides: both columns of every
    /// entry that is not an alias, and each override that is not one.
    pub fn questions(&self, overrides: &Overrides) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (token, entry) in &self.entries {
            let queries: Vec<&Query> = match overrides.get(token) {
                Some(q) => vec![q],
                None => [ThemeMode::Dark, ThemeMode::Light]
                    .into_iter()
                    .map(|m| entry.query(m))
                    .collect(),
            };
            for q in queries {
                if !matches!(q, Query::Alias(_)) {
                    out.insert(q.to_string());
                }
            }
        }
        out
    }

    /// Each token's colour for `mode`, or `None` where its question got no answer.
    pub fn resolve(
        &self,
        mode: ThemeMode,
        overrides: &Overrides,
        answers: &BTreeMap<String, String>,
    ) -> BTreeMap<String, Option<String>> {
        self.tokens()
            .map(|token| {
                (
                    token.to_string(),
                    self.colour(token, mode, overrides, answers, 0),
                )
            })
            .collect()
    }

    fn colour(
        &self,
        token: &str,
        mode: ThemeMode,
        overrides: &Overrides,
        answers: &BTreeMap<String, String>,
        depth: usize,
    ) -> Option<String> {
        assert!(depth < 8, "alias cycle at `{token}`");
        let query = match overrides.get(token) {
            Some(q) => q,
            None => self.entry(token)?.query(mode),
        };
        match query {
            Query::Alias(target) => self.colour(target, mode, overrides, answers, depth + 1),
            other => answers.get(&other.to_string()).cloned(),
        }
    }
}

/// Run `cmd` and return its stdout, panicking with its stderr if it fails.
pub fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// `answers` if every value is a CSS colour, else the first that is not, by query.
pub fn only_colours(answers: BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    for (query, value) in &answers {
        if color::parse_color(value).is_err() {
            return Err(format!(
                "`{query}` answered `{value}`, which is not a colour"
            ));
        }
    }
    Ok(answers)
}

/// Load `colorscheme` in a pristine Neovim (after running `setup`, if any) and answer `questions`.
pub fn ask(
    query_lua: &Path,
    rtp: &str,
    colorscheme: &str,
    background: &str,
    setup: Option<&str>,
    questions: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    let out = Command::new("nvim")
        .args(["--headless", "--clean", "-l"])
        .arg(query_lua)
        .env("RTP", rtp)
        .env("CS", colorscheme)
        .env("BGMODE", background)
        .env("SETUP", setup.unwrap_or(""))
        .env(
            "QUERIES",
            serde_json::to_string(questions).expect("serialise questions"),
        )
        .output()
        .map_err(|e| format!("spawn nvim: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(msg
            .lines()
            .next()
            .unwrap_or("nvim failed")
            .trim()
            .to_string());
    }
    let answers = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("query.lua printed no JSON: {e}"))?;
    only_colours(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const REAL: &str = include_str!("../../src/themes/generate/queries.yaml");

    fn table() -> Table {
        Table::parse(REAL).expect("queries.yaml is a valid table")
    }

    fn answers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn none() -> Overrides {
        Overrides::new()
    }

    /// `(alias token, target token)` for every `=token` in the real table, in either column.
    fn aliases() -> Vec<(String, String)> {
        let t = table();
        let mut out = Vec::new();
        for (token, entry) in &t.entries {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                if let Query::Alias(target) = entry.query(mode) {
                    out.push((token.clone(), target.clone()));
                }
            }
        }
        out
    }

    #[test]
    fn the_real_table_is_valid_and_lists_tokens_in_file_order() {
        let t = table();
        assert_eq!(t.tokens().next(), Some("background"));
        assert_eq!(t.tokens().last(), Some("ansi-bright-white"));
    }

    #[test]
    fn a_missing_required_token_is_named() {
        let yaml = REAL.replace("background: Normal.bg\n", "");
        let err = Table::parse(&yaml).err().expect("must be refused");
        assert!(err.contains("`background`"), "{err}");
    }

    #[test]
    fn a_missing_optional_token_is_fine() {
        let yaml = REAL.replace("chart-5: g:terminal_color_5\n", "");
        assert!(Table::parse(&yaml).is_ok());
    }

    #[test]
    fn an_unknown_token_is_named() {
        let yaml = format!("{REAL}\nbogus: Normal.fg\n");
        let err = Table::parse(&yaml).err().expect("must be refused");
        assert!(err.contains("`bogus`"), "{err}");
    }

    #[test]
    fn an_alias_to_nothing_is_refused() {
        let yaml = REAL.replace("popover: =card", "popover: =nowhere");
        let err = Table::parse(&yaml).err().expect("must be refused");
        assert!(err.contains("`=nowhere`"), "{err}");
    }

    #[test]
    fn a_background_that_depends_on_the_mode_is_refused() {
        let yaml = REAL.replace(
            "background: Normal.bg\n",
            "background: { dark: Normal.bg, light: NormalFloat.bg }\n",
        );
        let err = Table::parse(&yaml).err().expect("must be refused");
        assert!(err.contains("`background`"), "{err}");
    }

    #[test]
    fn the_page_is_asked_for_and_is_the_same_in_both_modes() {
        let t = table();
        assert!(t.questions(&none()).contains(&t.page()));
    }

    #[test]
    fn the_dark_column_is_read_for_dark_and_the_light_one_for_light() {
        let a = answers(&[("Pmenu.bg", "#111111"), ("CursorColumn.bg", "#eeeeee")]);
        let t = table();
        assert_eq!(
            t.resolve(ThemeMode::Dark, &none(), &a)["card"].as_deref(),
            Some("#111111")
        );
        assert_eq!(
            t.resolve(ThemeMode::Light, &none(), &a)["card"].as_deref(),
            Some("#eeeeee")
        );
    }

    #[test]
    fn an_override_replaces_the_query_for_both_modes_and_reaches_its_aliases() {
        let a = answers(&[("Pmenu.bg", "#111111"), ("NormalFloat.bg", "#222222")]);
        let ov: Overrides = [("card".to_string(), "NormalFloat.bg".parse().unwrap())].into();
        let r = table().resolve(ThemeMode::Dark, &ov, &a);
        assert_eq!(r["card"].as_deref(), Some("#222222"));
        assert_eq!(r["sidebar"].as_deref(), Some("#222222"));
    }

    #[test]
    fn an_override_is_asked_for_and_the_query_it_replaced_is_not() {
        let ov: Overrides = [("border".to_string(), "Conceal.fg".parse().unwrap())].into();
        let asked = table().questions(&ov);
        assert!(asked.contains("Conceal.fg"));
        assert!(!asked.contains("Whitespace.fg"));
    }

    #[test]
    fn an_unanswered_question_leaves_its_token_empty_and_its_aliases_with_it() {
        let r = table().resolve(ThemeMode::Dark, &none(), &answers(&[]));
        assert_eq!(r["card"], None);
        assert_eq!(r["popover"], None);
    }

    #[test]
    fn the_mode_is_read_from_the_page_colour() {
        assert_eq!(mode_of("#1a1b26"), ThemeMode::Dark);
        assert_eq!(mode_of("#e1e2e7"), ThemeMode::Light);
        assert_eq!(mode_of("#000000"), ThemeMode::Dark);
        assert_eq!(mode_of("#ffffff"), ThemeMode::Light);
    }

    #[test]
    fn an_attribute_outside_fg_bg_sp_is_refused_with_the_attribute_named() {
        let err = "Normal.bgg".parse::<Query>().unwrap_err();
        assert!(
            err.contains("`bgg`") && err.contains("fg, bg or sp"),
            "{err}"
        );
    }

    #[test]
    fn a_query_in_no_form_is_refused() {
        for bad in ["", "Normal", ".fg", "g:", "=", "Normal.", "Normal.fg.extra"] {
            assert!(bad.parse::<Query>().is_err(), "`{bad}` was accepted");
        }
    }

    #[test]
    fn a_group_name_may_contain_dots() {
        let q: Query = "@function.call.fg".parse().unwrap();
        assert_eq!(
            q,
            Query::Highlight {
                group: "@function.call".into(),
                attr: Attr::Fg
            }
        );
    }

    #[test]
    fn a_typo_in_the_table_is_refused_where_the_file_is_read() {
        let yaml = REAL.replace("accent: Visual.bg", "accent: Visual.bgg");
        let err = Table::parse(&yaml).err().expect("must be refused");
        assert!(err.contains("`accent`") && err.contains("`bgg`"), "{err}");
    }

    #[test]
    fn an_override_for_a_token_the_table_lacks_is_refused_by_name() {
        let ov: Overrides = [("muted-foregrond".to_string(), "Conceal.fg".parse().unwrap())].into();
        let err = table().check_overrides(&ov).unwrap_err();
        assert!(err.contains("`muted-foregrond`"), "{err}");
    }

    #[test]
    fn an_override_alias_to_nowhere_is_refused() {
        let ov: Overrides = [("card".to_string(), "=nowhere".parse().unwrap())].into();
        let err = table().check_overrides(&ov).unwrap_err();
        assert!(err.contains("=nowhere"), "{err}");
    }

    #[test]
    fn a_real_override_is_accepted() {
        let ov: Overrides = [(
            "muted-foreground".to_string(),
            "Conceal.fg".parse().unwrap(),
        )]
        .into();
        assert!(table().check_overrides(&ov).is_ok());
    }

    #[test]
    fn an_answer_that_is_not_a_colour_is_refused_naming_query_and_value() {
        let err = only_colours(answers(&[
            ("Normal.bg", "#1a1b26"),
            ("g:terminal_color_1", "banana"),
        ]))
        .unwrap_err();
        assert!(
            err.contains("g:terminal_color_1") && err.contains("banana"),
            "{err}"
        );
    }

    #[test]
    fn hex_answers_with_and_without_alpha_are_colours() {
        assert!(only_colours(answers(&[("a.fg", "#fff"), ("b.bg", "#1a1b26e6")])).is_ok());
    }

    fn hex_colour() -> impl Strategy<Value = String> {
        (0u32..=0xff_ffff).prop_map(|n| format!("#{n:06x}"))
    }

    /// Random answers to a random subset of the real table's questions.
    fn some_answers() -> impl Strategy<Value = BTreeMap<String, String>> {
        let questions: Vec<String> = table().questions(&none()).into_iter().collect();
        proptest::collection::btree_map(proptest::sample::select(questions), hex_colour(), 0..80)
    }

    proptest! {
        /// However the scheme answers, an alias has exactly its target's colour, in both modes.
        #[test]
        fn an_alias_always_equals_its_target(a in some_answers()) {
            let t = table();
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                let r = t.resolve(mode, &none(), &a);
                for (alias, target) in aliases() {
                    prop_assert_eq!(&r[&alias], &r[&target], "{} vs {}", alias, target);
                }
            }
        }

        /// A token is answered exactly when its (non-alias) query is, and then with that answer.
        #[test]
        fn a_token_is_the_answer_to_its_own_query(a in some_answers()) {
            let t = table();
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                let r = t.resolve(mode, &none(), &a);
                for (token, entry) in &t.entries {
                    let query = entry.query(mode);
                    if !matches!(query, Query::Alias(_)) {
                        prop_assert_eq!(&r[token], &a.get(&query.to_string()).cloned(), "{}", token);
                    }
                }
            }
        }

        /// A per-mode token takes the dark answer in a dark theme and the light one in a light
        /// theme, whatever the colours are.
        #[test]
        fn the_mode_column_is_the_one_read(dark in hex_colour(), light in hex_colour()) {
            let a = answers(&[("Pmenu.bg", &dark), ("CursorColumn.bg", &light)]);
            let t = table();
            let as_dark = t.resolve(ThemeMode::Dark, &none(), &a)["card"].clone();
            let as_light = t.resolve(ThemeMode::Light, &none(), &a)["card"].clone();
            prop_assert_eq!(as_dark, Some(dark));
            prop_assert_eq!(as_light, Some(light));
        }

        /// Removing any one required token from the table is always refused, by name.
        #[test]
        fn a_table_missing_any_required_token_is_refused(i in 0usize..(CORE_TOKENS.len() + ANSI_TOKENS.len())) {
            let token = CORE_TOKENS.iter().chain(ANSI_TOKENS).nth(i).unwrap();
            let yaml: String = REAL
                .lines()
                .filter(|l| !l.starts_with(&format!("{token}:")))
                .map(|l| format!("{l}\n"))
                .collect();
            let err = Table::parse(&yaml).err().expect("must be refused");
            prop_assert!(err.contains(&format!("`{token}`")), "{}", err);
        }

        /// Every query the grammar accepts prints back as exactly what was parsed.
        #[test]
        fn a_query_prints_as_it_was_written(
            group in "[A-Za-z@_][A-Za-z0-9@_.]{0,20}",
            attr in proptest::sample::select(vec!["fg", "bg", "sp"]),
            var in "[a-z_]{1,16}",
            token in "[a-z][a-z-]{0,16}",
        ) {
            for text in [format!("{group}.{attr}"), format!("g:{var}"), format!("={token}")] {
                let q: Query = text.parse().unwrap();
                prop_assert_eq!(q.to_string(), text);
            }
        }

        /// The mode a page colour is for never changes with how it is written.
        #[test]
        fn the_mode_is_a_function_of_the_colour(c in hex_colour()) {
            prop_assert_eq!(mode_of(&c), mode_of(&c.to_uppercase()));
        }
    }
}
