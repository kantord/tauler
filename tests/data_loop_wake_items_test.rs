//! A ping and a pending line arriving together must still deliver the line.
//!
//! The original bug was specific to a mode that no longer exists: a wake signal
//! put the loop into "awake", and that branch `continue`d before ever receiving
//! an item, so the item was skipped. Items are now drained unconditionally at
//! the top of every pass, which makes that shape unrepresentable — but the
//! property is worth holding onto whatever the loop looks like inside.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauler::data::data_loop::{DataLoop, ProcessIdentity, ProcessSpec, StreamItem, StreamSource};

fn echo_spec(msg: &str) -> ProcessSpec {
    ProcessSpec {
        identity: ProcessIdentity {
            bin: "/bin/sh".to_string(),
            key: "/bin/sh:".to_string(),
        },
        args: vec!["-c".into(), format!("echo {msg}").into()],
        env: BTreeMap::new(),
        current_dir: None,
        props: None,
    }
}

#[test]
fn a_ping_racing_a_stream_line_still_delivers_the_line() {
    let (mut data_loop, handle) = DataLoop::new();
    let wake_tx = data_loop.notifier();

    // Configure a spec that prints one line and then exits.
    handle.set_desired(vec![StreamSource::Process(echo_spec("hello_awake"))]);

    let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let collected_for_run = Arc::clone(&collected);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_run = Arc::clone(&stop);

    thread::spawn(move || {
        data_loop.run(
            stop_for_run,
            |item: StreamItem| {
                collected_for_run.lock().unwrap().push(item.line);
            },
            || {},
        );
    });

    // Ping immediately, so the ping and the subprocess's first line race.
    let _ = wake_tx.try_send(());

    // Wait up to 3 s for the item to be delivered.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if !collected.lock().unwrap().is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out: on_item was never called for a line that raced a ping"
        );
        thread::sleep(Duration::from_millis(20));
    }

    stop.store(true, Ordering::Relaxed);

    let items = collected.lock().unwrap();
    assert!(
        items.iter().any(|l| l == "hello_awake"),
        "expected 'hello_awake' to be delivered, got: {:?}",
        *items
    );
}
