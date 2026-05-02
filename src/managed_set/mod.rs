use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::hash::Hash;

pub mod reconcile;
pub use reconcile::{Reconcile, ReconcileErrors};

pub struct LifecycleContext {
    pub display_name: String,
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

pub trait Lifecycle: Display {
    /// Stable identity for this item. Forms a segment in the `KeyPath` used to address
    /// this item's logs and status in a supervisor tree.
    type Key: Hash + Eq + Clone + serde::Serialize + serde::de::DeserializeOwned;
    type State;
    type Context;
    type Output;
    type Error;

    /// Stable identity for this item within its parent's namespace.
    fn key(&self) -> Self::Key;

    /// Called once when this item first appears in the desired set. Returns the live
    /// state or an error; on `Err` the item is not added to the store.
    fn enter(
        self,
        ctx: &mut Self::Context,
        output: &mut Self::Output,
    ) -> Result<Self::State, Self::Error>;

    /// Called on every reconciliation cycle while the item remains present. Responsible
    /// for both synchronising this item's own state and triggering reconciliation of any
    /// child items it owns.
    fn reconcile_self(
        self,
        state: &mut Self::State,
        ctx: &mut Self::Context,
        output: &mut Self::Output,
    ) -> Result<(), Self::Error>;

    /// An `Err` return signals a zombie — cleanup did not complete cleanly.
    fn exit(
        state: Self::State,
        ctx: &mut Self::Context,
        output: &mut Self::Output,
    ) -> Result<(), Self::Error>;

    fn enhance_lifecycle_context(&self, _ctx: &mut LifecycleContext) {}

    fn enhance_lifecycle_state_context(_state: &Self::State, _ctx: &mut LifecycleContext) {}

    fn lifecycle_context(&self) -> LifecycleContext {
        let mut ctx = LifecycleContext {
            display_name: self.to_string(),
            metadata: serde_json::Map::new(),
        };
        self.enhance_lifecycle_context(&mut ctx);
        ctx
    }

    fn lifecycle_state_context(state: &Self::State) -> LifecycleContext {
        let mut ctx = LifecycleContext {
            display_name: String::new(),
            metadata: serde_json::Map::new(),
        };
        Self::enhance_lifecycle_state_context(state, &mut ctx);
        ctx
    }

    /// Override this wrapper to inject an observability hook around `enter`.
    fn wrap_enter(
        self,
        ctx: &mut Self::Context,
        output: &mut Self::Output,
    ) -> Result<Self::State, Self::Error>
    where
        Self: Sized,
    {
        self.enter(ctx, output)
    }

    /// Override this wrapper to inject an observability hook around `reconcile_self`.
    fn wrap_reconcile(
        self,
        state: &mut Self::State,
        ctx: &mut Self::Context,
        output: &mut Self::Output,
    ) -> Result<(), Self::Error>
    where
        Self: Sized,
    {
        self.reconcile_self(state, ctx, output)
    }

