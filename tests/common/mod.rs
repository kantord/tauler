#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;

use costae::jsx::{EvalOutput, JsxEvaluator};

pub fn eval_jsx(source: &str) -> EvalOutput {
    JsxEvaluator::new(source, serde_json::Value::Null, None)
        .expect("JsxEvaluator::new failed")
        .eval(&HashMap::new())
        .expect("eval failed")
}

pub fn eval_jsx_with_ctx(source: &str, ctx: serde_json::Value) -> EvalOutput {
    JsxEvaluator::new(source, ctx, None)
        .expect("JsxEvaluator::new failed")
        .eval(&HashMap::new())
        .expect("eval failed")
}

pub fn eval_jsx_with_streams(
    source: &str,
    streams: HashMap<(String, Option<String>), String>,
) -> EvalOutput {
    JsxEvaluator::new(source, serde_json::Value::Null, None)
        .expect("JsxEvaluator::new failed")
        .eval(&streams)
        .expect("eval failed")
}

pub fn eval_jsx_from_dir(source: &str, base_dir: &Path) -> EvalOutput {
    JsxEvaluator::new(source, serde_json::Value::Null, Some(base_dir))
        .expect("JsxEvaluator::new failed")
        .eval(&HashMap::new())
        .expect("eval failed")
}
