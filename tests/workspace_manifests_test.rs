//! Intra-workspace dependencies must require the *current* workspace version.
//!
//! This has broken CI twice, silently and in the same way both times. `cargo publish
//! --dry-run` verifies each crate from its packaged tarball, and resolves that crate's
//! dependencies through the registry when the requirement admits a published version.
//! A path dependency with a lower floor — `{ path = "..", version = "0.1.3" }` while
//! the workspace is at `0.1.4` — therefore gets verified against the *released* copy of
//! its sibling, not the one in the checkout.
//!
//! Nothing goes wrong while the two agree. The failure appears the moment a rename or a
//! signature change makes them differ, and it appears only in CI, as an error about an
//! item that plainly exists locally.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `version` under `[workspace.package]`, which every member inherits.
fn workspace_version(root_manifest: &str) -> String {
    let after = root_manifest
        .split_once("[workspace.package]")
        .expect("root manifest has a [workspace.package] section")
        .1;
    let line = after
        .lines()
        .find(|l| l.trim_start().starts_with("version ="))
        .expect("[workspace.package] declares a version");
    line.split('"')
        .nth(1)
        .expect("version is a quoted string")
        .to_string()
}

fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![root.join("Cargo.toml")];
    for entry in fs::read_dir(root).expect("repo root is readable").flatten() {
        let manifest = entry.path().join("Cargo.toml");
        if manifest.is_file() {
            found.push(manifest);
        }
    }
    found
}

#[test]
fn path_dependencies_require_the_current_workspace_version() {
    let root = repo_root();
    let expected = workspace_version(&fs::read_to_string(root.join("Cargo.toml")).unwrap());

    let mut stale = Vec::new();
    for manifest in manifests(&root) {
        let text = fs::read_to_string(&manifest).unwrap();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') || !line.contains("path =") || !line.contains("version =") {
                continue;
            }
            let Some(required) = line
                .split("version =")
                .nth(1)
                .and_then(|v| v.split('"').nth(1))
            else {
                continue;
            };
            if required != expected {
                stale.push(format!(
                    "{}: {line}",
                    manifest.strip_prefix(&root).unwrap().display()
                ));
            }
        }
    }

    assert!(
        stale.is_empty(),
        "these path dependencies do not require the current workspace version ({expected}), \
         so `cargo publish --dry-run` will verify against the released crate instead of the \
         local one:\n  {}",
        stale.join("\n  ")
    );
}
