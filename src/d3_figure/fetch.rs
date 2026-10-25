//! Gets the two files a D3Figure needs — d3 and linkedom — into memory, and
//! downloads them first if they are not cached yet. Mirrors
//! `pkg/fetch_manager.rs`, because this download rides the Package path (ADR
//! 0045, ADR 0041): [`Fetcher::sources`] never blocks on the network. A cold call returns `None` and starts one background thread; when
//! that thread has stored every missing file it sends once on `reload_tx` (the
//! same channel the file watcher uses), and the reload that follows finds the
//! files cached. A D3Figure whose files are not here yet draws nothing, which is
//! already its "not drawn yet" state.
//!
//! Deliberately small, like the fetch manager: an in-process flag so at most one
//! download thread runs at a time, plus a cloned `reload_tx`. A failed download
//! is logged once, sends nothing, and holds back the next attempt: the Draw
//! runtime asks every [`super::draw::NOT_READY_POLL`] while it has a figure to
//! draw, and offline each ask would otherwise start a download and log a
//! warning. The wait starts at [`BACKOFF_BASE`], doubles per consecutive failure
//! up to [`MAX_BACKOFF`], and resets on success. Nothing is persisted: a restart
//! tries at once.
//!
//! Every file is checked against its pinned sha256 by [`assets::load`] on every
//! call and by [`assets::store`] before it is written; this unit adds no checks
//! of its own. Real downloads are HTTPS only, send a `tauler/<version>`
//! User-Agent, and give up after [`DOWNLOAD_TIMEOUT`].
//!
//! `TAULER_D3_DIR` gives an offline Fetcher: it reads that directory, with the
//! same hash check, and never downloads — for machines with no network.

use super::assets::{self, Asset, D3, LINKEDOM};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// The two files a D3Figure needs, in memory, both verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    pub d3: String,
    pub linkedom: String,
}

/// `download(url)`: the whole response body, or why there is none.
type Download = dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync;

pub struct Fetcher {
    /// Where the files are cached (or, offline, where they already are).
    dir: PathBuf,
    /// `[d3, linkedom]`. The real constructors use [`D3`] and [`LINKEDOM`];
    /// tests pin their own, since nothing but the real files matches the real pins.
    assets: [&'static Asset; 2],
    /// `None` for an offline Fetcher: it never downloads, so never wakes anyone.
    online: Option<Online>,
    /// How many times [`Self::sources`] was called.
    #[cfg(test)]
    sources_calls: std::sync::atomic::AtomicUsize,
}

/// The most a download may read. The d3 tarball is ~0.9 MB and linkedom
/// ~0.6 MB, so this only stops a runaway response.
const MAX_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024;

/// The most one real download may take, connecting included. Both files
/// arrive in a second or two on any working connection; this only stops a
/// stalled one from holding the in-flight claim for good.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the first failed download holds back the next attempt.
const BACKOFF_BASE: Duration = Duration::from_secs(5);

/// The longest any failed download holds back the next attempt.
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// What a Fetcher that may download needs.
struct Online {
    reload_tx: mpsc::Sender<()>,
    download: Arc<Download>,
    /// `true` while a download thread runs; claimed synchronously before the
    /// thread is spawned, so a second call right after sees it.
    in_flight: Arc<AtomicBool>,
    /// Written by the download thread before it releases `in_flight`, so a
    /// call that claims `in_flight` next always sees the latest failure.
    backoff: Arc<Mutex<Backoff>>,
    /// [`BACKOFF_BASE`], except in tests.
    backoff_base: Duration,
}

/// Consecutive failed downloads, and the earliest moment for the next one.
#[derive(Default)]
struct Backoff {
    failures: u32,
    not_before: Option<Instant>,
}

impl Fetcher {
    /// Real downloads: HTTPS only, User-Agent `tauler/<version>`, through `ureq`.
    /// `dir` is where the files are cached.
    pub fn new(dir: PathBuf, reload_tx: mpsc::Sender<()>) -> Self {
        Self::build(
            dir,
            [&D3, &LINKEDOM],
            Some((reload_tx, Arc::new(download_https))),
        )
    }

    /// A Fetcher that only reads `dir` and never downloads: what `TAULER_D3_DIR`
    /// gives.
    pub fn offline(dir: PathBuf) -> Self {
        Self::build(dir, [&D3, &LINKEDOM], None)
    }

    /// From the environment: `TAULER_D3_DIR` set → [`Self::offline`] over that
    /// directory; otherwise [`Self::new`] over `cache_root/d3`, where
    /// `cache_root` is tauler's cache directory (`~/.cache/tauler`).
    pub fn from_env(cache_root: &Path, reload_tx: mpsc::Sender<()>) -> Self {
        choose(
            std::env::var_os("TAULER_D3_DIR").map(PathBuf::from),
            cache_root,
            reload_tx,
        )
    }

