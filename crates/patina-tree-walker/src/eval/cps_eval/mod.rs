//! CPS Evaluator - Evaluates CPS (Continuation-Passing Style) expressions
//!
//! This module implements evaluation of CpsExpr, which is the IR used for
//! implementing first-class continuations (call/cc). Delimited continuations
//! — the prompt API — are not IR forms but apply-time control primitives
//! over the same machine; see `prompts.rs`.
//!
//! # Architecture
//!
//! ```text
//! CoreExpr → [CPS Transform] → CpsExpr → [THIS MODULE] → Value
//! ```
//!
//! The CPS evaluator handles the following expression forms:
//!
//! **Trivial expressions** (evaluate immediately):
//! - `Literal` - Self-evaluating values
//! - `Var` - Variable references
//! - `ContRef` - Continuation variable references
//! - `Lambda` - CPS lambda (takes continuation parameter)
//!
//! **Serious expressions** (use continuations):
//! - `LetVal` - Bind trivial value and continue
//! - `LetCont` - Define local continuation
//! - `App` - Application with explicit continuation
//! - `Continue` - Invoke continuation with value
//! - `If` - Conditional
//! - `Set` - Mutation
//! - `Define` - Definition
//!
//! **Control operators**:
//! - `CallCC` - Capture current continuation
//!
//! - `PrimOp` - Primitive operations
//! - `Halt` - Program termination
//!
//! # Continuation Representation
//!
//! Continuations are represented as `Value::Continuation(CpsContinuation)` which
//! stores the continuation body, parameter name, captured environment, and
//! dynamic-wind state.
//!
//! # Trampoline Architecture
//!
//! The evaluator uses a fully-trampolined design to avoid Rust stack overflow:
//! - All evaluation returns a `StepResult` enum indicating either a final value
//!   or the next expression to evaluate
//! - A single loop in `eval()` processes steps iteratively
//! - No recursive Rust calls during normal CPS evaluation
//!
//! # Module Organization
//!
//! - `mod.rs` - CpsEvaluator struct, entry points, trampoline loop
//! - `types.rs` - ContEnv, ContValue, StepResult, PromptFrame, ExceptionHandler
//! - `environment.rs` - Variable lookup, closure creation
//! - `step.rs` - eval_one_step (main dispatch)
//! - `application.rs` - apply_cps_step (procedure application)
//! - `continuation.rs` - Continuation capture/restore, reification
//! - `wind.rs` - Dynamic-wind handling, promise forcing
//! - `prompts.rs` - The prompt API: prompts, aborts, composable continuations
//! - `exceptions.rs` - Exception routing through CPS handlers
//! - `quasiquote.rs` - Quasiquote template evaluation

mod application;
mod callback;
mod continuation;
mod environment;
mod exceptions;
mod gc_roots;
mod prompts;
pub mod quasiquote;
mod step;
mod types;
mod wind;

use crate::eval::error::EvalError;
use patina_core::Environment;
use patina_core::TaggedValue;
use patina_core::cps_expr::CpsExpr;
use patina_core::{GcController, GcDeferGuard};
use std::rc::Rc;
use tracing::debug;

use types::{ContEnv, StepResult, take_pending_escape};

/// CPS Evaluator state
///
/// Manages the evaluation of CpsExpr with support for:
/// - Continuation environment (mapping ContVar to ContValue)
/// - Prompt stack for delimited continuations
/// - Dynamic-wind state
pub struct CpsEvaluator<'a> {
    /// The tree-walker evaluator for primitive operations
    evaluator: &'a super::Evaluator,
}

impl<'a> CpsEvaluator<'a> {
    /// Create a new CPS evaluator
    pub fn new(evaluator: &'a super::Evaluator) -> Self {
        Self { evaluator }
    }

    // apply_from_direct is defined in wind.rs

