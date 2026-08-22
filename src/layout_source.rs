//! Which files a bar's layout is made of, and how to load them into a
//! [`crate::config::TaulerConfig`] plus a JS source string ready for
//! [`crate::jsx::JsxEvaluator`].
//!
//! Two coexisting formats, chosen once at startup and never re-detected — see
//! `docs/adr/0036-the-layout-files-config-moves-into-its-own-frontmatter.md`
//! for the full rationale:
//!
//! - `layout.op.mdx`: a single file, YAML frontmatter for `theme`/`fonts` above a JSX
//!   body. tauler does its own plain-text frontmatter split (see [`split_frontmatter`])
//!   and passes the body through byte-for-byte unchanged — there is no markdown/mdx
//!   lowering step. Layout files use an imperative `export default function
//!   render() {...}` convention, and real mdx lowering (e.g.
//!   `optative_script_mdx::lower::lower_to_tsx_with_frontmatter`) synthesizes its own
//!   `export default`, which collides with that convention. This is a deliberate,
//!   documented choice, not a missing feature — see `docs/adr/0036`.
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
    /// Opened a `---` frontmatter fence but never found a closing one.
    Frontmatter { path: std::path::PathBuf },
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
            Self::Frontmatter { path } => {
                write!(
                    f,
                    "unterminated frontmatter block in layout file {}: opened with `---` but no closing `---` line was found",
                    path.display()
                )
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

/// Outcome of [`split_frontmatter`]: whether `source` opened with a YAML frontmatter
/// block, and what's on each side of it.
#[derive(Debug, PartialEq)]
enum FrontmatterSplit<'a> {
    /// `source` does not open with a `---` line: there is no frontmatter at all, and the
    /// entire input is the body (the caller already holds `source` for that; this variant
    /// carries nothing further).
    NoFrontmatter,
    /// A closed `---`...`---` block was found. `frontmatter` is the raw lines between the
    /// fences, trailing newline kept and not trimmed; `body` is everything after the
    /// closing fence's line, unchanged.
    Frontmatter { frontmatter: &'a str, body: &'a str },
    /// `source` opens with a `---` line but no later line is exactly `---`: a typo'd or
    /// missing closing fence. Kept distinct from `NoFrontmatter` so a broken fence doesn't
    /// silently vanish into "no config".
    UnterminatedFrontmatter,
}

/// Splits a leading YAML frontmatter block off `source`, if present — a line that is
/// exactly `---`, then arbitrary lines, then another line that is exactly `---`. Plain
/// text search, not markdown parsing (tauler's layout body is never markdown — see
/// docs/adr/0036's revision).
fn split_frontmatter(source: &str) -> FrontmatterSplit<'_> {
    let (first_line, rest_start) = match source.find('\n') {
        Some(newline) => (&source[..newline], newline + 1),
        None => (source, source.len()),
    };
    if first_line != "---" {
        return FrontmatterSplit::NoFrontmatter;
    }

    let rest = &source[rest_start..];
    let mut search_pos = 0;
    loop {
        let remaining = &rest[search_pos..];
        match remaining.find('\n') {
            Some(newline) => {
                let line = &remaining[..newline];
                if line == "---" {
                    let frontmatter = &rest[..search_pos];
                    let body = &rest[search_pos + newline + 1..];
                    return FrontmatterSplit::Frontmatter { frontmatter, body };
                }
                search_pos += newline + 1;
            }
            None => {
                if remaining == "---" {
                    let frontmatter = &rest[..search_pos];
                    return FrontmatterSplit::Frontmatter {
                        frontmatter,
                        body: "",
                    };
                }
                return FrontmatterSplit::UnterminatedFrontmatter;
            }
        }
    }
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
            match split_frontmatter(&raw) {
                FrontmatterSplit::NoFrontmatter => Ok(LoadedLayout {
                    config: TaulerConfig::default(),
                    js_source: raw,
                }),
                FrontmatterSplit::Frontmatter { frontmatter, body } => {
                    let config = TaulerConfig::from_yaml(frontmatter).map_err(|source| {
                        LayoutLoadError::Config {
                            path: path.clone(),
                            source,
                        }
                    })?;
                    Ok(LoadedLayout {
                        config,
                        js_source: body.to_string(),
                    })
                }
                FrontmatterSplit::UnterminatedFrontmatter => {
                    Err(LayoutLoadError::Frontmatter { path: path.clone() })
                }
            }
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
    fn load_mdx_with_unterminated_frontmatter_is_a_frontmatter_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mdx_path = write(
            dir.path(),
            "layout.op.mdx",
            "---\ntheme:\n  mode: dark\n<Panel />\n",
        );

        let err =
            load(&LayoutSource::Mdx(mdx_path)).expect_err("unterminated frontmatter must error");

        assert!(
            matches!(err, LayoutLoadError::Frontmatter { .. }),
            "expected LayoutLoadError::Frontmatter, got: {err:?}"
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
    fn split_frontmatter_no_leading_fence_returns_no_frontmatter() {
        let source = "import { h } from 'esto'\n\n<Panel />\n";

        assert_eq!(split_frontmatter(source), FrontmatterSplit::NoFrontmatter);
    }

    #[test]
    fn split_frontmatter_well_formed_block_splits_frontmatter_and_body() {
        let source = "---\ntheme:\n  mode: light\n---\n<Panel />\n";

        assert_eq!(
            split_frontmatter(source),
            FrontmatterSplit::Frontmatter {
                frontmatter: "theme:\n  mode: light\n",
                body: "<Panel />\n",
            }
        );
    }

    #[test]
    fn split_frontmatter_unterminated_block_is_distinct_from_no_frontmatter() {
        // Opens with `---` but never closes: a typo'd closing fence must not silently
        // read as "no frontmatter at all" (which would swallow the whole block, YAML
        // and all, into the JS body).
        let source = "---\ntheme:\n  mode: light\n";

        assert_eq!(
            split_frontmatter(source),
            FrontmatterSplit::UnterminatedFrontmatter
        );
        assert_ne!(
            split_frontmatter(source),
            FrontmatterSplit::NoFrontmatter,
            "an unterminated fence must not be treated the same as no frontmatter"
        );
    }

    #[test]
    fn split_frontmatter_only_closes_on_the_first_closing_fence() {
        // A `---` line further down, inside the body, must not be mistaken for the
        // frontmatter's closing fence.
        let source = "---\ntheme:\n  mode: light\n---\n<Panel />\n---\nmore\n";

        assert_eq!(
            split_frontmatter(source),
            FrontmatterSplit::Frontmatter {
                frontmatter: "theme:\n  mode: light\n",
                body: "<Panel />\n---\nmore\n",
            }
        );
    }

    #[test]
    fn split_frontmatter_closing_fence_at_eof_yields_empty_body() {
        let source = "---\ntheme:\n  mode: light\n---\n";

        assert_eq!(
            split_frontmatter(source),
            FrontmatterSplit::Frontmatter {
                frontmatter: "theme:\n  mode: light\n",
                body: "",
            }
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
