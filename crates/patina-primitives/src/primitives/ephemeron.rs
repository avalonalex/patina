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

use crate::registry::{PrimitiveFn, PrimitiveRegistry, expect_arity};
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
        "Keeps obj reachable up to this point; returns an unspecified value.",
        reference_barrier,
    ));
}

/// The three accessors' shared guard: one arity check, one heap lookup, and
/// one error whose text names the caller.
///
/// Returns the raw `Option` so `ephemeron-broken?` can answer from it directly
/// rather than looking the object up a second time.
fn as_ephemeron(
    heap: &SharedHeap,
    who: &str,
    args: &[TaggedValue],
) -> Result<Option<(TaggedValue, TaggedValue)>, EvalError> {
    expect_arity(args, 1)?;
    let obj = args[0];
    let heap = heap.borrow();
    if !heap.is_ephemeron(obj) {
        return Err(EvalError::TypeError(format!(
            "{who}: expected an ephemeron, got {}",
            heap.type_name(obj)
        )));
    }
    Ok(heap.ephemeron_contents(obj))
}

fn make_ephemeron(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    expect_arity(args, 2)?;
    Ok(heap.borrow_mut().alloc_ephemeron(args[0], args[1]))
}

fn ephemeron_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    expect_arity(args, 1)?;
    Ok(TaggedValue::boolean(heap.borrow().is_ephemeron(args[0])))
}

/// A pair is broken exactly when the collector cleared it, which it signals by
/// setting the key to `#f`. SRFI 124 lets a key legitimately *be* `#f`, so
/// this asks the heap rather than comparing the key a caller could have
/// supplied — a pair made with a `#f` key is not broken until collected.
fn ephemeron_broken_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let pair = as_ephemeron(heap, "ephemeron-broken?", args)?;
    Ok(TaggedValue::boolean(pair.is_none()))
}

fn ephemeron_key(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let pair = as_ephemeron(heap, "ephemeron-key", args)?;
    Ok(pair.map_or(TaggedValue::FALSE, |(key, _)| key))
}

fn ephemeron_datum(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let pair = as_ephemeron(heap, "ephemeron-datum", args)?;
    Ok(pair.map_or(TaggedValue::FALSE, |(_, datum)| datum))
}

/// SRFI 124: "returns an unspecified value" after ensuring `obj` is still
/// reachable at the call.
///
/// Passing the argument through a primitive is the barrier: it is in the live
/// argument vector across the call, which the collector traces. That is enough
/// today for a second reason too — the VM over-retains stale registers, which
/// `GC_STAGE5_PRD.md` §7's rooting work is meant to remove — so this should be
/// re-examined when that lands rather than assumed to keep holding.
fn reference_barrier(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    expect_arity(args, 1)?;
    Ok(TaggedValue::UNSPECIFIED)
}
