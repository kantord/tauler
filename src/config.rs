use serde::Deserialize;
use crate::theme::ThemeMode;

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
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CostaeConfig {
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub fonts: FontConfig,
}

impl CostaeConfig {
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
        let config = CostaeConfig::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(config.theme.mode, ThemeMode::Light);
    }

    #[test]
    fn config_from_yaml_defaults_to_dark_when_theme_absent() {
        let config = CostaeConfig::from_yaml("").expect("empty yaml should parse");
        assert_eq!(config.theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn config_from_yaml_parses_optional_theme_file() {
        let yaml = "theme:\n  mode: dark\n  file: ~/.config/costae/my-theme.yaml";
        let config = CostaeConfig::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(config.theme.file.as_deref(), Some("~/.config/costae/my-theme.yaml"));
    }
}
