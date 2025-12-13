//! CPS (Continuation-Passing Style) Intermediate Representation
//!
//! This module defines `CpsExpr`, a continuation-passing style IR that makes
//! control flow explicit. Every sub-expression has an explicit continuation
//! representing "what to do next with the result".
//!
//! # Architecture
//!
//! ```text
//! CoreExpr → [CPS Transform] → CpsExpr → [CPS Evaluator] → Value
//! ```
//!
//! # Why CPS?
//!
//! CPS transformation is essential for implementing:
//! - `call/cc` (call-with-current-continuation)
//! - Delimited continuations (`shift`/`reset`)
//! - `dynamic-wind` (proper entry/exit handler invocation)
//! - Exception handling (`guard`, `raise`)
//!
//! In CPS, the continuation is always explicit, making it trivial to:
//! - Capture the current continuation (it's just a parameter)
//! - Invoke a different continuation (just call it)
//! - Implement delimited capture (capture up to a prompt marker)
//!
//! # Design Notes
//!
//! ## Continuation Representation
//!
//! Continuations are represented as `ContVar` (a variable name bound to a
//! continuation value). When evaluated, a `ContVar` resolves to a `Value::Continuation`.
//!
//! ## Tail Calls
//!
//! CPS naturally preserves tail call optimization. A tail call in direct style
//! becomes passing the current continuation to the callee in CPS.
//!
//! ## Administrative vs User Continuations
//!
//! - **Administrative continuations**: Introduced by CPS transformation for
//!   sequencing (e.g., evaluating arguments left-to-right). These are internal.
//! - **User continuations**: Captured by `call/cc` or `shift`. These are
//!   first-class values that can be stored and invoked.

use crate::scope::ScopeSet;
use crate::value::Value;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Symbol type (interned string)
pub type Symbol = Rc<str>;

/// Continuation variable - names a continuation in scope
pub type ContVar = Symbol;

/// Unique identifier for prompt tags
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromptTag {
    /// Human-readable name for debugging
    pub name: Symbol,
    /// Unique identifier (globally unique across all tags)
    pub id: u64,
}

impl PromptTag {
    /// Create a new prompt tag with a unique ID
    pub fn new(name: impl Into<Symbol>) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self {
            name: name.into(),
            id: COUNTER.fetch_add(1, Ordering::SeqCst),
        }
    }

    /// Create a prompt tag with a specific ID (for testing)
    pub fn with_id(name: impl Into<Symbol>, id: u64) -> Self {
        Self {
            name: name.into(),
            id,
        }
    }
}

impl std::fmt::Display for PromptTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#<prompt-tag:{}/{}>", self.name, self.id)
    }
}

/// CPS Expression - every sub-expression has explicit continuation
///
/// In CPS, there are two kinds of expressions:
/// - **Trivial expressions**: Evaluate immediately without control effects
///   (literals, variables, lambdas). These don't need continuations.
/// - **Serious expressions**: May have control effects (applications, if).
///   These take an explicit continuation.
#[derive(Debug, Clone)]
pub enum CpsExpr {
    // ==================== Trivial Expressions ====================
    // These evaluate immediately and don't invoke continuations directly.
    // They're used as arguments to serious expressions.
    /// Literal value (self-evaluating)
    /// Example: 42, #t, "hello"
    Literal(Rc<Value>),

    /// Variable reference
    /// Example: x, my-function
    Var { name: Symbol, scopes: ScopeSet },

    /// Continuation variable reference
    /// Example: k, return
    /// Resolves to a continuation value in the environment
    ContRef(ContVar),

