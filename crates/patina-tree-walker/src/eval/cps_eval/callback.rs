//! The [`ApplyContext`] a step hands a primitive: the evaluator, plus the
//! step's own dynamic environment.
//!
//! A Rust higher-order primitive — `member` with a predicate, `call-with-port`,
//! `force`, a parameter converter, `eval` — calls back into Scheme through
//! `ApplyContext::apply_proc` or `eval_expr`. The `Evaluator`'s own
//! implementation (`apply_context_impl.rs`) starts the callback's trampoline
//! with every stack empty, which is right for a call from outside any step
//! (`Backend::apply`) and wrong for one from inside: the callback then cannot
//! see the handlers, winds or prompts installed around the primitive. This
//! context carries the calling step's three stacks by reference and starts
//! the callback's trampoline under clones of them. The clone happens only
//! when a callback is actually made, so `(+ 1 2)` pays nothing for it.
//!
//! Everything else delegates to the evaluator.

use super::CpsEvaluator;
use super::types::{ExceptionHandler, PromptFrame};
use crate::eval::error::EvalError;
use patina_core::{DynamicWindRecord, Environment, FileSystem, Library, SharedHeap, TaggedValue};
use patina_primitives::ApplyContext;
use std::rc::Rc;
use std::sync::Arc;

pub(super) struct CallbackContext<'c, 'a, 's> {
    pub cps: &'c CpsEvaluator<'a>,
    pub prompt_stack: &'s [PromptFrame],
    pub dynamic_winds: &'s [DynamicWindRecord],
    pub exception_handlers: &'s [ExceptionHandler],
}

impl ApplyContext for CallbackContext<'_, '_, '_> {
    fn heap(&self) -> &SharedHeap {
        self.cps.evaluator.heap()
    }

    fn fs(&self) -> &Arc<dyn FileSystem> {
        self.cps.evaluator.fs()
    }

    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError> {
        self.cps.apply_from_direct_with(
            proc,
            args,
            self.prompt_stack.to_vec(),
            self.dynamic_winds.to_vec(),
            self.exception_handlers.to_vec(),
        )
    }

    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        use patina_frontend::Desugarer;

        let evaluator = self.cps.evaluator;
        let desugarer = Desugarer::with_env(env.clone()).with_fs(evaluator.fs().clone());
        // Bad syntax handed to the `eval` primitive is the caller's error,
        // raised while the program runs — catchable, like the VM's path.
        let core_expr = desugarer
            .desugar_tagged(expr, evaluator.heap())
            .map_err(|e| EvalError::InvalidSyntax(format!("eval: desugar error: {}", e)))?;

        super::eval_cps_with(
            &core_expr,
            env.clone(),
            evaluator,
            self.prompt_stack.to_vec(),
            self.dynamic_winds.to_vec(),
            self.exception_handlers.to_vec(),
        )
    }

    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError> {
        self.cps.evaluator.load_scheme_library(name)
    }

    fn interaction_environment(&self) -> Rc<Environment> {
        self.cps.evaluator.interaction_environment()
    }
}
