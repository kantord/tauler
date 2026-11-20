//! The compile pass that gives every Store its State identity (issue #271; see
//! `src/state.rs` in the `tauler` crate for the terms, and `local_store.rs` in this
//! crate for a Store declared inside a function).
//!
//! A source-to-source splice that runs before `optative_script::jsx::transform_source`:
//! the file is parsed with oxc and text is inserted at these places. No AST is
//! printed back and no insertion holds a line break, so every byte outside the
//! insertions — and every line number — stays as the author wrote it
//! (`every_line_stays_where_the_author_wrote_it`).
//!
//! ```text
//! const desk = store({...})                 at the top level of the file
//!   -> store({...}, "<module>#desk", "<token>")
//! const s = store({...})                    directly in a function, at any depth of blocks
//!   -> __tauler_local_store({...}, "<module>#<function>.s", "<token>")
//! function Menu() {...}                     every function the file binds at its top level
//!   -> function Menu() {...};__tauler_name(Menu, "<module>#Menu");
//! export default () => ...                  a default export that binds no name
//!   -> export default __tauler_name(() => ..., "<module>#default");
//! export default function () {...}          the same, written as a declaration
//!   -> export default function __tauler_default() {...};__tauler_name(Object.defineProperty(__tauler_default, "name", { value: "default" }), "<module>#default");
//! const menu = toggle(a, b)                 in a function
//!   -> const menu = __tauler_call("menu", toggle, a, b)
//! const menu = toggle(a, b)                 at the top level of the file
//!   -> const menu = __tauler_shared_call("<module>#menu", toggle, a, b)
//! const { open } = toggle(a, b)             anywhere; an array pattern and `let` too
//!   -> const { open } = __tauler_destructured_call("<module>:<line>", toggle, a, b)
//! ```
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
//! ways no static check sees (an alias, `import * as t`, `import("tau" + "ler")`); one
//! check at run time covers all of them.
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
//! DECISION: the pass runs on every file a layout loads, a plain `.js` import included;
//! only `transform_source` is kept to [`is_script_file`] (`.jsx`, `.tsx`, `.ts`, `.mts`),
//! so a `.js` file cannot hold JSX. Reason: a function that declares a Store, and a
//! component above one, may live in a `.js` file, and the pass inserts only plain
//! JavaScript. Rejected: leaving `.js` out — a Store declared there had no identity and
//! threw. Test: `local_store_a_function_called_twice_in_one_component_is_told_apart_by_its_consts`
//! in `src/jsx.rs` (its helper is `lib/ticks.js`).
//!
//! DECISION: the two refusals above that are not about tauler's `store` (a call of a
//! `store` that resolves to nothing, and the name `globals`) are made only in a file
//! [`is_script_file`] names. A plain `.js` import is left to mean what it says: a
//! vendored file may name a global called `globals`, or call a global called `store`,
//! and it loads. Reason: both refusals exist to tell a person writing a layout what to
//! write instead, and nobody edits a vendored file to suit tauler. A plain `.js` file that imports
//! `store` from `"tauler"` still gets every Store rule: those are found by the import,
//! not by the extension. Tests: `a_plain_js_file_may_use_a_global_called_globals_or_store`
//! and `a_plain_js_file_that_imports_store_gets_every_store_rule` here, and
//! `local_store_a_plain_js_import_that_names_globals_and_calls_a_global_store_loads`
//! in `src/jsx.rs`. Accepted: a plain `.js` file a person wrote that forgets the import
//! gets QuickJS's "store is not defined" and not the message that says to import it.
//!
//! DECISION: every file is parsed, whatever it holds.
//! Reason: a file with no Store of its own may hold a component that is above one, or
//! call an imported function that declares one, and the pass reads one file at a time
//! ("Which calls get a name" in `local_store.rs`). Cost, `store_pass_cost` in
//! `src/jsx.rs`, unoptimised test build, a 6.3 kB component file: 0.90 ms per load with
//! no Store in it and 0.94 ms with one, against 1.37 ms for
//! `transform_source` on the same file. Paid once per file per load, never per Tick.
//! Rejected: skipping a file with no function and no call — almost no file is one.
//!
//! ## Local Stores (`local_store.rs` in this crate holds the design)
//!
//! DECISION: `<function>` in a local Store's declaration is the binding the function is
//! declared under (`function toggle`, `const toggle = () =>`, `default` for a default
//! export with no name), joined with `.` through every function around it
//! (`Outer.Inner`). Test: `names_a_local_store_by_the_functions_around_it_and_its_const`.
//!
//! DECISION: what the pass can tell is wrong from one file, it refuses at load
//! (`refuses_a_local_store_that_has_no_name_to_be_found_by`):
//! - a `store(...)` that is not the whole value of a `const` with one name;
//! - one in a function with no name, or with a function around it that has none (a
//!   handler, a callback, a method). Reason: two such functions in one function would
//!   write one declaration, and two components that are alternatives would then share
//!   a value with no error. Rejected: naming it after the nearest named function;
//! - one in an `async` function or a generator: after the first `await` or `yield` no
//!   component is rendering;
//! - a second declaration of one `<function>.<const>` in a file (the same `const` name
//!   in two blocks of one function, or in two functions of one name inside one
//!   function). Reason: as alternatives they would share one value with no error.
//!
//! Everything else about a local Store is found while rendering (rule 2 in
//! `local_store.rs`).
//!
//! DECISION: a function the file binds at its top level is tagged where it is bound,
//! in a statement after its own (`;__tauler_name(Menu, "<module>#Menu");`); a default
//! export that binds no name is wrapped. Reason: the statement form leaves the
//! function's own `.name` and its hoisting alone.
//!
//! DECISION: `export default function () {...}` (`async` and generators too) is a
//! declaration with no name, and it is given one (`__tauler_default`, with `_` added
//! until no binding and no global the file names has it) and tagged in a statement
//! after its own. Reason: JavaScript makes a function declaration before any module
//! code runs, so a module that imports it in a cycle may call it while it loads;
//! wrapped in a call it would exist only once its statement ran, and that call threw
//! "not initialized". The tag statement also sets the function's `.name` back to
//! `"default"`, which is what JavaScript gives it; until that statement runs `.name` is
//! the name the pass gave. Accepted: `Object` in that statement is the file's own if
//! the file binds one. A default export that is an expression (an arrow function, a
//! function in parentheses) is not made early by JavaScript either, and stays wrapped.
//! Tests: `tags_every_function_bound_at_the_top_level_of_a_file` here and
//! `an_unnamed_default_function_is_callable_from_an_import_cycle_while_loading` in
//! `src/jsx.rs`.
//!
//! A function made by a call
//! (`const Menu = memo(...)`), a function inside a function and a class get no tag;
//! `local_store.rs` says what happens to a Store beneath one. Test:
//! `tags_every_function_bound_at_the_top_level_of_a_file`.
//!
//! DECISION: which calls get a name (`local_store.rs` holds the reason and the rejected
//! alternative): every `const name = f(...)` whose callee is a plain identifier bound in the file — an
//! import, a function, a `const`, a parameter — in a function or at the top level of
//! the file. A member call, `new`, `f?.()`, `await f()`, a `let` with one name and a
//! call of a global are left as written and carry no name (a global cannot declare a
//! Store: every file that can is named by this pass). Test:
//! `gives_a_call_that_is_the_whole_value_of_a_const_its_name`.
//!
//! DECISION: a declaration with an object or an array pattern whose whole value is such
//! a call (`const { open } = toggle()`, `let [open] = toggle()`), anywhere in the file,
//! is rewritten the same way with `__tauler_destructured_call` and the place of the
//! pattern, `<module>:<line>`, where a name would be. Reason: a Store declared beneath it
//! must fail (`local_store.rs`), and the pass cannot know from one file whether
//! `toggle` declares one, so it cannot refuse the call at load. Test:
//! `marks_a_call_whose_value_is_destructured_with_its_file_and_line`.
//!
//! DECISION: the rewrite of a call is `__tauler_call("menu", toggle, a, b)`: the callee
//! and the arguments stay where they are, and only the `(` becomes `, `. Reason: the
//! caller still evaluates the callee and then each argument once, in the author's
//! order, before anything is pushed; a plain identifier called in a module has no
//! `this` to lose; spread, defaults, type arguments and casts are untouched; and the
//! wrapper's `finally` removes the name when the callee throws. Evidence:
//! `local_store_a_named_call_behaves_as_the_call_the_author_wrote` in `src/jsx.rs`
//! and `a_typescript_call_keeps_its_type_arguments_and_its_cast` here. Accepted: one
//! more frame in a stack trace, and in a page "fn is not a function" where the
//! browser would have named the callee (QuickJS says only "not a function" either
//! way). Rejected: `(push("menu"), pop(toggle(a, b)))` around the call — nothing pops
//! when `toggle` throws and the error is caught, and the arguments would run with the
//! name already pushed.