    /// Lambda abstraction (CPS lambda takes continuation parameter)
    /// Example: (lambda (x y) body) becomes (lambda (x y k) body')
    ///
    /// In CPS, every user lambda takes an extra continuation parameter.
    /// When called, the lambda will eventually invoke this continuation
    /// with its result.
    Lambda {
        /// Regular parameters
        params: Vec<CpsParam>,
        /// Variadic parameter (if any)
        variadic: Option<CpsParam>,
        /// Continuation parameter (always present in CPS)
        cont_param: ContVar,
        /// Body in CPS form
        body: Rc<CpsExpr>,
        /// Binding scope for parameters without scopes (for hygiene)
        /// When present, parameters without explicit scopes will also be bound
        /// with this scope, allowing macro-expanded references to find them.
        binding_scope: Option<crate::ScopeId>,
    },

    // ==================== Serious Expressions ====================
    // These may have control effects and work with continuations.
    /// Let-bind a trivial expression and continue
    /// (let ((x trivial)) body)
    ///
    /// This is the CPS equivalent of sequencing. We bind the result of
    /// a trivial expression and continue with the body.
    LetVal {
        name: Symbol,
        value: Rc<CpsExpr>, // Must be trivial
        body: Rc<CpsExpr>,
    },

    /// Let-bind a continuation and continue
    /// (let-cont ((k (x) body)) expr)
    ///
    /// This defines a local continuation `k` that takes parameter `x`
    /// and executes `body`. The continuation is then available in `expr`.
    LetCont {
        /// Continuation name
        name: ContVar,
        /// Parameter the continuation receives
        param: Symbol,
        /// Continuation body
        cont_body: Rc<CpsExpr>,
        /// Expression where the continuation is in scope
        body: Rc<CpsExpr>,
    },

    /// Application (call a function with continuation)
    /// (f arg1 arg2 ... k)
    ///
    /// In CPS, all calls are tail calls. The continuation `k` represents
    /// "what to do with the result".
    App {
        func: Rc<CpsExpr>,  // Must be trivial (evaluates to procedure)
        args: Vec<CpsExpr>, // Must be trivial
        cont: ContVar,      // Continuation to receive result
    },

    /// Apply - like App but last arg is a list to flatten
    /// (apply f arg1 arg2 ... list k)
    ///
    /// The last argument must be a list whose elements are appended to the
    /// preceding arguments. This is the CPS form of Scheme's `apply`.
    Apply {
        func: Rc<CpsExpr>,  // Must be trivial (evaluates to procedure)
        args: Vec<CpsExpr>, // Must be trivial, last is a list to flatten
        cont: ContVar,      // Continuation to receive result
    },

    /// Continuation invocation (return a value via continuation)
    /// (k value)
    ///
    /// This "returns" by invoking the continuation with the value.
    /// This is how values flow back to callers in CPS.
    Continue {
        cont: ContVar,
        value: Rc<CpsExpr>, // Must be trivial
    },

    /// Conditional
    /// (if test (k1) (k2))
    ///
    /// In CPS, both branches are continuation invocations or tail expressions.
    If {
        test: Rc<CpsExpr>,       // Must be trivial
        consequent: Rc<CpsExpr>, // Serious expression
        alternate: Rc<CpsExpr>,  // Serious expression
    },

    /// Set! (mutation)
    /// (set! var value then-continue)
    Set {
        var: Symbol,
        scopes: ScopeSet,
        value: Rc<CpsExpr>, // Must be trivial
        cont: Rc<CpsExpr>,  // What to do after mutation
    },

    /// Define (top-level binding)
    Define {
        name: Symbol,
        value: Rc<CpsExpr>, // Must be trivial
        cont: Rc<CpsExpr>,  // What to do after definition
    },

    // ==================== Control Operators ====================
    // These are the primitives for implementing call/cc and shift/reset.
    /// Capture current continuation (for call/cc)
    /// (call/cc (lambda (k) body) current-k)
    ///
    /// Reifies the current continuation as a first-class value and passes
    /// it to the given procedure.
    CallCC {
        /// Procedure that receives the continuation
        proc: Rc<CpsExpr>,
        /// Current continuation (to be captured)
        cont: ContVar,
    },

