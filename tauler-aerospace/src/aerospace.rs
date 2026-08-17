//! Thin wrapper over the `aerospace` CLI.
//!
//! The CLI is used rather than the socket protocol at
//! `/tmp/bobko.aerospace-$USER.sock`: `subscribe` is exposed as a subcommand,
//! so a long-lived child process covers the event stream, and the two list
//! queries already emit JSON. Nothing here needs the wire format.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Command, Stdio};

use serde_json::Value;

const BIN: &str = "aerospace";

/// Fields the workspace strip needs. AeroSpace's plain `--json` reports only
/// the workspace name, so every other column has to be asked for.
const WORKSPACE_FORMAT: &str =
    "%{workspace}%{workspace-is-focused}%{workspace-is-visible}%{monitor-id}";
const WINDOW_FORMAT: &str =
    "%{window-id}%{workspace}%{app-name}%{app-bundle-id}%{window-title}%{window-is-fullscreen}";

fn query(args: &[&str]) -> Option<Value> {
    let out = Command::new(BIN).args(args).output().ok()?;
    if !out.status.success() {
        tracing::warn!(
            args = ?args,
            stderr = %String::from_utf8_lossy(&out.stderr).trim(),
            "aerospace query failed"
        );
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

pub fn list_workspaces() -> Option<Value> {
    query(&[
        "list-workspaces",
        "--all",
        "--format",
        WORKSPACE_FORMAT,
        "--json",
    ])
}

pub fn list_windows() -> Option<Value> {
    query(&["list-windows", "--all", "--format", WINDOW_FORMAT, "--json"])
}

pub fn switch_workspace(name: &str) {
    match Command::new(BIN).args(["workspace", name]).output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => tracing::warn!(
            workspace = %name,
            stderr = %String::from_utf8_lossy(&out.stderr).trim(),
            "workspace switch rejected"
        ),
        Err(e) => tracing::error!(workspace = %name, error = %e, "could not run aerospace"),
    }
}

/// Start the event stream. Its lines are only used as hints that something
/// changed — the payload is rebuilt from the list queries either way — so the
/// caller never parses them.
///
/// AeroSpace replays current state on connect unless `--no-send-initial` is
/// passed, which is what drives the first refresh.
pub fn subscribe() -> std::io::Result<(Child, BufReader<ChildStdout>)> {
    let mut child = Command::new(BIN)
        .args(["subscribe", "--all"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout was piped");
    Ok((child, BufReader::new(stdout)))
}

/// Read `reader` to exhaustion, calling `on_event` per line.
pub fn pump_events(reader: BufReader<ChildStdout>, mut on_event: impl FnMut()) {
    for line in reader.lines() {
        match line {
            Ok(_) => on_event(),
            Err(e) => {
                tracing::error!(error = %e, "event stream ended");
                return;
            }
        }
    }
}
