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
