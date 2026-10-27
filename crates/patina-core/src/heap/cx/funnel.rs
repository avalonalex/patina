//! The store funnel: the one way a value is written into a heap object
//! (`PRD/GC_PRD.md` §10, #703).
//!
//! A slot names a value field of a heap object: a pair's car or cdr, a
//! vector's element, a mutable cell's value, a record's field, a VM
//! closure's free variable, a promise's box, a parameter's value. A field
//! the census counts at more than one site has a slot for each: a
//! parameter's value after a converter, and the machines' own vector and
//! promise stores (#708). The trusted crates, which hold words, write through
//! [`Heap::store`] and [`Heap::try_store`].
//! [`Cx::store`] writes one; [`Cx::store_range`] and [`Cx::fill_range`]
//! write a run of a vector's elements (addition C4); [`Fresh`] takes the
//! initializing stores into an object no one else has seen yet.
//!
//! **Today it is a plain store.** The arena heap reports
//! [`BarrierKind::None`](crate::BarrierKind), so a store checks its holder
//! and writes, as `Heap`'s mutators do. It is the one place stage 7's
//! barrier and, under `threaded`, publication will go, so stores route
//! here (#708) rather than through `Heap` or a cloned `Rc` payload.
//!
//! **A store into the wrong object panics**, in every build: a slot of a
//! holder of another kind, or an index past its end. Checking the arguments
//! for a Scheme error is the caller's job, as it is before `Heap::set_car`.
//! A machine whose own check would repeat the funnel's asks it instead:
//! [`Heap::try_store`] answers `false` for a slot that does not exist, and
//! writes nothing.
//!
//! ```
//! use patina_core::{GcDriver, Instance, PairCar, Value, VectorElem, new_shared_heap};
//!
//! let heap = new_shared_heap();
//! let instance = Instance::default();
//! let mut driver = GcDriver::acquire(&heap).unwrap();
//! driver.mutate(&instance, |cx| {
//!     let pair = cx.alloc_pair(Value::NULL, Value::NULL);
//!     cx.store(PairCar(pair), Value::fixnum(1));
//!     assert_eq!(cx.car(pair), Value::fixnum(1));
//!
//!     let vector = cx.alloc_vector_fill(4, Value::FALSE);
//!     cx.store(VectorElem(vector, 3), Value::TRUE);
//!     cx.fill_range(vector, 0, 2, Value::fixnum(0));
//!     assert_eq!(cx.vector_ref(vector, 1), Value::fixnum(0));
//! });
//! ```

use super::Cx;
use crate::census::{self, Site};
use crate::heap::{Heap, PromiseState, check_storable};
use crate::tagged_value::TaggedValue;
use crate::value::Value;

/// A value field of a heap object, which [`Cx::store`] writes. Sealed: the
/// slot kinds are the ones below, and only the funnel can write one.
pub trait Slot<'gc>: sealed::Sealed {
    /// Write `v` into the slot, or, if it does not exist, write nothing and
    /// answer what it names. Callable only inside core, which alone can make
    /// the token: a slot is written through [`Cx::store`], or by the trusted
    /// crates through [`Heap::store`] and [`Heap::try_store`].
    #[doc(hidden)]
    fn write(self, heap: &mut Heap, v: TaggedValue, token: sealed::Token) -> sealed::Written;
}

mod sealed {
    pub trait Sealed {}

    /// The proof a write comes from the funnel: no code outside core can
    /// make one.
    pub struct Token(pub(super) ());

    /// A write's answer: `Err` names the slot that does not exist, for the
    /// panic of a store into it.
    pub type Written = Result<(), &'static str>;
}

/// A pair's car.
#[derive(Clone, Copy, Debug)]
pub struct PairCar<'gc>(pub Value<'gc>);

/// A pair's cdr.
#[derive(Clone, Copy, Debug)]
pub struct PairCdr<'gc>(pub Value<'gc>);

/// A vector's element, by index.
#[derive(Clone, Copy, Debug)]
pub struct VectorElem<'gc>(pub Value<'gc>, pub usize);

/// A mutable cell's value: a boxed variable's (the VM's `AllocCell`).
#[derive(Clone, Copy, Debug)]
pub struct CellValue<'gc>(pub Value<'gc>);

