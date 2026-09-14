//! Deduplicates concurrent fetch attempts for the same Package and wakes the
//! normal reload path once one lands. Deliberately small — per ADR 0041, "a plain
//! private struct, not a subsystem": an in-process dedup table plus a cloned
//! `reload_tx`. No backoff, no persisted failure state (cut during design — see
//! the ADR's "Cache hygiene" section); a failed fetch just becomes retriable again
//! the moment its thread finishes.

use crate::pkg::git::{fetch_and_place, fetch_head_and_place, PlaceOutcome};
use crate::pkg::lockfile::{Lockfile, PackageEntry};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

/// The ref segment [`FetchManager::request_first_fetch`] dedups against — never a
/// real commit sha (those are 40 hex characters), so it can't collide with one.
const FIRST_FETCH_REF: &str = "HEAD";

/// Identifies one in-flight (or completed) fetch attempt. `r#ref` is either a
/// pinned commit sha or a Development-mode ref segment — whichever
/// `pkg::cache::package_cache_path` already resolved it to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FetchKey {
    pub owner: String,
    pub repo: String,
    pub r#ref: String,
}

/// The dedup table itself, factored out so it can be cloned (cheaply — it's just
/// an `Arc`) into the spawned fetch thread, which has no `&FetchManager` to call
/// back into.
#[derive(Clone)]
struct InFlight(Arc<Mutex<HashSet<FetchKey>>>);

impl InFlight {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(HashSet::new())))
    }

    /// Claims `key` for this caller if nothing else already owns it. Pure and
    /// synchronous — no thread, no I/O. `true` means the caller now owns doing the
    /// fetch; `false` means someone else already does.
    fn try_start(&self, key: &FetchKey) -> bool {
        self.0.lock().unwrap().insert(key.clone())
    }

    fn finish(&self, key: &FetchKey) {
        self.0.lock().unwrap().remove(key);
    }
}

pub struct FetchManager {
    in_flight: InFlight,
    reload_tx: mpsc::Sender<()>,
}

impl FetchManager {
    pub fn new(reload_tx: mpsc::Sender<()>) -> Self {
        Self {
            in_flight: InFlight::new(),
            reload_tx,
        }
    }

    fn try_start(&self, key: &FetchKey) -> bool {
        self.in_flight.try_start(key)
    }

    /// Non-blocking. If `key` is already in flight, this is a no-op — the caller
    /// that's already fetching it will notify on success regardless. Otherwise
    /// spawns a background thread that fetches and places the checkout; on
    /// success it sends on `reload_tx` (the same channel a file watcher already
    /// uses) so the normal reload path retries on its own. On failure it logs
    /// loudly and sends nothing — there's nothing new to retry against yet.
    pub fn request_fetch(
        &self,
        key: FetchKey,
        remote: String,
        cache_root: PathBuf,
        final_path: PathBuf,
    ) {
        if !self.try_start(&key) {
            return;
        }
        let in_flight = self.in_flight.clone();
        let reload_tx = self.reload_tx.clone();
        let thread_key = key.clone();
        std::thread::spawn(move || {
            let sha_or_ref = thread_key.r#ref.clone();
            match fetch_and_place(&remote, &sha_or_ref, &cache_root, &final_path) {
                Ok(PlaceOutcome::Placed) => {
                    tracing::info!(
                        owner = thread_key.owner,
                        repo = thread_key.repo,
                        r#ref = thread_key.r#ref,
                        "package fetched"
                    );
                    let _ = reload_tx.send(());
                }
                Ok(PlaceOutcome::AlreadyPresent) => {
                    // Another writer (a concurrent tauler process, or a re-exec-
                    // orphaned clone) won the race — the Package is warm either
                    // way, so still worth a reload.
                    let _ = reload_tx.send(());
                }
                Err(e) => {
                    tracing::error!(
                        owner = thread_key.owner,
                        repo = thread_key.repo,
                        r#ref = thread_key.r#ref,
                        error = %e,
                        "package fetch failed"
                    );
                }
            }
            in_flight.finish(&thread_key);
        });
    }

