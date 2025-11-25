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
pub struct Desugarer {
    /// Optional environment for macro lookup
    /// If None, macros cannot be expanded (will cause desugar errors)
    env: Option<Rc<Environment>>,

    /// Current scope set for scope-based hygiene
    /// Accumulates scopes as we enter binding forms
    current_scopes: ScopeSet,
}

impl Desugarer {
    /// Create a new desugarer without macro expansion support
    ///
    /// This is used when the input is already fully macro-expanded.
    pub fn new() -> Self {
        Self {
            env: None,
            current_scopes: ScopeSet::new(),
        }
    }

    /// Create a new desugarer with macro expansion support
    ///
    /// This allows the desugarer to expand macros as it encounters them.
    /// The environment is used to look up macro definitions.
    pub fn with_env(env: Rc<Environment>) -> Self {
        Self {
            env: Some(env),
            current_scopes: ScopeSet::new(),
        }
    }

    /// Create a new desugarer with environment and specific scope set
    ///
    /// Used when creating child desugarers that inherit scope context.
    pub fn with_env_and_scopes(env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env: Some(env),
            current_scopes: scopes,
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
        };
        (desugarer, scope)
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
            | Value::Complex(_, _)
            | Value::Character(_)
            | Value::String(_)
            | Value::Bytevector(_)
            | Value::Unspecified => Ok(CoreExpr::Literal(value.clone())),

            // Variable reference
            Value::Symbol(s) => Ok(CoreExpr::Var(s.clone())),

            // Identifier (unified hygiene: marks-and-ribs + scope-sets)
            // If scopes is non-empty, use scope-based lookup (ScopedVar)
            // Otherwise use simple name lookup (Var) - marks handled at eval time
            Value::Identifier { name, scopes, .. } => {
                if scopes.is_empty() {
                    Ok(CoreExpr::Var(name.clone()))
                } else {
                    Ok(CoreExpr::ScopedVar {
                        name: name.clone(),
                        scopes: scopes.clone(),
                    })
                }
            }

            // Empty list (unusual in AST, but possible as literal)
            Value::Null => Ok(CoreExpr::Literal(Value::Null)),

            // Lists - special forms or application
            Value::Pair(_) => self.desugar_list(value),

            // Vectors - literal
            Value::Vector(_) => Ok(CoreExpr::Literal(value.clone())),

