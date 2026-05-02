use std::collections::BTreeMap;
use std::io::{BufRead, Seek, SeekFrom, Write as IoWrite};
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;

use crate::managed_set::Lifecycle;
use costae_lifecycle_derive::lifecycle_trace;

use super::{StreamItem, StreamKind};

/// Stable identity for a process: uniquely identifies which process to manage.
/// Used as the key in `Lifecycle` so that `ManagedSet` can track processes by identity.
#[derive(Hash, Eq, PartialEq, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProcessIdentity {
    pub bin: String,
    pub key: String,
}

// NOTE: env uses BTreeMap (not HashMap) for deterministic ordering; HashMap doesn't implement Hash.
#[derive(Clone, Debug)]
pub struct ProcessSource {
    pub identity: ProcessIdentity,
    pub script: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub current_dir: Option<PathBuf>,
    pub props: Option<serde_json::Value>,
}

pub struct ProcessState {
    pub child: std::process::Child,
    pub event_tx: mpsc::Sender<serde_json::Value>,
    pub last_sent_props: Option<serde_json::Value>,
}

/// Error type for process spawning failures.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("memfd_create failed for {bin}")]
    MemfdCreateFailed { bin: String },
    #[error("failed to spawn {bin}: {source}")]
    ProcessSpawnFailed {
        bin: String,
        #[source]
        source: std::io::Error,
    },
}

fn spawn_stdout_thread(
    stdout: std::process::ChildStdout,
    spec: ProcessSource,
    tx: mpsc::Sender<StreamItem>,
) {
    thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    let item = StreamItem {
                        key: (spec.identity.bin.clone(), spec.script.clone()),
                        stream: StreamKind::Stdout,
                        line: l,
                    };
                    if tx.send(item).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn spawn_stderr_thread(stderr: std::process::ChildStderr, bin_name: String) {
    thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(l) => tracing::warn!(module = %bin_name, "{l}"),
                Err(_) => break,
            }
        }
    });
}

fn spawn_stdin_thread(
    mut stdin: std::process::ChildStdin,
    event_rx: mpsc::Receiver<serde_json::Value>,
) {
    thread::spawn(move || {
        while let Ok(event) = event_rx.recv() {
            let line = serde_json::to_string(&event).unwrap_or_default() + "\n";
            if stdin.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
    });
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}{}", home, &path[1..])
    } else if path == "~" {
        std::env::var("HOME").unwrap_or_default()
    } else {
        path.to_string()
    }
}

pub(super) fn spawn_process(
    spec: ProcessSource,
    tx: &mpsc::Sender<StreamItem>,
) -> Result<ProcessState, SpawnError> {
    let bin = expand_tilde(&spec.identity.bin);
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(&spec.args);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(ref dir) = spec.current_dir {
        cmd.current_dir(dir);
    }

    // If a script is provided, write it to a memfd and pass the path as an argument.
    #[allow(clippy::option_if_let_else)]
    let _memfd_file = if let Some(ref content) = spec.script {
        let fd = unsafe { libc::memfd_create(c"costae-script".as_ptr(), 0) };
        if fd < 0 {
            return Err(SpawnError::MemfdCreateFailed {
                bin: spec.identity.bin.clone(),
            });
        }
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let _ = file.write_all(content.as_bytes());
        let _ = file.seek(SeekFrom::Start(0));
        cmd.arg(format!("/proc/self/fd/{}", fd));
        Some(file) // keep alive until after spawn so fd is inherited
    } else {
        None
    };

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(SpawnError::ProcessSpawnFailed { bin, source: e });
        }
    };

    if let Some(stdout) = child.stdout.take() {
        spawn_stdout_thread(stdout, spec.clone(), tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_stderr_thread(stderr, spec.identity.bin.clone());
    }
    let (event_tx, event_rx) = mpsc::channel::<serde_json::Value>();
    if let Some(stdin) = child.stdin.take() {
        spawn_stdin_thread(stdin, event_rx);
    }

    Ok(ProcessState {
        child,
        event_tx,
        last_sent_props: None,
    })
}

impl std::fmt::Display for ProcessSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.identity.bin)
    }
}

#[lifecycle_trace]
impl Lifecycle for ProcessSource {
    type Key = ProcessIdentity;
    type State = ProcessState;
    type Context = ();
    type Output = mpsc::Sender<StreamItem>;
    type Error = SpawnError;

    fn key(&self) -> ProcessIdentity {
        self.identity.clone()
    }

    fn enter(self, _ctx: &mut (), output: &mut Self::Output) -> Result<Self::State, Self::Error> {
        let props = self.props.clone();
        let mut state = spawn_process(self, output)?;
        if let Some(p) = props {
            let _ = state.event_tx.send(p.clone());
            state.last_sent_props = Some(p);
        }
        Ok(state)
    }

    fn reconcile_self(
        self,
        state: &mut Self::State,
        _ctx: &mut (),
        output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        if matches!(state.child.try_wait(), Ok(Some(_))) {
            tracing::warn!(bin = %self.identity.bin, "process exited");
            let props = self.props.clone();
            let mut new_state = spawn_process(self, output)?;
            if let Some(p) = props {
                let _ = new_state.event_tx.send(p.clone());
                new_state.last_sent_props = Some(p);
            }
            *state = new_state;
        } else if let Some(p) = self.props {
            if state.last_sent_props.as_ref() != Some(&p) {
                let _ = state.event_tx.send(p.clone());
                state.last_sent_props = Some(p);
            }
        }
        Ok(())
    }