    /// Non-blocking, for the "this Package has never been imported before" case
    /// (ADR 0041, req #1/#4): no Lockfile entry exists to pin a fetch to yet, so
    /// this clones `remote`'s default branch, learns whatever commit `HEAD`
    /// turned out to be, and writes that as the new Lockfile entry — the entry
    /// pins to "whatever commit was current at that moment," never anything else.
    /// Deduped like [`Self::request_fetch`], keyed on the literal `HEAD` ref
    /// segment (never a real sha) so a first encounter and an already-pinned
    /// fetch for the same Package can never collide in the dedup table.
    pub fn request_first_fetch(
        &self,
        owner: String,
        repo: String,
        remote: String,
        cache_root: PathBuf,
        lockfile_path: PathBuf,
    ) {
        let key = FetchKey {
            owner: owner.clone(),
            repo: repo.clone(),
            r#ref: FIRST_FETCH_REF.to_string(),
        };
        if !self.try_start(&key) {
            return;
        }
        let in_flight = self.in_flight.clone();
        let reload_tx = self.reload_tx.clone();
        std::thread::spawn(move || {
            match fetch_head_and_place(&remote, &owner, &repo, &cache_root) {
                Ok((sha, ..)) => match record_new_pin(&lockfile_path, &owner, &repo, &sha) {
                    Ok(()) => {
                        tracing::info!(
                            owner,
                            repo,
                            commit = sha,
                            "package pinned for the first time"
                        );
                        let _ = reload_tx.send(());
                    }
                    Err(e) => {
                        tracing::error!(owner, repo, error = %e, "failed to write new lockfile entry");
                    }
                },
                Err(e) => {
                    tracing::error!(owner, repo, error = %e, "first-time package fetch failed");
                }
            }
            in_flight.finish(&key);
        });
    }
}