    /// Establish a prompt (for reset)
    /// (prompt tag body k)
    ///
    /// Establishes a delimiter for delimited continuation capture.
    /// When `body` completes normally, its result goes to `k`.
    Prompt {
        tag: PromptTag,
        body: Rc<CpsExpr>,
        cont: ContVar,
    },

    /// Capture delimited continuation (for shift)
    /// (control tag (lambda (k) body))
    ///
    /// Captures the continuation up to the nearest prompt with matching tag.
    /// The captured continuation is passed to the procedure.
    Control { tag: PromptTag, proc: Rc<CpsExpr> },

    /// Abort to prompt
    /// (abort tag value)
    ///
    /// Discards the current continuation up to the prompt and returns
    /// the value to the prompt's handler.
    Abort { tag: PromptTag, value: Rc<CpsExpr> },

    // ==================== Template ====================
    /// Quasiquote template evaluation
    /// (quasiquote template k)
    ///
    /// Evaluates a quasiquote template, processing unquote and unquote-splicing.
    /// The template is a Value that may contain unquote/unquote-splicing forms
    /// which need to be evaluated at runtime.
    Quasiquote { template: Rc<Value>, cont: ContVar },

    // ==================== Primitives ====================
    /// Primitive operation (known at compile time)
    /// These don't need CPS transformation since they don't capture continuations.
    PrimOp {
        op: CpsPrimitive,
        args: Vec<CpsExpr>,
        cont: ContVar,
    },

    // Note: Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)
    // The CpsExpr::Parameterize variant has been removed.
    /// Halt - top-level continuation (program termination)
    /// This is the "end of the program" continuation.
    Halt(Rc<CpsExpr>),
}

/// A parameter in CPS lambda
#[derive(Debug, Clone)]
pub struct CpsParam {
    pub name: Symbol,
    pub scopes: ScopeSet,
}

impl CpsParam {
    pub fn simple(name: impl Into<Symbol>) -> Self {
        Self {
            name: name.into(),
            scopes: ScopeSet::new(),
        }
    }

    pub fn with_scopes(name: impl Into<Symbol>, scopes: ScopeSet) -> Self {
        Self {
            name: name.into(),
            scopes,
        }
    }
}

/// Primitive operations in CPS
///
/// These are operations that are known to not capture continuations,
/// so they can be compiled more efficiently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpsPrimitive {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Quotient,
    Remainder,
    Modulo,

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
    IsProcedure,
    IsContinuation,

    // Equality
    Eq,
    Eqv,
    Equal,

    // Vector operations
    MakeVector,
    VectorRef,
    VectorSet,
    VectorLength,

    // String operations
    MakeString,
    StringRef,
    StringLength,

    // I/O (simple, non-continuation-capturing)
    Display,
    Newline,

    // Continuation predicates
    IsPromptTag,
}

impl CpsExpr {
    /// Check if this is a trivial expression (no control effects)
    pub fn is_trivial(&self) -> bool {
        matches!(
            self,
            CpsExpr::Literal(_)
                | CpsExpr::Var { .. }
                | CpsExpr::ContRef(_)
                | CpsExpr::Lambda { .. }
        )
    }

    /// Get a human-readable description of the expression type
    pub fn kind(&self) -> &'static str {
        match self {
            CpsExpr::Literal(_) => "literal",
            CpsExpr::Var { .. } => "var",
            CpsExpr::ContRef(_) => "cont-ref",
            CpsExpr::Lambda { .. } => "lambda",
            CpsExpr::LetVal { .. } => "let-val",
            CpsExpr::LetCont { .. } => "let-cont",
            CpsExpr::App { .. } => "app",
            CpsExpr::Apply { .. } => "apply",
            CpsExpr::Continue { .. } => "continue",
            CpsExpr::If { .. } => "if",
            CpsExpr::Set { .. } => "set!",
            CpsExpr::Define { .. } => "define",
            CpsExpr::CallCC { .. } => "call/cc",
            CpsExpr::Prompt { .. } => "prompt",
            CpsExpr::Control { .. } => "control",
            CpsExpr::Abort { .. } => "abort",
            CpsExpr::Quasiquote { .. } => "quasiquote",
            CpsExpr::PrimOp { .. } => "prim-op",
            CpsExpr::Halt(_) => "halt",
        }
    }
}

