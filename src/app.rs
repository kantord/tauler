use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use notify::Watcher;
use tauler::config::TaulerConfig;
use tauler::data::data_loop::{
    BuiltInSource, DataLoopHandle, ProcessIdentity, ProcessSpec, Resource, StreamSource,
};
use tauler::hit_test::hit_test;
use tauler::layout::OutputInfo;
use tauler::managed_set::{Lifecycle, OptativeSet, Reconcile};
use tauler::outbox::Outbox;
use tauler::pointer::{read_handler, Capture, Handler};
#[cfg(not(target_os = "macos"))]
use tauler::presentation::PresentationThread;
// Only the constructors that spawn a presenter thread build one of these, and
// macOS has none: there the presenter owns the main thread and makes its own.
#[cfg(not(target_os = "macos"))]
use tauler::presentation::PresenterEvents;
use tauler::presentation::{PointerEvent, PointerPhase, PresenterEvent, SurfaceCommand};
use tauler::surface::{SurfaceOutputs, SurfaceSets};
use tauler::theme::resolver::resolve_theme_tokens;
use tauler::theme::{Theme, ThemeMode};
#[cfg(target_os = "linux")]
use tauler::windowing::wayland::WaylandDisplayServer;
#[cfg(not(target_os = "macos"))]
use tauler::x11::panel::PanelContext;

#[cfg(target_os = "linux")]
use crate::presenter::wayland::run_wayland_presenter_thread;
#[cfg(not(target_os = "macos"))]
use crate::presenter::x11::run_x11_presenter_thread;

pub(crate) type ModuleEventTxs =
    Arc<std::sync::Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>;
pub(crate) type SharedWatcher = Arc<std::sync::Mutex<notify::RecommendedWatcher>>;

struct WatchedPath(std::path::PathBuf);

impl std::fmt::Display for WatchedPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl Lifecycle for WatchedPath {
    type Key = std::path::PathBuf;
    type State = std::path::PathBuf;
    type Context = SharedWatcher;
    type Output = ();
    type Error = notify::Error;

    fn key(&self) -> std::path::PathBuf {
        self.0.clone()
    }

    fn display_name(&self) -> String {
        self.0.display().to_string()
    }

    fn enter(
        self,
        ctx: &mut SharedWatcher,
        _: &mut (),
    ) -> Result<std::path::PathBuf, notify::Error> {
        ctx.lock()
            .unwrap()
            .watch(&self.0, notify::RecursiveMode::NonRecursive)?;
        Ok(self.0)
    }

    fn reconcile_self(
        self,
        _: &mut std::path::PathBuf,
        _: &mut SharedWatcher,
        _: &mut (),
    ) -> Result<(), notify::Error> {
        Ok(())
    }

    fn exit(
        state: std::path::PathBuf,
        ctx: &mut SharedWatcher,
        _: &mut (),
    ) -> Result<(), notify::Error> {
        ctx.lock().unwrap().unwatch(&state)
    }
}

fn log_lifecycle_errors<K: std::fmt::Debug, E: std::fmt::Debug>(
    errors: tauler::managed_set::ReconcileErrors<K, E>,
) {
    for (key, err) in errors {
        tracing::error!(key = ?key, error = ?err, "lifecycle error");
    }
}

fn theme_file_watch_desired(path: Option<std::path::PathBuf>) -> Vec<WatchedPath> {
    match path {
        Some(p) => vec![WatchedPath(p)],
        None => vec![],
    }
}

fn make_builtin(key: &str) -> Option<BuiltInSource> {
    use tauler::x11::outputs::outputs_thread;
    match key {
        "tauler:outputs" => Some(BuiltInSource {
            key: key.to_string(),
            func: outputs_thread,
        }),
        _ => None,
    }
}