            // Runtime-only values should never appear in AST
            Value::Procedure(_)
            | Value::Parameter { .. }
            | Value::Macro { .. }
            | Value::Library(_)
            | Value::InputPort
            | Value::OutputPort
            | Value::Promise(_)
            | Value::Eof
            | Value::Values(_) => Err(DesugarError::RuntimeValueInAST {
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

        // MACRO-AWARE DESUGARING: Check if this is a macro call FIRST
        // If we have an environment, check for macros before special forms
        if let (Some(env), Value::Symbol(sym)) = (&self.env, &car)
            && let Some(Value::Macro { data, .. }) = env.get(sym)
        {
            // This is a macro! Expand it and recursively desugar the result
            let compiled_macro = data
                .downcast_ref::<patina_macros::CompiledMacro>()
                .ok_or_else(|| DesugarError::InvalidSyntax("Invalid macro data".to_string()))?;

            // Expand the macro
            let expanded =
                patina_macros::expand_macro(compiled_macro, value, env).map_err(|e| {
                    DesugarError::InvalidSyntax(format!("Macro expansion failed: {}", e))
                })?;

            // Recursively desugar the expanded result
            // This handles the case where macros expand to other macros
            return self.desugar(&expanded);
        }

        // Check if it's a core special form
        if let Value::Symbol(sym) = &car {
            match sym.as_ref() {
                // The core forms
                "quote" => self.desugar_quote(&cdr),
                "quasiquote" => self.desugar_quasiquote(&cdr),
                "lambda" => self.desugar_lambda(&cdr),
                "if" => self.desugar_if(&cdr),
                "set!" => self.desugar_set(&cdr),
                "define" => self.desugar_define(&cdr),
                "define-syntax" => self.desugar_define_syntax(&cdr),
                "import" => self.desugar_import(&cdr),
                "parameterize" => self.desugar_parameterize(&cdr),
                "begin" => self.desugar_begin(&cdr),
                "apply" => self.desugar_apply(&cdr),
                "expand" => self.desugar_expand(&cdr),
                "case-lambda" => self.desugar_case_lambda(&cdr),

                // Let-syntax forms: compile macros and desugar body
                "let-syntax" => self.desugar_let_syntax(&cdr),
                "letrec-syntax" => self.desugar_letrec_syntax(&cdr),

                // Everything else is either:
                // - A derived form that was already expanded by macros
                // - A procedure application
                _ => self.desugar_app(value),
            }
        } else {
            // ((lambda ...) args) or other complex operator
            self.desugar_app(value)
        }
    }

    /// Desugar quote: (quote datum) → Quote(datum)
    fn desugar_quote(&self, args: &Value) -> Result<CoreExpr> {
        let datum = utils::expect_one_arg(args, "quote")?;
        Ok(CoreExpr::Quote(datum.clone()))
    }

    /// Desugar quasiquote: (quasiquote template) → Quasiquote(template)
    /// The template is kept as-is (Value), and will be processed by the evaluator
    /// to handle unquote and unquote-splicing
    fn desugar_quasiquote(&self, args: &Value) -> Result<CoreExpr> {
        let template = utils::expect_one_arg(args, "quasiquote")?;
        Ok(CoreExpr::Quasiquote(template.clone()))
    }

    /// Desugar lambda: (lambda formals body ...) → Lambda { params, body }
    fn desugar_lambda(&self, args: &Value) -> Result<CoreExpr> {
        let (formals, body) = utils::parse_lambda_syntax(args)?;

        // Create a fresh scope for this lambda's bindings
        // Every lambda gets a scope so that let-syntax inside can properly capture
        // the lexical context.
        let binding_scope = ScopeId::fresh();
        let body_scopes = self.current_scopes.with_scope(binding_scope);

        // Desugar body with the new scope set
        // This ensures any let-syntax inside the lambda will capture the lambda's scope
        let body_desugarer = Desugarer {
            env: self.env.clone(),
            current_scopes: body_scopes,
        };

        let body_exprs: Vec<CoreExpr> = body
            .iter()
            .map(|e| body_desugarer.desugar(e))
            .collect::<Result<_>>()?;

        Ok(CoreExpr::Lambda {
            params: utils::convert_formals(&formals)?,
            body: body_exprs,
            binding_scope: Some(binding_scope),
        })
    }

    /// Desugar if: (if test then [else]) → If { test, then, else_ }
    fn desugar_if(&self, args: &Value) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec(args)?;

        match args_vec.as_slice() {
            [test, then] => {
                // Two-arg if: (if test then) → (if test then #<unspecified>)
                Ok(CoreExpr::If {
                    test: Box::new(self.desugar(test)?),
                    then: Box::new(self.desugar(then)?),
                    else_: Box::new(CoreExpr::Literal(Value::Unspecified)),
                })
            }
            [test, then, else_] => {
                // Three-arg if: (if test then else)
                Ok(CoreExpr::If {
                    test: Box::new(self.desugar(test)?),
                    then: Box::new(self.desugar(then)?),
                    else_: Box::new(self.desugar(else_)?),
                })
            }
            _ => Err(DesugarError::WrongArgCount {
                form: "if".to_string(),
                expected: "2 or 3".to_string(),
                got: args_vec.len(),
            }),
        }
    }