    /// Override this wrapper to inject an observability hook around `exit`.
    fn wrap_exit(
        state: Self::State,
        ctx: &mut Self::Context,
        output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        Self::exit(state, ctx, output)
    }
}

pub struct ManagedSet<T: Lifecycle> {
    store: HashMap<T::Key, T::State>,
}

impl<T: Lifecycle> Default for ManagedSet<T> {
    fn default() -> Self {
        Self {
            store: HashMap::new(),
        }
    }
}

impl<T: Lifecycle + 'static> ManagedSet<T>
where
    T::Error: Debug,
{
    pub fn new() -> Self {
        Self::default()
    }

    fn dedup_by_key(items: impl IntoIterator<Item = T>) -> HashMap<T::Key, T> {
        let mut map = HashMap::new();
        for item in items {
            map.insert(item.key(), item);
        }
        map
    }

    fn exit_removed(
        &mut self,
        new_map: &HashMap<T::Key, T>,
        ctx: &mut T::Context,
        output: &mut T::Output,
        errors: &mut ReconcileErrors<T::Key, T::Error>,
    ) {
        let exit_keys: Vec<T::Key> = self
            .store
            .keys()
            .filter(|k| !new_map.contains_key(*k))
            .cloned()
            .collect();
        for key in exit_keys {
            let state = self.store.remove(&key).unwrap();
            if let Err(e) = T::wrap_exit(state, ctx, output) {
                errors.push((key, e));
            }
        }
    }

    fn update_existing(
        &mut self,
        new_map: &mut HashMap<T::Key, T>,
        ctx: &mut T::Context,
        output: &mut T::Output,
        errors: &mut ReconcileErrors<T::Key, T::Error>,
    ) {
        let update_keys: Vec<T::Key> = new_map
            .keys()
            .filter(|k| self.store.contains_key(*k))
            .cloned()
            .collect();
        for key in update_keys {
            let item = new_map.remove(&key).unwrap();
            let state = self.store.get_mut(&key).unwrap();
            if let Err(e) = item.wrap_reconcile(state, ctx, output) {
                let old_state = self.store.remove(&key).unwrap();
                if let Err(exit_e) = T::wrap_exit(old_state, ctx, output) {
                    errors.push((key.clone(), exit_e));
                }
                errors.push((key, e));
            }
        }
    }

    fn enter_new(
        &mut self,
        mut new_map: HashMap<T::Key, T>,
        ctx: &mut T::Context,
        output: &mut T::Output,
        errors: &mut ReconcileErrors<T::Key, T::Error>,
    ) {
        let enter_keys: Vec<T::Key> = new_map
            .keys()
            .filter(|k| !self.store.contains_key(*k))
            .cloned()
            .collect();
        for key in enter_keys {
            let item = new_map.remove(&key).unwrap();
            match item.wrap_enter(ctx, output) {
                Ok(state) => {
                    self.store.insert(key, state);
                }
                Err(e) => {
                    errors.push((key, e));
                }
            }
        }
    }

    pub fn get(&self, key: &T::Key) -> Option<&T::State> {
        self.store.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&T::Key, &T::State)> {
        self.store.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&T::Key, &mut T::State)> {
        self.store.iter_mut()
    }

    pub fn get_mut(&mut self, key: &T::Key) -> Option<&mut T::State> {
        self.store.get_mut(key)
    }
}

impl<T: Lifecycle + 'static> reconcile::Reconcile<T> for ManagedSet<T>
where
    T::Error: Debug,
{
    fn reconcile(
        &mut self,
        desired: impl IntoIterator<Item = T>,
        ctx: &mut T::Context,
        output: &mut T::Output,
    ) -> ReconcileErrors<T::Key, T::Error> {
        let mut errors = ReconcileErrors::new();
        let mut new_map = Self::dedup_by_key(desired);
        self.exit_removed(&new_map, ctx, output, &mut errors);
        self.update_existing(&mut new_map, ctx, output, &mut errors);
        self.enter_new(new_map, ctx, output, &mut errors);
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestSpec {
        id: String,
        value: i32,
    }

    impl std::fmt::Display for TestSpec {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.id)
        }
    }

    impl Lifecycle for TestSpec {
        type Key = String;
        type State = i32;
        type Context = Arc<Mutex<Vec<String>>>;
        type Output = ();
        type Error = std::convert::Infallible;

        fn key(&self) -> String {
            self.id.clone()
        }

        fn enter(
            self,
            ctx: &mut Self::Context,
            _output: &mut (),
        ) -> Result<Self::State, Self::Error> {
            ctx.lock().unwrap().push(format!("enter:{}", self.id));
            Ok(self.value)
        }

        fn reconcile_self(
            self,
            state: &mut Self::State,
            ctx: &mut Self::Context,
            _output: &mut (),
        ) -> Result<(), Self::Error> {
            ctx.lock()
                .unwrap()
                .push(format!("reconcile_self:{}", self.id));
            *state = self.value;
            Ok(())
        }

        fn exit(
            state: Self::State,
            ctx: &mut Self::Context,
            _output: &mut Self::Output,
        ) -> Result<(), Self::Error> {
            ctx.lock().unwrap().push(format!("exit:{}", state));
            Ok(())
        }
    }