pub(crate) fn stream_calls_to_specs(calls: &[(String, Option<String>)]) -> Vec<StreamSource> {
    calls
        .iter()
        .map(|(bin, script)| {
            if let Some(builtin) = make_builtin(bin) {
                return StreamSource::BuiltIn(builtin);
            }
            let args = match script {
                Some(content) => vec![Resource::File {
                    content: content.clone(),
                }],
                None => vec![],
            };
            StreamSource::Process(ProcessSpec {
                identity: ProcessIdentity {
                    bin: bin.clone(),
                    key: format!("{}:{}", bin, script.as_deref().unwrap_or("")),
                },
                args,
                env: std::collections::BTreeMap::new(),
                current_dir: None,
                props: None,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn apply_eval_result(
    out: &tauler::jsx::EvalOutput,
    dpr: f32,
    primary_output_name: &str,
    output_map: &HashMap<String, OutputInfo>,
    handle: &DataLoopHandle,
    surface_set: &mut SurfaceSets,
    outputs: &mut SurfaceOutputs,
    mod_init_fn: &dyn Fn() -> serde_json::Value,
) -> bool {
    let mut specs = match tauler::parse_root_node(&out.layout) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "root node parse error");
            return false;
        }
    };
    // Panels whose output isn't in the map don't exist yet — skip them silently.
    // "output not specified" means primary output; same rule applies.
    specs.retain(|spec| {
        let name = spec.output.as_deref().unwrap_or(primary_output_name);
        output_map.contains_key(name)
    });
    for spec in &mut specs {
        let name = spec
            .output
            .as_deref()
            .unwrap_or(primary_output_name)
            .to_string();
        let out = output_map.get(&name);
        spec.dpr = out.map(|o| o.dpr).unwrap_or(dpr);
        // Resolve "unspecified" to the primary output's real name, so a panel
        // and a wallpaper that mean the same monitor agree on one key — that is
        // how `backdrop` pairs them up.
        spec.output = Some(name.clone());
        if spec.kind == tauler::SurfaceKind::Wallpaper {
            if let Some(out) = out {
                // A wallpaper is always exactly its display: geometry comes from
                // the output, never from the layout file. Tracking the origin here
                // too means a monitor that moves shows up as a spec change, which
                // is what triggers the repaint.
                spec.width = (out.width as f32 / spec.dpr).round() as u32;
                spec.height = (out.height as f32 / spec.dpr).round() as u32;
                spec.x = out.x as i32;
                spec.y = out.y as i32;
            }
        }
    }
    let mod_init = mod_init_fn();

    let module_bins: std::collections::HashSet<String> =
        out.module_calls.iter().map(|(b, _)| b.clone()).collect();
    let stream_specs = stream_calls_to_specs(&out.stream_calls)
        .into_iter()
        .filter(|s| match s {
            StreamSource::Process(p) => !module_bins.contains(&p.identity.bin),
            StreamSource::BuiltIn(_) => true,
        })
        .collect::<Vec<_>>();
    let module_specs: Vec<StreamSource> = out
        .module_calls
        .iter()
        .map(|(bin, jsx_props)| {
            StreamSource::Process(ProcessSpec {
                identity: ProcessIdentity {
                    bin: bin.clone(),
                    key: bin.clone(),
                },
                args: vec![],
                env: std::collections::BTreeMap::new(),
                current_dir: None,
                props: Some(merge_module_props(&mod_init, jsx_props)),
            })
        })
        .collect();
    let combined: Vec<StreamSource> = stream_specs.into_iter().chain(module_specs).collect();
    handle.set_desired(combined);

    let surface_errors = surface_set.reconcile_all(specs, outputs);
    log_lifecycle_errors(surface_errors);
    true
}

/// Merge a module's JSX-declared props (`<Module bin=".." gaps={{..}}>`) into
/// the derived init payload. Init keys win: that payload is the module
/// protocol, not user-editable state.
fn merge_module_props(
    mod_init: &serde_json::Value,
    jsx_props: &serde_json::Value,
) -> serde_json::Value {
    let (Some(init_map), Some(jsx_map)) = (mod_init.as_object(), jsx_props.as_object()) else {
        return mod_init.clone();
    };
    let mut merged = jsx_map.clone();
    for (k, v) in init_map {
        merged.insert(k.clone(), v.clone());
    }
    serde_json::Value::Object(merged)
}

/// The environment every module is told about at startup.
///
/// Facts only. tauler used to also derive `config: {left, right, outer_gap}` —
/// its guess at the i3 gaps the panels imply — and send it to *every* module,
/// including ones with no idea what it meant. That guess could never be right:
/// i3 reserves nothing for override-redirect windows, and has no left/right
/// dock concept at all (`W_DOCK_TOP`/`W_DOCK_BOTTOM` are its only variants), so
/// what to reserve is a decision, not a consequence of geometry. The layout
/// file now states it via `<Module gaps={{...}}>`, which passes through
/// untouched.
fn make_mod_init_value(
    output_name: &str,
    dpi: f32,
    screen_width_logical: u32,
    screen_height_logical: u32,
) -> serde_json::Value {
    serde_json::json!({
        "type": "init",
        "output": output_name,
        "dpi": dpi,
        "screen_width": screen_width_logical,
        "screen_height": screen_height_logical,
    })
}

// ---------------------------------------------------------------------------
// App — non-generic, DM lives on the presenter thread
// ---------------------------------------------------------------------------

pub(crate) struct TickReceivers {
    pub(crate) item_rx: mpsc::Receiver<((String, Option<String>), String)>,
    pub(crate) bin_reload_rx: mpsc::Receiver<()>,
    pub(crate) reload_rx: mpsc::Receiver<()>,
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct X11Init {
    pub(crate) panel_ctx: PanelContext,
    pub(crate) jsx_ctx: serde_json::Value,
}

/// macOS reports a backing scale factor, not DPI, so `dpi = dpr * 96`.
#[cfg(target_os = "macos")]
pub(crate) const DEFAULT_DPI: f32 = 96.0;

#[cfg(target_os = "macos")]
pub(crate) struct MacInit {
    pub(crate) command_tx: mpsc::Sender<SurfaceCommand>,
    pub(crate) event_rx: mpsc::Receiver<PresenterEvent>,
    pub(crate) screen_width_logical: u32,
    pub(crate) screen_height_logical: u32,
    pub(crate) dpr: f32,
    pub(crate) output_name: String,
}

pub(crate) struct App {
    theme: Theme,
    theme_mode: ThemeMode,
    /// The directory a layout is resolved from — `~/.config/tauler` in practice. Kept
    /// unconditionally (unlike `layout_source`, which may be `None`) so a reload can
    /// retry detection if nothing was found at boot.
    config_dir: std::path::PathBuf,
    /// Which files the layout is made of, chosen once — `None` if neither format was
    /// found. Re-detected on a reload only while still `None`; once a format is found it
    /// is locked in for the process's lifetime (`docs/adr/0036`: switching formats needs
    /// a restart).
    layout_source: Option<tauler::layout_source::LayoutSource>,
    dpr: f32,
    dpi: f32,
    output_name: String,
    screen_width_logical: u32,
    screen_height_logical: u32,
    output_map: HashMap<String, OutputInfo>,
    surfaces: SurfaceSets,
    import_watches: OptativeSet<WatchedPath>,
    theme_file_watch: OptativeSet<WatchedPath>,
    watcher: SharedWatcher,
    /// Shared with the reconciler thread, which evaluates the same layout file
    /// against the same data on its own schedule (ADR 0034).
    stream_values: tauler::units::SharedStreamValues,
    jsx_evaluator: Option<tauler::jsx::JsxEvaluator>,
    /// Dropped and respawned on every layout reload; `None` until the first
    /// layout loads.
    reconciler: Option<tauler::units::Reconciler>,
    handle: DataLoopHandle,
    jsx_ctx: serde_json::Value,
    item_rx: mpsc::Receiver<((String, Option<String>), String)>,
    bin_reload_rx: mpsc::Receiver<()>,
    reload_rx: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    last_tick: Arc<std::sync::atomic::AtomicU64>,
    outputs: SurfaceOutputs,
    event_rx: mpsc::Receiver<PresenterEvent>,
    module_event_txs: ModuleEventTxs,
    /// The drag in progress, if any: the box it was pressed in and what it last sent
    /// (`docs/adr/0020`). The handler itself lives in the JS capture slot.
    capture: Option<Capture>,
    /// Keeps one intent in flight per module, so a module slower than the
    /// pointer is never handed a queue it cannot drain.
    outbox: Outbox,
    presenter_thread: Option<thread::JoinHandle<()>>,
    /// Per-panel accessibility adapters, pushed to an AT on repaint (ADR 0038/0039).
    #[cfg(target_os = "linux")]
    a11y: tauler::a11y::A11y,
    /// Activations an AT raised on its own thread, reduced to `(panel_id, path)`
    /// and drained here on the tick thread so they reach `resolve`/`send` (ADR 0038).
    #[cfg(target_os = "linux")]
    a11y_rx: mpsc::Receiver<(String, Vec<usize>)>,
}

/// Start the render worker and return the two channels a reconciled surface
/// writes to.
///
/// The worker is not joined on shutdown and holds nothing but memory: it may be
/// part-way through a rasterization that no longer matters, and waiting for
/// those pixels would only delay the re-exec. Dropping [`SurfaceOutputs`] closes
/// its job channel, which is what tells it to stop.
fn spawn_render_worker(command_tx: mpsc::Sender<SurfaceCommand>) -> SurfaceOutputs {
    let (jobs, job_rx) = mpsc::channel();
    let commands = command_tx.clone();
    thread::spawn(move || tauler::render::worker::run(job_rx, command_tx));
    SurfaceOutputs { commands, jobs }
}

/// The layout and config a [`tauler::layout_source::LayoutSource`] resolves to right now,
/// or `None` when nothing is configured or the layout/JS itself is unreadable — the
/// caller draws nothing rather than crash, same as a missing file always has.
///
/// An unusable *config* (invalid frontmatter, or invalid `config.yaml` on the legacy
/// path) is different: it ends the process, same reasoning `docs/adr/0033` gives for an
/// unusable `theme.file`, extended in `docs/adr/0036` to the whole config. This is a
/// startup-only policy — call it from a constructor or `initial_load`, never from a
/// reload handler, which must never exit.
pub(crate) fn load_layout_or_exit(
    source: Option<&tauler::layout_source::LayoutSource>,
) -> Option<tauler::layout_source::LoadedLayout> {
    let Some(source) = source else {
        tracing::error!("no layout file found (checked layout.op.mdx and layout.jsx)");
        return None;
    };
    match tauler::layout_source::load(source) {
        Ok(loaded) => Some(loaded),
        Err(
            e @ (tauler::layout_source::LayoutLoadError::Config { .. }
            | tauler::layout_source::LayoutLoadError::Frontmatter { .. }),
        ) => {
            tracing::error!(error = %e, "unusable layout config");
            eprintln!("tauler: {e}");
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!(error = %e, "layout load error");
            None
        }
    }
}

/// Why a `theme.file` the config named could not be turned into a [`Theme`].
#[derive(Debug)]
enum ThemeLoadError {
    Read {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: std::path::PathBuf,
        source: serde_yaml::Error,
    },
}

impl std::fmt::Display for ThemeLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "cannot read theme file {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "invalid theme YAML in {}: {source}", path.display())
            }
        }
    }
}

