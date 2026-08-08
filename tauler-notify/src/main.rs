mod events;
mod model;
mod server;
mod store;

use std::time::Duration;
use tokio::sync::mpsc;

use events::parse_dismiss;
use model::{CloseReason, Event, Notification};
use server::NotifyServer;
use std::sync::atomic::AtomicU32;
use store::Notifications;

fn emit(notifications: &[Notification]) {
    if let Ok(json) = serde_json::to_string(&serde_json::json!({ "notifications": notifications }))
    {
        println!("{json}");
    }
}

/// Tells clients their notification is gone. Only call this once the store has
/// confirmed it removed something: a stale timer or a CloseNotification for an
/// already-closed id removes nothing, and announcing those would close a
/// client's newer notification that happens to reuse the id.
///
/// A failed emission costs a client one signal; it must not take the daemon
/// down with it.
async fn notify_closed(conn: &zbus::Connection, id: u32, reason: CloseReason) {
    let result = conn
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationClosed",
            &(id, u32::from(reason)),
        )
        .await;
    if let Err(e) = result {
        eprintln!("warning: could not emit NotificationClosed for {id}: {e}");
    }
}

fn expire_ms(timeout: i32) -> Option<u64> {
    match timeout {
        0 => None,         // never
        -1 => Some(5_000), // server default: 5 s
        ms => Some(ms as u64),
    }
}

#[tokio::main]
async fn main() {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Event>();

    // Stdin reader: tauler sends us intents as one JSON object per line.
    // Anything we don't recognise (including tauler's init event) is ignored.
    // Blocking reads must stay off the runtime's async workers, hence
    // spawn_blocking. When stdin closes the parent is gone, so exit outright
    // rather than lingering as an orphan holding the D-Bus name.
    {
        let event_tx = event_tx.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            let mut lines = stdin.lock().lines();
            while let Some(Ok(line)) = lines.next() {
                let id = serde_json::from_str::<serde_json::Value>(&line)
                    .ok()
                    .and_then(|val| parse_dismiss(&val));
                if let Some(id) = id {
                    let event = Event::Close {
                        id,
                        reason: CloseReason::Dismissed,
                    };
                    if event_tx.send(event).is_err() {
                        break;
                    }
                }
            }
            std::process::exit(0);
        });
    }

    let server = NotifyServer {
        tx: event_tx.clone(),
        next_id: AtomicU32::new(1),
        tags: std::sync::Mutex::new(std::collections::HashMap::new()),
    };

    let conn = zbus::connection::Builder::session()
        .expect("session bus unavailable")
        .name("org.freedesktop.Notifications")
        .expect("could not claim org.freedesktop.Notifications — is another daemon running?")
        .serve_at("/org/freedesktop/Notifications", server)
        .expect("serve_at failed")
        .build()
        .await
        .expect("D-Bus connection failed");

    // Emit empty list immediately so tauler has an initial value.
    emit(&[]);

    let mut store = Notifications::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::Add(n, timeout) => {
                let id = n.id;
                let generation = store.upsert(n);
                emit(store.items());

                // Schedule auto-removal. The generation travels with the timer
                // so a later replacement of the same id survives this one.
                if let Some(ms) = expire_ms(timeout) {
                    let tx = event_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                        let _ = tx.send(Event::Expire { id, generation });
                    });
                }
            }
            Event::Expire { id, generation } => {
                if store.expire(id, generation) {
                    emit(store.items());
                    notify_closed(&conn, id, CloseReason::Expired).await;
                }
            }
            Event::Close { id, reason } => {
                if store.remove(id) {
                    emit(store.items());
                    notify_closed(&conn, id, reason).await;
                }
            }
        }
    }
}
