//! `scripts/sync-workspace-versions.sh` is the repair half of
//! `workspace_manifests_test`: that test says a path dependency requires the wrong
//! version, this script rewrites it. The release PR runs it unattended, so the
//! substitution is worth pinning down — a regex that quietly matches nothing looks
//! exactly like a manifest that was already correct.

use std::fs;
use std::path::Path;
use std::process::Command;

fn run(root: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/sync-workspace-versions.sh");
    let output = Command::new(&script)
        .arg(root)
        .output()
        .expect("the script is executable");
    assert!(
        output.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

const ROOT_MANIFEST: &str = r#"[workspace]
members = ["sibling"]

[workspace.package]
version = "0.2.0"

[dependencies]
sibling = { path = "sibling", version = "0.1.9" }
"#;

#[test]
fn rewrites_a_stale_path_dependency_to_the_workspace_version() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("Cargo.toml"), ROOT_MANIFEST);

    run(root.path());

    let rewritten = fs::read_to_string(root.path().join("Cargo.toml")).unwrap();
    assert!(
        rewritten.contains(r#"sibling = { path = "sibling", version = "0.2.0" }"#),
        "root manifest was not rewritten:\n{rewritten}"
    );
}

#[test]
fn rewrites_member_manifests_too() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("Cargo.toml"), ROOT_MANIFEST);
    write(
        &root.path().join("sibling/Cargo.toml"),
        "[dependencies]\nparent = { path = \"..\", version = \"0.1.9\" }\n",
    );

    run(root.path());

    let rewritten = fs::read_to_string(root.path().join("sibling/Cargo.toml")).unwrap();
    assert!(
        rewritten.contains(r#"parent = { path = "..", version = "0.2.0" }"#),
        "member manifest was not rewritten:\n{rewritten}"
    );
}

#[test]
fn leaves_registry_dependencies_and_comments_alone() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("Cargo.toml"),
        r#"[workspace.package]
version = "0.2.0"

[dependencies]
serde = { version = "1.0.229", features = ["derive"] }
# sibling = { path = "sibling", version = "0.1.9" }

[[bin]]
path = "src/main.rs"
"#,
    );

    run(root.path());

    let rewritten = fs::read_to_string(root.path().join("Cargo.toml")).unwrap();
    assert!(
        rewritten.contains(r#"serde = { version = "1.0.229", features = ["derive"] }"#),
        "a registry dependency was rewritten:\n{rewritten}"
    );
    assert!(
        rewritten.contains(r#"# sibling = { path = "sibling", version = "0.1.9" }"#),
        "a commented-out dependency was rewritten:\n{rewritten}"
    );
    assert!(
        rewritten.contains(r#"path = "src/main.rs""#),
        "a target path was mangled:\n{rewritten}"
    );
}
