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
//!
//! One exception: a crate in [`INDEPENDENTLY_VERSIONED_CRATES`] opts out of workspace-
//! version lockstep on purpose (see the comment on `tauler-ui-macro`'s `version` field),
//! specifically so release-plz stops sweeping it into a version bump on cycles where it
//! has no commits of its own — release-plz/release-plz#2878. A dependency on one of
//! those crates is checked against nothing here; `scripts/sync-workspace-versions.sh`
//! leaves it alone the same way.

use std::fs;
use std::path::{Path, PathBuf};

/// Crate names this test does not expect to require the shared workspace version —
/// see the module doc comment.
const INDEPENDENTLY_VERSIONED_CRATES: &[&str] = &["tauler-ui-macro", "tauler-core"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `version` under `[workspace.package]`, which every lockstepped member inherits.
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
            if INDEPENDENTLY_VERSIONED_CRATES
                .iter()
                .any(|name| line.starts_with(&format!("{name} =")))
            {
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
         local one:\n  {}\nrun `just sync-versions` to fix",
        stale.join("\n  ")
    );
}

// The Rust version is written in several files. Renovate moves them together (see the
// "rust toolchain" rule in renovate.json); this test catches a hand edit or a partial
// bump that leaves them disagreeing.

/// Every `toolchain: "X.Y.Z"` pin under `.github/workflows`, as (file, version).
fn workflow_toolchain_pins(root: &Path) -> Vec<(String, String)> {
    let dir = root.join(".github/workflows");
    let mut pins = Vec::new();
    for entry in fs::read_dir(&dir)
        .expect("workflows are readable")
        .flatten()
    {
        let path = entry.path();
        let name = format!(".github/workflows/{}", entry.file_name().to_string_lossy());
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("toolchain:") {
                pins.push((name.clone(), rest.trim().trim_matches('"').to_string()));
            }
        }
    }
    pins
}

/// The X.Y.Z tag in `FROM rust:X.Y.Z-...`.
fn dockerfile_rust_version(dockerfile: &str) -> String {
    let line = dockerfile
        .lines()
        .find(|l| l.starts_with("FROM rust:"))
        .expect("tauler-e2e/Dockerfile has a `FROM rust:` line");
    line["FROM rust:".len()..]
        .split(['-', '@', ' '])
        .next()
        .unwrap()
        .to_string()
}

/// The `rust-version` under `[workspace.package]`.
fn workspace_rust_version(root_manifest: &str) -> String {
    let line = root_manifest
        .lines()
        .find(|l| l.starts_with("rust-version ="))
        .expect("the root manifest declares rust-version");
    line.split('"').nth(1).unwrap().to_string()
}

/// The version in the README's "Rust X.Y.Z or newer" requirement.
fn readme_rust_version(readme: &str) -> String {
    let line = readme
        .lines()
        .find(|l| l.starts_with("- Rust "))
        .expect("README.md lists a `- Rust X.Y.Z or newer` requirement");
    line["- Rust ".len()..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn version_tuple(v: &str) -> Vec<u64> {
    v.split('.')
        .map(|n| n.parse().expect("version parts are numbers"))
        .collect()
}

#[test]
fn rust_versions_agree_across_files() {
    let root = repo_root();
    let pins = workflow_toolchain_pins(&root);
    assert!(
        !pins.is_empty(),
        "found no `toolchain:` pins in the workflows"
    );
    let toolchain = pins[0].1.clone();

    let off: Vec<_> = pins.iter().filter(|(_, v)| *v != toolchain).collect();
    assert!(
        off.is_empty(),
        "`toolchain:` pins in .github/workflows disagree (expected {toolchain}): {off:?}. \
         Fix the odd ones out; Renovate normally bumps them all in one PR."
    );

    let docker =
        dockerfile_rust_version(&fs::read_to_string(root.join("tauler-e2e/Dockerfile")).unwrap());
    assert_eq!(
        docker, toolchain,
        "tauler-e2e/Dockerfile builds on rust:{docker} but the workflows pin {toolchain}; \
         fix the Dockerfile `FROM rust:` tag (Renovate normally does this)"
    );

    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let msrv = workspace_rust_version(&manifest);
    assert!(
        version_tuple(&msrv) <= version_tuple(&toolchain),
        "Cargo.toml rust-version {msrv} is newer than the pinned toolchain {toolchain}, so CI \
         could not build it; fix rust-version in Cargo.toml (Renovate normally does this)"
    );

    let readme = readme_rust_version(&fs::read_to_string(root.join("README.md")).unwrap());
    assert_eq!(
        readme, msrv,
        "README.md says Rust {readme} but Cargo.toml rust-version is {msrv}; fix the README \
         requirement (Renovate normally does this)"
    );
}
