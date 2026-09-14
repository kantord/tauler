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
    /// Serializes `record_new_pin` writes. Two sibling `@gh/...` imports in the
    /// same layout file, both cold in the same module-link pass, are two
    /// distinct `FetchKey`s (different `owner`/`repo`) and so are never deduped
    /// against each other by `in_flight` — but they write the *same* Lockfile
    /// file, and an unsynchronized read-modify-write from both threads could
    /// silently drop one pin. This lock is only about that same-process,
    /// same-file race; a second `tauler` process (or `pkg update` running
    /// concurrently) writing the same Lockfile is the different, cross-process
    /// case ADR 0041 already names as an accepted limitation.
    lockfile_writes: Arc<Mutex<()>>,
}

impl FetchManager {
    pub fn new(reload_tx: mpsc::Sender<()>) -> Self {
        Self {
            in_flight: InFlight::new(),
            reload_tx,
            lockfile_writes: Arc::new(Mutex::new(())),
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
        let lockfile_writes = Arc::clone(&self.lockfile_writes);
        std::thread::spawn(move || {
            match fetch_head_and_place(&remote, &owner, &repo, &cache_root) {
                Ok((sha, ..)) => {
                    let pin_result = {
                        let _guard = lockfile_writes.lock().unwrap();
                        record_new_pin(&lockfile_path, &owner, &repo, &sha)
                    };
                    match pin_result {
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
                            tracing::error!(
                                owner,
                                repo,
                                error = %e,
                                "failed to write new lockfile entry"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(owner, repo, error = %e, "first-time package fetch failed");
                }
            }
            in_flight.finish(&key);
        });
    }
}

/// Reads the Lockfile at `lockfile_path` (or starts a fresh one if it doesn't
/// exist yet), pins `owner/repo` to `sha`, and writes it back. Callers within
/// this process serialize through `FetchManager::lockfile_writes`; a second
/// `tauler` process (or `pkg update`) writing the same file concurrently is
/// still unsynchronized — see ADR 0041's cache hygiene section on why that
/// cross-process case is an accepted, named limitation rather than something
/// this issue asks for.
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

    /// Found by review: two *different* Packages discovered cold in the same
    /// module-link pass (two sibling `@gh/...` imports, both new) are two
    /// distinct `FetchKey`s, so `in_flight` never deduped them against each
    /// other — but both threads read-modify-write the *same* Lockfile file.
    /// Without `lockfile_writes` serializing that critical section, a losing
    /// thread's read (taken before the winning thread's write landed) could
    /// silently drop the winner's pin. This can't be forced deterministically
    /// without flake-prone timing tricks, so what's asserted is the one thing
    /// that has to hold regardless of interleaving now that writes are
    /// serialized: both pins end up in the final Lockfile, not just one.
    #[test]
    fn two_concurrent_first_fetches_for_different_packages_both_end_up_pinned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (repo_a, sha_a) = fixture_repo(dir.path());
        // A second, distinct fixture repo — `fixture_repo` always names its
        // directory `fixture-repo`, so build the second one by hand under a
        // different parent.
        let other_dir = dir.path().join("other");
        std::fs::create_dir_all(&other_dir).expect("create other dir");
        let (repo_b, sha_b) = fixture_repo(&other_dir);

        let (tx, rx) = mpsc::channel();
        let manager = Arc::new(FetchManager::new(tx));
        let cache_root = dir.path().join("cache");
        let lockfile_path = dir.path().join("tauler-pkg.lock");

        let m1 = Arc::clone(&manager);
        let lockfile_path_1 = lockfile_path.clone();
        let cache_root_1 = cache_root.clone();
        let repo_a_str = repo_a.to_string_lossy().into_owned();
        let t1 = std::thread::spawn(move || {
            m1.request_first_fetch(
                "pkg".to_string(),
                "a".to_string(),
                repo_a_str,
                cache_root_1,
                lockfile_path_1,
            );
        });

        let m2 = Arc::clone(&manager);
        let repo_b_str = repo_b.to_string_lossy().into_owned();
        let t2 = std::thread::spawn(move || {
            m2.request_first_fetch(
                "pkg".to_string(),
                "b".to_string(),
                repo_b_str,
                cache_root,
                lockfile_path.clone(),
            );
        });

        t1.join().unwrap();
        t2.join().unwrap();
        rx.recv_timeout(Duration::from_secs(10))
            .expect("first wake");
        rx.recv_timeout(Duration::from_secs(10))
            .expect("second wake");

        let final_lockfile_path = dir.path().join("tauler-pkg.lock");
        let lockfile = crate::pkg::lockfile::Lockfile::load_from_path(&final_lockfile_path)
            .expect("lockfile should be readable");
        assert_eq!(
            lockfile.get("pkg", "a").map(|e| &e.commit),
            Some(&sha_a),
            "pkg/a's pin must not have been lost to the race"
        );
        assert_eq!(
            lockfile.get("pkg", "b").map(|e| &e.commit),
            Some(&sha_b),
            "pkg/b's pin must not have been lost to the race"
        );
    }
}
