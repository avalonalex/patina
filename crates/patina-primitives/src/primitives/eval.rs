//! Eval primitives (R7RS Section 6.12 - scheme eval, scheme load)
//!
//! Implements:
//! - `environment` - Create an environment from import sets
//! - `eval` - Evaluate an expression in a given environment
//! - `load` - Load and evaluate a file in the interaction environment
//! - `interaction-environment` - Return the interaction environment
//! - `null-environment` - R5RS environment with only syntactic keywords
//! - `scheme-report-environment` - R5RS environment with all bindings

use crate::apply_context::ApplyContext;
use crate::registry::{PrimitiveFn, PrimitiveRegistry, Step, done_with_result};
use patina_core::port::out_of_descriptors;
use patina_core::{CollectKind, CoreExpr, CoreExprKind, Cx, Value, core_syntax::CoreForm};
use patina_frontend::{Desugarer, LibraryDefinition};
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::HasDiagnostic;
use patina_runtime::environment::Environment;
use patina_runtime::{Diagnostic, DiagnosticKind};
use std::collections::HashSet;
use std::rc::Rc;

/// Runtime arguments can contain cycles even though source import declarations
/// cannot. Check the pair graph before invoking the recursive import parser;
/// shared, acyclic sublists are valid and must not be mistaken for cycles.
fn check_import_datum<'gc>(cx: &Cx<'gc>, form: Value<'gc>) -> Result<(), EvalError> {
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    let mut work = vec![(form, false)];
    while let Some((value, leaving)) = work.pop() {
        if leaving {
            active.remove(&value.raw());
            complete.insert(value.raw());
            continue;
        }
        if complete.contains(&value.raw()) {
            continue;
        }
        if let Some((car, cdr)) = cx.try_pair(value) {
            if !active.insert(value.raw()) {
                return Err(EvalError::InvalidSyntax(
                    "environment: cyclic import set".to_string(),
                ));
            }
            work.push((value, true));
            work.push((cdr, false));
            work.push((car, false));
        }
    }
    Ok(())
}

/// (environment import-set ...) → immutable environment-specifier
fn primitive_environment<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    let import_sets = args
        .iter()
        .map(|&arg| {
            check_import_datum(cx, arg)?;
            LibraryDefinition::parse_import_set_tagged(arg, cx).map_err(|e| {
                EvalError::InvalidSyntax(format!("environment: {e}"))
                    .with_diagnostic(e.diagnostic())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let env = Rc::new(cx.new_namespace());
    for set in &import_sets {
        // Loading stays behind ApplyContext so a Scheme initializer can
        // transfer control normally. Selection never copies binding values.
        #[expect(
            clippy::disallowed_methods,
            reason = "holds `env`, the environment being built, across each load. Its bindings so \
                      far are imports only: shared locations the registered libraries own, or, \
                      for an export that is not a plain binding, a copy of a value the library's \
                      export table holds, both traced through the library registry (#406). So it \
                      holds no value of its own, and nothing here needs the load deferred"
        )]
        let library = ctx.load_scheme_library(set.library_name())?;
        for binding in set.resolve_bindings(library.export_names()) {
            let (name, export) = binding.map_err(patina_runtime::LibraryError::into_eval_error)?;
            library.import_into(&env, name, &export);
        }
    }
    Ok(cx.alloc_environment_specifier(env, false))
}

/// Reject core definitions before desugaring: define-syntax installs its
/// transformer during expansion. Follow the binding so aliases cannot bypass
/// the immutable-environment check, and a procedure named define still works.
fn is_definition<'gc>(cx: &Cx<'gc>, form: Value<'gc>, env: &Environment) -> bool {
    if !form.is_pair() {
        return false;
    }
    let binding = cx
        .get_symbol_or_identifier_name(cx.car(form))
        .and_then(|name| cx.lookup(env, &name));
    let core = binding.and_then(|value| cx.get_core_syntax(value));
    matches!(core, Some(CoreForm::Define | CoreForm::DefineSyntax))
}

/// Macros such as define-values can expand to a sequence of definitions.
/// Only inspect the outer body; definitions local to a lambda remain legal.
fn has_top_level_definition(expr: &CoreExpr) -> bool {
    let mut work = vec![expr];
    while let Some(expr) = work.pop() {
        match &expr.kind {
            CoreExprKind::Define { .. } => return true,
            CoreExprKind::Begin(body) => work.extend(body),
            _ => {}
        }
    }
    false
}

