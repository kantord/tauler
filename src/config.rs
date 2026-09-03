use crate::theme::ThemeMode;
use serde::Deserialize;

/// Expand a leading `~/` in a path the user wrote — in a layout file's frontmatter (or,
/// on the legacy path, `config.yaml`), or in a layout file's `bin`.
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
    pub primary_path: Option<String>,
    pub emoji: Option<String>,
    #[serde(default)]
    pub extra: Vec<ExtraFont>,
    /// A symbol font named by file, registered as the last-resort fallback
    /// instead of asking fontconfig for one. Not readable from the layout
    /// file's frontmatter: it exists for callers that need a host-independent
    /// render (tauler-screenshot).
    #[serde(skip)]
    pub symbol_path: Option<std::path::PathBuf>,
    /// Register only the fonts this config names by path (`primary_path`,
    /// `extra` paths, `symbol_path`) and never consult fontconfig or walk the
    /// system font directories. Not readable from the layout file's
    /// frontmatter: it exists for renders that must be identical on every host
    /// (tauler-screenshot's docs baselines).
    #[serde(skip)]
    pub files_only: bool,
}

/// One entry in `fonts.extra`: either a plain font-family name looked up on
/// the system, or an explicit `path:` to a font file — raw and unexpanded,
/// same as `primary_path`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ExtraFont {
    Name(String),
    Path { path: String },
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

/// `HOME` is a process-wide env var, and three tests across this crate (this
/// one, plus two in `render::tests`) mutate it. `cargo test` runs the whole
/// binary's tests in parallel threads by default, so without serialization
/// those three race each other and see torn/overwritten values. Every test
/// that touches `HOME` must hold this lock for the entire time `HOME`'s value
/// matters to it.
#[cfg(test)]
pub(crate) static HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    #[test]
    fn expand_tilde_expands_only_a_leading_home_slash() {
        // SAFETY: HOME is process-wide and this test mutates it, but
        // HOME_ENV_LOCK serializes every test in the crate that touches HOME,
        // so no other thread can observe or clobber it while this guard is held.
        let _guard = HOME_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn config_from_yaml_defaults_fonts_extra_to_empty_when_absent() {
        let config = TaulerConfig::from_yaml("").expect("empty yaml should parse");
        assert_eq!(config.fonts.extra, Vec::new());
    }

    #[test]
    fn config_from_yaml_parses_fonts_extra_plain_string_names() {
        let yaml = "fonts:\n  extra:\n    - Lora\n    - JetBrains Mono";
        let config = TaulerConfig::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(
            config.fonts.extra,
            vec![
                ExtraFont::Name("Lora".to_string()),
                ExtraFont::Name("JetBrains Mono".to_string()),
            ]
        );
    }

    #[test]
    fn config_from_yaml_parses_fonts_extra_path_object_without_expanding() {
        let yaml = "fonts:\n  extra:\n    - path: ~/.fonts/MyIconFont.ttf";
        let config = TaulerConfig::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(
            config.fonts.extra,
            vec![ExtraFont::Path {
                path: "~/.fonts/MyIconFont.ttf".to_string()
            }]
        );
    }

    #[test]
    fn config_from_yaml_parses_primary_path_as_raw_unexpanded_string() {
        let yaml = "fonts:\n  primary_path: ~/.fonts/Custom.ttf";
        let config = TaulerConfig::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(
            config.fonts.primary_path,
            Some("~/.fonts/Custom.ttf".to_string())
        );
    }
}
