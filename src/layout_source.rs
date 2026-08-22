//! Which files a bar's layout is made of, and how to load them into a
//! [`crate::config::TaulerConfig`] plus a JS source string ready for
//! [`crate::jsx::JsxEvaluator`].
//!
//! Two coexisting formats, chosen once at startup and never re-detected — see
//! `docs/adr/0036-the-layout-files-config-moves-into-its-own-frontmatter.md`
//! for the full rationale:
//!
//! - `layout.op.mdx`: a single file, YAML frontmatter for `theme`/`fonts`
//!   above a JSX body, lowered by `optative_script_mdx::lower::lower_to_tsx_with_frontmatter`.
//! - the legacy pair: `layout.jsx` (raw JSX, used verbatim) plus a sibling
//!   `config.yaml` (parsed by [`crate::config::TaulerConfig::from_yaml`]).

use crate::config::TaulerConfig;

/// Which files a bar's layout is made of — the new layout.op.mdx path, or the legacy
/// layout.jsx + config.yaml pair. See docs/adr/0036.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutSource {
    Mdx(std::path::PathBuf),
    Legacy {
        layout: std::path::PathBuf,
        config: std::path::PathBuf,
    },
}

impl LayoutSource {
    /// `layout.op.mdx` in `config_dir` if it exists, else the legacy pair if `layout.jsx`
    /// exists, else `None` (nothing configured at all). Checked once; never re-detected.
    pub fn detect(config_dir: &std::path::Path) -> Option<Self> {
        let mdx_path = config_dir.join("layout.op.mdx");
        if mdx_path.exists() {
            return Some(Self::Mdx(mdx_path));
        }
        let layout_path = config_dir.join("layout.jsx");
        if layout_path.exists() {
            return Some(Self::Legacy {
                layout: layout_path,
                config: config_dir.join("config.yaml"),
            });
        }
        None
    }
}

