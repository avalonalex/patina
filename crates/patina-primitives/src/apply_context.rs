//! ApplyContext trait - abstraction for backend-specific procedure application
//!
//! A primitive reaches the heap through its context (`Cx`, #706). This trait
//! is what the machine running it provides besides: the context itself, made
//! where the primitive is called, and the calls back into the evaluator that
//! higher-order and resumable primitives make (`force`, `member` with a custom
//! comparator, `load`'s interaction environment).

use crate::registry::{CxHandler, StepFn, TaggedStep};
use patina_core::{Cx, Value};
use patina_runtime::EvalError;
use patina_runtime::{Environment, Library, TaggedValue};
use std::rc::Rc;

/// What a machine provides a primitive besides its context.
///
/// Implemented by each backend (tree-walker, VM). It hands out no heap: a
/// primitive reads and allocates through the context the machine makes for
/// it ([`Self::call_cx`], [`Self::step_cx`]), and reaches the file system and
/// the command line through the context's instance (#706).
pub trait ApplyContext {
    /// Apply a procedure to arguments, returning the result.
    ///
    /// For higher-order primitives (force, member with comparator, etc.)
    /// that need to call back into the evaluator.
    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError>;

    /// Call `proc` on `args` from a higher-order primitive's context, as
    /// [`Self::apply_proc`] does, and answer its value in that context
    /// (#706). What the primitive holds across the call stays live: the call
    /// runs on a nested loop, which cannot collect. A machine brands the word
    /// the call answers.
    fn apply_in<'gc>(
        &self,
        cx: &mut Cx<'gc>,
        proc: Value<'gc>,
        args: &[Value<'gc>],
    ) -> Result<Value<'gc>, EvalError>;

    /// Evaluate a datum expression in the given environment.
    ///
    /// Used by `eval` and `interaction-environment` primitives.
    fn eval_expr(&self, expr: TaggedValue, env: &Rc<Environment>)
    -> Result<TaggedValue, EvalError>;

    /// Load a Scheme library by name.
    ///
    /// Used by `environment`, `null-environment`, `scheme-report-environment`.
    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError>;

    /// Get the interaction (global mutable) environment.
    ///
    /// Used by `load` and `interaction-environment`.
    fn interaction_environment(&self) -> Rc<Environment>;

    /// Call `handler`, a primitive that takes the context, on `args` (#706).
    ///
    /// The machine makes the context where it calls the primitive, between
    /// its safe points, and hands the argument words over as its values with
    /// no copy (`patina_core::heap::cx::enter_call`, which only a trusted
    /// crate may call).
    fn call_cx(&self, handler: CxHandler, args: &[TaggedValue]) -> Result<TaggedValue, EvalError>;

    /// Run `step`, one half of a resumable primitive or a higher-order
    /// primitive, in a context over `args` (#706): how the registry runs
    /// either. Its step leaves the context as words
    /// ([`Step::tagged`](crate::Step::tagged)); a higher-order primitive's is
    /// done with its value. The machine makes the context as it does for
    /// [`Self::call_cx`].
    fn step_cx(&self, args: &[TaggedValue], step: StepFn<'_>) -> Result<TaggedStep, EvalError>;
}

#[cfg(test)]
mod tests {
    use super::ApplyContext;
    use crate::PrimitiveRegistry;
    use crate::registry::{CxHandler, StepFn, TaggedStep};
    use patina_core::{Cx, NoGcScope, SharedHeap, Value};
    use patina_runtime::{Environment, EvalError, Library, TaggedValue};
    use std::rc::Rc;

    /// A context with no machine, as an embedder's own would be: it makes
    /// each context beneath a `NoGcScope`, each word crossing through a
    /// handle (`NoGcScope::cross_all`).
    struct Embedder {
        heap: SharedHeap,
    }

    impl ApplyContext for Embedder {
        fn apply_proc(
            &self,
            _: TaggedValue,
            _: Vec<TaggedValue>,
        ) -> Result<TaggedValue, EvalError> {
            unreachable!("no primitive here calls back")
        }

        fn apply_in<'gc>(
            &self,
            _: &mut Cx<'gc>,
            _: Value<'gc>,
            _: &[Value<'gc>],
        ) -> Result<Value<'gc>, EvalError> {
            unreachable!("no primitive here calls back")
        }

        fn eval_expr(&self, _: TaggedValue, _: &Rc<Environment>) -> Result<TaggedValue, EvalError> {
            unreachable!("no primitive here evaluates")
        }

        fn load_scheme_library(&self, _: &[String]) -> Result<Rc<Library>, EvalError> {
            unreachable!("no primitive here loads")
        }

        fn interaction_environment(&self) -> Rc<Environment> {
            unreachable!("no primitive here asks")
        }

        fn call_cx(
            &self,
            handler: CxHandler,
            args: &[TaggedValue],
        ) -> Result<TaggedValue, EvalError> {
            NoGcScope::cross_all(&self.heap, args, |cx, args| {
                handler(cx, args).map(Value::tagged)
            })
        }

        fn step_cx(&self, args: &[TaggedValue], step: StepFn<'_>) -> Result<TaggedStep, EvalError> {
            NoGcScope::cross_all(&self.heap, args, |cx, args| step(cx, args))
        }
    }

    /// The predicates take the context (#706), and run without a machine.
    #[test]
    fn a_primitive_taking_the_context_runs_without_a_machine() {
        let heap = patina_core::new_shared_heap();
        let ctx = Embedder { heap: heap.clone() };
        let mut registry = PrimitiveRegistry::new();
        crate::register_all(&mut registry);
        let apply = |name: &str, args: &[TaggedValue]| {
            registry.apply_tagged(&format!("scheme.base/{name}"), args, &ctx)
        };
        let pair = heap
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        assert_eq!(apply("pair?", &[pair]).unwrap(), TaggedValue::TRUE);
        assert_eq!(apply("list?", &[pair]).unwrap(), TaggedValue::TRUE);
        assert_eq!(apply("number?", &[pair]).unwrap(), TaggedValue::FALSE);
        assert_eq!(
            apply("boolean=?", &[TaggedValue::TRUE, TaggedValue::TRUE]).unwrap(),
            TaggedValue::TRUE
        );
        assert!(matches!(
            apply("boolean=?", &[TaggedValue::TRUE, pair]),
            Err(EvalError::TypeError(_))
        ));
    }
}
