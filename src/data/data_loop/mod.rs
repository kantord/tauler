mod builtin;

pub use builtin::{BuiltInSource, BuiltInState};
pub use optative_process_pool::{
    ProcessIdentity, ProcessSpec, ProcessState, ProcessSupervisor, Resource, SpawnError,
};

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::managed_set::reconcile::ReconcileErrors;
use crate::managed_set::{OptativeSet, Reconcile};
use optative_derive::Ephemeral;

#[derive(Debug, PartialEq, Eq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct StreamItem {
    pub key: (String, Option<String>),
    pub stream: StreamKind,
    pub line: String,
}

pub enum StreamSource {
    Process(ProcessSpec),
    BuiltIn(BuiltInSource),
}

fn log_lifecycle_errors<K: Debug, E: Debug>(errors: Vec<(K, E)>) {
    for (key, err) in errors {
        tracing::error!(key = ?key, error = ?err, "lifecycle error");
    }
}

pub struct DataLoopHandle {
    tx: mpsc::Sender<Vec<StreamSource>>,
}

impl DataLoopHandle {
    pub fn set_desired(&self, sources: Vec<StreamSource>) {
        let _ = self.tx.send(sources);
    }
}

#[derive(Ephemeral)]
struct BuiltInPool {
    #[reconciler(output = stream_tx)]
    inner: OptativeSet<BuiltInSource>,
    stream_tx: mpsc::Sender<StreamItem>,
}

impl BuiltInPool {
    fn new(stream_tx: mpsc::Sender<StreamItem>) -> Self {
        Self {
            inner: OptativeSet::new(),
            stream_tx,
        }
    }
    fn reconcile(
        &mut self,
        desired: Vec<BuiltInSource>,
    ) -> ReconcileErrors<String, std::convert::Infallible> {
        self.inner.reconcile(desired, &mut (), &mut self.stream_tx)
    }
}

/// Convert the crate's stream key (ProcessIdentity) back to tauler's
/// (bin, Option<script>) tuple. The convention is that `identity.key`
/// is formatted as `"bin:script"` (with an empty script when there is none).
fn identity_to_stream_key(identity: &ProcessIdentity) -> (String, Option<String>) {
    let bin = identity.bin.clone();
    let prefix = format!("{}:", bin);
    let script_part = identity.key.strip_prefix(&prefix).unwrap_or("");
    if script_part.is_empty() {
        (bin, None)
    } else {
        (bin, Some(script_part.to_string()))
    }
}

pub struct DataLoop {
    process_supervisor: ProcessSupervisor,
    builtin_pool: BuiltInPool,
    desired_processes: Vec<ProcessSpec>,
    desired_builtins: Vec<BuiltInSource>,
    timeout: Option<Duration>,
    /// Receives tauler-local StreamItems from BuiltIn sources.
    local_rx: mpsc::Receiver<StreamItem>,
    /// Receives crate StreamItems from the process supervisor.
    crate_rx: mpsc::Receiver<optative_process_pool::StreamItem>,
    extra_rx: Option<mpsc::Receiver<()>>,
    desired_rx: mpsc::Receiver<Vec<StreamSource>>,
    /// Shared snapshot of event senders, keyed by bin name.
    /// Updated on every `set_desired` call so callers outside `run` can route events.
    event_txs_snapshot: Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>,
}

impl DataLoop {
    pub fn new() -> (Self, DataLoopHandle) {
        let (local_tx, local_rx) = mpsc::channel();
        let (crate_tx, crate_rx) = mpsc::channel();
        let (desired_tx, desired_rx) = mpsc::channel();
        let event_txs_snapshot = Arc::new(Mutex::new(HashMap::new()));
        let data_loop = Self {
            process_supervisor: ProcessSupervisor::new(crate_tx),
            builtin_pool: BuiltInPool::new(local_tx),
            desired_processes: Vec::new(),
            desired_builtins: Vec::new(),
            timeout: None,
            local_rx,
            crate_rx,
            extra_rx: None,
            desired_rx,
            event_txs_snapshot,
        };
        let handle = DataLoopHandle { tx: desired_tx };
        (data_loop, handle)
    }

    /// Returns a clone of the shared event_txs snapshot Arc.
    /// Callers can hold this Arc and read from it while `run` is executing.
    pub fn event_txs_handle(&self) -> Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>> {
        Arc::clone(&self.event_txs_snapshot)
    }