use std::collections::{HashMap, HashSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, CallExpression, Declaration, ExportDefaultDeclarationKind, Expression,
    Function, IdentifierReference, ImportDeclarationSpecifier, Statement, VariableDeclaration,
    VariableDeclarator,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::{ScopeFlags, Scoping, SemanticBuilder, SymbolId};
use oxc_span::{GetSpan, SourceType};

/// The name `"tauler"` exports it under, and the name a file that forgot the import
/// calls.
const STORE: &str = "store";

/// What a layout file kept state in before Stores.
const GLOBALS: &str = "globals";

/// The module a layout file imports `store` from (`tauler-core/src/globals.rs`).
const TAULER_MODULE: &str = crate::globals::TAULER_MODULE;

/// The global that tags a function with `<module>#<function>` and answers the function
/// (`local_store.rs`).
const NAME_FN: &str = "__tauler_name";

/// The global that keeps a `const`'s name on the stack while the call it is the whole
/// value of runs (`local_store.rs`).
const CALL_FN: &str = "__tauler_call";

/// The global a `store` call inside a function is rewritten to call (`local_store.rs`).
const LOCAL_STORE_FN: &str = "__tauler_local_store";

/// [`CALL_FN`] for a `const` at the top level of a file, whose name is
/// `<module>#<const>`.
const SHARED_CALL_FN: &str = "__tauler_shared_call";

/// The global that runs a call whose value is destructured, and remembers where the
/// call is (`<module>:<line>`) while it runs (`local_store.rs`).
const DESTRUCTURED_CALL_FN: &str = "__tauler_destructured_call";

/// Whether `path` is a file `optative_script::jsx::transform_source` is run on: the
/// extensions a layout file, a component file or a Package file is written in. Any
/// other file a layout imports is plain JavaScript (`.js`).
pub fn is_script_file(path: &str) -> bool {
    matches!(
        path.rsplit_once('.').map(|(_, extension)| extension),
        Some("jsx" | "tsx" | "ts" | "mts")
    )
}

/// A `store(` the pass refuses, with the 1-based line it is on.
#[derive(Debug, PartialEq, Eq)]
pub struct StoreError {
    pub line: usize,
    pub message: &'static str,
}

const NOT_A_CONST: &str = "store() must be the whole value of a `const`, as in \
     `const name = store({...})`, at the top level of a file (shared) or directly in a \
     function (local to each component instance): its name is what finds the state again";
const ONE_ARGUMENT: &str =
    "store() takes exactly one argument, the default value; tauler adds the identity";
const NOT_IMPORTED: &str =
    "store is not a global: add `import { store } from \"tauler\"` to this file";
const IN_UNNAMED_FUNCTION: &str = "store() inside a function needs every function around \
     it to have a name, which is part of what finds the state again: write \
     `function name() {...}` or `const name = () => {...}` and declare the Store there. \
     A handler, a callback or a method written inline has no name";
const IN_ASYNC_FUNCTION: &str = "store() cannot be declared in an `async` function or a \
     generator: after the first `await` or `yield` no component is rendering";
const SAME_NAME_TWICE: &str = "this function already declares a Store under this name \
     (a second `const` of one name, or two functions of one name inside one function): \
     give each its own name, which is what finds the state again";

const GLOBALS_REMOVED: &str = "`globals` was removed: keep state in a Store instead — \
     `import { store } from \"tauler\"`, then `const name = store({...})` at the top \
     level of the file, and read and write `name.key`";

/// Names every Store `source` declares, tags every function bound at its top level,
/// gives every `const name = f(...)` its call name and marks every destructured call
/// (the module docs say how).
///
/// Refuses a file that calls tauler's `store` any other way; and, in a file
/// [`is_script_file`] names, a call of a `store` that was not imported and the name
/// `globals`.
///
/// `path` picks the syntax (`.ts`, `.tsx`, anything else is JSX) and answers
/// [`is_script_file`]. A source that does
/// not parse is returned unchanged: `transform_source` and QuickJS already report that,
/// and reporting it twice in two formats helps nobody.
pub fn rewrite(source: &str, path: &str, module: &str, token: &str) -> Result<String, StoreError> {
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::jsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.diagnostics.has_errors() {
        return Ok(source.to_string());
    }
    let program = parsed.program;
    let semantic = SemanticBuilder::new().build(&program).semantic;
    let mut pass = Pass {
        file: File {
            scoping: semantic.scoping(),
            stores: imported_stores(&program.body),
            script: is_script_file(path),
        },
        source,
        module,
        token,
        edits: Vec::new(),
        refusals: Refusals::default(),
        named_calls: Vec::new(),
        top_level: HashSet::new(),
        function_names: HashMap::new(),
        functions: Vec::new(),
        local_declarations: HashSet::new(),
    };
    for statement in &program.body {
        pass.top_level_statement(statement);
    }
    pass.visit_program(&program);
    if let Some((offset, message)) = pass.refusals.first {
        return Err(StoreError {
            line: line_of(source, offset as usize),
            message,
        });
    }

    let mut edits = pass.edits;
    // Stable: two edits at one offset keep the order they were queued in.
    edits.sort_by_key(|edit| edit.start);
    let mut out = String::with_capacity(source.len() + edits.len() * 48);
    let mut at = 0;
    for edit in edits {
        out.push_str(&source[at..edit.start]);
        out.push_str(&edit.text);
        at = edit.end;
    }
    out.push_str(&source[at..]);
    Ok(out)
}

/// `source[start..end]` becomes `text`. Never spans a line break, so every line number
/// stays as the author wrote it.
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

/// What the pass knows about one file: its scopes, and the bindings that are tauler's
/// `store`.
struct File<'s> {
    scoping: &'s Scoping,
    stores: Vec<SymbolId>,
    /// [`is_script_file`]: whether the two refusals that are not about tauler's `store`
    /// apply.
    script: bool,
}

