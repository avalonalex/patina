//! ApplyContext trait - abstraction for backend-specific procedure application
//!
//! Primitive procedures that need to call back into the evaluator (higher-order
//! primitives like `force`, `make-parameter`, `member` with custom comparator)
//! use this trait to remain backend-agnostic.

use crate::registry::CxHandler;
use patina_runtime::EvalError;
use patina_runtime::{Environment, FileSystem, Library, SharedHeap, TaggedValue};
use std::rc::Rc;
use std::sync::Arc;

/// Context for higher-order primitive invocations.
///
/// Implemented by each backend (tree-walker, VM) to allow shared primitive
/// implementations to call back into the evaluator when needed.
pub trait ApplyContext {
    /// Get the shared heap for value allocation
    fn heap(&self) -> &SharedHeap;

    /// Get the virtual filesystem for file I/O operations
    fn fs(&self) -> &Arc<dyn FileSystem>;

    /// Apply a procedure to arguments, returning the result.
    ///
    /// For higher-order primitives (force, member with comparator, etc.)
    /// that need to call back into the evaluator.
    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError>;

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
    /// The machines override this: each makes the context where it calls
    /// the primitive, between its safe points, and hands the argument words
    /// over as its values with no copy (`patina_core::heap::cx::enter_call`,
    /// which only a trusted crate may call). This default is for a context
    /// with no machine, an embedder's own: each word crosses as an
    /// embedder's value does, through a handle (`NoGcScope::cross_all`).
    fn call_cx(&self, handler: CxHandler, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
        patina_core::NoGcScope::cross_all(self.heap(), args, |cx, args| {
            handler(cx, args).map(patina_core::Value::tagged)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ApplyContext;
    use crate::PrimitiveRegistry;
    use patina_runtime::{Environment, EvalError, FileSystem, Library, SharedHeap, TaggedValue};
    use std::rc::Rc;
    use std::sync::Arc;

    /// A context with no machine, as an embedder's own would be: the heap
    /// and the file system alone. A primitive that takes the context runs
    /// through the default `call_cx`, each word crossing through a handle.
    struct Embedder {
        heap: SharedHeap,
        fs: Arc<dyn FileSystem>,
    }

    impl ApplyContext for Embedder {
        fn heap(&self) -> &SharedHeap {
            &self.heap
        }

        fn fs(&self) -> &Arc<dyn FileSystem> {
            &self.fs
        }

        fn apply_proc(
            &self,
            _: TaggedValue,
            _: Vec<TaggedValue>,
        ) -> Result<TaggedValue, EvalError> {
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
    }

    /// The predicates take the context (#706), and run without a machine.
    #[test]
    fn a_primitive_taking_the_context_runs_without_a_machine() {
        let heap = patina_core::new_shared_heap();
        let ctx = Embedder {
            heap: heap.clone(),
            fs: Arc::new(patina_core::NativeFs),
        };
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
