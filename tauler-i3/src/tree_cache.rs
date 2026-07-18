use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Thread-safe holder for the latest fetched i3/sway workspace snapshot.
///
/// Forward-looking plumbing for a future max-age-read consumer (per
/// plan.md) — for now only `publish()` has a caller (the refresh-worker),
/// so that's all this exposes; a `get(max_age)` blocking read can be added
/// back alongside whatever feature first needs it.
pub struct TreeCache<T> {
    state: Mutex<(Instant, Arc<T>)>,
}

impl<T> TreeCache<T> {
    pub fn new(initial: T) -> Self {
        Self {
            state: Mutex::new((Instant::now(), Arc::new(initial))),
        }
    }

    /// Publish a fresh value.
    pub fn publish(&self, value: T) {
        let mut guard = self.state.lock().unwrap();
        *guard = (Instant::now(), Arc::new(value));
    }
}