/// Why loading a `LayoutSource` failed.
#[derive(Debug)]
pub enum LayoutLoadError {
    Read {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    Lower {
        path: std::path::PathBuf,
        source: optative_script_mdx::lower::LowerError,
    },
    Config {
        path: std::path::PathBuf,
        source: serde_yaml::Error,
    },
}

impl std::fmt::Display for LayoutLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "cannot read layout file {}: {source}", path.display())
            }
            Self::Lower { path, source } => {
                write!(f, "cannot lower layout file {}: {source}", path.display())
            }
            Self::Config { path, source } => {
                write!(f, "invalid config YAML in {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LayoutLoadError {}

#[derive(Debug)]
pub struct LoadedLayout {
    pub config: TaulerConfig,
    pub js_source: String,
}

/// Loads the config and the JS source to feed into `JsxEvaluator`, from whichever format
/// `source` names.
pub fn load(source: &LayoutSource) -> Result<LoadedLayout, LayoutLoadError> {
    match source {
        LayoutSource::Mdx(path) => {
            let raw = std::fs::read_to_string(path).map_err(|source| LayoutLoadError::Read {
                path: path.clone(),
                source,
            })?;
            let (frontmatter, js_source) =
                optative_script_mdx::lower::lower_to_tsx_with_frontmatter(
                    &raw,
                    &path.to_string_lossy(),
                )
                .map_err(|source| LayoutLoadError::Lower {
                    path: path.clone(),
                    source,
                })?;
            let config = match frontmatter {
                Some(yaml_text) => TaulerConfig::from_yaml(&yaml_text).map_err(|source| {
                    LayoutLoadError::Config {
                        path: path.clone(),
                        source,
                    }
                })?,
                None => TaulerConfig::default(),
            };
            Ok(LoadedLayout { config, js_source })
        }
        LayoutSource::Legacy { layout, config } => {
            let js_source =
                std::fs::read_to_string(layout).map_err(|source| LayoutLoadError::Read {
                    path: layout.clone(),
                    source,
                })?;
            let config = if config.exists() {
                let yaml_text =
                    std::fs::read_to_string(config).map_err(|source| LayoutLoadError::Read {
                        path: config.clone(),
                        source,
                    })?;
                TaulerConfig::from_yaml(&yaml_text).map_err(|source| LayoutLoadError::Config {
                    path: config.clone(),
                    source,
                })?
            } else {
                TaulerConfig::default()
            };
            Ok(LoadedLayout { config, js_source })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    fn write(dir: &std::path::Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("write fixture file");
        path
    }

    #[test]
    fn detect_returns_mdx_when_only_layout_op_mdx_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mdx_path = write(dir.path(), "layout.op.mdx", "<Panel />\n");

        let detected = LayoutSource::detect(dir.path());

        assert_eq!(detected, Some(LayoutSource::Mdx(mdx_path)));
    }

    #[test]
    fn detect_returns_legacy_when_only_layout_jsx_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout_path = write(dir.path(), "layout.jsx", "<Panel />\n");

        let detected = LayoutSource::detect(dir.path());

        assert_eq!(
            detected,
            Some(LayoutSource::Legacy {
                layout: layout_path,
                config: dir.path().join("config.yaml"),
            })
        );
    }

    #[test]
    fn detect_prefers_mdx_when_both_exist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mdx_path = write(dir.path(), "layout.op.mdx", "<Panel />\n");
        write(dir.path(), "layout.jsx", "<Panel />\n");

        let detected = LayoutSource::detect(dir.path());

        assert_eq!(detected, Some(LayoutSource::Mdx(mdx_path)));
    }

    #[test]
    fn detect_returns_none_when_neither_exists() {
        let dir = tempfile::tempdir().expect("tempdir");

        let detected = LayoutSource::detect(dir.path());

        assert_eq!(detected, None);
    }

    #[test]
    fn load_mdx_with_frontmatter_parses_theme_and_keeps_jsx_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mdx_path = write(
            dir.path(),
            "layout.op.mdx",
            "---\ntheme:\n  mode: light\n---\nimport { h } from 'esto'\n\n<Panel id=\"sidebar\" />\n",
        );

        let loaded = load(&LayoutSource::Mdx(mdx_path)).expect("valid mdx should load");

        assert_eq!(loaded.config.theme.mode, ThemeMode::Light);
        assert!(
            loaded.js_source.contains(r#"<Panel id="sidebar" />"#),
            "expected the JSX body to survive lowering, got: {}",
            loaded.js_source
        );
    }

    #[test]
    fn load_mdx_without_frontmatter_defaults_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mdx_path = write(
            dir.path(),
            "layout.op.mdx",
            "import { h } from 'esto'\n\n<Panel id=\"sidebar\" />\n",
        );

        let loaded =
            load(&LayoutSource::Mdx(mdx_path)).expect("mdx without frontmatter should load");

        assert_eq!(loaded.config.theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn load_mdx_with_invalid_frontmatter_yaml_is_a_config_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mdx_path = write(
            dir.path(),
            "layout.op.mdx",
            "---\ntheme: [\n---\n<Panel />\n",
        );

        let err =
            load(&LayoutSource::Mdx(mdx_path)).expect_err("invalid frontmatter YAML must error");

        assert!(
            matches!(err, LayoutLoadError::Config { .. }),
            "expected LayoutLoadError::Config, got: {err:?}"
        );
    }

    #[test]
    fn load_legacy_reads_config_yaml_and_uses_layout_jsx_verbatim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout_path = write(dir.path(), "layout.jsx", "<Panel id=\"sidebar\" />\n");
        let config_path = write(dir.path(), "config.yaml", "theme:\n  mode: light\n");

        let loaded = load(&LayoutSource::Legacy {
            layout: layout_path,
            config: config_path,
        })
        .expect("valid legacy pair should load");

        assert_eq!(loaded.config.theme.mode, ThemeMode::Light);
        assert_eq!(loaded.js_source, "<Panel id=\"sidebar\" />\n");
    }

    #[test]
    fn load_legacy_missing_config_yaml_defaults_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout_path = write(dir.path(), "layout.jsx", "<Panel />\n");
        let config_path = dir.path().join("config.yaml"); // deliberately not written

        let loaded = load(&LayoutSource::Legacy {
            layout: layout_path,
            config: config_path,
        })
        .expect("missing config.yaml is not an error");

        assert_eq!(loaded.config.theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn load_legacy_invalid_config_yaml_is_a_config_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout_path = write(dir.path(), "layout.jsx", "<Panel />\n");
        let config_path = write(dir.path(), "config.yaml", "theme: [\n");

        let err = load(&LayoutSource::Legacy {
            layout: layout_path,
            config: config_path,
        })
        .expect_err("invalid config.yaml must error, not silently default");

        assert!(
            matches!(err, LayoutLoadError::Config { .. }),
            "expected LayoutLoadError::Config, got: {err:?}"
        );
    }

    #[test]
    fn load_legacy_missing_layout_jsx_is_a_read_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout_path = dir.path().join("layout.jsx"); // deliberately not written
        let config_path = write(dir.path(), "config.yaml", "theme:\n  mode: light\n");

        let err = load(&LayoutSource::Legacy {
            layout: layout_path,
            config: config_path,
        })
        .expect_err("missing layout.jsx must error");

        assert!(
            matches!(err, LayoutLoadError::Read { .. }),
            "expected LayoutLoadError::Read, got: {err:?}"
        );
    }
}
