//! Backend trait for pluggable interpreter implementations
//!
//! This module defines the core `Backend` trait that allows Patina to support
//! multiple evaluation strategies (tree-walker, VM, JIT) without changing the
//! high-level interpreter API.
//!
//! # Design Philosophy
//!
//! The Backend trait abstracts over evaluation strategy while maintaining access
//! to the global environment (needed for REPL, library loading, etc.). Each backend
//! is responsible for:
//!
//! - Evaluating expressions in a given environment
//! - Managing its own internal state (stack, registers, etc.)
//! - Providing a handle on its global namespace (`global_namespace`); the
//!   environment itself is the raw layer, through `heap()` and
//!   `Heap::held_environment`

use crate::Environment;
use patina_core::{Owned, OwnedEnvironment, SourceMap, TaggedValue};
use std::cell::RefCell;
use std::rc::Rc;

/// Core trait for interpreter backend implementations
///
/// This trait abstracts over different evaluation strategies:
/// - Tree-walker: Direct AST interpretation with trampoline TCO
/// - VM: Bytecode compilation and stack-based execution
/// - JIT: Native code generation with LLVM/Cranelift
///
/// Each backend must provide, until stage 5e:
/// 1. Expression evaluation in a given environment (`eval`)
/// 2. Its global environment (`global_env`), which callers reach through
///    the handle `global_namespace` answers
/// 3. Its heap (`heap`), and a backend-specific error type
///
/// # Words
///
/// The trait's methods are inside the embedding API's promise, changing only
/// by #601's convention (#831, `docs/README.md`, "Changing the embedding
/// API"). Their words are the raw layer: they take and answer `TaggedValue`s,
/// bare words that root nothing, and environments as `&Rc<Environment>`,
/// whose methods are raw. A host calls the `Interpreter`'s methods instead,
/// which keep values in handles (#605). A caller of these keeps two rules,
/// which both machines keep:
/// - **A word passed in is live** in the backend's heap: an immediate, or an
///   object nothing has freed since it was made, as a datum just read in a
///   context is, or as a handle's word is while the handle lives.
/// - **A word answered holds until the next evaluation,** which may collect.
///   Keep it in a handle (`Heap::hold`) to keep it past that.
///
/// S1 (4b.12, #839; `PRD/GC_PRD.md` §11.5; 4b's start decision 6 on #782)
/// added handle methods beside them as provided defaults: `eval_in`,
/// `define_in`, `call_owned` and `global_namespace`, which take and answer
/// `Owned` and `OwnedEnvironment` and root what they name through the call.
/// The old methods are deprecated for callers. An implementor still
/// provides the ones the trait requires (`eval` and `global_env`, with
/// `heap`, which is not deprecated) until stage 5e removes them; the
/// provided ones it may override. S1's four names stay at 5e (question 5
/// (a) on #839, pending the owner's ruling), so an implementor migrates once
/// (owner decision, 2026-10-21, #710).
///
/// rustc does not warn an implementation of a deprecated method. It warns
/// a call of one, in an implementation too: a wrapper that delegates `eval`
/// or `global_env` to another backend, or a new method written over
/// `self.global_env()`. An implementor writes the new methods from its own
/// state, through `heap()`, the raw layer, which changes at 5e (an
/// implementor that wants nothing raw keeps the defaults), and a wrapper
/// allows `deprecated` on its delegating bodies until 5e.
pub trait Backend {
    /// Backend-specific error type
    ///
    /// Should implement `std::error::Error` and ideally wrap or convert
    /// `RuntimeError` for common semantic errors.
    type Error: std::error::Error + crate::HasDiagnostic + Send + Sync + 'static;

