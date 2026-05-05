use std::sync::mpsc;
use std::time::Duration;
use tauler::presentation::PanelCommand;

pub(crate) mod wayland;
pub(crate) mod x11;

/// Drains the command channel for one presenter-thread iteration.
/// Blocks up to 8 ms waiting for the first command, then drains non-blocking.
/// Returns `true` when the thread should stop (Shutdown received or sender dropped).
pub(crate) fn drain_commands(
    command_rx: &mpsc::Receiver<PanelCommand>,
    mut apply: impl FnMut(PanelCommand),
) -> bool {
    use std::sync::mpsc::RecvTimeoutError;
    match command_rx.recv_timeout(Duration::from_millis(8)) {
        Ok(PanelCommand::Shutdown) => return true,
        Ok(cmd) => apply(cmd),
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => return true,
    }
    loop {
        match command_rx.try_recv() {
            Ok(PanelCommand::Shutdown) => return true,
            Ok(cmd) => apply(cmd),
            Err(_) => break,
        }
    }
    false
}
