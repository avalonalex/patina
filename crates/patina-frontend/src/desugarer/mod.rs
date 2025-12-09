// DesugarError contains Value which is large, but boxing it would add complexity
// for minimal benefit in this interpreter context
#![allow(clippy::result_large_err)]

//! Desugarer: Transform surface syntax (Value) to core IR (CoreExpr)
//!
//! This module converts Scheme's surface syntax into a minimal core IR.
//! It handles the **6 core forms** that cannot be expressed as macros:
//! - `quote`, `lambda`, `if`, `set!`, `define`, `begin`
//!
//! # Architecture
//!
//! ```text
//! Parser → Value → Macro Expander → Value → Desugarer → CoreExpr → Evaluator
//! ```
//!
//! # Design Decision: Core Forms Only
//!
//! **This desugarer intentionally handles ONLY core forms, not derived forms.**
//!
//! ## Why Not Desugar Derived Forms (let, cond, and, or, etc.)?
//!
//! Derived forms like `let`, `cond`, `and`, `or` are **already handled by macros**
//! in `lib/scheme/base-extras.scm`. The macro expander transforms them BEFORE
//! the desugarer runs:
//!
//! ```text
//! (let ((x 1)) x)
//!   → [Macro Expander] → ((lambda (x) x) 1)
//!   → [Desugarer] → CoreExpr::App { func: Lambda {...}, ... }
//! ```
//!
//! **The desugarer never sees "let"** - it's already been transformed by macros!
//!
//! Implementing `desugar_let`, `desugar_cond`, etc. would be:
//! - **Dead code** - Never called in normal operation (macros expand first)
//! - **Redundant** - Same logic as macro definitions
//! - **Misleading** - Suggests desugarer is "macro-independent" when it's not
//!
//! ## What If We Need Macro-Independent Desugaring Later?
//!
//! If we ever need to desugar derived forms without macro expansion
//! (e.g., for testing, bootstrapping, or alternative pipelines), we can
//! add them back. The implementations are straightforward:
//! - `let` → lambda application
//! - `cond` → nested `if`
//! - `and`/`or` → short-circuit `if`
//!
//! But until we have a concrete use case, we keep the desugarer simple
//! and focused on its actual job: translating core forms to IR.
//!
//! ## Core Forms vs Derived Forms
//!
//! | Form | Type | Handled By | Notes |
//! |------|------|------------|-------|
//! | `quote` | Core | Desugarer | Cannot be macro |
//! | `lambda` | Core | Desugarer | Cannot be macro |
//! | `if` | Core | Desugarer | Cannot be macro |
//! | `set!` | Core | Desugarer | Cannot be macro |
//! | `define` | Core | Desugarer | Cannot be macro |
//! | `begin` | Core | Desugarer | Cannot be macro |
//! | `let`, `let*`, `letrec` | Derived | Macros | Expand before desugarer |
//! | `cond`, `case`, `when`, `unless` | Derived | Macros | Expand before desugarer |
//! | `and`, `or` | Derived | Macros | Expand before desugarer |
//! | `do` | Derived | Macros | Expand before desugarer |
//!
//! ## See Also
//! - `lib/scheme/base-extras.scm` - Macro definitions for derived forms
//! - `PRD/phase1/CORE_IR_MIGRATION.md` - Full architecture design

mod error;
mod utils;

pub use error::{DesugarError, Result};

use patina_ir::CoreExpr;
use patina_runtime::{Environment, ScopeId, ScopeSet, Value};
use std::rc::Rc;

/// Desugarer converts Value (surface syntax) to CoreExpr (core IR)
///
/// **Macro-Aware Design**: The desugarer can optionally take an environment
/// to enable macro expansion during desugaring. This allows the desugarer to:
/// 1. Check if a form is a macro
/// 2. Expand the macro
/// 3. Recursively desugar the expanded result
///
/// This approach means we don't need to pre-expand all macros before desugaring.
/// The desugarer handles macro expansion selectively, only when encountering
/// macro calls during the desugaring process.
///
/// **Scope-Based Hygiene**: The desugarer tracks a current scope set that
/// accumulates as we enter binding forms (lambda, let-syntax, etc.). This
/// enables scope-based hygiene lookup where identifiers carry scope information.
///
/// **Name Shadowing**: The desugarer also tracks which names are shadowed by
/// local bindings (lambda parameters). These names should not be treated as
/// macro calls, even if a macro with that name exists in the environment.
/// This handles cases like `(let ((let odd?)) (let 8))` where the inner `let`
/// should call the variable, not expand the macro.
pub struct Desugarer {
    /// Optional environment for macro lookup
    /// If None, macros cannot be expanded (will cause desugar errors)
    env: Option<Rc<Environment>>,

    /// Current scope set for scope-based hygiene
    /// Accumulates scopes as we enter binding forms
    current_scopes: ScopeSet,

    /// Names that are shadowed by local bindings (lambda parameters)
    /// These should not be treated as macro calls
    shadowed_names: std::collections::HashSet<Rc<str>>,
}

impl Desugarer {
    /// Create a new desugarer without macro expansion support
    ///
    /// This is used when the input is already fully macro-expanded.
    pub fn new() -> Self {
        Self {
            env: None,
            current_scopes: ScopeSet::new(),
            shadowed_names: std::collections::HashSet::new(),
        }
    }

    /// Create a new desugarer with macro expansion support
    ///
    /// This allows the desugarer to expand macros as it encounters them.
    /// The environment is used to look up macro definitions.
    ///
    /// The desugarer compiles `define-syntax` immediately during desugaring
    /// and installs macros in the environment, returning `CoreExpr::Literal(Unspecified)`.
    pub fn with_env(env: Rc<Environment>) -> Self {
        Self {
            env: Some(env),
            current_scopes: ScopeSet::new(),
            shadowed_names: std::collections::HashSet::new(),
        }
    }

