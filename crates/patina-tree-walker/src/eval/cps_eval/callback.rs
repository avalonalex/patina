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
    pub(crate) fn eval_core(
        &self,
        expr: &patina_core::CoreExpr,
        literals: patina_core::LiteralPool,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        if let patina_core::CoreExprKind::Import { .. } = &expr.kind {
            #[expect(
                clippy::disallowed_methods,
                reason = "holds nothing of its own: `expr` is the caller's, and `eval_import` gives \
                          the reason for what it holds across each load"
            )]
            return self.eval_import(expr, &literals, env);
        }
        let expr = super::lower_quasiquotes_for(expr, self.cps.evaluator, &literals)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "holds the lowered `expr`, not read after the call: the run roots the CPS tree \
                      it is entered with, and copies of the caller's stacks move into its first \
                      step"
        )]
        let value = super::eval_cps_with(
            &expr,
            literals,
            env.clone(),
            self.cps.evaluator,
            self.prompt_stack.to_vec(),
            self.dynamic_winds.to_vec(),
            self.exception_handlers.to_vec(),
        );
        unhandled_is_final(value)
    }

    pub(crate) fn eval_import(
        &self,
        expr: &patina_core::CoreExpr,
        literals: &patina_core::LiteralPool,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        let patina_core::CoreExprKind::Import { import_sets } = &expr.kind else {
            unreachable!("eval_import requires an import")
        };
        // Every set is read through the pool, which roots it (#705), and
        // before any loads.
        let import_sets = import_sets
            .iter()
            .map(|&set| {
                patina_core::NoGcScope::with_cx(self.heap(), |cx| {
                    let set = literals.value(cx, set);
                    patina_frontend::LibraryDefinition::parse_import_set_tagged(set, cx)
                })
                .map_err(|e| {
                    EvalError::InvalidSyntax(format!("Invalid import set: {e}"))
                        .with_diagnostic(e.diagnostic())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for import_set in &import_sets {
            #[expect(
                clippy::disallowed_methods,
                reason = "holds the parsed import sets, which hold no value, and `env`, the \
                          caller's: the load may collect (#677)"
            )]
            self.cps
                .evaluator
                .process_import_for_eval_with(import_set, env, self)?;
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

    /// The evaluator's own: a step's call, inside the step (#706).
    fn call_cx(
        &self,
        handler: patina_primitives::CxHandler,
        args: &[TaggedValue],
    ) -> Result<TaggedValue, EvalError> {
        self.cps.evaluator.call_cx(handler, args)
    }

    /// A higher-order primitive's call back (#706): this context's
    /// [`Self::apply_proc`], with its value branded in the primitive's
    /// context.
    fn apply_in<'gc>(
        &self,
        _cx: &mut patina_core::Cx<'gc>,
        proc: patina_core::Value<'gc>,
        args: &[patina_core::Value<'gc>],
    ) -> Result<patina_core::Value<'gc>, EvalError> {
        let args = patina_core::Value::tagged_slice(args).to_vec();
        #[expect(
            clippy::disallowed_methods,
            reason = "the call back `apply_in` is for, on a nested trampoline that defers, as \
                      `apply_proc`'s own reason says; each of its callers gives what it holds \
                      across it, at its call"
        )]
        let result = self.apply_proc(proc.tagged(), args)?;
        // SAFETY: the call ran on a nested trampoline, which cannot collect,
        // and the word it answered stays live until the primitive's context,
        // whose brand `'gc` is, closes.
        Ok(unsafe { patina_core::Value::from_tagged(result) })
    }

    /// The evaluator's own: a step's resumable primitive, inside the step
    /// (#706).
    fn step_cx(
        &self,
        args: &[TaggedValue],
        step: patina_primitives::StepFn<'_>,
    ) -> Result<patina_primitives::TaggedStep, EvalError> {
        self.cps.evaluator.step_cx(args, step)
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

    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        let evaluator = self.cps.evaluator;
        #[expect(
            clippy::disallowed_methods,
            reason = "holds nothing of its own: the datum moves into the expansion, whose imports \
                      load beneath the context the expansion runs in"
        )]
        let (core_expr, literals) = expand_for_eval(evaluator, expr, env, self)?;

        #[expect(
            clippy::disallowed_methods,
            reason = "holds `core_expr`, the expanded datum, not read after the call: the run \
                      roots the CPS tree it is entered with"
        )]
        let value = self.eval_core(&core_expr, literals, env);
        value
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "holds nothing: the library comes back by value. A primitive that called back \
                  holds its values across the load, which runs nested under the calling step's \
                  trampoline, so it collects nowhere (#677)"
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
) -> Result<(patina_core::CoreExpr, patina_core::LiteralPool), EvalError> {
    let desugarer =
        patina_frontend::Desugarer::with_env(env.clone()).with_fs(evaluator.fs().clone());
    // The datum's literals, rooted until the CPS tree holds them (#705).
    let literals = patina_core::LiteralPool::new(&evaluator.heap().borrow());
    #[expect(
        clippy::disallowed_methods,
        reason = "the import callback loads libraries during the expansion, which holds the datum \
                  and the partial expansion: values of the context the expansion runs in \
                  (`NoGcScope::cross`), beneath which nothing collects"
    )]
    let core_expr = patina_core::NoGcScope::cross(evaluator.heap(), [expr], |cx, [form]| {
        desugarer.desugar_with_imports(
            form,
            cx,
            &literals,
            |set, env| evaluator.process_import_for_eval_with(set, env, context),
            |e| {
                EvalError::InvalidSyntax(format!("eval: desugar error: {e}"))
                    .with_diagnostic(e.diagnostic())
                    .at_opt(e.source_location().cloned())
            },
        )
    })?;
    let lowered = super::lower_quasiquotes_for(&core_expr, evaluator, &literals)?;
    Ok((lowered, literals))
}