    fn make_ctx() -> Arc<Mutex<Vec<String>>> {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn calls(ctx: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        ctx.lock().unwrap().clone()
    }

    #[test]
    fn new_item_calls_enter_and_stores_state() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![TestSpec {
                id: "a".to_string(),
                value: 42,
            }],
            &mut ctx,
            &mut (),
        );
        assert!(calls(&ctx).contains(&"enter:a".to_string()));
        assert_eq!(ms.get(&"a".to_string()), Some(&42));
    }

    #[test]
    fn removed_item_calls_exit_with_old_state() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![TestSpec {
                id: "a".to_string(),
                value: 99,
            }],
            &mut ctx,
            &mut (),
        );
        ms.reconcile(vec![], &mut ctx, &mut ());
        assert!(calls(&ctx).contains(&"exit:99".to_string()));
    }

    #[test]
    fn existing_item_calls_reconcile_self_not_enter() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![TestSpec {
                id: "a".to_string(),
                value: 1,
            }],
            &mut ctx,
            &mut (),
        );
        ms.reconcile(
            vec![TestSpec {
                id: "a".to_string(),
                value: 2,
            }],
            &mut ctx,
            &mut (),
        );
        let log = calls(&ctx);
        assert_eq!(log.iter().filter(|c| *c == "enter:a").count(), 1);
        assert!(log.contains(&"reconcile_self:a".to_string()));
    }

    #[test]
    fn duplicate_keys_in_batch_only_one_enter() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![
                TestSpec {
                    id: "a".to_string(),
                    value: 1,
                },
                TestSpec {
                    id: "a".to_string(),
                    value: 2,
                },
            ],
            &mut ctx,
            &mut (),
        );
        let log = calls(&ctx);
        assert_eq!(log.iter().filter(|c| *c == "enter:a").count(), 1);
    }

    #[test]
    fn get_returns_state_after_enter() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![TestSpec {
                id: "b".to_string(),
                value: 7,
            }],
            &mut ctx,
            &mut (),
        );
        assert_eq!(ms.get(&"b".to_string()), Some(&7));
    }

    #[test]
    fn get_returns_updated_state_after_reconcile() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![TestSpec {
                id: "c".to_string(),
                value: 10,
            }],
            &mut ctx,
            &mut (),
        );
        ms.reconcile(
            vec![TestSpec {
                id: "c".to_string(),
                value: 20,
            }],
            &mut ctx,
            &mut (),
        );
        assert_eq!(ms.get(&"c".to_string()), Some(&20));
    }

    #[test]
    fn iter_mut_yields_mutable_state_visible_via_get() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![TestSpec {
                id: "d".to_string(),
                value: 5,
            }],
            &mut ctx,
            &mut (),
        );
        for (_k, v) in ms.iter_mut() {
            *v = 99;
        }
        assert_eq!(ms.get(&"d".to_string()), Some(&99));
    }

    #[test]
    fn get_mut_returns_mutable_reference_visible_via_get() {
        let mut ctx = make_ctx();
        let mut ms: ManagedSet<TestSpec> = ManagedSet::new();
        ms.reconcile(
            vec![TestSpec {
                id: "e".to_string(),
                value: 3,
            }],
            &mut ctx,
            &mut (),
        );
        if let Some(v) = ms.get_mut(&"e".to_string()) {
            *v = 77;
        }
        assert_eq!(ms.get(&"e".to_string()), Some(&77));
    }

    mod enter_err {
        use super::super::*;

        #[derive(Clone)]
        struct FallibleSpec {
            id: String,
            fail: bool,
        }

        impl std::fmt::Display for FallibleSpec {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.id)
            }
        }

        #[derive(Debug, PartialEq)]
        struct FallibleError(String);

        impl Lifecycle for FallibleSpec {
            type Key = String;
            type State = String;
            type Context = ();
            type Output = ();
            type Error = FallibleError;

            fn key(&self) -> String {
                self.id.clone()
            }

            fn enter(self, _ctx: &mut (), _output: &mut ()) -> Result<String, FallibleError> {
                if self.fail {
                    Err(FallibleError(format!("enter failed for {}", self.id)))
                } else {
                    Ok(format!("state:{}", self.id))
                }
            }

            fn reconcile_self(
                self,
                state: &mut String,
                _ctx: &mut (),
                _output: &mut (),
            ) -> Result<(), FallibleError> {
                *state = format!("updated:{}", self.id);
                Ok(())
            }

            fn exit(
                _state: String,
                _ctx: &mut (),
                _output: &mut Self::Output,
            ) -> Result<(), FallibleError> {
                Ok(())
            }
        }

        #[test]
        fn enter_err_not_added_to_store_error_returned() {
            let mut ms: ManagedSet<FallibleSpec> = ManagedSet::new();
            let errors = ms.reconcile(
                vec![FallibleSpec {
                    id: "x".to_string(),
                    fail: true,
                }],
                &mut (),
                &mut (),
            );
            assert!(
                ms.get(&"x".to_string()).is_none(),
                "item must not be in store after enter Err"
            );
            assert_eq!(errors.len(), 1, "one error must be returned");
            assert_eq!(errors[0].0, "x");
            assert_eq!(errors[0].1, FallibleError("enter failed for x".to_string()));
        }

        #[test]
        fn enter_ok_adds_item_to_store_no_errors() {
            let mut ms: ManagedSet<FallibleSpec> = ManagedSet::new();
            let errors = ms.reconcile(
                vec![FallibleSpec {
                    id: "y".to_string(),
                    fail: false,
                }],
                &mut (),
                &mut (),
            );
            assert_eq!(ms.get(&"y".to_string()), Some(&"state:y".to_string()));
            assert!(errors.is_empty(), "no errors when enter returns Ok");
        }
    }

    mod reconcile_err {
        use super::super::*;

        #[derive(Clone)]
        struct UpdateFallibleSpec {
            id: String,
            fail_update: bool,
        }

        impl std::fmt::Display for UpdateFallibleSpec {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.id)
            }
        }

        #[derive(Debug, PartialEq)]
        struct UpdateError(String);

        static EXIT_CALLED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        impl Lifecycle for UpdateFallibleSpec {
            type Key = String;
            type State = String;
            type Context = ();
            type Output = ();
            type Error = UpdateError;

            fn key(&self) -> String {
                self.id.clone()
            }

            fn enter(self, _ctx: &mut (), _output: &mut ()) -> Result<String, UpdateError> {
                Ok(format!("state:{}", self.id))
            }

            fn reconcile_self(
                self,
                _state: &mut String,
                _ctx: &mut (),
                _output: &mut (),
            ) -> Result<(), UpdateError> {
                if self.fail_update {
                    Err(UpdateError(format!("update failed for {}", self.id)))
                } else {
                    Ok(())
                }
            }

            fn exit(
                _state: String,
                _ctx: &mut (),
                _output: &mut Self::Output,
            ) -> Result<(), UpdateError> {
                EXIT_CALLED.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }

        #[test]
        fn reconcile_err_exit_called_entry_removed_error_returned() {
            EXIT_CALLED.store(false, std::sync::atomic::Ordering::SeqCst);
            let mut ms: ManagedSet<UpdateFallibleSpec> = ManagedSet::new();

            let e1 = ms.reconcile(
                vec![UpdateFallibleSpec {
                    id: "z".to_string(),
                    fail_update: false,
                }],
                &mut (),
                &mut (),
            );
            assert!(e1.is_empty());
            assert!(
                ms.get(&"z".to_string()).is_some(),
                "item should be in store after successful enter"
            );

            let errors = ms.reconcile(
                vec![UpdateFallibleSpec {
                    id: "z".to_string(),
                    fail_update: true,
                }],
                &mut (),
                &mut (),
            );
            assert_eq!(
                errors.len(),
                1,
                "one error must be returned on reconcile failure"
            );
            assert_eq!(errors[0].0, "z");
            assert_eq!(errors[0].1, UpdateError("update failed for z".to_string()));
            assert!(
                ms.get(&"z".to_string()).is_none(),
                "item must be removed from store after reconcile Err"
            );
            assert!(
                EXIT_CALLED.load(std::sync::atomic::Ordering::SeqCst),
                "exit must be called after reconcile Err"
            );

            EXIT_CALLED.store(false, std::sync::atomic::Ordering::SeqCst);
            let e3 = ms.reconcile(
                vec![UpdateFallibleSpec {
                    id: "z".to_string(),
                    fail_update: false,
                }],
                &mut (),
                &mut (),
            );
            assert!(e3.is_empty(), "third call should succeed via enter");
            assert!(
                ms.get(&"z".to_string()).is_some(),
                "item should be re-entered on third call"
            );
        }
    }

    mod lifecycle_trace_tests {
        use super::super::*;
        use costae_lifecycle_derive::lifecycle_trace;

        struct TraceSpec {
            id: String,
        }

        impl std::fmt::Display for TraceSpec {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.id)
            }
        }

        #[lifecycle_trace]
        impl Lifecycle for TraceSpec {
            type Key = String;
            type State = ();
            type Context = ();
            type Output = ();
            type Error = std::convert::Infallible;

            fn key(&self) -> String {
                self.id.clone()
            }

            fn enter(self, _ctx: &mut (), _output: &mut ()) -> Result<(), Self::Error> {
                Ok(())
            }

            fn reconcile_self(
                self,
                _state: &mut (),
                _ctx: &mut (),
                _output: &mut (),
            ) -> Result<(), Self::Error> {
                Ok(())
            }

            fn exit(
                _state: (),
                _ctx: &mut (),
                _output: &mut Self::Output,
            ) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        #[test]
        #[tracing_test::traced_test]
        fn lifecycle_trace_entering_emits_info_event_with_key_name_metadata() {
            let mut ms: ManagedSet<TraceSpec> = ManagedSet::new();
            ms.reconcile(
                vec![TraceSpec {
                    id: "trace-panel".to_string(),
                }],
                &mut (),
                &mut (),
            );
            assert!(logs_contain("entering"), "expected an 'entering' log entry");
            assert!(
                logs_contain("trace-panel"),
                "expected key/name 'trace-panel' in log"
            );
        }
    }

    mod channel_output {
        use super::super::*;
        use std::sync::mpsc;

        #[derive(Clone)]
        struct ChannelOutputLifecycle {
            id: String,
        }

        impl std::fmt::Display for ChannelOutputLifecycle {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.id)
            }
        }

        impl Lifecycle for ChannelOutputLifecycle {
            type Key = String;
            type State = ();
            type Context = ();
            type Output = mpsc::Sender<String>;
            type Error = std::convert::Infallible;

            fn key(&self) -> String {
                self.id.clone()
            }

            fn enter(
                self,
                _ctx: &mut (),
                output: &mut mpsc::Sender<String>,
            ) -> Result<(), Self::Error> {
                output.send(format!("entered:{}", self.id)).unwrap();
                Ok(())
            }

            fn reconcile_self(
                self,
                _state: &mut (),
                _ctx: &mut (),
                output: &mut mpsc::Sender<String>,
            ) -> Result<(), Self::Error> {
                output.send(format!("reconciled:{}", self.id)).unwrap();
                Ok(())
            }

            fn exit(
                _state: (),
                _ctx: &mut (),
                _output: &mut Self::Output,
            ) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        #[test]
        fn enter_receives_output_and_can_write_to_it() {
            let (mut tx, rx) = mpsc::channel::<String>();
            let mut ms: ManagedSet<ChannelOutputLifecycle> = ManagedSet::new();
            ms.reconcile(
                vec![ChannelOutputLifecycle {
                    id: "o1".to_string(),
                }],
                &mut (),
                &mut tx,
            );
            drop(tx);
            let msgs: Vec<String> = rx.try_iter().collect();
            assert!(
                msgs.contains(&"entered:o1".to_string()),
                "enter must write to output; got: {:?}",
                msgs
            );
        }

        #[test]
        fn exit_does_not_receive_output() {
            let (mut tx, rx) = mpsc::channel::<String>();
            let mut ms: ManagedSet<ChannelOutputLifecycle> = ManagedSet::new();
            ms.reconcile(
                vec![ChannelOutputLifecycle {
                    id: "o2".to_string(),
                }],
                &mut (),
                &mut tx,
            );
            ms.reconcile(vec![], &mut (), &mut tx);
            drop(tx);
            let msgs: Vec<String> = rx.try_iter().collect();
            assert!(
                !msgs.iter().any(|m| m.starts_with("exited:")),
                "exit must not write to output; got: {:?}",
                msgs
            );
        }
    }
}
