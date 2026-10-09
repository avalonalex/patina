// InterpreterError contains DesugarError (large)
// Boxing would add complexity for minimal benefit in this interpreter context
#![allow(clippy::result_large_err)]

//! Patina Interpreter - High-level interface for Scheme evaluation
//!
//! This crate provides the `Interpreter` API that combines frontend (parsing)
//! with backend (evaluation). It supports multiple backend implementations
//! through the `Backend` trait.
//!
//! `Interpreter<B>` owns program reading, source tracking and error handling;
//! the backend owns desugaring and execution. `VmInterpreter` is the recommended
//! convenience type, matching the CLI. `TreeWalkInterpreter` selects the CPS
//! tree-walker explicitly. Scheme libraries must be available through the
//! usual library search paths (for example, `PATINA_LIBRARY_PATH` set before
//! starting the host).
//!
//! ```
//! # #[cfg(feature = "vm")]
//! # {
//! use patina_interpreter::VmInterpreter;
//! let interp = VmInterpreter::new_vm();
//! # // rustdoc runs in a temporary directory, outside the installed layout.
//! # interp.backend().add_library_search_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib"));
//! let result = interp.eval_program_owned("(import (scheme base)) (define x 40) (list (+ x 2))").unwrap();
//! assert_eq!(interp.display_tagged(&result), "(42)");
//! # }
//! ```
//!
//! A host keeps a value through a handle, [`Owned`], which roots it until the
//! handle is dropped; a bare `TaggedValue` is freed by the next collection
//! that finds nothing else holding it (#605). See [handles](Interpreter#handles).
//!
//! Values do not outlive their interpreter. Dropping an interpreter tears its
//! heap down (#604): every object is freed, so the heap's memory is returned
//! and each file port it left open is flushed and closed. A `TaggedValue` or
//! an environment kept past the interpreter names freed memory; a debug build
//! reports a read of one as a use after free. A handle kept past it holds
//! nothing: the interpreter that could read it is gone, and dropping it does
//! nothing.
//!
//! Features: `vm`, `tree-walker`, and `legacy-pipeline` (which enables
//! `tree-walker`). All are enabled by default to retain existing imports. Use
//! `default-features = false, features = ["vm"]` for a VM-only dependency graph.
//! With no features, `Interpreter<B>` supports a backend supplied by the host.
//! Legacy `SimpleInterpreter` and pipeline types remain tree-walker adapters;
//! migrate to an explicit interpreter type for structured errors.

#[cfg(feature = "legacy-pipeline")]
pub mod legacy;
#[cfg(feature = "legacy-pipeline")]
pub mod simple;
#[cfg(feature = "legacy-pipeline")]
#[allow(deprecated)]
pub use simple::SimpleInterpreter;
#[cfg(all(test, feature = "vm", feature = "tree-walker"))]
mod reentry_lint_control;

// Re-export types from workspace crates for convenience
#[cfg(feature = "legacy-pipeline")]
#[allow(deprecated)]
pub use legacy::{Pipeline, PipelineError, StandardPipeline};
pub use patina_core::error::SourceLocation;
pub use patina_core::{Owned, TaggedValue};
pub use patina_frontend::{
    DesugarError, Desugarer, LexError, Lexer, ParseError, Parser, SourceMap,
};
// Deprecated no-op, kept until stage 5e (#643).
#[allow(deprecated)]
pub use patina_frontend::prune_freed_locations;
pub use patina_ir::CoreExpr;
use patina_runtime::HasDiagnostic;
pub use patina_runtime::{Arity, Backend, Environment, EvalError, Procedure};
#[cfg(feature = "tree-walker")]
pub use patina_tree_walker::{Evaluator, TreeWalker};
#[cfg(feature = "vm")]
pub use patina_vm::{VmBackend, VmBackendError};
use std::cell::RefCell;
use std::rc::Rc;

/// What running a whole program with `-k` has to report back.
///
/// Carrying on past an error decides how much of the program runs, never
/// whether it succeeded: both fields feed [`ProgramOutcome::clean`], which is
/// what a caller that owns an exit status must consult.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramOutcome {
    /// Errors reported while evaluating top-level forms.
    pub eval_errors: usize,
    /// Whether the program was read to its end. A read error ends the run even
    /// under `-k`, because nothing after an unfinished datum can be read, so the
    /// rest of the program never ran.
    pub read_to_end: bool,
}

impl ProgramOutcome {
    /// The whole program ran, and nothing went wrong in it.
    pub fn clean(&self) -> bool {
        self.eval_errors == 0 && self.read_to_end
    }
}

/// Format any `InterpreterError` with source context.
///
/// An alias for [`format_backend_error_with_source`]: an
/// evaluation error or reader error that carries a position gets caret
/// context; errors without a position fall back to `Display`.
pub fn format_interpreter_error<E: std::error::Error + HasSourceLocation>(
    error: &InterpreterError<E>,
    source_map: &SourceMap,
) -> String {
    format_backend_error_with_source(error, source_map)
}

/// Format a `ParseError` with the same caret context an evaluation error gets.
///
/// Reader errors carry their own spans, including errors deferred by
/// lookahead. The source map supplies the source name and text, without
/// needing a successfully parsed value to look up.
pub fn format_parse_error_with_source(error: &ParseError, source_map: &SourceMap) -> String {
    error.format_with_source(source_map)
}

