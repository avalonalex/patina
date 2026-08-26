//! SRFI 124 ephemerons.
//!
//! An ephemeron holds a key and a datum. The collector holds the key *weakly*
//! and keeps the datum alive only for as long as the key is reachable by some
//! other path — so a datum that refers back to its own key does not keep the
//! pair alive, which is the whole point of the type and what a weak pair
//! cannot do. When the key does become unreachable the pair is *broken* and
//! both fields read as `#f`.
//!
//! The behaviour lives in the collector (`heap::gc`'s ephemeron fixpoint);
//! these procedures are the SRFI's surface over `HeapObjectData::Ephemeron`.

use crate::registry::{PrimitiveFn, PrimitiveRegistry};
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

pub fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "srfi.124",
        "make-ephemeron",
        Arity::Exact(2),
        "Returns an ephemeron with the given key and datum.",
        make_ephemeron,
    ));

    registry.register(PrimitiveFn::new_heap(
        "srfi.124",
        "ephemeron?",
        Arity::Exact(1),
        "Returns #t if obj is an ephemeron, #f otherwise.",
        ephemeron_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "srfi.124",
        "ephemeron-broken?",
        Arity::Exact(1),
        "Returns #t if the ephemeron's key has been collected, #f otherwise.",
        ephemeron_broken_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "srfi.124",
        "ephemeron-key",
        Arity::Exact(1),
        "Returns the ephemeron's key, or #f if it has been broken.",
        ephemeron_key,
    ));

    registry.register(PrimitiveFn::new_heap(
        "srfi.124",
        "ephemeron-datum",
        Arity::Exact(1),
        "Returns the ephemeron's datum, or #f if it has been broken.",
        ephemeron_datum,
    ));

    registry.register(PrimitiveFn::new_heap(
        "srfi.124",
        "reference-barrier",
        Arity::Exact(1),
        "Returns #t, having kept obj reachable up to this point.",
        reference_barrier,
    ));
}

fn arity_1(args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(args[0])
}

fn make_ephemeron(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    Ok(heap.borrow_mut().alloc_ephemeron(args[0], args[1]))
}

fn ephemeron_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let obj = arity_1(args)?;
    Ok(TaggedValue::boolean(heap.borrow().is_ephemeron(obj)))
}

/// A pair is broken exactly when the collector cleared it, which it signals by
/// setting the key to `#f`. SRFI 124 lets a key legitimately *be* `#f`, so
/// this asks the heap rather than comparing the key a caller could have
/// supplied — a pair made with a `#f` key is not broken until collected.
fn ephemeron_broken_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let obj = arity_1(args)?;
    let heap = heap.borrow();
    match heap.get_ephemeron(obj) {
        Some(_) => Ok(TaggedValue::boolean(heap.is_ephemeron_broken(obj))),
        None => Err(EvalError::TypeError(format!(
            "{}: expected an ephemeron, got {}",
            "ephemeron-broken?",
            heap.type_name(obj)
        ))),
    }
}

fn ephemeron_key(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let obj = arity_1(args)?;
    let heap = heap.borrow();
    match heap.get_ephemeron(obj) {
        Some((key, _)) => Ok(key),
        None => Err(EvalError::TypeError(format!(
            "{}: expected an ephemeron, got {}",
            "ephemeron-key",
            heap.type_name(obj)
        ))),
    }
}

fn ephemeron_datum(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let obj = arity_1(args)?;
    let heap = heap.borrow();
    match heap.get_ephemeron(obj) {
        Some((_, datum)) => Ok(datum),
        None => Err(EvalError::TypeError(format!(
            "{}: expected an ephemeron, got {}",
            "ephemeron-datum",
            heap.type_name(obj)
        ))),
    }
}

/// SRFI 124: "returns an unspecified value" after ensuring `obj` is still
/// reachable at the call. Patina's collector runs only at allocation points
/// and traces the live argument list, so an argument that reaches here is
/// reachable here; passing it through a primitive is the barrier.
fn reference_barrier(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    arity_1(args)?;
    Ok(TaggedValue::boolean(true))
}
