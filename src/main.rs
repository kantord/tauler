use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use tauler::data::data_loop::DataLoop;
#[cfg(not(target_os = "macos"))]
use tauler::data::data_loop::StreamItem;
use tauler::init_global_ctx;
#[cfg(target_os = "linux")]
use tauler::windowing::wayland::WaylandDisplayServer;
#[cfg(not(target_os = "macos"))]
use tauler::x11::panel::{i3_dpi, PanelContext};
#[cfg(not(target_os = "macos"))]
use x11rb::{
    connection::Connection,
    protocol::{randr::ConnectionExt as RandrExt, xproto::*},
    rust_connection::RustConnection,
};

mod app;
mod presenter;
use app::TickReceivers;
#[cfg(not(target_os = "macos"))]
use app::{App, X11Init};

const FREEZE_WATCHDOG_POLL_SECS: u64 = 10;
const FREEZE_STALE_THRESHOLD_SECS: u64 = 10;

fn detect_backend() -> &'static str {
    if cfg!(target_os = "macos") {
        return "macos";
    }
    if let Ok(b) = std::env::var("TAULER_BACKEND") {
        if b == "wayland" {
            return "wayland";
        }
        return "x11";
    }
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        "wayland"
    } else {
        "x11"
    }
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

fn install_panic_hook(log_path: String) {
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("PANIC: {info}");
        tracing::error!("{msg}");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = writeln!(f, "{msg}");
        }
    }));
}

fn spawn_freeze_watchdog(last_tick: Arc<std::sync::atomic::AtomicU64>, log_path: String) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(FREEZE_WATCHDOG_POLL_SECS));
        let last = last_tick.load(Ordering::Relaxed);
        if last == 0 {
            continue;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let stale = now.saturating_sub(last);
        if stale > FREEZE_STALE_THRESHOLD_SECS {
            let msg = format!("FREEZE: main loop stalled for {stale}s");
            tracing::error!("{msg}");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                use std::io::Write;
                let _ = writeln!(f, "{msg}");
            }
        }
    });
}

/// Watches for both layout formats at once — `layout.op.mdx`, and the legacy
/// `layout.jsx` + `config.yaml` pair — regardless of which one is actually active. This
/// is what lets a config created after tauler booted with nothing configured still be
/// picked up (`App::handle_layout_reload` re-detects while nothing was found yet); it is
/// also just as correct for the common case where one of the three never appears at all.
fn setup_file_watchers(
    config_dir: &std::path::Path,
    exe_path: &std::path::Path,
    reload_tx: mpsc::Sender<()>,
    bin_reload_tx: mpsc::Sender<()>,
    dl_wake_tx: mpsc::SyncSender<()>,
) -> std::sync::Arc<std::sync::Mutex<notify::RecommendedWatcher>> {
    use notify::{EventKind, RecursiveMode, Watcher};

    let exe = exe_path.to_path_buf();
    let watched_layout_paths = [
        config_dir.join("layout.op.mdx"),
        config_dir.join("layout.jsx"),
        config_dir.join("config.yaml"),
    ];

    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        match event.kind {
            EventKind::Modify(_) | EventKind::Create(_) => {}
            _ => return,
        }
        for path in &event.paths {
            if *path == exe {
                let _ = bin_reload_tx.send(());
                let _ = dl_wake_tx.try_send(());
            } else if watched_layout_paths.contains(path) {
                let _ = reload_tx.send(());
                let _ = dl_wake_tx.try_send(());
            }
        }
    })
    .expect("failed to create file watcher");

    let watcher = std::sync::Arc::new(std::sync::Mutex::new(watcher));

    {
        let mut w = watcher.lock().unwrap();
        let dirs: std::collections::HashSet<&std::path::Path> =
            [Some(config_dir), exe_path.parent()]
                .into_iter()
                .flatten()
                .collect();
        for dir in dirs {
            if dir.exists() {
                let _ = w.watch(dir, RecursiveMode::NonRecursive);
            }
        }
    }

    watcher
}

