//! The [`ApplyContext`] a step hands a primitive: the evaluator, plus the
//! step's own dynamic environment.
//!
//! `environment` and evaluated imports can initialize a Scheme library.
//! That body must inherit the caller's handlers, winds and prompts, including
//! when a dependency is loaded (#425). This context carries those stacks by
//! reference; a nested run clones them only when evaluation is needed.
//! Calls from outside a running step use `detached`, with empty stacks.
//! `apply_proc` and `eval_expr` remain available to embedding callers; the
//! resumable primitives hand their calls and datums to the machine itself.

use super::CpsEvaluator;
use super::types::{ExceptionHandler, PromptFrame};
use crate::eval::error::EvalError;
use patina_core::{DynamicWindRecord, Environment, FileSystem, Library, SharedHeap, TaggedValue};
use patina_primitives::ApplyContext;
use patina_runtime::HasDiagnostic;
use std::rc::Rc;
use std::sync::Arc;

pub(crate) struct CallbackContext<'c, 'a, 's> {
    pub cps: &'c CpsEvaluator<'a>,
    pub prompt_stack: &'s [PromptFrame],
    pub dynamic_winds: &'s [DynamicWindRecord],
    pub exception_handlers: &'s [ExceptionHandler],
}

impl<'a> CallbackContext<'_, 'a, '_> {
    /// A library body, evaluated under the caller's handlers, winds and
    /// prompts. Only an error that actually leaves a nested run has already
    /// been offered to those handlers; import/expansion failures have not.
    #[expect(
        clippy::disallowed_methods,
        reason = "holds the lowered `expr`, not read after the call: the run roots the CPS tree it \
                  is entered with, and copies of the caller's stacks move into its first step"
    )]
    pub(crate) fn eval_core(
        &self,
        expr: &patina_core::CoreExpr,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        if let patina_core::CoreExprKind::Import { .. } = &expr.kind {
            return self.eval_import(expr, env);
        }
        let expr = super::lower_quasiquotes_for(expr, self.cps.evaluator)?;
        unhandled_is_final(super::eval_cps_with(
            &expr,
            env.clone(),
            self.cps.evaluator,
            self.prompt_stack.to_vec(),
            self.dynamic_winds.to_vec(),
            self.exception_handlers.to_vec(),
        ))
    }

    pub(crate) fn eval_import(
        &self,
        expr: &patina_core::CoreExpr,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        let patina_core::CoreExprKind::Import { import_sets } = &expr.kind else {
            unreachable!("eval_import requires an import")
        };
        for import_set in import_sets {
            let import_set = patina_frontend::LibraryDefinition::parse_import_set_tagged(
                *import_set,
                self.heap(),
            )
            .map_err(|e| {
                EvalError::InvalidSyntax(format!("Invalid import set: {e}"))
                    .with_diagnostic(e.diagnostic())
            })?;
            #[expect(
                clippy::disallowed_methods,
                reason = "holds the import's later import-set datums (in `expr`) across each load. \
                          A Scheme library's body runs while its `ParsedLibrary` holds \
                          `GcDeferGuard::holding`, and a Rust-defined library runs no Scheme, so \
                          no collection runs during the load"
            )]
            self.cps
                .evaluator
                .process_import_for_eval_with(&import_set, env, self)?;
        }
        Ok(TaggedValue::UNSPECIFIED)
    }

    /// The context for a call from outside any step: nothing to inherit.
    pub(crate) fn detached(cps: &'a CpsEvaluator<'a>) -> CallbackContext<'a, 'a, 'static> {
        CallbackContext {
            cps,
            prompt_stack: &[],
            dynamic_winds: &[],
            exception_handlers: &[],
        }
    }
}

/// A catchable error leaving the nested run was offered to every handler the
/// run inherited — the calling step's own — and declined by all of them.
/// Marking it is what stops the call site from routing it through the same
/// handlers a second time (`types::mark_unhandled_in_callback`).
fn unhandled_is_final(result: Result<TaggedValue, EvalError>) -> Result<TaggedValue, EvalError> {
    // A detached load has no primitive caller to consume the flag. Leaving
    // one set would make a later, unrelated call bypass its own handlers.
    if super::types::current_trampoline() != 0 && result.as_ref().is_err_and(|e| e.is_catchable()) {
        super::types::mark_unhandled_in_callback();
    }
    result
}

impl ApplyContext for CallbackContext<'_, '_, '_> {
    fn heap(&self) -> &SharedHeap {
        self.cps.evaluator.heap()
    }

    fn fs(&self) -> &Arc<dyn FileSystem> {
        self.cps.evaluator.fs()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "holds nothing: `proc`, `args` and copies of the step's stacks move into the run. \
                  The step that dispatched the primitive holds its own stacks in Rust \
                  (docs/GC_DESIGN.md §5.1), safe because this run is nested and its \
                  `run_trampoline` defers"
    )]
    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError> {
        unhandled_is_final(self.cps.apply_from_direct_with(
            proc,
            args,
            self.prompt_stack.to_vec(),
            self.dynamic_winds.to_vec(),
            self.exception_handlers.to_vec(),
        ))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "holds `core_expr`, the expanded datum, not read after the call: the run roots \
                  the CPS tree it is entered with"
    )]
    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        let evaluator = self.cps.evaluator;
        let core_expr = expand_for_eval(evaluator, expr, env, self)?;

        self.eval_core(&core_expr, env)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "holds nothing; a Scheme library's body runs while its `ParsedLibrary` holds \
                  `GcDeferGuard::holding`"
    )]
    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError> {
        self.cps
            .evaluator
            .load_library_with(name, self)
            .map_err(patina_runtime::LibraryError::into_eval_error)
    }

    fn interaction_environment(&self) -> Rc<Environment> {
        self.cps.evaluator.interaction_environment()
    }
}

/// Expand the datum `expr` in `env` and lower its quasiquotes, as `eval`
/// does before running it — on a run of its own ([`CallbackContext`]'s
/// `eval_expr`) or on the running trampoline (a resumable primitive's
/// `Step::Eval`, #477).
///
/// Bad syntax handed to `eval` is the caller's error, raised while the
/// program runs — catchable, like the VM's path. Lowered here rather than
/// inside `eval_cps_with`: below `unhandled_is_final` a catchable error is
/// marked as having escaped a callback, and a `guard` around `eval` stops
/// seeing it. A malformed template is refused by the desugar now (#445), so
/// all that can fail in the lowering is a constructor missing from the
/// registry.
pub(super) fn expand_for_eval(
    evaluator: &crate::eval::Evaluator,
    expr: TaggedValue,
    env: &Rc<Environment>,
    context: &CallbackContext<'_, '_, '_>,
) -> Result<patina_core::CoreExpr, EvalError> {
    let desugarer =
        patina_frontend::Desugarer::with_env(env.clone()).with_fs(evaluator.fs().clone());
    #[expect(
        clippy::disallowed_methods,
        reason = "the import callback loads libraries during the expansion, which holds the datum \
                  and the partial expansion: guarded by `desugar_with_imports`' \
                  `GcDeferGuard::holding`"
    )]
    let core_expr = desugarer.desugar_with_imports(
        expr,
        evaluator.heap(),
        |set, env| evaluator.process_import_for_eval_with(set, env, context),
        |e| {
            EvalError::InvalidSyntax(format!("eval: desugar error: {e}"))
                .with_diagnostic(e.diagnostic())
                .at_opt(e.source_location().cloned())
        },
    )?;
    super::lower_quasiquotes_for(&core_expr, evaluator)
}
