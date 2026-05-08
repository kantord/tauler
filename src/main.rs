use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use tauler::config::{FontConfig, TaulerConfig};
use tauler::data::data_loop::{DataLoop, StreamItem};
use tauler::init_global_ctx;
use tauler::render::set_incremental_rendering;
use tauler::windowing::wayland::WaylandDisplayServer;
use tauler::x11::panel::{i3_dpi, PanelContext};
use x11rb::{
    connection::Connection,
    protocol::{randr::ConnectionExt as RandrExt, xproto::*},
    rust_connection::RustConnection,
};

mod app;
mod presenter;
use app::{App, TickReceivers, X11Init};

const FREEZE_WATCHDOG_POLL_SECS: u64 = 10;
const FREEZE_STALE_THRESHOLD_SECS: u64 = 10;

fn detect_backend() -> &'static str {
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

fn setup_file_watchers(
    layout_jsx_path: &std::path::Path,
    config_yaml_path: &std::path::Path,
    exe_path: &std::path::Path,
    reload_tx: mpsc::Sender<()>,
    bin_reload_tx: mpsc::Sender<()>,
    dl_wake_tx: mpsc::SyncSender<()>,
) -> std::sync::Arc<std::sync::Mutex<notify::RecommendedWatcher>> {
    use notify::{EventKind, RecursiveMode, Watcher};

    let exe = exe_path.to_path_buf();
    let layout_jsx = layout_jsx_path.to_path_buf();
    let config_yaml = config_yaml_path.to_path_buf();

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
            } else if *path == layout_jsx || *path == config_yaml {
                let _ = reload_tx.send(());
                let _ = dl_wake_tx.try_send(());
            }
        }
    })
    .expect("failed to create file watcher");

    let watcher = std::sync::Arc::new(std::sync::Mutex::new(watcher));

    {
        let mut w = watcher.lock().unwrap();
        for dir in [layout_jsx_path, config_yaml_path, exe_path]
            .iter()
            .filter_map(|p| p.parent())
            .collect::<std::collections::HashSet<_>>()
        {
            if dir.exists() {
                let _ = w.watch(dir, RecursiveMode::NonRecursive);
            }
        }
    }

    watcher
}

fn load_font_config(config_path: &std::path::Path) -> FontConfig {
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|yaml| TaulerConfig::from_yaml(&yaml).ok())
        .map(|c| c.fonts)
        .unwrap_or_default()
}

fn init_x11() -> Result<X11Init, Box<dyn std::error::Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let conn = Arc::new(conn);
    let screen = conn.setup().roots[screen_num].clone();

    let dpi = i3_dpi(&conn, screen.root, &screen);
    let dpr = dpi / 96.0;

    let primary_output = conn.randr_get_output_primary(screen.root)?.reply()?.output;
    let output_info = conn.randr_get_output_info(primary_output, 0)?.reply()?;
    let output_name = String::from_utf8_lossy(&output_info.name).into_owned();

    let output_map = tauler::x11::outputs::build_output_map(&conn, screen.root);

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
    let strut_atom = conn
        .intern_atom(false, b"_NET_WM_STRUT_PARTIAL")?
        .reply()?
        .atom;
    let strut_legacy_atom = conn.intern_atom(false, b"_NET_WM_STRUT")?.reply()?.atom;

    let panel_ctx = PanelContext {
        conn: Arc::clone(&conn),
        root: screen.root,
        depth: screen.root_depth,
        root_visual: screen.root_visual,
        black_pixel: screen.black_pixel,
        dpr,
        xrootpmap_atom,
        strut_atom,
        strut_legacy_atom,
        output_map: Arc::new(output_map),
        dpi,
        output_name,
        screen_width_logical,
        screen_height_logical,
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
    let layout_jsx_path = std::path::PathBuf::from(&home).join(".config/tauler/layout.jsx");
    let config_yaml_path = std::path::PathBuf::from(&home).join(".config/tauler/config.yaml");

    let tauler_config = TaulerConfig::from_yaml(
        &std::fs::read_to_string(&config_yaml_path).unwrap_or_default()
    ).unwrap_or_default();
    let font_config = load_font_config(&config_yaml_path);
    set_incremental_rendering(tauler_config.rendering.incremental);

    let last_tick = Arc::new(std::sync::atomic::AtomicU64::new(0));
    spawn_freeze_watchdog(Arc::clone(&last_tick), log_path);

    let (dl_wake_tx, dl_wake_rx) = mpsc::sync_channel::<()>(1);

    let (reload_tx, reload_rx) = mpsc::channel::<()>();
    let (bin_reload_tx, bin_reload_rx) = mpsc::channel::<()>();
    let _watcher = setup_file_watchers(
        &layout_jsx_path,
        &config_yaml_path,
        &exe_path,
        reload_tx,
        bin_reload_tx,
        dl_wake_tx.clone(),
    );

    let (mut data_loop, handle) = DataLoop::new();
    data_loop = data_loop.with_extra_rx(dl_wake_rx);
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

    if backend == "wayland" {
        tracing::info!("display backend: Wayland");
        let server = WaylandDisplayServer::connect()?;
        let mut app = App::new_wayland(
            server,
            handle,
            rx,
            layout_jsx_path,
            config_yaml_path,
            Arc::clone(&module_event_txs),
            Arc::clone(&stop),
            Arc::clone(&last_tick),
            Arc::clone(&_watcher),
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
            layout_jsx_path,
            config_yaml_path,
            module_event_txs,
            Arc::clone(&stop),
            Arc::clone(&last_tick),
            Arc::clone(&_watcher),
        );
        data_loop.run(
            Arc::clone(&stop),
            move |item: StreamItem| {
                let _ = item_tx.send((item.key, item.line));
            },
            move || app.tick(),
        );
    }

    // run() returned because stop was set (binary reload). App::drop handles cleanup.
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
