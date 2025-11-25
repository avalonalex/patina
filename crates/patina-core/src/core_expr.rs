use crate::scope::{ScopeId, ScopeSet};
use crate::value::Value;
use std::rc::Rc;

/// Symbol type (interned string)
pub type Symbol = Rc<str>;

/// Lambda parameter forms (fixed, variadic, or mixed)
#[derive(Debug, Clone, PartialEq)]
pub enum Formals {
    /// Fixed arity: (lambda (x y z) ...)
    Fixed(Vec<Symbol>),

    /// Variadic: (lambda args ...)
    Variadic(Symbol),

    /// Mixed: (lambda (x y . rest) ...)
    Mixed { fixed: Vec<Symbol>, rest: Symbol },
}

/// Case-lambda clause: (params body...)
#[derive(Debug, Clone)]
pub struct CaseLambdaClause {
    /// Parameter specification
    pub params: Formals,
    /// Body expressions
    pub body: Vec<CoreExpr>,
}

/// Core Scheme expressions after macro expansion and desugaring
///
/// This is the minimal IR that backends must handle.
/// All macros and derived forms are eliminated by the frontend.
#[derive(Debug, Clone)]
pub enum CoreExpr {
    /// Literal values: numbers, booleans, strings, etc.
    /// Example: 42, #t, "hello"
    Literal(Value),

    /// Variable reference
    /// Example: x, my-function
    Var(Symbol),

    /// Variable reference with scope set (for hygienic macros)
    /// Used when an identifier carries scope information from macro expansion.
    /// The evaluator uses scope-based lookup: finds binding where
    /// binding.scopes ⊆ reference.scopes.
    /// Example: x with scopes {S1, S2}
    ScopedVar { name: Symbol, scopes: ScopeSet },

    /// Quote: literal data
    /// Example: 'x, '(1 2 3)
    Quote(Value),

    /// Quasiquote: template with selective evaluation
    /// Example: `(a ,b ,@c) where b and c are evaluated
    /// The template is stored as a Value, and will be processed
    /// recursively by the evaluator to handle unquote/unquote-splicing
    Quasiquote(Value),

    /// Lambda abstraction
    /// Example: (lambda (x y) (+ x y))
    Lambda {
        params: Formals,
        body: Vec<CoreExpr>,
        /// Optional scope for parameter bindings (for scope-based hygiene)
        /// If Some, parameters are bound with this scope added to current scope set.
        /// This enables scope-based hygiene: free variables with matching scopes
        /// can see these bindings, while others cannot.
        binding_scope: Option<ScopeId>,
    },

    /// Conditional (always ternary after desugaring)
    /// Example: (if test then else)
    If {
        test: Box<CoreExpr>,
        then: Box<CoreExpr>,
        else_: Box<CoreExpr>,
    },

    /// Assignment
    /// Example: (set! x 42)
    Set { var: Symbol, value: Box<CoreExpr> },

    /// Sequencing
    /// Example: (begin expr1 expr2 expr3)
    Begin(Vec<CoreExpr>),

    /// Top-level definition
    /// Example: (define x 42), (define (f x) x)
    Define { name: Symbol, value: Box<CoreExpr> },

    /// Macro definition
    /// Example: (define-syntax when (syntax-rules () ...))
    /// The transformer is typically (syntax-rules ...) and is stored as-is (not desugared)
    /// It will be compiled to a Macro value by the evaluator
    DefineSyntax {
        name: Symbol,
        transformer: Value,          // Template data, not code - similar to Quote
        definition_scopes: ScopeSet, // Scopes at macro definition time for hygiene
    },

    /// Import: load library bindings
    /// Example: (import (scheme base))
    /// Import sets are kept as Values (declarative data, not code)
    Import { import_sets: Vec<Value> },

    /// Parameterize: dynamically rebind parameters
    /// Example: (parameterize ((param1 val1) (param2 val2)) body ...)
    /// Note: Body is NOT in tail position (TCO disabled to ensure proper stack cleanup)
    Parameterize {
        bindings: Vec<(CoreExpr, CoreExpr)>, // (param-expr, value-expr) pairs
        body: Vec<CoreExpr>,
    },

    /// Expand: show macro expansion without evaluating
    /// Example: (expand '(let ((x 1)) x)) => ((lambda (x) x) 1)
    /// This is a Patina debugging extension, not part of R7RS
    Expand { expr: Box<CoreExpr> },

    /// Case-lambda: multi-arity procedure with dispatch
    /// Example: (case-lambda (() 'zero) ((x) x) ((x y) (cons x y)))
    /// From R7RS (scheme case-lambda)
    CaseLambda { clauses: Vec<CaseLambdaClause> },

    /// Function application
    /// Example: (f x y), (+ 1 2)
    App {
        func: Box<CoreExpr>,
        args: Vec<CoreExpr>,
    },

