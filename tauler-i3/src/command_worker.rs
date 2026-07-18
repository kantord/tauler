//! Command-worker thread: owns one dedicated RUN_COMMAND connection, fed by
//! both the stdin thread and the subscribe thread (mpsc, multiple senders).

use std::sync::mpsc;

use crate::ipc::{I3Query, apply_bar_gap, switch_workspace};

/// Requests handled by the command-worker thread's dedicated RUN_COMMAND
/// connection. Fire-and-forget: senders don't wait for a reply.
pub enum CommandRequest {
    SwitchWorkspace(String),
    ApplyBarGap,
}

/// Run the command-worker loop: serve `CommandRequest`s off `rx` one at a
/// time over `query` until every sender has been dropped.
pub fn run(
    rx: mpsc::Receiver<CommandRequest>,
    mut query: I3Query,
    dpi: f32,
    bar_width: u32,
    outer_gap: u32,
) {
    while let Ok(req) = rx.recv() {
        match req {
            CommandRequest::SwitchWorkspace(name) => switch_workspace(&mut query, &name),
            CommandRequest::ApplyBarGap => apply_bar_gap(&mut query, dpi, bar_width, outer_gap),
        }
    }
}
