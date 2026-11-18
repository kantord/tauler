//! The compile pass that gives every Store its State identity (issue #271; see
//! `src/state.rs` in the `tauler` crate for the terms).
//!
//! A source-to-source splice that runs before `optative_script::jsx::transform_source`:
//! the file is parsed with oxc, every call of tauler's `store` is found, and
//! `, "<module>#<name>", "<token>"` is inserted after the call's one argument. No AST is
//! printed back, so every byte outside the insertions — and every line number — stays as
//! the author wrote it.
//!
//! DECISION: the pass is in this crate, behind the `store-pass` feature, and not in
//! `tauler` beside the evaluator that calls it. Reason: `tauler-docgen` runs it on every
//! example a page loads (`example_js` in `tauler-docgen/src/web.rs`) and depends on this
//! crate, not on `tauler`. The feature keeps oxc out of the wasm build.
//!
//! DECISION: "tauler's `store`" is the binding a file gets with
//! `import { store } from "tauler"` (or `import { store as s }`), found by scope analysis
//! (`oxc_semantic`), not by the spelling. A call whose callee resolves to any other
//! binding in the file (a local function, a parameter, an import from elsewhere) is left
//! alone. Evidence: `a_file_with_its_own_store_function_is_left_alone` and
//! `store_imported_under_another_name_is_named` in `src/jsx.rs`. Rejected: matching the
//! name alone — a Package with its own `store` helper would fail to load.
//!
//! DECISION: a call of a `store` that resolves to nothing in the file is refused with a
//! message that says to import it. Reason: "store is not defined" at run time does not
//! say what to write instead.
//!
//! DECISION: the pass also writes a token after the identity, and the host function
//! behind `store` refuses a declaration that does not carry it (`register_state_host` in
//! `src/jsx.rs`). The token is made per evaluator and exists only in the text this pass
//! hands to QuickJS, which no layout file can read. Reason: nothing in the syntax tells
//! an identity the pass wrote from one a person wrote, and `store` can be reached in
//! ways no static check sees (an alias, `import * as t`, `import("tau" + "ler")`, a
//! plain `.js` file the pass does not run on); one check at run time covers all of them.
//! Evidence: `store_called_with_a_hand_written_identity_is_refused` in `src/jsx.rs`.
//! Rejected: refusing each of those spellings here — a list, and a computed dynamic
//! import is not on it.
//!
//! DECISION: a file that names `globals`, resolving to nothing in the file, is refused
//! here with a message that points at `store`. `globals` was the state a layout file
//! had before Stores, and ADR 0051 removed it with no period where both exist. Reason:
//! this pass already has the file's scopes, so the check is one more name to look up,
//! and it is exact: a file's own `globals` variable or a `.globals` property is left
//! alone. Rejected: letting QuickJS throw "globals is not defined" at the first render
//! — the layout loads, and the message does not say what replaced it.
//!
//! DECISION: a file that contains neither the bytes `store` nor the bytes `globals` is
//! not parsed at all. Reason: the pass parses each file a second time
//! (`transform_source` parses it again), and almost no file declares a Store. Evidence:
//! `store_pass_cost` in `src/jsx.rs` prints the per-file cost of both branches.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    CallExpression, Declaration, Expression, IdentifierReference, ImportDeclarationSpecifier,
    Statement, VariableDeclaration,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SemanticBuilder, SymbolId};
use oxc_span::{GetSpan, SourceType};

/// The name `"tauler"` exports it under, and the name a file that forgot the import
/// calls.
const STORE: &str = "store";

/// What a layout file kept state in before Stores.
const GLOBALS: &str = "globals";

/// The module a layout file imports `store` from (`tauler-core/src/globals.rs`).
const TAULER_MODULE: &str = crate::globals::TAULER_MODULE;

/// A `store(` the pass refuses, with the 1-based line it is on.
#[derive(Debug, PartialEq, Eq)]
pub struct StoreError {
    pub line: usize,
    pub message: &'static str,
}

const NOT_TOP_LEVEL: &str = "store() must be the whole value of a top-level `const`, \
     as in `const name = store({...})`: its name is what finds the state again";
const ONE_ARGUMENT: &str =
    "store() takes exactly one argument, the default value; tauler adds the identity";
const NOT_IMPORTED: &str =
    "store is not a global: add `import { store } from \"tauler\"` to this file";

const GLOBALS_REMOVED: &str = "`globals` was removed: keep state in a Store instead — \
     `import { store } from \"tauler\"`, then `const name = store({...})` at the top \
     level of the file, and read and write `name.key`";

