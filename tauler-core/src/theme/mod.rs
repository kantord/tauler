use serde::Deserialize;
use std::collections::HashMap;

pub mod builtin;
pub mod contrast;
pub mod resolver;
pub mod swatch;

const DEFAULT_DARK: &str = include_str!("../themes/default-dark.yaml");

/// TERM: **Core token** — a colour name every theme must define, because
/// something in tauler (a component, or the docs' own examples) reads it.
/// Avoid: required colour, mandatory key.
///
/// DECISION: the core set is shadcn's 19 minimal tokens (20 with `destructive-foreground`, which `Badge` reads), not the 32 the shipped default
/// carries. `chart-*` and `sidebar-*` are optional: they are shadcn extras, no tauler
/// component reads them, and a missing optional token is inert, not wrong. Built-in themes
/// define all 32 anyway (the generator fills them) so a user copying one starts complete.
/// Rejected: requiring all 32 — 13 of them have no consumer, and a requirement nothing
/// reads is a chore for a theme author, not a guarantee.
///
/// DECISION: the 20 stay the required set even though `popover*`, `input`, `ring` and
/// `accent-foreground` have no consumer today — compatibility with shadcn-shaped themes is the
/// point of borrowing its vocabulary.
pub const CORE_TOKENS: &[&str] = &[
    "background",
    "foreground",
    "card",
    "card-foreground",
    "popover",
    "popover-foreground",
    "primary",
    "primary-foreground",
    "secondary",
    "secondary-foreground",
    "muted",
    "muted-foreground",
    "accent",
    "accent-foreground",
    "destructive",
    // Not in shadcn's current minimal set, but `Badge` (variant="destructive") draws
    // `text-destructive-foreground`, so it has a consumer today. Found by rendering the
    // swatch scene: the class survived token resolution unresolved.
    "destructive-foreground",
    "border",
    "input",
    "ring",
];

/// TERM: **ANSI token** — one of the 16 terminal colours, `ansi-<name>` and
/// `ansi-bright-<name>`, so a theme can drive a terminal's palette as well as the bar's.
/// Avoid: terminal colour, term color (ambiguous with a *terminal* the bar is not).
///
/// DECISION: the `ansi-` prefix is mandatory. A bare `red` or `white` key would be rewritten
/// by the resolver and shadow Tailwind's own `bg-white`.
///
/// DECISION: the spelling is `ansi-*`, not `term-*` or `terminal-*` — it names the standard
/// the 16 colours come from, and it is hard to change once users have written it into
/// theme files.
pub const ANSI_TOKENS: &[&str] = &[
    "ansi-black",
    "ansi-red",
    "ansi-green",
    "ansi-yellow",
    "ansi-blue",
    "ansi-magenta",
    "ansi-cyan",
    "ansi-white",
    "ansi-bright-black",
    "ansi-bright-red",
    "ansi-bright-green",
    "ansi-bright-yellow",
    "ansi-bright-blue",
    "ansi-bright-magenta",
    "ansi-bright-cyan",
    "ansi-bright-white",
];

/// Whether a theme is for a dark or a light background. A label the theme carries, not a switch
/// the user flips: see [`Theme`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
}

/// TERM: **Theme** — one named palette for one appearance: a colour map, plus radius and font
/// roles, and a label (`mode`) saying whether it is for a dark or a light background.
/// Avoid: skin, colour scheme (Neovim's word; a scheme is one input to a theme).
///
/// DECISION: one theme is one mode, labelled in the file. A scheme that comes in a dark and a
/// light style is two themes (`tokyonight-night`, `tokyonight-day`). Vim's single `background`
/// option made one scheme file branch on it, and a scheme loaded under the wrong background was
/// the usual failure; VS Code (`uiTheme`) and tinted-theming (`variant`) both ship one theme per
/// appearance with a label instead. It also means a theme and its mode cannot disagree, and a
/// theme cannot be "missing" the palette for the mode it is shown in.
/// Rejected: `colors.light` + `colors.dark` in one file, which is what this replaced.
///
/// DECISION: `name` is required, not optional. It is how a built-in is selected
/// (`theme.name`) and how an error says which theme it means; a file without one is a
/// load error. Rejected: `Option<String>` — one more branch in every message, for the sake
/// of not editing a file that has to be edited for the core-token rule anyway.
#[derive(Debug, Clone, Deserialize)]
pub struct Theme {
    pub name: String,
    pub mode: ThemeMode,
    pub colors: HashMap<String, String>,
    #[serde(default)]
    pub radius: HashMap<String, String>,
    #[serde(default)]
    pub fonts: HashMap<String, String>,
}

