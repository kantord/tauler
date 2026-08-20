//! AeroSpace workspace data source for tauler.
//!
//! Three threads. The subscribe thread turns AeroSpace's event stream into
//! refresh hints, the stdin thread turns tauler's click intents into
//! `aerospace workspace` calls, and the main loop rebuilds the strip on every
//! hint and writes one JSON line per refresh.
//!
//! Events say only what changed, never the whole state, so a hint is a signal
//! to re-query rather than something to apply.

mod aerospace;
mod events;
mod workspace;

use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::thread;

use events::{is_init_event, parse_switch_workspace};

/// Rebuild the strip and write it out. Skipped entirely when either query
/// fails, so a transient CLI error leaves the previous strip on screen.
fn refresh() {
    let (Some(workspaces), Some(windows)) =
        (aerospace::list_workspaces(), aerospace::list_windows())
    else {
        return;
    };
    let payload = workspace::payload(&workspace::build(&workspaces, &windows));
    let mut stdout = std::io::stdout().lock();
    if writeln!(stdout, "{payload}").is_err() || stdout.flush().is_err() {
        // tauler is gone; nothing left to serve.
        std::process::exit(0);
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let (hint_tx, hint_rx) = mpsc::channel::<()>();
    let (init_tx, init_rx) = mpsc::channel::<()>();

    // One thread owns stdin: the init event and every later click arrive on it,
    // and a `StdinLock` cannot be moved between threads.
    let stdin_hint_tx = hint_tx.clone();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut started = false;
        for line in stdin.lock().lines().map_while(Result::ok) {
            if !started {
                if is_init_event(&line) {
                    started = true;
                    let _ = init_tx.send(());
                }
                continue;
            }
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if let Some(name) = parse_switch_workspace(&val) {
                aerospace::switch_workspace(&name);
                // The switch raises its own event; this only shortens the gap.
                let _ = stdin_hint_tx.send(());
            }
        }
        std::process::exit(0);
    });

    if init_rx.recv().is_err() {
        return;
    }

    let (mut child, reader) = match aerospace::subscribe() {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!(error = %e, "cannot start `aerospace subscribe` — is AeroSpace running?");
            std::process::exit(1);
        }
    };

    let subscribe_tx = hint_tx.clone();
    thread::spawn(move || {
        aerospace::pump_events(reader, || {
            let _ = subscribe_tx.send(());
        });
        let _ = child.wait();
        tracing::error!("aerospace event stream closed");
        std::process::exit(1);
    });

    drop(hint_tx);
    while hint_rx.recv().is_ok() {
        refresh();
    }
}
