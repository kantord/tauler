//! Subscribe thread: persistent subscribe connection to workspace/window
//! events. Reconnects (with a 1s backoff, both on initial connect failure
//! and after a mid-stream drop) if the socket goes away.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use swayipc::{Connection, Event, EventStream, EventType};

use crate::command_worker::CommandRequest;
use crate::ipc;

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

/// Whether `event` should trigger a gap reconcile.
///
/// Every workspace event qualifies, deliberately. The payload is used purely
/// as a signal that *something* about workspaces changed; which workspace is
/// focused gets resolved separately via GET_WORKSPACES. Matching on specific
/// variants would be unsound: an event's `current` field names the workspace
/// the event is *about*, and for an urgency hint or a workspace being emptied
/// that is routinely a background workspace on a different output than the
/// focused one that `gaps ... current set` would actually write to.
pub fn is_workspace_event(event: &Event) -> bool {
    matches!(event, Event::Workspace(_))
}

/// Run the subscribe-thread loop: connect, stream events, forward refresh
/// hints and gap-reconcile commands, and reconnect with backoff on any
/// disconnect, until either channel's receiver is gone.
///
/// `gaps_enabled` is false in Wayland mode, where the layer-shell exclusive
/// zone reserves panel space and tauler must not write gaps at all.
pub fn run(
    socket: String,
    gaps_enabled: bool,
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
                    if gaps_enabled
                        && is_workspace_event(&ev)
                        && cmd_tx.send(CommandRequest::ReconcileGaps).is_err()
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
    fn focus_change_is_a_workspace_event() {
        assert!(is_workspace_event(&workspace_event("focus", Some("DP-1"))));
    }

    /// A workspace moving between outputs is what strands a gap on a monitor
    /// that no longer has a panel, so it must trigger a reconcile.
    #[test]
    fn workspace_move_is_a_workspace_event() {
        assert!(is_workspace_event(&workspace_event("move", Some("DP-1"))));
    }

    /// Deliberate: variants whose `current` is *not* the focused workspace
    /// still trigger a reconcile, because the focused workspace is resolved
    /// separately rather than read off the event.
    #[test]
    fn urgency_change_on_a_background_workspace_is_a_workspace_event() {
        assert!(is_workspace_event(&workspace_event(
            "urgent",
            Some("HDMI-A-1")
        )));
    }

    #[test]
    fn workspace_event_on_any_output_qualifies() {
        assert!(is_workspace_event(&workspace_event("init", Some("DP-9"))));
    }

    #[test]
    fn non_workspace_event_is_false() {
        // `ShutdownEvent` is `#[non_exhaustive]` too, so (like `WorkspaceEvent`
        // above) it's built via deserialization rather than a struct literal.
        let shutdown_event: ShutdownEvent =
            serde_json::from_value(json!({"change": "exit"})).expect("valid ShutdownEvent fixture");
        let ev = Event::Shutdown(shutdown_event);
        assert!(!is_workspace_event(&ev));
    }
}
