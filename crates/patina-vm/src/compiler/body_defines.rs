//! Which definitions a body or `Begin` contributes.
//!
//! One walk, shared, because two passes need the *same* answer and disagreed
//! about it. `alpha_rename` renames a lambda's internal definitions to unique
//! locals; `pass1_analysis` collects those same names to give them register
//! slots. A definition one pass sees and the other misses is renamed to a
//! local and then allocated as a global, so every call to the enclosing
//! lambda shares one cell:
//!
//! ```scheme
//! (define (make x) (begin (define-values (a b) (values x x)) (lambda () a)))
//! (define f1 (make 1)) (define f2 (make 2))
//! (list (f1) (f2))     ;; was (2 2) on the VM; (1 2) everywhere else
//! ```
//!
//! `begin` splices, so a definition inside one is a definition of the
//! enclosing body however deep it sits — and a macro reaches two levels
//! easily, since `define-values` expands to a `begin` of definitions that a
//! caller's own macro then wraps in another.
//!
//! "Shared" means shared by the `CoreExpr` passes here. There is a sibling
//! question one IR earlier — `Desugarer::is_regular_define_tagged` decides
//! whether a body has internal definitions by looking at direct children of a
//! *datum*, and also does not see through `begin` — which this cannot serve
//! because it is a different type, not because anyone chose to duplicate it.

use patina_core::core_expr::{CoreExpr, CoreExprKind, Symbol};
use patina_core::scope::ScopeSet;

/// Call `f` for each definition `exprs` contributes, looking through `Begin`
/// at any depth.
///
/// `Lambda` is deliberately not descended into: its body is its own scope and
/// contributes nothing here.
pub(crate) fn for_each_define(exprs: &[CoreExpr], f: &mut impl FnMut(&Symbol, &ScopeSet)) {
    for expr in exprs {
        match &expr.kind {
            CoreExprKind::Define { name, scopes, .. } => f(name, scopes),
            CoreExprKind::Begin(inner) => for_each_define(inner, f),
            _ => {}
        }
    }
}
