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
//! let result = interp.eval_program("(import (scheme base)) (define x 40) (list (+ x 2))").unwrap();
//! assert_eq!(interp.display_tagged(result), "(42)");
//! # }
//! ```
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
pub use patina_core::TaggedValue;
pub use patina_core::error::SourceLocation;
pub use patina_frontend::{
    DesugarError, Desugarer, LexError, Lexer, ParseError, Parser, SourceMap, prune_freed_locations,
};
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
/// let result = interp.eval_str("42").unwrap();
/// assert_eq!(result.as_fixnum(), Some(42));
/// # }
/// ```
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
    /// assert_eq!(interp.eval_str("42").unwrap().as_fixnum(), Some(42));
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

    /// Evaluate a string containing one Scheme expression.
    ///
    /// Uses the backend's evaluation strategy. Text after that expression is
    /// not evaluated, but it must still read: a remainder that ends inside a
    /// datum is an error rather than something to drop silently. Use
    /// `eval_program` to evaluate every form in a string. Source positions are
    /// recorded under `<eval>`; use the named variant to retain its source map.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "vm")]
    /// # {
    /// # use patina_interpreter::VmInterpreter;
    /// # let interp = VmInterpreter::new_vm();
    /// let result = interp.eval_str("42").unwrap();
    /// assert_eq!(result.as_fixnum(), Some(42));
    /// # }
    /// ```
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_str_with_source_name`: holds nothing"
    )]
    pub fn eval_str(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_str_with_source_name(input, "<eval>").0
    }

    /// Evaluate multiple expressions from a string, returning the last result
    ///
    /// This is useful for evaluating entire programs or test files.
    /// Each expression is parsed and evaluated in sequence, with the result
    /// of the last expression being returned.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_with_source_name`: holds nothing"
    )]
    pub fn eval_program(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_program_with_source_name(input, "<eval>").0
    }

    /// Evaluate multiple expressions from a string, continuing on errors
    ///
    /// Unlike `eval_program`, this method does not stop on the first error.
    /// Instead, it prints errors to stderr and continues with the next expression.
    /// This is useful for test suites where you want to see all failures.
    ///
    /// Returns the last successfully evaluated result, or Unspecified if all failed.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_resilient_tracked`: holds nothing"
    )]
    pub fn eval_program_resilient(&self, input: &str) -> TaggedValue {
        self.eval_program_resilient_tracked(input)
    }

    /// Evaluate a string containing one expression with its source positions,
    /// naming the source `source_name`, and return the source map that placed
    /// it for formatting an error. See [`Interpreter::eval_str`].
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
                      map, which records positions, not heap values; the parser is dropped and the \
                      datum is not read after the call"
        )]
        let result = self
            .backend
            .eval_with_source_map(expr, env, &source_map)
            .map_err(InterpreterError::Backend);
        (result, source_map)
    }

    /// [`Interpreter::eval_str_with_source_name`] for a source named `<eval>`.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_str_with_source_name`: holds nothing"
    )]
    pub fn eval_str_tracked(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_str_with_source_name(input, "<eval>").0
    }

    /// Evaluate a program (multiple expressions) with its source positions,
    /// naming the source `source_name`; stop at the first error, and return
    /// the source map for formatting it.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_with_fold_case`: holds nothing"
    )]
    pub fn eval_program_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> WithSourceMap<Result<TaggedValue, InterpreterError<B::Error>>> {
        self.eval_program_with_fold_case(input, source_name, &mut false)
    }

    /// Evaluate one interactive submission, retaining directives for the next.
    /// Each submission has its own source positions. Only directives actually
    /// consumed before an error survive; parser lookahead must not change them.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_in_env` in the global environment: holds nothing"
    )]
    pub fn eval_program_with_fold_case(
        &self,
        input: &str,
        source_name: &str,
        fold_case: &mut bool,
    ) -> WithSourceMap<Result<TaggedValue, InterpreterError<B::Error>>> {
        self.eval_program_in_env(input, source_name, fold_case, self.backend.global_env())
    }

    fn eval_program_in_env(
        &self,
        input: &str,
        source_name: &str,
        fold_case: &mut bool,
        env: &Rc<Environment>,
    ) -> WithSourceMap<Result<TaggedValue, InterpreterError<B::Error>>> {
        let (value, end, source_map) =
            self.run_forms(input, source_name, fold_case, env, |error, _| Some(error));
        let result = match end {
            FormsEnd::Read => Ok(value),
            FormsEnd::Stopped(error) => Err(InterpreterError::Backend(error)),
            FormsEnd::Unreadable(error) => Err(error.into()),
        };
        (result, source_map)
    }

    /// [`Interpreter::eval_program_with_source_name`] for a source named
    /// `<eval>`.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `eval_program_with_source_name`: holds nothing"
    )]
    pub fn eval_program_tracked(
        &self,
        input: &str,
    ) -> Result<TaggedValue, InterpreterError<B::Error>> {
        self.eval_program_with_source_name(input, "<eval>").0
    }

    /// Evaluate a program with source positions, printing each error and
    /// continuing past evaluation errors, as [`Interpreter::eval_program_resilient`]
    /// does. Returns the last value evaluated.
    pub fn eval_program_resilient_tracked(&self, input: &str) -> TaggedValue {
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

    /// Evaluate a program with a named source, reporting each error and
    /// carrying on to the next top-level form: the CLI's `-k`.
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
    /// [`patina_runtime::exit_status::exit_if_interrupted`].
    pub fn eval_program_resilient_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> (TaggedValue, ProgramOutcome)
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

    /// Read `input` a form at a time and evaluate each with its source
    /// positions: the one loop behind all program entry points.
    ///
    /// An evaluation error goes to `on_error`, with the source map for
    /// formatting it, which gives the error back to stop there or returns
    /// `None` to carry on with the next form. A read error always stops: the
    /// parser leaves the offending token where it was, so reading on would
    /// report it again without end. Returns the last value evaluated, how
    /// reading ended, and the source map.
    fn run_forms(
        &self,
        input: &str,
        source_name: &str,
        fold_case: &mut bool,
        env: &Rc<Environment>,
        mut on_error: impl FnMut(B::Error, &SourceMap) -> Option<B::Error>,
    ) -> (TaggedValue, FormsEnd<B::Error>, Rc<RefCell<SourceMap>>) {
        let mut value = TaggedValue::UNSPECIFIED;
        let heap = self.backend.global_env().heap();
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
            // Drop SourceMap entries for slots the previous form's evaluation
            // freed, before this iteration's parse can reuse them (§9.1).
            prune_freed_locations(heap, &source_map);
            let datum = parser.parse_next();
            *fold_case = parser.read_state().fold_case;
            match datum {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the outermost entry, so the backend's loop may collect. The parser \
                              holds no heap value between data (it clears its labels after each), \
                              and the source map is pruned of freed slots before each read \
                              (docs/GC_DESIGN.md §9.1). `value`, the last result, is held across \
                              the form unrooted: overwritten if the form succeeds, and returned \
                              stale only when every later form fails under `-k`, which the CLI \
                              drops; an embedder that keeps it meets #605"
                )]
                Ok(Some(expr)) => match self.backend.eval_with_source_map(expr, env, &source_map) {
                    Ok(result) => value = result,
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

    /// The interpreter's global environment, shared with its backend.
    pub fn global_env(&self) -> Rc<Environment> {
        self.backend.global_env().clone()
    }

    /// Format a TaggedValue for display using write notation (machine-readable)
    ///
    /// Uses the datum writer which properly handles all TaggedValue types
    /// including heap pairs, vectors, strings, and circular structures.
    /// Multiple values (from `values`) are unpacked and displayed one per line.
    pub fn display_tagged(&self, tv: TaggedValue) -> String {
        use patina_primitives::primitives::io::datum_writer::format_write_tagged;
        let heap = self.backend.global_env().heap();

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
    /// part of the generic `Backend` trait.
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
/// assert_eq!(interp.eval_str("42").unwrap().as_fixnum(), Some(42));
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
    /// For generic backend access, use `backend()` instead.
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
              does"
)]
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
    fn test_source_stamp_inner_forms() {
        // Inner pairs of a let expansion should have source in SourceMap after expansion
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(let ((x 1)) (+ x 2))";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name.clone(), sm.clone())
                .unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.backend().global_env().clone();
        interp
            .backend()
            .eval_with_source_map(expr, &global, &sm)
            .unwrap();
        // Expanded syntax retains its own history at the invocation position.
        let sm_ref = sm.borrow();
        assert!(
            sm_ref.iter_locations().any(|loc| loc.line == 1
                && loc.column == 1
                && sm_ref
                    .get_expansions(loc)
                    .is_some_and(|names| names.contains(&"let".into()))),
            "expanded inner forms should retain the 'let' invocation"
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
