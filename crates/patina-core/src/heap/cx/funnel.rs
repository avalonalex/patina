//! The store funnel: the one way a value is written into a heap object
//! (`PRD/GC_PRD.md` §10, #703).
//!
//! A slot names a value field of a heap object: a pair's car or cdr, a
//! vector's element, a mutable cell's value, a record's field, a VM
//! closure's free variable, a promise's box, a parameter's value.
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
    /// Write `v` into the slot. Callable only inside core, which alone can
    /// make the token: a slot is written through [`Cx::store`].
    #[doc(hidden)]
    fn write(self, heap: &mut Heap, v: TaggedValue, token: sealed::Token);
}

mod sealed {
    pub trait Sealed {}

    /// The proof a write comes from the funnel: no code outside core can
    /// make one.
    pub struct Token(pub(super) ());
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

/// A parameter object's current value.
#[derive(Clone, Copy, Debug)]
pub struct ParamValue<'gc>(pub Value<'gc>);

impl sealed::Sealed for PairCar<'_> {}
impl sealed::Sealed for PairCdr<'_> {}
impl sealed::Sealed for VectorElem<'_> {}
impl sealed::Sealed for CellValue<'_> {}
impl sealed::Sealed for RecordField<'_> {}
impl sealed::Sealed for ClosureFree<'_> {}
impl sealed::Sealed for PromiseBox<'_> {}
impl sealed::Sealed for ParamValue<'_> {}

/// The panic of a store into a slot that does not exist.
#[cold]
#[inline(never)]
fn no_such_slot(slot: &str) -> ! {
    panic!("Cx::store: no such slot ({slot}); check the holder before storing")
}

impl<'gc> Slot<'gc> for PairCar<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        if !self.0.is_pair() {
            no_such_slot("a pair's car of a non-pair");
        }
        heap.set_car(self.0.tagged(), v);
    }
}

impl<'gc> Slot<'gc> for PairCdr<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        if !self.0.is_pair() {
            no_such_slot("a pair's cdr of a non-pair");
        }
        heap.set_cdr(self.0.tagged(), v);
    }
}

impl<'gc> Slot<'gc> for VectorElem<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        let VectorElem(vector, index) = self;
        if !vector.is_vector() || index >= heap.vector_len(vector.tagged()) {
            no_such_slot("a vector element of a non-vector, or past its end");
        }
        heap.vector_set(vector.tagged(), index, v);
    }
}

impl<'gc> Slot<'gc> for CellValue<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        if !heap.write_mutable_cell(self.0.tagged(), v) {
            no_such_slot("a cell's value of a non-cell");
        }
    }
}

impl<'gc> Slot<'gc> for RecordField<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        let RecordField(record, index) = self;
        let Some((_, fields)) = heap.get_record(record.tagged()) else {
            no_such_slot("a record's field of a non-record");
        };
        let mut fields = fields.borrow_mut();
        let Some(field) = fields.get_mut(index) else {
            no_such_slot("a record's field past its last");
        };
        check_storable(v, "record");
        census::store(Site::RecordSet, Some(record.tagged()), v);
        *field = v;
    }
}

impl<'gc> Slot<'gc> for ClosureFree<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        let ClosureFree(closure, slot) = self;
        if !heap.set_vm_closure_free_var(closure.tagged(), slot, v) {
            no_such_slot("a closure's free variable of a non-closure, or past its last");
        }
    }
}

impl<'gc> Slot<'gc> for PromiseBox<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        let Some(cell) = heap.get_promise(self.0.tagged()) else {
            no_such_slot("a promise's box of a non-promise");
        };
        check_storable(v, "promise");
        census::store(Site::PromiseForcePrimitive, Some(self.0.tagged()), v);
        *cell.borrow_mut() = PromiseState::Forced(v);
    }
}

impl<'gc> Slot<'gc> for ParamValue<'gc> {
    fn write(self, heap: &mut Heap, v: TaggedValue, _: sealed::Token) {
        let Some((values, _)) = heap.get_parameter(self.0.tagged()) else {
            no_such_slot("a parameter's value of a non-parameter");
        };
        check_storable(v, "parameter");
        census::store(Site::ParameterInstall, Some(self.0.tagged()), v);
        let mut values = values.borrow_mut();
        match values.last_mut() {
            Some(top) => *top = v,
            None => values.push(v),
        }
    }
}

impl<'gc> Cx<'gc> {
    /// Write `v` into `slot`: the funnel. See the [module docs](self).
    #[inline]
    pub fn store<S: Slot<'gc>>(&mut self, slot: S, v: Value<'gc>) {
        slot.write(&mut self.heap.borrow_mut(), v.tagged(), sealed::Token(()));
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
        let mut heap = self.cx.heap.borrow_mut();
        check_storable(v.tagged(), "vector");
        heap.vector_slice_mut(self.object.tagged())[index] = v.tagged();
    }

    /// The initialized object.
    pub fn finish(self) -> Value<'gc> {
        self.object
    }
}
