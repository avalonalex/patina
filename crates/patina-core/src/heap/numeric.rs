//! Numeric operations for the Heap
//!
//! This module contains all numeric operations that work directly on TaggedValues.
//! Each method operates on HeapObjectData types (BigInt, Rational, Real, Complex)
//! directly, with inline fixnum fast paths.
//!
//! Operations include:
//! - Basic arithmetic: add, sub, mul, div, neg
//! - Integer division: quotient, remainder, modulo, floor-quotient, floor-remainder
//! - Comparisons: =, <, <=, >, >=
//! - Complex number operations: real-part, imag-part, magnitude, angle, make-rectangular, make-polar
//! - Number theory: numerator, denominator, exact, inexact, gcd, lcm
//! - Rounding: floor, ceiling, truncate, round
//! - Transcendentals: sin, cos, tan, asin, acos, atan, atan2, exp, log, sqrt, expt

use super::{Heap, HeapObjectData};
use crate::numeric::NumericError;
use crate::tagged_value::TaggedValue;
use num_bigint::{BigInt, Sign};
use num_complex::Complex64;
use num_rational::BigRational;
use num_traits::{FromPrimitive, One, Pow, Signed, ToPrimitive, Zero};

// =============================================================================
// Private Data Types
// =============================================================================

/// Extracted numeric data from a TaggedValue (non-complex only).
/// Used for type dispatch in arithmetic operations.
enum NumData {
    Fixnum(i64),
    BigInt(BigInt),
    Rational(BigRational),
    Real(f64),
}

// =============================================================================
// Free Helper Functions
// =============================================================================

/// GCD for i64 using Euclidean algorithm
#[inline]
fn gcd_i64(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.abs()
}

/// GCD for BigInt using Euclidean algorithm
fn gcd_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    let mut a = a.abs();
    let mut b = b.abs();
    while !b.is_zero() {
        let t = b.clone();
        b = &a % &b;
        a = t;
    }
    a
}

/// Floor division for BigInt
fn floor_div_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    let q = a / b;
    let r = a % b;
    if !r.is_zero() && r.sign() != b.sign() {
        q - 1
    } else {
        q
    }
}

/// Banker's rounding for f64 (ties to even)
fn bankers_round(x: f64) -> f64 {
    let rounded = x.round();
    let frac = x - x.floor();
    let is_midpoint = (frac - 0.5).abs() < 1e-10;
    if is_midpoint {
        let floor_val = x.floor();
        let ceil_val = x.ceil();
        if floor_val % 2.0 == 0.0 {
            floor_val
        } else {
            ceil_val
        }
    } else {
        rounded
    }
}

/// Checked integer power for i64
fn checked_pow_i64(base: i64, exp: u64) -> Option<i64> {
    if exp > 62 {
        return None;
    }
    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result = result.checked_mul(b)?;
        }
        e >>= 1;
        if e > 0 {
            b = b.checked_mul(b)?;
        }
    }
    Some(result)
}

/// Floor for BigRational
fn floor_rational(r: &BigRational) -> BigInt {
    if r.is_integer() {
        r.numer().clone()
    } else if r.is_negative() {
        r.numer() / r.denom() - 1
    } else {
        r.numer() / r.denom()
    }
}

/// Ceiling for BigRational
fn ceiling_rational(r: &BigRational) -> BigInt {
    if r.is_integer() {
        r.numer().clone()
    } else if r.is_positive() {
        r.numer() / r.denom() + 1
    } else {
        r.numer() / r.denom()
    }
}

/// Round for BigRational (banker's rounding - ties to even)
fn round_rational(r: &BigRational) -> BigInt {
    let floored = floor_rational(r);
    let frac = r - BigRational::from(floored.clone());
    let half = BigRational::new(BigInt::from(1), BigInt::from(2));
    if frac < half {
        floored
    } else if frac > half {
        floored + 1
    } else {
        // Exactly half - round to even
        if &floored % 2 == BigInt::zero() {
            floored
        } else {
            floored + 1
        }
    }
}

/// Decode f64 into mantissa, exponent, and sign (for float→rational conversion)
fn decode_float(f: f64) -> (u64, i32, i32) {
    if f == 0.0 {
        return (0, 0, if f.is_sign_negative() { -1 } else { 1 });
    }
    let bits = f.to_bits();
    let sign = if (bits >> 63) != 0 { -1 } else { 1 };
    let exponent_bits = ((bits >> 52) & 0x7FF) as i32;
    let mantissa_bits = bits & 0x000F_FFFF_FFFF_FFFF;
    if exponent_bits == 0 {
        (mantissa_bits, -1074, sign)
    } else {
        let mantissa = mantissa_bits | 0x0010_0000_0000_0000;
        let exponent = exponent_bits - 1023 - 52;
        (mantissa, exponent, sign)
    }
}

/// Convert float to exact rational representation
fn float_to_exact_rational(f: f64) -> Result<BigRational, NumericError> {
    if f.is_nan() || f.is_infinite() {
        return Err(NumericError::NoExactRepresentation {
            value: format!("{}", f),
        });
    }
    let (mantissa, exponent, sign) = decode_float(f);
    let two = BigInt::from(2);
    let numer = BigInt::from(mantissa);
    let rational = if exponent >= 0 {
        let scale = two.pow(exponent as u32);
        BigRational::new(numer * scale, BigInt::one())
    } else {
        let scale = two.pow((-exponent) as u32);
        BigRational::new(numer, scale)
    };
    if sign < 0 {
        Ok(-rational)
    } else {
        Ok(rational)
    }
}

// =============================================================================
// Heap Numeric Implementation
// =============================================================================

impl Heap {
    // =========================================================================
    // Predicate Helpers
    // =========================================================================