/// (eval expr-or-def environment-specifier) → values
///
/// Evaluates an expression in the specified environment.
/// If expr-or-def is a definition, the environment must be mutable.
///
/// The machine evaluates it ([`Step::Eval`]), as its own code rather than on
/// a run of its own from Rust, so a continuation captured in it carries the
/// caller: re-entered after `eval` has returned, it returns from `eval`
/// again (#477).
fn primitive_eval<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "eval expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let Some((env, mutable)) = cx.get_environment_specifier(args[1]) else {
        return Err(EvalError::TypeError(format!(
            "eval: expected environment, got {}",
            cx.type_name(args[1])
        )));
    };

    // Check if this is a definition
    if is_definition(cx, args[0], &env) && !mutable {
        return Err(EvalError::InvalidSyntax(
            "eval: cannot define in immutable environment".to_string(),
        ));
    }

    // Only immutable environments need a preliminary definition check. In
    // the mutable case expansion belongs to the machine: an import earlier
    // in a spliced form can provide syntax needed to expand its later forms
    // (#435), and expanding twice can also install a transformer twice.
    if !mutable {
        let desugarer = Desugarer::with_env(env.clone()).with_fs(cx.instance().fs());
        // Its literals are never read: the tree is only searched for a
        // definition, and dropped with the pool.
        let literals = cx.literal_pool();
        let core_expr = desugarer
            .desugar_tagged(args[0], cx, &literals)
            .map_err(|e| {
                EvalError::InvalidSyntax(format!("eval: failed to desugar expression: {e}"))
                    .with_diagnostic(e.diagnostic())
            })?;
        if has_top_level_definition(&core_expr) {
            return Err(EvalError::InvalidSyntax(
                "eval: cannot define in immutable environment".to_string(),
            ));
        }
    }

    Ok(Step::Eval {
        expr: args[0],
        env,
        state: Value::UNSPECIFIED,
    })
}

/// R5RS syntactic keywords (special forms)
/// These are the only bindings in (null-environment 5)
const R5RS_SYNTAX: &[&str] = &[
    "and",
    "begin",
    "case",
    "cond",
    "define",
    "define-syntax",
    "delay",
    "do",
    "if",
    "lambda",
    "let",
    "let*",
    "let-syntax",
    "letrec",
    "letrec-syntax",
    "or",
    "quasiquote",
    "quote",
    "set!",
    "syntax-rules",
];

/// The [`R5RS_SYNTAX`] names `(scheme base)` does not export.
///
/// R5RS puts `delay` in the base language; Patina puts it in `(scheme lazy)`.
/// Kept as data beside the list it qualifies, rather than as a name buried in
/// one of the two loops that install these — the loops used to differ on
/// whether the exception was checked at all.
///
/// Chasing it is not worth it: the macro expands to a `(scheme lazy)`
/// primitive that a null environment would not have either.
const R5RS_SYNTAX_NOT_IN_BASE: &[&str] = &["delay"];

/// Install the `R5RS_SYNTAX` keywords `(scheme base)` exports into `env`.
///
/// Shared by `null-environment` and `scheme-report-environment`, which ran
/// byte-identical copies of this loop. Every name resolves now that syntactic
/// keywords are bindings: the Scheme-level macros (`cond`, `let`, `do`, …)
/// always did, and the core keywords (`if`, `lambda`, `quote`, …) used to be
/// skipped silently here, with a comment claiming they were "handled by the
/// evaluator and don't need to be in the environment" — true only because they
/// were recognized in every scope, which is what made `(null-environment 5)`
/// not null.
fn install_r5rs_syntax(env: &Rc<Environment>, base_lib: &patina_core::Library) {
    for name in R5RS_SYNTAX {
        // The library's binding, as any import installs (#406).
        if !base_lib.import_into(env, *name, name) {
            debug_assert!(
                R5RS_SYNTAX_NOT_IN_BASE.contains(name),
                "(scheme base) stopped exporting the R5RS keyword '{name}'; \
                 add it to R5RS_SYNTAX_NOT_IN_BASE if that is intended"
            );
        }
    }
}

