//! Environment and variable handling for CPS evaluation
//!
//! This module contains functions for:
//! - Variable lookup and mutation
//! - CPS closure creation
//! - Trivial expression evaluation

use super::CpsEvaluator;
use super::types::ContEnv;
use crate::eval::error::EvalError;
use patina_core::Procedure;
use patina_core::cps_expr::{CpsExpr, CpsExprKind, CpsParam};
use patina_core::tagged_value::TaggedValue;
use patina_core::{Environment, ScopeSet, ScopedParam};
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Evaluate a trivial expression to a TaggedValue
    ///
    /// Trivial expressions don't have control effects and evaluate immediately.
    /// Returns TaggedValue directly for efficient passing through StepResult.
    pub(super) fn eval_trivial_tagged(
        &self,
        expr: &CpsExpr,
        env: &Rc<Environment>,
        cont_env: &ContEnv,
    ) -> Result<TaggedValue, EvalError> {
        match &expr.kind {
            CpsExprKind::Literal(v) => Ok(*v),

            CpsExprKind::Var { name, scopes } => self.lookup_var_tagged(name, scopes, env),

            CpsExprKind::ContRef(k) => {
                let cont = cont_env
                    .get(k)
                    .ok_or_else(|| EvalError::UndefinedVariable(k.to_string()))?;
                // Reify with empty dynamic winds for now (will be filled in by caller if needed)
                Ok(self.reify_continuation_tagged(cont, cont_env, &[]))
            }

            CpsExprKind::Lambda {
                params,
                variadic,
                cont_param,
                body,
                binding_scope,
            } => Ok(self.make_cps_closure_tagged(
                params,
                variadic.as_ref(),
                cont_param,
                body,
                env,
                *binding_scope,
            )),

            _ => Err(EvalError::InternalError(format!(
                "Non-trivial expression in trivial position: {}",
                expr.expr_kind()
            ))),
        }
    }

    /// Look up a variable in the environment, returning TaggedValue directly
    pub(super) fn lookup_var_tagged(
        &self,
        name: &str,
        scopes: &ScopeSet,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        // Use scoped lookup if scopes are present (for hygienic macros)
        if scopes.is_empty() {
            env.get(name)
                .ok_or_else(|| EvalError::UndefinedVariable(name.to_string()))
        } else {
            // Scope-based lookup for hygienic macros
            env.get_with_scopes(name, scopes)
                .ok_or_else(|| EvalError::UndefinedVariable(name.to_string()))
        }
    }

    /// Set a variable in the environment (TaggedValue version)
    pub(super) fn set_var_tagged(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
        env: &Rc<Environment>,
    ) -> Result<(), EvalError> {
        // Use scoped set if scopes are present (for hygienic macros)
        if scopes.is_empty() {
            env.set(name, value)
                .map_err(|_| EvalError::UndefinedVariable(name.to_string()))
        } else {
            // Scope-based set for hygienic macros
            env.set_with_scopes(name, scopes, value)
                .map_err(|_| EvalError::UndefinedVariable(name.to_string()))
        }
    }

    /// Create a CPS closure as TaggedValue
    ///
    /// CPS lambdas use the `Procedure::CpsLambda` variant which stores the actual
    /// CpsExpr body. When applied in `apply_cps`, the CPS body is evaluated with
    /// the continuation parameter bound to the current continuation.
    pub(super) fn make_cps_closure_tagged(
        &self,
        params: &[CpsParam],
        variadic: Option<&CpsParam>,
        cont_param: &Rc<str>,
        body: &Rc<CpsExpr>,
        env: &Rc<Environment>,
        binding_scope: Option<patina_core::ScopeId>,
    ) -> TaggedValue {
        // Convert CpsParams to ScopedParams
        let scoped_params: Vec<ScopedParam> = params
            .iter()
            .map(|p| ScopedParam {
                name: p.name.clone(),
                scopes: p.scopes.clone(),
            })
            .collect();

        let variadic_param = variadic.map(|p| ScopedParam {
            name: p.name.clone(),
            scopes: p.scopes.clone(),
        });

        // Allocate CpsLambda procedure directly on the heap
        self.evaluator
            .global_env
            .heap()
            .borrow_mut()
            .alloc_procedure(Rc::new(Procedure::CpsLambda {
                params: scoped_params,
                variadic: variadic_param,
                cont_param: cont_param.clone(),
                body: body.clone(),
                env: env.clone(),
                binding_scope,
            }))
    }
}
