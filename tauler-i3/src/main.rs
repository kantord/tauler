mod command_worker;
mod events;
mod ipc;
mod refresh_worker;
mod scheduler;
mod subscribe;
mod tree_cache;
mod workspace;

use std::io::BufRead;
use std::sync::mpsc;
use std::thread;

use command_worker::CommandRequest;
use events::{parse_click_event, parse_init_event};
use ipc::{I3Query, i3_socket_path, should_apply_bar_gap};
use tree_cache::TreeCache;

/// Wires together the four worker threads (command-worker, refresh-worker,
/// subscribe, stdin) and blocks until the stdin thread exits. Each thread's
/// actual behavior lives in its own module — see `command_worker`,
/// `refresh_worker`, and `subscribe`.
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Read init event then release the stdin lock before spawning threads.
    let init = {
        let stdin = std::io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            match lines.next() {
                Some(Ok(line)) => {
                    if let Some(ev) = parse_init_event(&line) {
                        break ev;
                    }
                }
                _ => return,
            }
        }
    };

    let socket = match i3_socket_path() {
        Ok(path) => path,
        Err(e) => {
            tracing::error!(error = %e, "cannot start tauler-i3");
            std::process::exit(1);
        }
    };

    // command-worker's channel: fed directly by both the stdin thread and
    // the subscribe thread (no central hub).
    let (cmd_tx, cmd_rx) = mpsc::channel::<CommandRequest>();
    // refresh-worker's hint channel: fed by the subscribe thread.
    let (refresh_tx, refresh_rx) = mpsc::channel::<()>();

    // Command-worker: owns one dedicated RUN_COMMAND connection.
    {
        let query = I3Query::new(socket.clone(), ipc::I3_IPC_TIMEOUT);
        let dpi = init.dpi;
        let bar_width = init.bar_width;
        let outer_gap = init.outer_gap;
        thread::spawn(move || command_worker::run(cmd_rx, query, dpi, bar_width, outer_gap));
    }

    // Apply the bar gap once at startup (mirrors the effect a workspace-focus
    // event would otherwise trigger) — needed because i3 may have just
    // (re)started and forgotten any previously-applied runtime gap, and
    // startup itself doesn't produce a workspace-focus event to react to.
    if should_apply_bar_gap(&init.output) {
        let _ = cmd_tx.send(CommandRequest::ApplyBarGap);
    }

    // Refresh-worker: owns one dedicated GET_TREE connection and runs the
    // debounce-with-max-wait scheduler.
    {
        let query = I3Query::new(socket.clone(), ipc::I3_IPC_TIMEOUT);
        let output = init.output.clone();
        let cache = TreeCache::new(Vec::new());
        thread::spawn(move || refresh_worker::run(query, output, refresh_rx, cache));
    }

    // Subscribe thread: persistent subscribe connection to workspace/window
    // events.
    {
        let socket = socket.clone();
        let output = init.output.clone();
        let cmd_tx = cmd_tx.clone();
        let refresh_tx = refresh_tx.clone();
        thread::spawn(move || subscribe::run(socket, output, cmd_tx, refresh_tx));
    }

    // Stdin thread: forward click events directly to the command-worker.
    // When stdin closes the parent is gone, so exit outright — other
    // threads may be blocked in reads and would otherwise keep an orphaned
    // process alive forever.
    let stdin_handle = thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut lines = stdin.lock().lines();
        while let Some(Ok(line)) = lines.next() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                tracing::debug!(event = %val, "stdin event");
                if let Some(name) = parse_click_event(&val)
                    && cmd_tx.send(CommandRequest::SwitchWorkspace(name)).is_err()
                {
                    break;
                }
            }
        }
        std::process::exit(0);
    });

    let _ = stdin_handle.join();
}
