//! Number/String conversion primitives
//!
//! Implements R7RS conversion procedures:
//! - `number->string` - Convert number to string representation
//! - `string->number` - Parse string to number

use crate::registry::{PrimitiveFn, PrimitiveRegistry};
use num_bigint::BigInt;
use num_rational::BigRational;

use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

// ========== TaggedValue Extraction Helpers ==========

/// Extract an integer (for radix) from TaggedValue
fn get_radix(tv: TaggedValue) -> Result<i64, EvalError> {
    if tv.is_fixnum() {
        return Ok(tv.as_fixnum_unchecked());
    }
    Err(EvalError::TypeError("radix must be an integer".to_string()))
}

/// Extract a string from TaggedValue
fn get_string(
    tv: TaggedValue,
    heap: &std::cell::Ref<'_, patina_core::Heap>,
) -> Result<String, EvalError> {
    heap.get_string_contents(tv)
        .ok_or_else(|| EvalError::TypeError("expected string".to_string()))
}

// ========== Conversion Primitives ==========

/// (number->string z [radix])
fn number_to_string(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1-2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    let radix = if args.len() == 2 {
        get_radix(args[1])?
    } else {
        10
    };

    // Validate radix
    if ![2, 8, 10, 16].contains(&radix) {
        return Err(EvalError::InvalidSyntax(format!(
            "radix must be 2, 8, 10, or 16, got {}",
            radix
        )));
    }

    let tv = args[0];
    let result = if tv.is_fixnum() {
        integer_to_string(tv.as_fixnum_unchecked(), radix)
    } else if let Some(n) = heap_ref.get_bigint(tv) {
        bigint_to_string(n, radix)
    } else if let Some(r) = heap_ref.get_rational(tv) {
        if radix == 10 {
            format!("{}", r)
        } else {
            let numer = bigint_to_string(r.numer(), radix);
            let denom = bigint_to_string(r.denom(), radix);
            format!("{}/{}", numer, denom)
        }
    } else if let Some(f) = heap_ref.get_real(tv) {
        if radix != 10 {
            return Err(EvalError::InvalidSyntax(
                "inexact numbers can only be converted with radix 10".to_string(),
            ));
        }
        real_to_string(f)
    } else if let Some((real_tv, imag_tv)) = heap_ref.get_complex(tv) {
        if radix != 10 {
            return Err(EvalError::InvalidSyntax(
                "complex numbers can only be converted with radix 10".to_string(),
            ));
        }
        complex_to_string_tagged(real_tv, imag_tv, &heap_ref)
    } else {
        return Err(EvalError::TypeError(format!(
            "expected number, got {}",
            heap_ref.type_name(tv)
        )));
    };

    drop(heap_ref);
    let tagged = heap.borrow_mut().alloc_string(result);
    Ok(tagged)
}

/// (string->number string [radix])
fn string_to_number(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1-2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    let string = get_string(args[0], &heap_ref)?;
    let default_radix = if args.len() == 2 {
        get_radix(args[1])? as u32
    } else {
        10
    };

    // Validate radix
    if ![2, 8, 10, 16].contains(&default_radix) {
        return Err(EvalError::InvalidSyntax(format!(
            "radix must be 2, 8, 10, or 16, got {}",
            default_radix
        )));
    }

    drop(heap_ref);

    // Fast path: a plain decimal integer, the common case for anything that
    // tokenizes text, parsed without standing up a lexer. `[+-]?digits` is
    // exactly what fits — a leading '+' is allowed by i64::from_str.
    let plain_integer = default_radix == 10
        && string
            .strip_prefix(['+', '-'])
            .unwrap_or(&string)
            .bytes()
            .all(|b| b.is_ascii_digit());
    if let Some(n) = plain_integer.then(|| string.parse::<i64>().ok()).flatten() {
        return Ok(if TaggedValue::fits_fixnum(n) {
            TaggedValue::fixnum(n)
        } else {
            heap.borrow_mut().alloc_bigint(num_bigint::BigInt::from(n))
        });
    }

    // The reader's number syntax is the definition of what string->number
    // accepts (R7RS 6.2.7); a second parser here had drifted — no infinities,
    // no complex numbers, `#e` through a double.
    match patina_frontend::Parser::number_from_str(&string, default_radix, heap.clone()) {
        Some(tagged) => Ok(tagged),
        None => Ok(TaggedValue::FALSE),
    }
}

