//! Every subprocess a layout reads from, reconciled once per tick.
//!
//! A layout does not start or stop anything. It re-declares, on every tick, which
//! subprocesses it wants to read from; this module diffs that set against the running one
//! and spawns, keeps, or kills accordingly. The identity that decides "same process" is
//! the `(bin, script)` pair — so two components asking for the same clock share one
//! subprocess without either knowing about the other (ADR 0009).
//!
//! The sharp edge is that a *changed spec* restarts a subprocess, and registering a bin
//! as a module changes its spec. A hook called inside a branch therefore restarts its
//! process on every transition — for a singleton like `tauler-notify` that means dropped
//! notifications and a briefly released D-Bus name. Hooks are meant to be called
//! unconditionally, at the same level of the same component.

mod builtin;

pub use builtin::{BuiltInSource, BuiltInState};
pub use optative_process_pool::{
    ProcessIdentity, ProcessSpec, ProcessState, ProcessSupervisor, Resource, SpawnError,
};

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// The shortest a pass may take, so bursts have a pass to be batched in.
///
/// Not a frame rate and not a poll period: nothing here runs on a schedule. It
/// bounds how fast the loop may spin when every pass is cheap, and it is the
/// window in which a flood of pointer events collapses to one.
const COALESCING_FLOOR: Duration = Duration::from_millis(2);

/// How often the loop runs with nothing to do.
///
/// Everything with work pings the loop, so this is no longer input latency — it
/// is how long a crashed subprocess stays dead, and how stale the freeze
/// watchdog's heartbeat may get. A dead subprocess signals nothing, so this
/// timer is the only thing that finds it, which makes the budget the
/// Non-interactive one: 400ms. The watchdog tolerates ten seconds
/// (`FREEZE_STALE_THRESHOLD_SECS`), so it is not the binding constraint.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(400);

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
    notify_tx: mpsc::SyncSender<()>,
}

impl DataLoopHandle {
    /// Declare which subprocesses should exist.
    ///
    /// Pings, because a layout that just asked for a stream is waiting on it:
    /// without one the subprocess would not be spawned until the supervision
    /// timer came round, and that timer is sized for finding a *crashed*
    /// process, not for starting one somebody just declared.
    pub fn set_desired(&self, sources: Vec<StreamSource>) {
        let _ = self.tx.send(sources);
        let _ = self.notify_tx.try_send(());
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

/// Forward every item from `rx` onto the loop's one item channel, converting on
/// the way, and ping the loop each time.
///
/// The ping is `try_send` on a capacity-1 channel: if one is already waiting,
/// dropping this one loses nothing, because the pass it wakes drains everything.
fn spawn_bridge<T: Send + 'static>(
    rx: mpsc::Receiver<T>,
    items_tx: mpsc::Sender<StreamItem>,
    notify_tx: mpsc::SyncSender<()>,
    convert: impl Fn(T) -> StreamItem + Send + 'static,
) {
    std::thread::spawn(move || {
        while let Ok(item) = rx.recv() {
            if items_tx.send(convert(item)).is_err() {
                break;
            }
            let _ = notify_tx.try_send(());
        }
    });
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
    /// Every source's lines, already converted, from both bridges.
    items_rx: mpsc::Receiver<StreamItem>,
    /// The one thing the loop waits on. Anything with work for the loop pings
    /// it; capacity is 1 and sends are `try_send`, so a burst of a thousand
    /// pings coalesces to one and the pass that follows drains the lot.
    notify_rx: mpsc::Receiver<()>,
    notify_tx: mpsc::SyncSender<()>,
    desired_rx: mpsc::Receiver<Vec<StreamSource>>,
    /// Shared snapshot of event senders, keyed by bin name.
    /// Updated on every `set_desired` call so callers outside `run` can route events.
    event_txs_snapshot: Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>,
}

impl DataLoop {
    pub fn new() -> (Self, DataLoopHandle) {
        let (local_tx, local_rx) = mpsc::channel();
        let (crate_tx, crate_rx) = mpsc::channel::<optative_process_pool::StreamItem>();
        let (desired_tx, desired_rx) = mpsc::channel();
        let (notify_tx, notify_rx) = mpsc::sync_channel::<()>(1);
        let (items_tx, items_rx) = mpsc::channel::<StreamItem>();

        // Two bridges, because the loop waits on one thing and these are the two
        // sources that cannot ping it themselves: the process pool is an
        // external crate that only knows how to send its own item type, and a
        // built-in source is handed a plain sender. Converting here is also what
        // leaves `try_recv_item` with a single channel to read.
        spawn_bridge(crate_rx, items_tx.clone(), notify_tx.clone(), |item| {
            StreamItem {
                key: identity_to_stream_key(&item.key),
                stream: match item.stream {
                    optative_process_pool::StreamKind::Stdout => StreamKind::Stdout,
                    optative_process_pool::StreamKind::Stderr => StreamKind::Stderr,
                },
                line: item.line,
            }
        });
        spawn_bridge(local_rx, items_tx, notify_tx.clone(), |item| item);

        let event_txs_snapshot = Arc::new(Mutex::new(HashMap::new()));
        let data_loop = Self {
            process_supervisor: ProcessSupervisor::new(crate_tx),
            builtin_pool: BuiltInPool::new(local_tx),
            desired_processes: Vec::new(),
            desired_builtins: Vec::new(),
            items_rx,
            notify_rx,
            notify_tx,
            desired_rx,
            event_txs_snapshot,
        };
        let handle = DataLoopHandle {
            tx: desired_tx,
            notify_tx: data_loop.notify_tx.clone(),
        };
        (data_loop, handle)
    }

