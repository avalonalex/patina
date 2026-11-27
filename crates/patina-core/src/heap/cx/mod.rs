//! `Cx<'gc>`: the mutation and allocation context (`PRD/GC_PRD.md` §11.3,
//! #703).
//!
//! **No method on `Cx` can collect.** A `Cx` is made only where no
//! collection can run while it lives:
//! - by the carrier's [`GcDriver`](crate::GcDriver), through
//!   [`GcDriver::mutate`](crate::GcDriver::mutate), whose `&mut` borrow keeps
//!   the driver, the one capability that collects, from being used while the
//!   context lives: the borrow checker refuses it;
//! - beneath a [`NoGcScope`](crate::NoGcScope), through
//!   [`NoGcScope::mutate`](crate::NoGcScope::mutate), where the depth keeps
//!   every collection posted until the scope closes.
//!
//! Debug and `gc-check` builds also count live contexts on the carrier and
//! assert at every collection that none is live.
//!
//! **The brand.** Each context's `'gc` is the lifetime of one `mutate` call,
//! chosen by a higher-ranked closure, as gc-arena's arenas choose theirs. The
//! [`Value`]s a context hands out carry it, so they cannot leave the call
//! (the closure's result cannot name `'gc`) nor reach another context.
//!
//! **Borrowing.** Reads take `&Cx`, mutation and allocation `&mut Cx` (until
//! stage 5e). Until 5e the heap is still behind its `RefCell`, which a context
//! borrows for each operation, as today's code does (DESIGN E.5). So a bulk
//! read, a [`Ref`] tied to the `&Cx` borrow, cannot be held across an
//! allocation, which needs `&mut`: the borrow checker refuses what was a
//! `BorrowMutError` at run time. No method returns `&BigInt`, `&mut [char]`
//! or `&mut [TaggedValue]` into the heap. [`Cx::get_object`] hands the datum
//! writer an object to match on, whose changing payloads it cannot borrow
//! (#708).
//!
//! **Coverage.** The methods are what `patina-frontend`, `patina-macros`,
//! `patina-primitives`, `patina-runtime` and the hosts call on `Heap` today,
//! under the same names, with values for words: the surface #704, #706 and
//! #707 move them onto. A method keeps its `Heap` name where its shape
//! holds, and changes its return type where `PRD/GC_PRD.md` forbids the old
//! one: a bignum is answered owned, a slice as a `Ref`, a record's,
//! parameter's or promise's contents as values rather than the `Rc` payload
//! that let a store bypass the funnel ([`funnel`]).
//!
//! ```
//! use patina_core::{GcDriver, Instance, Value, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let mut driver = GcDriver::acquire(&heap).expect("the carrier's driver is free");
//! let sum = driver.mutate(&instance, |cx| {
//!     let list = cx.list_from_iter([Value::fixnum(1), Value::fixnum(2)]);
//!     let two = cx.car(cx.cdr(list));
//!     two.as_fixnum().unwrap() + 1
//! });
//! assert_eq!(sum, 3);
//! ```
//!
//! The cases below pin the brand (#709): each fails to compile, beside a twin
//! that compiles once the violation is removed.
//!
//! A value cannot leave the call that made it, though what it says can:
//!
//! ```
//! use patina_core::{GcDriver, Instance, Value, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let mut driver = GcDriver::acquire(&heap).unwrap();
//! let car = driver.mutate(&instance, |cx| {
//!     let pair = cx.alloc_pair(Value::fixnum(1), Value::NULL);
//!     cx.car(pair).as_fixnum()
//! });
//! assert_eq!(car, Some(1));
//! ```
//!
//! The value leaving it, an error with no code to pin:
//!
//! ```compile_fail
//! use patina_core::{GcDriver, Instance, Value, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let mut driver = GcDriver::acquire(&heap).unwrap();
//! let pair = driver.mutate(&instance, |cx| cx.alloc_pair(Value::NULL, Value::NULL));
//! ```
//!
//! Nor can a value be used after a call that may collect: while a context
//! lives, the driver it borrows cannot reach a safe point. Once the context
//! has closed, the driver can:
//!
//! ```
//! use patina_core::{GcController, GcDriver, GcMode, Instance, Value, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let gc = std::cell::RefCell::new(GcController::new(GcMode::Off));
//! let mut driver = GcDriver::acquire(&heap).unwrap();
//! let car = driver.mutate(&instance, |cx| {
//!     let pair = cx.alloc_pair(Value::fixnum(1), Value::NULL);
//!     cx.car(pair).as_fixnum()
//! });
//! GcController::safe_point(&gc, &mut driver, |collect| collect(&[]));
//! assert_eq!(car, Some(1));
//! ```
//!
//! The safe point inside the context:
//!
//! ```compile_fail,E0499
//! use patina_core::{GcController, GcDriver, GcMode, Instance, Value, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let gc = std::cell::RefCell::new(GcController::new(GcMode::Off));
//! let mut driver = GcDriver::acquire(&heap).unwrap();
//! driver.mutate(&instance, |cx| {
//!     let pair = cx.alloc_pair(Value::NULL, Value::NULL);
//!     GcController::safe_point(&gc, &mut driver, |collect| collect(&[]));
//!     cx.car(pair)
//! });
//! ```
//!
//! A slice is a borrow of the context, so it cannot be held across an
//! allocation. Read out of it first:
//!
//! ```
//! use patina_core::{GcDriver, Instance, Value, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let mut driver = GcDriver::acquire(&heap).unwrap();
//! let car = driver.mutate(&instance, |cx| {
//!     let vector = cx.alloc_vector_fill(2, Value::fixnum(7));
//!     let first = cx.vector_slice(vector)[0];
//!     let pair = cx.alloc_pair(first, Value::NULL);
//!     cx.car(pair).as_fixnum()
//! });
//! assert_eq!(car, Some(7));
//! ```
//!
//! The slice held across the allocation:
//!
//! ```compile_fail,E0502
//! use patina_core::{GcDriver, Instance, Value, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let mut driver = GcDriver::acquire(&heap).unwrap();
//! driver.mutate(&instance, |cx| {
//!     let vector = cx.alloc_vector_fill(2, Value::fixnum(7));
//!     let elements = cx.vector_slice(vector);
//!     let pair = cx.alloc_pair(elements[0], Value::NULL);
//!     elements.len()
//! });
//! ```

