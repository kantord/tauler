use crate::theme::ThemeMode;
use serde::Deserialize;

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

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenderingConfig {
    #[serde(default = "default_true")]
    pub incremental: bool,
}

impl Default for RenderingConfig {
    fn default() -> Self {
        Self { incremental: true }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaulerConfig {
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub fonts: FontConfig,
    #[serde(default)]
    pub rendering: RenderingConfig,
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
