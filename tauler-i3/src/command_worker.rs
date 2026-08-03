//! Command-worker thread: owns one dedicated RUN_COMMAND connection, fed by
//! both the stdin thread and the subscribe thread (mpsc, multiple senders).

use std::sync::mpsc;

use crate::ipc::{BarConfig, I3Query, reconcile_gaps, switch_workspace};

/// Requests handled by the command-worker thread's dedicated RUN_COMMAND
/// connection. Fire-and-forget: senders don't wait for a reply.
pub enum CommandRequest {
    SwitchWorkspace(String),
    ReconcileGaps,
    /// A fresh props message from tauler core — panel geometry or declared
    /// gaps changed. Replaces the config and re-reconciles.
    UpdateConfig(BarConfig),
}

/// Run the command-worker loop: serve `CommandRequest`s off `rx` one at a
/// time over `query` until every sender has been dropped.
pub fn run(rx: mpsc::Receiver<CommandRequest>, mut query: I3Query, mut cfg: BarConfig) {
    while let Ok(req) = rx.recv() {
        match req {
            CommandRequest::SwitchWorkspace(name) => switch_workspace(&mut query, &name),
            CommandRequest::ReconcileGaps => reconcile_gaps(&mut query, &cfg),
            CommandRequest::UpdateConfig(next) => {
                cfg = next;
                reconcile_gaps(&mut query, &cfg);
            }
        }
    }
}