// ========== Helper Functions ==========

/// Convert an integer to string in the given radix
fn integer_to_string(n: i64, radix: i64) -> String {
    if n < 0 {
        // Handle negative numbers: format absolute value then add minus sign
        let abs_n = n.abs();
        let abs_str = match radix {
            2 => format!("{:b}", abs_n),
            8 => format!("{:o}", abs_n),
            10 => format!("{}", abs_n),
            16 => format!("{:x}", abs_n),
            _ => unreachable!(),
        };
        format!("-{}", abs_str)
    } else {
        match radix {
            2 => format!("{:b}", n),
            8 => format!("{:o}", n),
            10 => format!("{}", n),
            16 => format!("{:x}", n),
            _ => unreachable!(),
        }
    }
}

/// Convert a big integer to string in the given radix
fn bigint_to_string(n: &BigInt, radix: i64) -> String {
    match radix {
        2 => format!("{:b}", n),
        8 => format!("{:o}", n),
        10 => format!("{}", n),
        16 => format!("{:x}", n),
        _ => unreachable!(),
    }
}

/// Convert a real number to string for `number->string`
///
/// R7RS requires number->string to produce a result that, when read back,
/// produces an equivalent number. For extreme values, we use scientific
/// notation to ensure this property holds.
///
/// Note: Display/write formatting uses `patina_core::debug_format::format_real` instead.
/// This function differs by handling -0.0 and using scientific notation for extreme values.
fn real_to_string(f: f64) -> String {
    if f.is_infinite() {
        if f.is_sign_positive() {
            "+inf.0".to_string()
        } else {
            "-inf.0".to_string()
        }
    } else if f.is_nan() {
        "+nan.0".to_string()
    } else if f == 0.0 {
        // Handle zero explicitly (including negative zero)
        if f.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        }
    } else {
        let abs_f = f.abs();

        // Use scientific notation for:
        // - Numbers >= 1e15 (too many digits for decimal notation)
        // - Numbers < 1e-4 (too many leading zeros)
        // This matches chibi-scheme behavior
        if !(1e-4..1e15).contains(&abs_f) {
            // Format with scientific notation and normalize
            format_scientific(f)
        } else if f.fract() == 0.0 {
            // For whole numbers, ensure .0 suffix to indicate inexactness
            format!("{:.1}", f)
        } else {
            // Use Rust's default formatting for normal range floats
            format!("{}", f)
        }
    }
}

/// Format a number in scientific notation with R7RS-compatible output
///
/// Ensures:
/// - Explicit + sign on positive exponents (e.g., "1e+15" not "1e15")
/// - Decimal point in mantissa (e.g., "5.0e-324" not "5e-324")
fn format_scientific(f: f64) -> String {
    // Use full precision scientific notation
    let s = format!("{:e}", f);

    // Parse the result to normalize it
    if let Some(e_pos) = s.find('e') {
        let (mantissa, exp_part) = s.split_at(e_pos);

        // Ensure mantissa has a decimal point
        let mantissa = if !mantissa.contains('.') {
            format!("{}.0", mantissa)
        } else {
            mantissa.to_string()
        };

        // Ensure exponent has explicit sign
        let exp_str = &exp_part[1..]; // skip the 'e'
        let exp_with_sign = if !exp_str.starts_with('-') && !exp_str.starts_with('+') {
            format!("+{}", exp_str)
        } else {
            exp_str.to_string()
        };

        format!("{}e{}", mantissa, exp_with_sign)
    } else {
        // Fallback (shouldn't happen with {:e})
        s
    }
}