    pub fn with_extra_rx(mut self, rx: mpsc::Receiver<()>) -> Self {
        self.extra_rx = Some(rx);
        self
    }

    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn collect_event_txs(&self) -> HashMap<ProcessIdentity, mpsc::Sender<serde_json::Value>> {
        self.process_supervisor
            .iter()
            .map(|(identity, state)| (identity.clone(), state.event_tx.clone()))
            .collect()
    }

    pub fn send_event(&mut self, identity: &ProcessIdentity, event: serde_json::Value) {
        while let Ok(sources) = self.desired_rx.try_recv() {
            self.set_desired(sources);
        }
        let errors = self
            .process_supervisor
            .reconcile(self.desired_processes.clone());
        log_lifecycle_errors(errors);
        if let Some(state) = self.process_supervisor.get(identity) {
            let _ = state.event_tx.send(event);
        }
    }

    fn set_desired(&mut self, sources: Vec<StreamSource>) {
        let mut processes = vec![];
        let mut builtins = vec![];
        for s in sources {
            match s {
                StreamSource::Process(p) => processes.push(p),
                StreamSource::BuiltIn(b) => builtins.push(b),
            }
        }
        let mut seen = std::collections::HashSet::new();
        self.desired_processes = processes
            .into_iter()
            .filter(|s| seen.insert(s.identity.clone()))
            .collect();
        self.desired_builtins = builtins;
        let proc_errors = self
            .process_supervisor
            .reconcile(self.desired_processes.clone());
        log_lifecycle_errors(proc_errors);
        let builtin_errors = self.builtin_pool.reconcile(self.desired_builtins.clone());
        log_lifecycle_errors(builtin_errors);
        self.update_event_txs_snapshot();
    }

    fn update_event_txs_snapshot(&self) {
        let mut snapshot = self.event_txs_snapshot.lock().unwrap();
        *snapshot = self
            .process_supervisor
            .iter()
            .map(|(identity, state)| (identity.bin.clone(), state.event_tx.clone()))
            .collect();
    }