/// R5RS procedures (subset of scheme base that existed in R5RS)
/// Combined with R5RS_SYNTAX, these form (scheme-report-environment 5)
const R5RS_PROCEDURES: &[&str] = &[
    // Equivalence predicates
    "eq?",
    "eqv?",
    "equal?",
    // Numbers
    "+",
    "-",
    "*",
    "/",
    "=",
    "<",
    ">",
    "<=",
    ">=",
    "abs",
    "quotient",
    "remainder",
    "modulo",
    "gcd",
    "lcm",
    "numerator",
    "denominator",
    "floor",
    "ceiling",
    "truncate",
    "round",
    "rationalize",
    "exp",
    "log",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "sqrt",
    "expt",
    "make-rectangular",
    "make-polar",
    "real-part",
    "imag-part",
    "magnitude",
    "angle",
    "exact->inexact",
    "inexact->exact",
    "number->string",
    "string->number",
    // Number predicates
    "number?",
    "complex?",
    "real?",
    "rational?",
    "integer?",
    "exact?",
    "inexact?",
    "zero?",
    "positive?",
    "negative?",
    "odd?",
    "even?",
    // Booleans
    "not",
    "boolean?",
    // Pairs and lists
    "pair?",
    "cons",
    "car",
    "cdr",
    "set-car!",
    "set-cdr!",
    "caar",
    "cadr",
    "cdar",
    "cddr",
    "caaar",
    "caadr",
    "cadar",
    "caddr",
    "cdaar",
    "cdadr",
    "cddar",
    "cdddr",
    "caaaar",
    "caaadr",
    "caadar",
    "caaddr",
    "cadaar",
    "cadadr",
    "caddar",
    "cadddr",
    "cdaaar",
    "cdaadr",
    "cdadar",
    "cdaddr",
    "cddaar",
    "cddadr",
    "cdddar",
    "cddddr",
    "null?",
    "list?",
    "list",
    "length",
    "append",
    "reverse",
    "list-tail",
    "list-ref",
    "memq",
    "memv",
    "member",
    "assq",
    "assv",
    "assoc",
    // Symbols
    "symbol?",
    "symbol->string",
    "string->symbol",
    // Characters
    "char?",
    "char=?",
    "char<?",
    "char>?",
    "char<=?",
    "char>=?",
    "char-ci=?",
    "char-ci<?",
    "char-ci>?",
    "char-ci<=?",
    "char-ci>=?",
    "char-alphabetic?",
    "char-numeric?",
    "char-whitespace?",
    "char-upper-case?",
    "char-lower-case?",
    "char->integer",
    "integer->char",
    "char-upcase",
    "char-downcase",
    // Strings
    "string?",
    "make-string",
    "string",
    "string-length",
    "string-ref",
    "string-set!",
    "string=?",
    "string-ci=?",
    "string<?",
    "string>?",
    "string<=?",
    "string>=?",
    "string-ci<?",
    "string-ci>?",
    "string-ci<=?",
    "string-ci>=?",
    "substring",
    "string-append",
    "string->list",
    "list->string",
    "string-copy",
    "string-fill!",
    // Vectors
    "vector?",
    "make-vector",
    "vector",
    "vector-length",
    "vector-ref",
    "vector-set!",
    "vector->list",
    "list->vector",
    "vector-fill!",
    // Control features
    "procedure?",
    "apply",
    "map",
    "for-each",
    "force",
    "call-with-current-continuation",
    "values",
    "call-with-values",
    "dynamic-wind",
    // Eval
    "eval",
    "scheme-report-environment",
    "null-environment",
    // I/O
    "call-with-input-file",
    "call-with-output-file",
    "input-port?",
    "output-port?",
    "current-input-port",
    "current-output-port",
    "with-input-from-file",
    "with-output-to-file",
    "open-input-file",
    "open-output-file",
    "close-input-port",
    "close-output-port",
    "read",
    "read-char",
    "peek-char",
    "eof-object?",
    "char-ready?",
    "write",
    "display",
    "newline",
    "write-char",
    "load",
    // REPL
    "interaction-environment",
];

/// (null-environment version) → environment-specifier
///
/// Returns an environment with only the R5RS syntactic keywords.
/// Currently only version 5 is supported.
fn primitive_null_environment<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "null-environment expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let version = if args[0].is_fixnum() {
        args[0].as_fixnum_unchecked()
    } else {
        return Err(EvalError::TypeError(format!(
            "null-environment: expected integer version, got {}",
            cx.type_name(args[0])
        )));
    };

    if version != 5 {
        return Err(EvalError::InvalidSyntax(format!(
            "null-environment: unsupported version {}, only 5 is supported",
            version
        )));
    }

    // An environment with only syntactic keywords, on this context's heap
    let env = Rc::new(cx.new_namespace());

    // Load scheme base to get the keywords
    #[expect(
        clippy::disallowed_methods,
        reason = "holds `env`, an environment with nothing in it yet, so the load can free nothing \
                  this frame holds"
    )]
    let base_lib = ctx
        .load_scheme_library(&["scheme".to_string(), "base".to_string()])
        .map_err(|e| EvalError::InternalError(format!("Cannot load scheme base: {}", e)))?;

    install_r5rs_syntax(&env, &base_lib);

    Ok(cx.alloc_environment_specifier(env, false))
}