/// Format an evaluation error, from any backend, with source context from a
/// `SourceMap`.
///
/// When the error carries a location, this prints a caret-style context block
/// showing the relevant source line and position, and the macro expansion
/// chain recorded there:
///
/// ```text
///    1 | (define (foo) x)
///                     ^
/// Error: Undefined variable: x
///   at test.scm:1:15
/// ```
///
/// Falls back to `error.to_string()` when no source context is available.
pub fn format_error_with_source<E: std::error::Error + HasSourceLocation>(
    error: &E,
    source_map: &SourceMap,
) -> String {
    if let Some(loc) = error.source_location() {
        let mut parts = vec![error.message_without_location()];
        parts.push(format!("  at {}", loc));
        if let Some(ctx) = source_map.format_context(loc) {
            parts.push(ctx);
        }
        // Phase 4: macro expansion chain
        if let Some(names) = source_map.get_expansions(loc) {
            if names.len() == 1 {
                parts.push(format!("  macro expansion: {}", names[0]));
            } else {
                parts.push(format!(
                    "  macro expansion chain: {}",
                    names.join(" \u{2192} ")
                ));
            }
        }
        parts.join("\n")
    } else {
        error.to_string()
    }
}

// Re-export HasSourceLocation from patina-core for convenience.
pub use patina_core::error::HasSourceLocation;

/// Format any backend error that implements `HasSourceLocation` with source context.
///
/// This is the generic version of `format_interpreter_error` that works with
/// any backend error type (tree-walker, VM, etc.).
pub fn format_backend_error_with_source<E: std::error::Error + HasSourceLocation>(
    error: &InterpreterError<E>,
    source_map: &SourceMap,
) -> String {
    match error {
        InterpreterError::Parse(parse_err) => format_parse_error_with_source(parse_err, source_map),
        InterpreterError::Backend(backend_err) => format_error_with_source(backend_err, source_map),
        other => other.to_string(),
    }
}

/// What an evaluation returns with the source map that placed the text it
/// read, for formatting an error it reports.
pub type WithSourceMap<T> = (T, Rc<RefCell<SourceMap>>);

/// What [`Interpreter::display_tagged`] shows: a bare [`TaggedValue`], or a
/// handle, [`Owned`] (by value or by reference), which the interpreter that
/// made it reads.
pub trait AsValue: sealed::Sealed {
    /// The value, read from `heap`.
    #[doc(hidden)]
    fn value_in(&self, heap: &patina_core::Heap) -> TaggedValue;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::TaggedValue {}
    impl Sealed for super::Owned {}
    impl Sealed for &super::Owned {}
}

impl AsValue for TaggedValue {
    fn value_in(&self, _: &patina_core::Heap) -> TaggedValue {
        *self
    }
}

impl AsValue for Owned {
    fn value_in(&self, heap: &patina_core::Heap) -> TaggedValue {
        heap.held(self)
    }
}

impl AsValue for &Owned {
    fn value_in(&self, heap: &patina_core::Heap) -> TaggedValue {
        heap.held(self)
    }
}

/// How reading a program a form at a time ended.
enum FormsEnd<E> {
    /// Every form was read.
    Read,
    /// An evaluation error the caller chose to stop at.
    Stopped(E),
    /// The text could not be read on from here.
    Unreadable(ParseError),
}

/// High-level interpreter interface that combines parsing and evaluation
///
/// The interpreter is generic over the backend implementation, allowing
/// you to swap between different evaluation strategies (tree-walker, VM, JIT)
/// without changing your code.
///
/// # Type Parameters
///
/// - `B`: The backend implementation (must implement `Backend` trait)
///
/// # Example
///
/// ```
/// # #[cfg(feature = "vm")]
/// # {
/// use patina_interpreter::{Interpreter, VmBackend};
/// let interp = Interpreter::new(VmBackend::new());
/// let result = interp.eval_str_owned("42").unwrap();
/// assert_eq!(interp.display_tagged(&result), "42");
/// # }
/// ```
///
/// # Handles
///
/// A [`TaggedValue`] names a slot in the interpreter's heap and roots
/// nothing. Once the evaluation that answered it has returned, a later one
/// that collects can free it, and a value kept across that evaluation reads
/// whatever comes to occupy its slot (#605). The `eval_*_owned` methods
/// answer a handle, [`Owned`], instead, which roots its value until the
/// handle is dropped; [`Interpreter::lookup`] answers one for a global. The
/// interpreter that made a handle reads it ([`Interpreter::display_tagged`],
/// [`Interpreter::raw_value`]), and any other panics on it. The `eval_*`
/// methods that answer a bare value are deprecated.
///
/// The `_owned` suffix is temporary. At stage 5e of the collector's redesign
/// (`PRD/GC_PRD.md`, decision 13) the bare methods go and the plain names
/// answer handles; the `_owned` names stay as deprecated aliases of them
/// until stage 5g.
pub struct Interpreter<B: Backend> {
    backend: B,
}