    /// Try to receive one item from either the crate's process pool channel or
    /// the local builtin channel, converting to tauler's StreamItem.
    fn try_recv_item(&self) -> Result<StreamItem, mpsc::TryRecvError> {
        // Try the crate channel first (processes), then local (builtins).
        match self.crate_rx.try_recv() {
            Ok(crate_item) => {
                let key = identity_to_stream_key(&crate_item.key);
                let stream = match crate_item.stream {
                    optative_process_pool::StreamKind::Stdout => StreamKind::Stdout,
                    optative_process_pool::StreamKind::Stderr => StreamKind::Stderr,
                };
                Ok(StreamItem {
                    key,
                    stream,
                    line: crate_item.line,
                })
            }
            Err(mpsc::TryRecvError::Empty) => self.local_rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected) => {
                // Crate channel disconnected, try local.
                match self.local_rx.try_recv() {
                    Ok(item) => Ok(item),
                    Err(_) => Err(mpsc::TryRecvError::Disconnected),
                }
            }
        }
    }

    /// Blocking receive with timeout from either channel.
    fn recv_timeout_item(&self, timeout: Duration) -> Result<StreamItem, mpsc::RecvTimeoutError> {
        // Try non-blocking first.
        match self.try_recv_item() {
            Ok(item) => return Ok(item),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(mpsc::RecvTimeoutError::Disconnected)
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        // Block on the crate channel with the given timeout, then check local.
        match self.crate_rx.recv_timeout(timeout) {
            Ok(crate_item) => {
                let key = identity_to_stream_key(&crate_item.key);
                let stream = match crate_item.stream {
                    optative_process_pool::StreamKind::Stdout => StreamKind::Stdout,
                    optative_process_pool::StreamKind::Stderr => StreamKind::Stderr,
                };
                Ok(StreamItem {
                    key,
                    stream,
                    line: crate_item.line,
                })
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check local one more time before returning timeout.
                match self.local_rx.try_recv() {
                    Ok(item) => Ok(item),
                    Err(_) => Err(mpsc::RecvTimeoutError::Timeout),
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Try local before giving up.
                match self.local_rx.try_recv() {
                    Ok(item) => Ok(item),
                    Err(_) => Err(mpsc::RecvTimeoutError::Disconnected),
                }
            }
        }
    }

    pub fn run(
        &mut self,
        stop: Arc<AtomicBool>,
        mut on_item: impl FnMut(StreamItem),
        mut on_tick: impl FnMut(),
    ) {
        let mut awake = false;
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }

            // Drain desired_rx: apply any new desired sets sent via DataLoopHandle.
            while let Ok(sources) = self.desired_rx.try_recv() {
                self.set_desired(sources);
            }

            // Check extra_rx: if a message arrives, stay awake (no blocking recv) for the
            // rest of the run so the stop flag and further ticks are detected promptly.
            // If the extra_rx sender is dropped, treat that as a stop signal.
            if let Some(ref extra_rx) = self.extra_rx {
                match extra_rx.try_recv() {
                    Ok(()) => {
                        awake = true;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => break,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }

            // Reconcile: enter new, exit removed, update existing (restarts crashed processes).
            let proc_errors = self
                .process_supervisor
                .reconcile(self.desired_processes.clone());
            log_lifecycle_errors(proc_errors);
            let builtin_errors = self.builtin_pool.reconcile(self.desired_builtins.clone());
            log_lifecycle_errors(builtin_errors);
            self.update_event_txs_snapshot();

            on_tick();

            if awake {
                awake = false;
                match self.try_recv_item() {
                    Ok(item) => on_item(item),
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
                continue;
            }

            match self.recv_timeout_item(Duration::from_millis(50)) {
                Ok(item) => on_item(item),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn data_loop_new_returns_tuple_with_handle() {
        let (_data_loop, _handle): (DataLoop, DataLoopHandle) = DataLoop::new();
    }

    #[test]
    fn script_content_is_executed_and_output_delivered() {
        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh:echo from_script".to_string(),
            },
            args: vec![Resource::File {
                content: "echo from_script".to_string(),
            }],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec.clone())]);

        let items: Arc<Mutex<Vec<StreamItem>>> = Arc::new(Mutex::new(Vec::new()));
        let items_clone = Arc::clone(&items);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        data_loop.run(
            stop_for_run,
            |item| {
                items_clone.lock().unwrap().push(item);
                stop.store(true, Ordering::Relaxed);
            },
            || {},
        );

        let items = items.lock().unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(
            item.line, "from_script",
            "expected output from script content, got {:?}",
            item.line
        );
        assert_eq!(item.stream, StreamKind::Stdout);
    }

    #[test]
    fn duplicate_specs_without_key_spawn_only_one_process() {
        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh:".to_string(),
            },
            args: vec!["-c".into(), "echo hello; sleep 10".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![
            StreamSource::Process(spec.clone()),
            StreamSource::Process(spec.clone()),
        ]);

        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        thread::spawn(move || {
            data_loop.run(
                stop_clone,
                |item| {
                    let mut guard = collected_clone.lock().unwrap();
                    guard.push(item.line);
                    if guard.len() >= 2 {
                        stop.store(true, Ordering::Relaxed);
                    }
                },
                || {},
            );
        });

        thread::sleep(Duration::from_millis(500));

        let items = collected.lock().unwrap();
        let len = items.len();
        assert_eq!(
            len, 1,
            "expected exactly one process to be spawned for duplicate specs, \
             got {} items: {:?}",
            len, *items
        );
    }

    #[test]
    fn stdout_line_is_delivered_to_handler_with_correct_source_and_kind() {
        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh:".to_string(),
            },
            args: vec!["-c".into(), "echo hello".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec.clone())]);

        let items: Arc<Mutex<Vec<StreamItem>>> = Arc::new(Mutex::new(Vec::new()));
        let items_clone = Arc::clone(&items);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        data_loop.run(
            stop_for_run,
            |item| {
                items_clone.lock().unwrap().push(item);
                stop.store(true, Ordering::Relaxed);
            },
            || {},
        );

        let items = items.lock().unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.line, "hello");
        assert_eq!(item.key.0, spec.identity.bin);
        assert_eq!(item.stream, StreamKind::Stdout);
    }

    #[test]
    fn crashed_process_is_restarted_and_output_continues() {
        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh:".to_string(),
            },
            args: vec!["-c".into(), "echo hello".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec.clone())]);

        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_for_run = Arc::clone(&collected);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        let run_handle = thread::spawn(move || {
            data_loop.run(
                stop_for_run,
                |item| {
                    collected_for_run.lock().unwrap().push(item.line);
                },
                || {},
            );
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if !collected.lock().unwrap().is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for first output line"
            );
            thread::sleep(Duration::from_millis(20));
        }

        thread::sleep(Duration::from_millis(300));

        let count = collected.lock().unwrap().len();
        stop.store(true, Ordering::Relaxed);
        let _ = run_handle.join();

        assert!(
            count >= 2,
            "expected at least 2 output lines (original + restart), got {}",
            count
        );
    }

    #[test]
    fn run_stops_when_cancellation_token_is_set() {
        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh:".to_string(),
            },
            args: vec![
                "-c".into(),
                "while true; do echo tick; sleep 0.1; done".into(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec)]);

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);
        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_for_run = Arc::clone(&collected);

        let run_handle = thread::spawn(move || {
            data_loop.run(
                stop_for_run,
                |item| {
                    collected_for_run.lock().unwrap().push(item.line);
                },
                || {},
            );
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if !collected.lock().unwrap().is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for first tick"
            );
            thread::sleep(Duration::from_millis(20));
        }

        stop.store(true, Ordering::Relaxed);

        let joined = run_handle.join();
        assert!(
            joined.is_ok(),
            "run() thread panicked or did not stop after cancellation token was set"
        );
    }

    #[test]
    fn run_accepts_on_tick_callback() {
        let (mut data_loop, _handle) = DataLoop::new();
        let stop = Arc::new(AtomicBool::new(true));
        let tick_called = Arc::new(Mutex::new(false));
        let tick_called_clone = Arc::clone(&tick_called);

        data_loop.run(
            stop,
            |_item: StreamItem| {},
            move || {
                *tick_called_clone.lock().unwrap() = true;
            },
        );
    }

    #[test]
    fn extra_rx_wake_calls_on_tick_within_deadline() {
        let (wake_tx, wake_rx) = mpsc::channel::<()>();

        let (data_loop, _handle) = DataLoop::new();
        let mut data_loop = data_loop.with_extra_rx(wake_rx);

        let tick_called = Arc::new(AtomicBool::new(false));
        let tick_called_for_cb = Arc::clone(&tick_called);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);
        let stop_for_wake = Arc::clone(&stop);

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            let _ = wake_tx.send(());
            thread::sleep(Duration::from_millis(100));
            stop_for_wake.store(true, Ordering::Relaxed);
        });

        let start = std::time::Instant::now();
        data_loop.run(
            stop_for_run,
            |_item| {},
            move || {
                tick_called_for_cb.store(true, Ordering::Relaxed);
            },
        );
        let elapsed = start.elapsed();

        assert!(
            tick_called.load(Ordering::Relaxed),
            "on_tick was never called after extra_rx wake signal"
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "on_tick was not called within 200 ms deadline (took {:?})",
            elapsed
        );
    }

    #[test]
    fn props_init_message_is_sent_to_subprocess_stdin() {
        let props_value = serde_json::json!({"color": "red"});
        let expected_payload = serde_json::json!({"color": "red"});
        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "init-test".to_string(),
            },
            args: vec!["-c".into(), "read line; echo \"got:$line\"".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(props_value),
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec.clone())]);

        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        let run_handle = thread::spawn(move || {
            data_loop.run(
                stop_for_run,
                |item| {
                    collected_clone.lock().unwrap().push(item.line);
                },
                || {},
            );
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if !collected.lock().unwrap().is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for subprocess to echo init message"
            );
            thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Relaxed);
        let _ = run_handle.join();

        let items = collected.lock().unwrap();
        let expected_got = format!("got:{}", expected_payload);
        assert!(
            items.iter().any(|l| l == &expected_got),
            "expected echoed init payload {:?}, got: {:?}",
            expected_got,
            *items
        );
    }

    #[test]
    fn props_update_message_is_sent_to_subprocess_stdin_on_spec_update() {
        let initial_props = serde_json::json!({"step": 1});
        let updated_props = serde_json::json!({"step": 2});
        let expected_update_payload = serde_json::json!({"step": 2});

        let identity = ProcessIdentity {
            bin: "/bin/sh".to_string(),
            key: "update-test".to_string(),
        };

        let spec_v1 = ProcessSpec {
            identity: identity.clone(),
            args: vec![
                "-c".into(),
                "while read line; do echo \"got:$line\"; done".into(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(initial_props),
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec_v1.clone())]);

        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        let run_handle = thread::spawn(move || {
            data_loop.run(
                stop_for_run,
                |item| {
                    collected_clone.lock().unwrap().push(item.line);
                },
                || {},
            );
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if !collected.lock().unwrap().is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for subprocess to echo init message"
            );
            thread::sleep(Duration::from_millis(20));
        }

        let spec_v2 = ProcessSpec {
            identity: identity.clone(),
            args: vec![
                "-c".into(),
                "while read line; do echo \"got:$line\"; done".into(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(updated_props),
        };
        handle.set_desired(vec![StreamSource::Process(spec_v2)]);

        let expected_got = format!("got:{}", expected_update_payload);
        let update_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if collected.lock().unwrap().iter().any(|l| l == &expected_got) {
                break;
            }
            assert!(
                std::time::Instant::now() < update_deadline,
                "timed out waiting for subprocess to echo update message"
            );
            thread::sleep(Duration::from_millis(20));
        }

        thread::sleep(Duration::from_millis(150));

        stop.store(true, Ordering::Relaxed);
        let _ = run_handle.join();

        let items = collected.lock().unwrap();
        let count = items.iter().filter(|l| l.as_str() == expected_got).count();
        assert_eq!(
            count, 1,
            "expected updated props payload to be sent exactly once, but got {} occurrences: {:?}",
            count, *items
        );
    }

    #[test]
    fn send_event_writes_arbitrary_json_to_subprocess_stdin() {
        let identity = ProcessIdentity {
            bin: "/bin/sh".to_string(),
            key: "send-event-test".to_string(),
        };
        let spec = ProcessSpec {
            identity: identity.clone(),
            args: vec![
                "-c".into(),
                "while read line; do echo \"got:$line\"; done".into(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec)]);

        let event = serde_json::json!({"type": "ping", "id": 42});
        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        let run_handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            data_loop.send_event(&identity, event.clone());
            data_loop.run(
                stop_for_run,
                |item| {
                    collected_clone.lock().unwrap().push(item.line);
                },
                || {},
            );
        });

        let expected_got = format!("got:{}", serde_json::json!({"type": "ping", "id": 42}));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if collected.lock().unwrap().iter().any(|l| l == &expected_got) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for send_event echo"
            );
            thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Relaxed);
        let _ = run_handle.join();

        let items = collected.lock().unwrap();
        assert!(
            items.iter().any(|l| l == &expected_got),
            "expected echoed event payload {:?}, got: {:?}",
            expected_got,
            *items
        );
    }

    #[test]
    fn identical_props_sent_only_once_on_consecutive_set_desired() {
        let props_value = serde_json::json!({"step": 99});
        let identity = ProcessIdentity {
            bin: "/bin/sh".to_string(),
            key: "dedup-props-test".to_string(),
        };

        let spec = ProcessSpec {
            identity: identity.clone(),
            args: vec![
                "-c".into(),
                "while read line; do echo \"got:$line\"; done".into(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(props_value.clone()),
        };

        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(spec.clone())]);

        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        let run_handle = thread::spawn(move || {
            data_loop.run(
                stop_for_run,
                |item| {
                    collected_clone.lock().unwrap().push(item.line);
                },
                || {},
            );
        });

        let expected_got = format!("got:{}", props_value);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if collected.lock().unwrap().iter().any(|l| l == &expected_got) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for first props echo"
            );
            thread::sleep(Duration::from_millis(20));
        }

        handle.set_desired(vec![StreamSource::Process(spec.clone())]);

        thread::sleep(Duration::from_millis(300));

        stop.store(true, Ordering::Relaxed);
        let _ = run_handle.join();

        let items = collected.lock().unwrap();
        let count = items.iter().filter(|l| l.as_str() == expected_got).count();
        assert_eq!(
            count, 1,
            "expected props payload to be delivered exactly once, but got {} occurrences: {:?}",
            count, *items
        );
    }

    #[test]
    fn handle_set_desired_spawns_process_into_running_loop() {
        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh:".to_string(),
            },
            args: vec!["-c".into(), "echo handle_output".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        let (mut data_loop, handle) = DataLoop::new();

        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_for_run = Arc::clone(&collected);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);

        thread::spawn(move || {
            data_loop.run(
                stop_for_run,
                |item| {
                    collected_for_run.lock().unwrap().push(item.line);
                },
                || {},
            );
        });

        handle.set_desired(vec![StreamSource::Process(spec)]);

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if !collected.lock().unwrap().is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for output from handle-spawned process"
            );
            thread::sleep(Duration::from_millis(20));
        }

        stop.store(true, Ordering::Relaxed);

        let items = collected.lock().unwrap();
        assert!(
            items.iter().any(|l| l == "handle_output"),
            "expected 'handle_output' in collected lines, got: {:?}",
            *items
        );
    }
}