    /// GC safe point: the tree-walker's root set, handed to the shared driver.
    ///
    /// Called at the top of the trampoline loop, where the entire live machine
    /// state is reachable from `step` + `expr` and no heap borrow is
    /// outstanding (`docs/GC_DESIGN.md` §7). The protocol itself lives in
    /// `GcController::safe_point`; this supplies only the roots.
    #[inline]
    fn maybe_collect(&self, is_outermost: bool, step: &StepResult, expr: Option<&CpsExpr>) {
        let evaluator = self.evaluator;
        GcController::safe_point(
            &evaluator.gc,
            evaluator.global_env.heap(),
            &evaluator.gc_pending,
            is_outermost,
            |collect| {
                // Libraries are a root set. If a load is in flight we cannot
                // read the registry, so return without collecting rather than
                // trace a partial root set — a missing root is a
                // use-after-free.
                let Ok(registry) = evaluator.library_registry.try_borrow() else {
                    return;
                };
                let step_roots = gc_roots::StepRoots { step, expr };
                collect(&[evaluator, &*registry, &gc_roots::EscapeRoots, &step_roots]);
            },
        );
    }

    /// Evaluate a CPS expression to a final value
    ///
    /// This is the main entry point for CPS evaluation. The expression
    /// should be produced by CpsTransformer::transform_toplevel() which
    /// wraps the result in a Halt continuation.
    ///
    /// Uses a trampoline pattern to avoid Rust stack overflow on deep recursion.
    /// All CPS evaluation steps are processed iteratively in a single loop.
    ///
    /// Returns TaggedValue for efficient storage and to avoid Value round-trips.
    pub fn eval(&self, expr: &CpsExpr) -> Result<TaggedValue, EvalError> {
        self.eval_in_env(Rc::new(expr.clone()), self.evaluator.global_env.clone())
    }

    /// Evaluate a CPS expression in a specific environment
    ///
    /// Like `eval`, but allows specifying the environment to use.
    /// This is needed for library loading where definitions should
    /// go into the library's environment, not the global environment.
    ///
    /// Returns TaggedValue for efficient storage and to avoid Value round-trips.
    /// Takes the expression by `Rc` because the step loop walks it that way:
    /// descending into a `let`, an `if` branch or a lambda body is then a
    /// refcount bump rather than a deep clone of the subtree. It is also the
    /// node the trampoline's safe point roots, so the collector traces the
    /// tree the loop is actually walking rather than a copy of it.
    pub fn eval_in_env(
        &self,
        expr: Rc<CpsExpr>,
        env: Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        self.eval_in_env_with(expr, env, Vec::new(), Vec::new(), Vec::new())
    }

    /// Evaluate `expr` on a trampoline that starts under the given dynamic
    /// environment — empty for a top-level form, the calling step's for the
    /// `eval` primitive (`callback.rs`).
    pub(super) fn eval_in_env_with(
        &self,
        expr: Rc<CpsExpr>,
        env: Rc<Environment>,
        prompt_stack: Vec<types::PromptFrame>,
        dynamic_winds: Vec<patina_core::DynamicWindRecord>,
        exception_handlers: Vec<types::ExceptionHandler>,
    ) -> Result<TaggedValue, EvalError> {
        // Debug: show the CPS expression (enabled via RUST_LOG=patina_tree_walker::eval::cps_eval=debug)
        debug!(target: "patina_tree_walker::eval::cps_eval", input_expr = %expr, "CPS evaluation starting");
        let initial = StepResult::Continue {
            expr: expr.clone(),
            env,
            cont_env: ContEnv::new(),
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        };
        self.run_trampoline(initial, Some(&expr))
    }