/// A record's field, by index.
#[derive(Clone, Copy, Debug)]
pub struct RecordField<'gc>(pub Value<'gc>, pub usize);

/// A VM closure's free variable, by slot: the fix-up of a closure that
/// captures itself.
#[derive(Clone, Copy, Debug)]
pub struct ClosureFree<'gc>(pub Value<'gc>, pub usize);

/// A promise's box: a store makes the promise forced, with the value stored.
#[derive(Clone, Copy, Debug)]
pub struct PromiseBox<'gc>(pub Value<'gc>);

/// A vector's element, stored by the VM's inline `vector-set!`:
/// [`VectorElem`]'s field, which the GC census counts apart, at
/// `vm_vector_set` (#708).
#[derive(Clone, Copy, Debug)]
pub struct MachineVectorElem<'gc>(pub Value<'gc>, pub usize);

/// A promise's box, stored by a machine's own `force` (the VM's
/// `force_stub`, the tree-walker's CPS `force`): [`PromiseBox`]'s field,
/// which the GC census counts apart, at `promise_force_vm` (#708).
#[derive(Clone, Copy, Debug)]
pub struct MachinePromiseBox<'gc>(pub Value<'gc>);

/// A parameter object's current value.
#[derive(Clone, Copy, Debug)]
pub struct ParamValue<'gc>(pub Value<'gc>);

/// A parameter object's current value, as its converter answered it:
/// [`ParamValue`]'s field, which the GC census counts apart, at
/// `parameter_install_converted` (`%parameter-set!` resumed, #706).
#[derive(Clone, Copy, Debug)]
pub struct ConvertedParamValue<'gc>(pub Value<'gc>);

impl sealed::Sealed for PairCar<'_> {}
impl sealed::Sealed for PairCdr<'_> {}
impl sealed::Sealed for VectorElem<'_> {}
impl sealed::Sealed for CellValue<'_> {}
impl sealed::Sealed for RecordField<'_> {}
impl sealed::Sealed for ClosureFree<'_> {}
impl sealed::Sealed for PromiseBox<'_> {}
impl sealed::Sealed for MachineVectorElem<'_> {}
impl sealed::Sealed for MachinePromiseBox<'_> {}
impl sealed::Sealed for ParamValue<'_> {}
impl sealed::Sealed for ConvertedParamValue<'_> {}

/// The panic of a store into a slot that does not exist.
#[cold]
#[inline(never)]
fn no_such_slot(slot: &str) -> ! {
    panic!("Cx::store: no such slot ({slot}); check the holder before storing")
}

impl<'gc> Slot<'gc> for PairCar<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        if !self.0.is_pair() {
            return Err("a pair's car of a non-pair");
        }
        heap.set_car(self.0.tagged(), v);
        Ok(())
    }
}

impl<'gc> Slot<'gc> for PairCdr<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        if !self.0.is_pair() {
            return Err("a pair's cdr of a non-pair");
        }
        heap.set_cdr(self.0.tagged(), v);
        Ok(())
    }
}

impl<'gc> Slot<'gc> for VectorElem<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        let VectorElem(vector, index) = self;
        write_vector_elem(heap, vector, index, v, Site::VectorSet)
    }
}

impl<'gc> Slot<'gc> for MachineVectorElem<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        let MachineVectorElem(vector, index) = self;
        write_vector_elem(heap, vector, index, v, Site::VmVectorSet)
    }
}

/// Store `v` into the vector `vector`'s element `index`, counted in the
/// census at `site`.
#[inline]
fn write_vector_elem(
    heap: &mut Heap,
    vector: Value<'_>,
    index: usize,
    v: TaggedValue,
    site: Site,
) -> sealed::Written {
    if !vector.is_vector() || index >= heap.vector_len(vector.tagged()) {
        return Err("a vector element of a non-vector, or past its end");
    }
    heap.vector_store(site, vector.tagged(), index, v);
    Ok(())
}

impl<'gc> Slot<'gc> for CellValue<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        if !heap.write_mutable_cell(self.0.tagged(), v) {
            return Err("a cell's value of a non-cell");
        }
        Ok(())
    }
}