impl std::fmt::Display for CpsExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CpsExpr::Literal(v) => write!(f, "{}", v),
            CpsExpr::Var { name, scopes } => {
                if scopes.is_empty() {
                    write!(f, "{}", name)
                } else {
                    write!(f, "{}@{}", name, scopes)
                }
            }
            CpsExpr::ContRef(k) => write!(f, "#{}", k),
            CpsExpr::Lambda {
                params,
                variadic,
                cont_param,
                body,
                binding_scope: _,
            } => {
                write!(f, "(λ (")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", p.name)?;
                }
                if let Some(rest) = variadic {
                    if !params.is_empty() {
                        write!(f, " ")?;
                    }
                    write!(f, ". {}", rest.name)?;
                }
                write!(f, ") #{} {})", cont_param, body)
            }
            CpsExpr::LetVal { name, value, body } => {
                write!(f, "(let-val ({} {}) {})", name, value, body)
            }
            CpsExpr::LetCont {
                name,
                param,
                cont_body,
                body,
            } => {
                write!(
                    f,
                    "(let-cont (({} {}) {}) {})",
                    name, param, cont_body, body
                )
            }
            CpsExpr::App { func, args, cont } => {
                write!(f, "({}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, " #{})", cont)
            }
            CpsExpr::Apply { func, args, cont } => {
                write!(f, "(apply {}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, " #{})", cont)
            }
            CpsExpr::Continue { cont, value } => {
                write!(f, "(#{} {})", cont, value)
            }
            CpsExpr::If {
                test,
                consequent,
                alternate,
            } => {
                write!(f, "(if {} {} {})", test, consequent, alternate)
            }
            CpsExpr::Set {
                var, value, cont, ..
            } => {
                write!(f, "(set! {} {} {})", var, value, cont)
            }
            CpsExpr::Define { name, value, cont } => {
                write!(f, "(define {} {} {})", name, value, cont)
            }
            CpsExpr::CallCC { proc, cont } => {
                write!(f, "(call/cc {} #{})", proc, cont)
            }
            CpsExpr::Prompt { tag, body, cont } => {
                write!(f, "(prompt {} {} #{})", tag, body, cont)
            }
            CpsExpr::Control { tag, proc } => {
                write!(f, "(control {} {})", tag, proc)
            }
            CpsExpr::Abort { tag, value } => {
                write!(f, "(abort {} {})", tag, value)
            }
            CpsExpr::Quasiquote { template, cont } => {
                write!(f, "(quasiquote {} #{})", template, cont)
            }
            CpsExpr::PrimOp { op, args, cont } => {
                write!(f, "({:?}", op)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, " #{})", cont)
            }
            CpsExpr::Halt(v) => write!(f, "(halt {})", v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_tag_uniqueness() {
        let tag1 = PromptTag::new("test");
        let tag2 = PromptTag::new("test");

        // Same name but different IDs
        assert_eq!(tag1.name, tag2.name);
        assert_ne!(tag1.id, tag2.id);
        assert_ne!(tag1, tag2);
    }

    #[test]
    fn test_cps_expr_is_trivial() {
        let lit = CpsExpr::Literal(Rc::new(Value::Integer(42)));
        assert!(lit.is_trivial());

        let var = CpsExpr::Var {
            name: "x".into(),
            scopes: ScopeSet::new(),
        };
        assert!(var.is_trivial());

        let cont_ref = CpsExpr::ContRef("k".into());
        assert!(cont_ref.is_trivial());

        let app = CpsExpr::App {
            func: Rc::new(CpsExpr::Var {
                name: "f".into(),
                scopes: ScopeSet::new(),
            }),
            args: vec![],
            cont: "k".into(),
        };
        assert!(!app.is_trivial());
    }
}