/// What a call's callee is, as far as the pass cares.
#[derive(PartialEq)]
enum Callee {
    /// The binding `import { store } from "tauler"` made.
    Store,
    /// `store`, resolving to nothing in the file.
    Unimported,
    /// A plain identifier that resolves to some other binding of the file: an import, a
    /// function, a `const`, a parameter.
    Bound,
    /// A member call, a call of a global, anything else.
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
            Some(_) => Callee::Bound,
            None if callee.name == STORE && self.script => Callee::Unimported,
            None => Callee::Other,
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

/// One function the walk is inside.
struct FunctionFrame {
    /// `toggle`, or `Outer.Inner` for a function in a function. `None` when this
    /// function or one around it has no name.
    path: Option<String>,
    /// `async` or a generator.
    suspends: bool,
}

fn json(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

/// The function an expression is, through parentheses and TypeScript casts: its span
/// start, which is what [`Pass::function_names`] is keyed by.
fn function_start(expression: &Expression<'_>) -> Option<u32> {
    match expression.get_inner_expression() {
        Expression::ArrowFunctionExpression(arrow) => Some(arrow.span.start),
        Expression::FunctionExpression(function) => Some(function.span.start),
        _ => None,
    }
}

/// One walk over one file.
struct Pass<'s> {
    file: File<'s>,
    source: &'s str,
    module: &'s str,
    token: &'s str,
    edits: Vec<Edit>,
    refusals: Refusals,
    /// The span starts of the `store(...)` calls the pass named or refused by name.
    named_calls: Vec<u32>,
    /// The span starts of the `const`s that are statements of the file itself.
    top_level: HashSet<u32>,
    /// The binding a function expression is the whole value of, by the function's span
    /// start; `default` for an unnamed default export.
    function_names: HashMap<u32, String>,
    /// The functions around the node being visited, outermost first.
    functions: Vec<FunctionFrame>,
    /// Every `<function>.<const>` a local Store was declared under.
    local_declarations: HashSet<String>,
}

impl Pass<'_> {
    fn insert(&mut self, at: u32, text: String) {
        self.edits.push(Edit {
            start: at as usize,
            end: at as usize,
            text,
        });
    }