/// Format a numeric TaggedValue as a string (for complex number parts)
fn tagged_number_str(tv: TaggedValue, heap: &patina_core::Heap) -> String {
    if tv.is_fixnum() {
        return format!("{}", tv.as_fixnum_unchecked());
    }
    if let Some(n) = heap.get_bigint(tv) {
        return format!("{}", n);
    }
    if let Some(r) = heap.get_rational(tv) {
        return format!("{}", r);
    }
    if let Some(f) = heap.get_real(tv) {
        return real_to_string(f);
    }
    "???".to_string()
}

/// Prefix `+` unless the number already starts with a sign.
///
/// R7RS 7.1.1 spells an imaginary part `<sign> <ureal R> i`, so the sign is
/// part of the syntax rather than decoration: without it the result is not a
/// number and cannot be read back.
fn signed(s: String) -> String {
    if s.starts_with('+') || s.starts_with('-') {
        s
    } else {
        format!("+{s}")
    }
}

/// Convert complex number from TaggedValue parts (preserves exactness in display)
fn complex_to_string_tagged(r: TaggedValue, i: TaggedValue, heap: &patina_core::Heap) -> String {
    /// Exact ±1 only: `+i` reads back with an *exact* unit imaginary part, so
    /// spelling `(make-rectangular 0.0 1.0)` that way loses the inexactness —
    /// the same mistake as omitting an inexact zero real part. BigInt and
    /// Rational are included because `write` includes them, and the two
    /// writers must agree.
    fn is_exact_unit(tv: TaggedValue, heap: &patina_core::Heap, negative: bool) -> bool {
        let want = if negative { -1 } else { 1 };
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == want as i64;
        }
        if let Some(n) = heap.get_bigint(tv) {
            return n == &num_bigint::BigInt::from(want);
        }
        if let Some(r) = heap.get_rational(tv) {
            return r == &BigRational::from(num_bigint::BigInt::from(want));
        }
        false
    }
    let is_one = |tv: TaggedValue, heap: &patina_core::Heap| is_exact_unit(tv, heap, false);
    let is_neg_one = |tv: TaggedValue, heap: &patina_core::Heap| is_exact_unit(tv, heap, true);

    // A zero real part may be omitted only when it is *exact*: R7RS 7.1.1's
    // `<complex R>` reads a bare `<imaginary R>` as having an exact zero real
    // part, so writing `+2.0i` for `(make-rectangular 0.0 2.0)` reads back a
    // different number. R7RS 6.2.6 requires `(string->number (number->string
    // z))` to be equivalent to `z`, so the inexact zero is written out.
    // `Heap::is_exact_zero` is this question already, and asking it rather
    // than "is zero and not a boxed f64" keeps working if a flonum ever
    // becomes an immediate.
    let real_zero_is_exact = heap.is_exact_zero(r);
    let imag_zero_is_exact = heap.is_exact_zero(i);

    if imag_zero_is_exact {
        tagged_number_str(r, heap)
    } else if real_zero_is_exact {
        if is_one(i, heap) {
            "+i".to_string()
        } else if is_neg_one(i, heap) {
            "-i".to_string()
        } else {
            // The imaginary part must carry an explicit sign — `2.0i` is not a
            // number at all, and `string->number` rightly rejected what this
            // used to produce, so `number->string` did not round-trip.
            format!("{}i", signed(tagged_number_str(i, heap)))
        }
    } else if is_one(i, heap) {
        format!("{}+i", tagged_number_str(r, heap))
    } else if is_neg_one(i, heap) {
        format!("{}-i", tagged_number_str(r, heap))
    } else {
        // `signed` here too, not just in the pure-imaginary arm: a positive
        // infinity or NaN formats with its own leading `+`, so concatenating
        // another produced `1.0++inf.0i`, which is not a number.
        format!(
            "{}{}i",
            tagged_number_str(r, heap),
            signed(tagged_number_str(i, heap))
        )
    }
}

/// Register conversion primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "number->string",
        Arity::Range(1, 2),
        "(number->string z [radix]) - Convert number to string in given radix (2, 8, 10, or 16)",
        number_to_string,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "string->number",
        Arity::Range(1, 2),
        "(string->number string [radix]) - Parse string to number in given radix. Returns #f if invalid.",
        string_to_number,
    ));
}
