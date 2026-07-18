//! One lightweight end-to-end smoke test for the real `tauler-i3` binary.
//!
//! Everything else (scheduler, TreeCache, ipc.rs connection behavior,
//! workspace.rs tree-walking) already has unit tests inside `src/`. This is
//! specifically about proving `main()`'s wiring (stdin thread, subscribe
//! thread, command-worker, refresh-worker) actually works together as a
//! whole process, which nothing else exercises.
//!
//! `tauler-i3` is a `[[bin]]`-only crate (no `[lib]` target), so this is a
//! `tests/*.rs` integration test spawning the just-built binary via
//! `env!("CARGO_BIN_EXE_tauler-i3")`, talking to it over real stdin/stdout
//! pipes and a fake i3/sway IPC socket.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// i3-ipc message types this fake server understands, per i3's wire
/// protocol (matches `swayipc_types::CommandType`'s discriminants).
const RUN_COMMAND: u32 = 0;
const SUBSCRIBE: u32 = 2;
const GET_TREE: u32 = 4;

/// Unique socket path under the system temp dir.
fn temp_sock(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "tauler-i3-e2e-{name}-{}-{nanos}.sock",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

/// Read one framed i3-ipc request off the wire: magic bytes, native-endian
/// u32 length, native-endian u32 type, then the JSON payload. Mirrors
/// `ipc.rs`'s own (private) `#[cfg(test)]` helper of the same name — this is
/// a separate `tests/` binary that can't reach it, so it's duplicated here.
fn read_i3_frame(s: &mut UnixStream) -> std::io::Result<(u32, Vec<u8>)> {
    let mut hdr = [0u8; 14];
    s.read_exact(&mut hdr)?;
    let len = u32::from_ne_bytes(hdr[6..10].try_into().unwrap()) as usize;
    let typ = u32::from_ne_bytes(hdr[10..14].try_into().unwrap());
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf)?;
    Ok((typ, buf))
}

/// Write one framed i3-ipc reply, matching the same wire format.
fn write_i3_frame(s: &mut UnixStream, typ: u32, payload: &[u8]) -> std::io::Result<()> {
    s.write_all(&swayipc::MAGIC)?;
    s.write_all(&(payload.len() as u32).to_ne_bytes())?;
    s.write_all(&typ.to_ne_bytes())?;
    s.write_all(payload)
}

/// Build a fully-valid `swayipc::Node`-shaped JSON object, filling every
/// field the real `Node` struct requires with a reasonable placeholder.
/// Mirrors `workspace.rs`'s own `#[cfg(test)]` `node()` fixture.
fn node_json(id: i64, name: &str, node_type: &str, focus: &[i64], nodes: Vec<Value>) -> Value {
    let rect = json!({"x": 0, "y": 0, "width": 0, "height": 0});
    json!({
        "id": id,
        "name": name,
        "type": node_type,
        "border": "normal",
        "current_border_width": 0,
        "layout": "none",
        "orientation": "none",
        "percent": null,
        "rect": rect,
        "window_rect": rect,
        "deco_rect": rect,
        "geometry": rect,
        "urgent": false,
        "focused": false,
        "focus": focus,
        "floating": null,
        "nodes": nodes,
        "floating_nodes": [],
        "sticky": false,
        "representation": null,
        "fullscreen_mode": null,
        "scratchpad_state": null,
        "app_id": null,
        "pid": null,
        "window": null,
        "num": null,
        "window_properties": null,
        "marks": [],
        "inhibit_idle": null,
        "idle_inhibitors": null,
        "sandbox_engine": null,
        "sandbox_app_id": null,
        "sandbox_instance_id": null,
        "tag": null,
        "shell": null,
        "foreign_toplevel_identifier": null,
        "visible": null,
        "output": null,
    })
}

/// Minimal but realistic GET_TREE reply: root -> output "DP-1" -> content
/// con -> workspace "1: web" (no windows, kept minimal since this test only
/// cares that the workspace name round-trips end to end).
fn sample_tree_json() -> Value {
    let ws_web = node_json(4, "1: web", "workspace", &[], vec![]);
    let content = node_json(3, "content", "con", &[4], vec![ws_web]);
    let output = node_json(2, "DP-1", "output", &[3], vec![content]);
    node_json(1, "root", "root", &[2], vec![output])
}