/// A theme that does not define every token it must.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteTheme {
    pub name: String,
    pub missing: Vec<&'static str>,
}

impl std::fmt::Display for IncompleteTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "theme `{}` is missing: {}",
            self.name,
            self.missing.join(", ")
        )
    }
}

impl std::error::Error for IncompleteTheme {}

impl Theme {
    /// The shipped default for dark backgrounds, used when a config names no theme.
    pub fn default_theme() -> Self {
        serde_yaml::from_str::<Self>(DEFAULT_DARK)
            .expect("embedded default theme is invalid")
            .complete()
            .expect("embedded default theme is incomplete")
    }

    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    /// Every core and ANSI token this theme leaves undefined.
    ///
    /// SPEC: empty means the theme is complete. Order is stable (the order of the token
    /// lists above) so a message built from it does not shuffle between runs.
    ///
    /// DECISION: enforced everywhere a theme is loaded — a `theme.file`, a built-in, the
    /// embedded default — through [`Theme::complete`], under the ADR 0033 policy (startup
    /// exits, a reload keeps the theme on screen). Rejected: enforcing only on built-ins,
    /// which leaves the silent unstyled bar for every user-written theme.
    pub fn missing_tokens(&self) -> Vec<&'static str> {
        CORE_TOKENS
            .iter()
            .chain(ANSI_TOKENS)
            .filter(|t| !self.colors.contains_key(**t))
            .copied()
            .collect()
    }

    /// This theme if it defines every core and ANSI token, else why not.
    pub fn complete(self) -> Result<Self, IncompleteTheme> {
        let missing = self.missing_tokens();
        if missing.is_empty() {
            Ok(self)
        } else {
            Err(IncompleteTheme {
                name: self.name,
                missing,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_missing_tokens_says_which() {
        let theme =
            Theme::from_yaml("name: t\nmode: dark\ncolors:\n  background: \"#000\"\n").unwrap();
        let err = theme.complete().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("theme `t`"), "{msg}");
        assert!(msg.contains("foreground"), "{msg}");
        assert!(!msg.contains("missing: background"), "{msg}");
    }

    #[test]
    fn a_theme_without_a_mode_does_not_parse() {
        assert!(Theme::from_yaml("name: t\ncolors: {}\n").is_err());
    }

    #[test]
    fn the_mode_label_is_read_from_the_file() {
        let theme = Theme::from_yaml("name: t\nmode: light\ncolors: {}\n").unwrap();
        assert_eq!(theme.mode, ThemeMode::Light);
    }

    #[test]
    fn theme_from_yaml_defaults_fonts_to_empty_when_absent() {
        let yaml = "name: t\nmode: dark\ncolors: {}";
        let theme = Theme::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(theme.fonts, HashMap::new());
    }

    #[test]
    fn theme_from_yaml_parses_fonts() {
        let yaml = "name: t\nmode: dark\ncolors: {}\nfonts:\n  heading: \"Lora\"\n  base: \"Inter Variable\"";
        let theme = Theme::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(theme.fonts.get("heading").map(String::as_str), Some("Lora"));
        assert_eq!(
            theme.fonts.get("base").map(String::as_str),
            Some("Inter Variable")
        );
    }
}
