//! Fetching a Package's git repository onto disk. See ADR 0041's "Package cache"
//! section: a write always goes clone-into-a-temp-directory, `git checkout <ref>`,
//! then an atomic rename into the final, content-addressed path — a cache "hit" is
//! therefore only ever a fully-formed final directory, never a partial clone.
//!
//! Shells out to the system `git` (`std::process::Command`), matching this
//! codebase's existing pattern of shelling out for side effects (`sh` in
//! optative-script) rather than adding a `git2`/`libgit2` dependency.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Why fetching or placing a Package's checkout failed.
#[derive(Debug)]
pub enum FetchError {
    /// The `git` subprocess itself could not be spawned (e.g. `git` not on `PATH`).
    Spawn(std::io::Error),
    /// `git` ran and exited non-zero. Carries stderr so a caller can log it loudly,
    /// per ADR 0041's security posture — nothing about a Package fetch is silent.
    GitFailed { args: Vec<String>, stderr: String },
    /// The final rename failed for a reason other than the target already existing
    /// (which is treated as a benign race, not an error — see [`place`]).
    Rename(std::io::Error),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "could not run git: {e}"),
            Self::GitFailed { args, stderr } => {
                write!(f, "git {} failed: {stderr}", args.join(" "))
            }
            Self::Rename(e) => write!(f, "could not place fetched checkout: {e}"),
        }
    }
}

impl std::error::Error for FetchError {}

