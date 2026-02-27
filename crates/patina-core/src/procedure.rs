use std::rc::Rc;

use crate::core_expr::ScopedParam;
use crate::cps_expr::CpsExpr;
use crate::environment::Environment;
use crate::scope::ScopeId;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Arity {
    Exact(usize),
    Min(usize),
    Range(usize, usize),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Procedure {
    /// Built-in primitive procedure
    Primitive {
        name: &'static str,
        arity: Arity,
        /// Pre-computed "library/name" key for PrimitiveRegistry lookup.
        /// Computed once at registration time to avoid format!() on every call.
        qualified_name: Rc<str>,
    },

    /// CPS-style lambda - for use with CPS evaluator
    ///
    /// These lambdas are created by CPS transformation and must be evaluated
    /// by the CPS evaluator. They have an explicit continuation parameter
    /// and their body is a CpsExpr that will call the continuation.
    CpsLambda {
        /// Fixed parameters, each with optional scopes for hygiene
        params: Vec<ScopedParam>,
        /// Optional variadic parameter (rest parameter)
        variadic: Option<ScopedParam>,
        /// Name of the continuation parameter
        cont_param: Rc<str>,
        /// Procedure body (CPS-style CpsExpr)
        body: Rc<CpsExpr>,
        /// Captured environment for closures
        env: Rc<Environment>,
        /// Binding scope for parameters without scopes (for hygiene)
        /// When present, parameters without explicit scopes will also be bound
        /// with this scope, allowing macro-expanded references to find them.
        binding_scope: Option<ScopeId>,
    },
}
