//! Subscribe thread: persistent subscribe connection to workspace/window
//! events. Reconnects (with a 1s backoff, both on initial connect failure
//! and after a mid-stream drop) if the socket goes away.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use swayipc::{Connection, Event, EventStream, EventType, WorkspaceChange};

use crate::command_worker::CommandRequest;
use crate::ipc::{self, should_apply_bar_gap};

/// Connect and subscribe to workspace/window events, returning `None` (after
/// logging) on any failure along the way. The handshake uses a timeout so a
/// wedged server can't block forever; the timeout is cleared afterwards
/// (via a cloned handle, since `subscribe()` consumes the stream into an
/// `EventStream`) because events may legitimately be hours apart.
pub fn connect_and_subscribe(socket: &str) -> Option<EventStream> {
    let stream = match ipc::connect_with_timeout(socket, ipc::I3_IPC_TIMEOUT) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to connect to i3 socket");
            return None;
        }
    };
    let clear_timeout_handle = match stream.try_clone() {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(error = %e, "failed to clone i3 subscribe socket");
            return None;
        }
    };
    let events = match Connection::from(stream).subscribe([EventType::Workspace, EventType::Window])
    {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to subscribe to i3 events");
            return None;
        }
    };
    if let Err(e) = clear_timeout_handle.set_read_timeout(None) {
        tracing::warn!(error = %e, "failed to clear subscribe read timeout");
        return None;
    }
    tracing::info!("i3 subscription connected");
    Some(events)
}

/// Whether `event` is a workspace-focus change landing on `output` — the
/// case that requires re-applying the bar gap (X11/i3 mode only).
pub fn is_workspace_focus_change_on_output(event: &Event, output: &str) -> bool {
    match event {
        Event::Workspace(ws) => {
            ws.change == WorkspaceChange::Focus
                && ws.current.as_ref().and_then(|n| n.output.as_deref()) == Some(output)
        }
        _ => false,
    }
}

/// Run the subscribe-thread loop: connect, stream events, forward refresh
/// hints and bar-gap-reapply commands, and reconnect with backoff on any
/// disconnect, until either channel's receiver is gone.
pub fn run(
    socket: String,
    output: String,
    cmd_tx: mpsc::Sender<CommandRequest>,
    refresh_tx: mpsc::Sender<()>,
) {
    loop {
        let Some(events) = connect_and_subscribe(&socket) else {
            thread::sleep(Duration::from_secs(1));
            continue;
        };
        for event in events {
            match event {
                Ok(ev) => {
                    if refresh_tx.send(()).is_err() {
                        return;
                    }
                    if is_workspace_focus_change_on_output(&ev, &output)
                        && should_apply_bar_gap(&output)
                        && cmd_tx.send(CommandRequest::ApplyBarGap).is_err()
                    {
                        return;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "i3 subscription dropped, reconnecting");
                    break;
                }
            }
        }
        // Reconnect-after-mid-stream-drop path: same flat backoff as
        // the initial-connect-failure path above.
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use swayipc::{ShutdownEvent, WorkspaceEvent};

    /// Build a fully-valid `swayipc::Node`-shaped JSON workspace node, filling
    /// every field the real (`#[non_exhaustive]`) `Node` struct requires.
    /// Mirrors `workspace.rs`'s own `#[cfg(test)]` `node()` fixture and
    /// `tests/e2e_smoke.rs`'s `node_json()`.
    fn workspace_node_json(output: Option<&str>) -> Value {
        let rect = json!({"x": 0, "y": 0, "width": 0, "height": 0});
        json!({
            "id": 1,
            "name": "1: web",
            "type": "workspace",
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
            "focus": [],
            "floating": null,
            "nodes": [],
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
            "output": output,
        })
    }

    /// Build a `swayipc::Event::Workspace` fixture. `WorkspaceEvent` is
    /// `#[non_exhaustive]`, so (like `Node`) it can only be constructed via
    /// deserialization from this crate, not a struct literal.
    fn workspace_event(change: &str, current_output: Option<&str>) -> Event {
        let value = json!({
            "change": change,
            "current": workspace_node_json(current_output),
            "old": null,
        });
        let ws_event: WorkspaceEvent =
            serde_json::from_value(value).expect("valid WorkspaceEvent fixture");
        Event::Workspace(Box::new(ws_event))
    }

    #[test]
    fn focus_change_landing_on_target_output_is_true() {
        let ev = workspace_event("focus", Some("DP-1"));
        assert!(is_workspace_focus_change_on_output(&ev, "DP-1"));
    }

    #[test]
    fn focus_change_landing_on_different_output_is_false() {
        let ev = workspace_event("focus", Some("HDMI-A-1"));
        assert!(!is_workspace_focus_change_on_output(&ev, "DP-1"));
    }

    #[test]
    fn non_focus_workspace_change_is_false() {
        let ev = workspace_event("init", Some("DP-1"));
        assert!(!is_workspace_focus_change_on_output(&ev, "DP-1"));
    }

    #[test]
    fn non_workspace_event_is_false() {
        // `ShutdownEvent` is `#[non_exhaustive]` too, so (like `WorkspaceEvent`
        // above) it's built via deserialization rather than a struct literal.
        let shutdown_event: ShutdownEvent =
            serde_json::from_value(json!({"change": "exit"})).expect("valid ShutdownEvent fixture");
        let ev = Event::Shutdown(shutdown_event);
        assert!(!is_workspace_focus_change_on_output(&ev, "DP-1"));
    }
}