/// Rewrites every top-level `const X = store(default)` in `source` to
/// `const X = store(default, "<module>#X", "<token>")`.
///
/// Refuses a file that calls `store` any other way, and one that names `globals`.
///
/// `path` only picks the syntax (`.ts`, `.tsx`, anything else is JSX). A source that does
/// not parse is returned unchanged: `transform_source` and QuickJS already report that,
/// and reporting it twice in two formats helps nobody.
pub fn rewrite(source: &str, path: &str, module: &str, token: &str) -> Result<String, StoreError> {
    if !source.contains(STORE) && !source.contains(GLOBALS) {
        return Ok(source.to_string());
    }
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::jsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.diagnostics.has_errors() {
        return Ok(source.to_string());
    }
    let program = parsed.program;
    let semantic = SemanticBuilder::new().build(&program).semantic;
    let file = File {
        scoping: semantic.scoping(),
        stores: imported_stores(&program.body),
    };

    // (byte offset, text) to insert; and the span starts of the calls they belong to.
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let mut named_calls: Vec<u32> = Vec::new();
    let mut refusals = Refusals::default();

    for statement in &program.body {
        let declaration = match statement {
            Statement::VariableDeclaration(declaration) => declaration,
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => declaration,
                _ => continue,
            },
            _ => continue,
        };
        name_stores(
            declaration,
            &file,
            &format!("{module}#"),
            token,
            &mut inserts,
            &mut named_calls,
            &mut refusals,
        );
    }

    // Every other call of tauler's `store` is one the pass could not name; and `globals`.
    let mut rest = Rest {
        file: &file,
        named_calls: &named_calls,
        refusals: &mut refusals,
    };
    rest.visit_program(&program);
    if let Some((offset, message)) = refusals.first {
        return Err(StoreError {
            line: line_of(source, offset as usize),
            message,
        });
    }

    inserts.sort_by_key(|(offset, _)| *offset);
    let mut out = String::with_capacity(source.len() + inserts.len() * 48);
    let mut at = 0;
    for (offset, text) in inserts {
        out.push_str(&source[at..offset]);
        out.push_str(&text);
        at = offset;
    }
    out.push_str(&source[at..]);
    Ok(out)
}

/// What the pass knows about one file: its scopes, and the bindings that are tauler's
/// `store`.
struct File<'s> {
    scoping: &'s Scoping,
    stores: Vec<SymbolId>,
}

/// What a call's callee is, as far as the pass cares.
#[derive(PartialEq)]
enum Callee {
    /// The binding `import { store } from "tauler"` made.
    Store,
    /// `store`, resolving to nothing in the file.
    Unimported,
    Other,
}

impl File<'_> {
    fn callee(&self, call: &CallExpression<'_>) -> Callee {
        let Expression::Identifier(callee) = &call.callee else {
            return Callee::Other;
        };
        match self
            .scoping
            .get_reference(callee.reference_id())
            .symbol_id()
        {
            Some(symbol) if self.stores.contains(&symbol) => Callee::Store,
            None if callee.name == STORE => Callee::Unimported,
            _ => Callee::Other,
        }
    }
}

/// The local bindings of `store` in every `import { store } from "tauler"` of a file,
/// whatever each is renamed to.
fn imported_stores(body: &[Statement<'_>]) -> Vec<SymbolId> {
    let mut stores = Vec::new();
    for statement in body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        if import.source.value != TAULER_MODULE {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier {
                if specifier.imported.name() == STORE {
                    stores.push(specifier.local.symbol_id());
                }
            }
        }
    }
    stores
}

/// The earliest thing in the file the pass refuses.
#[derive(Default)]
struct Refusals {
    /// Byte offset and message.
    first: Option<(u32, &'static str)>,
}

impl Refusals {
    fn refuse(&mut self, offset: u32, message: &'static str) {
        if self.first.is_none_or(|(first, _)| offset < first) {
            self.first = Some((offset, message));
        }
    }
}

/// Queues the identity for each `X = store(default)` declarator of one top-level `const`.
fn name_stores(
    declaration: &VariableDeclaration<'_>,
    file: &File<'_>,
    module_prefix: &str,
    token: &str,
    inserts: &mut Vec<(usize, String)>,
    named_calls: &mut Vec<u32>,
    refusals: &mut Refusals,
) {
    if !declaration.kind.is_const() {
        return;
    }
    for declarator in &declaration.declarations {
        // `store({...}) as Desk` and `(store({...}))` are still the whole value.
        let Some(Expression::CallExpression(call)) = declarator
            .init
            .as_ref()
            .map(|init| init.get_inner_expression())
        else {
            continue;
        };
        if file.callee(call) != Callee::Store {
            continue;
        }
        // `const { open } = store(...)` has no one name; left for `Rest`.
        let Some(name) = declarator.id.get_binding_identifier() else {
            continue;
        };
        named_calls.push(call.span.start);
        if call.arguments.len() != 1 {
            refusals.refuse(call.span.start, ONE_ARGUMENT);
            continue;
        }
        let identity = format!("{module_prefix}{}", name.name);
        // After the argument, not before `)`: a trailing comma stays valid.
        inserts.push((
            call.arguments[0].span().end as usize,
            format!(
                ", {}, {}",
                serde_json::Value::String(identity),
                serde_json::Value::String(token.to_string())
            ),
        ));
    }
}

/// Finds what `name_stores` did not name.
struct Rest<'s> {
    file: &'s File<'s>,
    named_calls: &'s [u32],
    refusals: &'s mut Refusals,
}

