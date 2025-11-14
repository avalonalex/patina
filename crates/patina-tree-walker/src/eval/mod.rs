// Module declarations
mod application;
mod debug;
mod error;
mod primitives;
pub mod special_forms; // Registry-based special forms

// Re-export error type for public API
pub use error::EvalError;

use debug::DebugConfig;
use patina_runtime::environment::Environment;
use patina_runtime::library_loader::LibraryLoaderRegistry;
use patina_runtime::library_registry::LibraryRegistry;
use patina_runtime::value::{Procedure, Value};
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
    /// Registry of special forms
    pub(crate) special_form_registry: special_forms::SpecialFormRegistry,
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());

        // Create primitive registry and register all primitives
        let mut primitive_registry = primitives::PrimitiveRegistry::new();
        Self::register_all_primitives(&mut primitive_registry);

        // Install primitives into global environment (for backward compatibility)
        Self::install_primitives(&global_env);

        // Create special form registry and register all special forms
        let special_form_registry = special_forms::build_registry();

        // Create library registries
        let library_registry = RefCell::new(LibraryRegistry::with_default_paths());
        let loader_registry = RefCell::new(LibraryLoaderRegistry::new());

        let evaluator = Evaluator {
            global_env,
            debug: Rc::new(DebugConfig::new()),
            library_registry,
            loader_registry,
            primitive_registry,
            special_form_registry,
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
            vec!["scheme".to_string(), "case-lambda".to_string()],
            stdlib::build_scheme_case_lambda,
        );
        rust_loader.register(
            vec!["scheme".to_string(), "r5rs".to_string()],
            stdlib::build_scheme_r5rs,
        );

        // Register test frameworks
        rust_loader.register(
            vec!["chibi".to_string(), "test".to_string()],
            stdlib::build_chibi_test,
        );

        // Add Rust loader first (highest priority)
        loaders.add_loader(Box::new(rust_loader));

        // Add Scheme loader second (for .sld files)
        // Note: SchemeLibraryLoader is now stateless and uses EvaluatingLibraryLoader trait
        loaders.add_evaluating_loader(Box::new(SchemeLibraryLoader::new()));
    }

    fn load_bootstrap(&self) {
        // Embed bootstrap.scm at compile time
        const BOOTSTRAP: &str = include_str!("../../../../lib/bootstrap.scm");

        // Parse and evaluate all expressions in bootstrap
        // Silently ignore any errors (shouldn't happen in bootstrap)
        let mut parser = match patina_frontend::Parser::new(BOOTSTRAP) {
            Ok(p) => p,
            Err(_) => return, // Bootstrap failed to parse
        };

        // Parse and eval all expressions
        loop {
            match parser.parse() {
                Ok(expr) => {
                    // Evaluate, ignore result and errors
                    let _ = self.eval(&expr);
                }
                Err(patina_frontend::ParseError::UnexpectedEof) => break,
                Err(_) => break, // Stop on other errors
            }
        }
    }

    /// Main evaluation entry point with trampoline for tail call optimization
    ///
    /// This implements the trampoline pattern: instead of recursing directly for tail calls,
    /// we loop and process `TailCall` results iteratively. This enables proper tail recursion
    /// as required by R7RS - tail calls execute in constant stack space.
    pub fn eval(&self, expr: &Value) -> Result<Value, EvalError> {
        let mut current_expr = expr.clone();
        let mut current_env = self.global_env.clone();

        // Trampoline loop: keep evaluating until we get a final value
        loop {
            match self.eval_step(&current_expr, &current_env)? {
                EvalResult::Value(v) => return Ok(v),
                EvalResult::TailCall { expr, env } => {
                    // Tail call - continue loop with new expr and env
                    // This reuses the stack frame instead of growing the stack
                    current_expr = expr;
                    current_env = env;
                }
                EvalResult::TailCallPrimitive { proc, args } => {
                    // Primitive tail call - re-apply directly in the trampoline
                    // This allows primitives like call-with-values to participate in TCO
                    // Note: We pass in_tail_position=true since we're continuing the trampoline
                    match self.apply(proc, args, true)? {
                        EvalResult::Value(v) => return Ok(v),
                        EvalResult::TailCall { expr, env } => {
                            current_expr = expr;
                            current_env = env;
                        }
                        EvalResult::TailCallPrimitive { proc, args } => {
                            // Another primitive tail call - continue loop
                            // We'll handle this in the next iteration by reconstructing
                            // the application expression
                            let mut app_list = vec![proc];
                            app_list.extend(args);
                            current_expr = self.list_from_vec(app_list);
                            // Keep current_env unchanged
                        }
                    }
                }
            }
        }
    }

    /// Single evaluation step for the trampoline
    ///
    /// Returns EvalResult which can be either:
    /// - Value: evaluation complete, return this value
    /// - TailCall: tail position, bounce to expr in env
    ///
    /// The `in_tail_position` parameter indicates whether this expression is in tail context.
    /// If true, the final result can be returned as a TailCall for the trampoline to process.
    fn eval_step(&self, expr: &Value, env: &Rc<Environment>) -> Result<EvalResult, EvalError> {
        // Top-level trampoline evaluations are in tail position!
        // This allows the trampoline to bounce tail calls.
        self.eval_step_impl(expr, env, true)
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
            eprintln!(
                "[EVAL]{} Evaluating: {} (tail={})",
                self.debug.current_indent(),
                expr,
                in_tail_position
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
            | Value::Complex(_, _)
            | Value::Character(_)
            | Value::String(_)
            | Value::Vector(_)
            | Value::Bytevector(_) => Ok(EvalResult::Value(expr.clone())),

            // Variable lookup
            Value::Symbol(name) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    eprintln!("[ENV]{} Lookup: '{}'", self.debug.current_indent(), name);
                }

                // First try looking up in current environment
                if let Some(value) = env.get(name) {
                    return Ok(EvalResult::Value(value));
                }

                // If it's a gensym and not found, try looking it up in the global environment
                if patina_frontend::macro_expander::hygiene::is_gensym(name.as_ref())
                    && let Some(original_name) = extract_original_from_gensym(name.as_ref())
                    && let Some(value) = self.global_env.get(&Rc::from(original_name))
                {
                    return Ok(EvalResult::Value(value));
                }

                Err(EvalError::UndefinedVariable(name.to_string()))
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
                    eprintln!("[EVAL]{} => {}", self.debug.current_indent(), val)
                }
                Ok(EvalResult::TailCall { expr, .. }) => eprintln!(
                    "[EVAL]{} => TAIL CALL: {}",
                    self.debug.current_indent(),
                    expr
                ),
                Ok(EvalResult::TailCallPrimitive { proc, args }) => {
                    let args_str = args
                        .iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    eprintln!(
                        "[EVAL]{} => TAIL CALL PRIMITIVE: {} ({})",
                        self.debug.current_indent(),
                        proc,
                        args_str
                    )
                }
                Err(e) => eprintln!("[EVAL]{} => ERROR: {}", self.debug.current_indent(), e),
            }
        }

        result
    }

    /// Evaluate a list (procedure call or special form) with tail position tracking
    fn eval_list_impl(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (car, cdr) = self.extract_pair(expr)?;

        // Check if it's a special form in the registry first
        if let Value::Symbol(ref sym) = car
            && self.special_form_registry.contains(sym.as_ref())
        {
            return self.special_form_registry.eval(
                sym.as_ref(),
                &cdr,
                self,
                env,
                in_tail_position,
            );
        }

        // Check if this symbol is bound to a macro
        if let Value::Symbol(ref sym) = car
            && let Some(Value::Macro { data, .. }) = env.get(sym)
        {
            let compiled_macro = data
                .downcast_ref::<patina_frontend::macro_expander::CompiledMacro>()
                .ok_or_else(|| EvalError::InternalError("Invalid macro data".to_string()))?;

            if self.debug.is_enabled(debug::DebugStage::Expand) {
                eprintln!(
                    "[MACRO]{} Expanding macro '{}': {}",
                    self.debug.current_indent(),
                    sym,
                    expr
                );
                self.debug.indent();
            }

            let expanded = self.expand_macro(compiled_macro, expr, env)?;

            if self.debug.is_enabled(debug::DebugStage::Expand) {
                eprintln!(
                    "[MACRO]{} Expanded to: {}",
                    self.debug.current_indent(),
                    expanded
                );
                self.debug.dedent();
            }

            // Evaluate the expanded form, preserving tail position
            return self.eval_step_impl(&expanded, env, in_tail_position);
        }

        // Regular procedure call - this can be a tail call if in tail position
        let proc = self.eval_in_env(&car, env)?;
        let args = self.eval_arguments(&cdr, env)?;

        // Check if this is a lambda in tail position
        if in_tail_position
            && let Value::Procedure(Procedure::Lambda {
                params,
                variadic,
                body,
                env: lambda_env,
            }) = proc
        {
            // Tail call to lambda - set up environment and evaluate body
            // This is the key to tail recursion!

            // Check arity
            if variadic.is_some() {
                if args.len() < params.len() {
                    return Err(EvalError::WrongArity {
                        expected: format!("at least {}", params.len()),
                        actual: args.len(),
                    });
                }
            } else if args.len() != params.len() {
                return Err(EvalError::WrongArity {
                    expected: params.len().to_string(),
                    actual: args.len(),
                });
            }

            // Create new environment for the lambda
            let new_env = Rc::new(Environment::with_parent(lambda_env));

            // Bind parameters
            for (param, arg) in params.iter().zip(args.iter()) {
                new_env.define(param.clone(), arg.clone());
            }

            // Bind rest parameter if variadic
            if let Some(rest_param) = variadic {
                let rest_args: Vec<Value> = args.into_iter().skip(params.len()).collect();
                let rest_list = self.list_from_vec(rest_args);
                new_env.define(rest_param, rest_list);
            }

            // Return tail call to evaluate the body in the new environment
            // The body expressions are evaluated sequentially, with the last in tail position
            if body.is_empty() {
                return Ok(EvalResult::Value(Value::Unspecified));
            }

            // Evaluate all but the last expression
            for expr in &body[..body.len() - 1] {
                self.eval_in_env(expr, &new_env)?;
            }

            // Last expression is in tail position - return it for the trampoline
            return Ok(EvalResult::TailCall {
                expr: body.last().unwrap().clone(),
                env: new_env,
            });
        }

        // Not in tail position, or not a lambda - just apply normally
        self.apply(proc, args, in_tail_position)
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

        let lib = if let Some(lib) = lib_result {
            // Simple loader succeeded
            lib
        } else {
            // Try evaluating loaders (Scheme .sld files)
            let parsed = {
                let loaders = self.loader_registry.borrow();
                loaders.try_parse(name, &search_paths)?
            };

            if let Some(parsed) = parsed {
                // Parse succeeded, now evaluate
                self.evaluate_parsed_library(parsed)?
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

        // Register the loaded library
        let lib_rc = Rc::new(lib.clone());
        let mut registry = self.library_registry.borrow_mut();
        registry.register(lib)?;
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

    /// Parse lambda parameter list
    ///
    /// Handles:
    /// - `()` - no parameters
    /// - `x` - single variadic parameter
    /// - `(x y z)` - fixed parameters
    /// - `(x y . rest)` - fixed parameters + variadic
    ///
    /// Returns `(fixed_params, variadic_param)`
    pub(crate) fn parse_lambda_params(
        &self,
        params_expr: &Value,
    ) -> Result<(Vec<String>, Option<String>), EvalError> {
        match params_expr {
            // (lambda args body...) - single symbol, all args go to it
            Value::Symbol(s) => Ok((vec![], Some(s.to_string()))),

            // (lambda () body...) - no parameters
            Value::Null => Ok((vec![], None)),

            // (lambda (x y z) body...) or (lambda (x y . rest) body...)
            Value::Pair(_) => {
                let mut params = Vec::new();
                let mut current = params_expr.clone();

                loop {
                    match &current {
                        Value::Null => return Ok((params, None)),
                        Value::Symbol(s) => {
                            // Rest parameter: (x y . rest)
                            return Ok((params, Some(s.to_string())));
                        }
                        Value::Pair(pair) => {
                            if let Value::Symbol(param) = &pair.0 {
                                params.push(param.to_string());
                                current = pair.1.clone();
                            } else {
                                return Err(EvalError::InvalidSyntax(
                                    "lambda parameters must be symbols".to_string(),
                                ));
                            }
                        }
                        _ => {
                            return Err(EvalError::InvalidSyntax(
                                "invalid lambda parameter list".to_string(),
                            ));
                        }
                    }
                }
            }

            _ => Err(EvalError::InvalidSyntax(
                "lambda parameters must be a list or symbol".to_string(),
            )),
        }
    }

    /// Collect list items into a vector
    ///
    /// Verifies the list is proper (ends with null) and returns all elements.
    pub(crate) fn collect_list_items(&self, list: &Value) -> Result<Vec<Value>, EvalError> {
        let mut items = Vec::new();
        let mut current = list.clone();

        while let Value::Pair(pair) = current {
            items.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax("expected proper list".to_string()));
        }

        Ok(items)
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

    /// Compile a syntax-rules form using the V2 PVREF-based compiler
    pub(crate) fn compile_syntax_rules(
        &self,
        expr: &Value,
        name: Rc<str>,
    ) -> Result<patina_frontend::macro_expander::CompiledMacro, EvalError> {
        use patina_frontend::macro_expander::Compiler;

        // Must be a list starting with 'syntax-rules
        let (keyword, rest) = self.extract_pair(expr)?;

        match keyword {
            Value::Symbol(s) if s.as_ref() == "syntax-rules" => {}
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Expected syntax-rules".to_string(),
                ));
            }
        }

        // Parse literals list
        let (literals_expr, rules_expr) = self.extract_pair(&rest)?;
        let literals = self.parse_literals_list(&literals_expr)?;

        // Parse rules as (pattern, template) pairs
        let rules = self.parse_macro_rules(&rules_expr)?;

        // Compile using V2 compiler
        let mut compiler = Compiler::new(literals, None); // Use default ellipsis (...)
        compiler
            .compile_macro(name, rules)
            .map_err(|e| EvalError::InvalidSyntax(format!("Failed to compile macro: {}", e)))
    }

    /// Parse macro rules as (pattern, template) pairs for V2 compiler
    fn parse_macro_rules(&self, expr: &Value) -> Result<Vec<(Value, Value)>, EvalError> {
        let mut rules = Vec::new();
        let mut current = expr.clone();

        while let Value::Pair(rule_pair) = current {
            // Each rule is (pattern template)
            let (pattern, template_list) = self.extract_pair(&rule_pair.0)?;
            let (template, rest_of_rule) = self.extract_pair(&template_list)?;

            // Verify no extra elements in the rule
            if !matches!(rest_of_rule, Value::Null) {
                return Err(EvalError::InvalidSyntax(
                    "Each syntax-rules rule must have exactly 2 elements (pattern template)"
                        .to_string(),
                ));
            }

            rules.push((pattern, template));
            current = rule_pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "syntax-rules rules must be a proper list".to_string(),
            ));
        }

        if rules.is_empty() {
            return Err(EvalError::InvalidSyntax(
                "syntax-rules must have at least one rule".to_string(),
            ));
        }

        Ok(rules)
    }

    /// Parse the literals list: (lit1 lit2 ...)
    fn parse_literals_list(&self, expr: &Value) -> Result<Vec<Rc<str>>, EvalError> {
        let mut literals = Vec::new();
        let mut current = expr.clone();

        while let Value::Pair(pair) = current {
            match &pair.0 {
                Value::Symbol(s) => literals.push(s.clone()),
                _ => {
                    return Err(EvalError::InvalidSyntax(
                        "syntax-rules literals must be symbols".to_string(),
                    ));
                }
            }
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "syntax-rules literals must be a proper list".to_string(),
            ));
        }

        Ok(literals)
    }

    /// Extract a pair from a Value
    ///
    /// This is a common operation in special forms to parse argument lists.
    pub(crate) fn extract_pair(&self, expr: &Value) -> Result<(Value, Value), EvalError> {
        match expr {
            Value::Pair(pair) => Ok((pair.0.clone(), pair.1.clone())),
            _ => Err(EvalError::InvalidSyntax("Expected a pair".to_string())),
        }
    }

    /// Expand a macro using the V2 PVREF-based expander
    ///
    /// This is used by both define-syntax and the macro expansion logic in eval_list_impl.
    pub(crate) fn expand_macro(
        &self,
        compiled_macro: &patina_frontend::macro_expander::CompiledMacro,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        use patina_frontend::macro_expander::{CompiledMacroExpander, MacroExpander};

        let expander = CompiledMacroExpander::new(compiled_macro.clone());
        expander.expand(args, env).map_err(EvalError::from)
    }
}

/// Extract the original identifier name from a gensym
///
/// Gensyms have format: ##name#counter
/// This extracts "name" from that format.
fn extract_original_from_gensym(gensym: &str) -> Option<String> {
    if !gensym.starts_with("##") {
        return None;
    }

    // Skip "##" prefix
    let without_prefix = &gensym[2..];

    // Find the last '#' which separates name from counter
    without_prefix
        .rfind('#')
        .map(|last_hash| without_prefix[..last_hash].to_string())
}
