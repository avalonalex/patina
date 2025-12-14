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
use patina_runtime::{Arity, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Register conversion primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "number->string",
        Arity::Range(1, 2),
        "(number->string z [radix]) - Convert number to string in given radix (2, 8, 10, or 16)",
        |eval, args, _tail| eval.prim_number_to_string(args),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string->number",
        Arity::Range(1, 2),
        "(string->number string [radix]) - Parse string to number in given radix. Returns #f if invalid.",
        |eval, args, _tail| eval.prim_string_to_number(args),
    ));
}

impl Evaluator {
    /// (number->string z [radix])
    ///
    /// Converts a number to its string representation in the given radix.
    /// Radix must be 2, 8, 10, or 16. Defaults to 10.
    ///
    /// R7RS 6.2.6: The result never contains an explicit radix prefix.
    pub(super) fn prim_number_to_string(&self, args: Vec<Value>) -> Result<EvalResult, EvalError> {
        if args.is_empty() || args.len() > 2 {
            return Err(EvalError::WrongArity {
                expected: "1-2".to_string(),
                actual: args.len(),
            });
        }

        let number = &args[0];
        let radix = if args.len() == 2 {
            match &args[1] {
                Value::Integer(r) => *r,
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "radix must be an integer, got {}",
                        args[1]
                    )));
                }
            }
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

        let result = match number {
            Value::Integer(n) => self.integer_to_string(*n, radix),
            Value::BigInteger(n) => self.bigint_to_string(n, radix),
            Value::Rational(r) => {
                if radix == 10 {
                    format!("{}", r)
                } else {
                    // For non-decimal radices, convert numerator and denominator separately
                    let numer = self.bigint_to_string(r.numer(), radix);
                    let denom = self.bigint_to_string(r.denom(), radix);
                    format!("{}/{}", numer, denom)
                }
            }
            Value::Real(f) => {
                if radix != 10 {
                    return Err(EvalError::InvalidSyntax(
                        "inexact numbers can only be converted with radix 10".to_string(),
                    ));
                }
                self.real_to_string(*f)
            }
            Value::Complex(parts) => {
                if radix != 10 {
                    return Err(EvalError::InvalidSyntax(
                        "complex numbers can only be converted with radix 10".to_string(),
                    ));
                }
                let (ref r, ref i) = **parts;
                self.complex_to_string_values(r, i)
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "expected number, got {}",
                    number
                )));
            }
        };

        Ok(EvalResult::Value(Value::String(Rc::new(RefCell::new(
            result.chars().collect(),
        )))))
    }

    /// Convert an integer to string in the given radix
    fn integer_to_string(&self, n: i64, radix: i64) -> String {
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
    fn bigint_to_string(&self, n: &BigInt, radix: i64) -> String {
        match radix {
            2 => format!("{:b}", n),
            8 => format!("{:o}", n),
            10 => format!("{}", n),
            16 => format!("{:x}", n),
            _ => unreachable!(),
        }
    }

    /// Convert a real number to string
    ///
    /// R7RS requires number->string to produce a result that, when read back,
    /// produces an equivalent number. For extreme values, we use scientific
    /// notation to ensure this property holds.
    fn real_to_string(&self, f: f64) -> String {
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
                Self::format_scientific(f)
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

    /// Convert a complex number to string (old f64 interface)
    #[allow(dead_code)]
    fn complex_to_string(&self, r: f64, i: f64) -> String {
        if i == 0.0 {
            self.real_to_string(r)
        } else if r == 0.0 {
            if i == 1.0 {
                "+i".to_string()
            } else if i == -1.0 {
                "-i".to_string()
            } else {
                format!("{}i", self.real_to_string(i))
            }
        } else {
            let sign = if i >= 0.0 { "+" } else { "" };
            if i == 1.0 {
                format!("{}+i", self.real_to_string(r))
            } else if i == -1.0 {
                format!("{}-i", self.real_to_string(r))
            } else {
                format!(
                    "{}{}{}i",
                    self.real_to_string(r),
                    sign,
                    self.real_to_string(i)
                )
            }
        }
    }

    /// Convert complex number from Value parts (preserves exactness in display)
    fn complex_to_string_values(&self, r: &Value, i: &Value) -> String {
        use num_traits::Zero;

        // Helper to check if a value is zero
        fn is_zero(v: &Value) -> bool {
            match v {
                Value::Integer(n) => *n == 0,
                Value::BigInteger(n) => n.sign() == num_bigint::Sign::NoSign,
                Value::Rational(r) => r.is_zero(),
                Value::Real(f) => *f == 0.0,
                _ => false,
            }
        }

        // Helper to check if a value is one
        fn is_one(v: &Value) -> bool {
            match v {
                Value::Integer(n) => *n == 1,
                Value::Real(f) => *f == 1.0,
                _ => false,
            }
        }

        // Helper to check if a value is negative one
        fn is_neg_one(v: &Value) -> bool {
            match v {
                Value::Integer(n) => *n == -1,
                Value::Real(f) => *f == -1.0,
                _ => false,
            }
        }

        // Helper to check if value is negative
        fn is_negative(v: &Value) -> bool {
            match v {
                Value::Integer(n) => *n < 0,
                Value::BigInteger(n) => n.sign() == num_bigint::Sign::Minus,
                Value::Rational(r) => {
                    use num_traits::Zero;
                    r < &BigRational::zero()
                }
                Value::Real(f) => *f < 0.0,
                _ => false,
            }
        }

        // Helper to convert Value to string (for parts)
        fn value_str(v: &Value) -> String {
            format!("{}", v)
        }

        if is_zero(i) {
            value_str(r)
        } else if is_zero(r) {
            if is_one(i) {
                "+i".to_string()
            } else if is_neg_one(i) {
                "-i".to_string()
            } else {
                format!("{}i", value_str(i))
            }
        } else if is_one(i) {
            format!("{}+i", value_str(r))
        } else if is_neg_one(i) {
            format!("{}-i", value_str(r))
        } else if is_negative(i) {
            format!("{}{}i", value_str(r), value_str(i))
        } else {
            format!("{}+{}i", value_str(r), value_str(i))
        }
    }

    /// (string->number string [radix])
    ///
    /// Parses a string to a number in the given radix.
    /// Radix must be 2, 8, 10, or 16. Defaults to 10.
    /// Returns #f if the string is not a valid number.
    pub(super) fn prim_string_to_number(&self, args: Vec<Value>) -> Result<EvalResult, EvalError> {
        if args.is_empty() || args.len() > 2 {
            return Err(EvalError::WrongArity {
                expected: "1-2".to_string(),
                actual: args.len(),
            });
        }

        let string: String = match &args[0] {
            Value::String(s) => s.borrow().iter().collect(),
            _ => {
                return Err(EvalError::TypeError(format!(
                    "expected string, got {}",
                    args[0]
                )));
            }
        };

        let default_radix = if args.len() == 2 {
            match &args[1] {
                Value::Integer(r) => *r as u32,
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "radix must be an integer, got {}",
                        args[1]
                    )));
                }
            }
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
            return Ok(EvalResult::Value(Value::Boolean(false)));
        }

        // Try to parse as number using the lexer/parser
        // For now, use a simple implementation that handles common cases
        match self.parse_number_string(trimmed, default_radix) {
            Some(value) => Ok(EvalResult::Value(value)),
            None => Ok(EvalResult::Value(Value::Boolean(false))),
        }
    }

    /// Parse a number string with the given radix
    fn parse_number_string(&self, s: &str, radix: u32) -> Option<Value> {
        // Handle prefixes (#b, #o, #d, #x, #e, #i)
        let (s, actual_radix, exactness) = self.parse_number_prefix(s, radix);

        // Try to parse as different number types
        // First, try as integer
        if let Some(val) = self.try_parse_integer(s, actual_radix) {
            return Some(if let Some(false) = exactness {
                // Convert to inexact
                match val {
                    Value::Integer(n) => Value::Real(n as f64),
                    Value::BigInteger(n) => Value::Real(n.to_f64().unwrap_or(f64::INFINITY)),
                    _ => val,
                }
            } else {
                val
            });
        }

        // If radix is 10, try as floating point
        if actual_radix == 10 {
            if let Ok(f) = s.parse::<f64>() {
                return Some(Value::Real(f));
            }

            // Try parsing rational (e.g., "3/4")
            if s.contains('/') {
                let parts: Vec<&str> = s.split('/').collect();
                if parts.len() == 2
                    && let (Ok(numer), Ok(denom)) =
                        (parts[0].parse::<i64>(), parts[1].parse::<i64>())
                    && denom != 0
                {
                    return Some(Value::Rational(BigRational::new(
                        BigInt::from(numer),
                        BigInt::from(denom),
                    )));
                }
            }
        }

        None
    }

    /// Parse number prefix and return (remaining_string, radix, exactness)
    /// exactness: None = unspecified, Some(true) = exact, Some(false) = inexact
    fn parse_number_prefix<'a>(
        &self,
        s: &'a str,
        default_radix: u32,
    ) -> (&'a str, u32, Option<bool>) {
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

    /// Try to parse as integer in the given radix
    fn try_parse_integer(&self, s: &str, radix: u32) -> Option<Value> {
        // Try i64 first
        if let Ok(n) = i64::from_str_radix(s, radix) {
            return Some(Value::Integer(n));
        }

        // Try BigInt
        if let Ok(n) = BigInt::from_str_radix(s, radix) {
            return Some(Value::BigInteger(n));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_to_string_decimal() {
        let eval = Evaluator::new();
        let result = eval
            .prim_number_to_string(vec![Value::Integer(100)])
            .unwrap();
        match result {
            EvalResult::Value(Value::String(s)) => {
                assert_eq!(&*s.borrow(), &"100".chars().collect::<Vec<char>>());
            }
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_number_to_string_hex() {
        let eval = Evaluator::new();
        let result = eval
            .prim_number_to_string(vec![Value::Integer(256), Value::Integer(16)])
            .unwrap();
        match result {
            EvalResult::Value(Value::String(s)) => {
                assert_eq!(&*s.borrow(), &"100".chars().collect::<Vec<char>>());
            }
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_string_to_number_decimal() {
        let eval = Evaluator::new();
        let result = eval
            .prim_string_to_number(vec![Value::String(Rc::new(RefCell::new(
                "100".chars().collect(),
            )))])
            .unwrap();
        match result {
            EvalResult::Value(Value::Integer(100)) => {}
            _ => panic!("Expected 100, got {:?}", result),
        }
    }

    #[test]
    fn test_string_to_number_hex() {
        let eval = Evaluator::new();
        let result = eval
            .prim_string_to_number(vec![
                Value::String(Rc::new(RefCell::new("100".chars().collect()))),
                Value::Integer(16),
            ])
            .unwrap();
        match result {
            EvalResult::Value(Value::Integer(256)) => {}
            _ => panic!("Expected 256, got {:?}", result),
        }
    }

    #[test]
    fn test_string_to_number_invalid() {
        let eval = Evaluator::new();
        let result = eval
            .prim_string_to_number(vec![Value::String(Rc::new(RefCell::new(
                "not a number".chars().collect(),
            )))])
            .unwrap();
        match result {
            EvalResult::Value(Value::Boolean(false)) => {}
            _ => panic!("Expected #f, got {:?}", result),
        }
    }

    // Tests for scientific notation formatting (R7RS compliance)
    #[test]
    fn test_real_to_string_scientific_large() {
        let eval = Evaluator::new();
        // Very large numbers should use scientific notation
        assert_eq!(
            eval.real_to_string(1.7976931348623157e308),
            "1.7976931348623157e+308"
        );
        assert_eq!(
            eval.real_to_string(-1.7976931348623157e308),
            "-1.7976931348623157e+308"
        );
        assert_eq!(eval.real_to_string(1e15), "1.0e+15");
        assert_eq!(eval.real_to_string(1e100), "1.0e+100");
    }

    #[test]
    #[allow(clippy::excessive_precision)]
    fn test_real_to_string_scientific_small() {
        let eval = Evaluator::new();
        // Very small numbers should use scientific notation
        // Note: subnormal numbers may round during formatting
        let result = eval.real_to_string(4.940656458412465e-324);
        assert!(
            result.contains("e-"),
            "Expected scientific notation for subnormal: {}",
            result
        );

        assert_eq!(eval.real_to_string(1e-5), "1.0e-5");
        assert_eq!(eval.real_to_string(1e-10), "1.0e-10");
    }

    #[test]
    fn test_real_to_string_normal_range() {
        let eval = Evaluator::new();
        // Normal range should use decimal notation
        assert_eq!(eval.real_to_string(123.456), "123.456");
        assert_eq!(eval.real_to_string(0.0001), "0.0001");
        assert_eq!(eval.real_to_string(1e14), "100000000000000.0");
    }

    #[test]
    fn test_real_to_string_whole_numbers() {
        let eval = Evaluator::new();
        // Whole numbers should have .0 suffix to indicate inexactness
        assert_eq!(eval.real_to_string(1.0), "1.0");
        assert_eq!(eval.real_to_string(100.0), "100.0");
        assert_eq!(eval.real_to_string(-42.0), "-42.0");
    }

    #[test]
    fn test_real_to_string_special_values() {
        let eval = Evaluator::new();
        // Special float values
        assert_eq!(eval.real_to_string(f64::INFINITY), "+inf.0");
        assert_eq!(eval.real_to_string(f64::NEG_INFINITY), "-inf.0");
        assert_eq!(eval.real_to_string(f64::NAN), "+nan.0");
        assert_eq!(eval.real_to_string(0.0), "0.0");
        assert_eq!(eval.real_to_string(-0.0), "-0.0");
    }

    #[test]
    fn test_format_scientific_mantissa_decimal() {
        // Ensure scientific notation always includes decimal point
        // "5e-324" should become "5.0e-324"
        let s = Evaluator::format_scientific(5.0e-324);
        assert!(s.contains('.'), "Expected decimal point in mantissa: {}", s);
    }

    #[test]
    fn test_format_scientific_exponent_sign() {
        // Ensure exponent always has explicit sign
        let positive_exp = Evaluator::format_scientific(1e15);
        assert!(
            positive_exp.contains("e+"),
            "Expected explicit + in exponent: {}",
            positive_exp
        );

        let negative_exp = Evaluator::format_scientific(1e-15);
        assert!(
            negative_exp.contains("e-"),
            "Expected - in exponent: {}",
            negative_exp
        );
    }
}
