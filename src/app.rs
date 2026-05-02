use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use costae::config::CostaeConfig;
use costae::layout::OutputInfo;
use costae::theme::{Theme, ThemeMode};
use costae::theme::resolver::resolve_tw_in_json;
use costae::data::data_loop::{DataLoopHandle, BuiltInSource, ProcessIdentity, ProcessSource, StreamSource};
use costae::x11::click::do_hit_test;
use costae::x11::panel::PanelContext;
use costae::managed_set::{Lifecycle, ManagedSet, Reconcile};
use notify::Watcher;
use costae::windowing::wayland::WaylandDisplayServer;
use costae::panel::PanelSpec;
use costae::presentation::{PanelCommand, PresentationThread, PresenterEvent};

use crate::presenter::x11::run_x11_presenter_thread;
use crate::presenter::wayland::run_wayland_presenter_thread;

pub(crate) type ModuleEventTxs = Arc<std::sync::Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>;
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

    fn key(&self) -> std::path::PathBuf { self.0.clone() }

    fn enter(self, ctx: &mut SharedWatcher, _: &mut ()) -> Result<std::path::PathBuf, notify::Error> {
        ctx.lock().unwrap().watch(&self.0, notify::RecursiveMode::NonRecursive)?;
        Ok(self.0)
    }

    fn reconcile_self(self, _: &mut std::path::PathBuf, _: &mut SharedWatcher, _: &mut ()) -> Result<(), notify::Error> {
        Ok(())
    }

    fn exit(state: std::path::PathBuf, ctx: &mut SharedWatcher, _: &mut ()) -> Result<(), notify::Error> {
        ctx.lock().unwrap().unwatch(&state)
    }
}

fn log_lifecycle_errors<K: std::fmt::Debug, E: std::fmt::Debug>(errors: costae::managed_set::ReconcileErrors<K, E>) {
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
    use costae::x11::outputs::outputs_thread;
    match key {
        "costae:outputs" => Some(BuiltInSource { key: key.to_string(), func: outputs_thread }),
        _ => None,
    }
}

pub(crate) fn stream_calls_to_specs(calls: &[(String, Option<String>)]) -> Vec<StreamSource> {
    calls.iter().map(|(bin, script)| {
        if let Some(builtin) = make_builtin(bin) {
            return StreamSource::BuiltIn(builtin);
        }
        StreamSource::Process(ProcessSource {
            identity: ProcessIdentity {
                bin: bin.clone(),
                key: format!("{}:{}", bin, script.as_deref().unwrap_or("")),
            },
            script: script.clone(),
            args: vec![],
            env: std::collections::BTreeMap::new(),
            current_dir: None,
            props: None,
        })
    }).collect()
}

fn apply_eval_result(
    out: &costae::jsx::EvalOutput,
    dpr: f32,
    primary_output_name: &str,
    output_map: &HashMap<String, OutputInfo>,
    handle: &DataLoopHandle,
    panel_set: &mut ManagedSet<PanelSpec>,
    command_tx: &mut mpsc::Sender<PanelCommand>,
    mod_init_fn: &dyn Fn(&[costae::PanelSpecData]) -> serde_json::Value,
) -> bool {
    let mut specs = match costae::parse_root_node(&out.layout) {
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
        let name = spec.output.as_deref().unwrap_or(primary_output_name);
        spec.dpr = output_map.get(name).map(|o| o.dpr).unwrap_or(dpr);
    }
    let mod_init = mod_init_fn(&specs);

    let module_bins: std::collections::HashSet<String> =
        out.module_calls.iter().map(|(b, _)| b.clone()).collect();
    let stream_specs = stream_calls_to_specs(&out.stream_calls)
        .into_iter()
        .filter(|s| match s {
            StreamSource::Process(p) => !module_bins.contains(&p.identity.bin),
            StreamSource::BuiltIn(_) => true,
        })
        .collect::<Vec<_>>();
    let module_specs: Vec<StreamSource> = out.module_calls.iter().map(|(bin, _)| {
        StreamSource::Process(ProcessSource {
            identity: ProcessIdentity { bin: bin.clone(), key: bin.clone() },
            script: None,
            args: vec![],
            env: std::collections::BTreeMap::new(),
            current_dir: None,
            props: Some(mod_init.clone()),
        })
    }).collect();
    let combined: Vec<StreamSource> = stream_specs.into_iter().chain(module_specs).collect();
    handle.set_desired(combined);

    let panel_errors = panel_set.reconcile(
        specs.into_iter().map(PanelSpec),
        &mut (), command_tx,
    );
    log_lifecycle_errors(panel_errors);
    true
}