    fn exit(
        mut state: Self::State,
        _ctx: &mut (),
        _output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        let _ = state.child.kill();
        let _ = state.child.wait();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcessIdentity, ProcessSource};
    use crate::managed_set::Lifecycle;
    use std::collections::BTreeMap;

    fn make_source(bin: &str) -> ProcessSource {
        ProcessSource {
            identity: ProcessIdentity {
                bin: bin.to_string(),
                key: bin.to_string(),
            },
            script: None,
            args: vec![],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        }
    }

    #[test]
    fn process_identity_has_bin_and_key_fields() {
        let id = ProcessIdentity {
            bin: "mybin".to_string(),
            key: "mykey".to_string(),
        };
        assert_eq!(id.bin, "mybin");
        assert_eq!(id.key, "mykey");
    }

    #[test]
    fn process_identity_derives_hash_eq_partialeq_clone() {
        use std::collections::HashSet;
        let a = ProcessIdentity {
            bin: "bin".to_string(),
            key: "k".to_string(),
        };
        let b = a.clone();
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(!set.insert(b));
    }

    #[test]
    fn process_source_has_script_and_identity_fields() {
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "my-key".to_string(),
            },
            script: Some("echo hello".to_string()),
            args: vec!["--flag".to_string()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };
        assert_eq!(spec.identity.bin, "/bin/sh");
        assert_eq!(spec.identity.key, "my-key");
        assert!(spec.script.is_some());
    }

    #[test]
    fn lifecycle_key_returns_identity() {
        let id = ProcessIdentity {
            bin: "/usr/bin/cat".to_string(),
            key: "cat-key".to_string(),
        };
        let returned: ProcessIdentity = make_source("/usr/bin/cat").key();
        assert_eq!(returned.bin, id.bin);
    }

    mod spawn_process {
        use super::super::{spawn_process, SpawnError};
        use std::sync::mpsc;

        #[test]
        fn memfd_create_failed_error_displays_bin_name() {
            let err = SpawnError::MemfdCreateFailed {
                bin: "mybin".to_string(),
            };
            let msg = err.to_string();
            assert!(msg.contains("memfd_create failed"), "got: {msg}");
            assert!(msg.contains("mybin"), "got: {msg}");
        }

        #[test]
        fn nonexistent_binary_returns_process_spawn_failed() {
            let (tx, _rx) = mpsc::channel();
            let result = spawn_process(
                super::make_source("/nonexistent/binary/that/cannot/exist"),
                &tx,
            );
            match result {
                Err(SpawnError::ProcessSpawnFailed { bin, .. }) => {
                    assert_eq!(bin, "/nonexistent/binary/that/cannot/exist");
                }
                Err(other) => panic!("expected ProcessSpawnFailed, got: {:?}", other),
                Ok(_) => panic!("expected Err, got Ok"),
            }
        }

        #[test]
        fn tilde_bin_is_expanded_to_home_dir() {
            let home = std::env::var("HOME").expect("HOME must be set");
            let (tx, _rx) = mpsc::channel();
            let result = spawn_process(super::make_source("~/nonexistent-tilde-test-binary"), &tx);
            match result {
                Err(SpawnError::ProcessSpawnFailed { bin, .. }) => {
                    assert!(
                        !bin.starts_with('~'),
                        "bin must not contain literal ~; got: {bin}"
                    );
                    assert!(
                        bin.starts_with(&home),
                        "bin must start with HOME ({home}); got: {bin}"
                    );
                }
                Err(other) => panic!("expected ProcessSpawnFailed, got: {:?}", other),
                Ok(_) => panic!("expected Err, got Ok"),
            }
        }
    }

    mod lifecycle {
        use super::super::{ProcessIdentity, ProcessSource, SpawnError};
        use crate::managed_set::Lifecycle;
        use std::collections::BTreeMap;
        use std::sync::mpsc;

        #[test]
        fn reconcile_self_propagates_err_when_restart_spawn_fails() {
            let (mut tx, _rx) = mpsc::channel();

            let mut state = ProcessSource {
                identity: ProcessIdentity {
                    bin: "/bin/sh".to_string(),
                    key: "t".to_string(),
                },
                script: None,
                args: vec!["-c".to_string(), "exit 0".to_string()],
                env: BTreeMap::new(),
                current_dir: None,
                props: None,
            }
            .enter(&mut (), &mut tx)
            .expect("enter must succeed with /bin/sh");

            std::thread::sleep(std::time::Duration::from_millis(200));
            assert!(
                matches!(state.child.try_wait(), Ok(Some(_))),
                "child should have exited"
            );

            let result = super::make_source("/nonexistent/binary/that/cannot/exist")
                .reconcile_self(&mut state, &mut (), &mut tx);
            match result {
                Err(SpawnError::ProcessSpawnFailed { .. }) => {}
                Err(other) => panic!("expected ProcessSpawnFailed, got: {:?}", other),
                Ok(_) => panic!("expected Err, got Ok"),
            }
        }
    }
}