impl<B: Backend> Interpreter<B> {
    /// Create a new interpreter with the given backend
    ///
    /// # Arguments
    ///
    /// - `backend`: The backend implementation to use for evaluation
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "vm")]
    /// # {
    /// use patina_interpreter::{Interpreter, VmBackend};
    /// let interp = Interpreter::new(VmBackend::new());
    /// let value = interp.eval_str_owned("42").unwrap();
    /// assert_eq!(interp.raw_value(&value).as_fixnum(), Some(42));
    /// # }
    /// ```
    pub fn new(backend: B) -> Self {
        Interpreter { backend }
    }

    /// Set the program name and arguments returned by Scheme's `(command-line)`.
    ///
    /// The default is `("patina")`; host process options are never inherited.
    /// This setting belongs to this interpreter's heap and reaches library
    /// bodies, `eval`, and `load` as well as top-level evaluation. Configure it
    /// before evaluating the program. Each call to `(command-line)` returns
    /// fresh Scheme strings and a fresh list.
    ///
    /// ```
    /// # #[cfg(feature = "vm")]
    /// # {
    /// use patina_interpreter::VmInterpreter;
    /// let interp = VmInterpreter::new_vm();
    /// interp.set_command_line("embedded.scm", ["hello".to_owned()]);
    /// # }
    /// ```
    pub fn set_command_line(
        &self,
        program_name: impl Into<String>,
        arguments: impl IntoIterator<Item = String>,
    ) {
        // User-provided conversions/iterators can call back into the interpreter.
        // Consume them before borrowing its heap.
        let program_name = program_name.into();
        let arguments = arguments.into_iter().collect();
        self.backend
            .global_env()
            .heap()
            .borrow_mut()
            .set_command_line(program_name, arguments);
    }

    /// Evaluate a string containing one Scheme expression, and answer a
    /// handle that keeps its value until the handle is dropped.
    ///
    /// Uses the backend's evaluation strategy. Text after that expression is
    /// not evaluated, but it must still read: a remainder that ends inside a
    /// datum is an error rather than something to drop silently. Use
    /// `eval_program_owned` to evaluate every form in a string. Source
    /// positions are recorded under `<eval>`; use the named variant to retain
    /// its source map. The `_owned` suffix is temporary (see
    /// [handles](Interpreter#handles)).
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "vm")]
    /// # {
    /// # use patina_interpreter::VmInterpreter;
    /// # let interp = VmInterpreter::new_vm();
    /// let value = interp.eval_str_owned("42").unwrap();
    /// assert_eq!(interp.raw_value(&value).as_fixnum(), Some(42));
    /// # }
    /// ```
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_str_with_source_name_owned`: holds nothing"
    )]
    pub fn eval_str_owned(&self, input: &str) -> Result<Owned, InterpreterError<B::Error>> {
        self.eval_str_with_source_name_owned(input, "<eval>").0
    }

    /// Evaluate a string containing one expression with its source positions,
    /// naming the source `source_name`, and return the source map that placed
    /// it for formatting an error. See [`Interpreter::eval_str_owned`].
    pub fn eval_str_with_source_name_owned(
        &self,
        input: &str,
        source_name: &str,
    ) -> WithSourceMap<Result<Owned, InterpreterError<B::Error>>> {
        #[expect(
            clippy::disallowed_methods,
            reason = "`eval_str_in_env` in the global environment: holds nothing across it, and \
                      holds the value it answers before anything can collect"
        )]
        let (result, source_map) =
            self.eval_str_in_env(input, source_name, self.backend.global_env());
        (result.map(|value| self.hold(value)), source_map)
    }

    /// Evaluate a string containing one Scheme expression, answering its
    /// value bare. See [`Interpreter::eval_str_owned`].
    #[deprecated(
        note = "use `eval_str_owned`: nothing roots a bare value, so a later evaluation that \
                collects can free it (#605)"
    )]
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_str_in_env` in the global environment: holds nothing"
    )]
    pub fn eval_str(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_str_in_env(input, "<eval>", self.backend.global_env())
            .0
    }

    /// [`Interpreter::eval_str_with_source_name_owned`], answering the value
    /// bare.
    #[deprecated(
        note = "use `eval_str_with_source_name_owned`: nothing roots a bare value, so a later \
                evaluation that collects can free it (#605)"
    )]
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_str_in_env` in the global environment: holds nothing"
    )]
    pub fn eval_str_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> WithSourceMap<Result<TaggedValue, InterpreterError<B::Error>>> {
        self.eval_str_in_env(input, source_name, self.backend.global_env())
    }

    fn eval_str_in_env(
        &self,
        input: &str,
        source_name: &str,
        env: &Rc<Environment>,
    ) -> WithSourceMap<Result<TaggedValue, InterpreterError<B::Error>>> {
        let heap = self.backend.global_env().heap();
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        let mut parser = match Parser::new_with_source_map(
            input,
            heap.clone(),
            Rc::from(source_name),
            source_map.clone(),
        ) {
            Ok(p) => p,
            Err(e) => return (Err(e.into()), source_map),
        };
        let expr = match parser.parse().and_then(|e| parser.skip_rest().map(|()| e)) {
            Ok(e) => e,
            Err(e) => return (Err(e.into()), source_map),
        };
        drop(parser);
        #[expect(
            clippy::disallowed_methods,
            reason = "the outermost entry, so the backend's loop may collect. Holds the source \
                      map, keyed by raw bits it never dereferences, which owns no Scheme value: \
                      a collection can only leave it stale keys (docs/GC_DESIGN.md §9.1), which \
                      misattribute a diagnostic and free nothing. The parser is dropped and the \
                      datum is not read after the call"
        )]
        let result = self
            .backend
            .eval_with_source_map(expr, env, &source_map)
            .map_err(InterpreterError::Backend);
        (result, source_map)
    }

    /// [`Interpreter::eval_str`] under another name.
    #[deprecated(
        note = "use `eval_str_owned`: nothing roots a bare value, so a later evaluation that \
                collects can free it (#605)"
    )]
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_str_in_env` in the global environment: holds nothing"
    )]
    pub fn eval_str_tracked(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_str_in_env(input, "<eval>", self.backend.global_env())
            .0
    }

    /// Evaluate multiple expressions from a string, and answer a handle on
    /// the last result.
    ///
    /// This is useful for evaluating entire programs or test files. Each
    /// expression is parsed and evaluated in sequence, with the result of the
    /// last expression being returned. The `_owned` suffix is temporary (see
    /// [handles](Interpreter#handles)).
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_with_source_name_owned`: holds nothing"
    )]
    pub fn eval_program_owned(&self, input: &str) -> Result<Owned, InterpreterError<B::Error>> {
        self.eval_program_with_source_name_owned(input, "<eval>").0
    }

    /// Evaluate a program (multiple expressions) with its source positions,
    /// naming the source `source_name`; stop at the first error, and return
    /// the source map for formatting it.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_with_fold_case_owned`: holds nothing"
    )]
    pub fn eval_program_with_source_name_owned(
        &self,
        input: &str,
        source_name: &str,
    ) -> WithSourceMap<Result<Owned, InterpreterError<B::Error>>> {
        self.eval_program_with_fold_case_owned(input, source_name, &mut false)
    }

    /// Evaluate one interactive submission, retaining directives for the next.
    /// Each submission has its own source positions. Only directives actually
    /// consumed before an error survive; parser lookahead must not change them.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_in_env` in the global environment: holds nothing"
    )]
    pub fn eval_program_with_fold_case_owned(
        &self,
        input: &str,
        source_name: &str,
        fold_case: &mut bool,
    ) -> WithSourceMap<Result<Owned, InterpreterError<B::Error>>> {
        self.eval_program_in_env(input, source_name, fold_case, self.backend.global_env())
    }

    fn eval_program_in_env(
        &self,
        input: &str,
        source_name: &str,
        fold_case: &mut bool,
        env: &Rc<Environment>,
    ) -> WithSourceMap<Result<Owned, InterpreterError<B::Error>>> {
        let (value, end, source_map) =
            self.run_forms(input, source_name, fold_case, env, |error, _| Some(error));
        let result = match end {
            FormsEnd::Read => Ok(value),
            FormsEnd::Stopped(error) => Err(InterpreterError::Backend(error)),
            FormsEnd::Unreadable(error) => Err(error.into()),
        };
        (result, source_map)
    }

    /// Evaluate multiple expressions from a string, answering the last
    /// result bare. See [`Interpreter::eval_program_owned`].
    #[deprecated(
        note = "use `eval_program_owned`: nothing roots a bare value, so a later evaluation that \
                collects can free it (#605)"
    )]
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_owned`: holds nothing"
    )]
    pub fn eval_program(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_program_owned(input)
            .map(|value| self.raw_value(&value))
    }

    /// [`Interpreter::eval_program_with_source_name_owned`], answering the
    /// last result bare.
    #[deprecated(
        note = "use `eval_program_with_source_name_owned`: nothing roots a bare value, so a later \
                evaluation that collects can free it (#605)"
    )]
    pub fn eval_program_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> WithSourceMap<Result<TaggedValue, InterpreterError<B::Error>>> {
        #[expect(
            clippy::disallowed_methods,
            reason = "`eval_program_with_source_name_owned`: holds nothing across it"
        )]
        let (result, source_map) = self.eval_program_with_source_name_owned(input, source_name);
        (result.map(|value| self.raw_value(&value)), source_map)
    }

    /// [`Interpreter::eval_program_with_fold_case_owned`], answering the last
    /// result bare.
    #[deprecated(
        note = "use `eval_program_with_fold_case_owned`: nothing roots a bare value, so a later \
                evaluation that collects can free it (#605)"
    )]
    pub fn eval_program_with_fold_case(
        &self,
        input: &str,
        source_name: &str,
        fold_case: &mut bool,
    ) -> WithSourceMap<Result<TaggedValue, InterpreterError<B::Error>>> {
        #[expect(
            clippy::disallowed_methods,
            reason = "`eval_program_with_fold_case_owned`: holds nothing across it"
        )]
        let (result, source_map) =
            self.eval_program_with_fold_case_owned(input, source_name, fold_case);
        (result.map(|value| self.raw_value(&value)), source_map)
    }

    /// [`Interpreter::eval_program`] under another name.
    #[deprecated(
        note = "use `eval_program_owned`: nothing roots a bare value, so a later evaluation that \
                collects can free it (#605)"
    )]
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_owned`: holds nothing"
    )]
    pub fn eval_program_tracked(
        &self,
        input: &str,
    ) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_program_owned(input)
            .map(|value| self.raw_value(&value))
    }

    /// Evaluate multiple expressions from a string, continuing on errors, and
    /// answer a handle on the last result.
    ///
    /// Unlike `eval_program_owned`, this method does not stop on the first
    /// error. Instead, it prints errors to stderr and continues with the next
    /// expression. This is useful for test suites where you want to see all
    /// failures.
    ///
    /// Answers the last successfully evaluated result, or Unspecified if all
    /// failed. The `_owned` suffix is temporary (see
    /// [handles](Interpreter#handles)).
    pub fn eval_program_resilient_owned(&self, input: &str) -> Owned {
        let (value, end, _) = self.run_forms(
            input,
            "<eval>",
            &mut false,
            self.backend.global_env(),
            |error, _| {
                eprintln!("Error: {}", error);
                patina_runtime::exit_status::exit_if_interrupted();
                None
            },
        );
        if let FormsEnd::Unreadable(error) = end {
            eprintln!("Error: {}", error);
        }
        value
    }

    /// Evaluate multiple expressions from a string, continuing on errors,
    /// answering the last result bare. See
    /// [`Interpreter::eval_program_resilient_owned`].
    #[deprecated(
        note = "use `eval_program_resilient_owned`: nothing roots a bare value, so a later \
                evaluation that collects can free it (#605)"
    )]
    pub fn eval_program_resilient(&self, input: &str) -> TaggedValue {
        #[expect(
            clippy::disallowed_methods,
            reason = "`eval_program_resilient_owned`: holds nothing across it"
        )]
        let value = self.eval_program_resilient_owned(input);
        self.raw_value(&value)
    }

    /// [`Interpreter::eval_program_resilient`] under another name.
    #[deprecated(
        note = "use `eval_program_resilient_owned`: nothing roots a bare value, so a later \
                evaluation that collects can free it (#605)"
    )]
    pub fn eval_program_resilient_tracked(&self, input: &str) -> TaggedValue {
        #[expect(
            clippy::disallowed_methods,
            reason = "`eval_program_resilient_owned`: holds nothing across it"
        )]
        let value = self.eval_program_resilient_owned(input);
        self.raw_value(&value)
    }

    /// Evaluate a program with a named source, reporting each error and
    /// carrying on to the next top-level form: the CLI's `-k`. Answers a
    /// handle on the last result.
    ///
    /// Carrying on is a recovery policy, not a verdict. The returned
    /// [`ProgramOutcome`] counts the errors reported and says whether the
    /// program was read to its end; a caller that owns an exit status must fail
    /// on either, and [`ProgramOutcome::clean`] is that test. Each error is also
    /// recorded process-wide, through
    /// [`patina_runtime::exit_status::note_error_reported`], so a program that
    /// calls `(exit 0)` after failing still exits non-zero.
    ///
    /// An error that interrupted an `exit` stops the run instead of carrying
    /// on, and the caller ends the process with
    /// [`patina_runtime::exit_status::exit_if_interrupted`]. The `_owned`
    /// suffix is temporary (see [handles](Interpreter#handles)).
    pub fn eval_program_resilient_with_source_name_owned(
        &self,
        input: &str,
        source_name: &str,
    ) -> (Owned, ProgramOutcome)
    where
        B::Error: HasSourceLocation,
    {
        let mut eval_errors = 0usize;
        let (value, end, source_map) = self.run_forms(
            input,
            source_name,
            &mut false,
            self.backend.global_env(),
            |error, source_map| {
                eval_errors += 1;
                patina_runtime::exit_status::note_error_reported();
                let mut diagnostic = error.diagnostic();
                if diagnostic.path.is_none() {
                    diagnostic.path = Some(source_name.into());
                }
                patina_runtime::diagnostic::emit(diagnostic);
                eprintln!("Error: {}", format_error_with_source(&error, source_map));
                // The program asked to exit, so it does not carry on; the caller
                // ends the process.
                patina_runtime::exit_status::exit_interrupted().then_some(error)
            },
        );
        if let FormsEnd::Unreadable(error) = &end {
            patina_runtime::diagnostic::emit(error.diagnostic().at_path(source_name));
            eprintln!(
                "Error: {}",
                format_parse_error_with_source(error, &source_map.borrow())
            );
            patina_runtime::exit_status::note_error_reported();
        }
        let outcome = ProgramOutcome {
            eval_errors,
            read_to_end: matches!(end, FormsEnd::Read),
        };
        (value, outcome)
    }

    /// [`Interpreter::eval_program_resilient_with_source_name_owned`],
    /// answering the last result bare.
    #[deprecated(
        note = "use `eval_program_resilient_with_source_name_owned`: nothing roots a bare value, \
                so a later evaluation that collects can free it (#605)"
    )]
    pub fn eval_program_resilient_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> (TaggedValue, ProgramOutcome)
    where
        B::Error: HasSourceLocation,
    {
        #[expect(
            clippy::disallowed_methods,
            reason = "`eval_program_resilient_with_source_name_owned`: holds nothing across it"
        )]
        let (value, outcome) =
            self.eval_program_resilient_with_source_name_owned(input, source_name);
        (self.raw_value(&value), outcome)
    }

    /// Read `input` a form at a time and evaluate each with its source
    /// positions: the one loop behind all program entry points.
    ///
    /// An evaluation error goes to `on_error`, with the source map for
    /// formatting it, which gives the error back to stop there or returns
    /// `None` to carry on with the next form. A read error always stops: the
    /// parser leaves the offending token where it was, so reading on would
    /// report it again without end. Returns a handle on the last value
    /// evaluated, how reading ended, and the source map.
    fn run_forms(
        &self,
        input: &str,
        source_name: &str,
        fold_case: &mut bool,
        env: &Rc<Environment>,
        mut on_error: impl FnMut(B::Error, &SourceMap) -> Option<B::Error>,
    ) -> (Owned, FormsEnd<B::Error>, Rc<RefCell<SourceMap>>) {
        let heap = self.backend.global_env().heap();
        // The last value, held across each later form by a handle: it is the
        // answer when every later form fails, so a collection in one must not
        // free it (#605).
        let value = heap.borrow().hold(TaggedValue::UNSPECIFIED);
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        let mut parser = match Parser::new_with_source_map_and_fold_case(
            input,
            heap.clone(),
            Rc::from(source_name),
            source_map.clone(),
            *fold_case,
        ) {
            Ok(parser) => parser,
            Err(error) => {
                if let Some(span) = error.span() {
                    *fold_case = span.end.fold_case;
                }
                return (value, FormsEnd::Unreadable(error), source_map);
            }
        };
        loop {
            let datum = parser.parse_next();
            *fold_case = parser.read_state().fold_case;
            match datum {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the outermost entry, so the backend's loop may collect. The parser \
                              holds no heap value between data (it clears its labels after each), \
                              and the heap's provenance is not a root, pruned by sweep. `value`, \
                              the last result, is held across the form by a handle, which roots \
                              it (#605)"
                )]
                Ok(Some(expr)) => match self.backend.eval_with_source_map(expr, env, &source_map) {
                    Ok(result) => heap.borrow().set_held(&value, result),
                    Err(error) => {
                        let stop = on_error(error, &source_map.borrow());
                        if let Some(error) = stop {
                            return (value, FormsEnd::Stopped(error), source_map);
                        }
                    }
                },
                Ok(None) => return (value, FormsEnd::Read, source_map),
                Err(error) => return (value, FormsEnd::Unreadable(error), source_map),
            }
        }
    }

    /// The value bound to `name` in the global environment, held by a
    /// handle; `None` when the name is unbound.
    ///
    /// ```
    /// # #[cfg(feature = "vm")]
    /// # {
    /// # use patina_interpreter::VmInterpreter;
    /// # let interp = VmInterpreter::new_vm();
    /// # interp.backend().add_library_search_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib"));
    /// interp.eval_program_owned("(import (scheme base)) (define kept (list 1 2))").unwrap();
    /// let kept = interp.lookup("kept").unwrap();
    /// interp.eval_program_owned("(set! kept #f)").unwrap();
    /// assert_eq!(interp.display_tagged(&kept), "(1 2)");
    /// # }
    /// ```
    pub fn lookup(&self, name: &str) -> Option<Owned> {
        let value = self.backend.global_env().get(name)?;
        Some(self.hold(value))
    }

    /// The value `handle` holds, raw: a bare `TaggedValue`, valid until the
    /// next evaluation. Keep the handle, not this, across one. Panics on a
    /// handle another interpreter made.
    pub fn raw_value(&self, handle: &Owned) -> TaggedValue {
        self.backend.global_env().heap().borrow().held(handle)
    }

    /// A handle on `value`, which this interpreter answered.
    fn hold(&self, value: TaggedValue) -> Owned {
        self.backend.global_env().heap().borrow().hold(value)
    }

    /// The interpreter's global environment, shared with its backend.
    ///
    /// The raw layer (#605): what it holds and answers are bare
    /// `TaggedValue`s. A value read from it is rooted only while the name
    /// stays bound to it, so once the program rebinds the name a later
    /// evaluation that collects can free it; [`Interpreter::lookup`] answers
    /// a handle instead. An environment built from it with
    /// `Environment::with_parent` is not rooted between calls (#620).
    pub fn global_env(&self) -> Rc<Environment> {
        self.backend.global_env().clone()
    }

    /// Format a value for display using write notation (machine-readable):
    /// a bare `TaggedValue`, or a handle ([`Owned`]) this interpreter made.
    ///
    /// Uses the datum writer which properly handles all TaggedValue types
    /// including heap pairs, vectors, strings, and circular structures.
    /// Multiple values (from `values`) are unpacked and displayed one per line.
    pub fn display_tagged(&self, value: impl AsValue) -> String {
        use patina_primitives::primitives::io::datum_writer::format_write_tagged;
        let heap = self.backend.global_env().heap();
        let tv = value.value_in(&heap.borrow());

        // Unpack multiple values (R7RS: each value displayed on its own line)
        let vals = heap.borrow().get_values(tv).map(|v| v.to_vec());
        if let Some(vals) = vals {
            return vals
                .iter()
                .map(|v| format_write_tagged(*v, heap))
                .collect::<Vec<_>>()
                .join("\n");
        }

        format_write_tagged(tv, heap)
    }

    /// Get a reference to the underlying backend
    ///
    /// This allows access to backend-specific functionality that's not
    /// part of the generic `Backend` trait. The raw layer (#605): the
    /// backend's methods take and answer bare `TaggedValue`s and
    /// environments, which nothing roots between calls.
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