    /// The one constructor the others delegate to. `online: None` is offline.
    /// Crate-private so tests elsewhere (jsx.rs) can pin fake assets.
    pub(crate) fn build(
        dir: PathBuf,
        assets: [&'static Asset; 2],
        online: Option<(mpsc::Sender<()>, Arc<Download>)>,
    ) -> Self {
        let online = online.map(|(reload_tx, download)| Online {
            reload_tx,
            download,
            in_flight: Arc::new(AtomicBool::new(false)),
            backoff: Default::default(),
            backoff_base: BACKOFF_BASE,
        });
        Self {
            dir,
            assets,
            online,
            #[cfg(test)]
            sources_calls: Default::default(),
        }
    }

    /// This Fetcher with failed downloads held back from `base` on.
    #[cfg(test)]
    fn with_backoff_base(mut self, base: Duration) -> Self {
        if let Some(online) = &mut self.online {
            online.backoff_base = base;
        }
        self
    }

    /// How many times [`Self::sources`] was called.
    #[cfg(test)]
    pub(crate) fn sources_calls(&self) -> usize {
        self.sources_calls.load(Ordering::Relaxed)
    }

    /// Both files if both are cached and verified. Otherwise `None`, and —
    /// unless offline — a background thread downloads whatever is missing, at
    /// most one such thread at a time; when every file is stored it sends on
    /// `reload_tx` once. A failed download sends nothing, is logged, and holds
    /// back the next attempt (see the module docs).
    pub fn sources(&self) -> Option<Sources> {
        #[cfg(test)]
        self.sources_calls.fetch_add(1, Ordering::Relaxed);
        let [d3, linkedom] = self.assets.map(|asset| assets::load(asset, &self.dir));
        if let (Some(d3), Some(linkedom)) = (d3, linkedom) {
            return Some(Sources { d3, linkedom });
        }
        if let Some(online) = &self.online {
            online.start_download(&self.dir, self.assets);
        }
        None
    }
}

impl Online {
    /// Spawns the download thread unless one is already running or a failed
    /// download is still holding the next one back.
    fn start_download(&self, dir: &Path, assets: [&'static Asset; 2]) {
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let release = Release(Arc::clone(&self.in_flight));
        let held_back = self
            .backoff
            .lock()
            .unwrap()
            .not_before
            .is_some_and(|at| Instant::now() < at);
        if held_back {
            return;
        }
        let reload_tx = self.reload_tx.clone();
        let download = Arc::clone(&self.download);
        let backoff = Arc::clone(&self.backoff);
        let base = self.backoff_base;
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            let _release = release;
            match store_missing(&dir, assets, &*download) {
                Ok(()) => {
                    *backoff.lock().unwrap() = Backoff::default();
                    tracing::info!(dir = %dir.display(), "d3 files fetched");
                    let _ = reload_tx.send(());
                }
                Err(error) => {
                    let retry_in = {
                        let mut backoff = backoff.lock().unwrap();
                        backoff.failures = backoff.failures.saturating_add(1);
                        let delay = backoff_delay(base, backoff.failures);
                        backoff.not_before = Some(Instant::now() + delay);
                        delay
                    };
                    tracing::warn!(dir = %dir.display(), %error, ?retry_in, "d3 file fetch failed");
                }
            }
        });
    }
}

/// How long the next download is held back after `failures` (≥ 1) consecutive
/// failures: `base`, doubled per failure after the first, at most
/// [`MAX_BACKOFF`].
fn backoff_delay(base: Duration, failures: u32) -> Duration {
    let factor = 1u32
        .checked_shl(failures.saturating_sub(1))
        .unwrap_or(u32::MAX);
    base.saturating_mul(factor).min(MAX_BACKOFF)
}

/// Clears the in-flight claim when dropped, so a panicking download thread
/// does not block every later attempt.
struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Downloads and stores every asset not already loadable from `dir`; stops at
/// the first failure.
fn store_missing(
    dir: &Path,
    assets: [&'static Asset; 2],
    download: &Download,
) -> Result<(), String> {
    for asset in assets {
        if assets::load(asset, dir).is_some() {
            continue;
        }
        let body = download(asset.url).map_err(|e| format!("{}: {e}", asset.url))?;
        assets::store(asset, dir, &body).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// [`Fetcher::from_env`] with the environment read already, so tests need not
/// touch the process-global environment.
fn choose(env_dir: Option<PathBuf>, cache_root: &Path, reload_tx: mpsc::Sender<()>) -> Fetcher {
    match env_dir {
        Some(dir) => Fetcher::offline(dir),
        None => Fetcher::new(cache_root.join("d3"), reload_tx),
    }
}

/// The real downloader: refuses any URL that is not `https://`, sends a
/// `tauler/<version>` User-Agent, and reads the whole body within
/// [`DOWNLOAD_TIMEOUT`].
pub(crate) fn download_https(url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err(format!(
            "refusing to download {url}: only https:// URLs are allowed"
        ));
    }
    ureq::get(url)
        .config()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .header("User-Agent", concat!("tauler/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const D3_CONTENT: &str = "export const d3 = 'fixture';\n";
    const LINKEDOM_CONTENT: &str = "export const linkedom = 'fixture';\n";
    const D3_URL: &str = "https://example.invalid/d3.min.js";
    const LINKEDOM_URL: &str = "https://example.invalid/linkedom.js";

    /// Stand-ins for [`D3`] and [`LINKEDOM`] pinned to the fixture contents.
    fn fixture_assets() -> [&'static Asset; 2] {
        [
            assets::pinned_to("d3.min.js", D3_URL, None, D3_CONTENT),
            assets::pinned_to("linkedom.js", LINKEDOM_URL, None, LINKEDOM_CONTENT),
        ]
    }

    fn expected_sources() -> Sources {
        Sources {
            d3: D3_CONTENT.to_string(),
            linkedom: LINKEDOM_CONTENT.to_string(),
        }
    }

    fn cache_both(dir: &Path) {
        std::fs::write(dir.join("d3.min.js"), D3_CONTENT).expect("write d3");
        std::fs::write(dir.join("linkedom.js"), LINKEDOM_CONTENT).expect("write linkedom");
    }

    /// Every URL the fake downloader was asked for, in order.
    type Calls = Arc<Mutex<Vec<String>>>;

    fn calls_to(calls: &Calls, url: &str) -> usize {
        calls.lock().unwrap().iter().filter(|u| *u == url).count()
    }

    /// A downloader that records each URL and answers it with `answer(url)`.
    fn recording(
        calls: &Calls,
        answer: impl Fn(&str) -> Result<Vec<u8>, String> + Send + Sync + 'static,
    ) -> Arc<Download> {
        let calls = Arc::clone(calls);
        Arc::new(move |url: &str| {
            calls.lock().unwrap().push(url.to_string());
            answer(url)
        })
    }

    /// Serves the fixture contents for the fixture URLs.
    fn serve_fixtures(url: &str) -> Result<Vec<u8>, String> {
        match url {
            D3_URL => Ok(D3_CONTENT.as_bytes().to_vec()),
            LINKEDOM_URL => Ok(LINKEDOM_CONTENT.as_bytes().to_vec()),
            other => Err(format!("no fixture for {other}")),
        }
    }

    fn online_fetcher(dir: &Path, reload_tx: mpsc::Sender<()>, download: Arc<Download>) -> Fetcher {
        Fetcher::build(
            dir.to_path_buf(),
            fixture_assets(),
            Some((reload_tx, download)),
        )
    }

    #[test]
    fn with_both_files_cached_sources_returns_them_without_downloading() {
        let dir = tempfile::tempdir().expect("tempdir");
        cache_both(dir.path());
        let calls = Calls::default();
        let (tx, _rx) = mpsc::channel();
        let fetcher = online_fetcher(dir.path(), tx, recording(&calls, serve_fixtures));

        assert_eq!(fetcher.sources(), Some(expected_sources()));
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    #[test]
    fn with_nothing_cached_the_first_call_downloads_and_wakes_reload_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let calls = Calls::default();
        let (tx, rx) = mpsc::channel();
        let fetcher = online_fetcher(dir.path(), tx, recording(&calls, serve_fixtures));

        assert_eq!(fetcher.sources(), None);

        rx.recv_timeout(Duration::from_secs(10))
            .expect("reload_tx should be woken once both files are stored");
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "one download must wake reload_tx exactly once"
        );
        assert_eq!(fetcher.sources(), Some(expected_sources()));
    }

    #[test]
    fn two_calls_while_a_download_is_in_flight_start_only_one_download() {
        let dir = tempfile::tempdir().expect("tempdir");
        let calls = Calls::default();
        // The downloader blocks until the test drops `release`.
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let download = recording(&calls, move |url| {
            let _ = gate.lock().unwrap().recv();
            serve_fixtures(url)
        });
        let (tx, rx) = mpsc::channel();
        let fetcher = online_fetcher(dir.path(), tx, download);

        assert_eq!(fetcher.sources(), None);
        assert_eq!(fetcher.sources(), None);
        drop(release);

        rx.recv_timeout(Duration::from_secs(10))
            .expect("the one download should wake reload_tx");
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a deduped second call must not wake reload_tx again"
        );
        assert_eq!(calls_to(&calls, D3_URL), 1);
        assert_eq!(calls_to(&calls, LINKEDOM_URL), 1);
    }

    #[test]
    fn a_failed_download_does_not_wake_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let calls = Calls::default();
        let (tx, rx) = mpsc::channel();
        let download = recording(&calls, |_| Err("network unreachable".to_string()));
        let fetcher = online_fetcher(dir.path(), tx, download);

        assert_eq!(fetcher.sources(), None);

        assert!(
            rx.recv_timeout(Duration::from_millis(500)).is_err(),
            "a failed download must not wake reload_tx"
        );
    }

    #[test]
    fn after_a_failed_download_a_call_after_the_backoff_tries_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let calls = Calls::default();
        let (tx, _rx) = mpsc::channel();
        let download = recording(&calls, |_| Err("network unreachable".to_string()));
        let fetcher =
            online_fetcher(dir.path(), tx, download).with_backoff_base(Duration::from_millis(50));

        // Keep calling until the backoff has passed and a second attempt starts.
        let deadline = Instant::now() + Duration::from_secs(10);
        while calls_to(&calls, D3_URL) < 2 {
            assert!(
                Instant::now() < deadline,
                "no second download was attempted"
            );
            assert_eq!(fetcher.sources(), None);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn right_after_a_failed_download_further_calls_do_not_download_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let calls = Calls::default();
        let (tx, _rx) = mpsc::channel();
        let download = recording(&calls, |_| Err("network unreachable".to_string()));
        let fetcher = online_fetcher(dir.path(), tx, download);

        assert_eq!(fetcher.sources(), None);
        std::thread::sleep(Duration::from_millis(100));
        // What the draw runtime's polls do while offline.
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            assert_eq!(fetcher.sources(), None);
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(calls_to(&calls, D3_URL), 1);
    }

    #[test]
    fn the_backoff_doubles_per_consecutive_failure_up_to_its_cap() {
        let base = Duration::from_secs(5);

        assert_eq!(backoff_delay(base, 1), Duration::from_secs(5));
        assert_eq!(backoff_delay(base, 2), Duration::from_secs(10));
        assert_eq!(backoff_delay(base, 3), Duration::from_secs(20));
        assert_eq!(backoff_delay(base, 100), MAX_BACKOFF);
    }

    #[test]
    fn a_download_that_does_not_match_its_pin_does_not_wake_reload_and_leaves_nothing_loadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let calls = Calls::default();
        let (tx, rx) = mpsc::channel();
        let download = recording(&calls, |_| Ok(b"tampered".to_vec()));
        let fetcher = online_fetcher(dir.path(), tx, download);

        assert_eq!(fetcher.sources(), None);

        assert!(
            rx.recv_timeout(Duration::from_millis(500)).is_err(),
            "a download that fails its hash check must not wake reload_tx"
        );
        for asset in fixture_assets() {
            assert_eq!(assets::load(asset, dir.path()), None, "{}", asset.file_name);
        }
    }

    #[test]
    fn an_offline_fetcher_with_the_files_missing_returns_nothing_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let offline_dir = dir.path().join("offline");
        let fetcher = Fetcher::build(offline_dir.clone(), fixture_assets(), None);

        assert_eq!(fetcher.sources(), None);
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !offline_dir.exists(),
            "an offline fetcher must not create or fill its directory"
        );
    }

    #[test]
    fn an_offline_fetcher_with_the_files_present_returns_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        cache_both(dir.path());
        let fetcher = Fetcher::build(dir.path().to_path_buf(), fixture_assets(), None);

        assert_eq!(fetcher.sources(), Some(expected_sources()));
    }

    #[test]
    fn with_tauler_d3_dir_set_the_fetcher_is_offline_over_that_dir() {
        let (tx, _rx) = mpsc::channel();
        let env_dir = PathBuf::from("/srv/d3-files");

        let fetcher = choose(
            Some(env_dir.clone()),
            Path::new("/home/u/.cache/tauler"),
            tx,
        );

        assert_eq!(fetcher.dir, env_dir);
        assert!(
            fetcher.online.is_none(),
            "TAULER_D3_DIR must never download"
        );
    }

    #[test]
    fn without_tauler_d3_dir_the_fetcher_downloads_into_the_d3_cache_dir() {
        let (tx, _rx) = mpsc::channel();

        let fetcher = choose(None, Path::new("/home/u/.cache/tauler"), tx);

        assert_eq!(fetcher.dir, PathBuf::from("/home/u/.cache/tauler/d3"));
        assert!(fetcher.online.is_some());
        assert_eq!(fetcher.assets[0].url, D3.url);
        assert_eq!(fetcher.assets[1].url, LINKEDOM.url);
    }

    #[test]
    fn the_real_downloader_refuses_a_url_that_is_not_https() {
        let result = download_https("http://example.com/d3.min.js");

        let error = result.expect_err("an http:// URL must be refused");
        assert!(
            error.to_lowercase().contains("https"),
            "the refusal should say why: {error}"
        );
    }
}
