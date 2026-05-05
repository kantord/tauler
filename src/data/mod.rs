pub mod data_loop;

use crate::data::data_loop::{ProcessIdentity, ProcessSource, StreamItem, StreamKind};
use std::io::{Seek, SeekFrom, Write as IoWrite};
use std::os::unix::io::FromRawFd;
use std::sync::mpsc;
use std::thread;

pub struct SpawnedModule {
    pub rx: mpsc::Receiver<String>,
    pub child: std::process::Child,
    pub event_tx: mpsc::Sender<serde_json::Value>,
}

impl SpawnedModule {
    pub fn send_event(&self, event: &serde_json::Value) {
        let _ = self.event_tx.send(event.clone());
    }

    /// Consume the struct into its parts without running `Drop`.
    fn into_parts(
        self,
    ) -> (
        mpsc::Receiver<String>,
        std::process::Child,
        mpsc::Sender<serde_json::Value>,
    ) {
        let md = std::mem::ManuallyDrop::new(self);
        let rx = unsafe { std::ptr::read(&md.rx) };
        let child = unsafe { std::ptr::read(&md.child) };
        let event_tx = unsafe { std::ptr::read(&md.event_tx) };
        (rx, child, event_tx)
    }
}

impl Drop for SpawnedModule {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn spawn_module(bin: &str, script: Option<&str>) -> SpawnedModule {
    let (tx, rx) = mpsc::channel();
    let mut cmd = std::process::Command::new(bin);

    // If a script is provided, write it to a memfd and pass the path as argument
    #[allow(clippy::option_if_let_else)]
    let _memfd_file = if let Some(content) = script {
        let fd = unsafe { libc::memfd_create(c"tauler-script".as_ptr(), 0) };
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let _ = file.write_all(content.as_bytes());
        let _ = file.seek(SeekFrom::Start(0));
        cmd.arg(format!("/proc/self/fd/{}", fd));
        Some(file) // keep alive until after spawn so fd is inherited
    } else {
        None
    };

    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn module");
    // _memfd_file can now be dropped — child has inherited the fd

    let stdout = child.stdout.take().expect("no stdout");
    thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        use std::io::BufRead;
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut stdin = child.stdin.take().expect("no stdin");
    let (event_tx, event_rx) = mpsc::channel::<serde_json::Value>();
    thread::spawn(move || {
        while let Ok(event) = event_rx.recv() {
            if writeln!(stdin, "{}", event).is_err() {
                break;
            }
        }
    });

    SpawnedModule {
        rx,
        child,
        event_tx,
    }
}

pub struct SpawnedBiStream {
    pub child: std::process::Child,
    pub event_tx: mpsc::Sender<serde_json::Value>,
}

fn forward_stdout(
    rx: mpsc::Receiver<String>,
    tx: mpsc::Sender<StreamItem>,
    wake_tx: mpsc::SyncSender<()>,
    spec: ProcessSource,
) {
    thread::spawn(move || {
        while let Ok(line) = rx.recv() {
            let item = StreamItem {
                key: (spec.identity.bin.clone(), spec.script.clone()),
                stream: StreamKind::Stdout,
                line,
            };
            if tx.send(item).is_err() {
                break;
            }
            let _ = wake_tx.try_send(());
        }
        tracing::warn!(bin = %spec.identity.bin, script = ?spec.script, "stream subprocess exited");
    });
}

/// Spawn a bidirectional module subprocess (stdin for events, stdout for data).
/// Sends the init event immediately, then forwards stdout lines to `tx` as `StreamItem`.
pub fn spawn_bi_stream(
    bin: &str,
    init_event: &serde_json::Value,
    tx: mpsc::Sender<StreamItem>,
    wake_tx: mpsc::SyncSender<()>,
) -> SpawnedBiStream {
    let spawned = spawn_module(bin, None);
    spawned.send_event(init_event);
    let (rx, child, event_tx) = spawned.into_parts();
    let spec = ProcessSource {
        identity: ProcessIdentity {
            bin: bin.to_string(),
            key: bin.to_string(),
        },
        script: None,
        args: vec![],
        env: std::collections::BTreeMap::new(),
        current_dir: None,
        props: None,
    };
    forward_stdout(rx, tx, wake_tx, spec);
    SpawnedBiStream { child, event_tx }
}

/// Spawn a string-streaming subprocess (e.g. a bash script that prints one line per tick).
///
/// Each line emitted by the process is forwarded to `tx` as a `StreamItem`.
/// The returned `Child` must be kept alive; drop it to kill the process.
pub fn spawn_string_stream(
    bin: &str,
    script: Option<&str>,
    tx: mpsc::Sender<StreamItem>,
    wake_tx: mpsc::SyncSender<()>,
) -> std::process::Child {
    let spawned = spawn_module(bin, script);
    let spec = ProcessSource {
        identity: ProcessIdentity {
            bin: bin.to_string(),
            key: bin.to_string(),
        },
        script: script.map(str::to_string),
        args: vec![],
        env: std::collections::BTreeMap::new(),
        current_dir: None,
        props: None,
    };
    let (rx, child, _event_tx) = spawned.into_parts();
    forward_stdout(rx, tx, wake_tx, spec);
    child
}