fn make_mod_init_value(
    specs: &[costae::PanelSpecData],
    dpr: f32,
    output_name: &str,
    dpi: f32,
    screen_width_logical: u32,
    screen_height_logical: u32,
) -> serde_json::Value {
    let spec = specs.iter()
        .find(|p| p.anchor == Some(costae::PanelAnchor::Left))
        .or_else(|| specs.first());
    let (bar_w, og) = spec
        .map(|p| ((p.width as f32 * dpr).round() as u32, (p.outer_gap as f32 * dpr).round() as u32))
        .unwrap_or((250, 0));
    serde_json::json!({
        "type": "init",
        "config": {"width": bar_w, "outer_gap": og},
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

pub(crate) struct X11Init {
    pub(crate) panel_ctx: PanelContext,
    pub(crate) jsx_ctx: serde_json::Value,
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
    panels: ManagedSet<PanelSpec>,
    import_watches: ManagedSet<WatchedPath>,
    theme_file_watch: ManagedSet<WatchedPath>,
    watcher: SharedWatcher,
    stream_values: HashMap<(String, Option<String>), String>,
    jsx_evaluator: Option<costae::jsx::JsxEvaluator>,
    handle: DataLoopHandle,
    jsx_ctx: serde_json::Value,
    item_rx: mpsc::Receiver<((String, Option<String>), String)>,
    bin_reload_rx: mpsc::Receiver<()>,
    reload_rx: mpsc::Receiver<()>,
    layout_jsx_path: std::path::PathBuf,
    stop: Arc<AtomicBool>,
    last_tick: Arc<std::sync::atomic::AtomicU64>,
    command_tx: mpsc::Sender<PanelCommand>,
    event_rx: mpsc::Receiver<PresenterEvent>,
    module_event_txs: ModuleEventTxs,
    presenter_thread: Option<thread::JoinHandle<()>>,
}

fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        std::path::PathBuf::from(home).join(rest)
    } else {
        std::path::PathBuf::from(path)
    }
}

fn load_theme_from_config(config_path: &std::path::Path) -> (Theme, ThemeMode, Option<std::path::PathBuf>) {
    let config = std::fs::read_to_string(config_path)
        .ok()
        .and_then(|s| CostaeConfig::from_yaml(&s).ok())
        .unwrap_or_default();
    let theme_file_path = config.theme.file.as_deref().map(expand_tilde);
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
            }
        }
    };
    (theme, config.theme.mode, theme_file_path)
}

impl App {
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
        let pt = PresentationThread::new(panel_ctx);
        let presenter_thread = thread::spawn(move || {
            run_x11_presenter_thread(pt, command_rx, event_tx);
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
            panels: ManagedSet::new(),
            import_watches: ManagedSet::new(),
            theme_file_watch: ManagedSet::new(),
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
            command_tx,
            event_rx,
            module_event_txs,
            presenter_thread: Some(presenter_thread),
        };
        state.initial_load();
        state.reconcile_theme_file_watch(theme_file_path);
        state
    }

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
        let pt = PresentationThread::new(server);
        let presenter_thread = thread::spawn(move || {
            run_wayland_presenter_thread(pt, command_rx, event_tx);
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
            panels: ManagedSet::new(),
            import_watches: ManagedSet::new(),
            theme_file_watch: ManagedSet::new(),
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
            command_tx,
            event_rx,
            module_event_txs,
            presenter_thread: Some(presenter_thread),
        };
        state.initial_load();
        state.reconcile_theme_file_watch(theme_file_path);
        state
    }