/// Convenience type alias for interpreter with tree-walking backend
///
/// This explicitly selects the tree-walker. The recommended default is
/// `VmInterpreter`, matching the CLI.
///
/// # Example
///
/// ```
/// use patina_interpreter::TreeWalkInterpreter;
/// let interp = TreeWalkInterpreter::new_tree_walker();
/// let value = interp.eval_str_owned("42").unwrap();
/// assert_eq!(interp.display_tagged(&value), "42");
/// ```
#[cfg(feature = "tree-walker")]
pub type TreeWalkInterpreter = Interpreter<TreeWalker>;

// Specialized implementation for TreeWalker backend
#[cfg(feature = "tree-walker")]
impl Interpreter<TreeWalker> {
    /// Create a new interpreter with the TreeWalker backend
    ///
    /// This initializes an interpreter with full R7RS continuation support
    /// including call/cc, dynamic-wind, and exception handling.
    ///
    /// This is a convenience method that's equivalent to:
    /// ```
    /// use patina_interpreter::{Interpreter, TreeWalker};
    /// let interp = Interpreter::new(TreeWalker::new());
    /// ```
    pub fn new_tree_walker() -> Self {
        Self::new(TreeWalker::new())
    }

    /// Create an interpreter with a custom filesystem.
    pub fn new_tree_walker_with_fs(fs: std::sync::Arc<dyn patina_core::FileSystem>) -> Self {
        Self::new(TreeWalker::with_fs(fs))
    }