    /// Check if a numeric value is NaN
    #[inline]
    pub fn is_numeric_nan(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return false;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::Real(f) => f.is_nan(),
            HeapObjectData::Complex { real, imag } => {
                self.is_numeric_nan(*real) || self.is_numeric_nan(*imag)
            }
            _ => false,
        }
    }

    /// Check if a numeric value is zero (exact or inexact)
    ///
    /// Total: returns `false` for non-numbers. NOT a `zero?` implementation —
    /// the primitive must raise on non-numbers (see `numeric_eq_cmp`).
    #[inline]
    pub fn is_numeric_zero_tv(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == 0;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => n.sign() == Sign::NoSign,
            HeapObjectData::Rational(r) => r.is_zero(),
            HeapObjectData::Real(f) => *f == 0.0,
            HeapObjectData::Complex { real, imag } => {
                self.is_numeric_zero_tv(*real) && self.is_numeric_zero_tv(*imag)
            }
            _ => false,
        }
    }

    /// Check if a numeric value is negative (only meaningful for real numbers)
    ///
    /// Total: returns `false` for non-numbers. NOT a `negative?`
    /// implementation — the primitive must raise on non-numbers (see
    /// `numeric_lt`).
    #[inline]
    pub fn is_numeric_negative_tv(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() < 0;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => n.sign() == Sign::Minus,
            HeapObjectData::Rational(r) => r.is_negative(),
            HeapObjectData::Real(f) => *f < 0.0,
            _ => false,
        }
    }

    /// Convert a numeric TaggedValue to f64 (rejects Complex)
    #[inline]
    pub fn numeric_to_f64(&self, tv: TaggedValue) -> Result<f64, NumericError> {
        if tv.is_fixnum() {
            return Ok(tv.as_fixnum_unchecked() as f64);
        }
        if !tv.is_object() {
            return Err(NumericError::NotReal {
                got: "non-numeric".into(),
            });
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => Ok(n.to_f64().unwrap_or(f64::INFINITY)),
            HeapObjectData::Rational(r) => Ok(r.to_f64().unwrap_or(f64::INFINITY)),
            HeapObjectData::Real(f) => Ok(*f),
            _ => Err(NumericError::NotReal {
                got: "complex or non-numeric".into(),
            }),
        }
    }

    /// Check if a TaggedValue is a Real (f64) heap object (not fixnum, not BigInt, etc.)
    #[inline]
    fn is_real_object(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return false;
        }
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Real(_))
    }

    // =========================================================================
    // Conversion Helpers
    // =========================================================================

    /// Convert BigInt to tagged, using fixnum if it fits
    #[inline]
    fn bigint_to_tagged(&mut self, n: BigInt) -> TaggedValue {
        if let Some(i) = n.to_i64()
            && TaggedValue::fits_fixnum(i)
        {
            return TaggedValue::fixnum(i);
        }
        self.alloc_bigint(n)
    }

    /// Convert BigRational to tagged, simplifying to integer if possible
    #[inline]
    fn rational_to_tagged(&mut self, r: BigRational) -> TaggedValue {
        if r.is_integer() {
            return self.bigint_to_tagged(r.numer().clone());
        }
        self.alloc_rational(r)
    }

    /// Convert Complex64 to tagged, simplifying if imaginary is near zero
    #[inline]
    fn complex64_to_tagged(&mut self, c: Complex64) -> TaggedValue {
        if c.im.abs() < 1e-15 {
            self.alloc_real(c.re)
        } else {
            let real = self.alloc_real(c.re);
            let imag = self.alloc_real(c.im);
            self.alloc_complex(real, imag)
        }
    }

    /// Simplify complex: if imag is exact zero return real; if imag is inexact
    /// zero return real as inexact; otherwise alloc complex.
    fn simplify_complex_tagged(&mut self, real: TaggedValue, imag: TaggedValue) -> TaggedValue {
        if self.is_exact_zero(imag) {
            return real;
        }
        if let Some(f) = self.get_real(imag)
            && f == 0.0
        {
            if self.is_inexact_number(real) {
                return real;
            }
            return self.to_inexact_tagged(real);
        }
        self.alloc_complex(real, imag)
    }

    /// Convert exact to inexact (Real)
    #[allow(clippy::wrong_self_convention)]
    fn to_inexact_tagged(&mut self, tv: TaggedValue) -> TaggedValue {
        if tv.is_fixnum() {
            return self.alloc_real(tv.as_fixnum_unchecked() as f64);
        }
        if !tv.is_object() {
            return tv;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => {
                let f = n.to_f64().unwrap_or(f64::INFINITY);
                self.alloc_real(f)
            }
            HeapObjectData::Rational(r) => {
                let f = r.to_f64().unwrap_or(f64::INFINITY);
                self.alloc_real(f)
            }
            HeapObjectData::Real(_) => tv,
            HeapObjectData::Complex { real, imag } => {
                let (r, i) = (*real, *imag);
                let ir = self.to_inexact_tagged(r);
                let ii = self.to_inexact_tagged(i);
                self.alloc_complex(ir, ii)
            }
            _ => tv,
        }
    }

    /// Apply inexact contagion: if `inexact` is true and result is exact, convert
    fn apply_contagion_tagged(&mut self, result: TaggedValue, inexact: bool) -> TaggedValue {
        if !inexact {
            return result;
        }
        if self.is_inexact_number(result) {
            return result;
        }
        self.to_inexact_tagged(result)
    }

    // =========================================================================
    // Data Extraction Helpers
    // =========================================================================

    /// Extract non-complex numeric data from a TaggedValue (clones BigInt/Rational)
    fn extract_num_data(&self, tv: TaggedValue) -> Result<NumData, NumericError> {
        if tv.is_fixnum() {
            return Ok(NumData::Fixnum(tv.as_fixnum_unchecked()));
        }
        if !tv.is_object() {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => Ok(NumData::BigInt(n.clone())),
            HeapObjectData::Rational(r) => Ok(NumData::Rational(r.clone())),
            HeapObjectData::Real(f) => Ok(NumData::Real(*f)),
            _ => Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            }),
        }
    }

    /// Extract BigInt from a TaggedValue (for integer division operations)
    fn extract_bigint(&self, tv: TaggedValue) -> Result<BigInt, NumericError> {
        if tv.is_fixnum() {
            return Ok(BigInt::from(tv.as_fixnum_unchecked()));
        }
        if !tv.is_object() {
            return Err(NumericError::NotInteger {
                got: "non-numeric".into(),
            });
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => Ok(n.clone()),
            HeapObjectData::Rational(r) if r.is_integer() => Ok(r.numer().clone()),
            HeapObjectData::Real(f) if f.fract() == 0.0 && f.is_finite() => {
                Ok(BigInt::from(*f as i64))
            }
            _ => Err(NumericError::NotInteger {
                got: "non-integer".into(),
            }),
        }
    }

    /// Extract integer value from exponent if it's an exact integer
    fn exponent_as_integer_tv(&self, tv: TaggedValue) -> Option<i64> {
        if tv.is_fixnum() {
            return Some(tv.as_fixnum_unchecked());
        }
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => n.to_i64(),
            HeapObjectData::Rational(r) if r.is_integer() => r.numer().to_i64(),
            _ => None,
        }
    }

    /// Convert to BigRational for exact comparisons
    fn to_bigrational(&self, tv: TaggedValue) -> Result<BigRational, NumericError> {
        if tv.is_fixnum() {
            return Ok(BigRational::from(BigInt::from(tv.as_fixnum_unchecked())));
        }
        if !tv.is_object() {
            return Err(NumericError::NotReal {
                got: "non-numeric".into(),
            });
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => Ok(BigRational::from(n.clone())),
            HeapObjectData::Rational(r) => Ok(r.clone()),
            _ => Err(NumericError::NotReal {
                got: "non-exact-real".into(),
            }),
        }
    }

    // =========================================================================
    // Numeric Comparisons
    // =========================================================================

    /// Check if a == b numerically (handles NaN correctly)
    pub fn numeric_eq_cmp(&self, a: TaggedValue, b: TaggedValue) -> Result<bool, NumericError> {
        // Fast path: both fixnums
        if a.is_fixnum() && b.is_fixnum() {
            return Ok(a.as_fixnum_unchecked() == b.as_fixnum_unchecked());
        }

        // NaN is never equal to anything
        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(false);
        }

        self.numeric_eq_impl(a, b)
    }

    /// Equality implementation (no NaN check)
    fn numeric_eq_impl(&self, a: TaggedValue, b: TaggedValue) -> Result<bool, NumericError> {
        // Handle complex cases
        let a_complex = self.get_complex(a);
        let b_complex = self.get_complex(b);
        match (a_complex, b_complex) {
            (Some((ar, ai)), Some((br, bi))) => {
                Ok(self.numeric_eq_impl(ar, br)? && self.numeric_eq_impl(ai, bi)?)
            }
            (Some((ar, ai)), None) => {
                // Complex equals real iff imaginary is zero and real parts equal
                Ok(self.is_numeric_zero_tv(ai) && self.numeric_eq_impl(ar, b)?)
            }
            (None, Some((br, bi))) => {
                Ok(self.is_numeric_zero_tv(bi) && self.numeric_eq_impl(a, br)?)
            }
            (None, None) => self.eq_real_values(a, b),
        }
    }

    /// Equality for non-complex values
    fn eq_real_values(&self, a: TaggedValue, b: TaggedValue) -> Result<bool, NumericError> {
        // fixnum vs fixnum (already handled above, but just in case)
        if a.is_fixnum() && b.is_fixnum() {
            return Ok(a.as_fixnum_unchecked() == b.as_fixnum_unchecked());
        }

        // fixnum vs object
        if a.is_fixnum() {
            return self.eq_fixnum_vs_object(a.as_fixnum_unchecked(), b);
        }
        if b.is_fixnum() {
            return self.eq_fixnum_vs_object(b.as_fixnum_unchecked(), a);
        }

        // Both should be objects at this point
        if !a.is_object() {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }
        if !b.is_object() {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }
        let a_obj = self.get_object(a);
        let b_obj = self.get_object(b);
        match (a_obj, b_obj) {
            (HeapObjectData::BigInt(x), HeapObjectData::BigInt(y)) => Ok(x == y),
            (HeapObjectData::Rational(x), HeapObjectData::Rational(y)) => Ok(x == y),
            (HeapObjectData::Real(x), HeapObjectData::Real(y)) => Ok(x == y),

            // BigInt vs Rational
            (HeapObjectData::BigInt(x), HeapObjectData::Rational(y))
            | (HeapObjectData::Rational(y), HeapObjectData::BigInt(x)) => {
                Ok(BigRational::from(x.clone()) == *y)
            }

            // Real vs exact types
            (HeapObjectData::Real(x), HeapObjectData::BigInt(y))
            | (HeapObjectData::BigInt(y), HeapObjectData::Real(x)) => {
                Self::compare_real_exact_bigint(*x, y)
            }
            (HeapObjectData::Real(x), HeapObjectData::Rational(y))
            | (HeapObjectData::Rational(y), HeapObjectData::Real(x)) => {
                if let Some(exact_real) = BigRational::from_float(*x) {
                    Ok(exact_real == *y)
                } else {
                    Ok(*x == y.to_f64().unwrap_or(f64::INFINITY))
                }
            }

            _ => Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            }),
        }
    }

    /// Compare fixnum to an object for equality
    fn eq_fixnum_vs_object(&self, n: i64, obj: TaggedValue) -> Result<bool, NumericError> {
        if !obj.is_object() {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }
        match self.get_object(obj) {
            HeapObjectData::BigInt(y) => Ok(BigInt::from(n) == *y),
            HeapObjectData::Rational(y) => Ok(BigRational::from(BigInt::from(n)) == *y),
            HeapObjectData::Real(y) => Self::compare_real_exact_bigint(*y, &BigInt::from(n)),
            _ => Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            }),
        }
    }

    /// Compare a Real (f64) to an exact integer (BigInt)
    fn compare_real_exact_bigint(real: f64, exact: &BigInt) -> Result<bool, NumericError> {
        if real.fract() != 0.0 || !real.is_finite() {
            return Ok(false);
        }
        if let Some(real_as_bigint) = num_bigint::BigInt::from_f64(real) {
            Ok(real_as_bigint == *exact)
        } else {
            Ok(real == exact.to_f64().unwrap_or(f64::INFINITY))
        }
    }

    /// Check if a < b numerically
    pub fn numeric_lt(&self, a: TaggedValue, b: TaggedValue) -> Result<bool, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            return Ok(a.as_fixnum_unchecked() < b.as_fixnum_unchecked());
        }
        self.compare_ordered(a, b, |x, y| x < y, |x, y| x < y)
    }

    /// Check if a <= b numerically
    pub fn numeric_le(&self, a: TaggedValue, b: TaggedValue) -> Result<bool, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            return Ok(a.as_fixnum_unchecked() <= b.as_fixnum_unchecked());
        }
        self.compare_ordered(a, b, |x, y| x <= y, |x, y| x <= y)
    }

    /// Check if a > b numerically
    pub fn numeric_gt(&self, a: TaggedValue, b: TaggedValue) -> Result<bool, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            return Ok(a.as_fixnum_unchecked() > b.as_fixnum_unchecked());
        }
        self.compare_ordered(a, b, |x, y| x > y, |x, y| x > y)
    }

    /// Check if a >= b numerically
    pub fn numeric_ge(&self, a: TaggedValue, b: TaggedValue) -> Result<bool, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            return Ok(a.as_fixnum_unchecked() >= b.as_fixnum_unchecked());
        }
        self.compare_ordered(a, b, |x, y| x >= y, |x, y| x >= y)
    }

    /// Ordered comparison helper (rejects complex, handles NaN)
    fn compare_ordered(
        &self,
        a: TaggedValue,
        b: TaggedValue,
        f64_cmp: impl Fn(f64, f64) -> bool,
        rat_cmp: impl Fn(&BigRational, &BigRational) -> bool,
    ) -> Result<bool, NumericError> {
        // NaN comparisons always return false
        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(false);
        }
        // Complex numbers can't be ordered
        if self.get_complex(a).is_some() || self.get_complex(b).is_some() {
            return Err(NumericError::NotReal {
                got: "complex".into(),
            });
        }
        // If either is Real, use f64 comparison
        if self.is_real_object(a) || self.is_real_object(b) {
            let af = self.numeric_to_f64(a)?;
            let bf = self.numeric_to_f64(b)?;
            return Ok(f64_cmp(af, bf));
        }
        // Both exact - use BigRational for precision
        let ar = self.to_bigrational(a)?;
        let br = self.to_bigrational(b)?;
        Ok(rat_cmp(&ar, &br))
    }

    // =========================================================================
    // Arithmetic Operations
    // =========================================================================

    /// Add two tagged values
    pub fn numeric_add(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        // Fast path: both fixnums
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if let Some(sum) = x.checked_add(y)
                && TaggedValue::fits_fixnum(sum)
            {
                return Ok(TaggedValue::fixnum(sum));
            }
            let big = BigInt::from(x) + BigInt::from(y);
            return Ok(self.alloc_bigint(big));
        }

        // NaN propagation
        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(self.alloc_real(f64::NAN));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let result = self.add_impl(a, b)?;
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Add implementation without contagion (for complex recursion)
    fn add_impl(&mut self, a: TaggedValue, b: TaggedValue) -> Result<TaggedValue, NumericError> {
        // Handle complex cases
        let a_complex = self.get_complex(a);
        let b_complex = self.get_complex(b);
        match (a_complex, b_complex) {
            (Some((ar, ai)), Some((br, bi))) => {
                let real = self.add_impl(ar, br)?;
                let imag = self.add_impl(ai, bi)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (Some((ar, ai)), None) => {
                let real = self.add_impl(ar, b)?;
                return Ok(self.simplify_complex_tagged(real, ai));
            }
            (None, Some((br, bi))) => {
                let real = self.add_impl(a, br)?;
                return Ok(self.simplify_complex_tagged(real, bi));
            }
            (None, None) => {}
        }

        // Both non-complex
        let a_data = self.extract_num_data(a)?;
        let b_data = self.extract_num_data(b)?;
        self.add_real(a_data, b_data)
    }

    /// Add two non-complex numeric values
    fn add_real(&mut self, a: NumData, b: NumData) -> Result<TaggedValue, NumericError> {
        match (a, b) {
            (NumData::Fixnum(x), NumData::Fixnum(y)) => match x.checked_add(y) {
                Some(sum) if TaggedValue::fits_fixnum(sum) => Ok(TaggedValue::fixnum(sum)),
                Some(sum) => Ok(self.alloc_bigint(BigInt::from(sum))),
                None => Ok(self.alloc_bigint(BigInt::from(x) + BigInt::from(y))),
            },
            (NumData::BigInt(x), NumData::Fixnum(y)) | (NumData::Fixnum(y), NumData::BigInt(x)) => {
                Ok(self.bigint_to_tagged(x + BigInt::from(y)))
            }
            (NumData::BigInt(x), NumData::BigInt(y)) => Ok(self.bigint_to_tagged(x + y)),

            (NumData::Rational(x), NumData::Rational(y)) => Ok(self.rational_to_tagged(x + y)),
            (NumData::Rational(x), NumData::Fixnum(y))
            | (NumData::Fixnum(y), NumData::Rational(x)) => {
                Ok(self.rational_to_tagged(x + BigRational::from(BigInt::from(y))))
            }
            (NumData::Rational(x), NumData::BigInt(y))
            | (NumData::BigInt(y), NumData::Rational(x)) => {
                Ok(self.rational_to_tagged(x + BigRational::from(y)))
            }

            (NumData::Real(x), NumData::Real(y)) => Ok(self.alloc_real(x + y)),
            (NumData::Real(x), NumData::Fixnum(y)) | (NumData::Fixnum(y), NumData::Real(x)) => {
                Ok(self.alloc_real(x + y as f64))
            }
            (NumData::Real(x), NumData::BigInt(y)) | (NumData::BigInt(y), NumData::Real(x)) => {
                Ok(self.alloc_real(x + y.to_f64().unwrap_or(f64::INFINITY)))
            }
            (NumData::Real(x), NumData::Rational(y)) | (NumData::Rational(y), NumData::Real(x)) => {
                Ok(self.alloc_real(x + y.to_f64().unwrap_or(f64::INFINITY)))
            }
        }
    }

    /// Subtract: a - b
    pub fn numeric_sub(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if let Some(diff) = x.checked_sub(y)
                && TaggedValue::fits_fixnum(diff)
            {
                return Ok(TaggedValue::fixnum(diff));
            }
            let big = BigInt::from(x) - BigInt::from(y);
            return Ok(self.alloc_bigint(big));
        }

        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(self.alloc_real(f64::NAN));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let result = self.sub_impl(a, b)?;
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Subtract implementation without contagion
    fn sub_impl(&mut self, a: TaggedValue, b: TaggedValue) -> Result<TaggedValue, NumericError> {
        let a_complex = self.get_complex(a);
        let b_complex = self.get_complex(b);
        match (a_complex, b_complex) {
            (Some((ar, ai)), Some((br, bi))) => {
                let real = self.sub_impl(ar, br)?;
                let imag = self.sub_impl(ai, bi)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (Some((ar, ai)), None) => {
                let real = self.sub_impl(ar, b)?;
                return Ok(self.simplify_complex_tagged(real, ai));
            }
            (None, Some((br, bi))) => {
                let real = self.sub_impl(a, br)?;
                let neg_bi = self.neg_impl(bi)?;
                return Ok(self.simplify_complex_tagged(real, neg_bi));
            }
            (None, None) => {}
        }

        let a_data = self.extract_num_data(a)?;
        let b_data = self.extract_num_data(b)?;
        self.sub_real(a_data, b_data)
    }

    /// Subtract two non-complex numeric values
    fn sub_real(&mut self, a: NumData, b: NumData) -> Result<TaggedValue, NumericError> {
        match (a, b) {
            (NumData::Fixnum(x), NumData::Fixnum(y)) => match x.checked_sub(y) {
                Some(diff) if TaggedValue::fits_fixnum(diff) => Ok(TaggedValue::fixnum(diff)),
                Some(diff) => Ok(self.alloc_bigint(BigInt::from(diff))),
                None => Ok(self.alloc_bigint(BigInt::from(x) - BigInt::from(y))),
            },
            (NumData::BigInt(x), NumData::Fixnum(y)) => {
                Ok(self.bigint_to_tagged(x - BigInt::from(y)))
            }
            (NumData::Fixnum(x), NumData::BigInt(y)) => {
                Ok(self.bigint_to_tagged(BigInt::from(x) - y))
            }
            (NumData::BigInt(x), NumData::BigInt(y)) => Ok(self.bigint_to_tagged(x - y)),

            (NumData::Rational(x), NumData::Rational(y)) => Ok(self.rational_to_tagged(x - y)),
            (NumData::Rational(x), NumData::Fixnum(y)) => {
                Ok(self.rational_to_tagged(x - BigRational::from(BigInt::from(y))))
            }
            (NumData::Fixnum(x), NumData::Rational(y)) => {
                Ok(self.rational_to_tagged(BigRational::from(BigInt::from(x)) - y))
            }
            (NumData::Rational(x), NumData::BigInt(y)) => {
                Ok(self.rational_to_tagged(x - BigRational::from(y)))
            }
            (NumData::BigInt(x), NumData::Rational(y)) => {
                Ok(self.rational_to_tagged(BigRational::from(x) - y))
            }

            (NumData::Real(x), NumData::Real(y)) => Ok(self.alloc_real(x - y)),
            (NumData::Real(x), NumData::Fixnum(y)) => Ok(self.alloc_real(x - y as f64)),
            (NumData::Fixnum(x), NumData::Real(y)) => Ok(self.alloc_real(x as f64 - y)),
            (NumData::Real(x), NumData::BigInt(y)) => {
                Ok(self.alloc_real(x - y.to_f64().unwrap_or(f64::INFINITY)))
            }
            (NumData::BigInt(x), NumData::Real(y)) => {
                Ok(self.alloc_real(x.to_f64().unwrap_or(f64::INFINITY) - y))
            }
            (NumData::Real(x), NumData::Rational(y)) => {
                Ok(self.alloc_real(x - y.to_f64().unwrap_or(f64::INFINITY)))
            }
            (NumData::Rational(x), NumData::Real(y)) => {
                Ok(self.alloc_real(x.to_f64().unwrap_or(f64::INFINITY) - y))
            }
        }
    }

    /// Multiply: a * b
    pub fn numeric_mul(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if let Some(prod) = x.checked_mul(y)
                && TaggedValue::fits_fixnum(prod)
            {
                return Ok(TaggedValue::fixnum(prod));
            }
            let big = BigInt::from(x) * BigInt::from(y);
            return Ok(self.alloc_bigint(big));
        }

        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(self.alloc_real(f64::NAN));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let result = self.mul_impl(a, b)?;
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Multiply implementation without contagion
    fn mul_impl(&mut self, a: TaggedValue, b: TaggedValue) -> Result<TaggedValue, NumericError> {
        let a_complex = self.get_complex(a);
        let b_complex = self.get_complex(b);
        match (a_complex, b_complex) {
            (Some((ar, ai)), Some((br, bi))) => {
                // (a+bi)(c+di) = (ac-bd) + (ad+bc)i
                let ac = self.mul_impl(ar, br)?;
                let bd = self.mul_impl(ai, bi)?;
                let real = self.sub_impl(ac, bd)?;
                let ad = self.mul_impl(ar, bi)?;
                let bc = self.mul_impl(ai, br)?;
                let imag = self.add_impl(ad, bc)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (Some((ar, ai)), None) => {
                let real = self.mul_impl(ar, b)?;
                let imag = self.mul_impl(ai, b)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (None, Some((br, bi))) => {
                let real = self.mul_impl(a, br)?;
                let imag = self.mul_impl(a, bi)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (None, None) => {}
        }

        let a_data = self.extract_num_data(a)?;
        let b_data = self.extract_num_data(b)?;
        self.mul_real(a_data, b_data)
    }

    /// Multiply two non-complex numeric values
    fn mul_real(&mut self, a: NumData, b: NumData) -> Result<TaggedValue, NumericError> {
        match (a, b) {
            (NumData::Fixnum(x), NumData::Fixnum(y)) => match x.checked_mul(y) {
                Some(prod) if TaggedValue::fits_fixnum(prod) => Ok(TaggedValue::fixnum(prod)),
                Some(prod) => Ok(self.alloc_bigint(BigInt::from(prod))),
                None => Ok(self.alloc_bigint(BigInt::from(x) * BigInt::from(y))),
            },
            (NumData::BigInt(x), NumData::Fixnum(y)) | (NumData::Fixnum(y), NumData::BigInt(x)) => {
                Ok(self.bigint_to_tagged(x * BigInt::from(y)))
            }
            (NumData::BigInt(x), NumData::BigInt(y)) => Ok(self.bigint_to_tagged(x * y)),

            (NumData::Rational(x), NumData::Rational(y)) => Ok(self.rational_to_tagged(x * y)),
            (NumData::Rational(x), NumData::Fixnum(y))
            | (NumData::Fixnum(y), NumData::Rational(x)) => {
                Ok(self.rational_to_tagged(x * BigRational::from(BigInt::from(y))))
            }
            (NumData::Rational(x), NumData::BigInt(y))
            | (NumData::BigInt(y), NumData::Rational(x)) => {
                Ok(self.rational_to_tagged(x * BigRational::from(y)))
            }

            (NumData::Real(x), NumData::Real(y)) => Ok(self.alloc_real(x * y)),
            (NumData::Real(x), NumData::Fixnum(y)) | (NumData::Fixnum(y), NumData::Real(x)) => {
                Ok(self.alloc_real(x * y as f64))
            }
            (NumData::Real(x), NumData::BigInt(y)) | (NumData::BigInt(y), NumData::Real(x)) => {
                Ok(self.alloc_real(x * y.to_f64().unwrap_or(f64::INFINITY)))
            }
            (NumData::Real(x), NumData::Rational(y)) | (NumData::Rational(y), NumData::Real(x)) => {
                Ok(self.alloc_real(x * y.to_f64().unwrap_or(f64::INFINITY)))
            }
        }
    }

    /// Divide: a / b
    pub fn numeric_div(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        // NaN propagation
        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(self.alloc_real(f64::NAN));
        }

        // Division by zero check
        if self.is_numeric_zero_tv(b) {
            if self.is_exact_number(a) && self.is_exact_number(b) {
                return Err(NumericError::DivisionByZero);
            }
            if self.is_numeric_zero_tv(a) {
                return Ok(self.alloc_real(f64::NAN));
            }
            let sign = if self.is_numeric_negative_tv(a) {
                -1.0
            } else {
                1.0
            };
            return Ok(self.alloc_real(sign * f64::INFINITY));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let result = self.div_impl(a, b)?;
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Divide implementation without contagion
    fn div_impl(&mut self, a: TaggedValue, b: TaggedValue) -> Result<TaggedValue, NumericError> {
        let a_complex = self.get_complex(a);
        let b_complex = self.get_complex(b);
        match (a_complex, b_complex) {
            (Some((ar, ai)), Some((br, bi))) => {
                // (a+bi)/(c+di) = (ac+bd)/(c²+d²) + (bc-ad)/(c²+d²)i
                let c2 = self.mul_impl(br, br)?;
                let d2 = self.mul_impl(bi, bi)?;
                let denom = self.add_impl(c2, d2)?;
                let ac = self.mul_impl(ar, br)?;
                let bd = self.mul_impl(ai, bi)?;
                let num_real = self.add_impl(ac, bd)?;
                let real = self.div_impl(num_real, denom)?;
                let bc = self.mul_impl(ai, br)?;
                let ad = self.mul_impl(ar, bi)?;
                let num_imag = self.sub_impl(bc, ad)?;
                // Re-compute denom (since div_impl consumed the first one)
                let c2b = self.mul_impl(br, br)?;
                let d2b = self.mul_impl(bi, bi)?;
                let denom2 = self.add_impl(c2b, d2b)?;
                let imag = self.div_impl(num_imag, denom2)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (Some((ar, ai)), None) => {
                let real = self.div_impl(ar, b)?;
                let imag = self.div_impl(ai, b)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (None, Some((br, bi))) => {
                // a / (c+di) = a(c-di) / (c²+d²)
                let c2 = self.mul_impl(br, br)?;
                let d2 = self.mul_impl(bi, bi)?;
                let denom = self.add_impl(c2, d2)?;
                let ac = self.mul_impl(a, br)?;
                let ad = self.mul_impl(a, bi)?;
                let neg_ad = self.neg_impl(ad)?;
                let real = self.div_impl(ac, denom)?;
                let c2b = self.mul_impl(br, br)?;
                let d2b = self.mul_impl(bi, bi)?;
                let denom2 = self.add_impl(c2b, d2b)?;
                let imag = self.div_impl(neg_ad, denom2)?;
                return Ok(self.simplify_complex_tagged(real, imag));
            }
            (None, None) => {}
        }

        let a_data = self.extract_num_data(a)?;
        let b_data = self.extract_num_data(b)?;
        self.div_real(a_data, b_data)
    }

    /// Divide two non-complex numeric values
    fn div_real(&mut self, a: NumData, b: NumData) -> Result<TaggedValue, NumericError> {
        match (a, b) {
            // Integer / Integer -> Rational
            (NumData::Fixnum(x), NumData::Fixnum(y)) => {
                Ok(self.rational_to_tagged(BigRational::new(BigInt::from(x), BigInt::from(y))))
            }
            (NumData::BigInt(x), NumData::Fixnum(y)) => {
                Ok(self.rational_to_tagged(BigRational::new(x, BigInt::from(y))))
            }
            (NumData::Fixnum(x), NumData::BigInt(y)) => {
                Ok(self.rational_to_tagged(BigRational::new(BigInt::from(x), y)))
            }
            (NumData::BigInt(x), NumData::BigInt(y)) => {
                Ok(self.rational_to_tagged(BigRational::new(x, y)))
            }

            (NumData::Rational(x), NumData::Rational(y)) => Ok(self.rational_to_tagged(x / y)),
            (NumData::Rational(x), NumData::Fixnum(y)) => {
                Ok(self.rational_to_tagged(x / BigRational::from(BigInt::from(y))))
            }
            (NumData::Fixnum(x), NumData::Rational(y)) => {
                Ok(self.rational_to_tagged(BigRational::from(BigInt::from(x)) / y))
            }
            (NumData::Rational(x), NumData::BigInt(y)) => {
                Ok(self.rational_to_tagged(x / BigRational::from(y)))
            }
            (NumData::BigInt(x), NumData::Rational(y)) => {
                Ok(self.rational_to_tagged(BigRational::from(x) / y))
            }

            (NumData::Real(x), NumData::Real(y)) => Ok(self.alloc_real(x / y)),
            (NumData::Real(x), NumData::Fixnum(y)) => Ok(self.alloc_real(x / y as f64)),
            (NumData::Fixnum(x), NumData::Real(y)) => Ok(self.alloc_real(x as f64 / y)),
            (NumData::Real(x), NumData::BigInt(y)) => {
                Ok(self.alloc_real(x / y.to_f64().unwrap_or(f64::INFINITY)))
            }
            (NumData::BigInt(x), NumData::Real(y)) => {
                Ok(self.alloc_real(x.to_f64().unwrap_or(f64::INFINITY) / y))
            }
            (NumData::Real(x), NumData::Rational(y)) => {
                Ok(self.alloc_real(x / y.to_f64().unwrap_or(f64::INFINITY)))
            }
            (NumData::Rational(x), NumData::Real(y)) => {
                Ok(self.alloc_real(x.to_f64().unwrap_or(f64::INFINITY) / y))
            }
        }
    }

    /// Negate: -a
    pub fn numeric_neg(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        // Fast path: fixnum
        if a.is_fixnum() {
            let n = a.as_fixnum_unchecked();
            if let Some(neg) = n.checked_neg()
                && TaggedValue::fits_fixnum(neg)
            {
                return Ok(TaggedValue::fixnum(neg));
            }
            return Ok(self.alloc_bigint(-BigInt::from(n)));
        }

        self.neg_impl(a)
    }

    /// Negate implementation (no fixnum check)
    fn neg_impl(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            let n = a.as_fixnum_unchecked();
            if let Some(neg) = n.checked_neg()
                && TaggedValue::fits_fixnum(neg)
            {
                return Ok(TaggedValue::fixnum(neg));
            }
            return Ok(self.alloc_bigint(-BigInt::from(n)));
        }

        // Extract data to avoid borrow issues
        let complex = self.get_complex(a);
        if let Some((real, imag)) = complex {
            let neg_real = self.neg_impl(real)?;
            let neg_imag = self.neg_impl(imag)?;
            return Ok(self.alloc_complex(neg_real, neg_imag));
        }

        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => {
                // Unreachable due to check above, but handle gracefully
                match n.checked_neg() {
                    Some(neg) if TaggedValue::fits_fixnum(neg) => Ok(TaggedValue::fixnum(neg)),
                    Some(neg) => Ok(self.alloc_bigint(BigInt::from(neg))),
                    None => Ok(self.alloc_bigint(-BigInt::from(n))),
                }
            }
            NumData::BigInt(n) => Ok(self.bigint_to_tagged(-n)),
            NumData::Rational(r) => Ok(self.rational_to_tagged(-r)),
            NumData::Real(f) => Ok(self.alloc_real(-f)),
        }
    }

    // =========================================================================
    // Integer Division Operations
    // =========================================================================

    /// Quotient: integer division truncated toward zero
    pub fn numeric_quotient(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if y == 0 {
                return Err(NumericError::DivisionByZero);
            }
            return Ok(TaggedValue::fixnum(x / y));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let ai = self.extract_bigint(a)?;
        let bi = self.extract_bigint(b)?;
        if bi.is_zero() {
            return Err(NumericError::DivisionByZero);
        }
        let q = &ai / &bi;
        let result = self.bigint_to_tagged(q);
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Remainder: sign of result matches sign of dividend
    pub fn numeric_remainder(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if y == 0 {
                return Err(NumericError::DivisionByZero);
            }
            return Ok(TaggedValue::fixnum(x % y));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let ai = self.extract_bigint(a)?;
        let bi = self.extract_bigint(b)?;
        if bi.is_zero() {
            return Err(NumericError::DivisionByZero);
        }
        let r = &ai % &bi;
        let result = self.bigint_to_tagged(r);
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Modulo: sign of result matches sign of divisor
    pub fn numeric_modulo(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if y == 0 {
                return Err(NumericError::DivisionByZero);
            }
            let r = x % y;
            let result = if r == 0 || (r > 0) == (y > 0) {
                r
            } else {
                r + y
            };
            return Ok(TaggedValue::fixnum(result));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let ai = self.extract_bigint(a)?;
        let bi = self.extract_bigint(b)?;
        if bi.is_zero() {
            return Err(NumericError::DivisionByZero);
        }
        let r = &ai % &bi;
        let result = if r.is_zero() || r.sign() == bi.sign() {
            r
        } else {
            r + &bi
        };
        let tv = self.bigint_to_tagged(result);
        Ok(self.apply_contagion_tagged(tv, inexact))
    }

    /// Floor quotient: floor(a/b)
    pub fn numeric_floor_quotient(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if y == 0 {
                return Err(NumericError::DivisionByZero);
            }
            let q = x / y;
            let r = x % y;
            let result = if r != 0 && (r < 0) != (y < 0) {
                q - 1
            } else {
                q
            };
            return Ok(TaggedValue::fixnum(result));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let ai = self.extract_bigint(a)?;
        let bi = self.extract_bigint(b)?;
        if bi.is_zero() {
            return Err(NumericError::DivisionByZero);
        }
        let q = floor_div_bigint(&ai, &bi);
        let result = self.bigint_to_tagged(q);
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Floor remainder: a - b * floor(a/b)
    pub fn numeric_floor_remainder(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            if y == 0 {
                return Err(NumericError::DivisionByZero);
            }
            let q = x / y;
            let r = x % y;
            let fq = if r != 0 && (r < 0) != (y < 0) {
                q - 1
            } else {
                q
            };
            return Ok(TaggedValue::fixnum(x - fq * y));
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let ai = self.extract_bigint(a)?;
        let bi = self.extract_bigint(b)?;
        if bi.is_zero() {
            return Err(NumericError::DivisionByZero);
        }
        let q = floor_div_bigint(&ai, &bi);
        let r = ai - q * &bi;
        let result = self.bigint_to_tagged(r);
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    // =========================================================================
    // Complex Number Operations
    // =========================================================================

    /// Extract real part of a number
    pub fn real_part(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(a);
        }
        if !a.is_object() {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }
        match self.get_object(a) {
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_) | HeapObjectData::Real(_) => {
                Ok(a)
            }
            HeapObjectData::Complex { real, .. } => Ok(*real),
            _ => Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            }),
        }
    }

    /// Extract imaginary part of a number
    pub fn imag_part(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(TaggedValue::fixnum(0));
        }
        if !a.is_object() {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }
        match self.get_object(a) {
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_) | HeapObjectData::Real(_) => {
                Ok(TaggedValue::fixnum(0))
            }
            HeapObjectData::Complex { imag, .. } => Ok(*imag),
            _ => Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            }),
        }
    }

    /// Compute magnitude (|z|) of a number (always returns inexact)
    pub fn magnitude(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            let n = a.as_fixnum_unchecked();
            return Ok(self.alloc_real((n as f64).abs()));
        }

        let complex = self.get_complex(a);
        if let Some((real, imag)) = complex {
            let r = self.numeric_to_f64(real)?;
            let i = self.numeric_to_f64(imag)?;
            return Ok(self.alloc_real((r * r + i * i).sqrt()));
        }

        let f = self.numeric_to_f64(a)?;
        Ok(self.alloc_real(f.abs()))
    }

    /// Compute angle (argument) in radians (always returns inexact)
    pub fn angle(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            let n = a.as_fixnum_unchecked();
            let angle = if n >= 0 { 0.0 } else { std::f64::consts::PI };
            return Ok(self.alloc_real(angle));
        }

        let complex = self.get_complex(a);
        if let Some((real, imag)) = complex {
            let r = self.numeric_to_f64(real)?;
            let i = self.numeric_to_f64(imag)?;
            return Ok(self.alloc_real(i.atan2(r)));
        }

        // Non-complex: check if negative
        if self.is_numeric_negative_tv(a) {
            Ok(self.alloc_real(std::f64::consts::PI))
        } else {
            Ok(self.alloc_real(0.0))
        }
    }

    /// Construct complex from rectangular coordinates
    pub fn make_rectangular(
        &mut self,
        real: TaggedValue,
        imag: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        // If imag is fixnum 0, return real directly
        if imag.is_fixnum() && imag.as_fixnum_unchecked() == 0 {
            return Ok(real);
        }

        // Reject complex components
        if self.get_complex(real).is_some() {
            return Err(NumericError::NotReal {
                got: "complex".into(),
            });
        }
        if self.get_complex(imag).is_some() {
            return Err(NumericError::NotReal {
                got: "complex".into(),
            });
        }

        // Validate both are numbers
        if !self.is_number(real) {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }
        if !self.is_number(imag) {
            return Err(NumericError::NotANumber {
                got: "non-numeric".into(),
            });
        }

        // Simplify if imaginary is exact zero
        if self.is_exact_zero(imag) {
            return Ok(real);
        }

        // If imaginary is inexact zero, return real as inexact
        if let Some(f) = self.get_real(imag)
            && f == 0.0
        {
            return Ok(self.to_inexact_tagged(real));
        }

        Ok(self.alloc_complex(real, imag))
    }

    /// Construct complex from polar coordinates
    pub fn make_polar(
        &mut self,
        magnitude: TaggedValue,
        angle: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        // Reject complex components
        if self.get_complex(magnitude).is_some() {
            return Err(NumericError::NotReal {
                got: "complex".into(),
            });
        }
        if self.get_complex(angle).is_some() {
            return Err(NumericError::NotReal {
                got: "complex".into(),
            });
        }

        let r = self.numeric_to_f64(magnitude)?;
        let theta = self.numeric_to_f64(angle)?;
        let real = r * theta.cos();
        let imag = r * theta.sin();

        if imag.abs() < 1e-15 {
            Ok(self.alloc_real(real))
        } else {
            let rv = self.alloc_real(real);
            let iv = self.alloc_real(imag);
            Ok(self.alloc_complex(rv, iv))
        }
    }

    // =========================================================================
    // Number Theory Operations
    // =========================================================================

    /// Get numerator of a rational number
    pub fn numerator(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(a);
        }
        // Extract data before mutable ops
        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => Ok(TaggedValue::fixnum(n)),
            NumData::BigInt(_) => Ok(a),
            NumData::Rational(r) => Ok(self.bigint_to_tagged(r.numer().clone())),
            NumData::Real(f) => {
                let r = float_to_exact_rational(f)?;
                Ok(self.alloc_real(r.numer().to_f64().unwrap_or(f64::INFINITY)))
            }
        }
    }

    /// Get denominator of a rational number
    pub fn denominator(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(TaggedValue::fixnum(1));
        }
        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(_) => Ok(TaggedValue::fixnum(1)),
            NumData::BigInt(_) => Ok(TaggedValue::fixnum(1)),
            NumData::Rational(r) => Ok(self.bigint_to_tagged(r.denom().clone())),
            NumData::Real(f) => {
                let r = float_to_exact_rational(f)?;
                Ok(self.alloc_real(r.denom().to_f64().unwrap_or(f64::INFINITY)))
            }
        }
    }

    /// Convert to exact representation
    pub fn to_exact(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(a);
        }

        let complex = self.get_complex(a);
        if let Some((real, imag)) = complex {
            let exact_real = self.to_exact(real)?;
            let exact_imag = self.to_exact(imag)?;
            return Ok(self.alloc_complex(exact_real, exact_imag));
        }

        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => Ok(TaggedValue::fixnum(n)),
            NumData::BigInt(_) | NumData::Rational(_) => Ok(a),
            NumData::Real(f) => {
                if f.is_nan() {
                    return Err(NumericError::NoExactRepresentation {
                        value: "+nan.0".into(),
                    });
                }
                if f.is_infinite() {
                    return Err(NumericError::NoExactRepresentation {
                        value: if f > 0.0 {
                            "+inf.0".into()
                        } else {
                            "-inf.0".into()
                        },
                    });
                }
                // Handle integer case
                if f.trunc() == f && f.abs() < i64::MAX as f64 {
                    let i = f as i64;
                    if TaggedValue::fits_fixnum(i) {
                        return Ok(TaggedValue::fixnum(i));
                    }
                    return Ok(self.alloc_bigint(BigInt::from(i)));
                }
                // Convert via rational
                if let Some(r) = BigRational::from_float(f) {
                    if r.is_integer() {
                        return Ok(self.bigint_to_tagged(r.numer().clone()));
                    }
                    return Ok(self.alloc_rational(r));
                }
                // Fallback
                Ok(self.alloc_rational(BigRational::new(
                    BigInt::from((f * 1_000_000.0) as i64),
                    BigInt::from(1_000_000),
                )))
            }
        }
    }

    /// Convert to inexact representation
    pub fn to_inexact(&mut self, a: TaggedValue) -> TaggedValue {
        if a.is_fixnum() {
            return self.alloc_real(a.as_fixnum_unchecked() as f64);
        }
        self.to_inexact_tagged(a)
    }

    /// Compute GCD of multiple tagged values
    pub fn gcd_many(&mut self, args: &[TaggedValue]) -> Result<TaggedValue, NumericError> {
        if args.is_empty() {
            return Ok(TaggedValue::fixnum(0));
        }

        // Fast path: all fixnums
        if args.iter().all(|a| a.is_fixnum()) {
            let mut result = args[0].as_fixnum_unchecked().abs();
            for arg in &args[1..] {
                let n = arg.as_fixnum_unchecked().abs();
                result = gcd_i64(result, n);
            }
            return Ok(TaggedValue::fixnum(result));
        }

        let inexact = args.iter().any(|a| self.is_inexact_number(*a));
        let mut result = self.extract_bigint(args[0])?;
        for arg in &args[1..] {
            let bi = self.extract_bigint(*arg)?;
            result = gcd_bigint(&result, &bi);
        }
        let tv = self.bigint_to_tagged(result);
        Ok(self.apply_contagion_tagged(tv, inexact))
    }

    /// Compute LCM of multiple tagged values
    pub fn lcm_many(&mut self, args: &[TaggedValue]) -> Result<TaggedValue, NumericError> {
        if args.is_empty() {
            return Ok(TaggedValue::fixnum(1));
        }

        // Fast path: all fixnums
        if args.iter().all(|a| a.is_fixnum()) {
            let mut result = args[0].as_fixnum_unchecked().abs();
            for arg in &args[1..] {
                let n = arg.as_fixnum_unchecked().abs();
                if result == 0 || n == 0 {
                    return Ok(TaggedValue::fixnum(0));
                }
                let g = gcd_i64(result, n);
                if let Some(product) = result.checked_mul(n / g) {
                    result = product;
                } else {
                    // Overflow - fall through to BigInt path
                    break;
                }
                if arg == args.last().unwrap() {
                    return Ok(TaggedValue::fixnum(result));
                }
            }
            // If we didn't return above, one fixnum overflowed - start over with BigInt
        }

        let inexact = args.iter().any(|a| self.is_inexact_number(*a));
        let mut result = self.extract_bigint(args[0])?.abs();
        for arg in &args[1..] {
            let bi = self.extract_bigint(*arg)?.abs();
            if result.is_zero() || bi.is_zero() {
                let tv = TaggedValue::fixnum(0);
                return Ok(self.apply_contagion_tagged(tv, inexact));
            }
            let g = gcd_bigint(&result, &bi);
            result = (&result / &g) * &bi;
        }
        let tv = self.bigint_to_tagged(result);
        Ok(self.apply_contagion_tagged(tv, inexact))
    }

    // =========================================================================
    // Rounding Operations
    // =========================================================================

    /// Floor: rounds toward negative infinity
    pub fn numeric_floor(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(a);
        }
        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => Ok(TaggedValue::fixnum(n)),
            NumData::BigInt(_) => Ok(a),
            NumData::Rational(r) => {
                let floored = floor_rational(&r);
                Ok(self.bigint_to_tagged(floored))
            }
            NumData::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(a)
                } else {
                    Ok(self.alloc_real(f.floor()))
                }
            }
        }
    }

    /// Ceiling: rounds toward positive infinity
    pub fn numeric_ceiling(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(a);
        }
        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => Ok(TaggedValue::fixnum(n)),
            NumData::BigInt(_) => Ok(a),
            NumData::Rational(r) => {
                let ceiled = ceiling_rational(&r);
                Ok(self.bigint_to_tagged(ceiled))
            }
            NumData::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(a)
                } else {
                    Ok(self.alloc_real(f.ceil()))
                }
            }
        }
    }

    /// Truncate: rounds toward zero
    pub fn numeric_truncate(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(a);
        }
        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => Ok(TaggedValue::fixnum(n)),
            NumData::BigInt(_) => Ok(a),
            NumData::Rational(r) => {
                let truncated = r.numer() / r.denom();
                Ok(self.bigint_to_tagged(truncated))
            }
            NumData::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(a)
                } else {
                    Ok(self.alloc_real(f.trunc()))
                }
            }
        }
    }

    /// Round: rounds to nearest integer (banker's rounding for ties)
    pub fn numeric_round(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            return Ok(a);
        }
        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => Ok(TaggedValue::fixnum(n)),
            NumData::BigInt(_) => Ok(a),
            NumData::Rational(r) => {
                let rounded = round_rational(&r);
                Ok(self.bigint_to_tagged(rounded))
            }
            NumData::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(a)
                } else {
                    Ok(self.alloc_real(bankers_round(f)))
                }
            }
        }
    }

    /// Absolute value
    pub fn numeric_abs(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            let n = a.as_fixnum_unchecked();
            return Ok(TaggedValue::fixnum(n.abs()));
        }

        // Complex: abs is magnitude
        let complex = self.get_complex(a);
        if complex.is_some() {
            return self.magnitude(a);
        }

        let data = self.extract_num_data(a)?;
        match data {
            NumData::Fixnum(n) => Ok(TaggedValue::fixnum(n.abs())),
            NumData::BigInt(n) => Ok(self.bigint_to_tagged(n.abs())),
            NumData::Rational(r) => Ok(self.rational_to_tagged(r.abs())),
            NumData::Real(f) => Ok(self.alloc_real(f.abs())),
        }
    }

    /// Maximum of two values
    pub fn numeric_max(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            return Ok(TaggedValue::fixnum(x.max(y)));
        }

        // NaN propagation
        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(self.alloc_real(f64::NAN));
        }

        // Complex can't be compared
        if self.get_complex(a).is_some() || self.get_complex(b).is_some() {
            return Err(NumericError::NotReal {
                got: "complex".into(),
            });
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let result = if self.numeric_gt(a, b)? { a } else { b };
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    /// Minimum of two values
    pub fn numeric_min(
        &mut self,
        a: TaggedValue,
        b: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() && b.is_fixnum() {
            let x = a.as_fixnum_unchecked();
            let y = b.as_fixnum_unchecked();
            return Ok(TaggedValue::fixnum(x.min(y)));
        }

        if self.is_numeric_nan(a) || self.is_numeric_nan(b) {
            return Ok(self.alloc_real(f64::NAN));
        }

        if self.get_complex(a).is_some() || self.get_complex(b).is_some() {
            return Err(NumericError::NotReal {
                got: "complex".into(),
            });
        }

        let inexact = self.is_inexact_number(a) || self.is_inexact_number(b);
        let result = if self.numeric_lt(a, b)? { a } else { b };
        Ok(self.apply_contagion_tagged(result, inexact))
    }

    // =========================================================================
    // Transcendental Operations
    // =========================================================================

    /// Square root
    pub fn numeric_sqrt(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        let complex = self.get_complex(a);
        if let Some((real, imag)) = complex {
            let re_in = self.numeric_to_f64(real)?;
            let im_in = self.numeric_to_f64(imag)?;
            // Negative real axis: sqrt returns principal value with non-negative imag
            if re_in < 0.0 && im_in == 0.0 {
                let abs_sqrt = (-re_in).sqrt();
                let rv = self.alloc_real(0.0);
                let iv = self.alloc_real(abs_sqrt);
                return Ok(self.alloc_complex(rv, iv));
            }
            let c = Complex64::new(re_in, im_in);
            return Ok(self.complex64_to_tagged(c.sqrt()));
        }

        let x = self.numeric_to_f64(a)?;
        if x >= 0.0 {
            Ok(self.alloc_real(x.sqrt()))
        } else {
            let abs_sqrt = (-x).sqrt();
            let rv = self.alloc_real(0.0);
            let iv = self.alloc_real(abs_sqrt);
            Ok(self.alloc_complex(rv, iv))
        }
    }

    /// Square (x * x)
    pub fn numeric_square(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if a.is_fixnum() {
            let n = a.as_fixnum_unchecked();
            if let Some(sq) = n.checked_mul(n)
                && TaggedValue::fits_fixnum(sq)
            {
                return Ok(TaggedValue::fixnum(sq));
            }
            let big = BigInt::from(n) * BigInt::from(n);
            return Ok(self.alloc_bigint(big));
        }
        self.numeric_mul(a, a)
    }

    /// Exponentiation (base^power)
    pub fn numeric_expt(
        &mut self,
        base: TaggedValue,
        power: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        let inexact = self.is_inexact_number(base) || self.is_inexact_number(power);

        // Special case: 0^0 = 1
        if self.is_numeric_zero_tv(base) && self.is_numeric_zero_tv(power) {
            let one = TaggedValue::fixnum(1);
            return Ok(self.apply_contagion_tagged(one, inexact));
        }

        // Special case: 1^anything = 1
        if base.is_fixnum() && base.as_fixnum_unchecked() == 1 {
            let one = TaggedValue::fixnum(1);
            return Ok(self.apply_contagion_tagged(one, inexact));
        }

        // Exact base with exact non-negative integer exponent → exact result
        if self.is_exact_number(base)
            && self.is_exact_number(power)
            && let Some(exp_int) = self.exponent_as_integer_tv(power)
        {
            if exp_int >= 0 {
                return self.exact_integer_power(base, exp_int as u64);
            } else {
                // Negative integer exponent: 1/base^|exp|
                let pos_result = self.exact_integer_power(base, (-exp_int) as u64)?;
                return self.numeric_div(TaggedValue::fixnum(1), pos_result);
            }
        }

        // General case: use floating point / Complex64
        let base_complex = self.get_complex(base);
        let power_complex = self.get_complex(power);
        match (base_complex, power_complex) {
            (Some((br, bi)), Some((er, ei))) => {
                let bc = Complex64::new(self.numeric_to_f64(br)?, self.numeric_to_f64(bi)?);
                let ec = Complex64::new(self.numeric_to_f64(er)?, self.numeric_to_f64(ei)?);
                Ok(self.complex64_to_tagged(bc.powc(ec)))
            }
            (Some((br, bi)), None) => {
                let bc = Complex64::new(self.numeric_to_f64(br)?, self.numeric_to_f64(bi)?);
                let exp = self.numeric_to_f64(power)?;
                Ok(self.complex64_to_tagged(bc.powf(exp)))
            }
            (None, Some((er, ei))) => {
                let b = self.numeric_to_f64(base)?;
                let ec = Complex64::new(self.numeric_to_f64(er)?, self.numeric_to_f64(ei)?);
                let bc = Complex64::new(b, 0.0);
                Ok(self.complex64_to_tagged(bc.powc(ec)))
            }
            (None, None) => {
                let b = self.numeric_to_f64(base)?;
                let exp = self.numeric_to_f64(power)?;
                // Negative base with non-integer exponent → complex
                if b < 0.0 && exp.fract() != 0.0 {
                    let bc = Complex64::new(b, 0.0);
                    Ok(self.complex64_to_tagged(bc.powf(exp)))
                } else {
                    Ok(self.alloc_real(b.powf(exp)))
                }
            }
        }
    }

    /// Compute exact integer power: base^exp where exp is non-negative
    fn exact_integer_power(
        &mut self,
        base: TaggedValue,
        exp: u64,
    ) -> Result<TaggedValue, NumericError> {
        if exp == 0 {
            return Ok(TaggedValue::fixnum(1));
        }
        if exp == 1 {
            return Ok(base);
        }

        if base.is_fixnum() {
            let b = base.as_fixnum_unchecked();
            if let Some(result) = checked_pow_i64(b, exp) {
                if TaggedValue::fits_fixnum(result) {
                    return Ok(TaggedValue::fixnum(result));
                }
                return Ok(self.alloc_bigint(BigInt::from(result)));
            }
            let big = BigInt::from(b).pow(exp as u32);
            return Ok(self.bigint_to_tagged(big));
        }

        let data = self.extract_num_data(base)?;
        match data {
            NumData::BigInt(n) => {
                let result = n.pow(exp as u32);
                Ok(self.bigint_to_tagged(result))
            }
            NumData::Rational(r) => {
                let numer = r.numer().pow(exp as u32);
                let denom = r.denom().pow(exp as u32);
                Ok(self.rational_to_tagged(BigRational::new(numer, denom)))
            }
            _ => {
                let f = self.numeric_to_f64(base)?;
                Ok(self.alloc_real(f.powi(exp as i32)))
            }
        }
    }

    /// Sine
    pub fn numeric_sin(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.sin()));
        }
        let x = self.numeric_to_f64(a)?;
        Ok(self.alloc_real(x.sin()))
    }

    /// Cosine
    pub fn numeric_cos(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.cos()));
        }
        let x = self.numeric_to_f64(a)?;
        Ok(self.alloc_real(x.cos()))
    }

    /// Tangent
    pub fn numeric_tan(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.tan()));
        }
        let x = self.numeric_to_f64(a)?;
        Ok(self.alloc_real(x.tan()))
    }

    /// Arc sine (returns complex for |x| > 1)
    pub fn numeric_asin(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.asin()));
        }
        let x = self.numeric_to_f64(a)?;
        if x.abs() <= 1.0 {
            Ok(self.alloc_real(x.asin()))
        } else {
            let c = Complex64::new(x, 0.0);
            Ok(self.complex64_to_tagged(c.asin()))
        }
    }

    /// Arc cosine (returns complex for |x| > 1)
    pub fn numeric_acos(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.acos()));
        }
        let x = self.numeric_to_f64(a)?;
        if x.abs() <= 1.0 {
            Ok(self.alloc_real(x.acos()))
        } else {
            let c = Complex64::new(x, 0.0);
            Ok(self.complex64_to_tagged(c.acos()))
        }
    }

    /// Arc tangent (single argument)
    pub fn numeric_atan(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.atan()));
        }
        let x = self.numeric_to_f64(a)?;
        Ok(self.alloc_real(x.atan()))
    }

    /// Arc tangent (two arguments - atan2)
    pub fn numeric_atan2(
        &mut self,
        y: TaggedValue,
        x: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        let y_val = self.numeric_to_f64(y)?;
        let x_val = self.numeric_to_f64(x)?;
        Ok(self.alloc_real(y_val.atan2(x_val)))
    }

    /// Exponential (e^x)
    pub fn numeric_exp(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.exp()));
        }
        let x = self.numeric_to_f64(a)?;
        Ok(self.alloc_real(x.exp()))
    }

    /// Natural logarithm (returns complex for negative reals)
    pub fn numeric_log(&mut self, a: TaggedValue) -> Result<TaggedValue, NumericError> {
        if let Some((real, imag)) = self.get_complex(a) {
            let c = Complex64::new(self.numeric_to_f64(real)?, self.numeric_to_f64(imag)?);
            return Ok(self.complex64_to_tagged(c.ln()));
        }
        let x = self.numeric_to_f64(a)?;
        if x > 0.0 {
            Ok(self.alloc_real(x.ln()))
        } else if x == 0.0 {
            Ok(self.alloc_real(f64::NEG_INFINITY))
        } else {
            // Negative: log(x) = log(|x|) + πi
            let c = Complex64::new(x, 0.0);
            Ok(self.complex64_to_tagged(c.ln()))
        }
    }

    /// Logarithm with base: log_base(x) = log(x) / log(base)
    pub fn numeric_log_base(
        &mut self,
        x: TaggedValue,
        base: TaggedValue,
    ) -> Result<TaggedValue, NumericError> {
        let x_log = self.numeric_log(x)?;
        let base_log = self.numeric_log(base)?;
        self.numeric_div(x_log, base_log)
    }
}