use std::cell::{Ref, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use num_bigint::BigInt;
use num_rational::BigRational;

use super::mutator::Mutator;
use super::scope_sets::ScopeSetMemo;
use super::{Heap, HeapObjectData, PromiseState};
use crate::compiled_macro::{CompiledMacro, LiteralId};
use crate::environment::{BindingLocation, Environment, IntroducedDefinition, RelinkTarget};
use crate::error::{ExceptionKind, SourceLocation};
use crate::instance::Instance;
use crate::library::Library;
use crate::literal_pool::{Link, LinkKind, LiteralPool};
use crate::namespace_view::NamespaceView;
use crate::port::Port;
use crate::record_type::RecordTypeDescriptor;
use crate::scope::{ScopeId, ScopeSet};
use crate::scope_resolve::AmbiguousReference;
use crate::tagged_value::TaggedValue;
use crate::value::Value;
use crate::{RootProvider, RootToken};

pub mod funnel;
mod methods;

pub use funnel::{
    CellValue, ClosureFree, ConvertedParamValue, Fresh, MachinePromiseBox, MachineVectorElem,
    PairCar, PairCdr, ParamValue, PromiseBox, RecordField, Slot, VectorElem,
};

/// The mutation and allocation context. See the [module docs](self).
///
/// **In the embedding API's promise** (#831, `docs/README.md`, "Changing the
/// embedding API"): a host primitive is written against it, through
/// `Interpreter::with` and `Prim`. One rule sorts its methods. A method is
/// inside, changing only by #601's convention, unless its signature names a
/// raw type. Such a method is raw: `get_object`'s `HeapObjectData`,
/// `register_root`'s `RootProvider`, `get_procedure`'s `Procedure`, and the
/// rest. An `Environment` or a `Library` keeps a method inside, since their
/// names are promised and only their members raw.
pub struct Cx<'gc> {
    heap: &'gc RefCell<Heap>,
    mutator: &'gc Mutator,
    instance: &'gc Instance,
    /// Invariant in `'gc`, so a context's brand is exactly its own.
    brand: PhantomData<fn(&'gc ()) -> &'gc ()>,
}

/// Make a context over `heap` for `f`'s extent. Crate-private: a context is
/// made by the driver or beneath a scope, which vouch that no collection
/// runs while it lives.
pub(crate) fn enter<R>(
    heap: &RefCell<Heap>,
    mutator: &Mutator,
    instance: &Instance,
    f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R,
) -> R {
    mutator.enter_cx();
    let mut cx = Cx {
        heap,
        mutator,
        instance,
        brand: PhantomData,
    };
    f(&mut cx)
}

/// A context for a machine's call of a primitive, with the call's arguments
/// as its values (#706): what the VM and the tree-walker run a context
/// primitive in. The one entry outside this crate that makes a context
/// without the driver or a scope, for the trusted crates that dispatch
/// primitives.
///
/// # Safety
///
/// - No collection runs while `f` runs. A machine vouches for it where it
///   calls a primitive between its safe points: inside one instruction or
///   one step, by a primitive that cannot call back, and so reaches none. A
///   check build asserts it: every collection asserts that no context lives.
/// - Each word of `args` is live, on this heap: the machine read it from its
///   registers or its step as it made the call.
pub unsafe fn enter_call<R>(
    heap: &RefCell<Heap>,
    mutator: &Mutator,
    instance: &Instance,
    args: &[TaggedValue],
    f: impl for<'gc> FnOnce(&mut Cx<'gc>, &[Value<'gc>]) -> R,
) -> R {
    enter(heap, mutator, instance, |cx| {
        // SAFETY: the caller's guarantees: each word is live, and stays so
        // for the context's extent, beneath which nothing collects.
        let args = unsafe { Value::from_tagged_slice(args) };
        f(cx, args)
    })
}

impl Drop for Cx<'_> {
    fn drop(&mut self) {
        self.mutator.exit_cx();
    }
}

/// One walk's addition of a scope to identifiers (#788): the scope, and a
/// memo from each scope set the walk has added it to, by its entry in the
/// heap's scope-set table, to the entry of the set it made, so that the walk
/// interns each distinct set once. [`Cx::scope_addition`] makes one, and
/// [`Cx::add_scope_copying`] and [`Cx::alloc_identifier_adding_scope`] use
/// it.
///
/// Branded with its context's lifetime, so it cannot outlive the context,
/// beneath which nothing collects (#702, #707): no entry it names is freed
/// while it lives. Raw (`docs/README.md`), as are the methods that name it:
/// the macro expander's plumbing.
pub struct ScopeAddition<'gc> {
    scope: ScopeId,
    made: ScopeSetMemo,
    /// Invariant in `'gc`, as [`Cx`]'s brand is.
    brand: PhantomData<fn(&'gc ()) -> &'gc ()>,
}

/// A relinked spelling's symbol (#837, #790), made by [`Cx::relinked_name`]
/// for [`Cx::alloc_relinked_identifier`]: a symbol object the heap's intern
/// table does not hold, so that a spelling bound nowhere is no name to any
/// reader and is not kept for good. Opaque, so that no such symbol becomes
/// a datum: the identifiers that carry it hold it, and stand in quoted data
/// for their spelling interned ([`Cx::identifier_datum`]).
///
/// Branded with its context's lifetime, so it cannot outlive the context,
/// beneath which nothing collects (#702, #707). Raw (`docs/README.md`), as
/// are the methods that name it: the desugarer's relinking plumbing.
#[derive(Clone, Copy)]
pub struct RelinkedName<'gc> {
    name: Value<'gc>,
}