    /// Create an interpreter from an existing evaluator (TreeWalker-specific)
    ///
    /// This is useful for tests that need to configure the evaluator
    /// before creating the interpreter (e.g., adding search paths).
    ///
    /// This method is only available when using the TreeWalker backend.
    pub fn from_evaluator(evaluator: Evaluator) -> Self {
        Interpreter {
            backend: TreeWalker::from_evaluator(evaluator),
        }
    }

    /// Get a reference to the underlying evaluator (TreeWalker-specific)
    ///
    /// This provides access to evaluator-specific functionality.
    /// For generic backend access, use `backend()` instead. The raw layer, as
    /// `backend()` is (#605).
    ///
    /// This method is only available when using the TreeWalker backend.
    pub fn evaluator(&self) -> &Evaluator {
        self.backend.evaluator()
    }
}

// Implement Default only for TreeWalker backend
#[cfg(feature = "tree-walker")]
impl Default for Interpreter<TreeWalker> {
    fn default() -> Self {
        Self::new_tree_walker()
    }
}

/// Recommended embedding type, using the same VM backend as the CLI.
#[cfg(feature = "vm")]
pub type VmInterpreter = Interpreter<VmBackend>;

#[cfg(feature = "vm")]
impl Interpreter<VmBackend> {
    /// Construct the recommended VM interpreter.
    pub fn new_vm() -> Self {
        Self::new(VmBackend::new())
    }