    /// Evaluate a TaggedValue expression in the given environment
    ///
    /// This is the core evaluation method. The backend is responsible for:
    /// - Interpreting the expression according to R7RS semantics
    /// - Managing tail call optimization
    /// - Handling special forms and procedure application
    /// - Propagating errors appropriately
    ///
    /// # Arguments
    ///
    /// - `expr`: The expression to evaluate (as a TaggedValue)
    /// - `env`: The environment in which to evaluate the expression
    ///
    /// A backend may refuse an environment it cannot run code in: the VM
    /// runs code only in a namespace, and answers an error naming the kind
    /// for a frame, a raw one or one under a host namespace (#835); the
    /// tree-walker evaluates a frame by name.
    ///
    /// # Returns
    ///
    /// The resulting TaggedValue, or a backend-specific error.
    #[deprecated(
        note = "use `eval_in`, which takes a handle on the namespace and on the datum and answers \
                a handle; an implementor still provides this until stage 5e, and writes an \
                override of `eval_in` from its own state, not by calling this (#839)"
    )]
    fn eval(&self, expr: TaggedValue, env: &Rc<Environment>) -> Result<TaggedValue, Self::Error>;

    /// The global environment: the namespace top-level forms, library
    /// exports' imports and introspection use.
    ///
    /// Deprecated for callers, and required of implementors until stage 5e
    /// (#839): a caller takes a handle from [`Self::global_namespace`], and
    /// reads the environment, raw, with `Heap::held_environment` on it.
    #[deprecated(
        note = "use `global_namespace()` for a handle, and `Heap::held_environment` on it (through \
                `heap()`, the raw layer) for the environment; an implementor still provides this \
                until stage 5e, and writes an override of `global_namespace` from its own state, \
                not by calling this (#839)"
    )]
    fn global_env(&self) -> &Rc<Environment>;

    /// The heap the backend's values live on, and its environments'. An
    /// environment keeps no handle to its heap (#704), so a host reaches
    /// the heap here.
    fn heap(&self) -> &crate::heap::SharedHeap;

    /// Evaluate an expression in the global environment (convenience method)
    ///
    /// This is equivalent to `self.eval(expr, self.global_env())` but provided
    /// as a convenience for the common case of top-level evaluation.
    #[deprecated(note = "use `eval_in` with `global_namespace()` (#839)")]
    #[allow(
        deprecated,
        reason = "the default reaches the old methods until stage 5e (#839)"
    )]
    #[expect(
        clippy::disallowed_methods,
        reason = "holds `global`, a clone of the backend's global environment, which the backend \
                  roots: nothing the evaluation could free"
    )]
    fn eval_global(&self, expr: TaggedValue) -> Result<TaggedValue, Self::Error> {
        let global = self.global_env().clone();
        self.eval(expr, &global)
    }

    /// Evaluate an expression the parser read into `source_map`, so that what
    /// the evaluation reports carries the positions recorded there.
    ///
    /// Every caller that reads a program with positions — a file, standard
    /// input, a session — evaluates through this, which is what lets that
    /// reading be written once for all backends. A backend that does not
    /// track positions evaluates the expression as `eval` does. Either way
    /// it refuses the environments `eval` refuses: the VM a frame (#835).
    #[expect(
        clippy::disallowed_methods,
        reason = "holds nothing it reads as a value: the source map is keyed by raw bits it \
                  never dereferences and owns no Scheme value, so a collection can only leave it \
                  stale keys (docs/GC_DESIGN.md §9.1), which misattribute a diagnostic and free \
                  nothing"
    )]
    #[deprecated(note = "use `eval_in` with `Some(source_map)` (#839)")]
    #[allow(
        deprecated,
        reason = "the default reaches the old method until stage 5e (#839)"
    )]
    fn eval_with_source_map(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
        source_map: &Rc<RefCell<SourceMap>>,
    ) -> Result<TaggedValue, Self::Error> {
        let _ = source_map;
        self.eval(expr, env)
    }

    /// Bind `name` to `value` in `env`, as a program's `define` there does,
    /// so that code compiled there before the definition calls the new
    /// value as well.
    ///
    /// The default writes the environment's binding, which is all a backend
    /// needs that looks a name up at each call, as the tree-walker does. The
    /// VM compiles a call to a primitive's binding into a fast path, and
    /// marks the primitive before it rebinds the name, as its `Define`
    /// instruction does, so that the code compiled before stops taking the
    /// fast path (#673). A host defines through this, by way of
    /// `Interpreter::define` and `Interpreter::define_in`;
    /// `Environment::define` and `Environment::set` are the raw layer, which
    /// write the binding and nothing else.
    #[deprecated(
        note = "use `define_in`, which takes handles and marks a rebound primitive as this does \
                (#673, #839)"
    )]
    fn define(&self, env: &Rc<Environment>, name: &str, value: TaggedValue) {
        patina_core::NamespaceView::of(env).define(self.heap(), name, value);
    }

    /// Apply the procedure `f` to `args` as a top-level form: a driver
    /// entry, as [`Self::eval`] is, which may collect while the callee runs,
    /// and in which a continuation captured behaves as one captured in a form
    /// does (#710). The words are a host's handles' (`Interpreter::call`),
    /// which root them through the call.
    ///
    /// The default evaluates `((quote f) (quote arg) …)` in the global
    /// environment, so it depends on what `quote` is bound to there. Both
    /// machines apply without consulting a binding: they build the
    /// application as `CoreExpr`, not as a datum to expand.
    #[deprecated(note = "use `call_owned`, which takes and answers handles (#839)")]
    #[allow(
        deprecated,
        reason = "the default reaches the old method until stage 5e (#839)"
    )]
    fn call(&self, f: TaggedValue, args: &[TaggedValue]) -> Result<TaggedValue, Self::Error> {
        let mut words = Vec::with_capacity(args.len() + 1);
        words.push(f);
        words.extend_from_slice(args);
        let form = patina_core::NoGcScope::cross_all(self.heap(), &words, |cx, values| {
            let quote = cx.intern_symbol("quote");
            let quoted: Vec<_> = values
                .iter()
                .map(|&v| {
                    let datum = cx.alloc_pair(v, patina_core::Value::NULL);
                    cx.alloc_pair(quote, datum)
                })
                .collect();
            cx.list_from_iter(quoted).tagged()
        });
        #[expect(
            clippy::disallowed_methods,
            reason = "the call's own entry, outermost, so the backend's run may collect: holds the \
                      form, not read after the call, whose words the host's handles root"
        )]
        let value = self.eval_global(form);
        value
    }

    /// Evaluate the datum `expr` holds in the namespace `env` holds, and
    /// answer a handle on its value: [`Self::eval`] over handles. A driver
    /// entry: it may collect, with the datum and the namespace rooted by the
    /// caller's handles through the call. `source_map` is the map the datum
    /// was read into, when it was, so that what the evaluation reports
    /// carries its positions, as [`Self::eval_with_source_map`] does.
    ///
    /// It refuses what `eval` refuses: on the VM, a frame a deprecated holder
    /// held (#835). Panics on a handle from another heap.
    ///
    /// An implementor that overrides this writes it from its own state, not
    /// by calling the deprecated methods, which warn there as anywhere;
    /// reading and making handles goes through `heap()`, the raw layer, which
    /// changes at stage 5e. An implementor that wants nothing raw keeps this
    /// default. The parameter list is question 1 (a) on #839, pending the
    /// owner's ruling.
    #[allow(
        deprecated,
        reason = "the default reaches the old methods until stage 5e (#839)"
    )]
    fn eval_in(
        &self,
        env: &OwnedEnvironment,
        expr: &Owned,
        source_map: Option<&Rc<RefCell<SourceMap>>>,
    ) -> Result<Owned, Self::Error> {
        let (env, word) = {
            let heap = self.heap().borrow();
            (heap.held_environment(env), heap.held(expr))
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "holds `env`, an `Rc` of the namespace the caller's handle roots through the \
                      call, and the datum's word, which the caller's handle roots; the answer is \
                      held before the next evaluation, by `Backend`'s second rule"
        )]
        let value = match source_map {
            Some(map) => self.eval_with_source_map(word, &env, map),
            None => self.eval(word, &env),
        };
        let value = value?;
        Ok(self.heap().borrow().hold(value))
    }

    /// Bind `name` to the value `value` holds in the namespace `env` holds,
    /// as a program's `define` there does: [`Self::define`] over handles, so
    /// the VM marks the primitive it rebinds (#673). Evaluates nothing.
    /// Panics on a handle from another heap.
    #[allow(
        deprecated,
        reason = "the default reaches the old methods until stage 5e (#839)"
    )]
    fn define_in(&self, env: &OwnedEnvironment, name: &str, value: &Owned) {
        let (env, value) = {
            let heap = self.heap().borrow();
            (heap.held_environment(env), heap.held(value))
        };
        self.define(&env, name, value);
    }

    /// Apply the procedure `f` holds to the values `args` hold as a
    /// top-level form, and answer a handle on its value: [`Self::call`] over
    /// handles. A driver entry, as `eval_in` is; the handles root the callee
    /// and its arguments through the call. Panics on a handle from another
    /// heap.
    #[allow(
        deprecated,
        reason = "the default reaches the old methods until stage 5e (#839)"
    )]
    fn call_owned(&self, f: &Owned, args: &[Owned]) -> Result<Owned, Self::Error> {
        let (callee, operands) = {
            let heap = self.heap().borrow();
            (
                heap.held(f),
                args.iter().map(|arg| heap.held(arg)).collect::<Vec<_>>(),
            )
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "holds the callee's and the arguments' words, not read after the call: the \
                      caller's handles root them through it; the answer is held before the next \
                      evaluation, by `Backend`'s second rule"
        )]
        let value = self.call(callee, &operands);
        let value = value?;
        Ok(self.heap().borrow().hold(value))
    }

    /// A handle on the backend's global namespace, which roots it until the
    /// handle is dropped: what `eval_in` and `define_in` take for the global
    /// environment. A fresh handle each call.
    ///
    /// Until stage 5e, a host backend whose global environment is a frame
    /// (made with the deprecated `Environment::with_parent`) gets a handle on
    /// the frame, through the deprecated holder; from 5e, when the deprecated
    /// holders go, the handle holds a namespace, as `OwnedEnvironment`'s own
    /// documentation says (question 4 (b) on #839, pending the owner's
    /// ruling). An implementor that overrides this writes it from its own
    /// state (`heap().borrow().hold_namespace(..)` of its global), not over
    /// `global_env()`, which warns there as anywhere. Panics on a global of
    /// another heap.
    #[allow(
        deprecated,
        reason = "the default reaches the old methods until stage 5e (#839)"
    )]
    fn global_namespace(&self) -> OwnedEnvironment {
        let global = Rc::clone(self.global_env());
        self.heap().borrow().hold_global(global)
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "unit tests of the trait's default methods against a mock backend that evaluates \
              nothing"
)]
#[allow(
    deprecated,
    reason = "the old methods' defaults are tested, and the mocks implement and delegate to them, \
              until stage 5e (#839)"
)]
mod tests {
    use super::*;
    use crate::error::RuntimeError;

