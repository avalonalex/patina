use crate::error::SourceLocation;
use crate::scope::ScopeSet;
use crate::tagged_value::TaggedValue;
use std::rc::Rc;

/// Symbol type (interned string)
pub type Symbol = Rc<str>;

/// Lambda body representation - type-safe replacement for `dyn Any`
///
/// Lambda body stored as CoreExpr (preserves scope IDs for hygiene).
/// All lambdas are created with CoreExpr bodies.
pub type LambdaBody = Vec<CoreExpr>;

/// A parameter with optional scope set for hygiene
///
/// When a macro introduces a binding, the parameter carries scopes from the
/// macro expansion context. These scopes are used during binding to ensure
/// that references with matching scopes can find the binding, while references
/// with different scopes (from different macro expansions) cannot.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopedParam {
    /// The parameter name
    pub name: Symbol,
    /// Scopes from macro expansion (empty for non-macro-introduced params)
    pub scopes: ScopeSet,
}

impl ScopedParam {
    /// Create a parameter with no scopes (for non-macro code)
    pub fn simple(name: Symbol) -> Self {
        Self {
            name,
            scopes: ScopeSet::new(),
        }
    }

    /// Create a parameter with scopes (for macro-introduced bindings)
    pub fn with_scopes(name: Symbol, scopes: ScopeSet) -> Self {
        Self { name, scopes }
    }
}

impl std::fmt::Display for ScopedParam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.scopes.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}{{{}}}", self.name, self.scopes)
        }
    }
}

/// Lambda parameter forms (fixed, variadic, or mixed)
#[derive(Debug, Clone, PartialEq)]
pub enum Formals {
    /// Fixed arity: (lambda (x y z) ...)
    Fixed(Vec<ScopedParam>),

    /// Variadic: (lambda args ...)
    Variadic(ScopedParam),

    /// Mixed: (lambda (x y . rest) ...)
    Mixed {
        fixed: Vec<ScopedParam>,
        rest: ScopedParam,
    },
}

/// Core expression with optional source location
///
/// This wrapper struct pairs a `CoreExprKind` (the expression variant) with
/// an optional `SourceLocation` for error reporting. All source fields are
/// currently `None` — population happens in Phase 2.
#[derive(Debug, Clone)]
pub struct CoreExpr {
    /// The expression variant
    pub kind: CoreExprKind,
    /// Source location (populated in Phase 2)
    pub source: Option<SourceLocation>,
}

impl CoreExpr {
    /// Create a new CoreExpr with no source location
    pub fn new(kind: CoreExprKind) -> Self {
        Self { kind, source: None }
    }

    /// Create a new CoreExpr with a source location
    pub fn with_source(kind: CoreExprKind, source: SourceLocation) -> Self {
        Self {
            kind,
            source: Some(source),
        }
    }

    /// Create a new CoreExpr with an optional source location
    pub fn with_opt_source(kind: CoreExprKind, source: Option<SourceLocation>) -> Self {
        Self { kind, source }
    }

    /// Create an Rc<CoreExpr> with no source location
    pub fn rc(kind: CoreExprKind) -> Rc<Self> {
        Rc::new(Self::new(kind))
    }

    /// Check if this expression is in tail position
    pub fn is_tail_position(&self) -> bool {
        self.kind.is_tail_position()
    }

    /// Get a human-readable description of the expression type
    pub fn expr_kind(&self) -> &'static str {
        self.kind.expr_kind()
    }

    /// Map a function over all immediate children of this expression
    ///
    /// This is useful for implementing recursive transformations in compiler passes.
    /// Preserves the source location of this node on the rebuilt expression.
    pub fn map_children<F>(&self, f: F) -> CoreExpr
    where
        F: Fn(&CoreExpr) -> CoreExpr,
    {
        CoreExpr {
            kind: self.kind.map_children_inner(&f),
            source: self.source.clone(),
        }
    }
}

impl std::fmt::Display for CoreExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.kind, f)
    }
}

/// Core Scheme expressions after macro expansion and desugaring
///
/// This is the minimal IR that backends must handle.
/// All macros and derived forms are eliminated by the frontend.
#[derive(Debug, Clone)]
pub enum CoreExprKind {
    /// Literal values: numbers, booleans, strings, etc.
    /// Example: 42, #t, "hello"
    /// Uses TaggedValue (8 bytes) for compact representation
    Literal(TaggedValue),

