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

use crate::error::SourceLocation;
use crate::scope::ScopeSet;
use crate::tagged_value::TaggedValue;
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

/// CPS Expression wrapper with optional source location
///
/// This struct wraps a `CpsExprKind` with an optional source location,
/// enabling error messages to include file/line/column information.
#[derive(Debug, Clone)]
pub struct CpsExpr {
    /// The expression kind
    pub kind: CpsExprKind,
    /// Source location (populated in Phase 2)
    pub source: Option<SourceLocation>,
}

impl CpsExpr {
    /// Create a new CpsExpr with no source location
    pub fn new(kind: CpsExprKind) -> Self {
        Self { kind, source: None }
    }

    /// Create a new CpsExpr with a source location
    pub fn with_source(kind: CpsExprKind, source: SourceLocation) -> Self {
        Self {
            kind,
            source: Some(source),
        }
    }

    /// Create a new CpsExpr with an optional source location
    pub fn with_opt_source(kind: CpsExprKind, source: Option<SourceLocation>) -> Self {
        Self { kind, source }
    }

    /// Create a new Rc<CpsExpr> with no source location
    pub fn rc(kind: CpsExprKind) -> Rc<Self> {
        Rc::new(Self::new(kind))
    }

    /// Check if this is a trivial expression (no control effects)
    pub fn is_trivial(&self) -> bool {
        self.kind.is_trivial()
    }

    /// Get a human-readable description of the expression type
    pub fn expr_kind(&self) -> &'static str {
        self.kind.expr_kind()
    }

    /// Visit every heap value embedded in this expression tree
    /// (`Literal` nodes and `Quasiquote` templates). GC tracing hook.
    ///
    /// `seen` deduplicates by node address: bodies are shared via `Rc` across
    /// closures, so the caller passes one set per collection and shared
    /// subtrees are walked once. Recursion depth is bounded by program size.
    pub fn for_each_literal(
        &self,
        seen: &mut rustc_hash::FxHashSet<usize>,
        f: &mut dyn FnMut(TaggedValue),
    ) {
        if !seen.insert(self as *const Self as usize) {
            return;
        }
        match &self.kind {
            CpsExprKind::Literal(tv) => f(*tv),
            CpsExprKind::Quasiquote { template, .. } => f(*template),
            CpsExprKind::Var { .. } | CpsExprKind::ContRef(_) => {}
            CpsExprKind::Lambda { body, .. } => body.for_each_literal(seen, f),
            CpsExprKind::LetVal { value, body, .. } => {
                value.for_each_literal(seen, f);
                body.for_each_literal(seen, f);
            }
            CpsExprKind::LetCont {
                cont_body, body, ..
            } => {
                cont_body.for_each_literal(seen, f);
                body.for_each_literal(seen, f);
            }
            CpsExprKind::App { func, args, .. } | CpsExprKind::Apply { func, args, .. } => {
                func.for_each_literal(seen, f);
                for arg in args {
                    arg.for_each_literal(seen, f);
                }
            }
            CpsExprKind::Continue { value, .. } => value.for_each_literal(seen, f),
            CpsExprKind::If {
                test,
                consequent,
                alternate,
            } => {
                test.for_each_literal(seen, f);
                consequent.for_each_literal(seen, f);
                alternate.for_each_literal(seen, f);
            }
            CpsExprKind::Set { value, cont, .. } | CpsExprKind::Define { value, cont, .. } => {
                value.for_each_literal(seen, f);
                cont.for_each_literal(seen, f);
            }
            CpsExprKind::CallCC { proc, .. } | CpsExprKind::Control { proc, .. } => {
                proc.for_each_literal(seen, f);
            }
            CpsExprKind::Prompt { body, .. } => body.for_each_literal(seen, f),
            CpsExprKind::Abort { value, .. } => value.for_each_literal(seen, f),
            CpsExprKind::PrimOp { args, .. } => {
                for arg in args {
                    arg.for_each_literal(seen, f);
                }
            }
            CpsExprKind::Halt(expr) => expr.for_each_literal(seen, f),
        }
    }
}

impl std::fmt::Display for CpsExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.kind, f)
    }
}

