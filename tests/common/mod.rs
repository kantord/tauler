use std::collections::HashMap;

use tauler::jsx::{EvalOutput, JsxEvaluator};

pub fn eval_jsx(source: &str) -> EvalOutput {
    JsxEvaluator::new(source, serde_json::Value::Null, None)
        .expect("JsxEvaluator::new failed")
        .eval(&HashMap::new())
        .expect("eval failed")
}