fn run_git(args: &[&str], current_dir: Option<&Path>) -> Result<(), FetchError> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    // Never let a `git` subprocess block on a credential prompt — this runs
    // headless, off the render path, and must always eventually fail rather than
    // hang if a repo turns out to need auth.
    command.env("GIT_TERMINAL_PROMPT", "0");
    let output = command.output().map_err(FetchError::Spawn)?;
    if !output.status.success() {
        return Err(FetchError::GitFailed {
            args: args.iter().map(|s| s.to_string()).collect(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Clones `remote` (a URL, or a plain filesystem path — used by this module's own
/// tests against a local fixture repo) into `dest`, which must not already exist.
fn clone_into(remote: &str, dest: &Path) -> Result<(), FetchError> {
    run_git(&["clone", "--quiet", remote, &dest.to_string_lossy()], None)
}

/// Checks out `sha_or_ref` inside an already-cloned repo at `repo_dir`. A plain
/// `git clone` fetches full history by default (no shallow clone in v1), so any
/// commit reachable from any branch is already present locally — no extra `fetch`
/// is needed first.
fn checkout(repo_dir: &Path, sha_or_ref: &str) -> Result<(), FetchError> {
    run_git(&["checkout", "--quiet", sha_or_ref], Some(repo_dir))
}

/// What happened when placing a completed clone at its final, content-addressed
/// path.
#[derive(Debug, PartialEq, Eq)]
pub enum PlaceOutcome {
    /// `temp_dir` is now at `final_path`.
    Placed,
    /// `final_path` already existed — another fetch of the same key won the race.
    /// `temp_dir` was removed; its work was redundant, not wrong.
    AlreadyPresent,
}

/// Atomically moves a completed clone from `temp_dir` to `final_path`. A rename
/// failing because `final_path` already exists is a benign race, not an error —
/// see ADR 0041's "Package cache" section.
pub fn place(temp_dir: &Path, final_path: &Path) -> Result<PlaceOutcome, FetchError> {
    if final_path.exists() {
        std::fs::remove_dir_all(temp_dir).map_err(FetchError::Rename)?;
        return Ok(PlaceOutcome::AlreadyPresent);
    }
    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent).map_err(FetchError::Rename)?;
    }
    match std::fs::rename(temp_dir, final_path) {
        Ok(()) => Ok(PlaceOutcome::Placed),
        Err(_) if final_path.exists() => {
            // Lost a race that landed between the check above and the rename.
            std::fs::remove_dir_all(temp_dir).map_err(FetchError::Rename)?;
            Ok(PlaceOutcome::AlreadyPresent)
        }
        Err(e) => Err(FetchError::Rename(e)),
    }
}

/// A per-attempt, randomly-named temp directory under `cache_root` — never shared
/// between two fetch attempts, however they arose (a normal retry, or one orphaned
/// by tauler's own re-exec not killing its `git` child; see ADR 0041's cache
/// hygiene section).
fn unique_temp_dir(cache_root: &Path) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    cache_root.join(format!(".tmp-{}-{:x}", std::process::id(), nanos))
}

/// Removes `.tmp-*` directories under `cache_root` older than `min_age` — the
/// only automated cache bookkeeping in v1 (ADR 0041's "Cache hygiene" section).
/// `min_age` exists to tell an abandoned clone (tauler's own re-exec doesn't kill
/// a spawned `git clone` child, so a race between re-exec and an in-flight fetch
/// can orphan one) from one that's still legitimately running — it is
/// deliberately generous and shares no constant with anything else, since
/// nothing else needs a time budget once retries are no longer backed off.
/// Returns how many directories were removed. Only ever called from `tauler pkg
/// update`, never automatically at startup — see the ADR for why one trigger
/// point is enough.
pub fn reap_orphaned_temp_dirs(cache_root: &Path, min_age: Duration) -> std::io::Result<usize> {
    let entries = match std::fs::read_dir(cache_root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(".tmp-") {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|m| m.modified())
            .and_then(|modified| {
                SystemTime::now()
                    .duration_since(modified)
                    .map_err(|e| std::io::Error::other(e.to_string()))
            })
            .unwrap_or(Duration::ZERO);
        if age >= min_age {
            std::fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Clones `remote` at `sha_or_ref`, then atomically places it at `final_path`.
/// `cache_root` is where the per-attempt temp directory is created — the same root
/// `final_path` lives under, so the rename in [`place`] stays on one filesystem.
pub fn fetch_and_place(
    remote: &str,
    sha_or_ref: &str,
    cache_root: &Path,
    final_path: &Path,
) -> Result<PlaceOutcome, FetchError> {
    if final_path.exists() {
        return Ok(PlaceOutcome::AlreadyPresent);
    }
    std::fs::create_dir_all(cache_root).map_err(FetchError::Rename)?;
    let temp_dir = unique_temp_dir(cache_root);
    clone_into(remote, &temp_dir)?;
    checkout(&temp_dir, sha_or_ref)?;
    place(&temp_dir, final_path)
}

/// Reads the commit sha `HEAD` points at inside an already-cloned repo.
fn head_sha(repo_dir: &Path) -> Result<String, FetchError> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .map_err(FetchError::Spawn)?;
    if !output.status.success() {
        return Err(FetchError::GitFailed {
            args: vec!["rev-parse".to_string(), "HEAD".to_string()],
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Clones `remote`'s default branch — no pinned commit known yet, the "importing
/// this Package for the first time" case (ADR 0041, req #1/#4: an import with no
/// Lockfile entry creates one, pinned to whatever commit was current at that
/// moment). Returns the sha that ended up pinned and where it landed, so the
/// caller can write the new Lockfile entry.
pub fn fetch_head_and_place(
    remote: &str,
    owner: &str,
    repo: &str,
    cache_root: &Path,
) -> Result<(String, PathBuf, PlaceOutcome), FetchError> {
    std::fs::create_dir_all(cache_root).map_err(FetchError::Rename)?;
    let temp_dir = unique_temp_dir(cache_root);
    clone_into(remote, &temp_dir)?;
    let sha = head_sha(&temp_dir)?;
    let final_path = crate::pkg::cache::pinned_cache_path(cache_root, owner, repo, &sha);
    if final_path.exists() {
        std::fs::remove_dir_all(&temp_dir).map_err(FetchError::Rename)?;
        return Ok((sha, final_path, PlaceOutcome::AlreadyPresent));
    }
    let outcome = place(&temp_dir, &final_path)?;
    Ok((sha, final_path, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local, network-free stand-in for a real `@gh/owner/repo`: `git init`, one
    /// commit. Returns the repo path and the sha of that commit.
    fn fixture_repo(dir: &Path) -> (PathBuf, String) {
        let repo = dir.join("fixture-repo");
        std::fs::create_dir_all(&repo).expect("create fixture repo dir");
        run_git(&["init", "--quiet"], Some(&repo)).expect("git init");
        run_git(&["config", "user.email", "test@example.com"], Some(&repo))
            .expect("git config email");
        run_git(&["config", "user.name", "Test"], Some(&repo)).expect("git config name");
        std::fs::write(repo.join("index.jsx"), "export default () => null;")
            .expect("write index.jsx");
        run_git(&["add", "."], Some(&repo)).expect("git add");
        run_git(&["commit", "--quiet", "-m", "initial"], Some(&repo)).expect("git commit");
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .expect("git rev-parse");
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (repo, sha)
    }

    #[test]
    fn clone_into_checks_out_the_repos_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, _sha) = fixture_repo(dir.path());
        let dest = dir.path().join("clone-dest");

        clone_into(&repo.to_string_lossy(), &dest).expect("clone should succeed");

        assert!(dest.join("index.jsx").exists());
    }

    #[test]
    fn checkout_moves_the_working_tree_to_the_given_sha() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, first_sha) = fixture_repo(dir.path());
        std::fs::write(repo.join("index.jsx"), "export default () => 'second';")
            .expect("overwrite index.jsx");
        run_git(&["add", "."], Some(&repo)).expect("git add second");
        run_git(&["commit", "--quiet", "-m", "second"], Some(&repo)).expect("git commit second");

        let dest = dir.path().join("clone-dest");
        clone_into(&repo.to_string_lossy(), &dest).expect("clone should succeed");
        checkout(&dest, &first_sha).expect("checkout should succeed");

        let content = std::fs::read_to_string(dest.join("index.jsx")).expect("read index.jsx");
        assert_eq!(content, "export default () => null;");
    }

    #[test]
    fn place_moves_temp_dir_to_final_path_when_final_path_is_free() {
        let dir = tempfile::tempdir().expect("tempdir");
        let temp = dir.path().join("temp-clone");
        std::fs::create_dir_all(&temp).expect("create temp");
        std::fs::write(temp.join("marker"), "x").expect("write marker");
        let final_path = dir.path().join("final");

        let outcome = place(&temp, &final_path).expect("place should succeed");

        assert_eq!(outcome, PlaceOutcome::Placed);
        assert!(final_path.join("marker").exists());
        assert!(!temp.exists());
    }

    #[test]
    fn place_discards_temp_dir_when_final_path_already_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let temp = dir.path().join("temp-clone");
        std::fs::create_dir_all(&temp).expect("create temp");
        let final_path = dir.path().join("final");
        std::fs::create_dir_all(&final_path).expect("create pre-existing final");
        std::fs::write(final_path.join("already-here"), "x").expect("write marker");

        let outcome = place(&temp, &final_path).expect("place should succeed");

        assert_eq!(outcome, PlaceOutcome::AlreadyPresent);
        assert!(!temp.exists());
        assert!(final_path.join("already-here").exists());
    }

    #[test]
    fn fetch_and_place_produces_a_checkout_at_the_pinned_sha() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, sha) = fixture_repo(dir.path());
        let cache_root = dir.path().join("cache");
        let final_path = cache_root.join("gh").join("foo").join("bar").join(&sha);

        let outcome = fetch_and_place(&repo.to_string_lossy(), &sha, &cache_root, &final_path)
            .expect("fetch_and_place should succeed");

        assert_eq!(outcome, PlaceOutcome::Placed);
        assert!(final_path.join("index.jsx").exists());
    }

    #[test]
    fn fetch_and_place_is_a_no_op_when_the_final_path_is_already_warm() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, sha) = fixture_repo(dir.path());
        let cache_root = dir.path().join("cache");
        let final_path = cache_root.join("gh").join("foo").join("bar").join(&sha);
        std::fs::create_dir_all(&final_path).expect("pre-warm final path");
        std::fs::write(final_path.join("already-warm"), "x").expect("write marker");

        let outcome = fetch_and_place(&repo.to_string_lossy(), &sha, &cache_root, &final_path)
            .expect("fetch_and_place should succeed");

        assert_eq!(outcome, PlaceOutcome::AlreadyPresent);
        assert!(final_path.join("already-warm").exists());
    }

    #[test]
    fn checkout_of_an_unknown_ref_fails_loudly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, _sha) = fixture_repo(dir.path());
        let dest = dir.path().join("clone-dest");
        clone_into(&repo.to_string_lossy(), &dest).expect("clone should succeed");

        let result = checkout(&dest, "not-a-real-ref");

        assert!(matches!(result, Err(FetchError::GitFailed { .. })));
    }

    #[test]
    fn fetch_head_and_place_pins_whatever_commit_head_currently_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, expected_sha) = fixture_repo(dir.path());
        let cache_root = dir.path().join("cache");

        let (sha, final_path, outcome) =
            fetch_head_and_place(&repo.to_string_lossy(), "foo", "bar", &cache_root)
                .expect("fetch_head_and_place should succeed");

        assert_eq!(sha, expected_sha);
        assert_eq!(outcome, PlaceOutcome::Placed);
        assert_eq!(
            final_path,
            cache_root.join("gh").join("foo").join("bar").join(&sha)
        );
        assert!(final_path.join("index.jsx").exists());
    }

    #[test]
    fn fetch_head_and_place_is_a_no_op_when_that_commit_is_already_cached() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, sha) = fixture_repo(dir.path());
        let cache_root = dir.path().join("cache");
        let final_path = cache_root.join("gh").join("foo").join("bar").join(&sha);
        std::fs::create_dir_all(&final_path).expect("pre-warm final path");
        std::fs::write(final_path.join("already-warm"), "x").expect("write marker");

        let (_sha, _final_path, outcome) =
            fetch_head_and_place(&repo.to_string_lossy(), "foo", "bar", &cache_root)
                .expect("fetch_head_and_place should succeed");

        assert_eq!(outcome, PlaceOutcome::AlreadyPresent);
        assert!(final_path.join("already-warm").exists());
    }

    #[test]
    fn reap_orphaned_temp_dirs_removes_old_tmp_dirs_and_leaves_young_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache_root = dir.path().join("cache");
        std::fs::create_dir_all(&cache_root).expect("create cache root");

        let old = cache_root.join(".tmp-old");
        std::fs::create_dir_all(&old).expect("create old tmp dir");
        // Back-date it past the age floor — a freshly-created dir would never be
        // "old" within a fast-running test otherwise.
        let past = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::open(&old)
            .expect("open old tmp dir")
            .set_modified(past)
            .expect("set old mtime");

        let young = cache_root.join(".tmp-young");
        std::fs::create_dir_all(&young).expect("create young tmp dir");

        let not_a_tmp_dir = cache_root.join("gh");
        std::fs::create_dir_all(&not_a_tmp_dir).expect("create non-tmp dir");

        let removed = reap_orphaned_temp_dirs(&cache_root, Duration::from_secs(1800))
            .expect("should succeed");

        assert_eq!(removed, 1);
        assert!(!old.exists());
        assert!(young.exists());
        assert!(not_a_tmp_dir.exists());
    }

    #[test]
    fn reap_orphaned_temp_dirs_is_a_no_op_when_the_cache_root_does_not_exist_yet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache_root = dir.path().join("never-created");

        let removed = reap_orphaned_temp_dirs(&cache_root, Duration::from_secs(1800))
            .expect("should succeed");

        assert_eq!(removed, 0);
    }
}
