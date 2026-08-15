use crate::theme::ThemeMode;
use serde::Deserialize;

/// Expand a leading `~/` in a path the user wrote — in `config.yaml`, or in a
/// layout file's `bin`.
///
/// Lives here rather than next to either caller because both are the same
/// thing: a path supplied as configuration, which a person expects to be able
/// to write the way they write it in a shell.
///
/// `pub`, not `pub(crate)`: `src/app.rs` is a module of the *binary*, not of
/// the library, so it reaches this through `tauler::config` like any outside
/// caller would.
///
/// Only `~/` is expanded. A bare name is returned unchanged so that
/// `Command::new` still searches `PATH`, and `~user` is left alone rather than
/// guessed at.
pub fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        std::path::PathBuf::from(home).join(rest)
    } else {
        std::path::PathBuf::from(path)
    }
}

fn default_theme_mode() -> ThemeMode {
    ThemeMode::Dark
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeConfig {
    #[serde(default = "default_theme_mode")]
    pub mode: ThemeMode,
    #[serde(default)]
    pub file: Option<String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            file: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FontConfig {
    pub primary: Option<String>,
    pub primary_path: Option<std::path::PathBuf>,
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaulerConfig {
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub fonts: FontConfig,
}

impl TaulerConfig {
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    /// `HOME` is process-wide, so these three assertions share one guarded
    /// setting of it rather than racing each other across test threads.
    #[test]
    fn expand_tilde_expands_only_a_leading_home_slash() {
        // SAFETY: single-threaded within this test, and no other test in this
        // module reads HOME.
        unsafe { std::env::set_var("HOME", "/home/someone") };

        assert_eq!(
            expand_tilde("~/.local/bin/status"),
            std::path::PathBuf::from("/home/someone/.local/bin/status")
        );

        // A bare name must survive untouched, or `Command::new` stops finding
        // anything on PATH.
        assert_eq!(
            expand_tilde("tauler-i3"),
            std::path::PathBuf::from("tauler-i3")
        );

        // `~user` is somebody else's home and we do not guess at it.
        assert_eq!(
            expand_tilde("~other/bin/x"),
            std::path::PathBuf::from("~other/bin/x")
        );
    }

    #[test]
    fn config_from_yaml_parses_light_theme_mode() {
        let yaml = "theme:\n  mode: light";
        let config = TaulerConfig::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(config.theme.mode, ThemeMode::Light);
    }

    #[test]
    fn config_from_yaml_defaults_to_dark_when_theme_absent() {
        let config = TaulerConfig::from_yaml("").expect("empty yaml should parse");
        assert_eq!(config.theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn config_from_yaml_parses_optional_theme_file() {
        let yaml = "theme:\n  mode: dark\n  file: ~/.config/tauler/my-theme.yaml";
        let config = TaulerConfig::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(
            config.theme.file.as_deref(),
            Some("~/.config/tauler/my-theme.yaml")
        );
    }
}