    /// The step loop: run `initial` and every step after it until one
    /// delivers to this trampoline's `Halt`.
    ///
    /// One loop for every trampoline — a top-level form, a primitive's
    /// callback, the `eval` primitive's expression — because they differ in
    /// only two things, and both are decided here rather than by which copy
    /// of the loop is running: which stacks the first step carries (the
    /// caller's), and which continuations belong to this run
    /// (`TrampolineGuard`). Until 2026-09-10 the nested runs had a loop of
    /// their own with no escape arm, so the only continuation they could
    /// resume was none, and the outermost loop resumed everything that
    /// reached it — including chains that ended in a nested `Halt`, which it
    /// then delivered as the program's value.
    ///
    /// `expr` is the expression an outermost run was entered with, rooted at
    /// the safe point for its literals; a callback run has none. Every
    /// trampoline defers collection for its own extent and only the outermost
    /// one reaches its safe point un-deferred — a nested trampoline's caller
    /// holds live values in Rust locals that no root provider can see
    /// (`docs/GC_DESIGN.md` §7).
    fn run_trampoline(
        &self,
        initial: StepResult,
        expr: Option<&CpsExpr>,
    ) -> Result<TaggedValue, EvalError> {
        let gc_defer = GcDeferGuard::new(self.evaluator.global_env.heap());
        // Loop invariant, hoisted out of the safe point (see maybe_collect).
        let is_outermost = gc_defer.is_outermost();
        let trampoline = types::TrampolineGuard::enter();

        let mut current_step = initial;
        let mut step_count = 0;

        loop {
            // GC safe point: all live state is in `current_step` and `expr`,
            // both rooted below. No heap borrow is outstanding here.
            self.maybe_collect(is_outermost, &current_step, expr);

            step_count += 1;
            if step_count <= 30 {
                debug!(
                    target: "patina_tree_walker::eval::cps_eval",
                    step = step_count,
                    step_type = ?std::mem::discriminant(&current_step),
                    "CPS step"
                );
            }
            let step_result = match current_step {
                StepResult::Done(value) => {
                    debug!(
                        target: "patina_tree_walker::eval::cps_eval",
                        steps = step_count,
                        result = %value,
                        "CPS evaluation complete"
                    );
                    return Ok(value);
                }

                StepResult::Continue {
                    expr,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.eval_one_step(
                    &expr,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),

                StepResult::InvokeContinuation {
                    cont,
                    value,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.invoke_continuation_step(
                    cont,
                    value,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),

                StepResult::ApplyProc {
                    proc,
                    args,
                    cont,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.apply_cps_step(
                    proc,
                    args,
                    cont,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),
            };

            match step_result {
                Ok(step) => current_step = step,
                Err(EvalError::ContinuationEscape) => {
                    // A jump parked its target and unwound to here. Three
                    // cases, by the trampoline the target's chain ends in:
                    // this one, so resume it; an enclosing one still running,
                    // so keep unwinding — this loop is a primitive's
                    // callback, and the primitive is abandoned by the `?`
                    // its caller put on this call; or one that has already
                    // returned, whose `Halt` has no primitive left to return
                    // to. A jump inside its own trampoline never gets here:
                    // `jump_to_continuation` resumes those in place.
                    let Some((value_tagged, k)) = take_pending_escape() else {
                        return Err(EvalError::InternalError(
                            "ContinuationEscape without pending data".to_string(),
                        ));
                    };
                    if k.trampoline != trampoline.id() {
                        if types::trampoline_is_active(k.trampoline) {
                            types::set_pending_escape(value_tagged, k);
                            return Err(EvalError::ContinuationEscape);
                        }
                        return Err(EvalError::SchemeException {
                            kind: patina_core::ExceptionKind::Error,
                            message: "a continuation captured inside a primitive's callback \
                                      was invoked after that primitive returned; the \
                                      tree-walker cannot resume a callback whose primitive \
                                      is gone"
                                .to_string(),
                            irritants_display: String::new(),
                        });
                    }
                    debug!(
                        target: "patina_tree_walker::eval::cps_eval",
                        escaped_value = ?value_tagged,
                        "Continuation escape"
                    );

                    // Resume the captured continuation with the environment
                    // it names — `exception_handlers` and `prompt_stack`
                    // restored from it like `dynamic_winds`, since R7RS 6.11
                    // puts all three in the dynamic environment. An abort
                    // lands here too (`prompts.rs`), with a continuation
                    // whose stacks are cut back to its prompt. The wind
                    // thunks between the jump and here have already run, as
                    // steps of the trampoline the jump was made on, each in
                    // its own `dynamic-wind` call's environment (`wind.rs`).
                    //
                    // As a step rather than invoked here: when `resume` is
                    // itself a `Jump` — the continuation of a wind thunk's
                    // tail call, captured while a jump was running the thunk
                    // — invoking it parks a *second* escape, and a `?` on it
                    // would carry that escape out of the trampoline as an
                    // error. As a step it lands in this same arm on the next
                    // turn.
                    current_step = StepResult::InvokeContinuation {
                        cont: continuation::continuation_cont_value(&k),
                        value: value_tagged,
                        env: k.env.clone(),
                        cont_env: k.captured_cont_env.clone(),
                        prompt_stack: k.prompt_stack.clone(),
                        dynamic_winds: k.dynamic_winds.clone(),
                        exception_handlers: k.exception_handlers.clone(),
                    };
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Evaluate a CoreExpr using CPS transformation
///
/// This is the main entry point for CPS-based evaluation. It:
/// 1. Transforms CoreExpr to CpsExpr using CpsTransformer
/// 2. Evaluates the CpsExpr using CpsEvaluator
///
/// This function should be used when call/cc or delimited continuations
/// are needed. For regular code, use `eval_core` instead for better performance.
///
/// # Arguments
/// * `expr` - The CoreExpr to evaluate
/// * `env` - The environment for variable lookup and definitions
/// * `evaluator` - The evaluator instance (wrapped in Rc for sharing)
///
/// # Returns
/// The result of evaluating the expression as TaggedValue
pub fn eval_cps(
    expr: &patina_core::CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<TaggedValue, EvalError> {
    use patina_core::CoreExprKind;
    use patina_ir::CpsTransformer;

    // Handle Import specially - it's a side-effect that modifies the environment
    // and doesn't need CPS transformation
    if let CoreExprKind::Import { import_sets } = &expr.kind {
        let heap = evaluator.global_env.heap();
        for import_set in import_sets {
            let import_set =
                patina_frontend::LibraryDefinition::parse_import_set_tagged(*import_set, heap)
                    .map_err(|e| EvalError::InvalidSyntax(format!("Invalid import set: {}", e)))?;
            evaluator.process_import_for_eval(&import_set, &env)?;
        }
        return Ok(TaggedValue::UNSPECIFIED);
    }

    // Transform CoreExpr to CpsExpr
    let transformer = CpsTransformer::new();
    let cps_expr = transformer.transform_toplevel(expr);

    // Create CPS evaluator and evaluate in the specified environment
    let cps_evaluator = CpsEvaluator::new(evaluator);
    cps_evaluator.eval_in_env(Rc::new(cps_expr), env)
}

/// [`eval_cps`], on a trampoline that starts under the given dynamic
/// environment — the `eval` primitive's entry, from a step that has one.
pub(super) fn eval_cps_with(
    expr: &patina_core::CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
    prompt_stack: Vec<types::PromptFrame>,
    dynamic_winds: Vec<patina_core::DynamicWindRecord>,
    exception_handlers: Vec<types::ExceptionHandler>,
) -> Result<TaggedValue, EvalError> {
    use patina_core::CoreExprKind;
    use patina_ir::CpsTransformer;

    if let CoreExprKind::Import { .. } = &expr.kind {
        // An import touches no dynamic state; the plain entry handles it.
        return eval_cps(expr, env, evaluator);
    }
    let cps_expr = CpsTransformer::new().transform_toplevel(expr);
    CpsEvaluator::new(evaluator).eval_in_env_with(
        Rc::new(cps_expr),
        env,
        prompt_stack,
        dynamic_winds,
        exception_handlers,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::ScopeSet;
    use patina_core::TaggedValue;
    use patina_core::cps_expr::{CpsExprKind, CpsPrimitive};

    fn make_test_evaluator() -> super::super::Evaluator {
        super::super::Evaluator::new()
    }

    #[test]
    fn test_cps_eval_literal() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (halt 42)
        let expr = CpsExpr::new(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Literal(
            TaggedValue::fixnum(42),
        ))));
        let result = cps_eval.eval(&expr).unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_cps_eval_variable() {
        let evaluator = make_test_evaluator();
        evaluator
            .global_env
            .define("x".to_string(), TaggedValue::fixnum(10));
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (halt x)
        let expr = CpsExpr::new(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Var {
            name: Rc::from("x"),
            scopes: ScopeSet::new(),
        })));
        let result = cps_eval.eval(&expr).unwrap();
        assert_eq!(result.as_fixnum(), Some(10));
    }

    #[test]
    fn test_cps_eval_primop() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (let-cont ((k result) (halt result)) (+ 1 2 k))
        let halt_cont = CpsExpr::new(CpsExprKind::LetCont {
            name: Rc::from("k"),
            param: Rc::from("result"),
            cont_body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Var {
                name: Rc::from("result"),
                scopes: ScopeSet::new(),
            }))),
            body: CpsExpr::rc(CpsExprKind::PrimOp {
                op: CpsPrimitive::Add,
                args: vec![
                    CpsExpr::new(CpsExprKind::Literal(TaggedValue::fixnum(1))),
                    CpsExpr::new(CpsExprKind::Literal(TaggedValue::fixnum(2))),
                ],
                cont: Rc::from("k"),
            }),
        });

        let result = cps_eval.eval(&halt_cont).unwrap();
        assert_eq!(result.as_fixnum(), Some(3));
    }
}
