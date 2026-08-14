//! Per-tag default styles — the reason `<p>` has margins and `<div>` stacks.
//!
//! Presets are the lowest cascade layer: takumi applies them under `class` and `style`,
//! so anything written on the node wins. tauler does not compute them; the table below
//! is Chromium's user-agent stylesheet as restated by `takumi-html`, vendored rather
//! than depended on for the reasons in `docs/adr/0017`.
//!
//! The CSS strings are parsed once, on first use, into takumi `Style` values. Parsing is
//! deliberately loose: a declaration takumi does not understand is dropped and the rest
//! of the block still applies, which is what CSS itself does.

use std::collections::HashMap;
use std::sync::LazyLock;

use takumi::prelude::{Style, StyleDeclarationBlock};

// ─── vendored from takumi-html ─────────────────────────────────────────────────────
// Source: https://github.com/kane50613/takumi/blob/6d31b7c5feeefafc360e5b09500ebc4d849f6f27/takumi-html/src/lib.rs#L41-L150
// takumi-html 0.2.0 — Copyright (c) 2025 Kane Wang — MIT OR Apache-2.0
//
// Copied because `takumi-html`'s only entry point takes an HTML string and we have a
// tree (ADR 0017). Re-sync when bumping takumi-core: diff against the permalink above.
//
// TODO: a public `preset_for_tag` accessor upstream would delete this copy entirely,
// and with it the only way this file can go silently wrong.
const DEFAULT_PRESETS: &[(&str, &str)] = &[
  ("html", "display:block"),
  ("head", "display:none"),
  ("meta", "display:none"),
  ("title", "display:none"),
  ("link", "display:none"),
  ("style", "display:none"),
  ("script", "display:none"),
  ("noscript", "display:none"),
  ("datalist", "display:none"),
  ("template", "display:none"),
  ("body", "margin:8px;display:block"),
  ("p", "margin-top:1em;margin-bottom:1em;display:block"),
  (
    "blockquote",
    "margin-top:1em;margin-bottom:1em;margin-left:40px;margin-right:40px;display:block",
  ),
  (
    "figure",
    "margin-top:1em;margin-bottom:1em;margin-left:40px;margin-right:40px;display:block",
  ),
  ("figcaption", "display:block"),
  ("address", "font-style:italic;display:block"),
  ("article", "display:block"),
  ("aside", "display:block"),
  ("footer", "display:block"),
  ("header", "display:block"),
  ("hgroup", "display:block"),
  ("main", "display:block"),
  ("nav", "display:block"),
  ("section", "display:block"),
  ("center", "text-align:center;display:block"),
  (
    "hr",
    "margin-top:0.5em;margin-bottom:0.5em;margin-left:auto;margin-right:auto;border-width:1px;display:block",
  ),
  (
    "ul",
    "margin-top:1em;margin-bottom:1em;padding-left:40px;display:block",
  ),
  (
    "ol",
    "margin-top:1em;margin-bottom:1em;padding-left:40px;display:block",
  ),
  (
    "menu",
    "margin-top:1em;margin-bottom:1em;padding-left:40px;display:block",
  ),
  ("li", "display:block"),
  ("dl", "margin-top:1em;margin-bottom:1em;display:block"),
  ("dt", "display:block"),
  ("dd", "margin-left:40px;display:block"),
  ("form", "display:block"),
  (
    "fieldset",
    "margin-left:2px;margin-right:2px;padding-top:0.35em;padding-right:0.75em;padding-bottom:0.625em;padding-left:0.75em;border-width:2px;display:block",
  ),
  ("legend", "padding-left:2px;padding-right:2px;display:block"),
  ("details", "display:block"),
  ("summary", "display:block"),
  ("search", "display:block"),
  (
    "h1",
    "font-size:2em;margin-top:0.67em;margin-bottom:0.67em;margin-left:0;margin-right:0;font-weight:bold;display:block",
  ),
  (
    "h2",
    "font-size:1.5em;margin-top:0.83em;margin-bottom:0.83em;margin-left:0;margin-right:0;font-weight:bold;display:block",
  ),
  (
    "h3",
    "font-size:1.17em;margin-top:1em;margin-bottom:1em;margin-left:0;margin-right:0;font-weight:bold;display:block",
  ),
  (
    "h4",
    "margin-top:1.33em;margin-bottom:1.33em;margin-left:0;margin-right:0;font-weight:bold;display:block",
  ),
  (
    "h5",
    "font-size:0.83em;margin-top:1.67em;margin-bottom:1.67em;margin-left:0;margin-right:0;font-weight:bold;display:block",
  ),
  (
    "h6",
    "font-size:0.67em;margin-top:2.33em;margin-bottom:2.33em;margin-left:0;margin-right:0;font-weight:bold;display:block",
  ),
  ("u", "text-decoration:underline"),
  ("ins", "text-decoration:underline"),
  ("strong", "font-weight:bolder"),
  ("b", "font-weight:bolder"),
  ("i", "font-style:italic"),
  ("em", "font-style:italic"),
  ("cite", "font-style:italic"),
  ("dfn", "font-style:italic"),
  ("code", "font-family:monospace"),
  ("kbd", "font-family:monospace"),
  ("samp", "font-family:monospace"),
  (
    "pre",
    "font-family:monospace;white-space:pre;margin:1em 0;display:block",
  ),
  ("mark", "background-color:yellow;color:black"),
  ("big", "font-size:larger"),
  ("small", "font-size:smaller"),
  ("s", "text-decoration:line-through"),
  ("del", "text-decoration:line-through"),
  ("sub", "font-size:smaller;vertical-align:sub"),
  ("sup", "font-size:smaller;vertical-align:super"),
  ("div", "display:block"),
  ("br", "white-space:pre"),
];
// ─── end vendored ──────────────────────────────────────────────────────────────────

static PRESETS: LazyLock<HashMap<&'static str, Style>> = LazyLock::new(|| {
    DEFAULT_PRESETS
        .iter()
        .map(|&(tag, css)| (tag, Style::from(StyleDeclarationBlock::parse_loosy(css))))
        .collect()
});

/// The preset for `tag`, or `None` for a tag Chromium gives no defaults to.
///
/// A tag with no preset keeps takumi's own default, which is `display: inline` — the
/// same thing a browser does with an element it has never heard of.
pub fn preset_for_tag(tag: &str) -> Option<&'static Style> {
    PRESETS.get(tag)
}
