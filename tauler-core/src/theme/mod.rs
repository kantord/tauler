use serde::Deserialize;
use std::collections::HashMap;

pub mod resolver;

const DEFAULT_THEME: &str = include_str!("../themes/default.yaml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    /// Deferred — treated as Dark until OS integration is wired up.
    Auto,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeColors {
    #[serde(default)]
    pub light: HashMap<String, String>,
    #[serde(default)]
    pub dark: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Theme {
    pub colors: ThemeColors,
    #[serde(default)]
    pub radius: HashMap<String, String>,
    #[serde(default)]
    pub fonts: HashMap<String, String>,
}

impl Theme {
    pub fn default_theme() -> Self {
        serde_yaml::from_str(DEFAULT_THEME).expect("embedded default theme is invalid")
    }

    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    pub fn colors_for_mode(&self, mode: ThemeMode) -> &HashMap<String, String> {
        match mode {
            ThemeMode::Light => &self.colors.light,
            ThemeMode::Dark | ThemeMode::Auto => &self.colors.dark,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_from_yaml_defaults_fonts_to_empty_when_absent() {
        let yaml = "colors:\n  light: {}\n  dark: {}";
        let theme = Theme::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(theme.fonts, HashMap::new());
    }

    #[test]
    fn theme_from_yaml_parses_fonts() {
        let yaml = "colors:\n  light: {}\n  dark: {}\nfonts:\n  heading: \"Lora\"\n  base: \"Inter Variable\"";
        let theme = Theme::from_yaml(yaml).expect("valid yaml should parse");
        assert_eq!(theme.fonts.get("heading").map(String::as_str), Some("Lora"));
        assert_eq!(
            theme.fonts.get("base").map(String::as_str),
            Some("Inter Variable")
        );
    }
}
