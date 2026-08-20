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
    config_path: std::path::PathBuf,
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
    stream_values: HashMap<(String, Option<String>), String>,
    jsx_evaluator: Option<tauler::jsx::JsxEvaluator>,
    handle: DataLoopHandle,
    jsx_ctx: serde_json::Value,
    item_rx: mpsc::Receiver<((String, Option<String>), String)>,
    bin_reload_rx: mpsc::Receiver<()>,
    reload_rx: mpsc::Receiver<()>,
    layout_jsx_path: std::path::PathBuf,
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

fn parse_config(config_path: &std::path::Path) -> TaulerConfig {
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|s| TaulerConfig::from_yaml(&s).ok())
        .unwrap_or_default()
}

fn load_theme_from_config(
    config_path: &std::path::Path,
) -> (Theme, ThemeMode, Option<std::path::PathBuf>) {
    let config = parse_config(config_path);
    let theme_file_path = config
        .theme
        .file
        .as_deref()
        .map(tauler::config::expand_tilde);
    let theme = match theme_file_path.as_ref() {
        None => Theme::default_theme(),
        Some(p) => match std::fs::read_to_string(p) {
            Err(e) => {
                tracing::warn!(path = %p.display(), error = %e, "failed to read theme file, using default");
                Theme::default_theme()
            }
            Ok(s) => match Theme::from_yaml(&s) {
                Err(e) => {
                    tracing::warn!(path = %p.display(), error = %e, "invalid theme YAML, using default");
                    Theme::default_theme()
                }
                Ok(t) => t,
            },
        },
    };
    (theme, config.theme.mode, theme_file_path)
}