    /// Variable reference (with optional hygiene scopes)
    /// Example: x, my-function
    /// For hygienic macros, scopes carries scope information from macro expansion.
    /// The evaluator uses scope-based lookup when scopes is non-empty:
    /// finds binding where binding.scopes ⊆ reference.scopes.
    /// Empty scopes means simple name lookup (non-macro code).
    Var { name: Symbol, scopes: ScopeSet },

    /// Quote: literal data
    /// Example: 'x, '(1 2 3)
    /// Uses TaggedValue (8 bytes) for compact representation
    Quote(TaggedValue),

    /// Quasiquote: template with selective evaluation
    /// Example: `(a ,b ,@c) where b and c are evaluated
    ///
    /// The structure the template builds, with its unquoted expressions
    /// already desugared — see [`QuasiTemplate`]. No backend evaluates this:
    /// `patina_frontend::lower_quasiquotes` turns it into calls before either
    /// one lowers the tree.
    Quasiquote(QuasiTemplate),

    /// Lambda abstraction
    /// Example: (lambda (x y) (+ x y))
    Lambda {
        params: Formals,
        body: Vec<CoreExpr>,
        /// The scopes a parameter written in source stands in: every scope
        /// enclosing this lambda, plus one minted for it. Empty when there is
        /// no scope information (a hand-built tree).
        ///
        /// Accumulated, not a single fresh scope. A binder scoped `{fresh}`
        /// alone is unordered against its own enclosing binders, so a
        /// macro-introduced reference that reaches two of them has no most
        /// specific candidate and set-of-scopes resolution cannot decide —
        /// Larceny triage family 39. Accumulating makes the nesting a chain:
        /// an inner binder's scopes strictly contain an outer's, which is
        /// what lets the rule answer on its own.
        ///
        /// A parameter a macro introduced keeps its own scopes instead
        /// (`ScopedParam::scopes`); those already say where it was written.
        binding_scopes: Rc<ScopeSet>,
    },

    /// Conditional (always ternary after desugaring)
    /// Example: (if test then else)
    /// Uses Rc<CoreExpr> for efficient sharing in tail call optimization
    If {
        test: Rc<CoreExpr>,
        then: Rc<CoreExpr>,
        else_: Rc<CoreExpr>,
    },

    /// Assignment (with optional hygiene scopes)
    /// Example: (set! x 42)
    /// For hygienic macros, scopes carries scope information from macro expansion.
    /// The evaluator uses scope-based lookup when scopes is non-empty to find
    /// and update the correct binding.
    /// Empty scopes means simple name lookup (non-macro code).
    Set {
        var: Symbol,
        scopes: ScopeSet,
        value: Rc<CoreExpr>,
    },

    /// Sequencing
    /// Example: (begin expr1 expr2 expr3)
    Begin(Vec<CoreExpr>),

    /// Top-level definition
    /// Example: (define x 42), (define (f x) x)
    ///
    /// `scopes` is the hygiene scope set of the *defined* identifier, and is
    /// empty for a name written in source. A macro template that introduces a
    /// binding gets a fresh scope per expansion, so a recursive macro that
    /// defines one temporary per element must produce as many distinct
    /// bindings as it has elements — which it cannot do if the name alone
    /// identifies the binding. `Var` and `ScopedParam` have carried their
    /// scopes all along; this variant did not, so every such temporary
    /// collapsed onto the last one.
    Define {
        name: Symbol,
        scopes: ScopeSet,
        value: Rc<CoreExpr>,
    },

    /// Import: load library bindings
    /// Example: (import (scheme base))
    /// Import sets are kept as TaggedValues (declarative data, not code)
    Import { import_sets: Vec<TaggedValue> },

    // Note: Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)
    // The CoreExpr::Parameterize variant has been removed.
    /// Expand: show macro expansion without evaluating
    /// Example: (expand '(let ((x 1)) x)) => ((lambda (x) x) 1)
    /// This is a Patina debugging extension, not part of R7RS
    Expand { expr: Rc<CoreExpr> },

    /// Function application
    /// Example: (f x y), (+ 1 2)
    App {
        func: Rc<CoreExpr>,
        args: Vec<CoreExpr>,
    },

