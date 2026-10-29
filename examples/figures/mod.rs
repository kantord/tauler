//! What the d3 tracers share: getting d3 and linkedom the way tauler does, and
//! drawing a layout's `<D3Figure>`s without a main loop.
//!
//! In tauler the draw runtime runs on its own thread and a finished drawing
//! wakes the loop (`src/d3_figure/draw.rs`). A tracer has no loop, so it does
//! the same steps in line: eval the layout in a draw runtime
//! ([`JsxEvaluator::new_draw`]), [`JsxEvaluator::draw_figure`] each figure the
//! render eval named, and [`set_picture`] the markup — after which a raster of
//! the panel binds the Picture exactly as a frame on the bar does.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tauler::d3_figure::draw::TIME_LIMIT;
use tauler::d3_figure::fetch::Fetcher;
use tauler::jsx::{FigureCall, JsxEvaluator};
use tauler::pkg::{fetch_manager::FetchManager, PackageContext};
use tauler::render::picture::set_picture;

/// How long to wait for a first download of d3 and linkedom.
const DOWNLOAD_WAIT: Duration = Duration::from_secs(60);

pub fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// A PackageContext whose `"d3"` is ready to load: `TAULER_D3_DIR` if set
/// (never downloads), else `~/.cache/tauler/d3`, downloading into it on first
/// use and waiting up to [`DOWNLOAD_WAIT`]. Exits with a message if the files
/// never arrive. `base_dir` only names a Lockfile path; the examples import no
/// Packages, so it is never read.
pub fn package_context(base_dir: &Path) -> PackageContext {
    let (reload_tx, reload_rx) = mpsc::channel();
    let (fetcher, dir, offline) = match std::env::var_os("TAULER_D3_DIR").map(PathBuf::from) {
        Some(dir) => (Fetcher::offline(dir.clone()), dir, true),
        None => {
            let home = std::env::var_os("HOME").expect("HOME is set");
            let dir = PathBuf::from(home).join(".cache/tauler/d3");
            (Fetcher::new(dir.clone(), reload_tx.clone()), dir, false)
        }
    };
    let start = Instant::now();
    let mut announced = false;
    while fetcher.sources().is_none() {
        if offline {
            eprintln!(
                "TAULER_D3_DIR={} holds no verified d3.min.js + linkedom.js",
                dir.display()
            );
            std::process::exit(1);
        }
        if start.elapsed() > DOWNLOAD_WAIT {
            eprintln!(
                "d3 + linkedom did not arrive in {} within {}s (see RUST_LOG=tauler=info)",
                dir.display(),
                DOWNLOAD_WAIT.as_secs()
            );
            std::process::exit(1);
        }
        if !announced {
            println!(
                "waiting for d3 7.9.0 + linkedom 0.18.14 to download into {}",
                dir.display()
            );
            announced = true;
        }
        // A finished download sends here; a failed one sends nothing, so poll too.
        let _ = reload_rx.recv_timeout(Duration::from_millis(500));
    }
    if announced {
        println!(
            "d3 + linkedom ready after {:.1} s",
            start.elapsed().as_secs_f64()
        );
    }
    PackageContext {
        fetch_manager: Arc::new(FetchManager::new(reload_tx)),
        lockfile_path: base_dir.join("tauler-pkg.lock"),
        cache_root: base_dir.join(".data/pkg-cache"),
        d3: Arc::new(fetcher),
    }
}

/// What drawing a layout's figures cost.
pub struct Drawn {
    /// `new_draw` and its eval.
    pub setup_ms: f64,
    /// Loading d3 and linkedom into the draw runtime, once.
    pub libraries_ms: f64,
    /// Per figure, in document order: key, `draw` time, SVG length.
    pub figures: Vec<(String, f64, usize)>,
}

/// Draw every figure in `calls` in a fresh draw runtime for `source`, and store
/// each as the Picture for its key. Panics on a figure that fails to draw: a
/// tracer that silently shows an empty box would measure nothing.
pub fn draw_figures(
    source: &str,
    ctx: serde_json::Value,
    layout_path: &Path,
    pkg: &PackageContext,
    calls: &[FigureCall],
) -> Drawn {
    let t = Instant::now();
    let ev = JsxEvaluator::new_draw(source, ctx, Some(layout_path), Some(pkg.clone()))
        .expect("draw runtime compiles");
    ev.eval(&HashMap::new()).expect("draw runtime eval");
    let setup_ms = ms(t);

    // `draw_figure` loads the libraries before it looks the key up, so an
    // unknown key times the load alone.
    let t = Instant::now();
    let _ = ev.draw_figure("\0tracer-warmup", 1.0, 1.0, TIME_LIMIT);
    let libraries_ms = ms(t);

    let figures = calls
        .iter()
        .map(|call| {
            let t = Instant::now();
            let svg = ev
                .draw_figure(&call.key, call.width, call.height, TIME_LIMIT)
                .unwrap_or_else(|e| panic!("drawing {:?}: {e}", call.key));
            let took = ms(t);
            let len = svg.len();
            set_picture(&call.key, svg);
            (call.key.clone(), took, len)
        })
        .collect();
    Drawn {
        setup_ms,
        libraries_ms,
        figures,
    }
}