    /// `;__tauler_name(X, "<module>#X");` after the statement that binds `X`.
    fn tag_after(&mut self, statement_end: u32, name: &str) {
        let id = json(&format!("{}#{name}", self.module));
        self.insert(statement_end, format!(";{NAME_FN}({name}, {id});"));
    }

    /// `__tauler_name(<function>, "<module>#default");` around a default export that
    /// binds no name and is an expression. `function` is where the function itself starts, which parentheses
    /// may put after `start`.
    fn tag_around(&mut self, start: u32, end: u32, function: u32) {
        let id = json(&format!("{}#default", self.module));
        self.insert(start, format!("{NAME_FN}("));
        self.insert(end, format!(", {id});"));
        self.function_names.insert(function, "default".to_string());
    }

    /// Names `export default function () {...}` and tags it after its statement, so
    /// that it stays a declaration (the `DECISION` in the module docs).
    fn tag_default_declaration(&mut self, function: &Function<'_>, statement_end: u32) {
        let scoping = self.file.scoping;
        let mut name = "__tauler_default".to_string();
        while scoping.symbol_names().any(|used| used == name)
            || scoping
                .root_unresolved_references()
                .keys()
                .any(|used| used.as_str() == name)
        {
            name.push('_');
        }
        // Where the name goes: before the type parameters or the `(`.
        let at = function
            .type_parameters
            .as_ref()
            .map_or(function.params.span.start, |types| types.span.start);
        let after_space = self.source[..at as usize].ends_with(char::is_whitespace);
        let space = if after_space { "" } else { " " };
        self.insert(at, format!("{space}{name}"));
        let id = json(&format!("{}#default", self.module));
        self.insert(
            statement_end,
            format!(
                ";{NAME_FN}(Object.defineProperty({name}, \"name\", {{ value: \"default\" }}), {id});"
            ),
        );
        self.function_names
            .insert(function.span.start, "default".to_string());
    }

