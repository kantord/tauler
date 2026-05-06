pub use optative::*;

#[cfg(test)]
mod lifecycle_trace_tests {
    use optative::{Lifecycle, ManagedSet, Reconcile};
    use tauler_lifecycle_derive::lifecycle_trace;

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