    fn apply_eval_result_dispatch(&mut self, out: &costae::jsx::EvalOutput) -> bool {
        let mut layout = out.layout.clone();
        resolve_tw_in_json(&mut layout, &self.theme, self.theme_mode);
        let resolved_out = costae::jsx::EvalOutput {
            layout,
            stream_calls: out.stream_calls.clone(),
            module_calls: out.module_calls.clone(),
        };
        let (dpr, dpi, sw, sh) = (self.dpr, self.dpi, self.screen_width_logical, self.screen_height_logical);
        let output_name = self.output_name.clone();
        apply_eval_result(&resolved_out, dpr, &self.output_name, &self.output_map, &self.handle, &mut self.panels, &mut self.command_tx,
            &move |specs| make_mod_init_value(specs, dpr, &output_name, dpi, sw, sh))
    }

    fn reconcile_watch_set(
        set: &mut ManagedSet<WatchedPath>,
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
        if !self.layout_jsx_path.exists() { return; }
        let source = match std::fs::read_to_string(&self.layout_jsx_path) {
            Ok(s) => s,
            Err(e) => { tracing::error!(error = %e, "JSX file error"); return; }
        };
        let t = std::time::Instant::now();
        let base_dir = self.layout_jsx_path.parent().unwrap_or(&self.layout_jsx_path);
        let evaluator = match costae::jsx::JsxEvaluator::new(&source, self.jsx_ctx.clone(), Some(base_dir)) {
            Ok(e) => e,
            Err(e) => { tracing::error!(error = %e, "JSX compile error"); return; }
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
        if self.reload_rx.try_recv().is_err() { return false; }
        let (theme, mode, theme_file_path) = load_theme_from_config(&self.config_path);
        self.theme = theme;
        self.theme_mode = mode;

        self.handle.set_desired(vec![]);
        self.stream_values.clear();
        self.jsx_evaluator = None;

        if self.layout_jsx_path.exists() {
            match std::fs::read_to_string(&self.layout_jsx_path) {
                Ok(source) => {
                    let base_dir = self.layout_jsx_path.parent().unwrap_or(&self.layout_jsx_path);
                    match costae::jsx::JsxEvaluator::new(&source, self.jsx_ctx.clone(), Some(base_dir)) {
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

    pub(crate) fn tick(&mut self) {
        self.last_tick.store(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            Ordering::Relaxed,
        );

        let mut changed = false;

        while let Ok((key, value)) = self.item_rx.try_recv() {
            if self.stream_values.get(&key).map(|s| s.as_str()) != Some(value.as_str()) {
                self.stream_values.insert(key, value);
                changed = true;
            }
        }

        if changed {
            let eval_out = self.jsx_evaluator.as_ref().map(|e| {
                let t = std::time::Instant::now();
                let r = e.eval(&self.stream_values);
                tracing::debug!(elapsed_us = t.elapsed().as_micros(), "jsx re-eval");
                r
            });
            if let Some(eval_result) = eval_out {
                match eval_result {
                    Ok(out) => { self.apply_eval_result_dispatch(&out); }
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

        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                PresenterEvent::NeedsRender => {} // no-op: reconciler handles rendering
                PresenterEvent::OutputsChanged { outputs } => {
                    self.output_map = outputs.iter().map(|o| (o.name.clone(), o.clone())).collect();
                    if let Some(primary) = outputs.first() {
                        let screen_width = (primary.width as f32 / primary.dpr).round() as u32;
                        let screen_height = (primary.height as f32 / primary.dpr).round() as u32;
                        self.jsx_ctx["screen_width"] = serde_json::json!(screen_width);
                        self.jsx_ctx["screen_height"] = serde_json::json!(screen_height);
                        self.dpr = primary.dpr;
                        self.screen_width_logical = screen_width;
                        self.screen_height_logical = screen_height;
                        tracing::info!(screen_width, screen_height, dpr = primary.dpr, "outputs changed");
                    }
                    let eval_out = self.jsx_evaluator.as_ref().map(|e| e.eval(&self.stream_values));
                    if let Some(eval_result) = eval_out {
                        match eval_result {
                            Ok(out) => { self.apply_eval_result_dispatch(&out); }
                            Err(e) => tracing::error!(error = %e, "JSX re-eval error on output change"),
                        }
                    }
                }
                PresenterEvent::Click { panel_id, x, y, phys_width, phys_height, dpr } => {
                    if let Some(spec) = self.panels.get(&panel_id) {
                        let raw_layout = if spec.content.is_null() { None } else { Some(spec.content.clone()) };
                        let txs = self.module_event_txs.lock().unwrap();
                        do_hit_test(&raw_layout, &txs, phys_width, phys_height, dpr, x, y);
                    }
                }
            }
        }
    }

}

impl Drop for App {
    fn drop(&mut self) {
        log_lifecycle_errors(self.panels.reconcile(vec![], &mut (), &mut self.command_tx));
        let _ = self.command_tx.send(PanelCommand::Shutdown);
        if let Some(h) = self.presenter_thread.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_eval_result, load_theme_from_config, make_mod_init_value, stream_calls_to_specs, theme_file_watch_desired};
    use costae::data::data_loop::{DataLoop, StreamSource};
    use costae::managed_set::ManagedSet;
    use costae::panel::PanelSpec;
    use costae::presentation::PanelCommand;
    use costae::layout::OutputInfo;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::mpsc;

    fn make_eval_output(layout: serde_json::Value) -> costae::jsx::EvalOutput {
        costae::jsx::EvalOutput {
            layout,
            stream_calls: vec![],
            module_calls: vec![],
        }
    }

    fn noop_mod_init(_specs: &[costae::PanelSpecData]) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Claim A: apply_eval_result silently excludes any panel spec whose resolved output
    /// name is absent from the output_map. No PanelCommand::Create is sent for that spec.
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
        let mut panel_set: ManagedSet<PanelSpec> = ManagedSet::new();
        let (mut command_tx, command_rx) = mpsc::channel::<PanelCommand>();

        apply_eval_result(&out, 1.0, "DP-4", &output_map, &handle, &mut panel_set, &mut command_tx, &noop_mod_init);

        let cmds: Vec<PanelCommand> = command_rx.try_iter().collect();
        let create_count = cmds.iter().filter(|cmd| matches!(cmd, PanelCommand::Create { .. })).count();
        assert_eq!(
            create_count, 0,
            "expected no PanelCommand::Create for a panel whose output \"HDMI-1\" is absent from output_map, but got {} Create commands",
            create_count
        );
    }

    /// Claim B: A panel spec with output: None uses ctx.output_name (the primary output name)
    /// as its resolved output. If that name is also absent from the output_map, the spec is
    /// excluded and no PanelCommand::Create is sent.
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
        let mut panel_set: ManagedSet<PanelSpec> = ManagedSet::new();
        let (mut command_tx, command_rx) = mpsc::channel::<PanelCommand>();

        // primary output "DP-1" is not in output_map, so the null-output spec must be excluded
        apply_eval_result(&out, 1.0, "DP-1", &output_map, &handle, &mut panel_set, &mut command_tx, &noop_mod_init);

        let cmds: Vec<PanelCommand> = command_rx.try_iter().collect();
        let create_count = cmds.iter().filter(|cmd| matches!(cmd, PanelCommand::Create { .. })).count();
        assert_eq!(
            create_count, 0,
            "expected no PanelCommand::Create when panel output is None and primary output is absent from output_map, but got {} Create commands",
            create_count
        );
    }

    fn left_spec(width: u32) -> costae::PanelSpecData {
        costae::PanelSpecData {
            id: "p".into(),
            width,
            height: 30,
            x: 0,
            y: 0,
            outer_gap: 0,
            above: false,
            output: None,
            anchor: Some(costae::PanelAnchor::Left),
            content: serde_json::Value::Null,
            dpr: 1.0,
        }
    }

    fn wayland_mod_init(specs: &[costae::PanelSpecData]) -> serde_json::Value {
        make_mod_init_value(specs, 1.0, "", 96.0, 0, 0)
    }

    /// Claim: output field must be "" (empty string), NOT "wayland" or any compositor name.
    /// fetch_workspaces in costae-i3 filters all workspaces when output is non-empty.
    #[test]
    fn mod_init_wayland_output_is_empty_string() {
        let result = wayland_mod_init(&[left_spec(250)]);
        assert_eq!(result["output"].as_str(), Some(""),
            "output must be empty string — if it is \"wayland\", fetch_workspaces filters all workspaces");
    }

    #[test]
    fn mod_init_type_is_init() {
        let result = wayland_mod_init(&[left_spec(250)]);
        assert_eq!(result["type"].as_str(), Some("init"));
    }

    /// Claim: config.width must match the width of the left-anchored spec (no dpr scaling at 1.0).
    #[test]
    fn mod_init_config_width_matches_left_anchor_spec() {
        let result = wayland_mod_init(&[left_spec(320)]);
        assert_eq!(result["config"]["width"].as_u64(), Some(320),
            "config.width must match the left-anchored spec width");
    }

    #[test]
    fn stream_calls_to_specs_maps_calls_to_process_sources() {
        let calls = vec![
            ("bash".to_string(), None),
            ("python".to_string(), Some("print('hi')".to_string())),
        ];
        let specs = stream_calls_to_specs(&calls);
        assert_eq!(specs.len(), 2);
        let StreamSource::Process(ref s0) = specs[0] else { panic!("expected Process") };
        assert_eq!(s0.identity.bin, "bash");
        assert_eq!(s0.script, None);
        let StreamSource::Process(ref s1) = specs[1] else { panic!("expected Process") };
        assert_eq!(s1.identity.bin, "python");
        assert_eq!(s1.script, Some("print('hi')".to_string()));
    }

    #[test]
    fn stream_calls_to_specs_routes_costae_prefix_to_builtin() {
        let calls = vec![("costae:outputs".to_string(), None)];
        let specs = stream_calls_to_specs(&calls);
        assert_eq!(specs.len(), 1);
        assert!(matches!(specs[0], StreamSource::BuiltIn(_)), "costae: prefix must map to BuiltIn");
    }

    /// Claim: when theme.file is set to a tilde path in config, load_theme_from_config returns the
    /// tilde-expanded absolute path as the third tuple element.
    #[test]
    fn load_theme_from_config_returns_expanded_path_when_file_is_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.yaml");
        std::fs::write(&config_path, "theme:\n  file: ~/some/theme.yaml\n")
            .expect("write config");

        let (_theme, _mode, path) = load_theme_from_config(&config_path);

        let home = std::env::var("HOME").expect("HOME must be set");
        let expected = PathBuf::from(&home).join("some/theme.yaml");
        assert_eq!(path, Some(expected),
            "tilde in theme.file must be expanded to the real HOME directory");
    }

    /// Claim: when no theme.file is configured, load_theme_from_config returns None as the third
    /// tuple element so the caller knows there is no file to watch.
    #[test]
    fn load_theme_from_config_returns_none_when_no_file_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.yaml");
        std::fs::write(&config_path, "theme:\n  mode: dark\n")
            .expect("write config");

        let (_theme, _mode, path) = load_theme_from_config(&config_path);

        assert_eq!(path, None,
            "when no theme.file is set, the returned path must be None");
    }

    /// Claim: when a theme file path is provided, `theme_file_watch_desired` returns a
    /// single-element Vec whose entry has the given path as its key — so the caller can
    /// reconcile a ManagedSet<WatchedPath> to watch that file.
    #[test]
    fn theme_file_watch_desired_with_some_path_returns_single_entry_with_that_path() {
        let path = PathBuf::from("/tmp/my-theme.yaml");
        let desired = theme_file_watch_desired(Some(path.clone()));
        assert_eq!(desired.len(), 1,
            "Some(path) must produce exactly one desired watch entry");
        use costae::managed_set::Lifecycle;
        assert_eq!(desired[0].key(), path,
            "the entry's key must be the supplied path");
    }

    /// Claim: when no theme file path is present, `theme_file_watch_desired` returns an
    /// empty Vec — so the caller can reconcile a ManagedSet<WatchedPath> to remove any
    /// previously-registered theme watch.
    #[test]
    fn theme_file_watch_desired_with_none_returns_empty_vec() {
        let desired = theme_file_watch_desired(None);
        assert!(desired.is_empty(),
            "None must produce an empty desired set so the old watch is removed");
    }
}
