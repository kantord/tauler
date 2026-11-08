//! Log output must go to stderr: stdout carries the JSON-lines protocol.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn log_output_goes_to_stderr_not_stdout() {
    // Empty PATH: `aerospace` cannot be found, so startup logs an error.
    let empty_path =
        std::env::temp_dir().join(format!("tauler-aerospace-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty_path).expect("create empty PATH dir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_tauler-aerospace"))
        .env("PATH", &empty_path)
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tauler-aerospace");

    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, r#"{{"type":"init"}}"#).expect("write init event");
    stdin.flush().expect("flush init event");

    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = mpsc::channel();
    let tx_err = tx.clone();
    thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        let _ = tx.send(("stdout", s));
    });
    thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        let _ = tx_err.send(("stderr", s));
    });

    let mut out = None;
    let mut err = None;
    for _ in 0..2 {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(("stdout", s)) => out = Some(s),
            Ok((_, s)) => err = Some(s),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir(&empty_path);

    let out = out.expect("stdout closed in time");
    let err = err.expect("stderr closed in time");
    for line in out.lines() {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_ok(),
            "non-JSON line on stdout: {line:?}"
        );
    }
    assert!(
        err.contains("cannot start `aerospace subscribe`"),
        "expected error on stderr, got: {err:?}"
    );
}
