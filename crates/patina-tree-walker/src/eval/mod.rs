// Module declarations
mod application;
mod core_eval;
mod debug;
mod error;
mod primitives;
// Note: special_forms module removed - all special forms now in CoreExpr!

// Re-export error type for public API
pub use core_eval::eval_core;
pub use error::EvalError;

use debug::DebugConfig;
use patina_runtime::CoreExpr;
use patina_runtime::environment::Environment;
use patina_runtime::library_loader::LibraryLoaderRegistry;
use patina_runtime::library_registry::LibraryRegistry;
use patina_runtime::value::{LambdaBody, Procedure, Value};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Result of evaluation step in the trampoline
///
/// The trampoline pattern enables tail call optimization by converting
/// recursive calls into an iterative loop. Instead of making recursive
/// calls that grow the stack, tail positions return `TailCall` which
/// tells the trampoline to continue with the next computation.
#[derive(Debug)]
pub enum EvalResult {
    /// Final value - evaluation complete
    Value(Value),
    /// Tail call - continue trampolining with this expression and environment
    TailCall { expr: Value, env: Rc<Environment> },
    /// Tail call to a primitive procedure with already-evaluated arguments
    /// This enables primitives like call-with-values to participate in tail call optimization
    /// The procedure and arguments are already evaluated, so we just need to apply them
    TailCallPrimitive { proc: Value, args: Vec<Value> },
}