    /// Tags the functions one statement of the file binds, and notes its `const`s.
    fn top_level_statement(&mut self, statement: &Statement<'_>) {
        let end = statement.span().end;
        match statement {
            Statement::VariableDeclaration(declaration) => self.top_level_const(declaration, end),
            Statement::FunctionDeclaration(function) => self.top_level_function(function, end),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => {
                    self.top_level_const(declaration, end);
                }
                Declaration::FunctionDeclaration(function) => {
                    self.top_level_function(function, end);
                }
                _ => {}
            },
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    if function.id.is_some() {
                        self.top_level_function(function, end);
                    } else if function.body.is_some() {
                        self.tag_default_declaration(function, end);
                    }
                }
                ExportDefaultDeclarationKind::ClassDeclaration(_)
                | ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {}
                expression => {
                    let expression = expression.to_expression();
                    if let Some(function) = function_start(expression) {
                        let span = expression.span();
                        self.tag_around(span.start, span.end, function);
                    }
                }
            },
            _ => {}
        }
    }

    fn top_level_function(&mut self, function: &Function<'_>, statement_end: u32) {
        if let (Some(id), true) = (&function.id, function.body.is_some()) {
            self.tag_after(statement_end, &id.name);
        }
    }

    fn top_level_const(&mut self, declaration: &VariableDeclaration<'_>, statement_end: u32) {
        self.top_level.insert(declaration.span.start);
        for declarator in &declaration.declarations {
            let is_function = declarator.init.as_ref().and_then(function_start).is_some();
            if let (true, Some(name)) = (is_function, declarator.id.get_binding_identifier()) {
                self.tag_after(statement_end, &name.name);
            }
        }
    }

    fn enter_function(&mut self, start: u32, own_name: Option<&str>, suspends: bool) {
        let name = self
            .function_names
            .get(&start)
            .map(String::as_str)
            .or(own_name);
        let path = match (self.functions.last(), name) {
            (None, Some(name)) => Some(name.to_string()),
            (Some(outer), Some(name)) => outer.path.as_ref().map(|path| format!("{path}.{name}")),
            (_, None) => None,
        };
        self.functions.push(FunctionFrame { path, suspends });
    }

    /// Names one `const X = store(default)`, or refuses it by what is wrong with it.
    /// Anything it leaves alone is refused by `visit_call_expression` as not a `const`.
    fn declare_store(
        &mut self,
        declaration: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        call: &CallExpression<'_>,
    ) {
        if !declaration.kind.is_const() {
            return;
        }
        // `const { open } = store(...)` has no one name.
        let Some(name) = declarator.id.get_binding_identifier() else {
            return;
        };
        let identity = match self.functions.last() {
            None if self.top_level.contains(&declaration.span.start) => {
                format!("{}#{}", self.module, name.name)
            }
            // In a block or a loop of the file itself: shared, and reached how often?
            None => return,
            Some(function) => {
                let Some(path) = &function.path else {
                    self.named_calls.push(call.span.start);
                    self.refusals.refuse(call.span.start, IN_UNNAMED_FUNCTION);
                    return;
                };
                if function.suspends {
                    self.named_calls.push(call.span.start);
                    self.refusals.refuse(call.span.start, IN_ASYNC_FUNCTION);
                    return;
                }
                let local = format!("{path}.{}", name.name);
                let identity = format!("{}#{local}", self.module);
                if !self.local_declarations.insert(local) {
                    self.named_calls.push(call.span.start);
                    self.refusals.refuse(call.span.start, SAME_NAME_TWICE);
                    return;
                }
                identity
            }
        };
        self.named_calls.push(call.span.start);
        if call.arguments.len() != 1 {
            self.refusals.refuse(call.span.start, ONE_ARGUMENT);
            return;
        }
        if !self.functions.is_empty() {
            let callee = call.callee.span();
            self.edits.push(Edit {
                start: callee.start as usize,
                end: callee.end as usize,
                text: LOCAL_STORE_FN.to_string(),
            });
        }
        // After the argument, not before `)`: a trailing comma stays valid.
        self.insert(
            call.arguments[0].span().end,
            format!(", {}, {}", json(&identity), json(self.token)),
        );
    }

    /// `const menu = toggle(a, b)` becomes `const menu = __tauler_call("menu", toggle, a, b)`;
    /// a destructured call gets [`DESTRUCTURED_CALL_FN`] and its place the same way.
    fn name_call(
        &mut self,
        declaration: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        call: &CallExpression<'_>,
    ) {
        let (helper, name) = match declarator.id.get_binding_identifier() {
            // `const { open } = toggle()` and `const [open] = toggle()`: no one name, so
            // the marker is where the pattern is, for the message.
            None => {
                let line = line_of(self.source, declarator.span.start as usize);
                (DESTRUCTURED_CALL_FN, format!("{}:{line}", self.module))
            }
            Some(_) if !declaration.kind.is_const() => return,
            Some(name) if self.functions.is_empty() => {
                if !self.top_level.contains(&declaration.span.start) {
                    return;
                }
                // No component is around this call, so the module is part of the name
                // and what the call declares is shared (`__tauler_shared_call`).
                (SHARED_CALL_FN, format!("{}#{}", self.module, name.name))
            }
            Some(name) => (CALL_FN, name.name.to_string()),
        };
        // The `(` that opens the arguments: after the callee and its type arguments,
        // with only blanks between. A comment there leaves the call unnamed.
        let after_callee =
            call.type_arguments
                .as_ref()
                .map_or(call.callee.span().end, |types| types.span.end) as usize;
        let blanks = self.source[after_callee..]
            .bytes()
            .take_while(u8::is_ascii_whitespace)
            .count();
        let paren = after_callee + blanks;
        if self.source.as_bytes().get(paren) != Some(&b'(') {
            return;
        }
        self.insert(call.span.start, format!("{helper}({}, ", json(&name)));
        self.edits.push(Edit {
            start: paren,
            end: paren + 1,
            text: ", ".to_string(),
        });
    }
}