impl<'gc> Slot<'gc> for RecordField<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        let RecordField(record, index) = self;
        let Some((_, fields)) = heap.get_record(record.tagged()) else {
            return Err("a record's field of a non-record");
        };
        let mut fields = fields.borrow_mut();
        let Some(field) = fields.get_mut(index) else {
            return Err("a record's field past its last");
        };
        check_storable(v, "record");
        census::store(Site::RecordSet, Some(record.tagged()), v);
        *field = v;
        Ok(())
    }
}

impl<'gc> Slot<'gc> for ClosureFree<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        let ClosureFree(closure, slot) = self;
        if !heap.set_vm_closure_free_var(closure.tagged(), slot, v) {
            return Err("a closure's free variable of a non-closure, or past its last");
        }
        Ok(())
    }
}

impl<'gc> Slot<'gc> for PromiseBox<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        write_promise(heap, self.0, v, Site::PromiseForcePrimitive)
    }
}

impl<'gc> Slot<'gc> for MachinePromiseBox<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        write_promise(heap, self.0, v, Site::PromiseForceVm)
    }
}

/// Make the promise `promise` forced, with `v`, counted in the census at
/// `site`.
#[inline]
fn write_promise(
    heap: &mut Heap,
    promise: Value<'_>,
    v: TaggedValue,
    site: Site,
) -> sealed::Written {
    let Some(cell) = heap.get_promise(promise.tagged()) else {
        return Err("a promise's box of a non-promise");
    };
    check_storable(v, "promise");
    census::store(site, Some(promise.tagged()), v);
    *cell.borrow_mut() = PromiseState::Forced(v);
    Ok(())
}

impl<'gc> Slot<'gc> for ParamValue<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        write_parameter(heap, self.0, v, Site::ParameterInstall)
    }
}

impl<'gc> Slot<'gc> for ConvertedParamValue<'gc> {
    #[inline]
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) -> sealed::Written {
        write_parameter(heap, self.0, v, Site::ParameterInstallConverted)
    }
}

/// Make `v` the current value of the parameter object `param`, counted in
/// the census at `site`.
#[inline]
fn write_parameter(
    heap: &mut Heap,
    param: Value<'_>,
    v: TaggedValue,
    site: Site,
) -> sealed::Written {
    let Some((values, _)) = heap.get_parameter(param.tagged()) else {
        return Err("a parameter's value of a non-parameter");
    };
    check_storable(v, "parameter");
    census::store(site, Some(param.tagged()), v);
    let mut values = values.borrow_mut();
    match values.last_mut() {
        Some(top) => *top = v,
        None => values.push(v),
    }
    drop(values);
    if let Some(which) = heap.standard_port_parameter(param.tagged()) {
        refresh_current_port(heap, which, v);
    }
    Ok(())
}

/// A standard port parameter's value just became `v`: the interpreter's
/// current-port slot `which`, which a port-less `display` reads, becomes
/// `v`'s port (#608). Every write of a parameter's value comes through the
/// funnel, so the slot is the port its parameter holds, always: it holds
/// the port by its `Rc`, never by its word, so a word reused by a later
/// object cannot make it answer the wrong port. The writers validate first
/// (the converters, and `%parameterize-swap!` for the three), so `v` is a
/// port facing the right way; a `v` that is no port leaves the slot alone.
#[cold]
#[inline(never)]
fn refresh_current_port(heap: &Heap, which: crate::CurrentPort, v: TaggedValue) {
    let port = heap.get_port(v).cloned();
    debug_assert!(
        port.is_some(),
        "a standard port parameter set to a non-port"
    );
    if let Some(port) = port {
        heap.instance().set_current_port(which, port);
    }
}

impl Heap {
    /// Write `v` into `slot`: the funnel [`Cx::store`] is, for the trusted
    /// crates, which make its values from the words they hold
    /// (`Value::from_tagged`): the machines' own `force` and the
    /// tree-walker's parameter call (#708). Panics as [`Cx::store`] does.
    #[inline]
    pub fn store<'gc, S: Slot<'gc>>(&mut self, slot: S, v: Value<'gc>) {
        if let Err(slot) = slot.write(self, v.tagged(), sealed::Token(())) {
            no_such_slot(slot);
        }
    }