pub struct Evaluator {
    pub global_env: Rc<Environment>,
    pub(crate) debug: Rc<DebugConfig>,
    /// Registry of loaded libraries
    pub(crate) library_registry: RefCell<LibraryRegistry>,
    /// Registry of library loaders (Rust, Scheme, etc.)
    pub(crate) loader_registry: RefCell<LibraryLoaderRegistry>,
    /// Registry of primitive procedures
    pub(crate) primitive_registry: primitives::PrimitiveRegistry,
    // Note: special_form_registry removed - all special forms now in CoreExpr!
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());

        // Create primitive registry and register all primitives
        let mut primitive_registry = primitives::PrimitiveRegistry::new();
        Self::register_all_primitives(&mut primitive_registry);

        // Note: Primitives are now installed via library loading rather than
        // direct installation. The load_bootstrap() call below will load (scheme base)
        // and import it into the global environment.

        // Note: special_form_registry removed - all special forms now in CoreExpr!

        // Create library registries
        let library_registry = RefCell::new(LibraryRegistry::with_default_paths());
        let loader_registry = RefCell::new(LibraryLoaderRegistry::new());

        let evaluator = Evaluator {
            global_env,
            debug: Rc::new(DebugConfig::new()),
            library_registry,
            loader_registry,
            primitive_registry,
        };

        // Initialize library loaders
        evaluator.init_loaders();

        // Load bootstrap library
        evaluator.load_bootstrap();

        evaluator
    }

    /// Initialize library loaders
    ///
    /// Sets up the loader registry with:
    /// 1. RustLibraryLoader for built-in libraries (scheme base, etc.)
    /// 2. SchemeLibraryLoader for .sld files
    fn init_loaders(&self) {
        use crate::library_support::SchemeLibraryLoader;
        use patina_runtime::{RustLibraryLoader, stdlib};

        let mut loaders = self.loader_registry.borrow_mut();

        // Create Rust loader and register standard libraries
        let mut rust_loader = RustLibraryLoader::with_standard_libraries();

        // Register all R7RS standard libraries
        rust_loader.register(
            vec!["scheme".to_string(), "base".to_string()],
            stdlib::build_scheme_base,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "char".to_string()],
            stdlib::build_scheme_char,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "complex".to_string()],
            stdlib::build_scheme_complex,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "inexact".to_string()],
            stdlib::build_scheme_inexact,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "lazy".to_string()],
            stdlib::build_scheme_lazy,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "time".to_string()],
            stdlib::build_scheme_time,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "file".to_string()],
            stdlib::build_scheme_file,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "read".to_string()],
            stdlib::build_scheme_read,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "write".to_string()],
            stdlib::build_scheme_write,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "eval".to_string()],
            stdlib::build_scheme_eval,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "process-context".to_string()],
            stdlib::build_scheme_process_context,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "r5rs".to_string()],
            stdlib::build_scheme_r5rs,
        );

        // Register test framework primitives
        // The full (chibi test) library is in lib/chibi/test.sld
        rust_loader.register(
            vec![
                "chibi".to_string(),
                "test".to_string(),
                "primitives".to_string(),
            ],
            stdlib::build_chibi_test_primitives,
        );

        // Register Patina extensions
        rust_loader.register(
            vec!["patina".to_string(), "debug".to_string()],
            stdlib::build_patina_debug,
        );

        // Add Rust loader first (highest priority)
        loaders.add_loader(Box::new(rust_loader));

        // Add Scheme loader second (for .sld files)
        // Note: SchemeLibraryLoader is now stateless and uses EvaluatingLibraryLoader trait
        loaders.add_evaluating_loader(Box::new(SchemeLibraryLoader::new()));
    }

    fn load_bootstrap(&self) {
        // Load (scheme base) library
        // This will load Rust primitives and automatically load base-extras.scm
        let _ = self.load_library(&["scheme".to_string(), "base".to_string()]);

        // Load test framework (test-extras.scm contains the 'test' macro)
        let _ = self.load_library(&["chibi".to_string(), "test".to_string()]);

        // Load Patina debugging utilities
        // Auto-loaded in REPL for convenience (commonly used during development)
        let _ = self.load_library(&["patina".to_string(), "debug".to_string()]);

        // After loading libraries, import (scheme base) into global environment
        // This makes primitives and macros available without explicit import
        // (R7RS-style convenience for REPL)
        if let Some(lib) = self
            .library_registry
            .borrow()
            .get(&["scheme".to_string(), "base".to_string()])
        {
            for (name, value) in lib.exports.iter() {
                self.global_env.define(name.clone(), value.clone());
            }
        }

        // Import (patina debug) into global environment for REPL convenience
        if let Some(lib) = self
            .library_registry
            .borrow()
            .get(&["patina".to_string(), "debug".to_string()])
        {
            for (name, value) in lib.exports.iter() {
                self.global_env.define(name.clone(), value.clone());
            }
        }

        // Import (chibi test) into global environment for testing convenience
        if let Some(lib) = self
            .library_registry
            .borrow()
            .get(&["chibi".to_string(), "test".to_string()])
        {
            for (name, value) in lib.exports.iter() {
                self.global_env.define(name.clone(), value.clone());
            }
        }
    }

    /// Load Scheme-implemented extras for a library
    ///
    /// After loading Rust primitives, this checks for a corresponding `-extras.scm` file.
    /// If found, it's evaluated in the library's environment.
    ///
    /// Convention:
    /// - (scheme base) → lib/scheme/base-extras.scm
    /// - (scheme lazy) → lib/scheme/lazy-extras.scm
    ///
    /// Note: Some libraries are fully defined as .sld files:
    /// - (scheme case-lambda) → lib/scheme/case-lambda.sld
    /// - (chibi test) → lib/chibi/test.sld
    ///
    /// This allows any library to have:
    /// - Rust primitives (performance-critical operations)
    /// - Scheme code (derived forms, macros, convenience functions)
    fn load_library_extras(&self, name: &[String]) {
        if name.is_empty() {
            return;
        }

        // Construct relative extras file path: (scheme base) → scheme/base-extras.scm
        let mut relative_path = std::path::PathBuf::new();
        for part in &name[..name.len() - 1] {
            relative_path.push(part);
        }
        relative_path.push(format!("{}-extras.scm", name.last().unwrap()));

        // Search for the extras file in all library search paths
        let search_paths = self.library_search_paths();
        let extras_path = search_paths
            .iter()
            .map(|base| base.join(&relative_path))
            .find(|path| path.exists());

        let extras_path = match extras_path {
            Some(path) => path,
            None => return, // No extras file - that's fine
        };

        // Read the extras file
        let extras_content = match std::fs::read_to_string(&extras_path) {
            Ok(content) => content,
            Err(_) => return, // Can't read file - silently skip
        };

        // Get the library's environment
        let lib_env = {
            let registry = self.library_registry.borrow();
            match registry.get(name) {
                Some(lib) => lib.env.clone(),
                None => {
                    tracing::warn!(
                        library = ?name,
                        path = %extras_path.display(),
                        "Library not loaded, cannot load extras"
                    );
                    return;
                }
            }
        };

        // Create an evaluation environment that has access to (scheme base) primitives
        // The extras file needs these for macro definitions (display, write, newline, etc.)
        let eval_env = {
            // Start with library environment as base
            let env = Rc::new(Environment::new());

            // Import all (scheme base) exports if this isn't (scheme base) itself
            let is_scheme_base = name == ["scheme".to_string(), "base".to_string()];
            if !is_scheme_base {
                let registry = self.library_registry.borrow();
                if let Some(scheme_base) = registry.get(&["scheme".to_string(), "base".to_string()])
                {
                    // Add all (scheme base) exports to the evaluation environment
                    for (export_name, value) in &scheme_base.exports {
                        env.define(export_name.clone(), value.clone());
                    }
                }
            }

            // Add all library's own definitions (these can shadow scheme base)
            for (binding_name, value) in lib_env.bindings() {
                env.define(binding_name, value);
            }

            env
        };

        // Parse and evaluate all expressions in the evaluation environment
        let mut parser = match patina_frontend::Parser::new(&extras_content) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    path = %extras_path.display(),
                    error = ?e,
                    "Failed to parse extras file"
                );
                return;
            }
        };

        // Create desugarer with environment for macro expansion
        let desugarer = patina_frontend::Desugarer::with_env(eval_env.clone());

        loop {
            match parser.parse() {
                Ok(expr) => {
                    // Desugar to CoreExpr
                    let core_expr = match desugarer.desugar(&expr) {
                        Ok(ce) => ce,
                        Err(e) => {
                            tracing::warn!(
                                path = %extras_path.display(),
                                error = %e,
                                "Failed to desugar expression in extras file"
                            );
                            continue;
                        }
                    };

                    // Evaluate using CoreExpr evaluator
                    if let Err(e) = eval_core(&core_expr, eval_env.clone(), self) {
                        tracing::warn!(
                            path = %extras_path.display(),
                            error = %e,
                            "Failed to evaluate expression in extras file"
                        );
                    }
                }
                Err(patina_frontend::ParseError::UnexpectedEof) => break,
                Err(e) => {
                    tracing::warn!(
                        path = %extras_path.display(),
                        error = ?e,
                        "Parse error in extras file"
                    );
                    break;
                }
            }
        }

        // Copy any new definitions from the evaluation environment back to the library environment
        for (binding_name, value) in eval_env.bindings() {
            // Skip (scheme base) primitives - only copy definitions created by the extras file
            let is_from_extras = {
                let registry = self.library_registry.borrow();
                if let Some(scheme_base) = registry.get(&["scheme".to_string(), "base".to_string()])
                {
                    !scheme_base.exports.contains_key(&binding_name)
                } else {
                    true // If (scheme base) not loaded, copy everything
                }
            };

            if is_from_extras {
                lib_env.define(binding_name, value);
            }
        }
    }
    fn eval_step(&self, expr: &Value, env: &Rc<Environment>) -> Result<EvalResult, EvalError> {
        // Top-level trampoline evaluations are in tail position!
        // This allows the trampoline to bounce tail calls.
        self.eval_step_impl(expr, env, true)
    }

    /// Evaluate one step of an already macro-expanded expression
    ///
    /// Like `eval_step()`, but assumes macros have already been expanded.
    /// Calls `eval_list_impl_expanded()` instead of `eval_list_impl()`.
    fn eval_step_expanded(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
    ) -> Result<EvalResult, EvalError> {
        self.eval_step_impl_expanded(expr, env, true)
    }

    /// Implementation of eval_step with tail position tracking
    fn eval_step_impl(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Debug trace entry
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            tracing::debug!(
                target: "patina_tree_walker::eval",
                indent = %self.debug.current_indent(),
                expr = %expr,
                tail = in_tail_position,
                "Evaluating"
            );
            self.debug.indent();
        }

        let result = match expr {
            // Self-evaluating
            Value::Boolean(_)
            | Value::Integer(_)
            | Value::BigInteger(_)
            | Value::Rational(_)
            | Value::Real(_)
            | Value::Complex(_)
            | Value::Character(_)
            | Value::String(_)
            | Value::Vector(_)
            | Value::Bytevector(_) => Ok(EvalResult::Value(expr.clone())),

            // Variable lookup
            Value::Symbol(name) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    tracing::trace!(
                        target: "patina_tree_walker::env",
                        indent = %self.debug.current_indent(),
                        name = %name,
                        "Symbol lookup"
                    );
                }

                // Look up in current environment
                if let Some(value) = env.get(name) {
                    return Ok(EvalResult::Value(value));
                }

                Err(EvalError::UndefinedVariable(name.to_string()))
            }

            // Identifier lookup (Racket-style scope sets)
            // Uses scope-based lookup for hygienic binding resolution
            Value::Identifier(id) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    tracing::trace!(
                        target: "patina_tree_walker::env",
                        indent = %self.debug.current_indent(),
                        name = %id.name,
                        scopes = %id.scopes,
                        "Identifier lookup"
                    );
                }

                // Use scope-based lookup if scopes are present
                if !id.scopes.is_empty() {
                    if let Some(value) = env.get_with_scopes(&id.name, &id.scopes) {
                        return Ok(EvalResult::Value(value));
                    }
                    return Err(EvalError::UndefinedVariable(format!(
                        "{} (identifier with scopes {})",
                        id.name, id.scopes
                    )));
                }

                // Empty scopes: fall back to simple name lookup
                if let Some(value) = env.get(&id.name) {
                    return Ok(EvalResult::Value(value));
                }

                Err(EvalError::UndefinedVariable(id.name.to_string()))
            }

            // Empty list
            Value::Null => Ok(EvalResult::Value(Value::Null)),

            // Lists (procedure calls or special forms)
            Value::Pair(_) => self.eval_list_impl(expr, env, in_tail_position),

            _ => Ok(EvalResult::Value(expr.clone())),
        };

        // Debug trace exit
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            self.debug.dedent();
            match &result {
                Ok(EvalResult::Value(val)) => {
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        result = %val,
                        "=> value"
                    );
                }
                Ok(EvalResult::TailCall { expr, .. }) => {
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        expr = %expr,
                        "=> tail call"
                    );
                }
                Ok(EvalResult::TailCallPrimitive { proc, args }) => {
                    let args_str = args
                        .iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        proc = %proc,
                        args = %args_str,
                        "=> tail call primitive"
                    );
                }
                Err(e) => {
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        error = %e,
                        "=> error"
                    );
                }
            }
        }

        result
    }

    /// Evaluate a list (procedure call or special form) with tail position tracking
    ///
    /// All forms are routed through the CoreExpr pipeline. The desugarer handles:
    /// - Special forms (quote, if, lambda, define, set!, begin, etc.)
    /// - Macro expansion (macros are expanded during desugaring)
    /// - Procedure applications
    fn eval_list_impl(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        use patina_frontend::Desugarer;

        // Route ALL forms through CoreExpr - unified evaluation path
        let desugarer = Desugarer::with_env(env.clone());
        let core_expr = desugarer.desugar(expr).map_err(|e| {
            EvalError::InternalError(format!("Failed to desugar expression: {}", e))
        })?;

        let value = eval_core(&core_expr, env.clone(), self)?;
        Ok(EvalResult::Value(value))
    }

    /// Implementation of eval_step for already macro-expanded code
    ///
    /// This is identical to `eval_step_impl()` except it calls `eval_list_impl_expanded()`
    /// instead of `eval_list_impl()` for lists.
    fn eval_step_impl_expanded(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Debug trace entry
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            tracing::debug!(
                target: "patina_tree_walker::eval",
                indent = %self.debug.current_indent(),
                expr = %expr,
                tail = in_tail_position,
                "Evaluating (expanded)"
            );
            self.debug.indent();
        }

        let result = match expr {
            // Self-evaluating
            Value::Boolean(_)
            | Value::Integer(_)
            | Value::BigInteger(_)
            | Value::Rational(_)
            | Value::Real(_)
            | Value::Complex(_)
            | Value::Character(_)
            | Value::String(_)
            | Value::Vector(_)
            | Value::Bytevector(_) => Ok(EvalResult::Value(expr.clone())),

            // Variable lookup
            Value::Symbol(name) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    tracing::trace!(
                        target: "patina_tree_walker::env",
                        indent = %self.debug.current_indent(),
                        name = %name,
                        "Symbol lookup"
                    );
                }

                // Look up in current environment
                if let Some(value) = env.get(name) {
                    return Ok(EvalResult::Value(value));
                }

                Err(EvalError::UndefinedVariable(name.to_string()))
            }

            // Identifier lookup (Racket-style scope sets)
            Value::Identifier(id) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    tracing::trace!(
                        target: "patina_tree_walker::env",
                        indent = %self.debug.current_indent(),
                        name = %id.name,
                        scopes = %id.scopes,
                        "Identifier lookup"
                    );
                }

                // Use scope-based lookup if scopes are present
                if !id.scopes.is_empty() {
                    if let Some(value) = env.get_with_scopes(&id.name, &id.scopes) {
                        return Ok(EvalResult::Value(value));
                    }
                    return Err(EvalError::UndefinedVariable(format!(
                        "{} (identifier with scopes {})",
                        id.name, id.scopes
                    )));
                }

                // Empty scopes: fall back to simple name lookup
                if let Some(value) = env.get(&id.name) {
                    return Ok(EvalResult::Value(value));
                }

                Err(EvalError::UndefinedVariable(id.name.to_string()))
            }

            // Empty list
            Value::Null => Ok(EvalResult::Value(Value::Null)),

            // Lists (procedure calls or special forms) - use expanded version
            Value::Pair(_) => self.eval_list_impl_expanded(expr, env, in_tail_position),

            _ => Ok(EvalResult::Value(expr.clone())),
        };

        // Debug trace exit
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            self.debug.dedent();
            match &result {
                Ok(EvalResult::Value(val)) => {
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        result = %val,
                        "=> value"
                    );
                }
                Ok(EvalResult::TailCall { expr, .. }) => {
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        expr = %expr,
                        "=> tail call"
                    );
                }
                Ok(EvalResult::TailCallPrimitive { proc, args }) => {
                    let args_str = args
                        .iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        proc = %proc,
                        args = %args_str,
                        "=> tail call primitive"
                    );
                }
                Err(e) => {
                    tracing::debug!(
                        target: "patina_tree_walker::eval",
                        indent = %self.debug.current_indent(),
                        error = %e,
                        "=> error"
                    );
                }
            }
        }

        result
    }

    /// Evaluate a list for already macro-expanded code
    ///
    /// This is identical to `eval_list_impl()` except it SKIPS the macro expansion check.
    /// All macros must have been expanded before calling this.
    fn eval_list_impl_expanded(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (car, cdr) = self.extract_pair(expr)?;

        // Note: All special forms now in CoreExpr - no registry lookup needed!

        // SKIP macro expansion check - macros already expanded!
        // (Lines 534-587 from eval_list_impl are omitted here)

        // Regular procedure call - this can be a tail call if in tail position
        // Use eval_expanded to avoid re-expanding macros
        let proc = self.eval_expanded(&car, env)?;
        let args = self.eval_arguments_expanded(&cdr, env)?;

        // Check if this is a lambda in tail position
        if in_tail_position
            && let Value::Procedure(proc_box) = &proc
            && let Procedure::Lambda {
                params,
                variadic,
                body,
                env: lambda_env,
                binding_scope,
            } = proc_box.as_ref()
        {
            // Tail call to lambda - use shared helper methods
            let new_env =
                self.prepare_lambda_env(params, variadic, args, lambda_env, *binding_scope)?;
            return self.eval_lambda_body(body, &new_env, true);
        }

        // Not in tail position, or not a lambda - just apply normally
        self.apply(proc, args, in_tail_position)
    }

    /// Expand all macros in an expression without evaluating
    ///
    /// This recursively expands any macro calls in the expression, but does not
    /// evaluate the result. This is needed for the CoreExpr pipeline, which requires
    /// macro-expanded code before desugaring.
    ///
    /// Returns the fully macro-expanded expression.
    pub fn expand_all_macros(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        match expr {
            // Self-evaluating values - no macros to expand
            Value::Integer(_)
            | Value::BigInteger(_)
            | Value::Rational(_)
            | Value::Real(_)
            | Value::Complex(..)
            | Value::Boolean(_)
            | Value::Character(_)
            | Value::String(_)
            | Value::Bytevector(_)
            | Value::Procedure(_)
            | Value::Macro { .. }
            | Value::Port(_)
            | Value::Parameter { .. }
            | Value::Library(_)
            | Value::Values(_)
            | Value::Promise(_)
            | Value::Null => Ok(expr.clone()),

            // Symbols - no expansion needed
            Value::Symbol(_) => Ok(expr.clone()),

            // Identifiers - no expansion needed (unified hygiene)
            Value::Identifier { .. } => Ok(expr.clone()),

            // Vectors - recursively expand elements
            Value::Vector(vec) => {
                let expanded: Result<Vec<_>, _> = vec
                    .borrow()
                    .iter()
                    .map(|elem| self.expand_all_macros(elem, env))
                    .collect();
                Ok(Value::Vector(Rc::new(RefCell::new(expanded?))))
            }

            // Lists - check for macro calls and special forms
            Value::Pair(pair) => {
                // First get car and cdr
                let (car, cdr) = pair.borrow().clone();

                // Check if this is a macro call
                // Note: Special forms now in CoreExpr - no registry check needed
                if let Value::Symbol(ref sym) = car {
                    // Check if it's a macro
                    if let Some(Value::Macro(compiled_macro)) = env.get(sym) {
                        // This is a macro call - expand it
                        let expanded = self.expand_macro(&compiled_macro, expr, env)?;

                        // Recursively expand the result (macros can expand to macros)
                        return self.expand_all_macros(&expanded, env);
                    }
                }

                // Not a macro call - recursively expand car and cdr
                let expanded_car = self.expand_all_macros(&car, env)?;
                let expanded_cdr = self.expand_all_macros(&cdr, env)?;

                Ok(Value::Pair(Rc::new(RefCell::new((
                    expanded_car,
                    expanded_cdr,
                )))))
            }

            // Unspecified, EOF, and LabelPlaceholder - shouldn't appear in user code
            // LabelPlaceholder is resolved during parsing
            Value::Unspecified | Value::Eof | Value::LabelPlaceholder(_) => Ok(expr.clone()),
        }
    }

    /// Evaluate an already macro-expanded expression in a specific environment
    ///
    /// This is like `eval_in_env()`, but assumes that all macros have already been expanded.
    /// Used by the CoreExpr pipeline after `expand_all_macros()` has been called.
    ///
    /// IMPORTANT: This skips macro expansion in `eval_list_impl()`. Only use this if you've
    /// already called `expand_all_macros()` on the expression.
    ///
    /// Like `eval()`, this uses the trampoline pattern for tail call optimization.
    pub fn eval_expanded(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let mut current_expr = expr.clone();
        let mut current_env = env.clone();

        // Trampoline loop for TCO
        loop {
            match self.eval_step_expanded(&current_expr, &current_env)? {
                EvalResult::Value(v) => return Ok(v),
                EvalResult::TailCall { expr, env } => {
                    current_expr = expr;
                    current_env = env;
                }
                EvalResult::TailCallPrimitive { proc, args } => {
                    match self.apply(proc, args, true)? {
                        EvalResult::Value(v) => return Ok(v),
                        EvalResult::TailCall { expr, env } => {
                            current_expr = expr;
                            current_env = env;
                        }
                        EvalResult::TailCallPrimitive { proc, args } => {
                            let mut app_list = vec![proc];
                            app_list.extend(args);
                            current_expr = self.list_from_vec(app_list);
                        }
                    }
                }
            }
        }
    }

    /// Evaluate an expression in a specific environment
    ///
    /// Used by special forms, primitives, library loading, and backend trait implementation.
    /// Public to allow library loaders to evaluate library bodies and backends to evaluate
    /// in specific environments.
    ///
    /// Like `eval()`, this uses the trampoline pattern for tail call optimization.
    pub fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let mut current_expr = expr.clone();
        let mut current_env = env.clone();

        // Trampoline loop for TCO
        loop {
            match self.eval_step(&current_expr, &current_env)? {
                EvalResult::Value(v) => return Ok(v),
                EvalResult::TailCall { expr, env } => {
                    current_expr = expr;
                    current_env = env;
                }
                EvalResult::TailCallPrimitive { proc, args } => {
                    match self.apply(proc, args, true)? {
                        EvalResult::Value(v) => return Ok(v),
                        EvalResult::TailCall { expr, env } => {
                            current_expr = expr;
                            current_env = env;
                        }
                        EvalResult::TailCallPrimitive { proc, args } => {
                            let mut app_list = vec![proc];
                            app_list.extend(args);
                            current_expr = self.list_from_vec(app_list);
                        }
                    }
                }
            }
        }
    }

    // ========== Lambda Application Helper Methods ==========
    // These methods extract common logic shared between:
    // - apply() in application.rs (general case)
    // - eval_list_impl() in this file (tail call optimization)

    /// Prepare lambda environment by checking arity and binding parameters
    ///
    /// Returns a new environment with all parameters bound to their arguments.
    /// Handles both fixed-arity and variadic lambdas.
    ///
    /// Hygiene handling:
    /// - If parameter has scopes (from macro expansion), use those for binding
    /// - If binding_scope is provided and parameter has no scopes, use that
    /// - Always create an unscoped binding for backward compatibility
    pub(crate) fn prepare_lambda_env(
        &self,
        params: &[patina_runtime::ScopedParam],
        variadic: &Option<patina_runtime::ScopedParam>,
        args: Vec<Value>,
        parent_env: &Rc<Environment>,
        binding_scope: Option<patina_runtime::ScopeId>,
    ) -> Result<Rc<Environment>, EvalError> {
        use patina_runtime::ScopeSet;

        // Check arity
        if variadic.is_some() {
            // Variadic: need at least as many args as fixed params
            if args.len() < params.len() {
                return Err(EvalError::WrongArity {
                    expected: format!("at least {}", params.len()),
                    actual: args.len(),
                });
            }
        } else {
            // Fixed arity: need exact number of args
            if args.len() != params.len() {
                return Err(EvalError::WrongArity {
                    expected: params.len().to_string(),
                    actual: args.len(),
                });
            }
        }

        // Create new environment with lambda's captured environment as parent
        let new_env = Rc::new(Environment::with_parent(parent_env.clone()));

        // Helper to define bindings with proper hygiene
        let define = |param: &patina_runtime::ScopedParam, value: Value| {
            let name = param.name.to_string();

            // Scoped binding for hygienic lookup (get_with_scopes)
            // Priority: use parameter's own scopes (from macro), else use binding_scope
            let scopes = if !param.scopes.is_empty() {
                // Parameter has scopes from macro expansion - use them!
                Some(param.scopes.clone())
            } else {
                // No macro scopes, use lambda's binding scope if available
                binding_scope.map(ScopeSet::singleton)
            };

            // KEY HYGIENE FIX: Only create simple bindings for non-macro parameters
            // If a parameter has scopes (macro-introduced), it should ONLY be visible
            // via scoped lookup, not simple lookup. This prevents macro-introduced
            // bindings from shadowing use-site variables with the same name.
            if !param.scopes.is_empty() {
                // Macro-introduced parameter: ONLY scoped binding
                if let Some(scope_set) = scopes {
                    new_env.define_with_scopes(name, scope_set, value);
                }
            } else {
                // Non-macro parameter: simple binding + optional scoped binding
                new_env.define(name.clone(), value.clone());
                if let Some(scope_set) = scopes {
                    new_env.define_with_scopes(name, scope_set, value);
                }
            }
        };

        // Bind fixed parameters
        for (param, arg) in params.iter().zip(args.iter()) {
            define(param, arg.clone());
        }

        // Bind rest parameter if variadic
        if let Some(rest_param) = variadic {
            // Collect remaining args into a list
            let rest_args: Vec<Value> = args.into_iter().skip(params.len()).collect();
            let rest_list = self.list_from_vec(rest_args);
            define(rest_param, rest_list);
        }

        Ok(new_env)
    }

    /// Evaluate lambda body (Vec<CoreExpr>)
    pub(crate) fn eval_lambda_body(
        &self,
        body: &LambdaBody,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        use crate::eval::core_eval::{CoreEvalResult, eval_core_step};

        if body.is_empty() {
            return Ok(EvalResult::Value(Value::Unspecified));
        }

        // Evaluate all but the last expression
        for expr in &body[..body.len() - 1] {
            eval_core_step(expr, env.clone(), self)?;
        }

        // Evaluate last expression
        let last = body.last().unwrap();
        let result = eval_core_step(last, env.clone(), self)?;

        match result {
            CoreEvalResult::Value(v) => Ok(EvalResult::Value(v)),
            CoreEvalResult::TailCall {
                expr,
                env: tail_env,
            } => {
                if in_tail_position {
                    // Convert CoreExpr to Value for tail call
                    use crate::eval::core_eval::core_expr_to_value;
                    Ok(EvalResult::TailCall {
                        expr: core_expr_to_value(&expr)?,
                        env: tail_env,
                    })
                } else {
                    // Not in tail position - must resolve the tail call
                    self.resolve_tail_call_core_rc(expr, &tail_env)
                }
            }
        }
    }

    /// Resolve a CoreExpr tail call when not in tail position
    /// Takes Rc<CoreExpr> to match TailCall representation
    fn resolve_tail_call_core_rc(
        &self,
        expr: Rc<CoreExpr>,
        env: &Rc<Environment>,
    ) -> Result<EvalResult, EvalError> {
        use crate::eval::core_eval::{CoreEvalResult, eval_core_step};

        let mut current_expr = expr;
        let mut current_env = env.clone();

        loop {
            match eval_core_step(&current_expr, current_env.clone(), self)? {
                CoreEvalResult::Value(v) => return Ok(EvalResult::Value(v)),
                CoreEvalResult::TailCall { expr, env } => {
                    current_expr = expr;
                    current_env = env;
                }
            }
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

// Library loading methods
impl Evaluator {
    /// Load a library by name
    ///
    /// This method:
    /// 1. Checks if the library is already loaded
    /// 2. If not, uses the loader registry to load it
    /// 3. Registers it in the library registry
    ///
    /// Returns the loaded library or an error.
    pub fn load_library(
        &self,
        name: &[String],
    ) -> Result<Rc<patina_runtime::Library>, patina_runtime::LibraryError> {
        // Check if already loaded
        {
            let registry = self.library_registry.borrow();
            if let Some(lib) = registry.get(name) {
                return Ok(Rc::new(lib.clone()));
            }
        }

        // Check for circular dependencies
        {
            let mut registry = self.library_registry.borrow_mut();
            registry.begin_loading(name)?;
        }

        // Get search paths (copy to avoid borrow conflicts)
        let search_paths = {
            let registry = self.library_registry.borrow();
            registry.search_paths().to_vec()
        };

        // Try simple loaders first (Rust libraries)
        let lib_result = {
            let loaders = self.loader_registry.borrow();
            loaders.try_simple_load(name, &search_paths)?
        };

        let (lib, loaded_via_rust) = if let Some(lib) = lib_result {
            // Simple loader succeeded (Rust library)
            (lib, true)
        } else {
            // Try evaluating loaders (Scheme .sld files)
            let parsed = {
                let loaders = self.loader_registry.borrow();
                loaders.try_parse(name, &search_paths)?
            };

            if let Some(parsed) = parsed {
                // Parse succeeded, now evaluate
                (self.evaluate_parsed_library(parsed)?, false)
            } else {
                // No loader can handle this library
                self.library_registry.borrow_mut().end_loading(name);
                return Err(patina_runtime::LibraryError::NotFound(name.to_vec()));
            }
        };

        // End loading tracking
        {
            let mut registry = self.library_registry.borrow_mut();
            registry.end_loading(name);
        }

        // Register the library first
        {
            let mut registry = self.library_registry.borrow_mut();
            registry.register(lib)?;
        }

        // If loaded via Rust, load extras and update exports in-place
        if loaded_via_rust {
            // Load extras file - this adds definitions to the library's environment
            self.load_library_extras(name);

            // Update the library's exports to include everything from the environment
            {
                let mut registry = self.library_registry.borrow_mut();
                if let Some(library) = registry.get_mut(name) {
                    let all_bindings = library.env.bindings();

                    // Clear and re-export everything
                    library.exports.clear();
                    for (binding_name, value) in all_bindings {
                        library.export(binding_name, value);
                    }
                }
            }
        }

        // Return the registered library
        let lib_rc = {
            let registry = self.library_registry.borrow();
            Rc::new(registry.get(name).cloned().expect("Library should exist"))
        };

        Ok(lib_rc)
    }

    /// Check if a library is loaded
    pub fn is_library_loaded(&self, name: &[String]) -> bool {
        self.library_registry.borrow().is_loaded(name)
    }

    /// Get a loaded library
    pub fn get_library(&self, name: &[String]) -> Option<Rc<patina_runtime::Library>> {
        self.library_registry
            .borrow()
            .get(name)
            .map(|lib| Rc::new(lib.clone()))
    }

    /// Get the library search paths
    pub fn library_search_paths(&self) -> Vec<PathBuf> {
        self.library_registry.borrow().search_paths().to_vec()
    }

    /// Find a library file in the search paths
    pub fn find_library_file(&self, name: &[String]) -> Option<PathBuf> {
        self.library_registry.borrow().find_library_file(name)
    }

    /// Add a library search path (for testing)
    pub fn add_library_search_path(&self, path: PathBuf) {
        self.library_registry.borrow_mut().add_search_path(path);
    }

    /// Evaluate a parsed library
    ///
    /// This method is called after a library is parsed from a .sld file.
    /// It handles import resolution, body evaluation, and export collection.
    fn evaluate_parsed_library(
        &self,
        parsed: patina_runtime::library_loader::ParsedLibrary,
    ) -> Result<patina_runtime::Library, patina_runtime::LibraryError> {
        use patina_runtime::library_loader::ExportSpec;

        // Create a fresh environment for this library
        let lib_env = Rc::new(Environment::new());

        // Step 1: Resolve imports
        for import_set in &parsed.imports {
            self.process_import_set(import_set, &lib_env)?;
        }

        // Step 2: Evaluate library body (definitions only)
        for expr in &parsed.body {
            self.eval_in_env(expr, &lib_env).map_err(|e| {
                patina_runtime::LibraryError::ParseError {
                    file: parsed
                        .source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    message: format!("Error evaluating library body: {:?}", e),
                }
            })?;
        }

        // Step 3: Collect exports and create library
        let mut library = patina_runtime::Library::with_env(parsed.name.clone(), lib_env.clone());
        if let Some(source) = parsed.source {
            library.set_source(source);
        }

        for spec in &parsed.exports {
            match spec {
                ExportSpec::Identifier(name) => {
                    // Export with same name
                    if let Some(value) = lib_env.get(name) {
                        library.export(name.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: library
                                .source
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            message: format!("Exported identifier '{}' not defined", name),
                        });
                    }
                }

                ExportSpec::Rename { internal, external } => {
                    // Export with different name
                    if let Some(value) = lib_env.get(internal) {
                        library.export(external.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: library
                                .source
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            message: format!("Exported identifier '{}' not defined", internal),
                        });
                    }
                }
            }
        }

        Ok(library)
    }

    /// Process a single import set
    fn process_import_set(
        &self,
        import_set: &patina_runtime::library_loader::ImportSet,
        lib_env: &Rc<Environment>,
    ) -> Result<(), patina_runtime::LibraryError> {
        use patina_runtime::library_loader::ImportSet;
        use std::collections::HashSet;

        match import_set {
            ImportSet::Library(lib_name) => {
                // Direct library import: import all exports
                let imported_lib = self.load_library(lib_name)?;

                // Import all exports into this library's environment
                for (name, value) in &imported_lib.exports {
                    lib_env.define(name.clone(), value.clone());
                }
                Ok(())
            }

            ImportSet::Only {
                import_set,
                identifiers,
            } => {
                // Import only specific identifiers
                // First process the inner import set to get the bindings
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env)?;

                // Then import only the specified identifiers
                for id in identifiers {
                    if let Some(value) = temp_env.get(id) {
                        lib_env.define(id.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: String::new(),
                            message: format!("Identifier '{}' not found in import set", id),
                        });
                    }
                }
                Ok(())
            }

            ImportSet::Except {
                import_set,
                identifiers,
            } => {
                // Import all except specific identifiers
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env)?;

                let exclude: HashSet<_> = identifiers.iter().collect();

                // Import all bindings except the excluded ones
                for (name, value) in temp_env.bindings() {
                    if !exclude.contains(&name) {
                        lib_env.define(name, value);
                    }
                }

                Ok(())
            }

            ImportSet::Prefix { import_set, prefix } => {
                // Import with prefix: foo → prefix:foo
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env)?;

                // Import all bindings with the prefix added
                for (name, value) in temp_env.bindings() {
                    let prefixed_name = format!("{}{}", prefix, name);
                    lib_env.define(prefixed_name, value);
                }

                Ok(())
            }

            ImportSet::Rename {
                import_set,
                renames,
            } => {
                // Import with renames: old-name → new-name
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env)?;

                // Apply renames
                for (old_name, new_name) in renames {
                    if let Some(value) = temp_env.get(old_name) {
                        lib_env.define(new_name.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: String::new(),
                            message: format!("Identifier '{}' not found for rename", old_name),
                        });
                    }
                }
                Ok(())
            }
        }
    }

    /// Process an import set for eval context
    ///
    /// This imports library identifiers into a regular environment (not building a library).
    /// Used by the `import` special form.
    pub(crate) fn process_import_for_eval(
        &self,
        import_set: &patina_frontend::ImportSet,
        env: &Rc<Environment>,
    ) -> Result<(), EvalError> {
        use patina_frontend::ImportSet;
        use std::collections::HashSet;

        match import_set {
            ImportSet::Library(lib_name) => {
                // Load the library
                let lib = self.load_library(lib_name).map_err(|e| {
                    EvalError::InvalidSyntax(format!("Failed to load library: {}", e))
                })?;

                // Import all exports into the current environment
                for export_name in lib.export_names() {
                    if let Some(value) = lib.get_export(export_name) {
                        env.define(export_name.to_string(), value.clone());
                    }
                }
                Ok(())
            }

            ImportSet::Only {
                import_set,
                identifiers,
            } => {
                // First process the nested import set into a temporary environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Then import only the specified identifiers
                let allowed: HashSet<_> = identifiers.iter().collect();
                for (name, value) in temp_env.bindings() {
                    if allowed.contains(&name) {
                        env.define(name, value);
                    }
                }
                Ok(())
            }

            ImportSet::Except {
                import_set,
                identifiers,
            } => {
                // Process nested import set into temp environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Import all except specified identifiers
                let excluded: HashSet<_> = identifiers.iter().collect();
                for (name, value) in temp_env.bindings() {
                    if !excluded.contains(&name) {
                        env.define(name, value);
                    }
                }
                Ok(())
            }

            ImportSet::Prefix { import_set, prefix } => {
                // Process nested import set into temp environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Import all with prefix
                for (name, value) in temp_env.bindings() {
                    let prefixed_name = format!("{}{}", prefix, name);
                    env.define(prefixed_name, value);
                }
                Ok(())
            }

            ImportSet::Rename {
                import_set,
                renames,
            } => {
                // Process nested import set into temp environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Build rename map
                let rename_map: std::collections::HashMap<_, _> = renames.iter().cloned().collect();

                // Import with renaming
                for (name, value) in temp_env.bindings() {
                    let final_name = rename_map.get(&name).unwrap_or(&name);
                    env.define(final_name.clone(), value);
                }
                Ok(())
            }
        }
    }

    /// Extract a pair from a Value
    ///
    /// This is a common operation in special forms to parse argument lists.
    pub(crate) fn extract_pair(&self, expr: &Value) -> Result<(Value, Value), EvalError> {
        match expr {
            Value::Pair(pair) => {
                let pair_ref = pair.borrow();
                Ok((pair_ref.0.clone(), pair_ref.1.clone()))
            }
            _ => Err(EvalError::InvalidSyntax("Expected a pair".to_string())),
        }
    }

    /// Expand a macro using the V2 PVREF-based expander
    ///
    /// This is used by both define-syntax and the macro expansion logic in eval_list_impl.
    pub(crate) fn expand_macro(
        &self,
        compiled_macro: &patina_macros::CompiledMacro,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        // Use the expand_macro function from patina-macros, passing Environment directly
        patina_macros::expand_macro(compiled_macro, args, env)
            .map_err(|e| EvalError::InvalidSyntax(format!("Macro expansion failed: {}", e)))
    }
}
