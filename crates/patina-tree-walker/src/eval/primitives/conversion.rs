//! Number/String conversion primitives
//!
//! Implements R7RS conversion procedures:
//! - `number->string` - Convert number to string representation
//! - `string->number` - Parse string to number

use crate::eval::primitives::registry::{PrimitiveFn, PrimitiveRegistry};
use crate::eval::{EvalError, EvalResult, Evaluator};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Num, ToPrimitive};
use patina_core::TaggedValue;
use patina_runtime::Arity;

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
fn number_to_string(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<EvalResult, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1-2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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
    Ok(EvalResult::Tagged(tagged))
}

/// (string->number string [radix])
fn string_to_number(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<EvalResult, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1-2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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

    let trimmed = string.trim();
    if trimmed.is_empty() {
        return Ok(EvalResult::Tagged(TaggedValue::FALSE));
    }

    // Drop the immutable borrow before getting mutable borrow for allocation
    drop(heap_ref);
    match parse_number_string(trimmed, default_radix, &mut heap.borrow_mut()) {
        Some(tagged) => Ok(EvalResult::Tagged(tagged)),
        None => Ok(EvalResult::Tagged(TaggedValue::FALSE)),
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

/// Check if a numeric TaggedValue is zero
fn tagged_is_zero(tv: TaggedValue, heap: &patina_core::Heap) -> bool {
    use num_traits::Zero;
    if tv.is_fixnum() {
        return tv.as_fixnum_unchecked() == 0;
    }
    if let Some(n) = heap.get_bigint(tv) {
        return n.sign() == num_bigint::Sign::NoSign;
    }
    if let Some(r) = heap.get_rational(tv) {
        return r.is_zero();
    }
    if let Some(f) = heap.get_real(tv) {
        return f == 0.0;
    }
    false
}

/// Check if a numeric TaggedValue is negative
fn tagged_is_negative(tv: TaggedValue, heap: &patina_core::Heap) -> bool {
    if tv.is_fixnum() {
        return tv.as_fixnum_unchecked() < 0;
    }
    if let Some(n) = heap.get_bigint(tv) {
        return n.sign() == num_bigint::Sign::Minus;
    }
    if let Some(r) = heap.get_rational(tv) {
        use num_traits::Zero;
        return r < &BigRational::zero();
    }
    if let Some(f) = heap.get_real(tv) {
        return f < 0.0;
    }
    false
}

/// Convert complex number from TaggedValue parts (preserves exactness in display)
fn complex_to_string_tagged(r: TaggedValue, i: TaggedValue, heap: &patina_core::Heap) -> String {
    fn is_one(tv: TaggedValue, heap: &patina_core::Heap) -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == 1;
        }
        if let Some(f) = heap.get_real(tv) {
            return f == 1.0;
        }
        false
    }

    fn is_neg_one(tv: TaggedValue, heap: &patina_core::Heap) -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == -1;
        }
        if let Some(f) = heap.get_real(tv) {
            return f == -1.0;
        }
        false
    }

    if tagged_is_zero(i, heap) {
        tagged_number_str(r, heap)
    } else if tagged_is_zero(r, heap) {
        if is_one(i, heap) {
            "+i".to_string()
        } else if is_neg_one(i, heap) {
            "-i".to_string()
        } else {
            format!("{}i", tagged_number_str(i, heap))
        }
    } else if is_one(i, heap) {
        format!("{}+i", tagged_number_str(r, heap))
    } else if is_neg_one(i, heap) {
        format!("{}-i", tagged_number_str(r, heap))
    } else if tagged_is_negative(i, heap) {
        format!(
            "{}{}i",
            tagged_number_str(r, heap),
            tagged_number_str(i, heap)
        )
    } else {
        format!(
            "{}+{}i",
            tagged_number_str(r, heap),
            tagged_number_str(i, heap)
        )
    }
}

/// Parse a number string with the given radix, returning a TaggedValue
fn parse_number_string(s: &str, radix: u32, heap: &mut patina_core::Heap) -> Option<TaggedValue> {
    // Handle prefixes (#b, #o, #d, #x, #e, #i)
    let (s, actual_radix, exactness) = parse_number_prefix(s, radix);

    // Try to parse as different number types
    // First, try as integer
    if let Some(val) = try_parse_integer(s, actual_radix, heap) {
        if let Some(false) = exactness {
            // Convert to inexact
            let f = if val.is_fixnum() {
                val.as_fixnum_unchecked() as f64
            } else {
                heap.get_bigint(val)
                    .map(|n| n.to_f64().unwrap_or(f64::INFINITY))
                    .unwrap_or(0.0)
            };
            return Some(heap.alloc_real(f));
        }
        return Some(val);
    }

    // If radix is 10, try as floating point
    if actual_radix == 10 {
        if let Ok(f) = s.parse::<f64>() {
            return Some(heap.alloc_real(f));
        }

        // Try parsing rational (e.g., "3/4")
        if s.contains('/') {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 2
                && let (Ok(numer), Ok(denom)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>())
                && denom != 0
            {
                return Some(
                    heap.alloc_rational(BigRational::new(BigInt::from(numer), BigInt::from(denom))),
                );
            }
        }
    }

    None
}

