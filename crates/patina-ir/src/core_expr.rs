use patina_runtime::Value;
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
            CoreExpr::Quote(_) => "quote",
            CoreExpr::Quasiquote(_) => "quasiquote",
            CoreExpr::Lambda { .. } => "lambda",
            CoreExpr::If { .. } => "if",
            CoreExpr::Set { .. } => "set!",
            CoreExpr::Begin(_) => "begin",
            CoreExpr::Define { .. } => "define",
            CoreExpr::App { .. } => "application",
            CoreExpr::Apply { .. } => "apply",
            CoreExpr::PrimCall { .. } => "primitive-call",
            CoreExpr::Let { .. } => "let",
        }
    }
}

impl std::fmt::Display for CoreExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreExpr::Literal(v) => write!(f, "{}", v),
            CoreExpr::Var(s) => write!(f, "{}", s),
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