    /// Tear down every subprocess, and wait for each one to actually die.
    ///
    /// Must be called before the process exits or re-execs. `exec` keeps the
    /// PID and runs no destructors, so anything still tracked here survives into
    /// the new image as a child nothing holds a handle to — and `Child`'s own
    /// `Drop` never kills or reaps. Such a child cannot be waited on by anyone
    /// afterwards, so it becomes a zombie the moment it exits and stays one for
    /// the life of the process.
    ///
    /// Reconciling against an empty desired set is what does it: each process
    /// goes through `Lifecycle::exit`, which is SIGTERM, then SIGKILL after a
    /// grace period, then a wait.
    pub fn shutdown(&mut self) {
        log_lifecycle_errors(self.process_supervisor.shutdown_all());
        log_lifecycle_errors(self.builtin_pool.reconcile(vec![]));
    }

    /// How many subprocesses are still tracked. Shutdown drives this to zero.
    pub fn tracked_processes(&self) -> usize {
        self.process_supervisor.iter().count()
    }

    /// A handle anything can use to say "there is work for the loop".
    ///
    /// Holding one is what lets a source end the loop's wait instead of sitting
    /// in a channel until the supervision timer expires.
    pub fn notifier(&self) -> mpsc::SyncSender<()> {
        self.notify_tx.clone()
    }

