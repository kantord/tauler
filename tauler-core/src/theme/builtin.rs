//! The themes compiled into the binary, chosen by `theme.name`.
//!
//! TERM: **Built-in theme** — a theme whose YAML is embedded in the binary and selected by
//! its own `name`, as opposed to one a user points `theme.file` at.
//! Avoid: preset, bundled theme, stock theme.
//!
//! DECISION: the registry is a list of embedded files, not a `(name, file)` table. The name
//! lives inside the theme (`Theme::name`), so there is one place to rename it and no second
//! list to drift from the first. Rejected: a `match name { "tokyonight" => ... }` — a
//! theme added to the list but not the match would load and never be selectable.
//!
//! SPEC: every built-in is complete (`Theme::missing_tokens` empty), clears the hard contrast
//! floor, and has a unique name — each tested below.
//!
//! DECISION: the built-ins are parsed on every lookup. They are a few KB each and the
//! lookup happens at startup and on a config reload, so this is not worth a cache.

use super::Theme;

const SOURCES: &[&str] = &[
    include_str!("../themes/default-dark.yaml"),
    include_str!("../themes/default-light.yaml"),
    include_str!("../themes/tokyonight-night/theme.yaml"),
    include_str!("../themes/tokyonight-storm/theme.yaml"),
    include_str!("../themes/tokyonight-moon/theme.yaml"),
    include_str!("../themes/tokyonight-day/theme.yaml"),
];

/// A `theme.name` that matches no built-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTheme {
    pub name: String,
    pub known: Vec<String>,
}

impl std::fmt::Display for UnknownTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no built-in theme named `{}` (known: {})",
            self.name,
            self.known.join(", ")
        )
    }
}

impl std::error::Error for UnknownTheme {}

fn all() -> Vec<Theme> {
    SOURCES
        .iter()
        .map(|yaml| {
            Theme::from_yaml(yaml)
                .expect("embedded built-in theme is invalid")
                .complete()
                .expect("embedded built-in theme is incomplete")
        })
        .collect()
}

pub fn names() -> Vec<String> {
    all().into_iter().map(|t| t.name).collect()
}

/// The shipped default for `mode` — what a config that names no theme gets.
///
/// DECISION: the default is a pair of themes (`default-dark`, `default-light`), not one theme
/// with two palettes, like every other built-in. `theme.mode` in a config chooses between them
/// when no theme is named, so a config written before themes carried a mode keeps meaning what
/// it said.
pub fn default_for(mode: super::ThemeMode) -> Theme {
    let name = match mode {
        super::ThemeMode::Dark => "default-dark",
        super::ThemeMode::Light => "default-light",
    };
    by_name(name).expect("the shipped defaults are built in")
}

pub fn by_name(name: &str) -> Result<Theme, UnknownTheme> {
    let themes = all();
    let known = themes.iter().map(|t| t.name.clone()).collect();
    themes
        .into_iter()
        .find(|t| t.name == name)
        .ok_or(UnknownTheme {
            name: name.to_string(),
            known,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_built_in_is_found_by_the_name_written_inside_it() {
        assert_eq!(
            by_name("tokyonight-night").unwrap().name,
            "tokyonight-night"
        );
    }

    #[test]
    fn an_unknown_name_lists_the_known_ones() {
        let err = by_name("nope").unwrap_err();
        assert!(err.to_string().contains("tokyonight-night"), "{err}");
    }

    #[test]
    fn each_default_is_for_its_own_mode() {
        use crate::theme::ThemeMode;
        assert_eq!(default_for(ThemeMode::Dark).mode, ThemeMode::Dark);
        assert_eq!(default_for(ThemeMode::Light).mode, ThemeMode::Light);
    }

    #[test]
    fn built_in_names_are_unique() {
        let mut names = names();
        let before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), before);
    }

    /// `all()` already panics on an incomplete built-in, so listing the names is the check.
    #[test]
    fn every_built_in_is_complete() {
        assert_eq!(names().len(), SOURCES.len());
    }

    #[test]
    fn every_built_in_clears_the_hard_contrast_floor() {
        for name in names() {
            let theme = by_name(&name).unwrap();
            let failures =
                crate::theme::contrast::failures(&theme, crate::theme::contrast::HARD_FLOOR);
            assert!(failures.is_empty(), "{name}: {failures:?}");
        }
    }
}
