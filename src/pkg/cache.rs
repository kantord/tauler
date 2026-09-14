//! Where a Package's checkout lives on disk. See ADR 0041's "Package cache" and
//! "Development mode" sections for the reasoning: content-addressing by `<ref>` is
//! load-bearing (two Lockfiles can legitimately pin the same `owner/repo` to two
//! different commits, or independently put it in development mode, and a single
//! mutable checkout shared between them would race), not decoration.

use crate::pkg::lockfile::PackageEntry;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// `<cache_root>/gh/<owner>/<repo>/<sha>` — the checkout of a Package pinned to a
/// commit.
pub fn pinned_cache_path(cache_root: &Path, owner: &str, repo: &str, sha: &str) -> PathBuf {
    cache_root.join("gh").join(owner).join(repo).join(sha)
}

/// The `development-<hash>` ref segment for a Package in Development mode — not a
/// full path, just the piece that stands in for a commit sha. Hashed from the
/// declaring Lockfile's *canonicalized* path so the same physical Lockfile reached
/// via a symlink or a different relative spelling doesn't produce a spurious second
/// checkout. `DefaultHasher` (already in `std`) is deliberate: this is
/// collision-avoidance for a directory name, not a security boundary, so no
/// cryptographic hash is warranted.
pub fn development_ref(lockfile_path: &Path) -> std::io::Result<String> {
    let canonical = std::fs::canonicalize(lockfile_path)?;
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    Ok(format!("development-{:x}", hasher.finish()))
}

/// The full checkout path for a Package: pinned-by-commit if `entry.development` is
/// false, keyed by [`development_ref`] otherwise.
pub fn package_cache_path(
    cache_root: &Path,
    owner: &str,
    repo: &str,
    entry: &PackageEntry,
    lockfile_path: &Path,
) -> std::io::Result<PathBuf> {
    let ref_segment = if entry.development {
        development_ref(lockfile_path)?
    } else {
        entry.commit.clone()
    };
    Ok(pinned_cache_path(cache_root, owner, repo, &ref_segment))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkg::lockfile::PackageEntry;
    use std::path::PathBuf;

    #[test]
    fn pinned_cache_path_builds_gh_owner_repo_sha() {
        let cache_root = PathBuf::from("/home/user/.cache/tauler/pkg");
        let path = pinned_cache_path(&cache_root, "foo", "bar", "abc123");
        assert_eq!(
            path,
            PathBuf::from("/home/user/.cache/tauler/pkg/gh/foo/bar/abc123")
        );
    }

    #[test]
    fn development_ref_is_deterministic_for_the_same_canonical_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lockfile_path = dir.path().join("tauler-pkg.lock");
        std::fs::write(&lockfile_path, "").expect("write lockfile");

        let first = development_ref(&lockfile_path).expect("canonicalize should succeed");
        let second = development_ref(&lockfile_path).expect("canonicalize should succeed");
        assert_eq!(first, second);
        assert!(first.starts_with("development-"));
    }

    #[test]
    fn development_ref_differs_for_different_lockfiles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a.lock");
        let b = dir.path().join("b.lock");
        std::fs::write(&a, "").expect("write a");
        std::fs::write(&b, "").expect("write b");

        let ref_a = development_ref(&a).expect("canonicalize should succeed");
        let ref_b = development_ref(&b).expect("canonicalize should succeed");
        assert_ne!(ref_a, ref_b);
    }

    #[test]
    fn development_ref_is_the_same_through_a_relative_spelling_of_the_same_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lockfile_path = dir.path().join("tauler-pkg.lock");
        std::fs::write(&lockfile_path, "").expect("write lockfile");

        let via_absolute = development_ref(&lockfile_path).expect("canonicalize should succeed");
        let via_dot_slash = development_ref(&dir.path().join("./tauler-pkg.lock"))
            .expect("canonicalize should succeed");
        assert_eq!(via_absolute, via_dot_slash);
    }

    #[test]
    fn package_cache_path_uses_the_pinned_commit_when_not_in_development_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lockfile_path = dir.path().join("tauler-pkg.lock");
        std::fs::write(&lockfile_path, "").expect("write lockfile");
        let cache_root = PathBuf::from("/cache");
        let entry = PackageEntry {
            commit: "abc123".to_string(),
            development: false,
        };

        let path = package_cache_path(&cache_root, "foo", "bar", &entry, &lockfile_path)
            .expect("should succeed");
        assert_eq!(path, PathBuf::from("/cache/gh/foo/bar/abc123"));
    }

    #[test]
    fn package_cache_path_uses_the_development_ref_when_in_development_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lockfile_path = dir.path().join("tauler-pkg.lock");
        std::fs::write(&lockfile_path, "").expect("write lockfile");
        let cache_root = PathBuf::from("/cache");
        let entry = PackageEntry {
            commit: "abc123".to_string(),
            development: true,
        };

        let path = package_cache_path(&cache_root, "foo", "bar", &entry, &lockfile_path)
            .expect("should succeed");
        let expected_ref = development_ref(&lockfile_path).expect("should succeed");
        assert_eq!(
            path,
            PathBuf::from(format!("/cache/gh/foo/bar/{expected_ref}"))
        );
    }
}