    /// [`Heap::store`], for a slot that may not exist: `false`, with nothing
    /// written, if it does not. For a machine whose own check of the holder
    /// would repeat the funnel's: the VM's inline `vector-set!` and
    /// `WriteCell` (#708).
    #[inline]
    pub fn try_store<'gc, S: Slot<'gc>>(&mut self, slot: S, v: Value<'gc>) -> bool {
        slot.write(self, v.tagged(), sealed::Token(())).is_ok()
    }
}

impl<'gc> Cx<'gc> {
    /// Write `v` into `slot`: the funnel. See the [module docs](self).
    #[inline]
    pub fn store<S: Slot<'gc>>(&mut self, slot: S, v: Value<'gc>) {
        let written = slot.write(&mut self.heap.borrow_mut(), v.tagged(), sealed::Token(()));
        if let Err(slot) = written {
            no_such_slot(slot);
        }
    }

    /// Write `src` into the vector `vector`'s elements from `start`: one
    /// range entry under a barrier (C4). Panics if `vector` is no vector or
    /// the run passes its end.
    pub fn store_range(
        &mut self,
        vector: Value<'gc>,
        start: usize,
        src: impl ExactSizeIterator<Item = Value<'gc>>,
    ) {
        let mut heap = self.heap.borrow_mut();
        if !vector.is_vector() || start + src.len() > heap.vector_len(vector.tagged()) {
            no_such_slot("a vector range of a non-vector, or past its end");
        }
        for (i, v) in src.enumerate() {
            heap.vector_set(vector.tagged(), start + i, v.tagged());
        }
    }

    /// Write `v` into `len` of the vector `vector`'s elements from `start`:
    /// one range entry under a barrier (C4). Panics as
    /// [`Cx::store_range`] does.
    pub fn fill_range(&mut self, vector: Value<'gc>, start: usize, len: usize, v: Value<'gc>) {
        let mut heap = self.heap.borrow_mut();
        if !vector.is_vector() || start + len > heap.vector_len(vector.tagged()) {
            no_such_slot("a vector range of a non-vector, or past its end");
        }
        for i in start..start + len {
            heap.vector_set(vector.tagged(), i, v.tagged());
        }
    }

    /// Store into a pair's car: [`Cx::store`] with [`PairCar`].
    #[inline]
    pub fn set_car(&mut self, pair: Value<'gc>, v: Value<'gc>) {
        self.store(PairCar(pair), v);
    }

    /// Store into a pair's cdr: [`Cx::store`] with [`PairCdr`].
    #[inline]
    pub fn set_cdr(&mut self, pair: Value<'gc>, v: Value<'gc>) {
        self.store(PairCdr(pair), v);
    }

    /// Store into a vector's element: [`Cx::store`] with [`VectorElem`].
    #[inline]
    pub fn vector_set(&mut self, vector: Value<'gc>, index: usize, v: Value<'gc>) {
        self.store(VectorElem(vector, index), v);
    }

    /// A vector of `len` elements, each `fill`, to be initialized through
    /// the [`Fresh`] it answers before anything else is done with this
    /// context.
    pub fn fresh_vector(&mut self, len: usize, fill: Value<'gc>) -> Fresh<'_, 'gc> {
        let object = self.alloc_vector_fill(len, fill);
        Fresh { cx: self, object }
    }
}

/// An object just allocated and not yet seen by anyone, whose fields take
/// initializing stores: no barrier is owed for them, since the object is
/// young (`PRD/GC_PRD.md` §10). It holds the context's `&mut` borrow, so
/// nothing else is done with the context, and nothing else sees the object,
/// until [`Fresh::finish`] answers it.
pub struct Fresh<'m, 'gc> {
    cx: &'m mut Cx<'gc>,
    object: Value<'gc>,
}

impl<'gc> Fresh<'_, 'gc> {
    /// Initialize element `index`. Panics past the end.
    pub fn init(&mut self, index: usize, v: Value<'gc>) {
        let object = self.object.tagged();
        self.cx
            .heap
            .borrow_mut()
            .vector_init(object, index, v.tagged());
    }

    /// The initialized object.
    pub fn finish(self) -> Value<'gc> {
        self.object
    }
}