/// (scheme-report-environment version) → environment-specifier
///
/// Returns an environment with all R5RS bindings (syntax + procedures).
/// Currently only version 5 is supported.
fn primitive_scheme_report_environment<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "scheme-report-environment expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let version = if args[0].is_fixnum() {
        args[0].as_fixnum_unchecked()
    } else {
        return Err(EvalError::TypeError(format!(
            "scheme-report-environment: expected integer version, got {}",
            cx.type_name(args[0])
        )));
    };

    if version != 5 {
        return Err(EvalError::InvalidSyntax(format!(
            "scheme-report-environment: unsupported version {}, only 5 is supported",
            version
        )));
    }

    // An environment with all R5RS bindings, on this context's heap
    let env = Rc::new(cx.new_namespace());

    // Load scheme base
    #[expect(
        clippy::disallowed_methods,
        reason = "holds `env`, an environment with nothing in it yet, so the load can free nothing \
                  this frame holds"
    )]
    let base_lib = ctx
        .load_scheme_library(&["scheme".to_string(), "base".to_string()])
        .map_err(|e| EvalError::InternalError(format!("Cannot load scheme base: {}", e)))?;

    // Install syntactic keywords
    install_r5rs_syntax(&env, &base_lib);

    // Install procedures
    for name in R5RS_PROCEDURES {
        base_lib.import_into(&env, *name, name);
    }

    // Two optional libraries: absent is fine, but a *failure* is not the same
    // as absent. `load_scheme_library` is a re-entry boundary, so its error
    // may be the continuation-escape sentinel with a value parked on the VM —
    // discarding that would leave the escape in flight to be misread by the
    // next unrelated error. Unreachable today (nothing user-written runs while
    // `(scheme inexact)` loads), and the only sentinel-protocol break the
    // audit's sweep of every `apply_proc`/`eval_expr` site found (F7).
    #[expect(
        clippy::disallowed_methods,
        reason = "holds `env`, which has the imports of the libraries loaded so far: shared \
                  locations the registered libraries own, or, for an export that is not a plain \
                  binding, a copy of a value the library's export table holds, both traced \
                  through the library registry (#406). `base_lib` and an `inexact_lib` are \
                  copies of registered libraries, whose values the registry traces too. So the \
                  frame holds no value of its own, and nothing here needs the load deferred"
    )]
    let optional_library =
        |name: &str| -> Result<Option<Rc<patina_core::library::Library>>, EvalError> {
            match ctx.load_scheme_library(&["scheme".to_string(), name.to_string()]) {
                Ok(lib) => Ok(Some(lib)),
                Err(EvalError::ContinuationEscape) => Err(EvalError::ContinuationEscape),
                Err(_) => Ok(None),
            }
        };

    // Try to load additional libraries for R5RS features
    // (scheme inexact) for sin, cos, etc.
    if let Some(inexact_lib) = optional_library("inexact")? {
        for name in &[
            "sin", "cos", "tan", "asin", "acos", "atan", "exp", "log", "sqrt",
        ] {
            inexact_lib.import_into(&env, *name, name);
        }
    }

    // (scheme complex) for make-rectangular, etc.
    if let Some(complex_lib) = optional_library("complex")? {
        for name in &[
            "make-rectangular",
            "make-polar",
            "real-part",
            "imag-part",
            "magnitude",
            "angle",
        ] {
            complex_lib.import_into(&env, *name, name);
        }
    }

    // Add the R5RS-specific aliases
    // exact->inexact is an alias for inexact
    base_lib.import_into(&env, "exact->inexact", "inexact");
    // inexact->exact is an alias for exact
    base_lib.import_into(&env, "inexact->exact", "exact");

    // Add null-environment and scheme-report-environment themselves
    let r5rs_lib = vec!["scheme".to_string(), "r5rs".to_string()];
    cx.define_primitive(&env, "null-environment", Arity::Exact(1), r5rs_lib.clone());
    cx.define_primitive(&env, "scheme-report-environment", Arity::Exact(1), r5rs_lib);

    Ok(cx.alloc_environment_specifier(env, false))
}

