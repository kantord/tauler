use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use crate::managed_set::Lifecycle;
use optative_derive::lifecycle_trace;

use super::StreamItem;

pub struct BuiltInState {
    pub handle: JoinHandle<()>,
    pub stop: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct BuiltInSource {
    pub key: String,
    pub func: fn(mpsc::Sender<StreamItem>, String, Arc<AtomicBool>),
}

impl std::fmt::Display for BuiltInSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.key)
    }
}

#[lifecycle_trace]
impl Lifecycle for BuiltInSource {
    type Key = String;
    type State = BuiltInState;
    type Context = ();
    type Output = mpsc::Sender<StreamItem>;
    type Error = std::convert::Infallible;

    fn key(&self) -> String {
        self.key.clone()
    }

    fn display_name(&self) -> String {
        self.key.clone()
    }

    fn enter(self, _ctx: &mut (), output: &mut Self::Output) -> Result<Self::State, Self::Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let output_clone = output.clone();
        let key = self.key();
        let handle = std::thread::spawn(move || (self.func)(output_clone, key, stop_clone));
        Ok(BuiltInState { handle, stop })
    }

    fn reconcile_self(
        self,
        state: &mut Self::State,
        _ctx: &mut (),
        output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        if state.handle.is_finished() {
            let stop = Arc::new(AtomicBool::new(false));
            let stop_clone = Arc::clone(&stop);
            let output_clone = output.clone();
            let key = self.key();
            let handle = std::thread::spawn(move || (self.func)(output_clone, key, stop_clone));
            state.handle = handle;
            state.stop = stop;
        }
        Ok(())
    }

    fn exit(
        state: Self::State,
        _ctx: &mut (),
        _output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        state.stop.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::data_loop::StreamKind;
    use std::sync::atomic::Ordering;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    /// Test helper: sends exactly one StreamItem then returns.
    fn send_one_item(tx: mpsc::Sender<StreamItem>, key: String, _stop: Arc<AtomicBool>) {
        let _ = tx.send(StreamItem {
            key: (key, None),
            stream: StreamKind::Stdout,
            line: "hello-from-builtin".to_string(),
        });
    }

    // Claim 1: `enter` spawns a thread; a function that sends one StreamItem then exits
    // should deliver that item to the receiver.
    #[test]
    fn enter_spawns_thread_and_delivers_item() {
        let (mut tx, rx) = mpsc::channel::<StreamItem>();
        let source = BuiltInSource {
            key: "test-source".to_string(),
            func: send_one_item,
        };

        let _state = source
            .enter(&mut (), &mut tx)
            .expect("enter must return Ok(state)");

        let item = rx
            .recv_timeout(Duration::from_millis(500))
            .expect("enter must spawn a thread that delivers a StreamItem");
        assert_eq!(
            item.key.0, "test-source",
            "StreamItem key must match the source key"
        );
        assert_eq!(
            item.line, "hello-from-builtin",
            "StreamItem line must match what the helper function sent"
        );
    }

    // Claim 2: `update` restarts a finished thread; after the thread from `enter` exits
    // naturally, calling `update` should spawn a fresh thread that delivers another item.
    #[test]
    fn update_restarts_finished_thread() {
        let (mut tx, rx) = mpsc::channel::<StreamItem>();
        let source = BuiltInSource {
            key: "restart-source".to_string(),
            func: send_one_item,
        };

        // enter: thread runs, sends item, then finishes
        let mut state = source
            .clone()
            .enter(&mut (), &mut tx)
            .expect("enter must succeed");

        // drain the first item
        let _ = rx
            .recv_timeout(Duration::from_millis(500))
            .expect("first item must arrive after enter");

        // wait for the thread to finish naturally
        std::thread::sleep(Duration::from_millis(100));

        // update: should detect finished thread and restart it
        source
            .reconcile_self(&mut state, &mut (), &mut tx)
            .expect("reconcile_self must return Ok");

        let item = rx
            .recv_timeout(Duration::from_millis(500))
            .expect("update must restart the thread and deliver a new StreamItem");
        assert_eq!(
            item.key.0, "restart-source",
            "restarted thread must use the source key"
        );
        assert_eq!(
            item.line, "hello-from-builtin",
            "restarted thread must deliver the expected line"
        );
    }

    // Claim 3: `exit` sets the stop flag; calling `exit` on the state should set the
    // `stop` AtomicBool to true.
    #[test]
    fn exit_sets_stop_flag() {
        let (mut tx, rx) = mpsc::channel::<StreamItem>();
        let source = BuiltInSource {
            key: "exit-source".to_string(),
            func: send_one_item,
        };

        let state = source.enter(&mut (), &mut tx).expect("enter must succeed");

        // drain item so the channel doesn't block anything
        let _ = rx.recv_timeout(Duration::from_millis(500));

        // capture the stop Arc before handing state to exit
        let stop_clone = Arc::clone(&state.stop);

        assert!(
            !stop_clone.load(Ordering::Relaxed),
            "stop flag must be false before exit"
        );

        let (mut tx, _rx) = std::sync::mpsc::channel();
        let _ = BuiltInSource::exit(state, &mut (), &mut tx);

        assert!(
            stop_clone.load(Ordering::Relaxed),
            "exit must set the stop AtomicBool to true"
        );
    }
}
