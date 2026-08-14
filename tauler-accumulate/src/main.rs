//! Buffers a line-oriented stream and re-emits the last N values as a JSON array.
//!
//! tauler keeps exactly one value per stream — the latest line — so a layout file
//! cannot show anything that happened before now. This binary is what gives a naive
//! source history without making it stateful, per ADR 0014:
//!
//! ```sh
//! journalctl -f -o json | tauler-accumulate -n 5
//! ```
//!
//! It is a ring buffer and nothing else. Each input line is parsed as JSON if it
//! parses and kept as a string if it does not, so the accumulator never interprets
//! what it is holding — a Module's vocabulary stays the Module's business. Anything
//! that looks like a query or a transform belongs on the other side of the boundary:
//! in the pipe (`jq -c .MESSAGE | …`) or in the layout file, which is already
//! JavaScript.
//!
//! One array is written per input line, oldest first, starting from the very first
//! line rather than waiting for the window to fill — a widget that appears
//! immediately and grows beats one that is blank until N samples have arrived.

use std::collections::VecDeque;
use std::io::{BufRead, Write};

use clap::Parser;

#[derive(Parser)]
#[command(
    about = "Buffer a line stream and emit the last N values as a JSON array",
    long_about = None,
)]
struct Args {
    /// How many values to keep.
    #[arg(short = 'n', long = "count", default_value_t = 60)]
    count: usize,
}

/// One input line as it will appear in the window.
///
/// Parsing is a convenience for the consumer, not interpretation: a line that is
/// JSON arrives in the layout file as an object or a number, and a line that is not
/// arrives as a string. Either way the value is passed through unchanged.
fn parse_line(line: &str) -> serde_json::Value {
    serde_json::from_str(line).unwrap_or_else(|_| serde_json::Value::String(line.to_string()))
}

/// Push `line` into `window`, evicting the oldest value once it holds `count`.
fn push(window: &mut VecDeque<serde_json::Value>, line: &str, count: usize) {
    if count == 0 {
        return;
    }
    if window.len() == count {
        window.pop_front();
    }
    window.push_back(parse_line(line));
}

fn window_json(window: &VecDeque<serde_json::Value>) -> String {
    serde_json::Value::Array(window.iter().cloned().collect()).to_string()
}

fn run(input: impl BufRead, mut output: impl Write, count: usize) -> std::io::Result<()> {
    let mut window: VecDeque<serde_json::Value> = VecDeque::with_capacity(count);
    for line in input.lines() {
        push(&mut window, &line?, count);
        writeln!(output, "{}", window_json(&window))?;
        // A status bar reads this a line at a time and re-renders on each one.
        // Without a flush the window would sit in the pipe buffer until it filled,
        // which for a once-a-second source is minutes of a frozen widget.
        output.flush()?;
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run(stdin.lock(), stdout.lock(), args.count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accumulate(input: &str, count: usize) -> Vec<String> {
        let mut out = Vec::new();
        run(input.as_bytes(), &mut out, count).expect("run failed");
        String::from_utf8(out)
            .expect("output was not utf8")
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn emits_one_array_per_input_line() {
        let out = accumulate("1\n2\n3\n", 60);
        assert_eq!(out, vec!["[1]", "[1,2]", "[1,2,3]"]);
    }

    #[test]
    fn window_is_oldest_first() {
        let out = accumulate("1\n2\n3\n", 60);
        assert_eq!(out.last().unwrap(), "[1,2,3]");
    }

    #[test]
    fn evicts_the_oldest_value_once_full() {
        let out = accumulate("1\n2\n3\n4\n", 2);
        assert_eq!(out, vec!["[1]", "[1,2]", "[2,3]", "[3,4]"]);
    }

    #[test]
    fn emits_a_partial_window_before_it_fills() {
        let out = accumulate("1\n", 60);
        assert_eq!(out, vec!["[1]"]);
    }

    #[test]
    fn json_lines_are_embedded_as_json() {
        let out = accumulate(r#"{"a":1}"#.to_string().as_str(), 60);
        assert_eq!(out, vec![r#"[{"a":1}]"#]);
    }

    #[test]
    fn non_json_lines_are_embedded_as_strings() {
        let out = accumulate("hello\n", 60);
        assert_eq!(out, vec![r#"["hello"]"#]);
    }

    /// A bare number is valid JSON, so it must not arrive quoted — a chart doing
    /// arithmetic on the window would otherwise get strings.
    #[test]
    fn bare_numbers_are_not_quoted() {
        let out = accumulate("0.41\n", 60);
        assert_eq!(out, vec!["[0.41]"]);
    }

    /// Mixed input is the realistic case for a shell loop whose command
    /// occasionally fails and prints an error instead of a value.
    #[test]
    fn json_and_non_json_lines_can_share_a_window() {
        let out = accumulate("1\noops\n", 60);
        assert_eq!(out.last().unwrap(), r#"[1,"oops"]"#);
    }

    #[test]
    fn a_line_that_is_a_json_string_stays_one_value() {
        let out = accumulate(r#""already a string""#, 60);
        assert_eq!(out, vec![r#"["already a string"]"#]);
    }

    /// Guards the eviction arithmetic against an off-by-one that would let the
    /// window grow past `count`.
    #[test]
    fn window_never_exceeds_count() {
        let input = (0..100)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let out = accumulate(&input, 3);
        let last: serde_json::Value = serde_json::from_str(out.last().unwrap()).unwrap();
        assert_eq!(last, serde_json::json!([97, 98, 99]));
    }

    #[test]
    fn empty_input_emits_nothing() {
        assert!(accumulate("", 60).is_empty());
    }

    /// `-n 0` is meaningless but must not panic or emit a growing window.
    #[test]
    fn a_count_of_zero_emits_empty_windows() {
        let out = accumulate("1\n2\n", 0);
        assert_eq!(out, vec!["[]", "[]"]);
    }
}
