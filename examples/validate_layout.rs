//! Standalone check: does the live layout.op.mdx load + JSX-compile right now,
//! with the same PackageContext production tauler uses? No X11, no subprocess,
//! no window — read-only diagnostic, safe to run alongside a live tauler.
use std::sync::{mpsc, Arc};

fn main() {
    let home = std::env::var("HOME").unwrap();
    let config_dir = std::path::PathBuf::from(&home).join(".config/tauler");
    let pkg_cache_root = std::path::PathBuf::from(&home).join(".cache/tauler/pkg");

    let source = tauler::layout_source::LayoutSource::detect(&config_dir);
    println!("layout source: {source:?}");
    let Some(source) = source else {
        println!("NO LAYOUT SOURCE FOUND");
        return;
    };

    let loaded = match tauler::layout_source::load(&source) {
        Ok(l) => {
            println!("layout load: OK ({} bytes of JS)", l.js_source.len());
            l
        }
        Err(e) => {
            println!("LAYOUT LOAD ERROR: {e}");
            return;
        }
    };

    let (reload_tx, _reload_rx) = mpsc::channel::<()>();
    let fetch_manager = Arc::new(tauler::pkg::fetch_manager::FetchManager::new(reload_tx));
    let pkg_ctx = tauler::pkg::PackageContext {
        fetch_manager,
        lockfile_path: source.dir().join("tauler-pkg.lock"),
        cache_root: pkg_cache_root,
    };

    let ctx = serde_json::json!({});
    match tauler::jsx::JsxEvaluator::new_with_packages(
        &loaded.js_source,
        ctx,
        Some(&config_dir),
        pkg_ctx,
    ) {
        Ok(_) => println!("JSX COMPILE: OK"),
        Err(e) => println!("JSX COMPILE ERROR: {e}"),
    }
}