impl<'a> Visit<'a> for Pass<'_> {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        if function.body.is_none() {
            return walk::walk_function(self, function, flags);
        }
        self.enter_function(
            function.span.start,
            function.id.as_ref().map(|id| id.name.as_str()),
            function.r#async || function.generator,
        );
        walk::walk_function(self, function, flags);
        self.functions.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.enter_function(arrow.span.start, None, arrow.r#async);
        walk::walk_arrow_function_expression(self, arrow);
        self.functions.pop();
    }

    fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
        for declarator in &declaration.declarations {
            let Some(init) = &declarator.init else {
                continue;
            };
            if let (Some(start), Some(name)) =
                (function_start(init), declarator.id.get_binding_identifier())
            {
                self.function_names.insert(start, name.name.to_string());
            }
            // `store({...}) as Desk` and `(store({...}))` are still the whole value.
            let Expression::CallExpression(call) = init.get_inner_expression() else {
                continue;
            };
            match self.file.callee(call) {
                Callee::Store => self.declare_store(declaration, declarator, call),
                Callee::Bound => self.name_call(declaration, declarator, call),
                Callee::Unimported | Callee::Other => {}
            }
        }
        walk::walk_variable_declaration(self, declaration);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        match self.file.callee(call) {
            Callee::Store if !self.named_calls.contains(&call.span.start) => {
                self.refusals.refuse(call.span.start, NOT_A_CONST);
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
        if self.file.script && name.name == GLOBALS && unresolved() {
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
            assert_eq!((err.line, err.message), (line + 1, NOT_A_CONST), "{source}");
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

    /// The two refusals that are not about tauler's `store` are for the files a
    /// person writes as a layout: a plain `.js` file may name a global `globals` and
    /// call a global `store`.
    #[test]
    fn a_plain_js_file_may_use_a_global_called_globals_or_store() {
        for source in [
            "export const n = globals.count ?? 0;",
            "export const a = typeof store === 'function' ? store({}) : null;",
            "const a = store({});",
        ] {
            for path in ["vendor/lib.js", "vendor/lib.mjs", "vendor/lib.cjs"] {
                assert_eq!(
                    rewrite(source, path, path, "T").unwrap(),
                    source,
                    "{path}: {source}"
                );
            }
            for path in ["a.jsx", "a.tsx", "a.ts", "a.mts"] {
                assert!(
                    rewrite(source, path, path, "T").is_err(),
                    "{path}: {source}"
                );
            }
        }
    }

    /// A plain `.js` file that imports tauler's `store` gets every Store rule.
    #[test]
    fn a_plain_js_file_that_imports_store_gets_every_store_rule() {
        let js = |source: &str| rewrite(&format!("{IMPORT}{source}"), "lib/t.js", "lib/t.js", "T");
        assert_eq!(
            js("const desk = store({});\nexport function toggle() { const s = store({}); }").unwrap(),
            format!(
                "{IMPORT}const desk = store({{}}, \"lib/t.js#desk\", \"T\");\nexport function toggle() {{ const s = __tauler_local_store({{}}, \"lib/t.js#toggle.s\", \"T\"); }};__tauler_name(toggle, \"lib/t.js#toggle\");"
            )
        );
        for (source, message) in [
            ("function f() { return store({}); }", NOT_A_CONST),
            ("const a = store({}, \"mine\");", ONE_ARGUMENT),
            (
                "const o = { m() { const s = store({}); } };",
                IN_UNNAMED_FUNCTION,
            ),
            (
                "async function f() { const s = store({}); }",
                IN_ASYNC_FUNCTION,
            ),
        ] {
            assert_eq!(js(source).expect_err(source).message, message, "{source}");
        }
    }

    #[test]
    fn leaves_a_globals_that_is_the_files_own_alone() {
        for source in [
            "const globals = { n: 1 };\nexport const n = globals.n;",
            "export default (globals) => globals.n;",
            "const o = { globals: 1 };\nexport const n = o.globals;",
            "// <Panel> and <Module> are globals\nexport const n = 1;",
        ] {
            assert!(pass(source).is_ok(), "{source}");
        }
    }

    #[test]
    fn leaves_a_store_that_is_not_taulers_alone() {
        for source in [
            "const f = [(store) => store({})];",
            "import { store } from \"./mine\";\nconst a = [store({})];",
            "const store = [(v) => v];\nconst a = [store[0]({})];",
        ] {
            assert_eq!(pass(source).unwrap(), source, "{source}");
        }
    }

    #[test]
    fn a_shadowed_store_is_left_alone_beside_taulers() {
        let source = format!("{IMPORT}const f = [(store) => store({{}})];\nconst a = store({{}});");
        assert_eq!(
            pass(&source).unwrap(),
            format!("{IMPORT}const f = [(store) => store({{}})];\nconst a = store({{}}, \"a.jsx#a\", \"T\");")
        );
    }

    // ---- Local Stores ----

    /// The identity the pass wrote for the one Store `source` declares.
    fn declaration(source: &str) -> String {
        let out = pass(&format!("{IMPORT}{source}")).expect(source);
        let end = out
            .find("\", \"T\")")
            .unwrap_or_else(|| panic!("no Store in {out}"));
        let start = out[..end].rfind('"').unwrap() + 1;
        out[start..end].to_string()
    }

    #[test]
    fn names_a_local_store_by_the_functions_around_it_and_its_const() {
        for (source, identity) in [
            (
                "function toggle() { const s = store({}); }",
                "a.jsx#toggle.s",
            ),
            (
                "export function Menu() { const s = store({}); }",
                "a.jsx#Menu.s",
            ),
            (
                "const toggle = () => { const s = store({}); };",
                "a.jsx#toggle.s",
            ),
            (
                "export const toggle = function () { const s = store({}); };",
                "a.jsx#toggle.s",
            ),
            (
                "export default function () { const s = store({}); }",
                "a.jsx#default.s",
            ),
            (
                "export default () => { const s = store({}); };",
                "a.jsx#default.s",
            ),
            (
                "export default function Bar() { const s = store({}); }",
                "a.jsx#Bar.s",
            ),
            (
                "function Outer() { function Inner() { const s = store({}); } }",
                "a.jsx#Outer.Inner.s",
            ),
            (
                "function Outer() { const inner = () => { const s = store({}); }; }",
                "a.jsx#Outer.inner.s",
            ),
            // A condition or a loop around the declaration is allowed (rule 1).
            (
                "function f(on) { if (on) { for (;;) { const s = store({}); } } }",
                "a.jsx#f.s",
            ),
            (
                "function f() { const s = (store({}) as Desk); }",
                "a.jsx#f.s",
            ),
        ] {
            let path = if source.contains(" as ") {
                "a.tsx"
            } else {
                "a.jsx"
            };
            let out = rewrite(&format!("{IMPORT}{source}"), path, "a.jsx", "T").expect(source);
            assert!(
                out.contains(&format!("\"{identity}\", \"T\")")),
                "{source}: {out}"
            );
        }
    }

    #[test]
    fn refuses_a_local_store_that_has_no_name_to_be_found_by() {
        for (source, message) in [
            ("function f() { return store({}); }", NOT_A_CONST),
            ("function f() { let s = store({}); }", NOT_A_CONST),
            ("function f() { const { a } = store({ a: 1 }); }", NOT_A_CONST),
            ("function f() { const s = [store({})]; }", NOT_A_CONST),
            // A handler, a callback and a method are functions with no name.
            (
                "function F() { return <div on_click={() => { const s = store({}); }} />; }",
                IN_UNNAMED_FUNCTION,
            ),
            (
                "function f(rows) { rows.map((row) => { const s = store({}); }); }",
                IN_UNNAMED_FUNCTION,
            ),
            ("const o = { m() { const s = store({}); } };", IN_UNNAMED_FUNCTION),
            ("class C { m() { const s = store({}); } }", IN_UNNAMED_FUNCTION),
            // A named function inside one with no name has no whole name either.
            (
                "const all = [() => { function f() { const s = store({}); } }];",
                IN_UNNAMED_FUNCTION,
            ),
            ("async function f() { const s = store({}); }", IN_ASYNC_FUNCTION),
            ("function* f() { const s = store({}); }", IN_ASYNC_FUNCTION),
            ("const f = async () => { const s = store({}); };", IN_ASYNC_FUNCTION),
            (
                "function f(on) { if (on) { const s = store({}); } else { const s = store({}); } }",
                SAME_NAME_TWICE,
            ),
            (
                "function f(on) { if (on) { const g = () => { const s = store({}); }; } else { const g = () => { const s = store({}); }; } }",
                SAME_NAME_TWICE,
            ),
            ("function f() { const s = store({}, \"mine\"); }", ONE_ARGUMENT),
        ] {
            let err = pass(&format!("{IMPORT}{source}")).expect_err(source);
            assert_eq!(err.message, message, "{source}");
        }
    }

    /// A function that is not `async` may declare a Store although one around it is:
    /// only the function that runs the declaration has to run it while rendering.
    #[test]
    fn a_plain_function_inside_an_async_one_may_declare_a_local_store() {
        assert_eq!(
            declaration("async function load() { function f() { const s = store({}); } }"),
            "a.jsx#load.f.s"
        );
    }

    #[test]
    fn tags_every_function_bound_at_the_top_level_of_a_file() {
        for (source, expected) in [
            (
                "function Menu() {}",
                "function Menu() {};__tauler_name(Menu, \"a.jsx#Menu\");",
            ),
            (
                "export function Menu() {}\nconst n = 1;",
                "export function Menu() {};__tauler_name(Menu, \"a.jsx#Menu\");\nconst n = 1;",
            ),
            (
                "const Menu = () => null;",
                "const Menu = () => null;;__tauler_name(Menu, \"a.jsx#Menu\");",
            ),
            (
                "export const A = function () {}, b = 1, C = (() => 2);",
                "export const A = function () {}, b = 1, C = (() => 2);;__tauler_name(A, \"a.jsx#A\");;__tauler_name(C, \"a.jsx#C\");",
            ),
            (
                "export default function Menu() {}",
                "export default function Menu() {};__tauler_name(Menu, \"a.jsx#Menu\");",
            ),
            // A declaration stays a declaration, so it is still hoisted.
            (
                "export default function () {}",
                "export default function __tauler_default() {};__tauler_name(Object.defineProperty(__tauler_default, \"name\", { value: \"default\" }), \"a.jsx#default\");",
            ),
            (
                "export default async function* () {}",
                "export default async function* __tauler_default() {};__tauler_name(Object.defineProperty(__tauler_default, \"name\", { value: \"default\" }), \"a.jsx#default\");",
            ),
            // The name the pass gives it is one the file does not use.
            (
                "export default function () { return __tauler_default + __tauler_default_; }",
                "export default function __tauler_default__() { return __tauler_default + __tauler_default_; };__tauler_name(Object.defineProperty(__tauler_default__, \"name\", { value: \"default\" }), \"a.jsx#default\");",
            ),
            // An expression is not hoisted, and stays wrapped.
            (
                "export default (function () {});",
                "export default __tauler_name((function () {}), \"a.jsx#default\");;",
            ),
            (
                "export default () => null;",
                "export default __tauler_name(() => null, \"a.jsx#default\");;",
            ),
            // Not a function the file binds: nothing to tag.
            ("const Menu = memo(() => null);", "const Menu = memo(() => null);"),
            ("export default Menu;", "export default Menu;"),
            (
                "function Outer() { function Inner() {} }",
                "function Outer() { function Inner() {} };__tauler_name(Outer, \"a.jsx#Outer\");",
            ),
        ] {
            assert_eq!(pass(source).unwrap(), expected, "{source}");
        }
    }

    #[test]
    fn gives_a_call_that_is_the_whole_value_of_a_const_its_name() {
        let wrap = |body: &str| {
            format!("import {{ toggle }} from './t';\nconst F = [() => {{ {body} }}];")
        };
        for (body, expected) in [
            (
                "const menu = toggle(a, b);",
                "const menu = __tauler_call(\"menu\", toggle, a, b);",
            ),
            (
                "const menu = toggle();",
                "const menu = __tauler_call(\"menu\", toggle, );",
            ),
            (
                "const menu = toggle (...rest);",
                "const menu = __tauler_call(\"menu\", toggle , ...rest);",
            ),
            (
                "const a = toggle(1), b = toggle(2);",
                "const a = __tauler_call(\"a\", toggle, 1), b = __tauler_call(\"b\", toggle, 2);",
            ),
            (
                "const menu = (toggle(a));",
                "const menu = (__tauler_call(\"menu\", toggle, a));",
            ),
            // Only the call that is the whole value: the one in its arguments has no name.
            (
                "const menu = toggle(toggle(1));",
                "const menu = __tauler_call(\"menu\", toggle, toggle(1));",
            ),
            (
                "for (const x of xs) { const row = toggle(x); }",
                "for (const x of xs) { const row = __tauler_call(\"row\", toggle, x); }",
            ),
        ] {
            assert_eq!(pass(&wrap(body)).unwrap(), wrap(expected), "{body}");
        }
        // What carries no name is left exactly as it was written.
        for body in [
            "toggle();",
            "return toggle();",
            "let menu = toggle();",
            "const menu = await toggle();",
            "const menu = lib.toggle();",
            "const menu = new toggle();",
            "const menu = toggle?.();",
            "const menu = toggle /* why */ ();",
            // A global cannot declare a Store: the pass names every file that can.
            "const text = useStringStream('x');",
            "const n = Number(text);",
            "const v = eval('1');",
        ] {
            assert_eq!(pass(&wrap(body)).unwrap(), wrap(body), "{body}");
        }
    }

    /// A destructured call has no one name, so it gets a marker: the file and the line.
    #[test]
    fn marks_a_call_whose_value_is_destructured_with_its_file_and_line() {
        let wrap = |body: &str| {
            format!("import {{ toggle }} from './t';\nconst F = [() => {{\n {body} }}];")
        };
        for (body, expected) in [
            (
                "const { open, flip } = toggle(a, b);",
                "const { open, flip } = __tauler_destructured_call(\"a.jsx:3\", toggle, a, b);",
            ),
            (
                "const [open] = toggle();",
                "const [open] = __tauler_destructured_call(\"a.jsx:3\", toggle, );",
            ),
            (
                "let { open } = (toggle(...rest));",
                "let { open } = (__tauler_destructured_call(\"a.jsx:3\", toggle, ...rest));",
            ),
            (
                "const menu = toggle(), { open } = toggle(1);",
                "const menu = __tauler_call(\"menu\", toggle, ), { open } = __tauler_destructured_call(\"a.jsx:3\", toggle, 1);",
            ),
            // The line is the pattern's, not the call's.
            (
                "const { open } =\n toggle();",
                "const { open } =\n __tauler_destructured_call(\"a.jsx:3\", toggle, );",
            ),
        ] {
            assert_eq!(pass(&wrap(body)).unwrap(), wrap(expected), "{body}");
        }
        // The same at the top level of a file, in a block or not.
        assert_eq!(
            pass("import { toggle } from './t';\nexport const { open } = toggle();\n{ const [b] = toggle(); }")
                .unwrap(),
            "import { toggle } from './t';\nexport const { open } = __tauler_destructured_call(\"a.jsx:2\", toggle, );\n{ const [b] = __tauler_destructured_call(\"a.jsx:3\", toggle, ); }"
        );
        // What the pass cannot follow, and a global, are left exactly as written.
        for body in [
            "const { open } = await toggle();",
            "const { open } = lib.toggle();",
            "const { open } = toggle?.();",
            "const { open } = toggle().inner;",
            "const [first] = useStringStream('x');",
            "for (const { open } of toggle()) {}",
        ] {
            assert_eq!(pass(&wrap(body)).unwrap(), wrap(body), "{body}");
        }
    }

    #[test]
    fn a_typescript_call_keeps_its_type_arguments_and_its_cast() {
        let out = rewrite(
            "import { toggle } from './t';\nconst F = [() => { const menu = toggle<Menu>(a) as Menu; }];",
            "a.ts",
            "a.ts",
            "T",
        )
        .unwrap();
        assert_eq!(
            out,
            "import { toggle } from './t';\nconst F = [() => { const menu = __tauler_call(\"menu\", toggle<Menu>, a) as Menu; }];"
        );
    }

    /// At the top level of a file no component is around the call, so the module is
    /// part of the name.
    #[test]
    fn a_call_at_the_top_level_of_a_file_is_named_after_the_module_too() {
        assert_eq!(
            pass("import { toggle } from './t';\nexport const menu = toggle();\n{ const b = toggle(); }")
                .unwrap(),
            "import { toggle } from './t';\nexport const menu = __tauler_shared_call(\"a.jsx#menu\", toggle, );\n{ const b = toggle(); }"
        );
    }

    #[test]
    fn every_line_stays_where_the_author_wrote_it() {
        let source = format!(
            "{IMPORT}import {{ toggle }} from './t';\nexport function Menu() {{\n  const s = store({{\n    open: false,\n  }});\n  const menu = toggle(\n    1,\n  );\n  return null;\n}}\nexport default () =>\n  null;\n"
        );
        let out = pass(&source).unwrap();
        assert_ne!(out, source);
        assert_eq!(out.lines().count(), source.lines().count());
    }
}