    // Mock backend for testing
    struct MockBackend {
        global: Rc<Environment>,
        heap: crate::heap::SharedHeap,
    }

    impl MockBackend {
        fn new() -> Self {
            let heap = crate::heap::new_shared_heap();
            Self {
                global: Rc::new(Environment::with_heap(&heap)),
                heap,
            }
        }
    }

    impl Backend for MockBackend {
        type Error = RuntimeError;

        fn eval(
            &self,
            expr: TaggedValue,
            _env: &Rc<Environment>,
        ) -> Result<TaggedValue, Self::Error> {
            // Just return the expression (identity backend)
            Ok(expr)
        }

        fn global_env(&self) -> &Rc<Environment> {
            &self.global
        }

        fn heap(&self) -> &crate::heap::SharedHeap {
            &self.heap
        }
    }

    #[test]
    fn test_backend_trait() {
        let backend = MockBackend::new();

        let expr = TaggedValue::fixnum(42);
        let result = backend.eval_global(expr).unwrap();

        assert_eq!(result.as_fixnum(), Some(42));
    }

    /// A backend that does not track positions evaluates with a source map as
    /// it does without one.
    #[test]
    fn eval_with_source_map_defaults_to_eval() {
        let backend = MockBackend::new();
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        let global = backend.global_env().clone();
        let result = backend
            .eval_with_source_map(TaggedValue::fixnum(7), &global, &source_map)
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(7));
    }

    /// A backend that compiles nothing defines by writing the environment.
    #[test]
    fn define_defaults_to_the_environment() {
        let backend = MockBackend::new();
        let global = backend.global_env().clone();
        backend.define(&global, "answer", TaggedValue::fixnum(42));
        assert_eq!(
            global.get(backend.heap(), "answer"),
            Some(TaggedValue::fixnum(42))
        );
    }

    #[test]
    fn test_backend_eval_with_env() {
        let backend = MockBackend::new();

        let custom_env = Rc::new(Environment::with_heap(backend.heap()));
        let expr = TaggedValue::TRUE;
        let result = backend.eval(expr, &custom_env).unwrap();

        assert_eq!(result, TaggedValue::TRUE);
    }

    /// A backend over `MockBackend` that overrides the old provided methods
    /// to note which one a handle method's default went through, and then
    /// does what the default does (#839).
    struct Recorder {
        inner: MockBackend,
        calls: RefCell<Vec<String>>,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                inner: MockBackend::new(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Backend for Recorder {
        type Error = RuntimeError;

        fn eval(
            &self,
            expr: TaggedValue,
            env: &Rc<Environment>,
        ) -> Result<TaggedValue, Self::Error> {
            self.calls.borrow_mut().push("eval".to_string());
            self.inner.eval(expr, env)
        }

        fn global_env(&self) -> &Rc<Environment> {
            self.inner.global_env()
        }

        fn heap(&self) -> &crate::heap::SharedHeap {
            self.inner.heap()
        }

        fn eval_with_source_map(
            &self,
            expr: TaggedValue,
            env: &Rc<Environment>,
            source_map: &Rc<RefCell<SourceMap>>,
        ) -> Result<TaggedValue, Self::Error> {
            self.calls
                .borrow_mut()
                .push("eval_with_source_map".to_string());
            self.inner.eval_with_source_map(expr, env, source_map)
        }

        fn define(&self, env: &Rc<Environment>, name: &str, value: TaggedValue) {
            self.calls.borrow_mut().push(format!("define {name}"));
            self.inner.define(env, name, value);
        }

        fn call(&self, f: TaggedValue, args: &[TaggedValue]) -> Result<TaggedValue, Self::Error> {
            self.calls.borrow_mut().push(format!("call {}", args.len()));
            Ok(f)
        }
    }

    /// `eval_in`'s default evaluates the held datum in the held namespace
    /// through `eval`, or through `eval_with_source_map` when it is given a
    /// map, and answers a handle on the value.
    #[test]
    fn eval_in_defaults_to_eval() {
        let backend = MockBackend::new();
        let ns = backend.global_namespace();
        let expr = backend.heap().borrow().hold(TaggedValue::fixnum(7));
        let before = backend.heap().borrow().handle_count();
        let value = backend.eval_in(&ns, &expr, None).unwrap();
        assert_eq!(backend.heap().borrow().held(&value), TaggedValue::fixnum(7));
        assert_eq!(backend.heap().borrow().handle_count(), before + 1);
        drop(value);
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        let value = backend.eval_in(&ns, &expr, Some(&source_map)).unwrap();
        assert_eq!(backend.heap().borrow().held(&value), TaggedValue::fixnum(7));
        drop(value);
        assert_eq!(backend.heap().borrow().handle_count(), before);

        let recorder = Recorder::new();
        let ns = recorder.global_namespace();
        let expr = recorder.heap().borrow().hold(TaggedValue::fixnum(7));
        recorder.eval_in(&ns, &expr, None).unwrap();
        assert_eq!(recorder.calls(), ["eval"]);
        let recorder = Recorder::new();
        let ns = recorder.global_namespace();
        let expr = recorder.heap().borrow().hold(TaggedValue::fixnum(7));
        recorder.eval_in(&ns, &expr, Some(&source_map)).unwrap();
        assert_eq!(recorder.calls(), ["eval_with_source_map"]);
    }

    /// `define_in`'s default binds in the held namespace through `define`,
    /// so a backend that overrides `define` keeps its meaning.
    #[test]
    fn define_in_defaults_to_define() {
        let backend = MockBackend::new();
        let ns = backend.global_namespace();
        let value = backend.heap().borrow().hold(TaggedValue::fixnum(9));
        backend.define_in(&ns, "x", &value);
        assert_eq!(
            patina_core::NamespaceView::of(backend.global_env()).get(backend.heap(), "x"),
            Some(TaggedValue::fixnum(9))
        );

        let recorder = Recorder::new();
        let ns = recorder.global_namespace();
        let value = recorder.heap().borrow().hold(TaggedValue::fixnum(9));
        recorder.define_in(&ns, "x", &value);
        assert_eq!(recorder.calls(), ["define x"]);
    }

    /// `call_owned`'s default goes through `call`, which builds the quoted
    /// application and evaluates it in the global environment.
    #[test]
    fn call_owned_defaults_to_call() {
        let backend = MockBackend::new();
        let (f, args) = {
            let heap = backend.heap().borrow();
            (
                heap.hold(TaggedValue::fixnum(5)),
                [
                    heap.hold(TaggedValue::fixnum(1)),
                    heap.hold(TaggedValue::fixnum(2)),
                ],
            )
        };
        let form = backend.call_owned(&f, &args).unwrap();
        let heap = backend.heap().borrow();
        assert_eq!(
            patina_core::format_tagged(heap.held(&form), &heap),
            "((quote 5) (quote 1) (quote 2))"
        );
        drop(heap);

        let recorder = Recorder::new();
        let (f, args) = {
            let heap = recorder.heap().borrow();
            (
                heap.hold(TaggedValue::fixnum(5)),
                [
                    heap.hold(TaggedValue::fixnum(1)),
                    heap.hold(TaggedValue::fixnum(2)),
                ],
            )
        };
        let value = recorder.call_owned(&f, &args).unwrap();
        assert_eq!(
            recorder.heap().borrow().held(&value),
            TaggedValue::fixnum(5)
        );
        assert_eq!(recorder.calls(), ["call 2"]);
    }

    /// `global_namespace`'s default holds the backend's own global
    /// environment, in a fresh handle each call, which roots it until it is
    /// dropped.
    #[test]
    fn global_namespace_holds_the_global_environment() {
        let backend = MockBackend::new();
        let before = backend.heap().borrow().handle_count();
        let ns = backend.global_namespace();
        let again = backend.global_namespace();
        assert_eq!(backend.heap().borrow().handle_count(), before + 2);
        assert!(Rc::ptr_eq(
            &backend.heap().borrow().held_environment(&ns),
            backend.global_env()
        ));
        assert!(Rc::ptr_eq(
            &backend.heap().borrow().held_environment(&again),
            backend.global_env()
        ));
        drop((ns, again));
        assert_eq!(backend.heap().borrow().handle_count(), before);
    }

    /// Until stage 5e a backend whose global environment is a frame gets a
    /// handle on the frame, through the deprecated holder (question 4 (b) on
    /// #839, pending the owner's ruling; under (a) this panics instead).
    #[test]
    fn global_namespace_holds_a_frame_global_until_5e() {
        let mut backend = MockBackend::new();
        let frame = Rc::new(Environment::child_frame(Rc::clone(&backend.global)));
        backend.global = frame;
        let ns = backend.global_namespace();
        assert!(Rc::ptr_eq(
            &backend.heap().borrow().held_environment(&ns),
            backend.global_env()
        ));
    }
}
