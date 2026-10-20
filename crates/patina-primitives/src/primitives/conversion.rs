//! Number/String conversion primitives
//!
//! Implements R7RS conversion procedures:
//! - `number->string` - Convert number to string representation
//! - `string->number` - Parse string to number

use crate::registry::{PrimitiveFn, PrimitiveRegistry};
use num_bigint::BigInt;
use num_rational::BigRational;

use patina_core::{Cx, TaggedValue, Value};
use patina_runtime::Arity;
use patina_runtime::EvalError;

// ========== Argument Extraction Helpers ==========

/// Extract an integer (for radix) from a value
fn get_radix(tv: Value<'_>) -> Result<i64, EvalError> {
    if tv.is_fixnum() {
        return Ok(tv.as_fixnum_unchecked());
    }
    Err(EvalError::TypeError("radix must be an integer".to_string()))
}

/// Extract a string from a value
fn get_string<'gc>(cx: &Cx<'gc>, tv: Value<'gc>) -> Result<String, EvalError> {
    cx.get_string_contents(tv)
        .ok_or_else(|| EvalError::TypeError("expected string".to_string()))
}

// ========== Conversion Primitives ==========

/// (number->string z [radix])
fn number_to_string<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1-2".to_string(),
            actual: args.len(),
        });
    }

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
    } else if let Some(n) = cx.get_bigint(tv) {
        bigint_to_string(&n, radix)
    } else if let Some(r) = cx.get_rational(tv) {
        if radix == 10 {
            format!("{}", r)
        } else {
            let numer = bigint_to_string(r.numer(), radix);
            let denom = bigint_to_string(r.denom(), radix);
            format!("{}/{}", numer, denom)
        }
    } else if let Some(f) = cx.get_real(tv) {
        if radix != 10 {
            return Err(EvalError::InvalidSyntax(
                "inexact numbers can only be converted with radix 10".to_string(),
            ));
        }
        real_to_string(f)
    } else if let Some((real_tv, imag_tv)) = cx.get_complex(tv) {
        if radix != 10 && (!cx.is_exact_number(real_tv) || !cx.is_exact_number(imag_tv)) {
            return Err(EvalError::InvalidSyntax(
                "inexact numbers can only be converted with radix 10".to_string(),
            ));
        }
        complex_to_string(cx, real_tv, imag_tv, radix)
    } else {
        return Err(EvalError::TypeError(format!(
            "expected number, got {}",
            cx.type_name(tv)
        )));
    };

    Ok(cx.alloc_string(result))
}

/// (string->number string [radix])
fn string_to_number<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1-2".to_string(),
            actual: args.len(),
        });
    }

    let string = get_string(cx, args[0])?;
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
            Value::fixnum(n)
        } else {
            cx.alloc_bigint(num_bigint::BigInt::from(n))
        });
    }

    // The reader's number syntax is the definition of what string->number
    // accepts (R7RS 6.2.7); a second parser here had drifted — no infinities,
    // no complex numbers, `#e` through a double.
    let number = patina_frontend::Parser::number_from_str(&string, default_radix, cx);
    Ok(number.unwrap_or(Value::FALSE))
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

/// Use the same real spelling as datum writers and REPL output (#219).
fn real_to_string(f: f64) -> String {
    let mut result = String::new();
    patina_core::debug_format::format_real(f, &mut result);
    result
}

/// Format a numeric value as a string (for complex number parts)
fn number_str<'gc>(cx: &Cx<'gc>, tv: Value<'gc>, radix: i64) -> String {
    if tv.is_fixnum() {
        return integer_to_string(tv.as_fixnum_unchecked(), radix);
    }
    if let Some(n) = cx.get_bigint(tv) {
        return bigint_to_string(&n, radix);
    }
    if let Some(r) = cx.get_rational(tv) {
        if radix == 10 {
            return r.to_string();
        }
        return format!(
            "{}/{}",
            bigint_to_string(r.numer(), radix),
            bigint_to_string(r.denom(), radix)
        );
    }
    if let Some(f) = cx.get_real(tv) {
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

/// Convert complex number from its parts (preserves exactness in display)
fn complex_to_string<'gc>(cx: &Cx<'gc>, r: Value<'gc>, i: Value<'gc>, radix: i64) -> String {
    /// Exact ±1 only: `+i` reads back with an *exact* unit imaginary part, so
    /// spelling `(make-rectangular 0.0 1.0)` that way loses the inexactness —
    /// the same mistake as omitting an inexact zero real part. BigInt and
    /// Rational are included because `write` includes them, and the two
    /// writers must agree.
    fn is_exact_unit<'gc>(cx: &Cx<'gc>, tv: Value<'gc>, negative: bool) -> bool {
        let want = if negative { -1 } else { 1 };
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == want as i64;
        }
        if let Some(n) = cx.get_bigint(tv) {
            return n == num_bigint::BigInt::from(want);
        }
        if let Some(r) = cx.get_rational(tv) {
            return r == BigRational::from(num_bigint::BigInt::from(want));
        }
        false
    }
    let is_one = |tv: Value<'gc>| is_exact_unit(cx, tv, false);
    let is_neg_one = |tv: Value<'gc>| is_exact_unit(cx, tv, true);

    // A zero real part may be omitted only when it is *exact*: R7RS 7.1.1's
    // `<complex R>` reads a bare `<imaginary R>` as having an exact zero real
    // part, so writing `+2.0i` for `(make-rectangular 0.0 2.0)` reads back a
    // different number. R7RS 6.2.6 requires `(string->number (number->string
    // z))` to be equivalent to `z`, so the inexact zero is written out.
    // `Heap::is_exact_zero` is this question already, and asking it rather
    // than "is zero and not a boxed f64" keeps working if a flonum ever
    // becomes an immediate.
    let real_zero_is_exact = cx.is_exact_zero(r);
    let imag_zero_is_exact = cx.is_exact_zero(i);

    if imag_zero_is_exact {
        number_str(cx, r, radix)
    } else if real_zero_is_exact {
        if is_one(i) {
            "+i".to_string()
        } else if is_neg_one(i) {
            "-i".to_string()
        } else {
            // The imaginary part must carry an explicit sign — `2.0i` is not a
            // number at all, and `string->number` rightly rejected what this
            // used to produce, so `number->string` did not round-trip.
            format!("{}i", signed(number_str(cx, i, radix)))
        }
    } else if is_one(i) {
        format!("{}+i", number_str(cx, r, radix))
    } else if is_neg_one(i) {
        format!("{}-i", number_str(cx, r, radix))
    } else {
        // `signed` here too, not just in the pure-imaginary arm: a positive
        // infinity or NaN formats with its own leading `+`, so concatenating
        // another produced `1.0++inf.0i`, which is not a number.
        format!(
            "{}{}i",
            number_str(cx, r, radix),
            signed(number_str(cx, i, radix))
        )
    }
}

/// Register conversion primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "number->string",
        Arity::Range(1, 2),
        "(number->string z [radix]) - Convert number to string in given radix (2, 8, 10, or 16)",
        number_to_string,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string->number",
        Arity::Range(1, 2),
        "(string->number string [radix]) - Parse string to number in given radix. Returns #f if invalid.",
        string_to_number,
    ));
}