    /// Construct a VM interpreter with a host-supplied filesystem.
    pub fn new_vm_with_fs(fs: std::sync::Arc<dyn patina_core::FileSystem>) -> Self {
        Self::new(VmBackend::with_fs(fs))
    }
}

/// Combined error type for the interpreter
///
/// Generic over the backend error type, allowing different backends
/// to provide their own error types while maintaining a consistent
/// high-level error API.
#[derive(Debug, thiserror::Error)]
pub enum InterpreterError<E: std::error::Error> {
    #[error("Parse error: {0}")]
    Parse(ParseError),

    #[error("Lex error: {0}")]
    Lex(LexError),

    #[error("Desugar error: {0}")]
    Desugar(DesugarError),

    #[error("Backend error: {0}")]
    Backend(E),
}

// From implementations for frontend errors
impl<E: std::error::Error> From<ParseError> for InterpreterError<E> {
    fn from(e: ParseError) -> Self {
        InterpreterError::Parse(e)
    }
}

impl<E: std::error::Error> From<LexError> for InterpreterError<E> {
    fn from(e: LexError) -> Self {
        InterpreterError::Lex(e)
    }
}

impl<E: std::error::Error> From<DesugarError> for InterpreterError<E> {
    fn from(e: DesugarError) -> Self {
        InterpreterError::Desugar(e)
    }
}