/// Serve one accepted connection: dispatches each request by its message
/// type, since the real process may reuse one connection for many requests
/// (command-worker, refresh-worker) or hold it open as an event stream
/// (subscribe). Every RUN_COMMAND payload is forwarded to `run_command_tx`
/// so the test can observe what commands the real process actually sent.
fn serve_connection(mut s: UnixStream, tree: &Value, run_command_tx: &mpsc::Sender<String>) {
    while let Ok((typ, payload)) = read_i3_frame(&mut s) {
        match typ {
            RUN_COMMAND => {
                let cmd = String::from_utf8_lossy(&payload).into_owned();
                let _ = run_command_tx.send(cmd);
                if write_i3_frame(&mut s, typ, b"[{\"success\":true}]").is_err() {
                    break;
                }
            }
            SUBSCRIBE => {
                if write_i3_frame(&mut s, typ, b"{\"success\":true}").is_err() {
                    break;
                }
                // The real client now treats this connection as a long-lived
                // event stream. This fake server never pushes events, so
                // just block here (reading, so a client-side close is
                // noticed) until the connection is torn down at test end.
                let mut buf = [0u8; 1];
                let _ = s.read(&mut buf);
                break;
            }
            GET_TREE => {
                let body = serde_json::to_vec(tree).expect("tree fixture serializes");
                if write_i3_frame(&mut s, typ, &body).is_err() {
                    break;
                }
            }
            _ => break,
        }
    }
}

/// Start the fake i3/sway server on `path` in a background thread. Accepts
/// connections in a loop (the real process opens 2-3 separate connections:
/// subscribe, command-worker, refresh-worker) and hands each off to its own
/// handler thread. Returns a channel that yields every RUN_COMMAND payload
/// the real process sends, across any/all connections.
fn start_fake_i3_server(path: &str) -> mpsc::Receiver<String> {
    let listener = UnixListener::bind(path).expect("bind fake i3 socket");
    let (tx, rx) = mpsc::channel::<String>();
    let tree = sample_tree_json();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(s) = stream else { break };
            let tx = tx.clone();
            let tree = tree.clone();
            thread::spawn(move || serve_connection(s, &tree, &tx));
        }
    });
    rx
}

/// End-to-end smoke test covering the two paths nothing else exercises:
/// refresh-worker fetching the tree and writing workspace-JSON to stdout,
/// and a stdin click event reaching the command-worker as a RUN_COMMAND.
#[test]
fn stdin_click_and_refresh_worker_round_trip_through_real_process() {
    let sock_path = temp_sock("main");
    let run_command_rx = start_fake_i3_server(&sock_path);

    let mut child = Command::new(env!("CARGO_BIN_EXE_tauler-i3"))
        .env("I3SOCK", &sock_path)
        .env_remove("SWAYSOCK")
        // tracing_subscriber::fmt() defaults to writing to stdout, which
        // would otherwise interleave log lines with the workspace-JSON
        // protocol output this test asserts on.
        .env("RUST_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn tauler-i3");

    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");

    // Init event first, per the real I/O contract (events::parse_init_event).
    // A non-empty output enables the X11/i3 bar-gap path (should_apply_bar_gap),
    // so an extra RUN_COMMAND (the startup gap-apply) precedes the click's
    // switch-workspace command below — the assertion loop tolerates that.
    writeln!(
        stdin,
        r#"{{"type":"init","output":"DP-1","config":{{"width":24,"outer_gap":4}},"dpi":96.0}}"#
    )
    .expect("write init event");
    stdin.flush().expect("flush init event");

    // Read stdout lines on their own thread with a channel, so a hang can't
    // block the test forever.
    let (line_tx, line_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if line_tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Path 1: refresh-worker end-to-end — it should fetch the fake tree and
    // write a workspace-JSON line naming the workspace from that tree.
    let first_line = line_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("no workspace-json line from refresh-worker within 5s");
    assert!(
        first_line.contains("1: web"),
        "expected workspace name in refresh-worker stdout line, got: {first_line}"
    );

    // Path 2: stdin click -> command-worker end-to-end — write a click event
    // and confirm the fake server actually receives a matching RUN_COMMAND.
    writeln!(
        stdin,
        r#"{{"event":"click","data":{{"workspace":"1: web"}}}}"#
    )
    .expect("write click event");
    stdin.flush().expect("flush click event");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_switch_command = false;
    while Instant::now() < deadline {
        match run_command_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(cmd) if cmd.contains("workspace \"1: web\"") => {
                saw_switch_command = true;
                break;
            }
            // Startup's bar-gap command (or a stray retry) — keep waiting.
            Ok(_) => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(
        saw_switch_command,
        "expected a RUN_COMMAND containing workspace \"1: web\" after the click event"
    );

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&sock_path);
}