    /// Apply: apply procedure to list
    /// Example: (apply + '(1 2 3)), (apply f x y zs)
    /// Last argument is a list that gets spliced as arguments
    Apply {
        func: Box<CoreExpr>,
        args: Vec<CoreExpr>, // All args including the final list
    },

    // Optional optimized forms (added by passes)
    /// Primitive call (after optimization pass recognizes primitives)
    /// Example: (+ 1 2) where + is known to be the primitive
    PrimCall {
        prim: Primitive,
        args: Vec<CoreExpr>,
    },

    /// Local binding (after optimization pass recognizes let pattern)
    /// Example: Internal representation of ((lambda (x) body) value)
    Let {
        bindings: Vec<(Symbol, CoreExpr)>,
        body: Box<CoreExpr>,
    },
}

/// Primitive operations recognized by the optimizer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,

    // Comparison
    NumEq,
    Lt,
    Gt,
    Lte,
    Gte,

    // List operations
    Cons,
    Car,
    Cdr,
    List,

    // Type predicates
    IsNull,
    IsPair,
    IsNumber,
    IsBoolean,
    IsString,
    IsSymbol,
    // More primitives will be added as needed
}

impl CoreExpr {
    /// Check if this expression is in tail position
    pub fn is_tail_position(&self) -> bool {
        matches!(
            self,
            CoreExpr::If { .. } | CoreExpr::Begin(_) | CoreExpr::Let { .. } | CoreExpr::App { .. }
        )
    }

    /// Get a human-readable description of the expression type
    pub fn kind(&self) -> &'static str {
        match self {
            CoreExpr::Literal(_) => "literal",
            CoreExpr::Var(_) => "variable",
            CoreExpr::ScopedVar { .. } => "scoped-variable",
            CoreExpr::Quote(_) => "quote",
            CoreExpr::Quasiquote(_) => "quasiquote",
            CoreExpr::Lambda { .. } => "lambda",
            CoreExpr::If { .. } => "if",
            CoreExpr::Set { .. } => "set!",
            CoreExpr::Begin(_) => "begin",
            CoreExpr::Define { .. } => "define",
            CoreExpr::DefineSyntax { .. } => "define-syntax",
            CoreExpr::Import { .. } => "import",
            CoreExpr::Parameterize { .. } => "parameterize",
            CoreExpr::Expand { .. } => "expand",
            CoreExpr::CaseLambda { .. } => "case-lambda",
            CoreExpr::App { .. } => "application",
            CoreExpr::Apply { .. } => "apply",
            CoreExpr::PrimCall { .. } => "primitive-call",
            CoreExpr::Let { .. } => "let",
        }
    }

    /// Map a function over all immediate children of this expression
    ///
    /// This is useful for implementing recursive transformations in compiler passes.
    pub fn map_children<F>(&self, f: F) -> CoreExpr
    where
        F: Fn(&CoreExpr) -> CoreExpr,
    {
        match self {
            CoreExpr::Literal(_)
            | CoreExpr::Var(_)
            | CoreExpr::ScopedVar { .. }
            | CoreExpr::Quote(_)
            | CoreExpr::Quasiquote(_) => self.clone(),

            CoreExpr::Lambda {
                params,
                body,
                binding_scope,
            } => CoreExpr::Lambda {
                params: params.clone(),
                body: body.iter().map(&f).collect(),
                binding_scope: *binding_scope,
            },

            CoreExpr::If { test, then, else_ } => CoreExpr::If {
                test: Box::new(f(test)),
                then: Box::new(f(then)),
                else_: Box::new(f(else_)),
            },

            CoreExpr::Set { var, value } => CoreExpr::Set {
                var: var.clone(),
                value: Box::new(f(value)),
            },

            CoreExpr::Begin(exprs) => CoreExpr::Begin(exprs.iter().map(&f).collect()),

            CoreExpr::Define { name, value } => CoreExpr::Define {
                name: name.clone(),
                value: Box::new(f(value)),
            },

            CoreExpr::DefineSyntax {
                name,
                transformer,
                definition_scopes,
            } => CoreExpr::DefineSyntax {
                name: name.clone(),
                transformer: transformer.clone(),
                definition_scopes: definition_scopes.clone(),
            },

            CoreExpr::Import { import_sets } => CoreExpr::Import {
                import_sets: import_sets.clone(),
            },

            CoreExpr::Parameterize { bindings, body } => CoreExpr::Parameterize {
                bindings: bindings
                    .iter()
                    .map(|(param, val)| (f(param), f(val)))
                    .collect(),
                body: body.iter().map(&f).collect(),
            },

            CoreExpr::Expand { expr } => CoreExpr::Expand {
                expr: Box::new(f(expr)),
            },

            CoreExpr::CaseLambda { clauses } => CoreExpr::CaseLambda {
                clauses: clauses
                    .iter()
                    .map(|clause| CaseLambdaClause {
                        params: clause.params.clone(),
                        body: clause.body.iter().map(&f).collect(),
                    })
                    .collect(),
            },

            CoreExpr::App { func, args } => CoreExpr::App {
                func: Box::new(f(func)),
                args: args.iter().map(&f).collect(),
            },

            CoreExpr::Apply { func, args } => CoreExpr::Apply {
                func: Box::new(f(func)),
                args: args.iter().map(&f).collect(),
            },

            CoreExpr::PrimCall { prim, args } => CoreExpr::PrimCall {
                prim: *prim,
                args: args.iter().map(&f).collect(),
            },

            CoreExpr::Let { bindings, body } => CoreExpr::Let {
                bindings: bindings
                    .iter()
                    .map(|(var, val)| (var.clone(), f(val)))
                    .collect(),
                body: Box::new(f(body)),
            },
        }
    }
}

