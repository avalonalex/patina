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
//! or `&mut [TaggedValue]` into the heap, but for the transitional
//! [`Cx::get_object`], whose object the datum writer reads until #706 gives
//! it a view of the kind.
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
//! A value cannot leave the call that made it:
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
//! lives, the driver it borrows cannot reach a safe point.
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

use std::cell::{Ref, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use num_bigint::BigInt;
use num_rational::BigRational;

use super::mutator::Mutator;
use super::{Heap, HeapObjectData, PromiseState};
use crate::compiled_macro::{CompiledMacro, LiteralId};
use crate::environment::Environment;
use crate::error::{ExceptionKind, SourceLocation};
use crate::instance::Instance;
use crate::port::Port;
use crate::record_type::RecordTypeDescriptor;
use crate::scope::ScopeSet;
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
    /// system and the library query.
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

    /// `string-fill!`'s write: `ch` into `range` of the string `s`. Panics if
    /// `s` is no string or the range exceeds it; the caller checks both first.
    /// It replaces `Heap::get_string_chars_mut`, which handed out
    /// `&mut [char]` into the heap.
    pub fn string_fill(&mut self, s: Value<'gc>, ch: char, range: std::ops::Range<usize>) {
        assert!(s.is_string(), "Cx::string_fill: not a string");
        self.heap.borrow_mut().get_string_chars_mut(s.tagged())[range].fill(ch);
    }

    /// `string-copy!`'s write: `chars` into the string `s` from `at`. Panics
    /// if `s` is no string or the chars run past its end; the caller checks
    /// both first.
    pub fn string_copy_chars(&mut self, s: Value<'gc>, at: usize, chars: &[char]) {
        assert!(s.is_string(), "Cx::string_copy_chars: not a string");
        self.heap.borrow_mut().get_string_chars_mut(s.tagged())[at..at + chars.len()]
            .copy_from_slice(chars);
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

    /// [`Heap::get_port`], cloned.
    #[inline]
    pub fn get_port(&self, v: Value<'gc>) -> Option<Rc<Port>> {
        self.heap.borrow().get_port(v.tagged()).cloned()
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

    /// Bind `name` in `env` to the primitive `library`'s `name`
    /// ([`Environment::define_primitive`]).
    #[inline]
    pub fn define_primitive(
        &mut self,
        env: &Environment,
        name: &'static str,
        arity: crate::procedure::Arity,
        library: Vec<String>,
    ) {
        env.define_primitive(self.heap, name, arity, library);
    }

    /// What `name` is bound to in `env` or its parents
    /// ([`Environment::get`]), as a value: read as
    /// [`Self::resolve_with_scopes`] reads a binding, which it is as live
    /// as.
    #[inline]
    pub fn lookup(&self, env: &Environment, name: &str) -> Option<Value<'gc>> {
        debug_assert!(
            env.heap_id() == self.heap.borrow().id(),
            "Cx::lookup: an environment of another heap"
        );
        env.get(name).map(|word| self.brand(word))
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
        let (word, selected) = env.resolve_with_scopes(name, scopes)?;
        Ok((word.map(|word| self.brand(word)), selected))
    }

    /// [`Environment::scoped_macro_binding`] on this context's heap.
    #[inline]
    pub fn scoped_macro_binding(
        &self,
        env: &Rc<Environment>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<(Rc<Environment>, ScopeSet)> {
        env.scoped_macro_binding(name, scopes, &self.heap.borrow())
    }

    /// [`Heap::source`], cloned.
    #[inline]
    pub fn source(&self, v: Value<'gc>) -> Option<SourceLocation> {
        self.heap.borrow().source(v.tagged()).cloned()
    }

    /// [`Heap::get_object`], borrowed. Transitional: the datum writer and the
    /// parser match on the object's kind, and its words are raw. #706
    /// replaces it with a view of the kind.
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
        let r = self
            .heap
            .borrow_mut()
            .alloc_record(rtd, Rc::new(RefCell::new(words)));
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
        let r = self.heap.borrow_mut().alloc_parameter(
            Rc::new(RefCell::new(vec![init.tagged()])),
            converter.map(Value::tagged),
        );
        self.brand(r)
    }

    /// Whether `v` is a parameter object.
    #[inline]
    pub fn is_parameter(&self, v: Value<'gc>) -> bool {
        self.heap.borrow().get_parameter(v.tagged()).is_some()
    }

    /// The current value of the parameter object `v`, or `None` if `v` is
    /// none. It is written through the funnel ([`ParamValue`]).
    #[inline]
    pub fn parameter_value(&self, v: Value<'gc>) -> Option<Value<'gc>> {
        let (values, _) = self.heap.borrow().get_parameter(v.tagged())?;
        let word = values
            .borrow()
            .last()
            .copied()
            .unwrap_or(TaggedValue::UNSPECIFIED);
        Some(self.brand(word))
    }

    /// The converter of the parameter object `v`: `None` if `v` is none,
    /// `Some(None)` if it has none.
    #[inline]
    pub fn parameter_converter(&self, v: Value<'gc>) -> Option<Option<Value<'gc>>> {
        let (_, converter) = self.heap.borrow().get_parameter(v.tagged())?;
        Some(converter.map(|w| self.brand(w)))
    }

    /// A promise in `state`.
    pub fn alloc_promise(&mut self, state: PromiseView<'gc>) -> Value<'gc> {
        let state = match state {
            PromiseView::Delayed(thunk) => PromiseState::Delayed(thunk.tagged()),
            PromiseView::Forced(value) => PromiseState::Forced(value.tagged()),
        };
        let r = self
            .heap
            .borrow_mut()
            .alloc_promise(Rc::new(RefCell::new(state)));
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
    /// It is the heap's own walk ([`Heap::map_syntax_identifiers`]) under
    /// one borrow, since it is the hottest walk the expander makes; neither
    /// function sees the heap.
    pub fn rescope_identifiers(
        &mut self,
        root: Value<'gc>,
        picks: impl Fn(&str, &ScopeSet) -> bool,
        rescope: impl Fn(&ScopeSet) -> ScopeSet,
    ) -> Value<'gc> {
        if !picks_any_identifier(&self.heap.borrow(), root.tagged(), &picks) {
            return root;
        }
        let word = self.heap.borrow_mut().map_syntax_identifiers(
            root.tagged(),
            |heap, value, name, scopes| {
                if heap.is_source_identifier(value) || !picks(&name, &scopes) {
                    return None;
                }
                Some(heap.alloc_identifier(name, rescope(&scopes)))
            },
        );
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
            if let Some((name, scopes)) = self.get_identifier_data_any(value) {
                if let Some(copy) = transform(self, value, name, scopes) {
                    if self.get_symbol_name(copy).is_none() {
                        self.inherit_source(value, copy);
                    }
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
                    self.inherit_source(parent, copy);
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