/// What a loaded config asks tauler to render with: the mode, and the theme file to read if
/// one is named.
///
/// Reading that file is [`load_theme`]'s job. The two are separate because naming no file is
/// not a failure while naming one tauler cannot read is, and only the caller knows which of
/// those it can survive.
fn theme_selection(config: &TaulerConfig) -> (ThemeMode, Option<std::path::PathBuf>) {
    let file = config
        .theme
        .file
        .as_deref()
        .map(tauler::config::expand_tilde);
    (config.theme.mode, file)
}

/// The theme a `theme.file` names, or the shipped default when it names none.
///
/// A file that is named but unusable is an error rather than a fall back to the default —
/// the default palette is chroma 0 on every token, so substituting it renders a bar that
/// looks deliberate in the wrong colours. What to do about that error is the caller's, and
/// differs between startup and a reload (`docs/adr/0033`).
fn load_theme(file: Option<&std::path::Path>) -> Result<Theme, ThemeLoadError> {
    let Some(path) = file else {
        return Ok(Theme::default_theme());
    };
    let source = std::fs::read_to_string(path).map_err(|source| ThemeLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Theme::from_yaml(&source).map_err(|source| ThemeLoadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// The theme to start with, or no tauler at all (`docs/adr/0033`).
///
/// Startup has no palette on screen to keep, so an unusable `theme.file` ends the process.
/// The reason goes to stderr as well as the log, because a bar that never appears is read at
/// the terminal it was launched from.
fn load_theme_or_exit(file: Option<&std::path::Path>) -> Theme {
    match load_theme(file) {
        Ok(theme) => theme,
        Err(e) => {
            tracing::error!(error = %e, "unusable theme file");
            eprintln!("tauler: {e}");
            std::process::exit(1);
        }
    }
}

/// The theme to render with after a config reload: the newly loaded one, or the one already on
/// screen when the file went bad (`docs/adr/0033`).
///
/// Unlike startup this never exits. A reload fires on every write to the file, so an editor
/// saving it half-written is an ordinary event, and ending a running bar over one would be a
/// worse failure than the stale palette it replaces.
fn theme_after_reload(current: &Theme, loaded: Result<Theme, ThemeLoadError>) -> Theme {
    match loaded {
        Ok(theme) => theme,
        Err(e) => {
            tracing::error!(error = %e, "unusable theme file, keeping the theme already in use");
            current.clone()
        }
    }
}

impl App {
    #[cfg(not(target_os = "macos"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_x11(
        x11: X11Init,
        handle: DataLoopHandle,
        rx: TickReceivers,
        config_dir: std::path::PathBuf,
        layout_source: Option<tauler::layout_source::LayoutSource>,
        module_event_txs: ModuleEventTxs,
        stop: Arc<AtomicBool>,
        last_tick: Arc<std::sync::atomic::AtomicU64>,
        watcher: SharedWatcher,
        notifier: mpsc::SyncSender<()>,
    ) -> Self {
        let X11Init { panel_ctx, jsx_ctx } = x11;
        let dpr = panel_ctx.dpr;
        let dpi = panel_ctx.dpi;
        let output_name = panel_ctx.output_name.clone();
        let screen_width_logical = panel_ctx.screen_width_logical;
        let screen_height_logical = panel_ctx.screen_height_logical;
        let output_map: HashMap<String, OutputInfo> = (*panel_ctx.output_map).clone();
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let events = PresenterEvents::new(event_tx, notifier.clone());
        let pt = PresentationThread::new(panel_ctx);
        let presenter_thread = thread::spawn(move || {
            run_x11_presenter_thread(pt, command_rx, events);
        });
        let config = load_layout_or_exit(layout_source.as_ref())
            .map(|l| l.config)
            .unwrap_or_default();
        let (theme_mode, theme_file_path) = theme_selection(&config);
        let theme = load_theme_or_exit(theme_file_path.as_deref());
        #[cfg(target_os = "linux")]
        let (a11y, a11y_rx) = tauler::a11y::A11y::new(notifier);
        let mut state = Self {
            theme,
            theme_mode,
            config_dir,
            layout_source,
            dpr,
            dpi,
            output_name,
            screen_width_logical,
            screen_height_logical,
            output_map,
            surfaces: SurfaceSets::new(),
            import_watches: OptativeSet::new(),
            theme_file_watch: OptativeSet::new(),
            watcher,
            stream_values: Default::default(),
            reconciler: None,
            jsx_evaluator: None,
            handle,
            jsx_ctx,
            item_rx: rx.item_rx,
            bin_reload_rx: rx.bin_reload_rx,
            reload_rx: rx.reload_rx,
            stop,
            last_tick,
            outputs: spawn_render_worker(command_tx),
            event_rx,
            module_event_txs,
            capture: None,
            outbox: Outbox::new(),
            presenter_thread: Some(presenter_thread),
            #[cfg(target_os = "linux")]
            a11y,
            #[cfg(target_os = "linux")]
            a11y_rx,
        };
        state.initial_load();
        state.reconcile_theme_file_watch(theme_file_path);
        state
    }

    #[cfg(target_os = "linux")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_wayland(
        server: WaylandDisplayServer,
        handle: DataLoopHandle,
        rx: TickReceivers,
        config_dir: std::path::PathBuf,
        layout_source: Option<tauler::layout_source::LayoutSource>,
        module_event_txs: ModuleEventTxs,
        stop: Arc<AtomicBool>,
        last_tick: Arc<std::sync::atomic::AtomicU64>,
        watcher: SharedWatcher,
        notifier: mpsc::SyncSender<()>,
    ) -> Self {
        let (screen_width, screen_height) = server.primary_output_size().unwrap_or((1920, 1080));
        let initial_dpr = server.primary_output_scale();
        let jsx_ctx = serde_json::json!({
            "output": "wayland",
            "dpi": 96.0,
            "screen_width": screen_width,
            "screen_height": screen_height,
        });
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let events = PresenterEvents::new(event_tx, notifier.clone());
        let pt = PresentationThread::new(server);
        let presenter_thread = thread::spawn(move || {
            run_wayland_presenter_thread(pt, command_rx, events);
        });
        let config = load_layout_or_exit(layout_source.as_ref())
            .map(|l| l.config)
            .unwrap_or_default();
        let (theme_mode, theme_file_path) = theme_selection(&config);
        let theme = load_theme_or_exit(theme_file_path.as_deref());
        let (a11y, a11y_rx) = tauler::a11y::A11y::new(notifier);
        let mut state = Self {
            theme,
            theme_mode,
            config_dir,
            layout_source,
            dpr: initial_dpr,
            dpi: 96.0,
            output_name: String::new(),
            screen_width_logical: screen_width,
            screen_height_logical: screen_height,
            output_map: HashMap::new(),
            surfaces: SurfaceSets::new(),
            import_watches: OptativeSet::new(),
            theme_file_watch: OptativeSet::new(),
            watcher,
            stream_values: Default::default(),
            reconciler: None,
            jsx_evaluator: None,
            handle,
            jsx_ctx,
            item_rx: rx.item_rx,
            bin_reload_rx: rx.bin_reload_rx,
            reload_rx: rx.reload_rx,
            stop,
            last_tick,
            outputs: spawn_render_worker(command_tx),
            event_rx,
            module_event_txs,
            capture: None,
            outbox: Outbox::new(),
            presenter_thread: Some(presenter_thread),
            a11y,
            a11y_rx,
        };
        state.initial_load();
        state.reconcile_theme_file_watch(theme_file_path);
        state
    }

    /// Spawns no presenter thread: on macOS the presenter is already on the
    /// main thread, so the channel ends are passed in.
    #[cfg(target_os = "macos")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_macos(
        mac: MacInit,
        handle: DataLoopHandle,
        rx: TickReceivers,
        config_dir: std::path::PathBuf,
        layout_source: Option<tauler::layout_source::LayoutSource>,
        module_event_txs: ModuleEventTxs,
        stop: Arc<AtomicBool>,
        last_tick: Arc<std::sync::atomic::AtomicU64>,
        watcher: SharedWatcher,
    ) -> Self {
        let MacInit {
            command_tx,
            event_rx,
            screen_width_logical,
            screen_height_logical,
            dpr,
            output_name,
        } = mac;
        let jsx_ctx = serde_json::json!({
            "output": output_name,
            "dpi": dpr * DEFAULT_DPI,
            "screen_width": screen_width_logical,
            "screen_height": screen_height_logical,
        });
        // `apply_eval_result` silently drops panels whose output is missing here.
        let output_map = HashMap::from([(
            output_name.clone(),
            OutputInfo {
                name: output_name.clone(),
                x: 0,
                y: 0,
                // Physical pixels, as randr reports them: a `<wallpaper>` takes
                // its size from here and is rasterized at that size.
                width: (screen_width_logical as f32 * dpr).round() as u32,
                height: (screen_height_logical as f32 * dpr).round() as u32,
                dpr,
            },
        )]);
        let config = load_layout_or_exit(layout_source.as_ref())
            .map(|l| l.config)
            .unwrap_or_default();
        let (theme_mode, theme_file_path) = theme_selection(&config);
        let theme = load_theme_or_exit(theme_file_path.as_deref());
        let mut state = Self {
            theme,
            theme_mode,
            config_dir,
            layout_source,
            dpr,
            dpi: dpr * DEFAULT_DPI,
            output_name,
            screen_width_logical,
            screen_height_logical,
            output_map,
            surfaces: SurfaceSets::new(),
            import_watches: OptativeSet::new(),
            theme_file_watch: OptativeSet::new(),
            watcher,
            stream_values: Default::default(),
            reconciler: None,
            jsx_evaluator: None,
            handle,
            jsx_ctx,
            item_rx: rx.item_rx,
            bin_reload_rx: rx.bin_reload_rx,
            reload_rx: rx.reload_rx,
            stop,
            last_tick,
            outputs: spawn_render_worker(command_tx),
            event_rx,
            module_event_txs,
            capture: None,
            outbox: Outbox::new(),
            presenter_thread: None,
        };
        state.initial_load();
        state.reconcile_theme_file_watch(theme_file_path);
        state
    }

    fn apply_eval_result_dispatch(&mut self, out: &tauler::jsx::EvalOutput) -> bool {
        let mut layout = out.layout.clone();
        resolve_theme_tokens(&mut layout, &self.theme, self.theme_mode);
        // An `<img src="…">` naming a file has to be read off disk and put in
        // the render context's image store before the frame is built; takumi
        // resolves `src` against that store and has no filesystem of its own.
        // Without this an `<img>` renders as nothing at all — silently, since
        // a missing file and an undecodable one are both just an absent key.
        //
        // Per tick, but not per read: the loader skips any src already in the
        // store, so this is one read per distinct path for the process's life.
        tauler::preload_layout_images(&layout);
        let resolved_out = tauler::jsx::EvalOutput {
            layout,
            stream_calls: out.stream_calls.clone(),
            module_calls: out.module_calls.clone(),
        };
        let (dpr, dpi, sw, sh) = (
            self.dpr,
            self.dpi,
            self.screen_width_logical,
            self.screen_height_logical,
        );
        let output_name = self.output_name.clone();
        apply_eval_result(
            &resolved_out,
            dpr,
            &self.output_name,
            &self.output_map,
            &self.handle,
            &mut self.surfaces,
            &mut self.outputs,
            &move || make_mod_init_value(&output_name, dpi, sw, sh),
        )
    }

    fn reconcile_watch_set(
        set: &mut OptativeSet<WatchedPath>,
        desired: impl IntoIterator<Item = WatchedPath>,
        watcher: &mut SharedWatcher,
    ) {
        log_lifecycle_errors(set.reconcile(desired, watcher, &mut ()));
    }

    fn reconcile_import_watches(&mut self, paths: Vec<std::path::PathBuf>) {
        Self::reconcile_watch_set(
            &mut self.import_watches,
            paths.into_iter().map(WatchedPath),
            &mut self.watcher,
        );
    }

    fn reconcile_theme_file_watch(&mut self, path: Option<std::path::PathBuf>) {
        Self::reconcile_watch_set(
            &mut self.theme_file_watch,
            theme_file_watch_desired(path),
            &mut self.watcher,
        );
    }

    fn initial_load(&mut self) {
        let Some(loaded) = load_layout_or_exit(self.layout_source.as_ref()) else {
            return;
        };
        let source = loaded.js_source;
        let t = std::time::Instant::now();
        let base_dir = self.config_dir.clone();
        let evaluator =
            match tauler::jsx::JsxEvaluator::new(&source, self.jsx_ctx.clone(), Some(&base_dir)) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!(error = %e, "JSX compile error");
                    return;
                }
            };
        let loaded = evaluator.loaded_paths();
        let eval_out = evaluator.eval(&self.stream_values.read().unwrap());
        match eval_out {
            Ok(out) => {
                tracing::debug!(elapsed_ms = t.elapsed().as_millis(), "jsx eval");
                self.apply_eval_result_dispatch(&out);
                self.jsx_evaluator = Some(evaluator);
                self.reconcile_import_watches(loaded);
                self.spawn_reconciler(&source, &base_dir);
            }
            Err(e) => tracing::error!(error = %e, "JSX eval error"),
        }
    }

    /// Hand the layout file to a fresh reconciler thread, stopping the one it
    /// replaces first.
    ///
    /// This happens whether or not the layout declares a Unit, because only the
    /// reconciler runtime can answer that and a layout can start declaring one
    /// between two Ticks. The cost of being wrong is one module evaluation per
    /// load, which ADR 0034 already accounts for; the cost of guessing wrong the
    /// other way would be a Unit that silently never reconciles.
    fn spawn_reconciler(&mut self, source: &str, base_dir: &std::path::Path) {
        let Some(globals) = self.jsx_evaluator.as_ref().map(|e| e.globals_handle()) else {
            return;
        };
        self.reconciler = None;
        self.reconciler = Some(tauler::units::Reconciler::spawn(
            source.to_string(),
            self.jsx_ctx.clone(),
            Some(base_dir.to_path_buf()),
            std::sync::Arc::clone(&self.stream_values),
            globals,
        ));
    }

    fn handle_layout_reload(&mut self) -> bool {
        if self.reload_rx.try_recv().is_err() {
            return false;
        }

        // A format found at boot is locked in for the process's lifetime — switching
        // formats needs a restart (`docs/adr/0036`). Only retry detection here while
        // nothing was found yet, so a config created after a from-empty boot is still
        // picked up without one.
        if self.layout_source.is_none() {
            self.layout_source = tauler::layout_source::LayoutSource::detect(&self.config_dir);
        }
        let Some(source) = self.layout_source.clone() else {
            tracing::error!("no layout file found (checked layout.op.mdx and layout.jsx)");
            return false;
        };
        let loaded = match tauler::layout_source::load(&source) {
            Ok(loaded) => loaded,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "layout/config load error, keeping the layout already in use"
                );
                return false;
            }
        };

        let (mode, theme_file_path) = theme_selection(&loaded.config);
        tauler::reload_font_config(loaded.config.fonts);
        // Every frame the worker has kept was drawn with the fonts just replaced.
        let _ = self
            .outputs
            .jobs
            .send(tauler::render::worker::RenderJob::FontsChanged);
        let theme_loaded = load_theme(theme_file_path.as_deref());
        self.theme = theme_after_reload(&self.theme, theme_loaded);
        self.theme_mode = mode;

        self.handle.set_desired(vec![]);
        self.stream_values.write().unwrap().clear();
        self.jsx_evaluator = None;
        self.reconciler = None;

        let base_dir = self.config_dir.clone();
        match tauler::jsx::JsxEvaluator::new(
            &loaded.js_source,
            self.jsx_ctx.clone(),
            Some(&base_dir),
        ) {
            Ok(evaluator) => {
                let loaded_paths = evaluator.loaded_paths();
                let values = self.stream_values.read().unwrap().clone();
                match evaluator.eval(&values) {
                    Ok(out) => {
                        self.apply_eval_result_dispatch(&out);
                        self.jsx_evaluator = Some(evaluator);
                        self.reconcile_import_watches(loaded_paths);
                        self.spawn_reconciler(&loaded.js_source, &base_dir);
                    }
                    Err(e) => tracing::error!(error = %e, "JSX eval error"),
                }
            }
            Err(e) => tracing::error!(error = %e, "JSX compile error"),
        }
        self.reconcile_theme_file_watch(theme_file_path);
        tracing::info!("layout reloaded");
        true
    }

    /// One pointer event, run through the capture state machine (`docs/adr/0020`).
    ///
    /// A press hit-tests, fires `on_click` and `on_drag`, and starts a capture if the
    /// element has one. A motion never hit-tests: it measures against the box the press
    /// snapshotted, which is what lets a drag outlive the ticks it spans. A release ends
    /// the capture and dispatches nothing.
    fn on_pointer(&mut self, event: PointerEvent) {
        match event.phase {
            PointerPhase::Release => self.end_capture(),
            PointerPhase::Move => self.pointer_moved(&event),
            PointerPhase::Press => {
                self.end_capture();
                self.pointer_pressed(&event);
            }
        }
    }

    fn end_capture(&mut self) {
        if self.capture.take().is_some() {
            if let Some(evaluator) = self.jsx_evaluator.as_ref() {
                evaluator.release_handler();
            }
        }
    }

    /// Resolve a handler to the intents it wants sent, calling into JavaScript if it is
    /// a function rather than an array (`docs/adr/0021`).
    fn resolve(
        &self,
        value: &serde_json::Value,
        pointer: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        match read_handler(value)? {
            Handler::Intents(intents) => Some(intents),
            Handler::Function(id) => self.jsx_evaluator.as_ref()?.invoke_handler(id, pointer),
        }
    }

    /// Hand a handler's intents to their modules, one channel at a time.
    ///
    /// `supersedable` is what a drag produces: intents describing where the
    /// pointer is, of which only the newest is worth sending. A click is not
    /// that — it describes something that happened — so it goes out whatever
    /// else is in flight.
    fn send(&mut self, intents: &serde_json::Value, supersedable: bool) {
        let Some(list) = intents.as_array() else {
            tracing::warn!(intents = %intents, "intents is not an array");
            return;
        };
        let now = std::time::Instant::now();
        for intent in list {
            let (Some(channel), Some(event)) = (
                intent.get("channel").and_then(|c| c.as_str()),
                intent.get("event"),
            ) else {
                tracing::warn!(intent = %intent, "intent has no channel or no event");
                continue;
            };
            if supersedable {
                if let Some(ready) = self.outbox.offer(now, channel, event.clone()) {
                    self.write(channel, ready);
                }
            } else {
                self.outbox.urgent(now, channel);
                self.write(channel, event.clone());
            }
        }
    }

    /// Put one event on one module's stdin.
    fn write(&self, channel: &str, event: serde_json::Value) {
        let txs = self.module_event_txs.lock().unwrap();
        match txs.get(channel) {
            Some(tx) => {
                let ok = tx.send(event).is_ok();
                tracing::debug!(channel, ok, "intent dispatched");
            }
            None => tracing::warn!(
                channel,
                known = ?txs.keys().collect::<Vec<_>>(),
                "intent channel not found"
            ),
        }
    }

    fn pointer_pressed(&mut self, event: &PointerEvent) {
        let Some(spec) = self.surfaces.spec(&event.panel_id) else {
            return;
        };
        if spec.content.is_null() {
            return;
        }
        let content = spec.content.clone();
        let Some(hit) = hit_test(
            &content,
            event.phys_width,
            event.phys_height,
            event.dpr,
            event.x,
            event.y,
        ) else {
            tracing::debug!(x = event.x, y = event.y, "pointer: nothing under the press");
            return;
        };
        // On a press the pointer is the press, so the two points coincide.
        let press = (event.x, event.y);
        let pointer = hit.rect.pointer(press, press, event.dpr, event.buttons);

        if let Some(intents) = hit
            .on_click
            .as_ref()
            .and_then(|h| self.resolve(h, &pointer))
        {
            self.send(&intents, false);
        }

        // A press is the first drag event, so a plain click still sets a value and a
        // control needs only the one handler (`docs/adr/0020`).
        let Some(on_drag) = hit.on_drag.as_ref() else {
            return;
        };
        if let (Some(Handler::Function(id)), Some(evaluator)) =
            (read_handler(on_drag), self.jsx_evaluator.as_ref())
        {
            evaluator.capture_handler(id);
        }
        let mut capture = Capture::new(event.panel_id.clone(), hit.rect, event.dpr, press);
        if let Some(intents) = self.resolve(on_drag, &pointer) {
            self.send(&intents, false);
            capture.seed(intents);
        }
        self.capture = Some(capture);
    }

    /// A motion during a capture. No hit test: the box was snapshotted at press, which is
    /// both what makes this cheap and what lets it survive a tick.
    fn pointer_moved(&mut self, event: &PointerEvent) {
        let Some(capture) = self.capture.as_ref() else {
            return;
        };
        if capture.panel_id != event.panel_id {
            return;
        }
        let pointer = capture.pointer((event.x, event.y), event.buttons);
        let Some(intents) = self
            .jsx_evaluator
            .as_ref()
            .and_then(|e| e.invoke_captured_handler(&pointer))
        else {
            return;
        };
        if self.capture.as_mut().is_some_and(|c| c.is_new(&intents)) {
            self.send(&intents, true);
        }
    }

    /// Push every panel's a11y tree to the AT, if one is attached.
    ///
    /// Reconciles the per-panel adapters to the current panel set; each rebuild is
    /// gated by `update_if_active`, so with no AT attached nothing is built (ADR
    /// 0039).
    #[cfg(target_os = "linux")]
    fn update_a11y(&mut self) {
        let panels: Vec<tauler::a11y::PanelInfo> = self
            .surfaces
            .panel_specs()
            .into_iter()
            .filter(|s| s.kind == tauler::SurfaceKind::Panel)
            .map(|s| tauler::a11y::PanelInfo {
                id: s.id.clone(),
                content: s.content.clone(),
                width: (s.width as f32 * s.dpr).round() as u32,
                height: (s.height as f32 * s.dpr).round() as u32,
                dpr: s.dpr,
            })
            .collect();
        self.a11y.reconcile(&panels);
    }

    /// Handle activations an AT raised: each is a `(panel_id, path)` reduced on the
    /// platform thread, re-derived and dispatched here so a `$handler` reaches the
    /// QuickJS runtime (ADR 0038).
    #[cfg(target_os = "linux")]
    fn drain_a11y_actions(&mut self) {
        let actions: Vec<(String, Vec<usize>)> = self.a11y_rx.try_iter().collect();
        for (panel_id, path) in actions {
            self.activate(panel_id, path);
        }
    }

    /// Fire a node's `on_click` as an AT activation: a press at the box origin
    /// (`x`/`y`/`press_x`/`press_y` of `0`, real width and height — ADR 0038).
    #[cfg(target_os = "linux")]
    fn activate(&mut self, panel_id: String, path: Vec<usize>) {
        let Some(spec) = self.surfaces.spec(&panel_id) else {
            return;
        };
        if spec.kind != tauler::SurfaceKind::Panel || spec.content.is_null() {
            return;
        }
        let width = (spec.width as f32 * spec.dpr).round() as u32;
        let height = (spec.height as f32 * spec.dpr).round() as u32;
        let content = spec.content.clone();
        let dpr = spec.dpr;
        let Some((on_click, rect)) =
            tauler::a11y::click_at_path(&content, width, height, dpr, &path)
        else {
            return;
        };
        // The ADR fabricates the pointer as a press at the element's box origin,
        // so `x`/`y`/`press_x`/`press_y` are `0` (ADR 0038). `Rect::pointer`
        // measures from the box's top-left, so the "where the pointer is" and
        // "where it went down" points are the box's own origin — not the panel's.
        let pointer = rect.pointer(
            (rect.x, rect.y),
            (rect.x, rect.y),
            dpr,
            tauler::a11y::ACTIVATE_BUTTONS,
        );
        if let Some(intents) = self.resolve(&on_click, &pointer) {
            self.send(&intents, false);
        }
    }

    pub(crate) fn tick(&mut self) {
        self.last_tick.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            Ordering::Relaxed,
        );

        let mut changed = false;

        let mut answered: Vec<String> = Vec::new();
        while let Ok((key, value)) = self.item_rx.try_recv() {
            answered.push(key.0.clone());
            let stale = self.stream_values.read().unwrap().get(&key) != Some(&value);
            if stale {
                self.stream_values.write().unwrap().insert(key, value);
                changed = true;
            }
        }

        // A module that emitted a line has read what it was last sent, so its
        // channel is free for whatever the drag has produced since.
        let now = std::time::Instant::now();
        for channel in answered {
            if let Some(next) = self.outbox.answered(now, &channel) {
                self.write(&channel, next);
            }
        }
        // And a module that never answers must not be silenced for good.
        for (channel, intent) in self.outbox.released(now) {
            self.write(&channel, intent);
        }

        if changed {
            // What an eval costs is a question for `benches/pipeline.rs`, which
            // can ask it repeatably. It used to be logged from here, at INFO, on
            // every pass — which is to say several dozen times a second.
            let eval_out = self
                .jsx_evaluator
                .as_ref()
                .map(|e| e.eval(&self.stream_values.read().unwrap()));
            if let Some(eval_result) = eval_out {
                match eval_result {
                    Ok(out) => {
                        self.apply_eval_result_dispatch(&out);
                    }
                    Err(e) => tracing::error!(error = %e, "JSX re-eval error"),
                }
            }
        }

        if self.bin_reload_rx.try_recv().is_ok() {
            tracing::info!("binary changed, restarting...");
            self.stop.store(true, Ordering::Relaxed);
            return;
        }

        self.handle_layout_reload();

        // Collect before acting: a pass drains everything that arrived since the
        // last one, and a run of motions in that batch is worth exactly one
        // handler call. Acting event-by-event as they were drained is what made
        // a fast drag cost a QuickJS invocation per motion event.
        let events: Vec<PresenterEvent> = self.event_rx.try_iter().collect();
        for event in tauler::pointer::compress_motion(events) {
            match event {
                PresenterEvent::NeedsRender => {} // no-op: reconciler handles rendering
                PresenterEvent::OutputsChanged { outputs } => {
                    self.output_map = outputs
                        .iter()
                        .map(|o| (o.name.clone(), o.clone()))
                        .collect();
                    if let Some(primary) = outputs.first() {
                        let screen_width = (primary.width as f32 / primary.dpr).round() as u32;
                        let screen_height = (primary.height as f32 / primary.dpr).round() as u32;
                        self.jsx_ctx["screen_width"] = serde_json::json!(screen_width);
                        self.jsx_ctx["screen_height"] = serde_json::json!(screen_height);
                        self.dpr = primary.dpr;
                        self.screen_width_logical = screen_width;
                        self.screen_height_logical = screen_height;
                        tracing::info!(
                            screen_width,
                            screen_height,
                            dpr = primary.dpr,
                            "outputs changed"
                        );
                    }
                    let eval_out = self
                        .jsx_evaluator
                        .as_ref()
                        .map(|e| e.eval(&self.stream_values.read().unwrap()));
                    if let Some(eval_result) = eval_out {
                        match eval_result {
                            Ok(out) => {
                                self.apply_eval_result_dispatch(&out);
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "JSX re-eval error on output change")
                            }
                        }
                    }
                }
                PresenterEvent::Pointer(event) => self.on_pointer(event),
            }
        }

        #[cfg(target_os = "linux")]
        {
            self.update_a11y();
            self.drain_a11y_actions();
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        log_lifecycle_errors(self.surfaces.clear(&mut self.outputs));
        let _ = self.outputs.commands.send(SurfaceCommand::Shutdown);
        if let Some(h) = self.presenter_thread.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_eval_result, load_layout_or_exit, load_theme, make_mod_init_value,
        merge_module_props, stream_calls_to_specs, theme_after_reload, theme_file_watch_desired,
        theme_selection,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use tauler::config::TaulerConfig;
    use tauler::data::data_loop::{DataLoop, StreamSource};
    use tauler::layout::OutputInfo;
    use tauler::presentation::SurfaceCommand;
    use tauler::render::worker::RenderJob;
    use tauler::surface::{SurfaceOutputs, SurfaceSets};
    use tauler::theme::Theme;

    /// Outputs wired to a stand-in for the worker, which answers a `Now` job with
    /// a blank frame of the right size and swallows everything else.
    ///
    /// These tests are about which surfaces a layout produces, so the repaint
    /// requests are of no interest here — `src/surface` is where what gets asked
    /// for is checked.
    fn test_outputs() -> (SurfaceOutputs, mpsc::Receiver<SurfaceCommand>) {
        let (commands, command_rx) = mpsc::channel::<SurfaceCommand>();
        let (jobs, job_rx) = mpsc::channel::<RenderJob>();
        std::thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                if let RenderJob::Now { request, reply } = job {
                    let _ = reply.send(tauler::presentation::SurfaceFrame {
                        pixels: std::sync::Arc::new(vec![
                            0u8;
                            (request.width * request.height * 4)
                                as usize
                        ]),
                        width: request.width,
                        height: request.height,
                    });
                }
            }
        });
        (SurfaceOutputs { commands, jobs }, command_rx)
    }

    fn make_eval_output(layout: serde_json::Value) -> tauler::jsx::EvalOutput {
        tauler::jsx::EvalOutput {
            layout,
            stream_calls: vec![],
            module_calls: vec![],
        }
    }

    fn noop_mod_init() -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Claim A: apply_eval_result silently excludes any panel spec whose resolved output
    /// name is absent from the output_map. No SurfaceCommand::Create is sent for that spec.
    #[test]
    fn apply_eval_result_excludes_panel_with_unknown_output_name() {
        let layout = serde_json::json!({
            "type": "root",
            "children": [{
                "type": "panel",
                "id": "test-panel",
                "width": 100,
                "height": 200,
                "output": "HDMI-1",
                "anchor": "left"
            }]
        });
        let out = make_eval_output(layout);

        // output_map does NOT contain "HDMI-1"
        let output_map: HashMap<String, OutputInfo> = HashMap::new();

        let (_data_loop, handle) = DataLoop::new();
        let mut surface_set = SurfaceSets::new();
        let (mut outputs, command_rx) = test_outputs();

        apply_eval_result(
            &out,
            1.0,
            "DP-4",
            &output_map,
            &handle,
            &mut surface_set,
            &mut outputs,
            &noop_mod_init,
        );

        let cmds: Vec<SurfaceCommand> = command_rx.try_iter().collect();
        let create_count = cmds
            .iter()
            .filter(|cmd| matches!(cmd, SurfaceCommand::Create { .. }))
            .count();
        assert_eq!(
            create_count, 0,
            "expected no SurfaceCommand::Create for a panel whose output \"HDMI-1\" is absent from output_map, but got {} Create commands",
            create_count
        );
    }

    /// Claim B: A panel spec with output: None uses ctx.output_name (the primary output name)
    /// as its resolved output. If that name is also absent from the output_map, the spec is
    /// excluded and no SurfaceCommand::Create is sent.
    #[test]
    fn apply_eval_result_excludes_panel_with_null_output_when_primary_output_absent() {
        let layout = serde_json::json!({
            "type": "root",
            "children": [{
                "type": "panel",
                "id": "test-panel-null-output",
                "width": 100,
                "height": 200,
                "output": null,
                "anchor": "left"
            }]
        });
        let out = make_eval_output(layout);

        // output_map is empty — the primary output name "DP-1" is also absent
        let output_map: HashMap<String, OutputInfo> = HashMap::new();

        let (_data_loop, handle) = DataLoop::new();
        let mut surface_set = SurfaceSets::new();
        let (mut outputs, command_rx) = test_outputs();

        // primary output "DP-1" is not in output_map, so the null-output spec must be excluded
        apply_eval_result(
            &out,
            1.0,
            "DP-1",
            &output_map,
            &handle,
            &mut surface_set,
            &mut outputs,
            &noop_mod_init,
        );

        let cmds: Vec<SurfaceCommand> = command_rx.try_iter().collect();
        let create_count = cmds
            .iter()
            .filter(|cmd| matches!(cmd, SurfaceCommand::Create { .. }))
            .count();
        assert_eq!(
            create_count, 0,
            "expected no SurfaceCommand::Create when panel output is None and primary output is absent from output_map, but got {} Create commands",
            create_count
        );
    }

    fn output(name: &str, x: i16, y: i16, width: u32, height: u32, dpr: f32) -> OutputInfo {
        OutputInfo {
            name: name.to_string(),
            x,
            y,
            width,
            height,
            dpr,
        }
    }

    /// A surface that names no output means "the primary one". That has to be
    /// resolved to the primary's real name before reconciling, or a panel and a
    /// wallpaper meaning the same monitor carry different keys (`None` vs
    /// `Some("DP-1")`) and `backdrop` never pairs them up.
    #[test]
    fn apply_eval_result_resolves_an_unspecified_output_to_the_primary_name() {
        tauler::init_global_ctx(tauler::config::FontConfig::default());
        let layout = serde_json::json!({
            "type": "root",
            "children": [{ "type": "panel", "id": "p", "width": 10, "height": 10 }]
        });
        let out = make_eval_output(layout);
        let output_map: HashMap<String, OutputInfo> =
            [("DP-1".to_string(), output("DP-1", 0, 0, 2560, 1440, 1.0))]
                .into_iter()
                .collect();

        let (_data_loop, handle) = DataLoop::new();
        let mut surface_set = SurfaceSets::new();
        let (mut outputs, _command_rx) = test_outputs();

        apply_eval_result(
            &out,
            1.0,
            "DP-1",
            &output_map,
            &handle,
            &mut surface_set,
            &mut outputs,
            &noop_mod_init,
        );

        assert_eq!(
            surface_set.spec("p").and_then(|s| s.output.as_deref()),
            Some("DP-1"),
            "a panel with no declared output must be reconciled under the primary output's name"
        );
    }

    /// A `<wallpaper>` declares no geometry — it always covers its display
    /// exactly, so the dimensions come from the output, not the layout file.
    #[test]
    fn apply_eval_result_sizes_wallpaper_to_its_output() {
        tauler::init_global_ctx(tauler::config::FontConfig::default());
        let layout = serde_json::json!({
            "type": "root",
            "children": [{ "type": "wallpaper", "id": "bg", "output": "DP-1" }]
        });
        let out = make_eval_output(layout);
        let output_map: HashMap<String, OutputInfo> =
            [("DP-1".to_string(), output("DP-1", 100, 50, 2560, 1440, 2.0))]
                .into_iter()
                .collect();

        let (_data_loop, handle) = DataLoop::new();
        let mut surface_set = SurfaceSets::new();
        let (mut outputs, command_rx) = test_outputs();

        apply_eval_result(
            &out,
            1.0,
            "DP-1",
            &output_map,
            &handle,
            &mut surface_set,
            &mut outputs,
            &noop_mod_init,
        );

        let cmds: Vec<SurfaceCommand> = command_rx.try_iter().collect();
        let Some(SurfaceCommand::PaintWallpaper { spec, frame }) = cmds
            .into_iter()
            .find(|c| matches!(c, SurfaceCommand::PaintWallpaper { .. }))
        else {
            panic!("expected a PaintWallpaper command for the wallpaper");
        };
        assert_eq!(
            (frame.width, frame.height),
            (2560, 1440),
            "the rendered buffer must be exactly the output's physical size"
        );
        assert_eq!(
            (spec.width, spec.height),
            (1280, 720),
            "logical dimensions must be the output size divided by its DPR"
        );
        assert_eq!(
            (spec.x, spec.y),
            (100, 50),
            "wallpaper position must track the output's origin"
        );
        assert_eq!(spec.dpr, 2.0);
    }

    fn wayland_mod_init() -> serde_json::Value {
        make_mod_init_value("", 96.0, 0, 0)
    }

    /// Claim: output field must be "" (empty string), NOT "wayland" or any compositor name.
    /// fetch_workspaces in tauler-i3 filters all workspaces when output is non-empty.
    #[test]
    fn mod_init_wayland_output_is_empty_string() {
        let result = wayland_mod_init();
        assert_eq!(
            result["output"].as_str(),
            Some(""),
            "output must be empty string — if it is \"wayland\", fetch_workspaces filters all workspaces"
        );
    }

    #[test]
    fn mod_init_type_is_init() {
        let result = wayland_mod_init();
        assert_eq!(result["type"].as_str(), Some("init"));
    }

    #[test]
    /// `gaps` is an ordinary prop now: tauler neither derives nor rescales it,
    /// so whatever the layout declared is what the module receives. i3 applies
    /// `logical_px` itself, so a logical value must arrive intact.
    fn module_props_carry_jsx_declared_values_alongside_init() {
        let init = serde_json::json!({"type": "init", "output": "DP-4"});
        let jsx = serde_json::json!({"gaps": {"left": 272, "top": 26}});
        let merged = merge_module_props(&init, &jsx);
        assert_eq!(merged["gaps"]["left"].as_u64(), Some(272));
        assert_eq!(merged["gaps"]["top"].as_u64(), Some(26));
        assert_eq!(merged["output"].as_str(), Some("DP-4"));
    }

    /// The init payload is a protocol, not user-editable state — a JSX prop
    /// must not be able to redefine `type` and break module parsing.
    #[test]
    fn init_keys_win_over_conflicting_jsx_props() {
        let init = serde_json::json!({"type": "init", "config": {"left": 250}});
        let jsx = serde_json::json!({"type": "bogus", "config": {"left": 1}});
        let merged = merge_module_props(&init, &jsx);
        assert_eq!(merged["type"].as_str(), Some("init"));
        assert_eq!(merged["config"]["left"].as_u64(), Some(250));
    }

    #[test]
    fn merge_is_a_noop_when_the_module_declares_no_props() {
        let init = serde_json::json!({"type": "init", "config": {"left": 250}});
        assert_eq!(merge_module_props(&init, &serde_json::Value::Null), init);
    }

    #[test]
    fn stream_calls_to_specs_maps_calls_to_process_specs() {
        use tauler::data::data_loop::Resource;

        let calls = vec![
            ("bash".to_string(), None),
            ("python".to_string(), Some("print('hi')".to_string())),
        ];
        let specs = stream_calls_to_specs(&calls);
        assert_eq!(specs.len(), 2);
        let StreamSource::Process(ref s0) = specs[0] else {
            panic!("expected Process")
        };
        assert_eq!(s0.identity.bin, "bash");
        assert!(s0.args.is_empty(), "no-script call should have no args");
        let StreamSource::Process(ref s1) = specs[1] else {
            panic!("expected Process")
        };
        assert_eq!(s1.identity.bin, "python");
        assert_eq!(
            s1.args,
            vec![Resource::File {
                content: "print('hi')".to_string()
            }]
        );
    }

    #[test]
    fn stream_calls_to_specs_routes_tauler_prefix_to_builtin() {
        let calls = vec![("tauler:outputs".to_string(), None)];
        let specs = stream_calls_to_specs(&calls);
        assert_eq!(specs.len(), 1);
        assert!(
            matches!(specs[0], StreamSource::BuiltIn(_)),
            "tauler: prefix must map to BuiltIn"
        );
    }

    /// Claim: when theme.file is set to a tilde path in config, theme_selection returns
    /// the tilde-expanded absolute path.
    #[test]
    fn theme_selection_returns_expanded_path_when_file_is_configured() {
        let config = TaulerConfig::from_yaml("theme:\n  file: ~/some/theme.yaml\n")
            .expect("valid yaml should parse");

        let (_mode, path) = theme_selection(&config);

        let home = std::env::var("HOME").expect("HOME must be set");
        let expected = PathBuf::from(&home).join("some/theme.yaml");
        assert_eq!(
            path,
            Some(expected),
            "tilde in theme.file must be expanded to the real HOME directory"
        );
    }

    /// Claim: when no theme.file is configured, theme_selection returns None so the
    /// caller knows there is no file to watch.
    #[test]
    fn theme_selection_returns_none_when_no_file_configured() {
        let config =
            TaulerConfig::from_yaml("theme:\n  mode: dark\n").expect("valid yaml should parse");

        let (_mode, path) = theme_selection(&config);

        assert_eq!(
            path, None,
            "when no theme.file is set, the returned path must be None"
        );
    }

    fn theme_with_dark_bg(bg: &str) -> Theme {
        Theme::from_yaml(&format!("colors:\n  dark:\n    background: \"{bg}\"\n"))
            .expect("test theme must parse")
    }

    fn dark_background(theme: &Theme) -> Option<String> {
        theme.colors.dark.get("background").cloned()
    }

    /// Claim: a theme.file that cannot be read is an error. Falling back to the default palette
    /// renders a bar that looks deliberate in the wrong colours, so the caller has to be told.
    #[test]
    fn load_theme_errors_when_the_file_cannot_be_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nowhere.yaml");

        let err = load_theme(Some(&missing)).expect_err("an unreadable theme file must not load");

        assert!(
            err.to_string().contains(&missing.display().to_string()),
            "the error must name the path that failed, got: {err}"
        );
    }

    /// Claim: a theme.file that is not valid YAML is an error for the same reason an unreadable
    /// one is — a bad indent must not read as a design choice.
    #[test]
    fn load_theme_errors_when_the_file_is_invalid_yaml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("theme.yaml");
        std::fs::write(&path, "colors:\n dark:\n   - not: a map\n").expect("write theme");

        let err = load_theme(Some(&path)).expect_err("invalid theme YAML must not load");

        assert!(
            err.to_string().contains(&path.display().to_string()),
            "the error must name the path that failed, got: {err}"
        );
    }

    /// Claim: no theme.file at all is not a failure — it is the documented way to ask for the
    /// shipped default. Only a theme the user named and tauler could not honour is an error.
    #[test]
    fn load_theme_returns_the_default_when_no_file_is_configured() {
        let theme = load_theme(None).expect("no theme.file must not be an error");

        assert_eq!(
            theme.colors.dark,
            Theme::default_theme().colors.dark,
            "with no theme.file the shipped default must be used"
        );
    }

    /// Claim: no layout file at all is not a crash — it draws nothing, the same way a missing
    /// `layout.jsx` always has. This must stay reachable, not exit(1): a fresh install with
    /// nothing configured yet is a normal state, not an unusable config (docs/adr/0036).
    #[test]
    fn load_layout_or_exit_returns_none_when_no_source_is_detected() {
        assert!(load_layout_or_exit(None).is_none());
    }

    /// Claim: a theme.file that breaks while tauler is running leaves the palette already on
    /// screen in place. Startup has no theme to keep and exits; a reload does, and an editor
    /// saving a half-written file must not repaint the bar in the default grey.
    #[test]
    fn theme_after_reload_keeps_the_current_theme_when_the_file_goes_bad() {
        let current = theme_with_dark_bg("#112233");
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nowhere.yaml");

        let theme = theme_after_reload(&current, load_theme(Some(&missing)));

        assert_eq!(
            dark_background(&theme).as_deref(),
            Some("#112233"),
            "a failed reload must keep the theme already in use, not substitute the default"
        );
    }

    /// Claim: a reload that does load replaces the theme, or fixing the file would never take
    /// effect.
    #[test]
    fn theme_after_reload_takes_the_new_theme_when_it_loads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("theme.yaml");
        std::fs::write(&path, "colors:\n  dark:\n    background: \"#445566\"\n")
            .expect("write theme");

        let theme = theme_after_reload(&theme_with_dark_bg("#112233"), load_theme(Some(&path)));

        assert_eq!(
            dark_background(&theme).as_deref(),
            Some("#445566"),
            "a successful reload must apply the newly loaded theme"
        );
    }

    /// Claim: when a theme file path is provided, `theme_file_watch_desired` returns a
    /// single-element Vec whose entry has the given path as its key — so the caller can
    /// reconcile a OptativeSet<WatchedPath> to watch that file.
    #[test]
    fn theme_file_watch_desired_with_some_path_returns_single_entry_with_that_path() {
        let path = PathBuf::from("/tmp/my-theme.yaml");
        let desired = theme_file_watch_desired(Some(path.clone()));
        assert_eq!(
            desired.len(),
            1,
            "Some(path) must produce exactly one desired watch entry"
        );
        use tauler::managed_set::Lifecycle;
        assert_eq!(
            desired[0].key(),
            path,
            "the entry's key must be the supplied path"
        );
    }

    /// Claim: when no theme file path is present, `theme_file_watch_desired` returns an
    /// empty Vec — so the caller can reconcile a OptativeSet<WatchedPath> to remove any
    /// previously-registered theme watch.
    #[test]
    fn theme_file_watch_desired_with_none_returns_empty_vec() {
        let desired = theme_file_watch_desired(None);
        assert!(
            desired.is_empty(),
            "None must produce an empty desired set so the old watch is removed"
        );
    }
}
