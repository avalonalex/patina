//! Bitwise operations on exact integers (SRFI 151 core).
//!
//! Only the operations that cannot be expressed efficiently in Scheme live
//! here; everything derived from them is in `lib/srfi/151/bitwise.scm`. This
//! mirrors how the reference implementation splits the SRFI, and it is the
//! reason the library is bundled at all: written in portable Scheme, `bitwise-
//! and` on a bignum is unusably slow.
//!
//! Semantics are two's-complement over integers of unbounded width, which is
//! what SRFI 151 specifies — negative operands behave as if sign-extended
//! infinitely to the left. `num-bigint` models that for the bignum path;
//! all-fixnum calls take an allocation-free i64 path first, which agrees with
//! the unbounded-width semantics because these operations are closed over the
//! sign-extended fixnum range (see `fixnum_fold`). The SRFI 143 `fx*` ops
//! alias these primitives, so the fast path is what makes them "fast fixnum"
//! in practice rather than bignum-allocating (audit 2026-08-10, C1).

use crate::registry::PrimitiveRegistry;
use num_bigint::BigInt;
use patina_core::{Heap, SharedHeap, TaggedValue};
use patina_runtime::{Arity, EvalError};

type PrimResult = Result<TaggedValue, EvalError>;

/// Read an exact integer argument as a `BigInt`.
fn int_arg(name: &str, tv: TaggedValue, heap: &Heap) -> Result<BigInt, EvalError> {
    if let Some(n) = tv.as_fixnum() {
        return Ok(BigInt::from(n));
    }
    if tv.is_object()
        && let Some(b) = heap.get_bigint(tv)
    {
        return Ok(b.clone());
    }
    Err(EvalError::TypeError(format!(
        "{name}: expected an exact integer, got {}",
        patina_core::format_tagged(tv, heap)
    )))
}

/// Narrow back to a fixnum when it fits, so ordinary-sized results stay
/// immediate instead of allocating.
fn int_result(n: BigInt, heap: &SharedHeap) -> TaggedValue {
    match i64::try_from(&n) {
        Ok(small) if TaggedValue::fits_fixnum(small) => TaggedValue::fixnum(small),
        _ => heap.borrow_mut().alloc_bigint(n),
    }
}

/// Fold `fx` over all-fixnum arguments, immediately and allocation-free.
///
/// Sound for and/ior/xor because bitwise operations over sign-extended two's
/// complement are *closed* over any power-of-two signed range: the result's
/// bits above the operands' width are a function of the sign bits alone, so
/// for fixnum inputs the i64 result equals the infinite-precision result and
/// is itself a fixnum — no overflow check needed. `identity` is the
/// operator's identity element, so no first-argument special case is needed
/// either. Returns `None` on the first non-fixnum argument so the caller
/// falls back to the bignum fold.
fn fixnum_fold(
    identity: i64,
    args: &[TaggedValue],
    fx: fn(i64, i64) -> i64,
) -> Option<TaggedValue> {
    let mut acc = identity;
    for arg in args {
        acc = fx(acc, arg.as_fixnum()?);
    }
    Some(TaggedValue::fixnum(acc))
}

/// Fold an n-ary bitwise operator over its arguments.
fn fold(
    name: &'static str,
    identity: i64,
    args: &[TaggedValue],
    heap: &SharedHeap,
    fx: fn(i64, i64) -> i64,
    op: fn(BigInt, BigInt) -> BigInt,
) -> PrimResult {
    if let Some(result) = fixnum_fold(identity, args, fx) {
        return Ok(result);
    }
    let Some((first, rest)) = args.split_first() else {
        return Ok(TaggedValue::fixnum(identity));
    };
    let h = heap.borrow();
    let mut acc = int_arg(name, *first, &h)?;
    for arg in rest {
        acc = op(acc, int_arg(name, *arg, &h)?);
    }
    drop(h);
    Ok(int_result(acc, heap))
}

fn bitwise_and(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    fold("bitwise-and", -1, args, heap, |a, b| a & b, |a, b| a & b)
}

fn bitwise_ior(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    fold("bitwise-ior", 0, args, heap, |a, b| a | b, |a, b| a | b)
}

fn bitwise_xor(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    fold("bitwise-xor", 0, args, heap, |a, b| a ^ b, |a, b| a ^ b)
}

fn bitwise_not(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    // Closed over the fixnum range: !n = -n - 1 (see fixnum_fold).
    if let Some(n) = args[0].as_fixnum() {
        return Ok(TaggedValue::fixnum(!n));
    }
    let n = {
        let h = heap.borrow();
        int_arg("bitwise-not", args[0], &h)?
    };
    Ok(int_result(!n, heap))
}

