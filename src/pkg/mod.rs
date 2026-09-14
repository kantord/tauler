//! Git-backed Packages a layout file imports directly (`@gh/owner/repo`).
//!
//! See `docs/adr/0041-packages-plug-into-the-existing-resolver-and-reload-path.md`
//! and CONTEXT.md's **Package**/**Development mode** entries for the design this
//! module implements.

pub mod cache;
pub mod fetch_manager;
pub mod git;
pub mod lockfile;
pub mod specifier;
pub mod update;

use std::path::PathBuf;
use std::sync::Arc;

/// Everything the module resolver needs to resolve `@gh/owner/repo` specifiers for
/// one layout file: where its Lockfile lives, where the package cache root is, and
/// the shared [`fetch_manager::FetchManager`] that dedupes and notifies on cold
/// misses. Threaded into [`crate::jsx::JsxEvaluator`] construction the same way
/// `base_dir` already is — see ADR 0041.
#[derive(Clone)]
pub struct PackageContext {
    pub fetch_manager: Arc<fetch_manager::FetchManager>,
    pub lockfile_path: PathBuf,
    pub cache_root: PathBuf,
}

/// `@gh/<owner>/<repo>` → its GitHub HTTPS clone URL. GitHub only in v1 — see ADR
/// 0041.
pub fn remote_url(owner: &str, repo: &str) -> String {
    format!("https://github.com/{owner}/{repo}.git")
}