    /// Returns a clone of the shared event_txs snapshot Arc.
    /// Callers can hold this Arc and read from it while `run` is executing.
    pub fn event_txs_handle(&self) -> Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>> {
        Arc::clone(&self.event_txs_snapshot)
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

    /// One item, whichever bridge put it there.
    fn try_recv_item(&self) -> Result<StreamItem, mpsc::TryRecvError> {
        self.items_rx.try_recv()
    }

    pub fn run(
        &mut self,
        stop: Arc<AtomicBool>,
        mut on_item: impl FnMut(StreamItem),
        mut on_tick: impl FnMut(),
    ) {
        loop {
            let pass_started = Instant::now();

            if stop.load(Ordering::Relaxed) {
                break;
            }

            // Drain desired_rx: apply any new desired sets sent via DataLoopHandle.
            while let Ok(sources) = self.desired_rx.try_recv() {
                self.set_desired(sources);
            }

            // Reconcile: enter new, exit removed, update existing (restarts crashed processes).
            let proc_errors = self
                .process_supervisor
                .reconcile(self.desired_processes.clone());
            log_lifecycle_errors(proc_errors);
            let builtin_errors = self.builtin_pool.reconcile(self.desired_builtins.clone());
            log_lifecycle_errors(builtin_errors);
            self.update_event_txs_snapshot();

            // Every line that arrived since the last pass, before the tick that
            // reads them — so a value is acted on in the pass it arrives in
            // rather than the one after.
            loop {
                match self.try_recv_item() {
                    Ok(item) => on_item(item),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            on_tick();

            // `on_tick` is where the stop flag gets set — a replaced binary asks
            // for a re-exec from inside it. Checking here rather than only at the
            // top means we do not wait out a supervision interval first.
            if stop.load(Ordering::Relaxed) {
                break;
            }

            // Hold the floor before waiting again. Passes are what batches get
            // formed in, so a pass that finishes in microseconds would leave
            // nothing to batch — and would spin.
            if let Some(rest) = COALESCING_FLOOR.checked_sub(pass_started.elapsed()) {
                std::thread::sleep(rest);
            }

            // One thing to wait on. The timeout is not a poll for input any
            // more — everything with work pings us — it is how often
            // supervision runs when the desktop is idle.
            match self.notify_rx.recv_timeout(SUPERVISION_INTERVAL) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
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

    /// Nothing may still be tracked after a shutdown, because the caller is
    /// about to `exec` and anything left is a child the next image cannot reap.
    #[test]
    fn shutdown_leaves_no_process_tracked() {
        let (mut data_loop, handle) = DataLoop::new();
        handle.set_desired(vec![StreamSource::Process(ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "/bin/sh:sleep".to_string(),
            },
            args: vec!["-c".into(), "sleep 30".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        })]);

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);
        let notifier = data_loop.notifier();
        let run = thread::spawn(move || {
            data_loop.run(stop_for_run, |_| {}, || {});
            data_loop
        });

        // Give the pass that spawns it time to happen, then stop the loop.
        thread::sleep(Duration::from_millis(200));
        stop.store(true, Ordering::Relaxed);
        let _ = notifier.try_send(());
        let mut data_loop = run.join().expect("run thread");

        assert_eq!(
            data_loop.tracked_processes(),
            1,
            "the desired process should be running before shutdown"
        );
        data_loop.shutdown();
        assert_eq!(
            data_loop.tracked_processes(),
            0,
            "shutdown must leave nothing for the next image to inherit"
        );
    }

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

        // Not `len() == 1`: the script exits after echoing, so the supervisor is
        // entitled to restart it and produce the line again before the stop flag
        // is seen. What the test is about is that the script content ran at all.
        let items = items.lock().unwrap();
        let item = items.first().expect("no output from the script");
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

        // A process that exits signals nothing — only the supervision timer
        // finds it — so a restart cannot be seen sooner than
        // `SUPERVISION_INTERVAL`. Wait out two of them rather than racing one.
        thread::sleep(SUPERVISION_INTERVAL * 2 + Duration::from_millis(100));

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

    /// The loop waits on the notifier and nothing else, so anything holding one
    /// can end that wait.
    ///
    /// Measured by how fast a stop is noticed: the loop is idle and blocked when
    /// the ping arrives, so if the ping did nothing the stop would not be seen
    /// until `SUPERVISION_INTERVAL` expired. Before this change a wake signal
    /// could not interrupt a blocking receive at all — it only skipped the
    /// *next* one, which is why a pointer event could sit for a whole poll
    /// period.
    #[test]
    fn a_notify_ping_ends_the_wait() {
        let (mut data_loop, _handle) = DataLoop::new();
        let wake_tx = data_loop.notifier();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);
        let run_handle = thread::spawn(move || {
            data_loop.run(stop_for_run, |_item| {}, || {});
        });

        // Long enough that the loop has finished its first pass and is blocked,
        // short enough that the supervision timer has not fired.
        thread::sleep(SUPERVISION_INTERVAL / 4);

        let pinged = Instant::now();
        stop.store(true, Ordering::Relaxed);
        let _ = wake_tx.try_send(());
        let _ = run_handle.join();
        let noticed = pinged.elapsed();

        assert!(
            noticed < SUPERVISION_INTERVAL / 2,
            "the ping did not end the wait: the stop took {noticed:?}, which is \
             the supervision timer firing rather than the ping"
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
