use crate::error::SourceLocation;
use crate::literal_pool::{Link, Lit};
use crate::scope::ScopeSet;
#[cfg(test)]
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

impl Drop for CoreExpr {
    fn drop(&mut self) {
        // The drop glue that follows recurses into the children, a level of
        // the code at a time; where the stack has no room for that, they go
        // to a new segment (#617). A leaf, most of any tree, has none.
        let leaf = matches!(
            self.kind,
            CoreExprKind::Literal(_)
                | CoreExprKind::Var { .. }
                | CoreExprKind::Quote(_)
                | CoreExprKind::Import { .. }
        );
        if !leaf && !crate::walk::has_stack_room() {
            let placeholder = CoreExprKind::Literal(Lit::UNSPECIFIED);
            crate::walk::drop_deep(std::mem::replace(&mut self.kind, placeholder));
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
    /// An immediate inline, anything on the heap an entry of the
    /// compilation's literal pool (#705, `crate::literal_pool`).
    Literal(Lit),

    /// Variable reference (with optional hygiene scopes)
    /// Example: x, my-function
    /// For hygienic macros, scopes carries scope information from macro expansion.
    /// The evaluator uses scope-based lookup when scopes is non-empty:
    /// finds binding where binding.scopes ⊆ reference.scopes.
    /// Empty scopes means simple name lookup (non-macro code).
    ///
    /// `link`, where set, is the binding the reference means, which no
    /// backend resolves again (#837): a binding record, an entry of the
    /// compilation's literal pool, which an early-bound template reference
    /// links. A linked reference keeps its spelling, for messages and for
    /// the desugarer's settling, and always has scopes
    /// ([`CoreExprKind::linked_var`]). Nothing links one yet: every
    /// reference the desugarer makes has `link: None` until 4b.10's switch.
    Var {
        name: Symbol,
        scopes: ScopeSet,
        link: Option<Link>,
    },

    /// Quote: literal data
    /// Example: 'x, '(1 2 3)
    /// The datum, through the compilation's literal pool, as `Literal`'s.
    Quote(Lit),

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
    ///
    /// `link` is the binding assigned, where set, as [`CoreExprKind::Var`]'s
    /// (#837).
    Set {
        var: Symbol,
        scopes: ScopeSet,
        value: Rc<CoreExpr>,
        link: Option<Link>,
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
    /// Import sets are kept as data (declarative, not code), through the
    /// compilation's literal pool.
    Import { import_sets: Vec<Lit> },

    // Note: Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)
    // The CoreExpr::Parameterize variant has been removed.
    /// Reserved macro-expansion debug IR. Execution semantics are not
    /// implemented: the frontend rejects `expand`, and executable-IR
    /// validation rejects a hand-built node rather than discarding it.
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
    Datum(Lit),
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
            // Grown with the depth of the template (#617).
            QuasiTemplate::Build(constructor, parts) => {
                crate::walk::ensure_sufficient_stack(|| {
                    QuasiTemplate::Build(
                        *constructor,
                        parts.iter().map(|part| part.map_unquoted(f)).collect(),
                    )
                })
            }
        }
    }

    /// Call `f` on each unquoted expression, in the template's order.
    pub fn for_each_unquoted(&self, f: &mut dyn FnMut(&CoreExpr)) {
        match self {
            QuasiTemplate::Datum(_) => {}
            QuasiTemplate::Unquoted(expr) => f(expr),
            // Grown with the depth of the template (#617).
            QuasiTemplate::Build(_, parts) => crate::walk::ensure_sufficient_stack(|| {
                for part in parts {
                    part.for_each_unquoted(f);
                }
            }),
        }
    }
}

impl Drop for QuasiTemplate {
    fn drop(&mut self) {
        // As `CoreExpr`'s, a level of the template at a time (#617).
        if let QuasiTemplate::Build(_, parts) = self
            && !crate::walk::has_stack_room()
        {
            crate::walk::drop_deep(std::mem::take(parts));
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
    /// A reference to `name` at `scopes`, resolved by the backend.
    pub fn var(name: Symbol, scopes: ScopeSet) -> Self {
        CoreExprKind::Var {
            name,
            scopes,
            link: None,
        }
    }

    /// A reference to `name` at `scopes` bound to `link` where it is set
    /// (#837). A link is made only for a macro-introduced reference, which
    /// carries its expansion's scope, and a backend relies on it: asserted
    /// in every build.
    #[track_caller]
    pub fn linked_var(name: Symbol, scopes: ScopeSet, link: Option<Link>) -> Self {
        assert!(
            link.is_none() || !scopes.is_empty(),
            "a linked reference has its expansion's scopes (#837)"
        );
        CoreExprKind::Var { name, scopes, link }
    }

    /// An assignment of `value` to `var` at `scopes`, resolved by the
    /// backend.
    pub fn set(var: Symbol, scopes: ScopeSet, value: Rc<CoreExpr>) -> Self {
        CoreExprKind::Set {
            var,
            scopes,
            value,
            link: None,
        }
    }

    /// An assignment bound to `link` where it is set, as
    /// [`linked_var`](Self::linked_var) (#837).
    #[track_caller]
    pub fn linked_set(
        var: Symbol,
        scopes: ScopeSet,
        value: Rc<CoreExpr>,
        link: Option<Link>,
    ) -> Self {
        assert!(
            link.is_none() || !scopes.is_empty(),
            "a linked assignment has its expansion's scopes (#837)"
        );
        CoreExprKind::Set {
            var,
            scopes,
            value,
            link,
        }
    }

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

            // The link is carried: a rebuild that dropped it would resolve
            // the assignment by name again (#837).
            CoreExprKind::Set {
                var,
                scopes,
                value,
                link,
            } => CoreExprKind::Set {
                var: var.clone(),
                scopes: scopes.clone(),
                value: Rc::new(f(value)),
                link: *link,
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
            CoreExprKind::Var { name, scopes, link } => {
                if scopes.is_empty() {
                    write!(f, "{}", name)?;
                } else {
                    write!(f, "{}@{}", name, scopes)?;
                }
                match link {
                    Some(link) => write!(f, "→{link}"),
                    None => Ok(()),
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
            CoreExprKind::Set {
                var,
                scopes,
                value,
                link,
            } => {
                write!(f, "(set! {var}")?;
                if !scopes.is_empty() {
                    write!(f, "@{scopes}")?;
                }
                if let Some(link) = link {
                    write!(f, "→{link}")?;
                }
                write!(f, " {value})")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A linked reference's link (#837) costs `CoreExprKind` nothing: the
    /// pool's number is never 0, so `Option<Link>` fits a `Link`'s 8 bytes,
    /// and `Var` and `Set` stay below `Lambda`, the largest variant, 112
    /// bytes before the field as after it (measured 2026-11-13).
    #[test]
    fn a_link_leaves_the_node_its_size() {
        assert_eq!(std::mem::size_of::<Option<Link>>(), 8);
        assert_eq!(std::mem::size_of::<CoreExprKind>(), 112);
        assert_eq!(std::mem::size_of::<CoreExpr>(), 168);
    }

    /// A link displays beside the spelling, and a rebuild of the node's
    /// children carries it; a linked node without scopes is refused.
    #[test]
    fn a_link_displays_and_survives_a_rebuild() {
        use crate::binding_record::{BindingLink, Linkable};
        use crate::environment::Environment;
        use crate::literal_pool::LiteralPool;

        let heap = crate::heap::new_shared_heap();
        let pool = LiteralPool::new(&heap.borrow());
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "f", TaggedValue::fixnum(1));
        let record = ns.local_record("f").unwrap();
        let link = pool.add_link(Linkable::new(BindingLink::new(record)));
        let scopes = ScopeSet::from_iter([crate::scope::ScopeId(3)]);

        let var = CoreExprKind::linked_var("f".into(), scopes.clone(), Some(link));
        assert_eq!(var.to_string(), format!("f@{scopes}→#<link 0>"));
        assert_eq!(
            CoreExprKind::var("f".into(), ScopeSet::new()).to_string(),
            "f"
        );
        let set = CoreExpr::new(CoreExprKind::linked_set(
            "f".into(),
            scopes.clone(),
            Rc::new(CoreExpr::new(var)),
            Some(link),
        ));
        assert_eq!(
            set.to_string(),
            format!("(set! f@{scopes}→#<link 0> f@{scopes}→#<link 0>)")
        );
        let rebuilt = set.map_children(|child| child.clone());
        match &rebuilt.kind {
            CoreExprKind::Set { link: carried, .. } => assert_eq!(*carried, Some(link)),
            other => panic!("{other}"),
        }
        assert_eq!(pool.link(link).record(), record);

        let refused = std::panic::catch_unwind(|| {
            CoreExprKind::linked_var("f".into(), ScopeSet::new(), Some(link))
        });
        assert!(refused.is_err(), "a linked reference has scopes");
    }

    /// A rebuild carries an unquoted reference's link through the
    /// template (`QuasiTemplate::map_unquoted`), as settling rebuilds a
    /// form (#837): dropped there, the reference would be resolved by name
    /// again.
    #[test]
    fn an_unquoted_link_survives_a_rebuild() {
        use crate::binding_record::{BindingLink, Linkable};
        use crate::environment::Environment;
        use crate::literal_pool::LiteralPool;

        let heap = crate::heap::new_shared_heap();
        let pool = LiteralPool::new(&heap.borrow());
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "f", TaggedValue::fixnum(1));
        let record = ns.local_record("f").unwrap();
        let link = pool.add_link(Linkable::new(BindingLink::new(record)));
        let scopes = ScopeSet::from_iter([crate::scope::ScopeId(3)]);
        let template = CoreExpr::new(CoreExprKind::Quasiquote(QuasiTemplate::Build(
            QuasiConstructor::List,
            vec![QuasiTemplate::Unquoted(Rc::new(CoreExpr::new(
                CoreExprKind::linked_var("f".into(), scopes, Some(link)),
            )))],
        )));
        let rebuilt = template.map_children(|child| child.clone());
        let CoreExprKind::Quasiquote(QuasiTemplate::Build(_, parts)) = &rebuilt.kind else {
            panic!("{rebuilt}")
        };
        match parts.as_slice() {
            [QuasiTemplate::Unquoted(expr)] => match &expr.kind {
                CoreExprKind::Var { link: carried, .. } => assert_eq!(*carried, Some(link)),
                other => panic!("{other}"),
            },
            other => panic!("{other:?}"),
        }
    }

    /// Dropping a tree recurses a level at a time; on a 256 KiB thread these
    /// are each far deeper than the stack would hold without the `Drop` that
    /// moves to a new segment (#617).
    #[test]
    fn trees_deeper_than_the_stack_drop() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let leaf = || {
                    CoreExpr::new(CoreExprKind::Literal(Lit::immediate(TaggedValue::fixnum(
                        0,
                    ))))
                };
                let mut nested = leaf();
                for _ in 0..100_000 {
                    // Through a `Vec` and through an `Rc`.
                    nested = CoreExpr::new(CoreExprKind::If {
                        test: Rc::new(leaf()),
                        then: Rc::new(CoreExpr::new(CoreExprKind::Begin(vec![nested]))),
                        else_: Rc::new(leaf()),
                    });
                }
                drop(nested);
                let mut template = QuasiTemplate::Datum(Lit::immediate(TaggedValue::NULL));
                for _ in 0..100_000 {
                    template = QuasiTemplate::Build(QuasiConstructor::List, vec![template]);
                }
                drop(template);
            })
            .expect("spawn")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    }
}
