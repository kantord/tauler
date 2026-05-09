use super::Lifecycle;

pub type ReconcileErrors<K, E> = Vec<(K, E)>;

pub trait Reconcile<T: Lifecycle> {
    fn reconcile(
        &mut self,
        desired: impl IntoIterator<Item = T>,
        ctx: &mut T::Context,
        output: &mut T::Output,
    ) -> ReconcileErrors<T::Key, T::Error>;
}

#[cfg(test)]
mod tests {
    mod fixtures {
        use crate::reconcile::{Reconcile, ReconcileErrors};
        use crate::Lifecycle;
        use std::convert::Infallible;
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        pub struct Item {
            pub id: &'static str,
            pub value: i32,
        }

        impl std::fmt::Display for Item {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.id)
            }
        }

        pub type Ctx = Arc<Mutex<Vec<String>>>;

        impl Lifecycle for Item {
            type Key = String;
            type State = i32;
            type Context = Ctx;
            type Output = ();
            type Error = Infallible;

            fn key(&self) -> String {
                self.id.to_string()
            }

            fn enter(self, ctx: &mut Ctx, _output: &mut ()) -> Result<i32, Infallible> {
                ctx.lock().unwrap().push(format!("enter:{}", self.id));
                Ok(self.value)
            }

            fn reconcile_self(
                self,
                state: &mut i32,
                ctx: &mut Ctx,
                _output: &mut (),
            ) -> Result<(), Infallible> {
                ctx.lock()
                    .unwrap()
                    .push(format!("reconcile_self:{}", self.id));
                *state = self.value;
                Ok(())
            }

            fn exit(_state: i32, ctx: &mut Ctx, _output: &mut ()) -> Result<(), Infallible> {
                ctx.lock().unwrap().push("exit".to_string());
                Ok(())
            }
        }

        pub fn make_ctx() -> Ctx {
            Arc::new(Mutex::new(Vec::new()))
        }

        pub fn log(ctx: &Ctx) -> Vec<String> {
            ctx.lock().unwrap().clone()
        }

        pub struct RecordingReconciler {
            pub calls: Vec<Vec<String>>,
        }

        impl RecordingReconciler {
            pub fn new() -> Self {
                Self { calls: Vec::new() }
            }
        }

        impl Reconcile<Item> for RecordingReconciler {
            fn reconcile(
                &mut self,
                desired: impl IntoIterator<Item = Item>,
                _ctx: &mut Ctx,
                _output: &mut (),
            ) -> ReconcileErrors<String, Infallible> {
                self.calls
                    .push(desired.into_iter().map(|i| i.id.to_string()).collect());
                vec![]
            }
        }

        pub fn drive<R: Reconcile<Item>>(
            reconciler: &mut R,
            items: Vec<Item>,
            ctx: &mut Ctx,
        ) -> ReconcileErrors<String, Infallible> {
            reconciler.reconcile(items, ctx, &mut ())
        }
    }

    mod trait_usability {
        use super::fixtures::{drive, make_ctx, Item, RecordingReconciler};
        use crate::ManagedSet;

        #[test]
        fn accepts_managed_set() {
            let mut ctx = make_ctx();
            let mut ms: ManagedSet<Item> = ManagedSet::new();
            assert!(drive(&mut ms, vec![Item { id: "a", value: 1 }], &mut ctx).is_empty());
        }

        #[test]
        fn accepts_mock_reconciler() {
            let mut ctx = make_ctx();
            let mut mock = RecordingReconciler::new();
            drive(&mut mock, vec![Item { id: "b", value: 2 }], &mut ctx);
            assert_eq!(mock.calls, vec![vec!["b"]]);
        }
    }

    mod managed_set_via_trait {
        use super::fixtures::{log, make_ctx, Item};
        use crate::{ManagedSet, Reconcile};

        fn check<R: Reconcile<Item>>(
            reconciler: &mut R,
            setup: Vec<Item>,
            action: Vec<Item>,
            expected_log_entry: &str,
        ) {
            let mut ctx = make_ctx();
            reconciler.reconcile(setup, &mut ctx, &mut ());
            reconciler.reconcile(action, &mut ctx, &mut ());
            assert!(
                log(&ctx).iter().any(|e| e == expected_log_entry),
                "expected {:?} in log, got {:?}",
                expected_log_entry,
                log(&ctx)
            );
        }

        #[test]
        fn calls_enter_for_new_item() {
            check(
                &mut ManagedSet::new(),
                vec![],
                vec![Item { id: "a", value: 1 }],
                "enter:a",
            );
        }

        #[test]
        fn calls_reconcile_self_for_existing_item() {
            check(
                &mut ManagedSet::new(),
                vec![Item { id: "b", value: 1 }],
                vec![Item { id: "b", value: 2 }],
                "reconcile_self:b",
            );
        }

        #[test]
        fn calls_exit_for_removed_item() {
            check(
                &mut ManagedSet::new(),
                vec![Item { id: "c", value: 5 }],
                vec![],
                "exit",
            );
        }
    }

    mod mock_reconciler {
        use super::fixtures::{drive, log, make_ctx, Item, RecordingReconciler};

        #[test]
        fn records_calls_without_managing_state() {
            let mut ctx = make_ctx();
            let mut mock = RecordingReconciler::new();

            drive(&mut mock, vec![Item { id: "x", value: 99 }], &mut ctx);
            drive(
                &mut mock,
                vec![Item { id: "y", value: 1 }, Item { id: "z", value: 2 }],
                &mut ctx,
            );

            assert_eq!(mock.calls.len(), 2);
            assert_eq!(mock.calls[0], vec!["x"]);
            assert!(mock.calls[1].iter().any(|s| s == "y"));
            assert!(mock.calls[1].iter().any(|s| s == "z"));
            assert!(log(&ctx).is_empty());
        }
    }
}