/// `(arithmetic-shift n count)` — left when `count` is positive, right when
/// negative. The right shift is arithmetic (sign-propagating), which is what
/// two's-complement semantics require.
fn arithmetic_shift(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    // Fixnum fast path. Right shifts of a fixnum always shrink toward the
    // sign (a shift past the width leaves 0 or -1), so they never leave the
    // fixnum range. Left shifts go through i128 — a fixnum payload plus a
    // < 64-bit shift cannot overflow it — and keep the fast path only when
    // the result narrows back to a fixnum; anything wider falls through to
    // the bignum path below.
    if let Some(n) = args[0].as_fixnum()
        && let Some(count) = args[1].as_fixnum()
    {
        if count < 0 {
            let shift = (-count).min(63) as u32;
            return Ok(TaggedValue::fixnum(n >> shift));
        }
        if count < 64 {
            let wide = (n as i128) << count;
            if let Ok(small) = i64::try_from(wide)
                && TaggedValue::fits_fixnum(small)
            {
                return Ok(TaggedValue::fixnum(small));
            }
        }
    }
    let (n, count) = {
        let h = heap.borrow();
        let n = int_arg("arithmetic-shift", args[0], &h)?;
        let count = args[1].as_fixnum().ok_or_else(|| {
            EvalError::TypeError("arithmetic-shift: shift count must be a fixnum".to_string())
        })?;
        (n, count)
    };
    // num-bigint allocates the whole result up front, so an unguarded absurd
    // left-shift count ((expt 2 40) bits is ~137 GB) aborts the process in the
    // allocator instead of raising. Cap it at 2^30 bits — a 128 MiB integer,
    // far beyond any legitimate program — so the failure stays a catchable
    // Scheme error, consistent with the bignum-count rejection above. Right
    // shifts only ever shrink the operand and need no cap.
    const MAX_LEFT_SHIFT: i64 = 1 << 30;
    if count > MAX_LEFT_SHIFT {
        return Err(EvalError::TypeError(format!(
            "arithmetic-shift: shift count {count} exceeds the supported maximum of {MAX_LEFT_SHIFT}"
        )));
    }
    let shifted = if count >= 0 {
        n << (count as usize)
    } else {
        n >> ((-count) as usize)
    };
    Ok(int_result(shifted, heap))
}

/// Population count. For a negative argument SRFI 151 counts *zero* bits,
/// which keeps `(bit-count n)` finite despite the infinite sign extension.
fn bit_count(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    if let Some(n) = args[0].as_fixnum() {
        let counted = if n < 0 { !n } else { n };
        return Ok(TaggedValue::fixnum(i64::from(counted.count_ones())));
    }
    let n = {
        let h = heap.borrow();
        int_arg("bit-count", args[0], &h)?
    };
    // SRFI 151 counts zero bits for a negative argument, which is the same as
    // counting one bits of its complement -- finite despite the sign extension.
    let counted = if n.sign() == num_bigint::Sign::Minus {
        !n
    } else {
        n
    };
    let count = counted.magnitude().count_ones();
    Ok(TaggedValue::fixnum(count as i64))
}

/// Bits needed to represent `n`, excluding sign. `(integer-length 0)` is 0 and
/// `(integer-length -1)` is 0: both are all-zeros/all-ones under sign extension.
fn integer_length(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    if let Some(n) = args[0].as_fixnum() {
        let n = if n < 0 { !n } else { n };
        return Ok(TaggedValue::fixnum(i64::from(64 - n.leading_zeros())));
    }
    let n = {
        let h = heap.borrow();
        int_arg("integer-length", args[0], &h)?
    };
    let n = if n.sign() == num_bigint::Sign::Minus {
        !n
    } else {
        n
    };
    Ok(TaggedValue::fixnum(n.bits() as i64))
}

fn bit_set_p(heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    // Fast path: bits 60 and above of a fixnum (range ±2^60) are pure sign
    // extension, so clamping the index to 63 reads the sign bit, which is
    // the infinite-precision answer.
    if let Some(index) = args[0].as_fixnum()
        && index >= 0
        && let Some(n) = args[1].as_fixnum()
    {
        return Ok(TaggedValue::boolean((n >> index.min(63)) & 1 == 1));
    }
    let (index, n) = {
        let h = heap.borrow();
        let index = args[0]
            .as_fixnum()
            .ok_or_else(|| EvalError::TypeError("bit-set?: index must be a fixnum".to_string()))?;
        (index, int_arg("bit-set?", args[1], &h)?)
    };
    if index < 0 {
        return Err(EvalError::TypeError(
            "bit-set?: index must be non-negative".to_string(),
        ));
    }
    Ok(TaggedValue::boolean(n.bit(index as u64)))
}

/// Whether a value is a fixnum — an exact integer small enough to be immediate
/// rather than heap-allocated.
///
/// SRFI 143 needs this to derive `fx-width` from the representation instead of
/// hardcoding it, so the library cannot claim a range the tagging does not
/// actually provide.
fn is_fixnum(_heap: &SharedHeap, args: &[TaggedValue]) -> PrimResult {
    Ok(TaggedValue::boolean(args[0].is_fixnum()))
}

pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;

    // n-ary, with the identity SRFI 151 specifies for the empty case.
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "bitwise-and",
        Arity::Min(0),
        "Bitwise AND of its arguments; -1 with no arguments.",
        bitwise_and,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "bitwise-ior",
        Arity::Min(0),
        "Bitwise inclusive OR of its arguments; 0 with no arguments.",
        bitwise_ior,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "bitwise-xor",
        Arity::Min(0),
        "Bitwise exclusive OR of its arguments; 0 with no arguments.",
        bitwise_xor,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "bitwise-not",
        Arity::Exact(1),
        "Bitwise complement (two's complement, so (bitwise-not n) is -n-1).",
        bitwise_not,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "arithmetic-shift",
        Arity::Exact(2),
        "Shift left by count bits, or arithmetically right when count is negative.",
        arithmetic_shift,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "bit-count",
        Arity::Exact(1),
        "Count of 1 bits in a non-negative argument, or 0 bits in a negative one.",
        bit_count,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "integer-length",
        Arity::Exact(1),
        "Number of bits needed to represent the argument, excluding sign.",
        integer_length,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "fixnum?",
        Arity::Exact(1),
        "Whether the value is an immediate exact integer rather than a bignum.",
        is_fixnum,
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.bitwise",
        "bit-set?",
        Arity::Exact(2),
        "Whether bit index is set in the integer.",
        bit_set_p,
    ));
}