    /// Apply: apply procedure to list
    /// Example: (apply + '(1 2 3)), (apply f x y zs)
    /// Last argument is a list that gets spliced as arguments
    Apply {
        func: Rc<CoreExpr>,
        args: Vec<CoreExpr>, // All args including the final list
    },
}

/// A quasiquote template as the desugarer leaves it: the structure it
/// builds, with every unquoted expression already desugared.
///
/// Two halves of one job, split between two places because each needs
/// something the other cannot have. An unquoted expression is ordinary code
/// and has to be desugared by the desugarer standing in the form, since only
/// that one knows the form's local keywords, which of its names are local
/// variables, and what #438's early binding recorded (#445). The list
/// constructors are procedure *values* from a backend's primitive registry,
/// which the frontend cannot name. So the desugarer derives this, and
/// `patina_frontend::lower_quasiquotes` turns each [`QuasiTemplate::Build`]
/// into a call once a backend supplies the constructors.
#[derive(Debug, Clone)]
pub enum QuasiTemplate {
    /// Part of the template that is data, used as it was written. It becomes
    /// a `Quote`.
    Datum(TaggedValue),
    /// An unquoted expression, desugared where it was written.
    Unquoted(Rc<CoreExpr>),
    /// A call of one of the constructors, on the values of the parts.
    Build(QuasiConstructor, Vec<QuasiTemplate>),
}

/// The procedures a quasiquote template is built with. `cons` is not among
/// them: a dotted tail goes through `append`, whose last argument may be
/// anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuasiConstructor {
    List,
    Append,
    ListToVector,
}

impl QuasiConstructor {
    /// The `(scheme base)` name of the procedure.
    pub fn name(self) -> &'static str {
        match self {
            QuasiConstructor::List => "list",
            QuasiConstructor::Append => "append",
            QuasiConstructor::ListToVector => "list->vector",
        }
    }
}

impl QuasiTemplate {
    /// This template with `f` applied to each unquoted expression.
    pub fn map_unquoted<F>(&self, f: &F) -> QuasiTemplate
    where
        F: Fn(&CoreExpr) -> CoreExpr,
    {
        match self {
            QuasiTemplate::Datum(datum) => QuasiTemplate::Datum(*datum),
            QuasiTemplate::Unquoted(expr) => QuasiTemplate::Unquoted(Rc::new(f(expr))),
            QuasiTemplate::Build(constructor, parts) => QuasiTemplate::Build(
                *constructor,
                parts.iter().map(|part| part.map_unquoted(f)).collect(),
            ),
        }
    }

    /// Call `f` on each unquoted expression, in the template's order.
    pub fn for_each_unquoted(&self, f: &mut dyn FnMut(&CoreExpr)) {
        match self {
            QuasiTemplate::Datum(_) => {}
            QuasiTemplate::Unquoted(expr) => f(expr),
            QuasiTemplate::Build(_, parts) => {
                for part in parts {
                    part.for_each_unquoted(f);
                }
            }
        }
    }
}

/// Shows what the template builds, with the constructors by name:
/// `` `(a ,b) `` displays as `(list 'a b)`.
impl std::fmt::Display for QuasiTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuasiTemplate::Datum(datum) => write!(f, "'{}", datum),
            QuasiTemplate::Unquoted(expr) => write!(f, "{}", expr),
            QuasiTemplate::Build(constructor, parts) => {
                write!(f, "({}", constructor.name())?;
                for part in parts {
                    write!(f, " {}", part)?;
                }
                write!(f, ")")
            }
        }
    }
}

impl CoreExprKind {
    /// Check if this expression is in tail position
    pub fn is_tail_position(&self) -> bool {
        matches!(
            self,
            CoreExprKind::If { .. } | CoreExprKind::Begin(_) | CoreExprKind::App { .. }
        )
    }