/// A promise's contents, as values: what [`Cx::promise_state`] answers and
/// [`Cx::alloc_promise`] takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseView<'gc> {
    /// Not yet forced: the thunk that computes it.
    Delayed(Value<'gc>),
    /// Forced: its value.
    Forced(Value<'gc>),
}

impl<'gc> Cx<'gc> {
    /// Brand a word the heap answered within this context. Sound because no
    /// collection runs while the context lives: a word read now stays valid
    /// for `'gc`, whatever is stored over it meanwhile.
    #[inline]
    fn brand(&self, word: TaggedValue) -> Value<'gc> {
        // SAFETY: see the doc above.
        unsafe { Value::from_tagged(word) }
    }

    /// [`Cx::brand`], for a vector of words.
    fn brand_vec(&self, words: Vec<TaggedValue>) -> Vec<Value<'gc>> {
        words.into_iter().map(|w| self.brand(w)).collect()
    }

    /// The interpreter's metadata: the command line, the features, the file
    /// system and the library query; and its current ports (#618), which a
    /// port-less `display` reads here, with no borrow of the heap.
    #[inline]
    pub fn instance(&self) -> &'gc Instance {
        self.instance
    }

    // ------------------------------------------------------------------------
    // Reads whose `Heap` form handed out a reference into the heap
    // ------------------------------------------------------------------------

    /// [`Heap::get_bigint`], owned: no `&BigInt` into the heap.
    #[inline]
    pub fn get_bigint(&self, v: Value<'gc>) -> Option<BigInt> {
        self.heap.borrow().get_bigint(v.tagged()).cloned()
    }

    /// [`Heap::get_rational`], owned.
    #[inline]
    pub fn get_rational(&self, v: Value<'gc>) -> Option<BigRational> {
        self.heap.borrow().get_rational(v.tagged()).cloned()
    }

    /// [`Heap::get_string_chars`], as a borrow tied to `&self`.
    #[inline]
    pub fn get_string_chars(&self, s: Value<'gc>) -> Ref<'_, [char]> {
        Ref::map(self.heap.borrow(), |h| h.get_string_chars(s.tagged()))
    }

    /// `string-fill!`'s write: `ch` into `range` of the string `s`, in place,
    /// where nothing hands out a `&mut` into the heap (#708). Panics if `s`
    /// is no string or the range exceeds it; the caller checks both first.
    pub fn string_fill(&mut self, s: Value<'gc>, ch: char, range: std::ops::Range<usize>) {
        assert!(s.is_string(), "Cx::string_fill: not a string");
        self.heap
            .borrow_mut()
            .string_fill_chars(s.tagged(), range, ch);
    }

    /// `string-copy!`'s write: `chars` into the string `s` from `at`. Panics
    /// if `s` is no string or the chars run past its end; the caller checks
    /// both first.
    pub fn string_copy_chars(&mut self, s: Value<'gc>, at: usize, chars: &[char]) {
        assert!(s.is_string(), "Cx::string_copy_chars: not a string");
        self.heap
            .borrow_mut()
            .string_copy_chars(s.tagged(), at, chars);
    }

    /// [`Heap::get_symbol_name`], as a borrow tied to `&self`.
    #[inline]
    pub fn get_symbol_name(&self, v: Value<'gc>) -> Option<Ref<'_, str>> {
        Ref::filter_map(self.heap.borrow(), |h| h.get_symbol_name(v.tagged())).ok()
    }

    /// [`Heap::get_symbol_or_identifier_name`], as a borrow tied to `&self`.
    #[inline]
    pub fn get_symbol_or_identifier_name(&self, v: Value<'gc>) -> Option<Ref<'_, str>> {
        Ref::filter_map(self.heap.borrow(), |h| {
            h.get_symbol_or_identifier_name(v.tagged())
        })
        .ok()
    }

    /// [`Heap::get_values`], as values, borrowed.
    #[inline]
    pub fn get_values(&self, v: Value<'gc>) -> Option<Ref<'_, [Value<'gc>]>> {
        Ref::filter_map(self.heap.borrow(), |h| {
            // SAFETY: the words are a live values object's, read within this
            // context ([`Cx::brand`]).
            h.get_values(v.tagged())
                .map(|words| unsafe { Value::from_tagged_slice(words) })
        })
        .ok()
    }

    /// [`Heap::vector_slice`], as values, borrowed.
    #[inline]
    pub fn vector_slice(&self, v: Value<'gc>) -> Ref<'_, [Value<'gc>]> {
        Ref::map(self.heap.borrow(), |h| {
            // SAFETY: as for `get_values`.
            unsafe { Value::from_tagged_slice(h.vector_slice(v.tagged())) }
        })
    }

    /// [`Heap::get_exception`], owned: its kind, message and irritants.
    pub fn get_exception(&self, v: Value<'gc>) -> Option<(ExceptionKind, String, Vec<Value<'gc>>)> {
        let heap = self.heap.borrow();
        let (kind, message, irritants) = heap.get_exception(v.tagged())?;
        Some((
            kind.clone(),
            message.to_string(),
            irritants.iter().map(|&w| self.brand(w)).collect(),
        ))
    }

    /// The kind of the error object `v`, or `None` if `v` is none: what the
    /// predicates (`error-object?`, `file-error?`) ask, without the copies of
    /// the message and the irritants [`Cx::get_exception`] makes.
    #[inline]
    pub fn exception_kind(&self, v: Value<'gc>) -> Option<ExceptionKind> {
        let heap = self.heap.borrow();
        heap.get_exception(v.tagged())
            .map(|(kind, _, _)| kind.clone())
    }

    /// [`Heap::get_identifier_data`], owned.
    #[inline]
    pub fn get_identifier_data(&self, v: Value<'gc>) -> Option<(Rc<str>, ScopeSet)> {
        let heap = self.heap.borrow();
        let (name, scopes) = heap.get_identifier_data(v.tagged())?;
        Some((Rc::clone(name), scopes.clone()))
    }

    /// [`Heap::get_macro`], cloned.
    #[inline]
    pub fn get_macro(&self, v: Value<'gc>) -> Option<Rc<CompiledMacro>> {
        self.heap.borrow().get_macro(v.tagged()).cloned()
    }

    /// The literal `id` of the macro `mac` ([`Heap::macro_literal`]): `None`
    /// if `mac` is no macro or has no such literal. Sound because `mac` is
    /// live for `'gc`, and the object it names traces its literals (#718).
    #[inline]
    pub fn macro_literal(&self, mac: Value<'gc>, id: LiteralId) -> Option<Value<'gc>> {
        let word = self.heap.borrow().macro_literal(mac.tagged(), id)?;
        Some(self.brand(word))
    }

    /// [`Heap::get_port`]: the port `v` is, its `Rc` cloned out of the
    /// heap's `PortTable`, so the caller reads or writes it after the access
    /// has ended (#785). Always inlined: every port operation calls it, and
    /// left to itself the compiler called it out of line once the table's
    /// read made it longer, about 8 instructions a call more.
    #[inline(always)]
    pub fn get_port(&self, v: Value<'gc>) -> Option<Rc<Port>> {
        self.heap.borrow().get_port(v.tagged())
    }

    /// [`Heap::is_port`]: whether `v` is a port, from its slot alone, with no
    /// read of the heap's `PortTable`.
    #[inline]
    pub fn is_port(&self, v: Value<'gc>) -> bool {
        self.heap.borrow().is_port(v.tagged())
    }

    /// [`Heap::get_record_type`], cloned.
    #[inline]
    pub fn get_record_type(&self, v: Value<'gc>) -> Option<Rc<RecordTypeDescriptor>> {
        self.heap.borrow().get_record_type(v.tagged()).cloned()
    }

    /// [`Heap::get_environment_specifier`], cloned.
    #[inline]
    pub fn get_environment_specifier(&self, v: Value<'gc>) -> Option<(Rc<Environment>, bool)> {
        self.heap
            .borrow()
            .get_environment_specifier(v.tagged())
            .map(|(env, mutable)| (Rc::clone(env), mutable))
    }

    /// `env`'s mutable specifier ([`Environment::mutable_specifier`]): the
    /// same object each time, which `env` holds and traces with its
    /// bindings, allocated the first time. `load` evaluates in the
    /// interaction environment's.
    #[inline]
    pub fn mutable_specifier(&mut self, env: &Rc<Environment>) -> Value<'gc> {
        let word = env.mutable_specifier(self.heap);
        self.brand(word)
    }

    /// A new empty namespace whose values live on this context's heap
    /// ([`Environment::with_heap`]): what `environment` and the R5RS
    /// constructors build.
    #[inline]
    pub fn new_namespace(&self) -> Environment {
        Environment::namespace(&self.heap.borrow())
    }

    /// Bind `name` in `env` to the primitive `library`'s `name`, through the
    /// namespace view ([`NamespaceView::define_primitive`](crate::NamespaceView::define_primitive)).
    #[inline]
    pub fn define_primitive(
        &mut self,
        env: &Environment,
        name: &'static str,
        arity: crate::procedure::Arity,
        library: Vec<String>,
    ) {
        crate::NamespaceView::of(env).define_primitive(self.heap, name, arity, library);
    }

    /// What `name` is bound to in `env` or its parents, a lookup through the
    /// namespace view ([`NamespaceView::get`](crate::NamespaceView::get)), as a value: read as
    /// [`Self::resolve_with_scopes`] reads a binding, which it is as live
    /// as.
    #[inline]
    pub fn lookup(&self, env: &Environment, name: &str) -> Option<Value<'gc>> {
        debug_assert!(
            env.heap_id() == self.heap.borrow().id(),
            "Cx::lookup: an environment of another heap"
        );
        crate::NamespaceView::of(env)
            .get(self.heap, name)
            .map(|word| self.brand(word))
    }

    /// A literal pool on this context's heap ([`LiteralPool::new`]), for a
    /// form a primitive desugars, as `eval` does to check an immutable
    /// environment's.
    #[inline]
    pub fn literal_pool(&self) -> crate::LiteralPool {
        crate::LiteralPool::new(&self.heap.borrow())
    }

    /// What `name`, written with `scopes`, resolves to in `env`
    /// ([`Environment::resolve_with_scopes`]), as a value, and whether a
    /// binding's scopes selected it: how converted code reads a binding, a
    /// macro object among them (#704). An environment's bindings are traced
    /// with it, and a context reads one the collector reaches or one made
    /// within the context, as the desugarer's expansion environments are; so
    /// a binding read now is as live as a heap object's field read now.
    #[inline]
    pub fn resolve_with_scopes(
        &self,
        env: &Environment,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<(Option<Value<'gc>>, bool), Box<crate::scope_resolve::AmbiguousReference>> {
        debug_assert!(
            env.heap_id() == self.heap.borrow().id(),
            "Cx::resolve_with_scopes: an environment of another heap"
        );
        let (word, selected) = env.resolve_with_scopes(self.heap, name, scopes)?;
        Ok((word.map(|word| self.brand(word)), selected))
    }

    /// [`Environment::scoped_macro_binding`] on this context's heap, which
    /// each namespace's step borrows for its read and releases after.
    #[inline]
    pub fn scoped_macro_binding(
        &self,
        env: &Rc<Environment>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<(Rc<Environment>, ScopeSet)> {
        env.scoped_macro_binding(self.heap, name, scopes)
    }

    // ------------------------------------------------------------------------
    // Namespace operations over a view (#834)
    //
    // Each takes a `NamespaceView`, a raw type, so each is raw under
    // `docs/README.md`'s method rule. Each hands the view this context's
    // heap, which the environment borrows at the namespace step and
    // releases: no method returns the heap or a guard. Reads take `&self`;
    // writes take `&mut self`, since a namespace's write can allocate a
    // binding record and a cell (#834).
    // ------------------------------------------------------------------------

    /// The location `name` reaches in `ns` ([`NamespaceView::binding_location`]).
    /// Raw: names `NamespaceView`.
    #[inline]
    pub fn binding_location(&self, ns: NamespaceView<'_>, name: &str) -> Option<BindingLocation> {
        ns.binding_location(self.heap, name)
    }

    /// Which scoped binding `name` at `scopes` denotes in `ns`
    /// ([`NamespaceView::scoped_binding_of`]). Raw: names `NamespaceView`.
    #[inline]
    pub fn scoped_binding_of(
        &self,
        ns: NamespaceView<'_>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<Option<ScopeSet>, Box<AmbiguousReference>> {
        ns.scoped_binding_of(self.heap, name, scopes)
    }

    /// Whether `name`'s name-only view in `ns` is the binding `scopes`
    /// denote ([`NamespaceView::name_reaches_binding_of`]). Raw: names
    /// `NamespaceView`.
    #[inline]
    pub fn name_reaches_binding_of(
        &self,
        ns: NamespaceView<'_>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<bool, Box<AmbiguousReference>> {
        ns.name_reaches_binding_of(self.heap, name, scopes)
    }

    /// The macro-introduced top-level definition `scopes` select for `name`
    /// in `ns`'s root ([`NamespaceView::introduced_definition`]). Raw: names
    /// `NamespaceView`.
    #[inline]
    pub fn introduced_definition(
        &self,
        ns: NamespaceView<'_>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<IntroducedDefinition> {
        ns.introduced_definition(self.heap, name, scopes)
    }

    /// The names a program bound in `ns` only ([`NamespaceView::local_names`]).
    /// Raw: names `NamespaceView`.
    #[inline]
    pub fn local_names(&self, ns: NamespaceView<'_>) -> Vec<String> {
        ns.local_names(self.heap)
    }

    /// Define `name` as `value` in `ns` ([`NamespaceView::define`]): an
    /// insert, a store or a re-point. Raw: names `NamespaceView`.
    #[inline]
    pub fn define(&mut self, ns: NamespaceView<'_>, name: impl Into<Rc<str>>, value: Value<'gc>) {
        ns.define(self.heap, name, value.tagged());
    }

    /// Define `name` at `scopes` in `ns` ([`NamespaceView::define_with_scopes`]).
    /// Raw: names `NamespaceView`.
    #[inline]
    pub fn define_with_scopes(
        &mut self,
        ns: NamespaceView<'_>,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: Value<'gc>,
    ) {
        ns.define_with_scopes(self.heap, name, scopes, value.tagged());
    }

    /// Carry `staged_name` from `staging` into `ns` as `name`
    /// ([`NamespaceView::copy_binding`]). Raw: names `NamespaceView`.
    #[inline]
    pub fn copy_binding(
        &mut self,
        ns: NamespaceView<'_>,
        name: impl Into<Rc<str>>,
        staging: &Rc<Environment>,
        staged_name: &str,
    ) -> bool {
        ns.copy_binding(self.heap, name, staging, staged_name)
    }

    /// The record an early-bound reference to `name` links when `ns` is
    /// its macro's environment, branded by this context, and the location
    /// of its cell (#837; `Environment::early_link`, the owner's ruling
    /// E1): the record `ns`'s own walk finds for the name, or a fresh one
    /// over a frame's import, which allocates. `None` where the walk finds
    /// nothing or a frame's own binding. Pool the handle
    /// ([`LiteralPool::add_link`](crate::LiteralPool::add_link)) before the
    /// context closes: nothing roots it until then but the context, beneath
    /// which nothing collects. Raw: names `NamespaceView`.
    #[inline]
    pub fn early_link(
        &mut self,
        ns: NamespaceView<'_>,
        name: &str,
    ) -> Option<(BindingLocation, crate::Linkable<'gc>)> {
        let (location, link) = ns.env().early_link(self.heap, name)?;
        Some((location, crate::Linkable::new(link, LinkKind::Early)))
    }

    /// The record a relinked mention of `name` links (#837's second PR;
    /// `Environment::relink_record`, the owner's ruling E1), branded by
    /// this context, of [`LinkKind::Relinked`]: `ns` is the environment
    /// `target` is asked of, the macro's real definition environment for
    /// [`RelinkTarget::ByName`], never a declarations view of it, and for
    /// the others the environment that holds the definition. `None` where
    /// the target has no record (a frame's own variable, the VM's renamed
    /// global before its `Define` has run, an identity nobody holds), and
    /// the mention is left as written. A frame's import and a frame's
    /// keyword are answered by a fresh record, which allocates. Pool the
    /// handle ([`LiteralPool::add_link`](crate::LiteralPool::add_link))
    /// before the context closes, as [`early_link`](Self::early_link)'s.
    /// Raw: names `NamespaceView`, `RelinkTarget` and `Linkable`.
    #[inline]
    pub fn relink_link(
        &mut self,
        ns: NamespaceView<'_>,
        name: &str,
        target: RelinkTarget<'_>,
    ) -> Option<crate::Linkable<'gc>> {
        let link = ns.env().relink_record(self.heap, name, target)?;
        Some(crate::Linkable::new(link, LinkKind::Relinked))
    }

    /// A pooled link as a handle of this context again (#837): the record
    /// `link` stands for in `pool`, of the link's kind. Sound because the
    /// pool is alive, so it roots the record, and the handle cannot outlive
    /// the context, beneath which nothing collects. For reading what a
    /// link's record holds ([`linked_value`](Self::linked_value),
    /// [`linked_location`](Self::linked_location)) and for handing a
    /// form's link to a macro object that will carry it
    /// ([`alloc_macro_with_relinks`](Self::alloc_macro_with_relinks)).
    /// Panics, as [`LiteralPool::link`] does, on a link of another pool.
    /// Raw: names `Linkable`.
    #[inline]
    #[track_caller]
    pub fn linkable(&self, pool: &LiteralPool, link: Link) -> crate::Linkable<'gc> {
        crate::Linkable::new(pool.link(link), link.kind())
    }

    /// What `link`'s record holds now (#837): record → cell → value, as a
    /// linked read reads it. `None` where the cell holds `UNBOUND`, which
    /// no record a link names does. Raw: names `Linkable`.
    #[inline]
    pub fn linked_value(&self, link: crate::Linkable<'gc>) -> Option<Value<'gc>> {
        let value = link.link().value(&self.heap.borrow())?;
        Some(self.brand(value))
    }

    /// Where `link`'s record's cell is (#837), as
    /// [`binding_location`](Self::binding_location) names a namespace's
    /// binding or an import, so that the two compare: an import is its
    /// exporter's cell. Computed when asked and held across nothing: a
    /// cell's word is an identity only beneath the context that read it
    /// (the rulings of 2026-11-03). Raw: names `Linkable`.
    #[inline]
    pub fn linked_location(&self, link: crate::Linkable<'gc>) -> BindingLocation {
        link.link().record().cell(&self.heap.borrow()).location()
    }

    /// A record over a fresh free cell holding `UNBOUND`, branded by this
    /// context, for a backend's tests (#837): a pinned operand over it is
    /// one no early link makes, and reaches the cold paths that refuse to
    /// read or write a pinned entry by name (the VM's `store_global_cold`),
    /// which no record a program can link reaches. Pool it as
    /// [`early_link`](Self::early_link)'s. Only with `test-support`.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn unbound_link_for_tests(&mut self) -> crate::Linkable<'gc> {
        let mut heap = self.heap.borrow_mut();
        let cell = crate::BindingCell::free(&mut heap, TaggedValue::UNBOUND);
        let record = crate::binding_record::Record::new(&mut heap, cell);
        crate::Linkable::new(crate::BindingLink::new(record), LinkKind::Early)
    }

    /// Whether `name` is `ns`'s import of `location` (#837;
    /// `Environment::imports_location`): a slot of its own over a cell it
    /// does not own, at `location`. `false` for a frame. Raw: names
    /// `NamespaceView`.
    #[inline]
    pub fn imports_location(
        &self,
        ns: NamespaceView<'_>,
        name: &str,
        location: &BindingLocation,
    ) -> bool {
        ns.env().imports_location(self.heap, name, location)
    }

    /// Import `library`'s `export` into `ns` as `name`
    /// ([`Library::import_into`]): the library's binding, not a copy. Raw:
    /// names `NamespaceView`.
    #[inline]
    pub fn import_into(
        &mut self,
        library: &Library,
        ns: NamespaceView<'_>,
        name: impl Into<Rc<str>>,
        export: &str,
    ) -> bool {
        library.import_into(self.heap, ns.env(), name, export)
    }

    /// [`define_with_scopes`](Self::define_with_scopes) for a frame, through
    /// a shared context: a frame's own binding takes no borrow and allocates
    /// nothing, so it needs no `&mut`. For the
    /// desugarer's binding forms and declarations, which run where only
    /// `&Cx` is in hand. Raw: names `NamespaceView`.
    ///
    /// # Panics
    ///
    /// When `frame` views a namespace, whose write takes the heap mutably.
    #[inline]
    pub fn define_with_scopes_in_frame(
        &self,
        frame: NamespaceView<'_>,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: Value<'gc>,
    ) {
        assert!(
            !frame.env().is_namespace(),
            "Cx::define_with_scopes_in_frame: a frame's own binding, which takes no borrow and \
             allocates nothing"
        );
        frame.define_with_scopes(self.heap, name, scopes, value.tagged());
    }

    /// Import `cell` into the frame `frame` as `name`
    /// ([`NamespaceView::import_cell`]), through a shared context: a frame's
    /// import allocates nothing and takes no borrow, so it needs no `&mut`.
    /// A read through it borrows the heap shared, as a read of any frame's
    /// import does. For the desugarer's declaration views. Raw: names
    /// `NamespaceView` and `BindingCell`.
    ///
    /// # Panics
    ///
    /// When `frame` views a namespace.
    #[inline]
    pub fn import_cell_in_frame(
        &self,
        frame: NamespaceView<'_>,
        name: impl Into<Rc<str>>,
        cell: crate::BindingCell,
    ) {
        assert!(
            !frame.env().is_namespace(),
            "Cx::import_cell_in_frame: a frame's import, which takes no borrow and allocates \
             nothing"
        );
        frame.import_cell(self.heap, name, cell);
    }

    /// A free cell holding `UNSPECIFIED`, for a declaration's location
    /// (#463), which every view of the declaration imports. Raw (names
    /// `BindingCell`) and hidden: the cell carries no `'gc` brand, so it
    /// must not outlive this context — the next collection may sweep it and
    /// reuse its slot. The desugarer drops it with the form.
    #[doc(hidden)]
    #[inline]
    pub fn new_declaration_cell(&mut self) -> crate::BindingCell {
        crate::BindingCell::free(&mut self.heap.borrow_mut(), TaggedValue::UNSPECIFIED)
    }

    /// [`Heap::source`].
    #[inline]
    pub fn source(&self, v: Value<'gc>) -> Option<SourceLocation> {
        self.heap.borrow().source(v.tagged())
    }

    /// [`Heap::alloc_identifier_relocated`]: a copy of the identifier `v`
    /// located at `location`, for an identifier another context made.
    /// Raw: the expansion stamp's and the template compiler's plumbing
    /// (#791).
    pub fn alloc_identifier_relocated(
        &mut self,
        v: Value<'gc>,
        location: &SourceLocation,
    ) -> Option<Value<'gc>> {
        let copy = self
            .heap
            .borrow_mut()
            .alloc_identifier_relocated(v.tagged(), location)?;
        Some(self.brand(copy))
    }

    /// [`Heap::alloc_identifier_formed`]: a copy of the identifier `v`
    /// whose form span is `form`, for the head of a list a template builds
    /// that another context made. Raw: the template expander's (#792).
    pub fn alloc_identifier_formed(
        &mut self,
        v: Value<'gc>,
        form: Option<&SourceLocation>,
    ) -> Option<Value<'gc>> {
        let copy = self
            .heap
            .borrow_mut()
            .alloc_identifier_formed(v.tagged(), form)?;
        Some(self.brand(copy))
    }

    /// [`Heap::record_form`]: the form span of the identifier `v`, made in
    /// this context, written in place. Raw: the template expander's (#792).
    #[inline]
    pub fn record_form(&mut self, v: Value<'gc>, form: Option<&SourceLocation>) {
        self.heap.borrow_mut().record_form(v.tagged(), form)
    }

    /// [`Heap::form_span`]: the span of the list the identifier `v` heads,
    /// which is that list's location ([`Self::source`]).
    #[inline]
    pub fn form_span(&self, v: Value<'gc>) -> Option<SourceLocation> {
        self.heap.borrow().form_span(v.tagged())
    }

    /// [`Heap::record_form_span`]: the reader's, when a list closes.
    #[inline]
    pub fn record_form_span(&mut self, v: Value<'gc>, form: &SourceLocation) {
        self.heap.borrow_mut().record_form_span(v.tagged(), form)
    }

    /// [`Heap::record_head_source`]: the reader's, for a list's head it has
    /// just made, when the list closes.
    #[inline]
    pub fn record_head_source(
        &mut self,
        v: Value<'gc>,
        own: &SourceLocation,
        form: &SourceLocation,
    ) {
        self.heap
            .borrow_mut()
            .record_head_source(v.tagged(), own, form)
    }

    /// [`Heap::documents`]: `(gc-stats)`'s `documents` (#791).
    #[inline]
    pub fn documents(&self) -> usize {
        self.heap.borrow().documents()
    }

    /// [`Heap::document_bytes`]: `(gc-stats)`'s `document-bytes` (#791).
    #[inline]
    pub fn document_bytes(&self) -> usize {
        self.heap.borrow().document_bytes()
    }

    /// [`Heap::get_object`], borrowed: for the datum writer, which matches on
    /// the object's kind. Its words are raw, and a payload that changes is
    /// in an [`Interior`](crate::heap::Interior), which a match can name but
    /// not borrow (#708).
    #[inline]
    pub fn get_object(&self, v: Value<'gc>) -> Ref<'_, HeapObjectData> {
        Ref::map(self.heap.borrow(), |h| h.get_object(v.tagged()))
    }

    // ------------------------------------------------------------------------
    // Records, parameters and promises, by value rather than by `Rc` payload
    // ------------------------------------------------------------------------

    /// A record of type `rtd` with `fields`.
    pub fn alloc_record(
        &mut self,
        rtd: Rc<RecordTypeDescriptor>,
        fields: Vec<Value<'gc>>,
    ) -> Value<'gc> {
        let words = fields.into_iter().map(Value::tagged).collect();
        let r = self.heap.borrow_mut().alloc_record(rtd, words);
        self.brand(r)
    }

    /// The type of the record `v`, or `None` if `v` is no record.
    #[inline]
    pub fn record_type_of(&self, v: Value<'gc>) -> Option<Rc<RecordTypeDescriptor>> {
        self.heap
            .borrow()
            .get_record(v.tagged())
            .map(|(rtd, _)| Rc::clone(rtd))
    }

    /// Whether `v` is a record of the type `rtd`, a record-type object: the
    /// two descriptors compared by identity, with no allocation (#736). An
    /// `rtd` that is no record type answers `false`.
    #[inline]
    pub fn is_record_of(&self, v: Value<'gc>, rtd: Value<'gc>) -> bool {
        let heap = self.heap.borrow();
        match (
            heap.get_record(v.tagged()),
            heap.get_record_type(rtd.tagged()),
        ) {
            (Some((record_type, _)), Some(expected)) => Rc::ptr_eq(record_type, expected),
            _ => false,
        }
    }

    /// How many fields the record `v` has, or `None` if `v` is no record.
    #[inline]
    pub fn record_len(&self, v: Value<'gc>) -> Option<usize> {
        self.heap
            .borrow()
            .get_record(v.tagged())
            .map(|(_, fields)| fields.borrow().len())
    }

    /// Field `index` of the record `v`, or `None` if `v` is no record or has
    /// no such field. A field is written through the funnel
    /// ([`RecordField`]).
    #[inline]
    pub fn record_ref(&self, v: Value<'gc>, index: usize) -> Option<Value<'gc>> {
        let heap = self.heap.borrow();
        let (_, fields) = heap.get_record(v.tagged())?;
        let word = fields.borrow().get(index).copied()?;
        Some(self.brand(word))
    }

    /// A parameter object whose value is `init`, with `converter` if it has
    /// one.
    pub fn alloc_parameter(
        &mut self,
        init: Value<'gc>,
        converter: Option<Value<'gc>>,
    ) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .alloc_parameter(init.tagged(), converter.map(Value::tagged));
        self.brand(r)
    }

    /// Whether `v` is a parameter object.
    #[inline]
    pub fn is_parameter(&self, v: Value<'gc>) -> bool {
        self.heap.borrow().is_parameter(v.tagged())
    }

    /// The current value of the parameter object `v`, or `None` if `v` is
    /// none. It is written through the funnel ([`ParamValue`]).
    #[inline]
    pub fn parameter_value(&self, v: Value<'gc>) -> Option<Value<'gc>> {
        let (value, _) = self.heap.borrow().parameter_parts(v.tagged())?;
        Some(self.brand(value.unwrap_or(TaggedValue::UNSPECIFIED)))
    }

    /// The converter of the parameter object `v`: `None` if `v` is none,
    /// `Some(None)` if it has none.
    #[inline]
    pub fn parameter_converter(&self, v: Value<'gc>) -> Option<Option<Value<'gc>>> {
        let (_, converter) = self.heap.borrow().parameter_parts(v.tagged())?;
        Some(converter.map(|w| self.brand(w)))
    }

    /// Which of the heap's standard port parameters `v` is, if it is one
    /// ([`Heap::standard_port_parameter`], #608).
    #[inline]
    pub fn standard_port_parameter(&self, v: Value<'gc>) -> Option<crate::CurrentPort> {
        self.heap.borrow().standard_port_parameter(v.tagged())
    }

    /// `parameterize`'s loop for bindings that are not all parameter
    /// objects, once its first use has compiled it
    /// ([`Heap::parameterize_each`], #817).
    pub fn parameterize_each(&self) -> Option<Value<'gc>> {
        let each = self.heap.borrow().parameterize_each()?;
        Some(self.brand(each))
    }

    /// Record `parameterize`'s loop, rooted for the heap's life. A root,
    /// not a heap object's field, so no store funnel.
    pub fn set_parameterize_each(&mut self, procedure: Value<'gc>) {
        self.heap.borrow().set_parameterize_each(procedure.tagged());
    }

    /// A promise in `state`.
    pub fn alloc_promise(&mut self, state: PromiseView<'gc>) -> Value<'gc> {
        let state = match state {
            PromiseView::Delayed(thunk) => PromiseState::Delayed(thunk.tagged()),
            PromiseView::Forced(value) => PromiseState::Forced(value.tagged()),
        };
        let r = self.heap.borrow_mut().alloc_promise(state);
        self.brand(r)
    }

    /// The contents of the promise `v`, or `None` if `v` is none. A forced
    /// value is stored through the funnel ([`PromiseBox`]).
    #[inline]
    pub fn promise_state(&self, v: Value<'gc>) -> Option<PromiseView<'gc>> {
        let cell = self.heap.borrow().get_promise(v.tagged())?;
        let state = *cell.borrow();
        Some(match state {
            PromiseState::Delayed(thunk) => PromiseView::Delayed(self.brand(thunk)),
            PromiseState::Forced(value) => PromiseView::Forced(self.brand(value)),
        })
    }

    // ------------------------------------------------------------------------
    // Lists, from iterators of values
    // ------------------------------------------------------------------------

    /// [`Heap::list_from_iter`], from values.
    #[inline]
    pub fn list_from_iter<I>(&mut self, iter: I) -> Value<'gc>
    where
        I: IntoIterator<Item = Value<'gc>>,
        I::IntoIter: DoubleEndedIterator,
    {
        let r = self
            .heap
            .borrow_mut()
            .list_from_iter(iter.into_iter().map(Value::tagged));
        self.brand(r)
    }

    /// [`Heap::list_from_iter_with_tail`], from values.
    #[inline]
    pub fn list_from_iter_with_tail<I>(&mut self, iter: I, tail: Value<'gc>) -> Value<'gc>
    where
        I: IntoIterator<Item = Value<'gc>>,
        I::IntoIter: DoubleEndedIterator,
    {
        let r = self
            .heap
            .borrow_mut()
            .list_from_iter_with_tail(iter.into_iter().map(Value::tagged), tail.tagged());
        self.brand(r)
    }

    // ------------------------------------------------------------------------
    // Roots
    // ------------------------------------------------------------------------

    /// Register `provider` with the heap's root set until the token drops
    /// (`Heap::root_set`, #682): the runtime's library registry.
    pub fn register_root(&self, provider: Box<dyn RootProvider>) -> RootToken {
        self.heap.borrow().root_set().register(provider)
    }

    // ------------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------------

    /// `v` written for a diagnostic: [`format_tagged`](crate::format_tagged).
    #[inline]
    pub fn format(&self, v: Value<'gc>) -> String {
        crate::format_tagged(v.tagged(), &self.heap.borrow())
    }

    /// `v` written briefly, for a diagnostic about part of a form:
    /// [`format_tagged_brief`](crate::format_tagged_brief).
    #[inline]
    pub fn format_brief(&self, v: Value<'gc>) -> String {
        crate::format_tagged_brief(v.tagged(), &self.heap.borrow())
    }

    /// `v` written with each identifier's scopes, for the macro debug log:
    /// [`format_tagged_with_scopes`](crate::format_tagged_with_scopes).
    #[inline]
    pub fn format_with_scopes(&self, v: Value<'gc>) -> String {
        crate::format_tagged_with_scopes(v.tagged(), &self.heap.borrow())
    }

    // ------------------------------------------------------------------------
    // Syntax
    // ------------------------------------------------------------------------

    /// Give each syntax identifier in `root` that `picks` picks the scopes
    /// `rescope` makes of its own: the macro expander's scope flip and the
    /// desugarer's scope additions (#704). An identifier the reader wrote,
    /// which has a symbol's binding behavior, is never picked, and a symbol
    /// is no identifier.
    ///
    /// The walk copies only the pairs and vectors above an identifier that
    /// changes, keeps every other part of `root` as it is, shared structure
    /// and cycles included, and answers `root` itself when nothing changes.
    /// It is the heap's own walk under one borrow, since it is the hottest
    /// walk the expander makes; neither function sees the heap. `rescope`
    /// depends on the set alone, so it runs, and its set is interned in the
    /// heap's scope-set table, once for each distinct set the walk changes
    /// (#788).
    pub fn rescope_identifiers(
        &mut self,
        root: Value<'gc>,
        picks: impl Fn(&str, &ScopeSet) -> bool,
        rescope: impl Fn(&ScopeSet) -> ScopeSet,
    ) -> Value<'gc> {
        if !picks_any_identifier(&self.heap.borrow(), root.tagged(), &picks) {
            return root;
        }
        let word = self
            .heap
            .borrow_mut()
            .rescope_identifiers(root.tagged(), picks, rescope);
        self.brand(word)
    }

    /// A memo for one walk that adds `scope` to identifiers (#788), for
    /// [`Cx::add_scope_copying`] and [`Cx::alloc_identifier_adding_scope`].
    pub fn scope_addition(&self, scope: ScopeId) -> ScopeAddition<'gc> {
        ScopeAddition {
            scope,
            made: ScopeSetMemo::default(),
            brand: PhantomData,
        }
    }

    /// A copy of the identifier `v` whose set is its own with `addition`'s
    /// scope added, located where `v` is (it takes `v`'s source id, #791),
    /// or `None` if `v` is no identifier: the macro expander's mark of a
    /// substituted identifier. The copy is no written name, whatever `v`
    /// was. Each distinct set is added to, and the result interned, once in
    /// the memo's life (#788). Raw: it names the raw [`ScopeAddition`].
    pub fn add_scope_copying(
        &mut self,
        addition: &mut ScopeAddition<'gc>,
        v: Value<'gc>,
    ) -> Option<Value<'gc>> {
        let mut heap = self.heap.borrow_mut();
        let copy = heap.identifier_adding_scope(&mut addition.made, addition.scope, v.tagged())?;
        drop(heap);
        Some(self.brand(copy))
    }

    /// An identifier for the symbol `symbol` whose set is `addition`'s
    /// scope alone: the macro expander's mark of a substituted symbol, which
    /// carries the symbol itself (#790), its set interned once in the memo's
    /// life (#788). Raw: it names the raw [`ScopeAddition`].
    pub fn alloc_identifier_adding_scope(
        &mut self,
        addition: &mut ScopeAddition<'gc>,
        symbol: Value<'gc>,
    ) -> Value<'gc> {
        let word = self.heap.borrow_mut().identifier_in_scope(
            &mut addition.made,
            addition.scope,
            symbol.tagged(),
        );
        self.brand(word)
    }

    /// The symbol a relinked spelling's identifiers carry (#790): made once
    /// for each spelling a relinking gives, for every occurrence it renames
    /// ([`Cx::alloc_relinked_identifier`]). No table holds it. Raw: it
    /// answers the raw [`RelinkedName`].
    pub fn relinked_name(&mut self, spelling: Rc<str>) -> RelinkedName<'gc> {
        let word = self.heap.borrow_mut().alloc_relinked_name(spelling);
        RelinkedName {
            name: self.brand(word),
        }
    }

    /// [`Self::alloc_relinked_identifier`] located where the identifier
    /// `original` is, the reference relinking renamed: its source id, with
    /// no write (#791, #793). Raw: it names the raw [`RelinkedName`].
    pub fn alloc_relinked_copy(
        &mut self,
        original: Value<'gc>,
        name: &RelinkedName<'gc>,
        scopes: ScopeSet,
    ) -> Value<'gc> {
        let word = self.heap.borrow_mut().alloc_relinked_copy(
            original.tagged(),
            name.name.tagged(),
            scopes,
        );
        self.brand(word)
    }

    /// An identifier relinking renamed (#837): `name`'s spelling, which is
    /// bound nowhere and interned nowhere (#790), with the set `scopes`. Raw:
    /// it names the raw [`RelinkedName`].
    pub fn alloc_relinked_identifier(
        &mut self,
        name: &RelinkedName<'gc>,
        scopes: ScopeSet,
    ) -> Value<'gc> {
        let word = self
            .heap
            .borrow_mut()
            .alloc_relinked_identifier(name.name.tagged(), scopes);
        self.brand(word)
    }

    /// [`Heap::map_syntax_identifiers`], whose `transform` takes this context.
    pub fn map_syntax_identifiers(
        &mut self,
        root: Value<'gc>,
        transform: impl FnMut(&mut Cx<'gc>, Value<'gc>, Rc<str>, ScopeSet) -> Option<Value<'gc>>,
    ) -> Value<'gc> {
        self.map_syntax_identifiers_memo(root, &mut std::collections::HashMap::new(), transform)
    }

    /// [`Heap::map_syntax_identifiers_memo`], whose `transform` takes this
    /// context: the same walk, made of this context's operations. `Heap`
    /// keeps its own for [`Cx::rescope_identifiers`], which walks under one
    /// borrow.
    pub fn map_syntax_identifiers_memo(
        &mut self,
        root: Value<'gc>,
        replacements: &mut std::collections::HashMap<Value<'gc>, Value<'gc>>,
        mut transform: impl FnMut(&mut Cx<'gc>, Value<'gc>, Rc<str>, ScopeSet) -> Option<Value<'gc>>,
    ) -> Value<'gc> {
        self.walk_identifiers_memo(root, replacements, |cx, value| {
            let (name, scopes) = cx.get_identifier_data_any(value)?;
            transform(cx, value, name, scopes)
        })
    }

    /// `root` as quoted data: each identifier in it replaced by the symbol
    /// it stands for ([`Cx::identifier_datum`]), its name's, with no lookup
    /// of its spelling (#790). The walk of
    /// [`Cx::map_syntax_identifiers_memo`], sharing `replacements` across
    /// the roots of one form as it does. Raw (`docs/README.md`): quote
    /// stripping's plumbing.
    pub fn strip_identifiers_memo(
        &mut self,
        root: Value<'gc>,
        replacements: &mut std::collections::HashMap<Value<'gc>, Value<'gc>>,
    ) -> Value<'gc> {
        self.walk_identifiers_memo(root, replacements, |cx, value| cx.identifier_datum(value))
    }

    /// The walk under [`Cx::map_syntax_identifiers_memo`] and
    /// [`Cx::strip_identifiers_memo`]: `copy_of` is asked of each value it
    /// meets that is no pair or vector, and answers an identifier's copy, or
    /// `None` to leave the value as it is.
    fn walk_identifiers_memo(
        &mut self,
        root: Value<'gc>,
        replacements: &mut std::collections::HashMap<Value<'gc>, Value<'gc>>,
        mut copy_of: impl FnMut(&mut Cx<'gc>, Value<'gc>) -> Option<Value<'gc>>,
    ) -> Value<'gc> {
        use std::collections::{HashMap, HashSet};
        if let Some(&copy) = replacements.get(&root) {
            return copy;
        }
        let mut parents = HashMap::<Value<'gc>, Vec<Value<'gc>>>::new();
        let mut seen = HashSet::new();
        let mut pending = vec![root];
        let mut changed = Vec::new();
        while let Some(value) = pending.pop() {
            if !seen.insert(value) {
                continue;
            }
            if let Some(&copy) = replacements.get(&value) {
                if copy != value {
                    changed.push(value);
                }
                continue;
            }
            if self.is_identifier(value) {
                if let Some(copy) = copy_of(self, value) {
                    self.heap
                        .borrow_mut()
                        .locate_copy(value.tagged(), copy.tagged());
                    replacements.insert(value, copy);
                    changed.push(value);
                }
                continue;
            }
            let children = if value.is_pair() {
                let (car, cdr) = self.get_pair(value);
                vec![car, cdr]
            } else if value.is_vector() {
                self.vector_slice(value).to_vec()
            } else {
                continue;
            };
            for child in children {
                if child.is_pair() || child.is_vector() || self.is_identifier(child) {
                    parents.entry(child).or_default().push(value);
                    pending.push(child);
                }
            }
        }
        let mut containers = Vec::new();
        while let Some(child) = changed.pop() {
            if let Some(ancestors) = parents.get(&child) {
                for &parent in ancestors {
                    if replacements.contains_key(&parent) {
                        continue;
                    }
                    let copy = if parent.is_pair() {
                        self.alloc_pair(Value::UNSPECIFIED, Value::NULL)
                    } else {
                        let len = self.vector_len(parent);
                        self.alloc_vector(vec![Value::UNSPECIFIED; len])
                    };
                    replacements.insert(parent, copy);
                    containers.push((parent, copy));
                    changed.push(parent);
                }
            }
        }
        let mapped = |value: Value<'gc>| replacements.get(&value).copied().unwrap_or(value);
        for (original, copy) in containers {
            if original.is_pair() {
                let (car, cdr) = self.get_pair(original);
                self.set_car(copy, mapped(car));
                self.set_cdr(copy, mapped(cdr));
            } else {
                for i in 0..self.vector_len(original) {
                    let element = self.vector_ref(original, i);
                    self.vector_set(copy, i, mapped(element));
                }
            }
        }
        mapped(root)
    }
}

/// Whether `root` holds a syntax identifier, other than one the reader
/// wrote, that `picks` picks: [`Cx::rescope_identifiers`]' scan, which spares
/// the copying walk when nothing would change.
///
/// The tree may be cyclic: a quoted datum with labels (`'#0=(a b . #0#)`) is
/// a legitimate macro argument, and Larceny's `base` suite hands `test`
/// several. So no pair or vector is entered twice. Cycles come only from
/// the reader, whose data holds symbols rather than identifiers, so a
/// revisited container contributes nothing.
fn picks_any_identifier(
    heap: &Heap,
    root: TaggedValue,
    picks: &impl Fn(&str, &ScopeSet) -> bool,
) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut pending = vec![root];
    while let Some(value) = pending.pop() {
        if !seen.insert(value.raw_bits()) {
            continue;
        }
        if !heap.is_source_identifier(value)
            && heap
                .get_identifier_data(value)
                .is_some_and(|(name, scopes)| picks(name, scopes))
        {
            return true;
        }
        if value.is_pair() {
            let (car, cdr) = heap.get_pair(value);
            pending.extend([cdr, car]);
        } else if value.is_vector() {
            pending.extend(heap.vector_slice(value));
        }
    }
    false
}

#[cfg(test)]
mod tests;