/// Parse number prefix and return (remaining_string, radix, exactness)
/// exactness: None = unspecified, Some(true) = exact, Some(false) = inexact
fn parse_number_prefix(s: &str, default_radix: u32) -> (&str, u32, Option<bool>) {
    let mut s = s;
    let mut radix = default_radix;
    let mut exactness = None;

    // Parse up to 2 prefixes
    for _ in 0..2 {
        if s.starts_with("#b") || s.starts_with("#B") {
            radix = 2;
            s = &s[2..];
        } else if s.starts_with("#o") || s.starts_with("#O") {
            radix = 8;
            s = &s[2..];
        } else if s.starts_with("#d") || s.starts_with("#D") {
            radix = 10;
            s = &s[2..];
        } else if s.starts_with("#x") || s.starts_with("#X") {
            radix = 16;
            s = &s[2..];
        } else if s.starts_with("#e") || s.starts_with("#E") {
            exactness = Some(true);
            s = &s[2..];
        } else if s.starts_with("#i") || s.starts_with("#I") {
            exactness = Some(false);
            s = &s[2..];
        } else {
            break;
        }
    }

    (s, radix, exactness)
}

/// Try to parse as integer in the given radix, returning a TaggedValue
fn try_parse_integer(s: &str, radix: u32, heap: &mut patina_core::Heap) -> Option<TaggedValue> {
    // Try i64 first
    if let Ok(n) = i64::from_str_radix(s, radix) {
        if TaggedValue::fits_fixnum(n) {
            return Some(TaggedValue::fixnum(n));
        } else {
            return Some(heap.alloc_bigint(BigInt::from(n)));
        }
    }

    // Try BigInt
    if let Ok(n) = BigInt::from_str_radix(s, radix) {
        return Some(heap.alloc_bigint(n));
    }

    None
}

/// Register conversion primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "number->string",
        Arity::Range(1, 2),
        "(number->string z [radix]) - Convert number to string in given radix (2, 8, 10, or 16)",
        |eval, args, _tail| number_to_string(eval, args),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "string->number",
        Arity::Range(1, 2),
        "(string->number string [radix]) - Parse string to number in given radix. Returns #f if invalid.",
        |eval, args, _tail| string_to_number(eval, args),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_to_string_decimal() {
        assert_eq!(integer_to_string(100, 10), "100");
    }

    #[test]
    fn test_number_to_string_hex() {
        assert_eq!(integer_to_string(256, 16), "100");
    }

    #[test]
    fn test_string_to_number_decimal() {
        let heap = patina_core::new_shared_heap();
        let result = parse_number_string("100", 10, &mut heap.borrow_mut());
        assert_eq!(result, Some(TaggedValue::fixnum(100)));
    }

    #[test]
    fn test_string_to_number_hex() {
        let heap = patina_core::new_shared_heap();
        let result = parse_number_string("100", 16, &mut heap.borrow_mut());
        assert_eq!(result, Some(TaggedValue::fixnum(256)));
    }

    #[test]
    fn test_string_to_number_invalid() {
        let heap = patina_core::new_shared_heap();
        let result = parse_number_string("not a number", 10, &mut heap.borrow_mut());
        assert!(result.is_none());
    }

    // Tests for scientific notation formatting (R7RS compliance)
    #[test]
    fn test_real_to_string_scientific_large() {
        // Very large numbers should use scientific notation
        assert_eq!(
            real_to_string(1.7976931348623157e308),
            "1.7976931348623157e+308"
        );
        assert_eq!(
            real_to_string(-1.7976931348623157e308),
            "-1.7976931348623157e+308"
        );
        assert_eq!(real_to_string(1e15), "1.0e+15");
        assert_eq!(real_to_string(1e100), "1.0e+100");
    }

    #[test]
    #[allow(clippy::excessive_precision)]
    fn test_real_to_string_scientific_small() {
        // Very small numbers should use scientific notation
        // Note: subnormal numbers may round during formatting
        let result = real_to_string(4.940656458412465e-324);
        assert!(
            result.contains("e-"),
            "Expected scientific notation for subnormal: {}",
            result
        );

        assert_eq!(real_to_string(1e-5), "1.0e-5");
        assert_eq!(real_to_string(1e-10), "1.0e-10");
    }

    #[test]
    fn test_real_to_string_normal_range() {
        // Normal range should use decimal notation
        assert_eq!(real_to_string(123.456), "123.456");
        assert_eq!(real_to_string(0.0001), "0.0001");
        assert_eq!(real_to_string(1e14), "100000000000000.0");
    }

    #[test]
    fn test_real_to_string_whole_numbers() {
        // Whole numbers should have .0 suffix to indicate inexactness
        assert_eq!(real_to_string(1.0), "1.0");
        assert_eq!(real_to_string(100.0), "100.0");
        assert_eq!(real_to_string(-42.0), "-42.0");
    }

    #[test]
    fn test_real_to_string_special_values() {
        // Special float values
        assert_eq!(real_to_string(f64::INFINITY), "+inf.0");
        assert_eq!(real_to_string(f64::NEG_INFINITY), "-inf.0");
        assert_eq!(real_to_string(f64::NAN), "+nan.0");
        assert_eq!(real_to_string(0.0), "0.0");
        assert_eq!(real_to_string(-0.0), "-0.0");
    }

    #[test]
    fn test_format_scientific_mantissa_decimal() {
        // Ensure scientific notation always includes decimal point
        // "5e-324" should become "5.0e-324"
        let s = format_scientific(5.0e-324);
        assert!(s.contains('.'), "Expected decimal point in mantissa: {}", s);
    }

    #[test]
    fn test_format_scientific_exponent_sign() {
        // Ensure exponent always has explicit sign
        let positive_exp = format_scientific(1e15);
        assert!(
            positive_exp.contains("e+"),
            "Expected explicit + in exponent: {}",
            positive_exp
        );

        let negative_exp = format_scientific(1e-15);
        assert!(
            negative_exp.contains("e-"),
            "Expected - in exponent: {}",
            negative_exp
        );
    }
}
