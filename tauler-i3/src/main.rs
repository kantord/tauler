mod events;
mod ipc;
mod workspace;

use std::io::BufRead;
use std::sync::mpsc;
use std::thread;

use events::{ModuleEvent, parse_click_event, parse_init_event};
use ipc::{apply_bar_gap, i3_recv, i3_send, i3_socket_path, should_apply_bar_gap};
use workspace::{build_workspace_data, fetch_workspaces};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Read init event then release the stdin lock before spawning threads
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

    let socket = i3_socket_path();
    let (event_tx, event_rx) = mpsc::channel::<ModuleEvent>();

    // Thread: forward stdin lines as Stdin events
    {
        let event_tx = event_tx.clone();
        thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut lines = stdin.lock().lines();
            while let Some(Ok(line)) = lines.next() {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line)
                    && event_tx.send(ModuleEvent::Stdin(val)).is_err()
                {
                    break;
                }
            }
        });
    }

    // Emit initial workspace state
    if let Ok(ws) = fetch_workspaces(&socket, &init.output) {
        if should_apply_bar_gap(&init.output) && ws.iter().any(|w| w.focused) {
            apply_bar_gap(&socket, init.dpi, init.bar_width, init.outer_gap);
        }
        println!("{}", build_workspace_data(&ws));
    }

    // Thread: subscribe to i3 workspace events and forward as I3 events.
    // Reconnects automatically if the socket drops (e.g. i3 restart).
    {
        let event_tx = event_tx.clone();
        let socket_clone = socket.clone();
        thread::spawn(move || {
            loop {
                let mut sub = match std::os::unix::net::UnixStream::connect(&socket_clone) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to connect to i3 socket, retrying in 1s");
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        continue;
                    }
                };
                if i3_send(&mut sub, 2, b"[\"workspace\", \"window\"]").is_err() {
                    continue;
                }
                if i3_recv(&mut sub).is_err() {
                    continue;
                }
                tracing::info!("i3 subscription connected");
                // Trigger an immediate workspace refresh after (re)connect
                if event_tx
                    .send(ModuleEvent::I3(0x80000000, b"{}".to_vec()))
                    .is_err()
                {
                    return;
                }
                loop {
                    match i3_recv(&mut sub) {
                        Ok((typ, payload)) => {
                            if event_tx.send(ModuleEvent::I3(typ, payload)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "i3 subscription dropped, reconnecting");
                            break;
                        }
                    }
                }
            }
        });
    }

    // Main event loop
    while let Ok(event) = event_rx.recv() {
        match event {
            ModuleEvent::I3(0x80000000, payload) => {
                if let Ok(ev) = serde_json::from_slice::<serde_json::Value>(&payload)
                    && should_apply_bar_gap(&init.output)
                    && ev["current"]["output"].as_str() == Some(init.output.as_str())
                {
                    apply_bar_gap(&socket, init.dpi, init.bar_width, init.outer_gap);
                }
                if let Ok(ws) = fetch_workspaces(&socket, &init.output) {
                    println!("{}", build_workspace_data(&ws));
                }
            }
            ModuleEvent::I3(0x80000003, _) => {
                if let Ok(ws) = fetch_workspaces(&socket, &init.output) {
                    println!("{}", build_workspace_data(&ws));
                }
            }
            ModuleEvent::I3(_, _) => {}
            ModuleEvent::Stdin(val) => {
                tracing::debug!(event = %val, "stdin event");
                if let Some(name) = parse_click_event(&val) {
                    ipc::switch_workspace(&socket, &name);
                }
            }
        }
    }
}