    /// Create a new desugarer with environment and specific scope set
    ///
    /// Used when creating child desugarers that inherit scope context.
    pub fn with_env_and_scopes(env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env: Some(env),
            current_scopes: scopes,
            shadowed_names: std::collections::HashSet::new(),
        }
    }

    /// Get the current scope set
    pub fn current_scopes(&self) -> &ScopeSet {
        &self.current_scopes
    }

    /// Create a child desugarer with an additional scope
    ///
    /// Used when entering a binding form (lambda, let-syntax, etc.)
    #[allow(dead_code)]
    fn with_fresh_scope(&self) -> (Self, ScopeId) {
        let scope = ScopeId::fresh();
        let new_scopes = self.current_scopes.with_scope(scope);
        let desugarer = Self {
            env: self.env.clone(),
            current_scopes: new_scopes,
            shadowed_names: self.shadowed_names.clone(),
        };
        (desugarer, scope)
    }

    /// Create a child desugarer with additional shadowed names
    ///
    /// Used when entering a lambda body where parameters shadow outer bindings.
    /// Names in `new_shadows` will not be treated as macro calls even if a
    /// macro with that name exists in the environment.
    fn with_shadowed_names(
        &self,
        new_shadows: impl IntoIterator<Item = Rc<str>>,
        new_scopes: ScopeSet,
    ) -> Self {
        let mut shadowed = self.shadowed_names.clone();
        shadowed.extend(new_shadows);
        Self {
            env: self.env.clone(),
            current_scopes: new_scopes,
            shadowed_names: shadowed,
        }
    }

    /// Check if a name is shadowed by a local binding
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed_names.contains(name)
    }

    /// Check if a value is a `define-syntax` form
    /// Returns Some((name, transformer)) if it is, None otherwise
    fn try_parse_define_syntax(&self, value: &Value) -> Option<(Rc<str>, Value)> {
        // Must be a list
        let (car, cdr) = match value {
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                (borrowed.0.clone(), borrowed.1.clone())
            }
            _ => return None,
        };

        // First element must be 'define-syntax' symbol/identifier
        let name_str = match &car {
            Value::Symbol(s) => s.clone(),
            Value::Identifier(id) => id.name.clone(),
            _ => return None,
        };

        if name_str.as_ref() != "define-syntax" {
            return None;
        }

        // Parse arguments: (name transformer)
        let args_vec = utils::list_to_vec(&cdr).ok()?;
        if args_vec.len() != 2 {
            return None;
        }

        // Extract macro name
        let macro_name = match &args_vec[0] {
            Value::Symbol(s) => s.clone(),
            Value::Identifier(id) => id.name.clone(),
            _ => return None,
        };

        Some((macro_name, args_vec[1].clone()))
    }

    /// Create a child desugarer with a new environment (for let-syntax bodies)
    ///
    /// This inherits the current shadowed_names and uses the new environment and scopes.
    fn with_new_env(&self, env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env: Some(env),
            current_scopes: scopes,
            shadowed_names: self.shadowed_names.clone(),
        }
    }

    /// Desugar a Value (surface syntax) to CoreExpr (core IR)
    ///
    /// This is the main entry point. It handles:
    /// - Self-evaluating literals
    /// - Variable references
    /// - Special forms (core and derived)
    /// - Function application
    pub fn desugar(&self, value: &Value) -> Result<CoreExpr> {
        match value {
            // Self-evaluating literals
            Value::Boolean(_)
            | Value::Integer(_)
            | Value::BigInteger(_)
            | Value::Rational(_)
            | Value::Real(_)
            | Value::Complex(_)
            | Value::Character(_)
            | Value::String(_)
            | Value::Bytevector(_)
            | Value::Unspecified => Ok(CoreExpr::Literal(Rc::new(value.clone()))),

            // Variable reference (no scopes for plain symbols)
            Value::Symbol(s) => Ok(CoreExpr::Var {
                name: s.clone(),
                scopes: ScopeSet::new(),
            }),

            // Identifier with potential hygiene scopes
            Value::Identifier(id) => Ok(CoreExpr::Var {
                name: id.name.clone(),
                scopes: id.scopes.clone(),
            }),

            // Empty list (unusual in AST, but possible as literal)
            Value::Null => Ok(CoreExpr::Literal(Rc::new(Value::Null))),

            // Lists - special forms or application
            Value::Pair(_) => self.desugar_list(value),

            // Vectors - literal
            Value::Vector(_) => Ok(CoreExpr::Literal(Rc::new(value.clone()))),

            // Runtime-only values should never appear in AST
            Value::Procedure(_)
            | Value::Parameter { .. }
            | Value::Macro(_)
            | Value::Library(_)
            | Value::Port(_)
            | Value::Promise(_)
            | Value::Eof
            | Value::Values(_)
            | Value::LabelPlaceholder(_)
            | Value::RecordType(_)
            | Value::Record { .. } => Err(DesugarError::RuntimeValueInAST {
                value: value.clone(),
                context: "Cannot desugar runtime-only value".to_string(),
            }),
        }
    }

    /// Desugar a list (special form or application)
    fn desugar_list(&self, value: &Value) -> Result<CoreExpr> {
        // Extract car and cdr
        let (car, cdr) = match value {
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                (borrowed.0.clone(), borrowed.1.clone())
            }
            _ => return Err(DesugarError::InvalidSyntax("Expected pair".to_string())),
        };

        // Extract the operator name and check if it's a macro-introduced keyword
        // Identifiers with non-empty scopes come from macro templates
        // Symbols without scopes come from user code
        let (name, is_macro_introduced) = match &car {
            Value::Symbol(s) => (Some(s.clone()), false),
            Value::Identifier(id) => (Some(id.name.clone()), !id.scopes.is_empty()),
            _ => (None, false),
        };

        // Determine if this name is shadowed by a local binding
        // Only user code (Symbol) can be shadowed - macro-introduced identifiers
        // always refer to their original binding
        let is_shadowed = name
            .as_ref()
            .map(|n| !is_macro_introduced && self.is_shadowed(n))
            .unwrap_or(false);

        // MACRO-AWARE DESUGARING: Check if this is a macro call
        // Rules for when to expand as a macro:
        // 1. Must have an environment for macro lookup
        // 2. Must NOT be shadowed by local binding (only applies to user code)
        // 3. Must actually be bound to a macro in the environment
        //
        // Note: Macro-introduced identifiers (is_macro_introduced=true) should
        // STILL be checked for macros. The macro template's `let` should expand
        // to the `let` macro, not be treated as a variable.
        if let (Some(env), Some(sym)) = (&self.env, &name)
            && !is_shadowed
            && let Some(Value::Macro(compiled_macro)) = env.get(sym)
        {
            // This is a macro! Expand it and recursively desugar the result
            // Pass shadowed_names for literal shadowing check (R7RS 4.3.2)
            let expanded = patina_macros::expand_macro_with_shadowed(
                &compiled_macro,
                value,
                env,
                &self.shadowed_names,
            )
            .map_err(|e| DesugarError::InvalidSyntax(format!("Macro expansion failed: {}", e)))?;

            // Recursively desugar the expanded result
            return self.desugar(&expanded);
        }

        // Check if it's a core special form
        // This handles both:
        // - Symbol("if") from user code (not shadowed)
        // - Identifier("if", {scopes}) from macro templates
        if let Some(sym) = &name {
            // If NOT shadowed, treat as special form
            // If shadowed (user code with local binding), treat as application
            if !is_shadowed {
                match sym.as_ref() {
                    // The core forms (true special forms, NOT macros)
                    "quote" => return self.desugar_quote(&cdr),
                    "quasiquote" => return self.desugar_quasiquote(&cdr),
                    "lambda" => return self.desugar_lambda(&cdr),
                    "if" => return self.desugar_if(&cdr),
                    "set!" => return self.desugar_set(&cdr),
                    "define" => return self.desugar_define(&cdr),
                    "define-syntax" => return self.desugar_define_syntax(&cdr),
                    "import" => return self.desugar_import(&cdr),
                    "parameterize" => return self.desugar_parameterize(&cdr),
                    "begin" => return self.desugar_begin(&cdr),
                    "apply" => return self.desugar_apply(&cdr),
                    "expand" => return self.desugar_expand(&cdr),
                    // case-lambda is now a macro via SRFI-16 (lib/scheme/case-lambda-extras.scm)

                    // Let-syntax forms: compile macros and desugar body
                    "let-syntax" => return self.desugar_let_syntax(&cdr),
                    "letrec-syntax" => return self.desugar_letrec_syntax(&cdr),

                    // Conditional expansion (compile-time)
                    "cond-expand" => return self.desugar_cond_expand(&cdr),

                    // Not a special form - fall through to application
                    _ => {}
                }
            }
        }

        // Everything else is a procedure application
        self.desugar_app(value)
    }

    /// Desugar quote: (quote datum) → Quote(datum)
    fn desugar_quote(&self, args: &Value) -> Result<CoreExpr> {
        let datum = utils::expect_one_arg(args, "quote")?;
        Ok(CoreExpr::Quote(Rc::new(datum.clone())))
    }

    /// Desugar quasiquote: (quasiquote template) → Quasiquote(template)
    /// The template is kept as-is (Value), and will be processed by the evaluator
    /// to handle unquote and unquote-splicing
    fn desugar_quasiquote(&self, args: &Value) -> Result<CoreExpr> {
        let template = utils::expect_one_arg(args, "quasiquote")?;
        Ok(CoreExpr::Quasiquote(Rc::new(template.clone())))
    }

    /// Desugar lambda: (lambda formals body ...) → Lambda { params, body }
    ///
    /// ## Hygiene Handling
    ///
    /// When parameters come from macro expansion, they carry scope information
    /// (as `Value::Identifier` with scopes). These scopes are preserved in the
    /// `ScopedParam` and used at runtime for hygienic binding.
    ///
    /// The `binding_scope` field is only used when parameters DON'T have scopes
    /// (i.e., when they're plain `Symbol`s from non-macro code). For macro-introduced
    /// parameters, the parameter's own scopes are used instead.
    ///
    /// ## Variable Shadowing
    ///
    /// Lambda parameters shadow any outer bindings, including macros. When
    /// desugaring the body, parameter names are added to `shadowed_names` so
    /// that uses of those names are not treated as macro calls. This enables
    /// code like `(let ((let odd?)) (let 8))` where the inner `let` should
    /// call the variable, not expand the `let` macro.
    fn desugar_lambda(&self, args: &Value) -> Result<CoreExpr> {
        let (formals, body) = utils::parse_lambda_syntax(args)?;

        // Convert formals first to check if they have scopes from macro expansion
        let params = utils::convert_formals(&formals)?;

        // Check if any parameter has scopes from macro expansion
        let has_macro_scopes = match &params {
            patina_ir::Formals::Fixed(ps) => ps.iter().any(|p| !p.scopes.is_empty()),
            patina_ir::Formals::Variadic(p) => !p.scopes.is_empty(),
            patina_ir::Formals::Mixed { fixed, rest } => {
                fixed.iter().any(|p| !p.scopes.is_empty()) || !rest.scopes.is_empty()
            }
        };

        // Create a fresh scope for this lambda's bindings
        // This is used for:
        // 1. Non-macro parameters (they have no scopes, need fresh scope)
        // 2. let-syntax inside the lambda (to capture lexical context)
        let binding_scope = ScopeId::fresh();
        let body_scopes = self.current_scopes.with_scope(binding_scope);

        // Extract parameter names for shadowing
        // These names will not be treated as macro calls in the body
        let param_names = utils::formals_to_names(&params);

        // Desugar body with:
        // 1. The new scope set (for hygiene)
        // 2. Parameter names added to shadowed_names (so they don't trigger macro expansion)
        let body_desugarer = self.with_shadowed_names(param_names, body_scopes.clone());

        // Process body expressions sequentially to handle define-syntax
        // When we encounter define-syntax, we compile the macro immediately
        // and update the environment for subsequent expressions
        let body_exprs =
            self.desugar_body_with_internal_defines(&body_desugarer, &body, &body_scopes)?;

        // If parameters have macro scopes, set binding_scope to None
        // since we'll use the parameter-specific scopes at runtime.
        // Otherwise, use the fresh scope for all parameters.
        let binding_scope = if has_macro_scopes {
            None // Parameters carry their own scopes
        } else {
            Some(binding_scope) // Use fresh scope for all
        };

        Ok(CoreExpr::Lambda {
            params,
            body: body_exprs,
            binding_scope,
        })
    }

    /// Desugar a body that may contain internal define-syntax forms
    ///
    /// This processes body expressions sequentially. When encountering `define-syntax`,
    /// it compiles the macro immediately and adds it to the environment so subsequent
    /// expressions can use it. This is required for code like:
    ///
    /// ```scheme
    /// (let ()
    ///   (define-syntax my-macro ...)
    ///   (my-macro ...))  ; Must see my-macro as a macro, not a variable
    /// ```
    ///
    /// This also handles macro-generating macros: when a macro expands to `define-syntax`,
    /// the resulting macro definition is compiled and added to the environment.
    ///
    /// Returns the desugared body expressions (excluding define-syntax forms,
    /// which are processed but not emitted to CoreExpr).
    ///
    /// Note: define-syntax is now compiled immediately by desugar_define_syntax()
    /// and returns Literal(Unspecified). The macro is installed in the environment
    /// during desugaring. This function filters out the Unspecified results from
    /// define-syntax forms to avoid spurious expressions in the body.
    fn desugar_body_with_internal_defines(
        &self,
        initial_desugarer: &Desugarer,
        body: &[Value],
        body_scopes: &ScopeSet,
    ) -> Result<Vec<CoreExpr>> {
        // If we don't have an environment, fall back to simple desugaring
        let env = match &initial_desugarer.env {
            Some(e) => e.clone(),
            None => {
                return body.iter().map(|e| initial_desugarer.desugar(e)).collect();
            }
        };

        let mut body_exprs = Vec::new();
        let mut current_env = env.clone();
        // Create initial desugarer with the environment - with_new_env handles cloning shadowed_names
        let mut current_desugarer = initial_desugarer.with_new_env(env, body_scopes.clone());

        for expr in body {
            // Check if this is a define-syntax form BEFORE desugaring
            if let Some((macro_name, transformer)) = self.try_parse_define_syntax(expr) {
                // Compile the macro immediately
                let compiled_macro = self.compile_syntax_rules_with_scopes(
                    &transformer,
                    macro_name.clone(),
                    &current_env,
                    body_scopes,
                )?;

                // Create a new environment with the macro binding
                let new_env = Rc::new(Environment::with_parent(current_env.clone()));
                new_env.define(
                    macro_name.to_string(),
                    Value::Macro(Rc::new(compiled_macro)),
                );

                // Update the current desugarer to use the new environment
                // with_new_env creates a new Desugarer, no Clone trait needed
                current_env = new_env.clone();
                current_desugarer = current_desugarer.with_new_env(new_env, body_scopes.clone());

                // Don't emit anything - macro definition is compile-time only
            } else {
                // Regular expression - desugar with current environment
                let desugared = current_desugarer.desugar(expr)?;

                // Filter out Literal(Unspecified) from macro definitions
                // (macro-generating macros also install immediately and return Unspecified)
                if !matches!(&desugared, CoreExpr::Literal(v) if matches!(v.as_ref(), Value::Unspecified))
                {
                    body_exprs.push(desugared);
                }
            }
        }

        // Body must have at least one non-define-syntax expression
        if body_exprs.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "Body must contain at least one expression (not just define-syntax)".to_string(),
            ));
        }

        Ok(body_exprs)
    }

    /// Desugar if: (if test then [else]) → If { test, then, else_ }
    fn desugar_if(&self, args: &Value) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec(args)?;

        match args_vec.as_slice() {
            [test, then] => {
                // Two-arg if: (if test then) → (if test then #<unspecified>)
                Ok(CoreExpr::If {
                    test: Rc::new(self.desugar(test)?),
                    then: Rc::new(self.desugar(then)?),
                    else_: Rc::new(CoreExpr::Literal(Rc::new(Value::Unspecified))),
                })
            }
            [test, then, else_] => {
                // Three-arg if: (if test then else)
                Ok(CoreExpr::If {
                    test: Rc::new(self.desugar(test)?),
                    then: Rc::new(self.desugar(then)?),
                    else_: Rc::new(self.desugar(else_)?),
                })
            }
            _ => Err(DesugarError::WrongArgCount {
                form: "if".to_string(),
                expected: "2 or 3".to_string(),
                got: args_vec.len(),
            }),
        }
    }

    /// Desugar set!: (set! var value) → Set { var, scopes, value }
    fn desugar_set(&self, args: &Value) -> Result<CoreExpr> {
        let (var, value) = utils::expect_two_args(args, "set!")?;

        let desugared_value = Rc::new(self.desugar(&value)?);

        match &var {
            Value::Symbol(s) => Ok(CoreExpr::Set {
                var: s.clone(),
                scopes: ScopeSet::new(),
                value: desugared_value,
            }),
            // Handle hygienic identifiers from macro expansion
            Value::Identifier(id) => Ok(CoreExpr::Set {
                var: id.name.clone(),
                scopes: id.scopes.clone(),
                value: desugared_value,
            }),
            _ => Err(DesugarError::InvalidSyntax(
                "set! requires a symbol as first argument".to_string(),
            )),
        }
    }

    /// Desugar define: (define var value) or (define (name params...) body...)
    ///
    /// Special case: (define () value) - used by define-values with no variables.
    /// Evaluates value for side effects, returns unspecified.
    fn desugar_define(&self, args: &Value) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec(args)?;

        match args_vec.as_slice() {
            // (define var value)
            [Value::Symbol(name), value] => Ok(CoreExpr::Define {
                name: name.clone(),
                value: Rc::new(self.desugar(value)?),
            }),

            // Handle hygienic identifiers for variable names
            [Value::Identifier(id), value] => Ok(CoreExpr::Define {
                name: id.name.clone(),
                value: Rc::new(self.desugar(value)?),
            }),

            // (define () value) - empty variable list from define-values
            // Evaluate value for side effects, discard result
            [Value::Null, value] => {
                // Transform to: (begin value #<unspecified>)
                Ok(CoreExpr::Begin(vec![
                    self.desugar(value)?,
                    CoreExpr::Literal(Rc::new(Value::Unspecified)),
                ]))
            }

            // (define (name params...) body...)
            [Value::Pair(_), body @ ..] => {
                let (name, params) = utils::parse_define_function(&args_vec[0])?;

                if body.is_empty() {
                    return Err(DesugarError::EmptyBody("define".to_string()));
                }

                // Create a fresh scope for this lambda's bindings
                // Every lambda gets a scope for proper hygiene
                let binding_scope = ScopeId::fresh();
                let body_scopes = self.current_scopes.with_scope(binding_scope);

                // Convert and extract parameter names for shadowing
                let converted_params = utils::convert_formals(&params)?;
                let param_names = utils::formals_to_names(&converted_params);

                // Desugar body with the new scope set and shadowed names
                let body_desugarer = self.with_shadowed_names(param_names, body_scopes);

                let lambda = CoreExpr::Lambda {
                    params: converted_params,
                    body: body
                        .iter()
                        .map(|e| body_desugarer.desugar(e))
                        .collect::<Result<_>>()?,
                    binding_scope: Some(binding_scope),
                };

                Ok(CoreExpr::Define {
                    name,
                    value: Rc::new(lambda),
                })
            }

            other => Err(DesugarError::InvalidSyntax(format!(
                "define requires (define var value) or (define (name ...) body), got {} args with first arg type: {}",
                other.len(),
                if other.is_empty() {
                    "none"
                } else {
                    other[0].type_name()
                }
            ))),
        }
    }

    /// Desugar define-syntax: (define-syntax name transformer)
    ///
    /// The macro is compiled immediately during desugaring and installed in the
    /// environment. Returns `CoreExpr::Literal(Unspecified)` since the macro
    /// definition is a compile-time operation with no runtime effect.
    fn desugar_define_syntax(&self, args: &Value) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec(args)?;

        if args_vec.len() != 2 {
            return Err(DesugarError::InvalidSyntax(
                "define-syntax requires (define-syntax name transformer)".to_string(),
            ));
        }

        // Extract name - can be Symbol or Identifier (from macro expansion)
        let name = match &args_vec[0] {
            Value::Symbol(s) => s.clone(),
            Value::Identifier(id) => id.name.clone(),
            _ => {
                return Err(DesugarError::InvalidSyntax(
                    "define-syntax requires (define-syntax name transformer)".to_string(),
                ));
            }
        };

        let transformer = &args_vec[1];

        // Compile macro immediately and install in environment
        let env = self.env.as_ref().ok_or_else(|| {
            DesugarError::InvalidSyntax(
                "define-syntax requires environment (use Desugarer::with_env)".into(),
            )
        })?;

        let compiled_macro = self.compile_syntax_rules_with_scopes(
            transformer,
            name.clone(),
            env,
            &self.current_scopes,
        )?;

        // Install in environment
        env.define(name.to_string(), Value::Macro(Rc::new(compiled_macro)));

        // Return unspecified - macro is now in environment
        Ok(CoreExpr::Literal(Rc::new(Value::Unspecified)))
    }

    /// Desugar import: (import import-set ...) → Import { import_sets }
    fn desugar_import(&self, args: &Value) -> Result<CoreExpr> {
        // Extract import sets as a vector
        // Import sets are kept as Values (declarative data, not code to evaluate)
        let import_sets = utils::list_to_vec(args)?;

        if import_sets.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "import requires at least one import set".to_string(),
            ));
        }

        Ok(CoreExpr::Import { import_sets })
    }

    /// Desugar parameterize: (parameterize ((param val) ...) body ...)
    fn desugar_parameterize(&self, args: &Value) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec(args)?;

        if args_vec.len() < 2 {
            return Err(DesugarError::InvalidSyntax(
                "parameterize requires bindings and at least one body expression".to_string(),
            ));
        }

        // First element is bindings list
        let bindings_value = &args_vec[0];
        let bindings_list = utils::list_to_vec(bindings_value)?;

        // Parse each binding as (param value)
        let mut bindings = Vec::new();
        for binding in bindings_list {
            let binding_vec = utils::list_to_vec(&binding)?;
            if binding_vec.len() != 2 {
                return Err(DesugarError::InvalidSyntax(
                    "Each parameterize binding must be (param value)".to_string(),
                ));
            }
            let param_expr = self.desugar(&binding_vec[0])?;
            let value_expr = self.desugar(&binding_vec[1])?;
            bindings.push((param_expr, value_expr));
        }

        // Rest are body expressions
        let body_exprs: Vec<CoreExpr> = args_vec[1..]
            .iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_>>()?;

        if body_exprs.is_empty() {
            return Err(DesugarError::EmptyBody("parameterize".to_string()));
        }

        Ok(CoreExpr::Parameterize {
            bindings,
            body: body_exprs,
        })
    }

    /// Desugar begin: (begin expr ...) → Begin(exprs)
    ///
    /// R7RS allows empty begin: (begin) → #<unspecified>
    fn desugar_begin(&self, args: &Value) -> Result<CoreExpr> {
        let exprs = utils::list_to_vec(args)?;

        let core_exprs: Vec<CoreExpr> = exprs
            .iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_>>()?;

        // Special cases:
        // (begin) → #<unspecified> (R7RS allows this)
        if core_exprs.is_empty() {
            return Ok(CoreExpr::Literal(Rc::new(Value::Unspecified)));
        }

        // Optimize: (begin expr) → expr
        if core_exprs.len() == 1 {
            Ok(core_exprs.into_iter().next().unwrap())
        } else {
            Ok(CoreExpr::Begin(core_exprs))
        }
    }

    /// Desugar apply: (apply proc arg1 ... argN list) → Apply { func, args }
    ///
    /// The last argument is a list that gets spliced as arguments.
    /// This requires special handling during evaluation.
    fn desugar_apply(&self, args: &Value) -> Result<CoreExpr> {
        let exprs = utils::list_to_vec(args)?;

        if exprs.len() < 2 {
            return Err(DesugarError::InvalidSyntax(
                "apply requires at least 2 arguments (procedure and list)".to_string(),
            ));
        }

        // First argument is the procedure
        let func = Rc::new(self.desugar(&exprs[0])?);

        // Remaining arguments (including the final list)
        let args_exprs: Vec<CoreExpr> = exprs[1..]
            .iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_>>()?;

        Ok(CoreExpr::Apply {
            func,
            args: args_exprs,
        })
    }

    /// Desugar expand: (expand expr) → Expand { expr }
    ///
    /// Expand is a Patina debugging extension that shows macro expansion
    /// without evaluating the result. This is useful for understanding
    /// macro transformations.
    fn desugar_expand(&self, args: &Value) -> Result<CoreExpr> {
        let expr = utils::expect_one_arg(args, "expand")?;

        Ok(CoreExpr::Expand {
            expr: Rc::new(self.desugar(&expr)?),
        })
    }

    /// Desugar let-syntax: (let-syntax ((name transformer) ...) body ...)
    ///
    /// Compiles macros in parent environment, creates extended environment,
    /// and desugars body in that environment. The result is just the desugared body -
    /// macros are completely eliminated during desugaring (compile-time only).
    fn desugar_let_syntax(&self, args: &Value) -> Result<CoreExpr> {
        self.desugar_let_syntax_impl(args, false)
    }

    /// Desugar letrec-syntax: (letrec-syntax ((name transformer) ...) body ...)
    ///
    /// Compiles macros in new environment (allowing mutual recursion),
    /// and desugars body in that environment. Macros are eliminated during desugaring.
    fn desugar_letrec_syntax(&self, args: &Value) -> Result<CoreExpr> {
        self.desugar_let_syntax_impl(args, true)
    }

    /// Common implementation for let-syntax and letrec-syntax
    ///
    /// # Arguments
    /// - `args`: The arguments to let-syntax/letrec-syntax (bindings and body)
    /// - `is_letrec`: true for letrec-syntax, false for let-syntax
    fn desugar_let_syntax_impl(&self, args: &Value, is_letrec: bool) -> Result<CoreExpr> {
        // We need an environment to compile macros
        let env = self.env.as_ref().ok_or_else(|| {
            DesugarError::InvalidSyntax(
                "let-syntax requires macro environment (desugarer must be created with with_env)"
                    .to_string(),
            )
        })?;

        // Create a fresh scope for this let-syntax binding form
        // This is critical for scope-based hygiene: macros defined here
        // will capture this scope, allowing free variables to resolve
        // to definition-time bindings.
        let let_syntax_scope = ScopeId::fresh();
        let definition_scopes = self.current_scopes.with_scope(let_syntax_scope);

        // Parse arguments: (bindings body...)
        let args_vec = utils::list_to_vec(args)?;

        if args_vec.len() < 2 {
            return Err(DesugarError::InvalidSyntax(format!(
                "{} requires bindings and at least one body expression",
                if is_letrec {
                    "letrec-syntax"
                } else {
                    "let-syntax"
                }
            )));
        }

        // Parse bindings: ((name transformer) ...)
        let bindings_value = &args_vec[0];
        let bindings_list = utils::list_to_vec(bindings_value)?;

        // Determine compilation environment
        // - let-syntax: compile in parent environment
        // - letrec-syntax: compile in new environment (for mutual recursion)
        let compile_env = if is_letrec {
            Rc::new(Environment::with_parent(env.clone()))
        } else {
            env.clone()
        };

        // Compile each macro binding with definition scopes
        let mut macro_bindings = Vec::new();

        for binding in bindings_list {
            let binding_vec = utils::list_to_vec(&binding)?;
            if binding_vec.len() != 2 {
                return Err(DesugarError::InvalidSyntax(
                    "Each let-syntax binding must be (name transformer)".to_string(),
                ));
            }

            // Extract name - can be Symbol or Identifier (from macro expansion)
            let name = match &binding_vec[0] {
                Value::Symbol(s) => s.clone(),
                Value::Identifier(id) => id.name.clone(),
                _ => {
                    return Err(DesugarError::InvalidSyntax(
                        "Macro name must be a symbol".to_string(),
                    ));
                }
            };

            // Compile transformer using patina_macros with definition scopes
            let transformer = &binding_vec[1];
            let compiled_macro = self.compile_syntax_rules_with_scopes(
                transformer,
                name.clone(),
                &compile_env,
                &definition_scopes,
            )?;

            macro_bindings.push((name, compiled_macro));
        }

        // Create environment with macro bindings
        let body_env = Rc::new(Environment::with_parent(env.clone()));
        for (name, compiled_macro) in macro_bindings {
            let macro_value = Value::Macro(Rc::new(compiled_macro));
            body_env.define(name.to_string(), macro_value);
        }

        // Create desugarer with extended environment AND scope set
        // The body sees the let-syntax scope (for bindings introduced by macros)
        // We use with_new_env to inherit shadowed_names from the parent
        let body_desugarer = self.with_new_env(body_env, definition_scopes.clone());

        // Desugar body expressions in extended environment
        // Use desugar_body_with_internal_defines to handle:
        // 1. define-syntax in body (compile macros immediately)
        // 2. Regular define in body (will need wrapping in lambda for proper scoping)
        let body = &args_vec[1..];

        // Check if body contains internal defines (not define-syntax)
        // If so, wrap in lambda for proper R7RS scoping
        let has_internal_defines = body.iter().any(|e| self.is_regular_define(e));

        if has_internal_defines {
            // Wrap body in implicit lambda for proper internal define scoping
            // (let-syntax () (define x 1) body...) => (let-syntax () ((lambda () (define x 1) body...)))
            // This ensures internal defines create local bindings, not modify outer scope
            let desugared_body =
                self.desugar_body_with_internal_defines(&body_desugarer, body, &definition_scopes)?;

            // Wrap in a lambda call: ((lambda () body...))
            Ok(CoreExpr::App {
                func: Rc::new(CoreExpr::Lambda {
                    params: patina_ir::Formals::Fixed(vec![]),
                    body: desugared_body,
                    binding_scope: Some(ScopeId::fresh()),
                }),
                args: vec![],
            })
        } else {
            // No internal defines - just desugar body with define-syntax handling
            let desugared_body =
                self.desugar_body_with_internal_defines(&body_desugarer, body, &definition_scopes)?;

            // Return body as Begin expression
            // Optimize: single expression doesn't need Begin wrapper
            if desugared_body.len() == 1 {
                Ok(desugared_body.into_iter().next().unwrap())
            } else {
                Ok(CoreExpr::Begin(desugared_body))
            }
        }
    }

    /// Check if a value is a regular `define` form (not `define-syntax`)
    fn is_regular_define(&self, value: &Value) -> bool {
        let (car, _cdr) = match value {
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                (borrowed.0.clone(), borrowed.1.clone())
            }
            _ => return false,
        };

        let name = match &car {
            Value::Symbol(s) => s.clone(),
            Value::Identifier(id) => id.name.clone(),
            _ => return false,
        };

        name.as_ref() == "define"
    }

    /// Desugar cond-expand: (cond-expand clause ...)
    ///
    /// R7RS §4.2.1: Conditional expansion based on feature requirements.
    /// This is a compile-time construct - the first matching clause's body
    /// is expanded and returned; other clauses are discarded.
    ///
    /// Each clause is: (<feature-requirement> <expression> ...)
    /// Special clause: (else <expression> ...) - must be last
    fn desugar_cond_expand(&self, args: &Value) -> Result<CoreExpr> {
        use crate::cond_expand::evaluate_feature_requirement;
        use patina_runtime::features::FeatureRegistry;

        let clauses = utils::list_to_vec(args)?;

        if clauses.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "cond-expand requires at least one clause".to_string(),
            ));
        }

        // Get feature registry for evaluating requirements
        let features = FeatureRegistry::new();

        // Callback for checking if a library can be loaded
        // In expression context, we don't have access to the library loader,
        // so we use the environment to check if a library is available
        let can_load_library = |_lib_name: &[String]| {
            // For now, we can't check library availability in the desugarer
            // This would require access to the library loader registry
            // Return false for all library checks
            false
        };

        for (i, clause) in clauses.iter().enumerate() {
            let clause_list = utils::list_to_vec(clause)?;

            if clause_list.is_empty() {
                return Err(DesugarError::InvalidSyntax(
                    "cond-expand clause cannot be empty".to_string(),
                ));
            }

            let requirement = &clause_list[0];
            let body = &clause_list[1..];

            // Check for 'else' clause (must be last)
            let is_else = matches!(requirement, Value::Symbol(s) if s.as_ref() == "else")
                || matches!(requirement, Value::Identifier(id) if id.name.as_ref() == "else");

            if is_else {
                if i != clauses.len() - 1 {
                    return Err(DesugarError::InvalidSyntax(
                        "cond-expand: else clause must be last".to_string(),
                    ));
                }

                // else always matches - desugar the body
                return self.desugar_cond_expand_body(body);
            }

            // Evaluate the feature requirement
            let matches = evaluate_feature_requirement(requirement, &features, &can_load_library)
                .map_err(|e| {
                DesugarError::InvalidSyntax(format!(
                    "cond-expand: invalid feature requirement: {}",
                    e
                ))
            })?;

            if matches {
                // This clause matches - desugar the body
                return self.desugar_cond_expand_body(body);
            }
        }

        // No clause matched - this is an error per R7RS
        Err(DesugarError::InvalidSyntax(
            "cond-expand: no matching clause".to_string(),
        ))
    }

    /// Desugar the body of a cond-expand clause
    fn desugar_cond_expand_body(&self, body: &[Value]) -> Result<CoreExpr> {
        if body.is_empty() {
            // Empty body returns unspecified
            return Ok(CoreExpr::Literal(Rc::new(Value::Unspecified)));
        }

        // Desugar all body expressions
        let desugared: Vec<CoreExpr> = body
            .iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_>>()?;

        // Optimize: single expression doesn't need Begin wrapper
        if desugared.len() == 1 {
            Ok(desugared.into_iter().next().unwrap())
        } else {
            Ok(CoreExpr::Begin(desugared))
        }
    }

    /// Compile a syntax-rules transformer with scope set for scope-based hygiene
    ///
    /// This version passes the definition scopes to the compiler so that
    /// free variables in templates carry the correct scope set.
    fn compile_syntax_rules_with_scopes(
        &self,
        expr: &Value,
        name: Rc<str>,
        env: &Rc<Environment>,
        scopes: &ScopeSet,
    ) -> Result<patina_macros::CompiledMacro> {
        use patina_macros::Compiler;

        // Must be a list starting with 'syntax-rules
        let list = utils::list_to_vec(expr)?;

        if list.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "Expected syntax-rules".to_string(),
            ));
        }

        // Check first element is 'syntax-rules
        // Can be either Symbol or Identifier (from macro expansion)
        let is_syntax_rules = match &list[0] {
            Value::Symbol(s) => s.as_ref() == "syntax-rules",
            Value::Identifier(id) => id.name.as_ref() == "syntax-rules",
            _ => false,
        };
        if !is_syntax_rules {
            return Err(DesugarError::InvalidSyntax(
                "Expected syntax-rules".to_string(),
            ));
        }

        if list.len() < 2 {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules requires literals and rules".to_string(),
            ));
        }

        // R7RS syntax-rules has two forms:
        // (syntax-rules (<literal> ...) <rule> ...)
        // (syntax-rules <ellipsis> (<literal> ...) <rule> ...)
        //
        // Detect which form by checking if list[1] is a list (literals) or symbol (custom ellipsis)
        let (custom_ellipsis, literals_index) = match &list[1] {
            // If it's a list or null, it's the literals list (standard form)
            Value::Pair(_) | Value::Null => (None, 1),
            // If it's a symbol or identifier, it's a custom ellipsis
            Value::Symbol(s) => (Some(s.clone()), 2),
            Value::Identifier(id) => (Some(id.name.clone()), 2),
            _ => {
                return Err(DesugarError::InvalidSyntax(
                    "syntax-rules: expected literals list or ellipsis identifier".to_string(),
                ));
            }
        };

        // Validate we have enough elements for custom ellipsis form
        if custom_ellipsis.is_some() && list.len() < 3 {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules with custom ellipsis requires literals and rules".to_string(),
            ));
        }

        // Parse literals: (symbol ...)
        let literals_value = &list[literals_index];
        let literals = self.parse_literals_list(literals_value)?;

        // Parse rules as (pattern, template) pairs
        let rules_start = literals_index + 1;
        let rules_list = if list.len() > rules_start {
            // Convert rules Vec back to list Value for parsing
            utils::vec_to_list(&list[rules_start..])
        } else {
            Value::Null
        };

        let rules = self.parse_macro_rules(&rules_list)?;

        // Compile using Compiler with environment AND scope set for scope-based hygiene
        let mut compiler =
            Compiler::with_env_and_scopes(literals, custom_ellipsis, env.clone(), scopes.clone());
        compiler
            .compile_macro(name, rules)
            .map_err(|e| DesugarError::InvalidSyntax(format!("Failed to compile macro: {}", e)))
    }

    /// Parse the literals list: (lit1 lit2 ...)
    fn parse_literals_list(&self, expr: &Value) -> Result<Vec<Rc<str>>> {
        let mut literals = Vec::new();
        let mut current = expr.clone();

        while let Value::Pair(pair) = current {
            let pair_ref = pair.borrow();
            // Accept both Symbol and Identifier (from macro expansion)
            match &pair_ref.0 {
                Value::Symbol(s) => literals.push(s.clone()),
                Value::Identifier(id) => literals.push(id.name.clone()),
                _ => {
                    return Err(DesugarError::InvalidSyntax(
                        "syntax-rules literals must be symbols".to_string(),
                    ));
                }
            }
            current = pair_ref.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules literals must be a proper list".to_string(),
            ));
        }

        Ok(literals)
    }

    /// Parse macro rules as (pattern, template) pairs
    fn parse_macro_rules(&self, expr: &Value) -> Result<Vec<(Value, Value)>> {
        let mut rules = Vec::new();
        let mut current = expr.clone();

        while let Value::Pair(rule_pair) = current {
            let rule_pair_ref = rule_pair.borrow();

            // Each rule is (pattern template)
            let rule_list = utils::list_to_vec(&rule_pair_ref.0)?;

            if rule_list.len() != 2 {
                return Err(DesugarError::InvalidSyntax(
                    "Each syntax-rules rule must have exactly 2 elements (pattern template)"
                        .to_string(),
                ));
            }

            rules.push((rule_list[0].clone(), rule_list[1].clone()));
            current = rule_pair_ref.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules rules must be a proper list".to_string(),
            ));
        }

        if rules.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules must have at least one rule".to_string(),
            ));
        }

        Ok(rules)
    }

    /// Desugar application: (func arg1 arg2 ...) → App { func, args }
    fn desugar_app(&self, value: &Value) -> Result<CoreExpr> {
        let list = utils::list_to_vec(value)?;

        if list.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "Cannot evaluate empty list".to_string(),
            ));
        }

        let func = self.desugar(&list[0])?;
        let args: Vec<CoreExpr> = list[1..]
            .iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_>>()?;

        Ok(CoreExpr::App {
            func: Rc::new(func),
            args,
        })
    }
}