    /// Get a human-readable description of the expression type
    pub fn expr_kind(&self) -> &'static str {
        match self {
            CoreExprKind::Literal(_) => "literal",
            CoreExprKind::Var { .. } => "variable",
            CoreExprKind::Quote(_) => "quote",
            CoreExprKind::Quasiquote(_) => "quasiquote",
            CoreExprKind::Lambda { .. } => "lambda",
            CoreExprKind::If { .. } => "if",
            CoreExprKind::Set { .. } => "set!",
            CoreExprKind::Begin(_) => "begin",
            CoreExprKind::Define { .. } => "define",
            CoreExprKind::Import { .. } => "import",
            CoreExprKind::Expand { .. } => "expand",
            CoreExprKind::App { .. } => "application",
            CoreExprKind::Apply { .. } => "apply",
        }
    }

    /// Map a function over all immediate children (internal helper)
    fn map_children_inner<F>(&self, f: &F) -> CoreExprKind
    where
        F: Fn(&CoreExpr) -> CoreExpr,
    {
        match self {
            CoreExprKind::Literal(_) | CoreExprKind::Var { .. } | CoreExprKind::Quote(_) => {
                self.clone()
            }

            CoreExprKind::Quasiquote(template) => {
                CoreExprKind::Quasiquote(template.map_unquoted(f))
            }

            CoreExprKind::Lambda {
                params,
                body,
                binding_scopes,
            } => CoreExprKind::Lambda {
                params: params.clone(),
                body: body.iter().map(f).collect(),
                binding_scopes: binding_scopes.clone(),
            },

            CoreExprKind::If { test, then, else_ } => CoreExprKind::If {
                test: Rc::new(f(test)),
                then: Rc::new(f(then)),
                else_: Rc::new(f(else_)),
            },

            CoreExprKind::Set { var, scopes, value } => CoreExprKind::Set {
                var: var.clone(),
                scopes: scopes.clone(),
                value: Rc::new(f(value)),
            },

            CoreExprKind::Begin(exprs) => CoreExprKind::Begin(exprs.iter().map(f).collect()),

            CoreExprKind::Define {
                name,
                scopes,
                value,
            } => CoreExprKind::Define {
                name: name.clone(),
                scopes: scopes.clone(),
                value: Rc::new(f(value)),
            },

            CoreExprKind::Import { import_sets } => CoreExprKind::Import {
                import_sets: import_sets.clone(),
            },

            CoreExprKind::Expand { expr } => CoreExprKind::Expand {
                expr: Rc::new(f(expr)),
            },

            CoreExprKind::App { func, args } => CoreExprKind::App {
                func: Rc::new(f(func)),
                args: args.iter().map(f).collect(),
            },

            CoreExprKind::Apply { func, args } => CoreExprKind::Apply {
                func: Rc::new(f(func)),
                args: args.iter().map(f).collect(),
            },
        }
    }
}

impl std::fmt::Display for CoreExprKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreExprKind::Literal(v) => write!(f, "{}", v),
            CoreExprKind::Var { name, scopes } => {
                if scopes.is_empty() {
                    write!(f, "{}", name)
                } else {
                    write!(f, "{}@{}", name, scopes)
                }
            }
            CoreExprKind::Quote(v) => write!(f, "'{}", v),
            CoreExprKind::Quasiquote(template) => write!(f, "(quasiquote {})", template),
            CoreExprKind::Lambda { params, .. } => {
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
            CoreExprKind::If { test, then, else_ } => {
                write!(f, "(if {} {} {})", test, then, else_)
            }
            CoreExprKind::Set { var, scopes, value } => {
                if scopes.is_empty() {
                    write!(f, "(set! {} {})", var, value)
                } else {
                    write!(f, "(set! {}@{} {})", var, scopes, value)
                }
            }
            CoreExprKind::Begin(exprs) => {
                write!(f, "(begin")?;
                for expr in exprs {
                    write!(f, " {}", expr)?;
                }
                write!(f, ")")
            }
            CoreExprKind::Define {
                name,
                scopes,
                value,
            } => {
                if scopes.is_empty() {
                    write!(f, "(define {} {})", name, value)
                } else {
                    // `name@{scopes}`, the spelling `Var` and `Set` use above.
                    write!(f, "(define {}@{} {})", name, scopes, value)
                }
            }
            CoreExprKind::Import { import_sets } => {
                write!(f, "(import")?;
                for import_set in import_sets {
                    write!(f, " {}", import_set)?;
                }
                write!(f, ")")
            }
            CoreExprKind::Expand { expr } => {
                write!(f, "(expand {})", expr)
            }
            CoreExprKind::App { func, args } => {
                write!(f, "({}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, ")")
            }
            CoreExprKind::Apply { func, args } => {
                write!(f, "(apply {}", func)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                write!(f, ")")
            }
        }
    }
}