impl std::fmt::Display for CoreExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreExpr::Literal(v) => write!(f, "{}", v),
            CoreExpr::Var(s) => write!(f, "{}", s),
            CoreExpr::ScopedVar { name, scopes } => {
                // Display scoped var with debug info showing scopes
                write!(f, "{}@{}", name, scopes)
            }
            CoreExpr::Quote(v) => write!(f, "'{}", v),
            CoreExpr::Quasiquote(v) => write!(f, "`{}", v),
            CoreExpr::Lambda { params, .. } => {
                write!(f, "(lambda ")?;
                match params {
                    Formals::Fixed(ps) => {
                        write!(f, "(")?;
                        for (i, p) in ps.iter().enumerate() {
                            if i > 0 {
                                write!(f, " ")?;
                            }
                            write!(f, "{}", p)?;
                        }
                        write!(f, ")")?;
                    }
                    Formals::Variadic(p) => write!(f, "{}", p)?,
                    Formals::Mixed { fixed, rest } => {
                        write!(f, "(")?;
                        for (i, p) in fixed.iter().enumerate() {
                            if i > 0 {
                                write!(f, " ")?;
                            }
                            write!(f, "{}", p)?;
                        }
                        write!(f, " . {})", rest)?;
                    }
                }
                write!(f, " ...)")
            }
            CoreExpr::If { test, then, else_ } => {
                write!(f, "(if {} {} {})", test, then, else_)
            }
            CoreExpr::Set { var, value } => {
                write!(f, "(set! {} {})", var, value)
            }
            CoreExpr::Begin(exprs) => {
                write!(f, "(begin")?;
                for expr in exprs {
                    write!(f, " {}", expr)?;
                }
                write!(f, ")")
            }
            CoreExpr::Define { name, value } => {
                write!(f, "(define {} {})", name, value)
            }
            CoreExpr::DefineSyntax {
                name, transformer, ..
            } => {
                write!(f, "(define-syntax {} {})", name, transformer)
            }
            CoreExpr::Import { import_sets } => {
                write!(f, "(import")?;
                for import_set in import_sets {
                    write!(f, " {}", import_set)?;
                }
                write!(f, ")")
            }
            CoreExpr::Parameterize { bindings, body } => {
                write!(f, "(parameterize (")?;
                for (i, (param, val)) in bindings.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "({} {})", param, val)?;
                }
                write!(f, ")")?;
                for expr in body {
                    write!(f, " {}", expr)?;
                }
                write!(f, ")")
            }
            CoreExpr::Expand { expr } => {
                write!(f, "(expand {})", expr)
            }
            CoreExpr::CaseLambda { clauses } => {
                write!(f, "(case-lambda")?;
                for clause in clauses {
                    write!(f, " (")?;
                    match &clause.params {
                        Formals::Fixed(ps) => {
                            write!(f, "(")?;
                            for (i, p) in ps.iter().enumerate() {
                                if i > 0 {
                                    write!(f, " ")?;
                                }
                                write!(f, "{}", p)?;
                            }
                            write!(f, ")")?;
                        }
                        Formals::Variadic(p) => write!(f, "{}", p)?,
                        Formals::Mixed { fixed, rest } => {
                            write!(f, "(")?;
                            for (i, p) in fixed.iter().enumerate() {
                                if i > 0 {
                                    write!(f, " ")?;
                                }
                                write!(f, "{}", p)?;
                            }
                            write!(f, " . {})", rest)?;
                        }
                    }
                    for expr in &clause.body {
                        write!(f, " {}", expr)?;
                    }
                    write!(f, ")")?;
                }
                write!(f, ")")
            }
            CoreExpr::App { func, args } => {
                write!(f, "({}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, ")")
            }
            CoreExpr::Apply { func, args } => {
                write!(f, "(apply {}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, ")")
            }
            CoreExpr::PrimCall { prim, args } => {
                write!(f, "({:?}", prim)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, ")")
            }
            CoreExpr::Let { bindings, body } => {
                write!(f, "(let (")?;
                for (i, (var, val)) in bindings.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "({} {})", var, val)?;
                }
                write!(f, ") {})", body)
            }
        }
    }
}