/// The state `load` keeps between the forms of its file, a vector: the forms
/// still to evaluate, the parse error that ends them (a string) or `#f`, and
/// the environment specifier they are evaluated in.
///
/// Every continuation captured in a form shares it, so one captured in an
/// earlier form and re-entered from a later one carries on with the form
/// after the one that re-entered it — the position of the port chibi's
/// `load` reads from, and what `escape_from_primitive.rs` checks.
mod load_state {
    pub const PATH: usize = 3;
    pub(super) const FORMS: usize = 0;
    pub(super) const ERROR: usize = 1;
    pub(super) const ENV: usize = 2;
}

/// (load filename) → unspecified
/// (load filename environment-specifier) → unspecified
///
/// Reads and evaluates all expressions from the given file.
/// If an environment specifier is given, evaluates in that environment;
/// otherwise uses the interaction environment.
///
/// The machine evaluates each form ([`Step::Eval`]), as `eval` has it do,
/// and resumes this with the next (#477). The file is parsed first, to the
/// end or to a parse error, which is raised once the forms before it have
/// run — where reading form by form raised it.
///
/// A read that fails because descriptors ran out asks for a full collection
/// at the call ([`Step::Collect`]), which closes the file ports it finds
/// dead, and reads once more when resumed, as the open primitives do
/// (`io/file.rs`, #607) and as chibi's `load` does, whose read is an
/// `open-input-file`. The state it keeps is a pair of the file name and the
/// environment specifier, which [`load_resume`] tells apart from the vector
/// it keeps between forms ([`load_state`]).
fn primitive_load<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "load expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract filename string
    let filename = cx.get_string_contents(args[0]).ok_or_else(|| {
        EvalError::TypeError(format!(
            "load: expected string filename, got {}",
            cx.type_name(args[0])
        ))
    })?;

    // The environment to evaluate in, as a specifier the state can hold.
    let env_spec = if args.len() == 2 {
        if cx.get_environment_specifier(args[1]).is_none() {
            return Err(EvalError::TypeError(format!(
                "load: expected environment, got {}",
                cx.type_name(args[1])
            )));
        }
        args[1]
    } else {
        // The machine's, which on the VM, inside a library's body, is the
        // library's: why `load` takes the `ApplyContext` beside the context.
        cx.mutable_specifier(&ctx.interaction_environment())
    };

    match read_source(cx, &filename) {
        Err(e) if out_of_descriptors(&e) => {
            let state = cx.alloc_pair(args[0], env_spec);
            Ok(Step::Collect {
                kind: CollectKind::Major,
                state,
            })
        }
        content => load_forms(cx, filename, env_spec, content),
    }
}

/// Read `load`'s file whole.
fn read_source(cx: &Cx<'_>, filename: &str) -> std::io::Result<String> {
    cx.instance()
        .fs()
        .read_to_string(std::path::Path::new(filename))
}

/// `load` with its file read, or the error the read failed with, which is
/// raised: parse the forms, keep them in the state, and evaluate the first.
fn load_forms<'gc>(
    cx: &mut Cx<'gc>,
    filename: String,
    env_spec: Value<'gc>,
    content: std::io::Result<String>,
) -> Result<Step<'gc>, EvalError> {
    let content = content.map_err(|e| {
        EvalError::IOError(format!("load: cannot open '{}': {}", filename, e))
            .with_diagnostic(Diagnostic::new(DiagnosticKind::Io, e.to_string()).at_path(&filename))
    })?;

    let parse_error = |e: &patina_frontend::ParseError| {
        format!(
            "load: parse error in '{filename}': {}",
            e.format_in_source(&filename, &content)
        )
    };
    let mut parser =
        patina_frontend::Parser::new_program(&content, &filename, false).map_err(|e| {
            EvalError::InvalidSyntax(parse_error(&e)).with_diagnostic(
                Diagnostic::new(DiagnosticKind::Parse, e.to_string()).at_path(&filename),
            )
        })?;
    let mut forms = Vec::new();
    let error = loop {
        match parser.parse_next(cx) {
            Ok(Some(expr)) => forms.push(expr),
            Ok(None) => break None,
            Err(e) => break Some(parse_error(&e)),
        }
    };

    let forms = cx.list_from_iter(forms);
    let error = match error {
        Some(message) => cx.alloc_string(message),
        None => Value::FALSE,
    };
    let path = cx.alloc_string(filename);
    let state = cx.alloc_vector(vec![forms, error, env_spec, path]);
    load_next(cx, state)
}