/// CPS Expression Kind - every sub-expression has explicit continuation
///
/// In CPS, there are two kinds of expressions:
/// - **Trivial expressions**: Evaluate immediately without control effects
///   (literals, variables, lambdas). These don't need continuations.
/// - **Serious expressions**: May have control effects (applications, if).
///   These take an explicit continuation.
#[derive(Debug, Clone)]
pub enum CpsExprKind {
    // ==================== Trivial Expressions ====================
    // These evaluate immediately and don't invoke continuations directly.
    // They're used as arguments to serious expressions.
    /// Literal value (self-evaluating)
    /// Example: 42, #t, "hello"
    Literal(TaggedValue),

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
        /// The scopes a parameter written in source stands in — see
        /// [`CoreExprKind::Lambda`]'s field of the same name.
        ///
        /// [`CoreExprKind::Lambda`]: crate::core_expr::CoreExprKind::Lambda
        binding_scopes: std::rc::Rc<crate::ScopeSet>,
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
    ///
    /// `scopes` mirrors `Set`'s, and for the same reason: the defined
    /// identifier's hygiene scopes are part of which binding this is, not
    /// decoration on it. Unlike the CoreExpr variant's, these are the scopes
    /// the binding *lives at*: the CPS transform stamps a source-written
    /// internal define with its body's `binding_scopes`, exactly as
    /// `application.rs` binds a source-written parameter. Empty only for a
    /// top-level define, which is a plain global.
    Define {
        name: Symbol,
        scopes: ScopeSet,
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
    /// The template is a TaggedValue that may contain unquote/unquote-splicing forms
    /// which need to be evaluated at runtime.
    Quasiquote {
        template: TaggedValue,
        cont: ContVar,
    },

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

impl CpsPrimitive {
    /// Get the pre-computed qualified name for registry lookup.
    ///
    /// All CPS primitives are from "scheme.base", so the qualified name
    /// is always "scheme.base/<name>". Returns `&'static str` — zero allocation.
    ///
    /// Returns `None` for primitives that are handled specially (IsContinuation,
    /// IsPromptTag) and don't go through the registry.
    pub fn qualified_name(&self) -> Option<&'static str> {
        match self {
            CpsPrimitive::Add => Some("scheme.base/+"),
            CpsPrimitive::Sub => Some("scheme.base/-"),
            CpsPrimitive::Mul => Some("scheme.base/*"),
            CpsPrimitive::Div => Some("scheme.base//"),
            CpsPrimitive::Quotient => Some("scheme.base/quotient"),
            CpsPrimitive::Remainder => Some("scheme.base/remainder"),
            CpsPrimitive::Modulo => Some("scheme.base/modulo"),
            CpsPrimitive::NumEq => Some("scheme.base/="),
            CpsPrimitive::Lt => Some("scheme.base/<"),
            CpsPrimitive::Gt => Some("scheme.base/>"),
            CpsPrimitive::Lte => Some("scheme.base/<="),
            CpsPrimitive::Gte => Some("scheme.base/>="),
            CpsPrimitive::Cons => Some("scheme.base/cons"),
            CpsPrimitive::Car => Some("scheme.base/car"),
            CpsPrimitive::Cdr => Some("scheme.base/cdr"),
            CpsPrimitive::List => Some("scheme.base/list"),
            CpsPrimitive::IsNull => Some("scheme.base/null?"),
            CpsPrimitive::IsPair => Some("scheme.base/pair?"),
            CpsPrimitive::IsNumber => Some("scheme.base/number?"),
            CpsPrimitive::IsBoolean => Some("scheme.base/boolean?"),
            CpsPrimitive::IsString => Some("scheme.base/string?"),
            CpsPrimitive::IsSymbol => Some("scheme.base/symbol?"),
            CpsPrimitive::IsProcedure => Some("scheme.base/procedure?"),
            CpsPrimitive::Eq => Some("scheme.base/eq?"),
            CpsPrimitive::Eqv => Some("scheme.base/eqv?"),
            CpsPrimitive::Equal => Some("scheme.base/equal?"),
            CpsPrimitive::MakeVector => Some("scheme.base/make-vector"),
            CpsPrimitive::VectorRef => Some("scheme.base/vector-ref"),
            CpsPrimitive::VectorSet => Some("scheme.base/vector-set!"),
            CpsPrimitive::VectorLength => Some("scheme.base/vector-length"),
            CpsPrimitive::MakeString => Some("scheme.base/make-string"),
            CpsPrimitive::StringRef => Some("scheme.base/string-ref"),
            CpsPrimitive::StringLength => Some("scheme.base/string-length"),
            CpsPrimitive::Display => Some("scheme.base/display"),
            CpsPrimitive::Newline => Some("scheme.base/newline"),
            // Special-cased primitives handled before registry lookup
            CpsPrimitive::IsContinuation | CpsPrimitive::IsPromptTag => None,
        }
    }
}

impl CpsExprKind {
    /// Check if this is a trivial expression (no control effects)
    pub fn is_trivial(&self) -> bool {
        matches!(
            self,
            CpsExprKind::Literal(_)
                | CpsExprKind::Var { .. }
                | CpsExprKind::ContRef(_)
                | CpsExprKind::Lambda { .. }
        )
    }