impl App {
    #[cfg(not(target_os = "macos"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_x11(
        x11: X11Init,
        handle: DataLoopHandle,
        rx: TickReceivers,
        layout_jsx_path: std::path::PathBuf,
        config_path: std::path::PathBuf,
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
        let events = PresenterEvents::new(event_tx, notifier);
        let pt = PresentationThread::new(panel_ctx);
        let presenter_thread = thread::spawn(move || {
            run_x11_presenter_thread(pt, command_rx, events);
        });
        let (theme, theme_mode, theme_file_path) = load_theme_from_config(&config_path);
        let mut state = Self {
            theme,
            theme_mode,
            config_path,
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
            stream_values: HashMap::new(),
            jsx_evaluator: None,
            handle,
            jsx_ctx,
            item_rx: rx.item_rx,
            bin_reload_rx: rx.bin_reload_rx,
            reload_rx: rx.reload_rx,
            layout_jsx_path,
            stop,
            last_tick,
            outputs: spawn_render_worker(command_tx),
            event_rx,
            module_event_txs,
            capture: None,
            outbox: Outbox::new(),
            presenter_thread: Some(presenter_thread),
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
        layout_jsx_path: std::path::PathBuf,
        config_path: std::path::PathBuf,
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
        let events = PresenterEvents::new(event_tx, notifier);
        let pt = PresentationThread::new(server);
        let presenter_thread = thread::spawn(move || {
            run_wayland_presenter_thread(pt, command_rx, events);
        });
        let (theme, theme_mode, theme_file_path) = load_theme_from_config(&config_path);
        let mut state = Self {
            theme,
            theme_mode,
            config_path,
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
            stream_values: HashMap::new(),
            jsx_evaluator: None,
            handle,
            jsx_ctx,
            item_rx: rx.item_rx,
            bin_reload_rx: rx.bin_reload_rx,
            reload_rx: rx.reload_rx,
            layout_jsx_path,
            stop,
            last_tick,
            outputs: spawn_render_worker(command_tx),
            event_rx,
            module_event_txs,
            capture: None,
            outbox: Outbox::new(),
            presenter_thread: Some(presenter_thread),
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
        layout_jsx_path: std::path::PathBuf,
        config_path: std::path::PathBuf,
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
        let (theme, theme_mode, theme_file_path) = load_theme_from_config(&config_path);
        let mut state = Self {
            theme,
            theme_mode,
            config_path,
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
            stream_values: HashMap::new(),
            jsx_evaluator: None,
            handle,
            jsx_ctx,
            item_rx: rx.item_rx,
            bin_reload_rx: rx.bin_reload_rx,
            reload_rx: rx.reload_rx,
            layout_jsx_path,
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
        if !self.layout_jsx_path.exists() {
            return;
        }
        let source = match std::fs::read_to_string(&self.layout_jsx_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "JSX file error");
                return;
            }
        };
        let t = std::time::Instant::now();
        let base_dir = self
            .layout_jsx_path
            .parent()
            .unwrap_or(&self.layout_jsx_path);
        let evaluator =
            match tauler::jsx::JsxEvaluator::new(&source, self.jsx_ctx.clone(), Some(base_dir)) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!(error = %e, "JSX compile error");
                    return;
                }
            };
        let loaded = evaluator.loaded_paths();
        let eval_out = evaluator.eval(&self.stream_values);
        match eval_out {
            Ok(out) => {
                tracing::debug!(elapsed_ms = t.elapsed().as_millis(), "jsx eval");
                self.apply_eval_result_dispatch(&out);
                self.jsx_evaluator = Some(evaluator);
                self.reconcile_import_watches(loaded);
            }
            Err(e) => tracing::error!(error = %e, "JSX eval error"),
        }
    }

    fn handle_layout_reload(&mut self) -> bool {
        if self.reload_rx.try_recv().is_err() {
            return false;
        }
        let config = parse_config(&self.config_path);
        tauler::reload_font_config(config.fonts);
        // Every frame the worker has kept was drawn with the fonts just replaced.
        let _ = self
            .outputs
            .jobs
            .send(tauler::render::worker::RenderJob::FontsChanged);
        let (theme, mode, theme_file_path) = load_theme_from_config(&self.config_path);
        self.theme = theme;
        self.theme_mode = mode;

        self.handle.set_desired(vec![]);
        self.stream_values.clear();
        self.jsx_evaluator = None;

        if self.layout_jsx_path.exists() {
            match std::fs::read_to_string(&self.layout_jsx_path) {
                Ok(source) => {
                    let base_dir = self
                        .layout_jsx_path
                        .parent()
                        .unwrap_or(&self.layout_jsx_path);
                    match tauler::jsx::JsxEvaluator::new(
                        &source,
                        self.jsx_ctx.clone(),
                        Some(base_dir),
                    ) {
                        Ok(evaluator) => {
                            let loaded = evaluator.loaded_paths();
                            match evaluator.eval(&self.stream_values) {
                                Ok(out) => {
                                    self.apply_eval_result_dispatch(&out);
                                    self.jsx_evaluator = Some(evaluator);
                                    self.reconcile_import_watches(loaded);
                                }
                                Err(e) => tracing::error!(error = %e, "JSX eval error"),
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "JSX compile error"),
                    }
                }
                Err(e) => tracing::error!(error = %e, "JSX file error"),
            }
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
            if self.stream_values.get(&key).map(|s| s.as_str()) != Some(value.as_str()) {
                self.stream_values.insert(key, value);
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
                .map(|e| e.eval(&self.stream_values));
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
                        .map(|e| e.eval(&self.stream_values));
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
        apply_eval_result, load_theme_from_config, make_mod_init_value, merge_module_props,
        stream_calls_to_specs, theme_file_watch_desired,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use tauler::data::data_loop::{DataLoop, StreamSource};
    use tauler::layout::OutputInfo;
    use tauler::presentation::SurfaceCommand;
    use tauler::render::worker::RenderJob;
    use tauler::surface::{SurfaceOutputs, SurfaceSets};

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

    /// Claim: when theme.file is set to a tilde path in config, load_theme_from_config returns the
    /// tilde-expanded absolute path as the third tuple element.
    #[test]
    fn load_theme_from_config_returns_expanded_path_when_file_is_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.yaml");
        std::fs::write(&config_path, "theme:\n  file: ~/some/theme.yaml\n").expect("write config");

        let (_theme, _mode, path) = load_theme_from_config(&config_path);

        let home = std::env::var("HOME").expect("HOME must be set");
        let expected = PathBuf::from(&home).join("some/theme.yaml");
        assert_eq!(
            path,
            Some(expected),
            "tilde in theme.file must be expanded to the real HOME directory"
        );
    }

    /// Claim: when no theme.file is configured, load_theme_from_config returns None as the third
    /// tuple element so the caller knows there is no file to watch.
    #[test]
    fn load_theme_from_config_returns_none_when_no_file_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.yaml");
        std::fs::write(&config_path, "theme:\n  mode: dark\n").expect("write config");

        let (_theme, _mode, path) = load_theme_from_config(&config_path);

        assert_eq!(
            path, None,
            "when no theme.file is set, the returned path must be None"
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