/// Reads the Lockfile at `lockfile_path` (or starts a fresh one if it doesn't
/// exist yet), pins `owner/repo` to `sha`, and writes it back. Not a merge
/// against concurrent writers — see ADR 0041's cache hygiene section on why that
/// is an accepted, named limitation rather than something this issue asks for.
fn record_new_pin(lockfile_path: &Path, owner: &str, repo: &str, sha: &str) -> std::io::Result<()> {
    let mut lockfile = Lockfile::load_from_path(lockfile_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    lockfile.set(
        owner,
        repo,
        PackageEntry {
            commit: sha.to_string(),
            development: false,
        },
    );
    lockfile.save_to_path(lockfile_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn key(r#ref: &str) -> FetchKey {
        FetchKey {
            owner: "foo".to_string(),
            repo: "bar".to_string(),
            r#ref: r#ref.to_string(),
        }
    }

    #[test]
    fn try_start_claims_a_fresh_key() {
        let (tx, _rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        assert!(manager.try_start(&key("abc123")));
    }

    #[test]
    fn try_start_refuses_a_key_already_claimed() {
        let (tx, _rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        assert!(manager.try_start(&key("abc123")));
        assert!(!manager.try_start(&key("abc123")));
    }

    #[test]
    fn a_key_is_claimable_again_after_finish() {
        let (tx, _rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        assert!(manager.try_start(&key("abc123")));
        manager.in_flight.finish(&key("abc123"));
        assert!(manager.try_start(&key("abc123")));
    }

    /// A local, network-free stand-in for a real `@gh/owner/repo`.
    fn fixture_repo(dir: &std::path::Path) -> (PathBuf, String) {
        let repo = dir.join("fixture-repo");
        std::fs::create_dir_all(&repo).expect("create fixture repo dir");
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .status()
                .expect("run git");
            assert!(status.success());
        };
        run(&["init", "--quiet"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(repo.join("index.jsx"), "export default () => null;")
            .expect("write index.jsx");
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "initial"]);
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .expect("git rev-parse");
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (repo, sha)
    }

    #[test]
    fn request_fetch_wakes_reload_on_success() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, sha) = fixture_repo(dir.path());
        let (tx, rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        let cache_root = dir.path().join("cache");
        let final_path = cache_root.join("gh").join("foo").join("bar").join(&sha);

        manager.request_fetch(
            key(&sha),
            repo.to_string_lossy().into_owned(),
            cache_root,
            final_path.clone(),
        );

        rx.recv_timeout(Duration::from_secs(10))
            .expect("reload_tx should be woken on a successful fetch");
        assert!(final_path.join("index.jsx").exists());
    }

    #[test]
    fn request_fetch_does_not_wake_reload_on_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, _sha) = fixture_repo(dir.path());
        let (tx, rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        let cache_root = dir.path().join("cache");
        let final_path = cache_root
            .join("gh")
            .join("foo")
            .join("bar")
            .join("not-a-real-sha");

        manager.request_fetch(
            key("not-a-real-sha"),
            repo.to_string_lossy().into_owned(),
            cache_root,
            final_path,
        );

        assert!(
            rx.recv_timeout(Duration::from_secs(2)).is_err(),
            "reload_tx should not be woken by a failed fetch"
        );
    }

    #[test]
    fn a_second_request_for_a_key_already_in_flight_is_a_no_op() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, sha) = fixture_repo(dir.path());
        let (tx, rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        let cache_root = dir.path().join("cache");
        let final_path = cache_root.join("gh").join("foo").join("bar").join(&sha);

        // try_start is synchronous, so claiming happens before any thread is
        // spawned — a second call right after is guaranteed to see the key
        // still in flight, no timing race in the test itself.
        manager.request_fetch(
            key(&sha),
            repo.to_string_lossy().into_owned(),
            cache_root.clone(),
            final_path.clone(),
        );
        manager.request_fetch(
            key(&sha),
            repo.to_string_lossy().into_owned(),
            cache_root,
            final_path,
        );

        // Exactly one wake, not two — the second call was a no-op.
        rx.recv_timeout(Duration::from_secs(10))
            .expect("first fetch should wake reload");
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a deduped second request must not send a second wake"
        );
    }

    #[test]
    fn request_first_fetch_pins_head_and_creates_the_lockfile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, expected_sha) = fixture_repo(dir.path());
        let (tx, rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        let cache_root = dir.path().join("cache");
        let lockfile_path = dir.path().join("tauler-pkg.lock");

        manager.request_first_fetch(
            "foo".to_string(),
            "bar".to_string(),
            repo.to_string_lossy().into_owned(),
            cache_root.clone(),
            lockfile_path.clone(),
        );

        rx.recv_timeout(Duration::from_secs(10))
            .expect("reload_tx should be woken once the first pin lands");

        let lockfile = crate::pkg::lockfile::Lockfile::load_from_path(&lockfile_path)
            .expect("lockfile should be readable");
        let entry = lockfile
            .get("foo", "bar")
            .expect("foo/bar should now be pinned");
        assert_eq!(entry.commit, expected_sha);
        assert!(!entry.development);

        let final_path = cache_root
            .join("gh")
            .join("foo")
            .join("bar")
            .join(&expected_sha);
        assert!(final_path.join("index.jsx").exists());
    }

    #[test]
    fn a_first_fetch_and_a_pinned_fetch_for_the_same_package_do_not_collide_in_the_dedup_table() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo, sha) = fixture_repo(dir.path());
        let (tx, rx) = mpsc::channel();
        let manager = FetchManager::new(tx);
        let cache_root = dir.path().join("cache");
        let lockfile_path = dir.path().join("tauler-pkg.lock");
        let final_path = cache_root.join("gh").join("foo").join("bar").join(&sha);

        manager.request_first_fetch(
            "foo".to_string(),
            "bar".to_string(),
            repo.to_string_lossy().into_owned(),
            cache_root.clone(),
            lockfile_path,
        );
        manager.request_fetch(
            key(&sha),
            repo.to_string_lossy().into_owned(),
            cache_root,
            final_path,
        );

        // Both should complete and wake reload — neither was silently deduped
        // against the other despite naming the same owner/repo.
        rx.recv_timeout(Duration::from_secs(10))
            .expect("first wake");
        rx.recv_timeout(Duration::from_secs(10))
            .expect("second wake");
    }
}