    /// Desugar set!: (set! var value) → Set { var, value }
    fn desugar_set(&self, args: &Value) -> Result<CoreExpr> {
        let (var, value) = utils::expect_two_args(args, "set!")?;

        let var_sym = match &var {
            Value::Symbol(s) => s.clone(),
            // Handle hygienic identifiers from macro expansion
            Value::Identifier { name, .. } => name.clone(),
            _ => {
                return Err(DesugarError::InvalidSyntax(
                    "set! requires a symbol as first argument".to_string(),
                ));
            }
        };

        Ok(CoreExpr::Set {
            var: var_sym,
            value: Box::new(self.desugar(&value)?),
        })
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
                value: Box::new(self.desugar(value)?),
            }),

            // Handle hygienic identifiers for variable names
            [Value::Identifier { name, .. }, value] => Ok(CoreExpr::Define {
                name: name.clone(),
                value: Box::new(self.desugar(value)?),
            }),

            // (define () value) - empty variable list from define-values
            // Evaluate value for side effects, discard result
            [Value::Null, value] => {
                // Transform to: (begin value #<unspecified>)
                Ok(CoreExpr::Begin(vec![
                    self.desugar(value)?,
                    CoreExpr::Literal(Value::Unspecified),
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

                // Desugar body with the new scope set
                let body_desugarer = Desugarer {
                    env: self.env.clone(),
                    current_scopes: body_scopes,
                };

                let lambda = CoreExpr::Lambda {
                    params: utils::convert_formals(&params)?,
                    body: body
                        .iter()
                        .map(|e| body_desugarer.desugar(e))
                        .collect::<Result<_>>()?,
                    binding_scope: Some(binding_scope),
                };

                Ok(CoreExpr::Define {
                    name,
                    value: Box::new(lambda),
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
    /// The transformer is kept as a Value (not desugared) because it's template data,
    /// not code to be evaluated. Similar to how quote keeps its datum as a Value.
    fn desugar_define_syntax(&self, args: &Value) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec(args)?;

        match args_vec.as_slice() {
            // (define-syntax name transformer)
            [Value::Symbol(name), transformer] => Ok(CoreExpr::DefineSyntax {
                name: name.clone(),
                transformer: transformer.clone(), // Keep as Value, don't desugar
                definition_scopes: self.current_scopes.clone(), // Capture scopes for hygiene
            }),

            _ => Err(DesugarError::InvalidSyntax(
                "define-syntax requires (define-syntax name transformer)".to_string(),
            )),
        }
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
            return Ok(CoreExpr::Literal(Value::Unspecified));
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
        let func = Box::new(self.desugar(&exprs[0])?);

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
            expr: Box::new(self.desugar(&expr)?),
        })
    }

    /// Desugar case-lambda: (case-lambda clause ...) → CaseLambda { clauses }
    ///
    /// Each clause has the form (params body...) where params follows lambda syntax.
    fn desugar_case_lambda(&self, args: &Value) -> Result<CoreExpr> {
        let clauses = utils::list_to_vec(args)?;

        if clauses.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "case-lambda requires at least one clause".to_string(),
            ));
        }

        let mut desugared_clauses = Vec::new();

        for clause in clauses {
            // Each clause should be (params body...)
            let clause_list = utils::list_to_vec(&clause)?;

            if clause_list.len() < 2 {
                return Err(DesugarError::InvalidSyntax(
                    "case-lambda clause must have params and at least one body expression"
                        .to_string(),
                ));
            }

            // First element is params
            let params = &clause_list[0];
            let formals = utils::convert_formals(params)?;

            // Rest are body expressions
            let body_exprs: Vec<CoreExpr> = clause_list[1..]
                .iter()
                .map(|e| self.desugar(e))
                .collect::<Result<_>>()?;

            desugared_clauses.push(patina_ir::CaseLambdaClause {
                params: formals,
                body: body_exprs,
            });
        }

        Ok(CoreExpr::CaseLambda {
            clauses: desugared_clauses,
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

            // Extract name as symbol
            let name = match &binding_vec[0] {
                Value::Symbol(s) => s.clone(),
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
            let macro_value = Value::Macro {
                name: name.clone(),
                data: Rc::new(compiled_macro),
            };
            body_env.define(name.to_string(), macro_value);
        }

        // Create desugarer with extended environment AND scope set
        // The body sees the let-syntax scope (for bindings introduced by macros)
        let body_desugarer = Desugarer::with_env_and_scopes(body_env, definition_scopes);

        // Desugar body expressions in extended environment
        let body = &args_vec[1..];
        let desugared_body: Vec<CoreExpr> = body
            .iter()
            .map(|e| body_desugarer.desugar(e))
            .collect::<Result<_>>()?;

        // Return body as Begin expression
        // Optimize: single expression doesn't need Begin wrapper
        if desugared_body.len() == 1 {
            Ok(desugared_body.into_iter().next().unwrap())
        } else {
            Ok(CoreExpr::Begin(desugared_body))
        }
    }

    /// Compile a syntax-rules transformer into a CompiledMacro
    ///
    /// This is a helper method that mirrors what the evaluator does,
    /// but is accessible during desugaring.
    #[allow(dead_code)]
    fn compile_syntax_rules(
        &self,
        expr: &Value,
        name: Rc<str>,
        env: &Rc<Environment>,
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
        match &list[0] {
            Value::Symbol(s) if s.as_ref() == "syntax-rules" => {}
            _ => {
                return Err(DesugarError::InvalidSyntax(
                    "Expected syntax-rules".to_string(),
                ));
            }
        }

        if list.len() < 2 {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules requires literals and rules".to_string(),
            ));
        }

        // Parse literals: (symbol ...)
        let literals_value = &list[1];
        let literals = self.parse_literals_list(literals_value)?;

        // Parse rules as (pattern, template) pairs
        let rules_list = if list.len() > 2 {
            // Convert rules Vec back to list Value for parsing
            utils::vec_to_list(&list[2..])
        } else {
            Value::Null
        };

        let rules = self.parse_macro_rules(&rules_list)?;

        // Compile using Compiler with environment capture for hygiene
        // Pass the runtime Environment as Rc<dyn Any> following Gauche's approach
        let mut compiler = Compiler::with_env(literals, None, env.clone() as Rc<dyn std::any::Any>);
        compiler
            .compile_macro(name, rules)
            .map_err(|e| DesugarError::InvalidSyntax(format!("Failed to compile macro: {}", e)))
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
        match &list[0] {
            Value::Symbol(s) if s.as_ref() == "syntax-rules" => {}
            _ => {
                return Err(DesugarError::InvalidSyntax(
                    "Expected syntax-rules".to_string(),
                ));
            }
        }

        if list.len() < 2 {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules requires literals and rules".to_string(),
            ));
        }

        // Parse literals: (symbol ...)
        let literals_value = &list[1];
        let literals = self.parse_literals_list(literals_value)?;

        // Parse rules as (pattern, template) pairs
        let rules_list = if list.len() > 2 {
            // Convert rules Vec back to list Value for parsing
            utils::vec_to_list(&list[2..])
        } else {
            Value::Null
        };

        let rules = self.parse_macro_rules(&rules_list)?;

        // Compile using Compiler with environment AND scope set for scope-based hygiene
        let mut compiler = Compiler::with_env_and_scopes(
            literals,
            None,
            env.clone() as Rc<dyn std::any::Any>,
            scopes.clone(),
        );
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
            match &pair_ref.0 {
                Value::Symbol(s) => literals.push(s.clone()),
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
            func: Box::new(func),
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
        assert!(matches!(result, CoreExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn test_desugar_boolean() {
        let desugarer = Desugarer::new();
        let value = Value::Boolean(true);
        let result = desugarer.desugar(&value).unwrap();
        assert!(matches!(result, CoreExpr::Literal(Value::Boolean(true))));
    }

    #[test]
    fn test_desugar_string() {
        let desugarer = Desugarer::new();
        let value = Value::String(Rc::new(RefCell::new("hello".to_string())));
        let result = desugarer.desugar(&value).unwrap();
        assert!(matches!(result, CoreExpr::Literal(Value::String(_))));
    }

    #[test]
    fn test_desugar_character() {
        let desugarer = Desugarer::new();
        let value = Value::Character('a');
        let result = desugarer.desugar(&value).unwrap();
        assert!(matches!(result, CoreExpr::Literal(Value::Character('a'))));
    }

    // =========================================================================
    // Variables
    // =========================================================================

    #[test]
    fn test_desugar_variable() {
        let desugarer = Desugarer::new();
        let value = Value::Symbol(Rc::from("x"));
        let result = desugarer.desugar(&value).unwrap();

        if let CoreExpr::Var(name) = result {
            assert_eq!(name.as_ref(), "x");
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
        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("quote")),
            Value::Symbol(Rc::from("x")),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Quote(val) = result {
            assert!(matches!(val, Value::Symbol(_)));
        } else {
            panic!("Expected Quote, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_quote_list() {
        let desugarer = Desugarer::new();
        // (quote (1 2 3))
        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("quote")),
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
            Value::Symbol(Rc::from("lambda")),
            utils::vec_to_list(&[Value::Symbol(Rc::from("x")), Value::Symbol(Rc::from("y"))]),
            utils::vec_to_list(&[
                Value::Symbol(Rc::from("+")),
                Value::Symbol(Rc::from("x")),
                Value::Symbol(Rc::from("y")),
            ]),
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
            Value::Symbol(Rc::from("lambda")),
            Value::Symbol(Rc::from("args")),
            utils::vec_to_list(&[
                Value::Symbol(Rc::from("car")),
                Value::Symbol(Rc::from("args")),
            ]),
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
            Value::Symbol(Rc::from("x")),
            Value::Pair(Rc::new(RefCell::new((
                Value::Symbol(Rc::from("y")),
                Value::Symbol(Rc::from("rest")), // Dotted pair
            )))),
        ))));

        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("lambda")),
            formals,
            Value::Symbol(Rc::from("x")),
        ]);

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
            Value::Symbol(Rc::from("if")),
            Value::Boolean(true),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::If { test, then, else_ } = result {
            assert!(matches!(*test, CoreExpr::Literal(Value::Boolean(true))));
            assert!(matches!(*then, CoreExpr::Literal(Value::Integer(1))));
            assert!(matches!(*else_, CoreExpr::Literal(Value::Integer(2))));
        } else {
            panic!("Expected If, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_if_two_args() {
        let desugarer = Desugarer::new();
        // (if #t 1)
        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("if")),
            Value::Boolean(true),
            Value::Integer(1),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::If { test, then, else_ } = result {
            assert!(matches!(*test, CoreExpr::Literal(Value::Boolean(true))));
            assert!(matches!(*then, CoreExpr::Literal(Value::Integer(1))));
            assert!(matches!(*else_, CoreExpr::Literal(Value::Unspecified)));
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
            Value::Symbol(Rc::from("set!")),
            Value::Symbol(Rc::from("x")),
            Value::Integer(42),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Set { var, value } = result {
            assert_eq!(var.as_ref(), "x");
            assert!(matches!(*value, CoreExpr::Literal(Value::Integer(42))));
        } else {
            panic!("Expected Set, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_set_non_symbol_error() {
        let desugarer = Desugarer::new();
        // (set! 123 42) - invalid
        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("set!")),
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
            Value::Symbol(Rc::from("define")),
            Value::Symbol(Rc::from("x")),
            Value::Integer(42),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::Define { name, value } = result {
            assert_eq!(name.as_ref(), "x");
            assert!(matches!(*value, CoreExpr::Literal(Value::Integer(42))));
        } else {
            panic!("Expected Define, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_define_function() {
        let desugarer = Desugarer::new();
        // (define (add x y) (+ x y))
        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("define")),
            utils::vec_to_list(&[
                Value::Symbol(Rc::from("add")),
                Value::Symbol(Rc::from("x")),
                Value::Symbol(Rc::from("y")),
            ]),
            utils::vec_to_list(&[
                Value::Symbol(Rc::from("+")),
                Value::Symbol(Rc::from("x")),
                Value::Symbol(Rc::from("y")),
            ]),
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
        let list = utils::vec_to_list(&[Value::Symbol(Rc::from("begin")), Value::Integer(42)]);

        let result = desugarer.desugar(&list).unwrap();
        // Single expression should be optimized away
        assert!(matches!(result, CoreExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn test_desugar_begin_multiple_exprs() {
        let desugarer = Desugarer::new();
        // (begin 1 2 3)
        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("begin")),
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
        let list = utils::vec_to_list(&[Value::Symbol(Rc::from("begin"))]);

        let result = desugarer.desugar(&list).unwrap();
        assert!(matches!(result, CoreExpr::Literal(Value::Unspecified)));
    }

    // =========================================================================
    // Application
    // =========================================================================

    #[test]
    fn test_desugar_application() {
        let desugarer = Desugarer::new();
        // (+ 1 2)
        let list = utils::vec_to_list(&[
            Value::Symbol(Rc::from("+")),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        let result = desugarer.desugar(&list).unwrap();
        if let CoreExpr::App { func, args } = result {
            assert!(matches!(*func, CoreExpr::Var(_)));
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
                Value::Symbol(Rc::from("lambda")),
                utils::vec_to_list(&[Value::Symbol(Rc::from("x"))]),
                Value::Symbol(Rc::from("x")),
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
}