#[cfg(not(target_os = "macos"))]
fn init_x11() -> Result<X11Init, Box<dyn std::error::Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let conn = Arc::new(conn);
    let screen = conn.setup().roots[screen_num].clone();

    let dpi = i3_dpi(&conn, screen.root, &screen);
    let dpr = dpi / 96.0;

    let output_map = tauler::x11::outputs::build_output_map(&conn, screen.root);

    // RandR answers 0 when no output is marked primary, and `GetOutputInfo(0)`
    // is a protocol error — so a bare X server (Xvfb, a session started by hand)
    // used to take tauler down before it drew anything. The name is only used to
    // look up a logical screen size below, which already has a fallback, so
    // there is nothing here worth failing over.
    let primary_output = conn.randr_get_output_primary(screen.root)?.reply()?.output;
    let output_name = (primary_output != 0)
        .then(|| {
            conn.randr_get_output_info(primary_output, 0)
                .ok()?
                .reply()
                .ok()
        })
        .flatten()
        .map(|info| String::from_utf8_lossy(&info.name).into_owned())
        .or_else(|| tauler::x11::outputs::fallback_output_name(&output_map))
        .unwrap_or_default();

    let root_screen_width = screen.width_in_pixels as u32;
    let root_screen_height = screen.height_in_pixels as u32;

    let (screen_width_logical, screen_height_logical) = output_map
        .get(&output_name)
        .map(|o| {
            (
                (o.width as f32 / dpr).round() as u32,
                (o.height as f32 / dpr).round() as u32,
            )
        })
        .unwrap_or((
            screen.width_in_pixels as u32,
            screen.height_in_pixels as u32,
        ));

    conn.change_window_attributes(
        screen.root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    let xrootpmap_atom: Option<u32> = conn
        .intern_atom(false, b"_XROOTPMAP_ID")
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.atom);
    let panel_ctx = PanelContext {
        conn: Arc::clone(&conn),
        root: screen.root,
        depth: screen.root_depth,
        root_visual: screen.root_visual,
        black_pixel: screen.black_pixel,
        dpr,
        xrootpmap_atom,
        output_map: Arc::new(output_map),
        dpi,
        output_name,
        screen_width_logical,
        screen_height_logical,
        root_screen_width,
        root_screen_height,
        root_bg: None,
    };

    let jsx_ctx = serde_json::json!({
        "output": panel_ctx.output_name,
        "dpi": dpi,
        "screen_width": screen_width_logical,
        "screen_height": screen_height_logical,
    });

    Ok(X11Init { panel_ctx, jsx_ctx })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let log_path = {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.local/share/tauler-crash.log")
    };
    install_panic_hook(log_path.clone());

    let exe_path = std::env::current_exe().unwrap_or_default();

    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::path::PathBuf::from(&home).join(".config/tauler");

    // `layout.op.mdx` if it exists, else the legacy `layout.jsx` + `config.yaml` pair,
    // else nothing configured yet (`docs/adr/0036`) — decided once, here, for the
    // process's lifetime; a reload only re-detects if this is `None`.
    let layout_source = tauler::layout_source::LayoutSource::detect(&config_dir);
    let font_config = app::load_layout_or_exit(layout_source.as_ref())
        .map(|l| l.config.fonts)
        .unwrap_or_default();

    let last_tick = Arc::new(std::sync::atomic::AtomicU64::new(0));
    spawn_freeze_watchdog(Arc::clone(&last_tick), log_path);

    // `mut` only on the platforms that call `run` on it here; macOS hands it to
    // the presenter, which owns the main thread and runs the loop on a worker.
    #[cfg_attr(target_os = "macos", allow(unused_mut))]
    let (mut data_loop, handle) = DataLoop::new();
    // Everything that has work for the loop holds one of these; it is what ends
    // the loop's wait, so nothing sits in a channel until the supervision timer.
    let notifier = data_loop.notifier();

    let (reload_tx, reload_rx) = mpsc::channel::<()>();
    let (bin_reload_tx, bin_reload_rx) = mpsc::channel::<()>();
    let _watcher = setup_file_watchers(
        &config_dir,
        &exe_path,
        reload_tx,
        bin_reload_tx,
        notifier.clone(),
    );

    let module_event_txs = data_loop.event_txs_handle();

    let (item_tx, item_rx) = mpsc::channel::<((String, Option<String>), String)>();
    let stop = Arc::new(AtomicBool::new(false));
    let rx = TickReceivers {
        item_rx,
        bin_reload_rx,
        reload_rx,
    };
    let backend = detect_backend();

    init_global_ctx(font_config);

    // AppKit owns the main thread, so `App` moves to a worker thread.
    #[cfg(target_os = "macos")]
    presenter::macos::run(presenter::macos::MacBoot {
        data_loop,
        handle,
        rx,
        item_tx,
        config_dir,
        layout_source,
        module_event_txs,
        stop: Arc::clone(&stop),
        last_tick,
        watcher: Arc::clone(&_watcher),
    })?;

    #[cfg(not(target_os = "macos"))]
    if backend == "wayland" {
        tracing::info!("display backend: Wayland");
        let server = WaylandDisplayServer::connect()?;
        let mut app = App::new_wayland(
            server,
            handle,
            rx,
            config_dir,
            layout_source,
            Arc::clone(&module_event_txs),
            Arc::clone(&stop),
            Arc::clone(&last_tick),
            Arc::clone(&_watcher),
            notifier.clone(),
        );
        data_loop.run(
            Arc::clone(&stop),
            move |item: StreamItem| {
                let _ = item_tx.send((item.key, item.line));
            },
            move || app.tick(),
        );
    } else {
        tracing::info!("display backend: X11");
        let x11 = init_x11()?;
        let mut app = App::new_x11(
            x11,
            handle,
            rx,
            config_dir,
            layout_source,
            module_event_txs,
            Arc::clone(&stop),
            Arc::clone(&last_tick),
            Arc::clone(&_watcher),
            notifier.clone(),
        );
        data_loop.run(
            Arc::clone(&stop),
            move |item: StreamItem| {
                let _ = item_tx.send((item.key, item.line));
            },
            move || app.tick(),
        );
    }

    // run() returned because stop was set (binary reload). App::drop has taken
    // the surfaces and the presenter down; the subprocesses are separate and
    // have to go before `exec`, which keeps the PID and runs no destructors.
    // Anything still tracked would survive into the new image as a child it
    // holds no handle to, and so could never be reaped — a permanent zombie the
    // moment it exits.
    // macOS moved the loop into the presenter's worker thread, which shuts it
    // down there instead — `data_loop` is not ours to touch by this point.
    #[cfg(not(target_os = "macos"))]
    data_loop.shutdown();

    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(&exe_path);
    cmd.env("TAULER_BACKEND", backend);
    if let Ok(mtime) = std::fs::metadata(&exe_path).and_then(|m| m.modified()) {
        if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
            cmd.env("TAULER_EXE_MTIME_NS", dur.as_nanos().to_string());
        }
    }
    let _ = cmd.exec();

    Ok(())
}