impl<'a> Visit<'a> for Rest<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        match self.file.callee(call) {
            Callee::Store if !self.named_calls.contains(&call.span.start) => {
                self.refusals.refuse(call.span.start, NOT_TOP_LEVEL);
            }
            Callee::Unimported => self.refusals.refuse(call.span.start, NOT_IMPORTED),
            _ => {}
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_identifier_reference(&mut self, name: &IdentifierReference<'a>) {
        let unresolved = || {
            self.file
                .scoping
                .get_reference(name.reference_id())
                .symbol_id()
                .is_none()
        };
        if name.name == GLOBALS && unresolved() {
            self.refusals.refuse(name.span.start, GLOBALS_REMOVED);
        }
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMPORT: &str = "import { store } from \"tauler\";\n";

    fn pass(source: &str) -> Result<String, StoreError> {
        rewrite(source, "a.jsx", "a.jsx", "T")
    }

    #[test]
    fn names_a_private_and_an_exported_store() {
        let out = pass(&format!(
            "{IMPORT}const history = store({{ entries: [] }});\nexport const desk = store({{ open: false }},);\n"
        ))
        .unwrap();
        assert_eq!(
            out,
            format!(
                "{IMPORT}const history = store({{ entries: [] }}, \"a.jsx#history\", \"T\");\nexport const desk = store({{ open: false }}, \"a.jsx#desk\", \"T\",);\n"
            )
        );
    }

    #[test]
    fn names_a_store_imported_under_another_name() {
        let out = pass("import { store as s } from \"tauler\";\nconst desk = s({});").unwrap();
        assert_eq!(
            out,
            "import { store as s } from \"tauler\";\nconst desk = s({}, \"a.jsx#desk\", \"T\");"
        );
    }

    #[test]
    fn refuses_every_place_that_has_no_name() {
        for (source, line) in [
            ("function f() {\n  return store({});\n}", 2),
            ("let a = store({});", 1),
            ("const o = { a: store({}) };", 1),
            ("for (const x of [1]) {\n\n  const a = store({});\n}", 3),
            ("const { a } = store({ a: 1 });", 1),
            ("export default store({});", 1),
            ("const a = [store({})];", 1),
        ] {
            let err = pass(&format!("{IMPORT}{source}")).expect_err(source);
            assert_eq!(
                (err.line, err.message),
                (line + 1, NOT_TOP_LEVEL),
                "{source}"
            );
        }
    }

    #[test]
    fn refuses_a_hand_written_identity() {
        let err = pass(&format!("{IMPORT}const a = store({{}}, \"mine\");")).unwrap_err();
        assert_eq!(err.message, ONE_ARGUMENT);
        let err = pass(&format!("{IMPORT}const a = store();")).unwrap_err();
        assert_eq!(err.message, ONE_ARGUMENT);
    }

    #[test]
    fn reports_the_earliest_refusal() {
        let err = pass(&format!(
            "{IMPORT}const a = store();\nfunction f() {{ return store({{}}); }}"
        ))
        .unwrap_err();
        assert_eq!((err.line, err.message), (2, ONE_ARGUMENT));
    }

    #[test]
    fn refuses_a_store_call_that_was_not_imported() {
        for source in [
            "const a = store({});",
            "function f() {\n return store({}); }",
        ] {
            let err = pass(source).expect_err(source);
            assert_eq!(err.message, NOT_IMPORTED, "{source}");
            assert!(err.message.contains("import { store } from \"tauler\""));
        }
    }

    #[test]
    fn refuses_globals_and_points_at_store() {
        for (source, line) in [
            (
                "export default () => {\n  globals.n = 1;\n  return null;\n};",
                2,
            ),
            ("const n = globals.count ?? 0;", 1),
            ("const { a } = globals;", 1),
            ("function f() {\n\n  return <div>{globals.a}</div>;\n}", 3),
            ("const g = globalThis.x ?? globals;", 1),
        ] {
            let err = pass(source).expect_err(source);
            assert_eq!((err.line, err.message), (line, GLOBALS_REMOVED), "{source}");
            assert!(err.message.contains("store"));
        }
    }

    #[test]
    fn leaves_a_globals_that_is_the_files_own_alone() {
        for source in [
            "const globals = { n: 1 };\nexport default () => <div>{globals.n}</div>;",
            "export default (globals) => globals.n;",
            "const o = { globals: 1 };\nexport const n = o.globals;",
            "// <Panel> and <Module> are globals\nexport default () => null;",
        ] {
            assert_eq!(pass(source).unwrap(), source, "{source}");
        }
    }

    #[test]
    fn leaves_a_store_that_is_not_taulers_alone() {
        for source in [
            "function f(store) { return store({}); }",
            "import { store } from \"./mine\";\nconst a = store({});",
            "const store = (v) => v;\nconst a = store({});",
        ] {
            assert_eq!(pass(source).unwrap(), source, "{source}");
        }
    }

    #[test]
    fn a_shadowed_store_is_left_alone_beside_taulers() {
        let source =
            format!("{IMPORT}function f(store) {{ return store({{}}); }}\nconst a = store({{}});");
        assert_eq!(
            pass(&source).unwrap(),
            format!("{IMPORT}function f(store) {{ return store({{}}); }}\nconst a = store({{}}, \"a.jsx#a\", \"T\");")
        );
    }
}