impl Default for Desugarer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_ir::Formals;
    use std::cell::RefCell;
    use std::rc::Rc;

    // =========================================================================
    // Self-evaluating literals
    // =========================================================================

    #[test]
    fn test_desugar_integer() {
        let desugarer = Desugarer::new();
        let value = Value::Integer(42);
        let result = desugarer.desugar(&value).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(42)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_boolean() {
        let desugarer = Desugarer::new();
        let value = Value::Boolean(true);
        let result = desugarer.desugar(&value).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Boolean(true)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_string() {
        let desugarer = Desugarer::new();
        let value = Value::String(Rc::new(RefCell::new("hello".to_string())));
        let result = desugarer.desugar(&value).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::String(_)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_character() {
        let desugarer = Desugarer::new();
        let value = Value::Character('a');
        let result = desugarer.desugar(&value).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Character('a')));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    // =========================================================================
    // Variables
    // =========================================================================

    #[test]
    fn test_desugar_variable() {
        let desugarer = Desugarer::new();
        let value = Value::symbol("x");
        let result = desugarer.desugar(&value).unwrap();

        if let CoreExpr::Var { name, scopes } = result {
            assert_eq!(name.as_ref(), "x");
            assert!(scopes.is_empty());
        } else {
            panic!("Expected Var, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: quote
    // =========================================================================

    #[test]
    fn test_desugar_quote_symbol() {
        let desugarer = Desugarer::new();
        // (quote x)
        let list = utils::vec_to_list(&[Value::symbol("quote"), Value::symbol("x")]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Quote(val) = result {
            assert!(matches!(val.as_ref(), Value::Symbol(_)));
        } else {
            panic!("Expected Quote, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_quote_list() {
        let desugarer = Desugarer::new();
        // (quote (1 2 3))
        let list = utils::vec_to_list(&[
            Value::symbol("quote"),
            utils::vec_to_list(&[Value::Integer(1), Value::Integer(2), Value::Integer(3)]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        assert!(matches!(result, CoreExpr::Quote(_)));
    }

    // =========================================================================
    // Core Form: lambda
    // =========================================================================

    #[test]
    fn test_desugar_lambda_fixed_params() {
        let desugarer = Desugarer::new();
        // (lambda (x y) (+ x y))
        let list = utils::vec_to_list(&[
            Value::symbol("lambda"),
            utils::vec_to_list(&[Value::symbol("x"), Value::symbol("y")]),
            utils::vec_to_list(&[Value::symbol("+"), Value::symbol("x"), Value::symbol("y")]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Lambda { params, body, .. } = result {
            assert!(matches!(params, Formals::Fixed(_)));
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Lambda, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_lambda_variadic() {
        let desugarer = Desugarer::new();
        // (lambda args (car args))
        let list = utils::vec_to_list(&[
            Value::symbol("lambda"),
            Value::symbol("args"),
            utils::vec_to_list(&[Value::symbol("car"), Value::symbol("args")]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Lambda { params, body, .. } = result {
            assert!(matches!(params, Formals::Variadic(_)));
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Lambda, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_lambda_rest_params() {
        let desugarer = Desugarer::new();
        // (lambda (x y . rest) x)
        let formals = Value::Pair(Rc::new(RefCell::new((
            Value::symbol("x"),
            Value::Pair(Rc::new(RefCell::new((
                Value::symbol("y"),
                Value::symbol("rest"), // Dotted pair
            )))),
        ))));

        let list = utils::vec_to_list(&[Value::symbol("lambda"), formals, Value::symbol("x")]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Lambda { params, .. } = result {
            assert!(matches!(params, Formals::Mixed { .. }));
        } else {
            panic!("Expected Lambda, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: if
    // =========================================================================

    #[test]
    fn test_desugar_if_three_args() {
        let desugarer = Desugarer::new();
        // (if #t 1 2)
        let list = utils::vec_to_list(&[
            Value::symbol("if"),
            Value::Boolean(true),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::If { test, then, else_ } = result {
            if let CoreExpr::Literal(v) = &*test {
                assert!(matches!(v.as_ref(), Value::Boolean(true)));
            } else {
                panic!("Expected Literal test");
            }
            if let CoreExpr::Literal(v) = &*then {
                assert!(matches!(v.as_ref(), Value::Integer(1)));
            } else {
                panic!("Expected Literal then");
            }
            if let CoreExpr::Literal(v) = &*else_ {
                assert!(matches!(v.as_ref(), Value::Integer(2)));
            } else {
                panic!("Expected Literal else");
            }
        } else {
            panic!("Expected If, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_if_two_args() {
        let desugarer = Desugarer::new();
        // (if #t 1)
        let list =
            utils::vec_to_list(&[Value::symbol("if"), Value::Boolean(true), Value::Integer(1)]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::If { test, then, else_ } = result {
            if let CoreExpr::Literal(v) = &*test {
                assert!(matches!(v.as_ref(), Value::Boolean(true)));
            } else {
                panic!("Expected Literal test");
            }
            if let CoreExpr::Literal(v) = &*then {
                assert!(matches!(v.as_ref(), Value::Integer(1)));
            } else {
                panic!("Expected Literal then");
            }
            if let CoreExpr::Literal(v) = &*else_ {
                assert!(matches!(v.as_ref(), Value::Unspecified));
            } else {
                panic!("Expected Literal else");
            }
        } else {
            panic!("Expected If, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: set!
    // =========================================================================

    #[test]
    fn test_desugar_set() {
        let desugarer = Desugarer::new();
        // (set! x 42)
        let list = utils::vec_to_list(&[
            Value::symbol("set!"),
            Value::symbol("x"),
            Value::Integer(42),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Set { var, scopes, value } = result {
            assert_eq!(var.as_ref(), "x");
            assert!(scopes.is_empty());
            if let CoreExpr::Literal(v) = &*value {
                assert!(matches!(v.as_ref(), Value::Integer(42)));
            } else {
                panic!("Expected Literal value");
            }
        } else {
            panic!("Expected Set, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_set_non_symbol_error() {
        let desugarer = Desugarer::new();
        // (set! 123 42) - invalid
        let list = utils::vec_to_list(&[
            Value::symbol("set!"),
            Value::Integer(123),
            Value::Integer(42),
        ]);

        let result = desugarer.desugar(&list);
        assert!(result.is_err());
    }

    // =========================================================================
    // Core Form: define
    // =========================================================================

    #[test]
    fn test_desugar_define_variable() {
        let desugarer = Desugarer::new();
        // (define x 42)
        let list = utils::vec_to_list(&[
            Value::symbol("define"),
            Value::symbol("x"),
            Value::Integer(42),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Define { name, value } = result {
            assert_eq!(name.as_ref(), "x");
            if let CoreExpr::Literal(v) = &*value {
                assert!(matches!(v.as_ref(), Value::Integer(42)));
            } else {
                panic!("Expected Literal value");
            }
        } else {
            panic!("Expected Define, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_define_function() {
        let desugarer = Desugarer::new();
        // (define (add x y) (+ x y))
        let list = utils::vec_to_list(&[
            Value::symbol("define"),
            utils::vec_to_list(&[Value::symbol("add"), Value::symbol("x"), Value::symbol("y")]),
            utils::vec_to_list(&[Value::symbol("+"), Value::symbol("x"), Value::symbol("y")]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Define { name, value } = result {
            assert_eq!(name.as_ref(), "add");
            assert!(matches!(*value, CoreExpr::Lambda { .. }));
        } else {
            panic!("Expected Define, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: begin
    // =========================================================================

    #[test]
    fn test_desugar_begin_single_expr() {
        let desugarer = Desugarer::new();
        // (begin 42)
        let list = utils::vec_to_list(&[Value::symbol("begin"), Value::Integer(42)]);

        let result = desugarer.desugar(&list).unwrap();
        // Single expression should be optimized away
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(42)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_begin_multiple_exprs() {
        let desugarer = Desugarer::new();
        // (begin 1 2 3)
        let list = utils::vec_to_list(&[
            Value::symbol("begin"),
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Begin(exprs) = result {
            assert_eq!(exprs.len(), 3);
        } else {
            panic!("Expected Begin, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_begin_empty() {
        let desugarer = Desugarer::new();
        // (begin) → #<unspecified> (R7RS allows this)
        let list = utils::vec_to_list(&[Value::symbol("begin")]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Unspecified));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    // =========================================================================
    // Application
    // =========================================================================

    #[test]
    fn test_desugar_application() {
        let desugarer = Desugarer::new();
        // (+ 1 2)
        let list = utils::vec_to_list(&[Value::symbol("+"), Value::Integer(1), Value::Integer(2)]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::App { func, args } = result {
            assert!(matches!(*func, CoreExpr::Var { .. }));
            assert_eq!(args.len(), 2);
        } else {
            panic!("Expected App, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_lambda_application() {
        let desugarer = Desugarer::new();
        // ((lambda (x) x) 42)
        let list = utils::vec_to_list(&[
            utils::vec_to_list(&[
                Value::symbol("lambda"),
                utils::vec_to_list(&[Value::symbol("x")]),
                Value::symbol("x"),
            ]),
            Value::Integer(42),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::App { func, args } = result {
            assert!(matches!(*func, CoreExpr::Lambda { .. }));
            assert_eq!(args.len(), 1);
        } else {
            panic!("Expected App, got {:?}", result);
        }
    }

    // =========================================================================
    // Error cases
    // =========================================================================

    #[test]
    fn test_desugar_empty_list_error() {
        let desugarer = Desugarer::new();
        let list = Value::Null;
        let result = desugarer.desugar(&list);
        // Empty list as literal is fine
        assert!(result.is_ok());
    }

    #[test]
    fn test_desugar_runtime_value_error() {
        let desugarer = Desugarer::new();
        // Runtime-only values like Eof shouldn't appear in AST
        let value = Value::Eof;
        let result = desugarer.desugar(&value);
        assert!(result.is_err());
    }

    // =========================================================================
    // cond-expand
    // =========================================================================

    #[test]
    fn test_cond_expand_r7rs_feature() {
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs 42))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[Value::symbol("r7rs"), Value::Integer(42)]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(42)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_patina_feature() {
        let desugarer = Desugarer::new();
        // (cond-expand (patina 'patina-impl))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[
                Value::symbol("patina"),
                utils::vec_to_list(&[Value::symbol("quote"), Value::symbol("patina-impl")]),
            ]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        // Should be Quote expression
        assert!(matches!(result, CoreExpr::Quote(_)));
    }

    #[test]
    fn test_cond_expand_else_clause() {
        let desugarer = Desugarer::new();
        // (cond-expand (nonexistent 1) (else 99))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[Value::symbol("nonexistent"), Value::Integer(1)]),
            utils::vec_to_list(&[Value::symbol("else"), Value::Integer(99)]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(99)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_no_match_error() {
        let desugarer = Desugarer::new();
        // (cond-expand (nonexistent 1))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[Value::symbol("nonexistent"), Value::Integer(1)]),
        ]);

        let result = desugarer.desugar(&list);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("no matching clause"));
    }

    #[test]
    fn test_cond_expand_and_requirement() {
        let desugarer = Desugarer::new();
        // (cond-expand ((and r7rs patina) 100))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[
                utils::vec_to_list(&[
                    Value::symbol("and"),
                    Value::symbol("r7rs"),
                    Value::symbol("patina"),
                ]),
                Value::Integer(100),
            ]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(100)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_or_requirement() {
        let desugarer = Desugarer::new();
        // (cond-expand ((or nonexistent r7rs) 200))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[
                utils::vec_to_list(&[
                    Value::symbol("or"),
                    Value::symbol("nonexistent"),
                    Value::symbol("r7rs"),
                ]),
                Value::Integer(200),
            ]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(200)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_not_requirement() {
        let desugarer = Desugarer::new();
        // (cond-expand ((not nonexistent) 300))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[
                utils::vec_to_list(&[Value::symbol("not"), Value::symbol("nonexistent")]),
                Value::Integer(300),
            ]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(300)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_multiple_expressions() {
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs 1 2 3))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[
                Value::symbol("r7rs"),
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Begin(exprs) = result {
            assert_eq!(exprs.len(), 3);
        } else {
            panic!("Expected Begin, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_first_match_wins() {
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs 1) (patina 2) (else 3))
        // r7rs should match first
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[Value::symbol("r7rs"), Value::Integer(1)]),
            utils::vec_to_list(&[Value::symbol("patina"), Value::Integer(2)]),
            utils::vec_to_list(&[Value::symbol("else"), Value::Integer(3)]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Integer(1)));
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_else_not_last_error() {
        let desugarer = Desugarer::new();
        // (cond-expand (else 1) (r7rs 2))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[Value::symbol("else"), Value::Integer(1)]),
            utils::vec_to_list(&[Value::symbol("r7rs"), Value::Integer(2)]),
        ]);

        let result = desugarer.desugar(&list);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("else clause must be last"));
    }

    #[test]
    fn test_cond_expand_empty_body() {
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs))
        let list = utils::vec_to_list(&[
            Value::symbol("cond-expand"),
            utils::vec_to_list(&[Value::symbol("r7rs")]),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Literal(v) = result {
            assert!(matches!(v.as_ref(), Value::Unspecified));
        } else {
            panic!("Expected Literal(Unspecified), got {:?}", result);
        }
    }
}
