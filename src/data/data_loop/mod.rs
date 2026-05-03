mod builtin;
mod process;

pub use builtin::{BuiltInSource, BuiltInState};
pub use process::{ProcessIdentity, ProcessSource, ProcessState, SpawnError};

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::managed_set::reconcile::ReconcileErrors;
use crate::managed_set::{ManagedSet, Reconcile};
use tauler_lifecycle_derive::Ephemeral;

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
    Process(ProcessSource),
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
struct ProcessPool {
    #[reconciler(output = stream_tx)]
    inner: ManagedSet<ProcessSource>,
    stream_tx: mpsc::Sender<StreamItem>,
}

impl ProcessPool {
    fn new(stream_tx: mpsc::Sender<StreamItem>) -> Self {
        Self {
            inner: ManagedSet::new(),
            stream_tx,
        }
    }
    fn reconcile(
        &mut self,
        desired: Vec<ProcessSource>,
    ) -> ReconcileErrors<ProcessIdentity, SpawnError> {
        self.inner.reconcile(desired, &mut (), &mut self.stream_tx)
    }
    fn get(&self, identity: &ProcessIdentity) -> Option<&ProcessState> {
        self.inner.get(identity)
    }
    fn iter(&self) -> impl Iterator<Item = (&ProcessIdentity, &ProcessState)> {
        self.inner.iter()
    }
}

#[derive(Ephemeral)]
struct BuiltInPool {
    #[reconciler(output = stream_tx)]
    inner: ManagedSet<BuiltInSource>,
    stream_tx: mpsc::Sender<StreamItem>,
}

impl BuiltInPool {
    fn new(stream_tx: mpsc::Sender<StreamItem>) -> Self {
        Self {
            inner: ManagedSet::new(),
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

pub struct DataLoop {
    process_pool: ProcessPool,
    builtin_pool: BuiltInPool,
    desired_processes: Vec<ProcessSource>,
    desired_builtins: Vec<BuiltInSource>,
    timeout: Option<Duration>,
    rx: mpsc::Receiver<StreamItem>,
    extra_rx: Option<mpsc::Receiver<()>>,
    desired_rx: mpsc::Receiver<Vec<StreamSource>>,
    /// Shared snapshot of event senders, keyed by bin name.
    /// Updated on every `set_desired` call so callers outside `run` can route events.
    event_txs_snapshot: Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>,
}

impl DataLoop {
    pub fn new() -> (Self, DataLoopHandle) {
        let (stream_tx, rx) = mpsc::channel();
        let (desired_tx, desired_rx) = mpsc::channel();
        let event_txs_snapshot = Arc::new(Mutex::new(HashMap::new()));
        let data_loop = Self {
            process_pool: ProcessPool::new(stream_tx.clone()),
            builtin_pool: BuiltInPool::new(stream_tx),
            desired_processes: Vec::new(),
            desired_builtins: Vec::new(),
            timeout: None,
            rx,
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
        self.process_pool
            .iter()
            .map(|(identity, state)| (identity.clone(), state.event_tx.clone()))
            .collect()
    }

    pub fn send_event(&mut self, identity: &ProcessIdentity, event: serde_json::Value) {
        while let Ok(sources) = self.desired_rx.try_recv() {
            self.set_desired(sources);
        }
        let errors = self.process_pool.reconcile(self.desired_processes.clone());
        log_lifecycle_errors(errors);
        if let Some(state) = self.process_pool.get(identity) {
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
        let proc_errors = self.process_pool.reconcile(self.desired_processes.clone());
        log_lifecycle_errors(proc_errors);
        let builtin_errors = self.builtin_pool.reconcile(self.desired_builtins.clone());
        log_lifecycle_errors(builtin_errors);
        self.update_event_txs_snapshot();
    }

    fn update_event_txs_snapshot(&self) {
        let mut snapshot = self.event_txs_snapshot.lock().unwrap();
        *snapshot = self
            .process_pool
            .iter()
            .map(|(identity, state)| (identity.bin.clone(), state.event_tx.clone()))
            .collect();
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
            let proc_errors = self.process_pool.reconcile(self.desired_processes.clone());
            log_lifecycle_errors(proc_errors);
            let builtin_errors = self.builtin_pool.reconcile(self.desired_builtins.clone());
            log_lifecycle_errors(builtin_errors);
            self.update_event_txs_snapshot();

            on_tick();

            if awake {
                awake = false;
                match self.rx.try_recv() {
                    Ok(item) => on_item(item),
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
                continue;
            }

            match self.rx.recv_timeout(Duration::from_millis(50)) {
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
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh".to_string(),
            },
            args: vec![],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
            script: Some("echo from_script".to_string()),
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
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh".to_string(),
            },
            args: vec!["-c".to_string(), "echo hello; sleep 10".to_string()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
            script: None,
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
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh".to_string(),
            },
            args: vec!["-c".to_string(), "echo hello".to_string()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
            script: None,
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
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh".to_string(),
            },
            args: vec!["-c".to_string(), "echo hello".to_string()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
            script: None,
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
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh".to_string(),
            },
            args: vec![
                "-c".to_string(),
                "while true; do echo tick; sleep 0.1; done".to_string(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
            script: None,
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
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "init-test".to_string(),
            },
            args: vec![
                "-c".to_string(),
                "read line; echo \"got:$line\"".to_string(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(props_value),
            script: None,
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

        let spec_v1 = ProcessSource {
            identity: identity.clone(),
            args: vec![
                "-c".to_string(),
                "while read line; do echo \"got:$line\"; done".to_string(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(initial_props),
            script: None,
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

        let spec_v2 = ProcessSource {
            identity: identity.clone(),
            args: vec![
                "-c".to_string(),
                "while read line; do echo \"got:$line\"; done".to_string(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(updated_props),
            script: None,
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
        let spec = ProcessSource {
            identity: identity.clone(),
            args: vec![
                "-c".to_string(),
                "while read line; do echo \"got:$line\"; done".to_string(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
            script: None,
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

        let spec = ProcessSource {
            identity: identity.clone(),
            args: vec![
                "-c".to_string(),
                "while read line; do echo \"got:$line\"; done".to_string(),
            ],
            env: BTreeMap::new(),
            current_dir: None,
            props: Some(props_value.clone()),
            script: None,
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
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh".to_string(),
            },
            args: vec!["-c".to_string(), "echo handle_output".to_string()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
            script: None,
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