/// `load` resumed. After the collection its read asked for, `state` is the
/// pair of the file name and the environment specifier: read again, whether
/// or not the machine collected, as the opens do, so that a second failure
/// raises the error the system gives. Otherwise a form has returned, and
/// `state` is the vector of [`load_state`]: carry on with the next.
fn load_resume<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    state: Value<'gc>,
    _value: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    let Some((name, env_spec)) = cx.try_pair(state) else {
        return load_next(cx, state);
    };
    let filename = cx
        .get_string_contents(name)
        .ok_or_else(|| EvalError::InternalError("load: lost its file name".into()))?;
    let content = read_source(cx, &filename);
    load_forms(cx, filename, env_spec, content)
}

/// `load`'s next step: evaluate the next form of its file, raise the parse
/// error that ends them, or finish. What a form returned is not used.
fn load_next<'gc>(cx: &mut Cx<'gc>, state: Value<'gc>) -> Result<Step<'gc>, EvalError> {
    let forms = cx.vector_ref(state, load_state::FORMS);
    let error = cx.vector_ref(state, load_state::ERROR);
    let env_spec = cx.vector_ref(state, load_state::ENV);
    if let Some((expr, rest)) = cx.try_pair(forms) {
        cx.vector_set(state, load_state::FORMS, rest);
        let (env, _) = cx
            .get_environment_specifier(env_spec)
            .ok_or_else(|| EvalError::InternalError("load: lost its environment".into()))?;
        return Ok(Step::Eval { expr, env, state });
    }
    if let Some(message) = cx.get_string_contents(error) {
        let path = cx
            .get_string_contents(cx.vector_ref(state, load_state::PATH))
            .unwrap();
        let diagnostic = Diagnostic::new(DiagnosticKind::Parse, &message).at_path(path);
        return Err(EvalError::InvalidSyntax(message).with_diagnostic(diagnostic));
    }
    Ok(Step::Done(Value::UNSPECIFIED))
}

/// (interaction-environment) → environment-specifier
///
/// Returns a mutable environment specifier for the interaction (global)
/// environment: the same object every call, as chibi, Gauche and Chez answer
/// (`Environment::mutable_specifier`), so a loop of these allocates nothing.
/// The environment is the backend's `interaction_environment`, which on the
/// VM, inside a library's body, is the library's (`mutable_specifier`'s doc
/// says what that means for the cache).
fn primitive_interaction_environment<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "interaction-environment expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }

    Ok(cx.mutable_specifier(&ctx.interaction_environment()))
}

/// Register eval primitives
pub fn register(registry: &mut PrimitiveRegistry) {
    // environment - create environment from import sets
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.eval",
        "environment",
        Arity::Min(0),
        "Creates an immutable environment from the given import sets.",
        primitive_environment,
    ));

    // eval - evaluate expression in environment
    registry.register(PrimitiveFn::new_resumable(
        "scheme.eval",
        "eval",
        Arity::Exact(2),
        "Evaluates an expression in the specified environment.",
        primitive_eval,
        done_with_result,
    ));

    // null-environment - R5RS environment with only syntactic keywords
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.r5rs",
        "null-environment",
        Arity::Exact(1),
        "Returns an environment with only the R5RS syntactic keywords.",
        primitive_null_environment,
    ));

    // scheme-report-environment - R5RS environment with all bindings
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.r5rs",
        "scheme-report-environment",
        Arity::Exact(1),
        "Returns an environment with all R5RS bindings.",
        primitive_scheme_report_environment,
    ));

    // load - read and evaluate a file (scheme load library)
    registry.register(PrimitiveFn::new_resumable(
        "patina.internal.eval",
        "load",
        Arity::Range(1, 2),
        "Reads and evaluates all expressions from a file.",
        primitive_load,
        load_resume,
    ));

    // interaction-environment - return the REPL/global environment (scheme repl library)
    registry.register(PrimitiveFn::new_higher_order(
        "patina.internal.eval",
        "interaction-environment",
        Arity::Exact(0),
        "Returns the interaction environment.",
        primitive_interaction_environment,
    ));
}
