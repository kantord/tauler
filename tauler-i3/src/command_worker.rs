//! Command-worker thread: owns one dedicated RUN_COMMAND connection, fed by
//! both the stdin thread and the subscribe thread (mpsc, multiple senders).

use std::sync::mpsc;

use crate::ipc::{BarGeometry, I3Query, reconcile_gaps, switch_workspace};

/// Requests handled by the command-worker thread's dedicated RUN_COMMAND
/// connection. Fire-and-forget: senders don't wait for a reply.
pub enum CommandRequest {
    SwitchWorkspace(String),
    ReconcileGaps,
}

/// Run the command-worker loop: serve `CommandRequest`s off `rx` one at a
/// time over `query` until every sender has been dropped.
///
/// `ReconcileGaps` reads GET_WORKSPACES on this same connection before
/// writing, so the focus lookup and the gap write can't be interleaved with
/// another request.
pub fn run(
    rx: mpsc::Receiver<CommandRequest>,
    mut query: I3Query,
    dpi: f32,
    bar_output: String,
    geom: BarGeometry,
) {
    while let Ok(req) = rx.recv() {
        match req {
            CommandRequest::SwitchWorkspace(name) => switch_workspace(&mut query, &name),
            CommandRequest::ReconcileGaps => reconcile_gaps(&mut query, dpi, &bar_output, geom),
        }
    }
}
