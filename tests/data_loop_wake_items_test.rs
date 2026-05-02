/// Tests that DataLoop still calls on_item even when it is in "awake" mode
/// (i.e. after extra_rx fires).  The bug: when `awake == true` the loop does
/// `continue` before calling `recv_timeout`, so `on_item` is never invoked.
use costae::data::data_loop::{DataLoop, ProcessIdentity, ProcessSource, StreamItem, StreamSource};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

fn echo_spec(msg: &str) -> ProcessSource {
    ProcessSource {
        identity: ProcessIdentity {
            bin: "/bin/sh".to_string(),
            key: "/bin/sh".to_string(),
        },
        args: vec!["-c".to_string(), format!("echo {msg}")],
        env: BTreeMap::new(),
        current_dir: None,
        props: None,
        script: None,
    }
}

/// When `extra_rx` fires (putting the loop into awake mode) and there is also
/// a pending `StreamItem` in `self.rx`, `on_item` must still be called for
/// that item.
#[test]
fn awake_mode_still_delivers_stream_items() {
    let (wake_tx, wake_rx) = mpsc::channel::<()>();

    let (mut data_loop, handle) = DataLoop::new();
    data_loop = data_loop.with_extra_rx(wake_rx);

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

    // Send a wake signal immediately so the loop enters awake mode before (or
    // around the time) the subprocess output arrives.
    let _ = wake_tx.send(());

    // Wait up to 3 s for the item to be delivered.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if !collected.lock().unwrap().is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out: on_item was never called while loop was in awake mode"
        );
        thread::sleep(Duration::from_millis(20));
    }

    stop.store(true, Ordering::Relaxed);

    let items = collected.lock().unwrap();
    assert!(
        items.iter().any(|l| l == "hello_awake"),
        "expected 'hello_awake' to be delivered via on_item while awake, got: {:?}",
        *items
    );
}