impl<E: std::error::Error + patina_runtime::HasDiagnostic> patina_runtime::HasDiagnostic
    for InterpreterError<E>
{
    fn diagnostic(&self) -> patina_runtime::Diagnostic {
        match self {
            Self::Parse(error) => error.diagnostic(),
            Self::Lex(error) => patina_runtime::Diagnostic::new(
                patina_runtime::DiagnosticKind::Parse,
                error.to_string(),
            ),
            Self::Desugar(error) => error.diagnostic(),
            Self::Backend(error) => error.diagnostic(),
        }
    }
}

#[cfg(all(test, feature = "tree-walker"))]
#[expect(
    clippy::disallowed_methods,
    reason = "unit tests evaluate through the interpreter from outside any loop, as an embedder \
              does, and none reads a value from one evaluation after another that may collect \
              (#605's shape)"
)]
// The deprecated bare-value forms, which these tests still cover until stage
// 5e removes them (#605).
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn test_interpreter_with_tree_walker() {
        let interp = Interpreter::new(TreeWalker::new());
        let result = interp.eval_str("42").unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_tree_walk_interpreter_alias() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("42").unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_interpreter_default() {
        let interp = TreeWalkInterpreter::default();
        let result = interp.eval_str("(+ 1 2)").unwrap();
        assert_eq!(result.as_fixnum(), Some(3));
    }

    #[test]
    fn test_eval_program() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program("(define x 10) (define y 20) (+ x y)")
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(30));
    }

    #[test]
    fn test_eval_str_quote() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("'hello").unwrap();
        assert_eq!(interp.display_tagged(result), "hello");
    }

    #[test]
    fn test_eval_str_if() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("(if #t 1 2)").unwrap();
        assert_eq!(result.as_fixnum(), Some(1));

        let result = interp.eval_str("(if #f 1 2)").unwrap();
        assert_eq!(result.as_fixnum(), Some(2));
    }

    #[test]
    fn test_eval_lambda() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program("(define f (lambda (x) (+ x 1))) (f 41)")
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_eval_begin() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("(begin 1 2 3)").unwrap();
        assert_eq!(result.as_fixnum(), Some(3));
    }

    #[test]
    fn test_eval_set() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_program("(define x 10) (set! x 42) x").unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_eval_str_tracked() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str_tracked("(+ 1 2 3)").unwrap();
        assert_eq!(result.as_fixnum(), Some(6));
    }

    #[test]
    fn test_eval_program_tracked() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program_tracked("(define x 10) (define y 20) (+ x y)")
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(30));
    }

    #[test]
    fn test_eval_program_resilient_tracked() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_program_resilient_tracked("(+ 1 2)");
        assert_eq!(result.as_fixnum(), Some(3));
    }

    #[test]
    fn test_source_info_desugarer_integration() {
        // Verify that source info flows from parser through desugarer
        let backend = TreeWalker::new();
        let heap = backend.global_env().heap().clone();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let mut parser =
            Parser::new_with_source_map("(+ 1 2)", heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);

        // Desugar with source map
        let env = backend.global_env().clone();
        let desugarer = Desugarer::with_env_and_source_map(env, sm);
        let internal_heap = backend.global_env().heap();
        let core_expr = desugarer.desugar_tagged(expr, internal_heap).unwrap();

        // The CoreExpr should have source info from the list form
        assert!(
            core_expr.source.is_some(),
            "CoreExpr should have source location"
        );
        let loc = core_expr.source.as_ref().unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 1);
        assert_eq!(&*loc.source, "test.scm");
    }

    #[test]
    fn test_eval_error_with_location_tracked() {
        // When using tracked eval, undefined variable errors inside call forms carry source location.
        // The identifier span is preserved through the CPS trivial-value path.
        let interp = TreeWalkInterpreter::new_tree_walker();
        // The unbound identifier starts at column 7.
        let err = interp
            .eval_str_tracked("(list undefined-variable)")
            .unwrap_err();
        if let InterpreterError::Backend(eval_err) = &err {
            // The error should carry a source location from the identifier
            assert!(
                eval_err.source_location().is_some(),
                "tracked eval error should have source location, got: {eval_err}"
            );
            let loc = eval_err.source_location().unwrap();
            assert_eq!(loc.line, 1);
            assert_eq!(loc.column, 7);
            assert_eq!(loc.length, Some(18));
        } else {
            panic!("Expected backend error, got: {err:?}");
        }
    }

    #[test]
    fn test_format_error_with_source() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("<eval>");
        let input = "(+ 1 bad-var)";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.evaluator().global_env.clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        if let Err(eval_err) = result {
            let formatted = format_error_with_source(&eval_err, &sm.borrow());
            // Should include the caret context since source text was stored
            assert!(
                formatted.contains("bad-var") || formatted.contains("at"),
                "formatted error should contain context: {formatted}"
            );
        } else {
            panic!("Expected eval error for undefined variable");
        }
    }

    #[test]
    fn test_source_map_format_context() {
        let mut sm = SourceMap::new();
        sm.set_source_text("(define (foo) x)\n(foo)".to_string());
        let loc = patina_core::error::SourceLocation::new("<test>", 1, 15);
        let ctx = sm.format_context(&loc);
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert!(ctx.contains("(define (foo) x)"), "should show source line");
        assert!(ctx.contains('^'), "should show caret");
    }

    #[test]
    fn test_expansion_chain_single_macro() {
        // (let ((x 1)) (+ x y)) with undefined y should show "macro expansion: let"
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(let ((x 1)) (+ x y))";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.backend().global_env().clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        assert!(result.is_err(), "expected error for undefined y");
        let eval_err = result.unwrap_err();
        let formatted = format_error_with_source(&eval_err, &sm.borrow());
        assert!(
            formatted.contains("macro expansion: let"),
            "should show macro expansion chain, got: {formatted}"
        );
    }

    #[test]
    fn test_expansion_chain_nested_macros() {
        // (cond (#t (+ 0 y))) — y is in a call position so it carries a source location.
        // cond expands to an if form; both cond and (potentially) if should appear in chain.
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(cond (#t (+ 0 y)))";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.backend().global_env().clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        assert!(result.is_err(), "expected error for undefined y");
        let eval_err = result.unwrap_err();
        let formatted = format_error_with_source(&eval_err, &sm.borrow());
        // cond expands first — at minimum "macro expansion: cond" or a chain should appear
        assert!(
            formatted.contains("macro expansion"),
            "should show macro expansion info, got: {formatted}"
        );
        assert!(
            formatted.contains("cond"),
            "should mention cond in expansion, got: {formatted}"
        );
    }

    #[test]
    fn test_no_regression_unexpanded_errors() {
        // Errors from non-macro code still show location without expansion line
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(+ 1 undefined-var)";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.backend().global_env().clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        assert!(result.is_err());
        let eval_err = result.unwrap_err();
        let formatted = format_error_with_source(&eval_err, &sm.borrow());
        // Should contain error message but NOT "macro expansion" line
        assert!(
            formatted.contains("undefined-var"),
            "should show variable name: {formatted}"
        );
        assert!(
            !formatted.contains("macro expansion"),
            "non-macro error should not show expansion chain: {formatted}"
        );
    }
}
