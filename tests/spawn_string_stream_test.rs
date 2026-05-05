use std::sync::mpsc;
use std::time::Duration;

use tauler::spawn_bi_stream;
use tauler::spawn_string_stream;

#[test]
fn spawn_string_stream_delivers_line_as_triple() {
    let (tx, rx) = mpsc::channel();
    let (wake_tx, wake_rx) = mpsc::sync_channel(1);

    let mut child = spawn_string_stream("sh", Some("echo hello"), tx, wake_tx);

    let item = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(item.key.0, "sh");
    assert_eq!(item.key.1, Some("echo hello".to_string()));
    assert_eq!(item.line, "hello");

    // wake_tx must have been signalled after the line was sent
    wake_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    child.wait().ok();
}

#[test]
fn spawn_string_stream_signals_wake_tx_after_each_line() {
    let (tx, rx) = mpsc::channel();
    let (wake_tx, wake_rx) = mpsc::sync_channel(4);

    let mut child = spawn_string_stream("sh", Some("echo first\necho second"), tx, wake_tx);

    // First line + wake signal
    let item1 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(item1.line, "first");
    wake_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    // Second line + wake signal
    let item2 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(item2.line, "second");
    wake_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    child.wait().ok();
}

#[test]
fn spawn_bi_stream_script_field_is_none() {
    let (tx, rx) = mpsc::channel();
    let (wake_tx, _wake_rx) = mpsc::sync_channel(1);
    // `echo` with no arguments prints a blank line immediately and exits
    let _bi = spawn_bi_stream("echo", &serde_json::json!(null), tx, wake_tx);
    let item = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(item.key.0, "echo");
    assert_eq!(
        item.key.1, None,
        "spawn_bi_stream must forward script=None, not Some(...)"
    );
    assert_eq!(item.line, "");
}