    /// Get a human-readable description of the expression type
    pub fn expr_kind(&self) -> &'static str {
        match self {
            CpsExprKind::Literal(_) => "literal",
            CpsExprKind::Var { .. } => "var",
            CpsExprKind::ContRef(_) => "cont-ref",
            CpsExprKind::Lambda { .. } => "lambda",
            CpsExprKind::LetVal { .. } => "let-val",
            CpsExprKind::LetCont { .. } => "let-cont",
            CpsExprKind::App { .. } => "app",
            CpsExprKind::Apply { .. } => "apply",
            CpsExprKind::Continue { .. } => "continue",
            CpsExprKind::If { .. } => "if",
            CpsExprKind::Set { .. } => "set!",
            CpsExprKind::Define { .. } => "define",
            CpsExprKind::CallCC { .. } => "call/cc",
            CpsExprKind::Prompt { .. } => "prompt",
            CpsExprKind::Control { .. } => "control",
            CpsExprKind::Abort { .. } => "abort",
            CpsExprKind::Quasiquote { .. } => "quasiquote",
            CpsExprKind::PrimOp { .. } => "prim-op",
            CpsExprKind::Halt(_) => "halt",
        }
    }
}

impl std::fmt::Display for CpsExprKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CpsExprKind::Literal(v) => write!(f, "{}", v),
            CpsExprKind::Var { name, scopes } => {
                if scopes.is_empty() {
                    write!(f, "{}", name)
                } else {
                    write!(f, "{}@{}", name, scopes)
                }
            }
            CpsExprKind::ContRef(k) => write!(f, "#{}", k),
            CpsExprKind::Lambda {
                params,
                variadic,
                cont_param,
                body,
                binding_scopes: _,
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
            CpsExprKind::LetVal { name, value, body } => {
                write!(f, "(let-val ({} {}) {})", name, value, body)
            }
            CpsExprKind::LetCont {
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
            CpsExprKind::App { func, args, cont } => {
                write!(f, "({}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, " #{})", cont)
            }
            CpsExprKind::Apply { func, args, cont } => {
                write!(f, "(apply {}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, " #{})", cont)
            }
            CpsExprKind::Continue { cont, value } => {
                write!(f, "(#{} {})", cont, value)
            }
            CpsExprKind::If {
                test,
                consequent,
                alternate,
            } => {
                write!(f, "(if {} {} {})", test, consequent, alternate)
            }
            CpsExprKind::Set {
                var, value, cont, ..
            } => {
                write!(f, "(set! {} {} {})", var, value, cont)
            }
            CpsExprKind::Define {
                name, value, cont, ..
            } => {
                write!(f, "(define {} {} {})", name, value, cont)
            }
            CpsExprKind::CallCC { proc, cont } => {
                write!(f, "(call/cc {} #{})", proc, cont)
            }
            CpsExprKind::Prompt { tag, body, cont } => {
                write!(f, "(prompt {} {} #{})", tag, body, cont)
            }
            CpsExprKind::Control { tag, proc } => {
                write!(f, "(control {} {})", tag, proc)
            }
            CpsExprKind::Abort { tag, value } => {
                write!(f, "(abort {} {})", tag, value)
            }
            CpsExprKind::Quasiquote { template, cont } => {
                write!(f, "(quasiquote {} #{})", template, cont)
            }
            CpsExprKind::PrimOp { op, args, cont } => {
                write!(f, "({:?}", op)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, " #{})", cont)
            }
            CpsExprKind::Halt(v) => write!(f, "(halt {})", v),
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
        let lit = CpsExpr::new(CpsExprKind::Literal(TaggedValue::fixnum(42)));
        assert!(lit.is_trivial());

        let var = CpsExpr::new(CpsExprKind::Var {
            name: "x".into(),
            scopes: ScopeSet::new(),
        });
        assert!(var.is_trivial());

        let cont_ref = CpsExpr::new(CpsExprKind::ContRef("k".into()));
        assert!(cont_ref.is_trivial());

        let app = CpsExpr::new(CpsExprKind::App {
            func: CpsExpr::rc(CpsExprKind::Var {
                name: "f".into(),
                scopes: ScopeSet::new(),
            }),
            args: vec![],
            cont: "k".into(),
        });
        assert!(!app.is_trivial());
    }
}
