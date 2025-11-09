# Numeric Tower Implementation Design (Revised)

**Status**: Supporting Reference Document
**Date**: 2025-11-04
**Based on**: Chibi-Scheme reference implementation analysis

> **Note**: For the canonical implementation guide, see `NUMERIC_SUMMARY.md`.
> This document provides detailed design rationale and code examples.

## Executive Summary

After analyzing chibi-scheme's implementation (the R7RS reference implementation), we've discovered a **much simpler approach** than initially designed. The key insight: **exactness is implicit in types**, eliminating the need for metadata flags.

## Key Design Change: Type-Implicit Exactness

### The Insight from Chibi-Scheme

Chibi-scheme doesn't use exactness flags. Instead:

```c
// From chibi-scheme
Integer/Bignum/Ratio = exact (no flag needed)
Flonum = inexact (always)
Complex = depends on components
```

This means we can **eliminate 3 extra variants** from our original design!

### Recommended Patina Implementation

```rust
pub enum Value {
    // Numbers - exactness is IMPLICIT
    Integer(i64),           // Always exact
    BigInteger(BigInt),     // Always exact
    Rational(BigRational),  // Always exact
    Real(f64),              // Always inexact (IEEE 754)
    Complex(Box<(Value, Value)>),  // Exact if both parts are exact

    // ... other types
}

// Exactness checking - no metadata needed!
impl Value {
    pub fn is_exact(&self) -> bool {
        matches!(self,
            Value::Integer(_) |
            Value::BigInteger(_) |
            Value::Rational(_)
        )
    }

    pub fn is_inexact(&self) -> bool {
        matches!(self, Value::Real(_))
    }
}
```

**This is MUCH simpler** than the original design with separate ExactInteger/InexactInteger variants!

## Critical Implementation Details

### 1. Ratio Normalization (MUST DO)

Ratios must be normalized on creation to ensure canonical representation:

```rust
impl BigRational {
    // When creating a rational from division
    pub fn new_normalized(n: BigInt, d: BigInt) -> Self {
        let mut ratio = BigRational::new(n, d);
        // num-rational already normalizes internally!
        // But we need to check if denominator is 1
        ratio
    }
}

// When / operator is called with exact integers
fn divide_exact(a: i64, b: i64) -> Value {
    let ratio = BigRational::new(
        BigInt::from(a),
        BigInt::from(b)
    );

    // If reduced to integer, return Integer not Rational
    if ratio.denom() == &BigInt::from(1) {
        Value::Integer(ratio.numer().to_i64().unwrap())
    } else {
        Value::Rational(ratio)
    }
}
```

**Why this matters**:
- Ensures `(/ 6 3)` returns `Integer(2)`, not `Rational(2/1)`
- Makes equality checking work: `(= 2 (/ 6 3))` => `#t`
- Canonical representation: only ONE way to represent each number

### 2. Overflow Detection and Promotion

Integer arithmetic must transparently promote to BigInt on overflow:

```rust
fn add_integers(a: i64, b: i64) -> Value {
    match a.checked_add(b) {
        Some(result) => Value::Integer(result),
        None => {
            // Overflow: promote to BigInt
            let result = BigInt::from(a) + BigInt::from(b);
            Value::BigInteger(result)
        }
    }
}

fn multiply_integers(a: i64, b: i64) -> Value {
    match a.checked_mul(b) {
        Some(result) => Value::Integer(result),
        None => {
            let result = BigInt::from(a) * BigInt::from(b);
            Value::BigInteger(result)
        }
    }
}
```

### 3. Division Semantics (CRITICAL)

`/` and `quotient` are **different operations**:

```rust
// / operator - returns EXACT result
fn divide(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.as_slice() {
        [Value::Integer(a), Value::Integer(b)] if *b != 0 => {
            // MUST return rational, not float!
            let ratio = BigRational::new(
                BigInt::from(*a),
                BigInt::from(*b)
            );

            // Simplify if possible
            if ratio.denom() == &BigInt::from(1) {
                Ok(Value::Integer(ratio.numer().to_i64().unwrap()))
            } else {
                Ok(Value::Rational(ratio))
            }
        }
        [Value::Integer(_), Value::Integer(0)] => {
            // Exact zero divisor is ERROR
            Err(EvalError::DivisionByZero)
        }
        [Value::Real(a), Value::Real(b)] => {
            // Inexact division follows IEEE 754
            Ok(Value::Real(a / b))  // May return inf or nan
        }
        // ... more cases
    }
}

// quotient - integer division (truncate toward zero)
fn quotient(a: Value, b: Value) -> Result<Value, EvalError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) if b != 0 => {
            Ok(Value::Integer(a / b))  // Rust's / truncates toward zero
        }
        (Value::Integer(_), Value::Integer(0)) => {
            Err(EvalError::DivisionByZero)
        }
        // ... more cases
    }
}

// remainder - remainder after quotient
fn remainder(a: Value, b: Value) -> Result<Value, EvalError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) if b != 0 => {
            Ok(Value::Integer(a % b))  // Rust's % gives truncate remainder
        }
        // ... more cases
    }
}
```

**Key difference**:
```scheme
(/ 1 2)       => 1/2    (exact rational)
(quotient 1 2) => 0     (integer division)
(/ 1.0 2.0)   => 0.5    (inexact)
```

### 4. Complex Numbers as Pairs

```rust
pub enum Value {
    Complex(Box<(Value, Value)>),  // (real-part, imag-part)
    // ...
}

// Constructor
fn make_rectangular(real: Value, imag: Value) -> Value {
    // Optimization: if imaginary is zero, return just the real part
    if is_zero(&imag) {
        return real;
    }
    Value::Complex(Box::new((real, imag)))
}

// Accessors
fn real_part(z: Value) -> Value {
    match z {
        Value::Complex(box (r, _)) => r,
        other => other,  // Real numbers are their own real part
    }
}

fn imag_part(z: Value) -> Value {
    match z {
        Value::Complex(box (_, i)) => i,
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => {
            Value::Integer(0)  // Exact zero for exact reals
        }
        Value::Real(_) => {
            Value::Real(0.0)  // Inexact zero for inexact reals
        }
    }
}
```

### 5. Exactness Conversion

```rust
// exact: inexact -> exact
fn exact(z: Value) -> Result<Value, EvalError> {
    match z {
        // Already exact
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => Ok(z),

        // Convert float to rational
        Value::Real(f) => {
            if f.is_infinite() || f.is_nan() {
                Err(EvalError::ImplementationRestriction(
                    "Cannot convert infinity/NaN to exact"
                ))
            } else {
                // num-rational can convert from float
                match BigRational::from_float(f) {
                    Some(r) => {
                        // Check if it's actually an integer
                        if r.denom() == &BigInt::from(1) {
                            Ok(Value::BigInteger(r.numer().clone()))
                        } else {
                            Ok(Value::Rational(r))
                        }
                    }
                    None => Err(EvalError::ConversionError(
                        format!("Cannot convert {} to exact", f)
                    ))
                }
            }
        }

        // Convert complex
        Value::Complex(box (r, i)) => {
            let real_exact = exact(r)?;
            let imag_exact = exact(i)?;
            Ok(make_rectangular(real_exact, imag_exact))
        }
    }
}

// inexact: exact -> inexact
fn inexact(z: Value) -> Result<Value, EvalError> {
    match z {
        // Already inexact
        Value::Real(_) => Ok(z),

        // Convert integer to float
        Value::Integer(n) => Ok(Value::Real(n as f64)),
        Value::BigInteger(n) => {
            Ok(Value::Real(n.to_f64().unwrap_or(f64::INFINITY)))
        }
        Value::Rational(r) => {
            Ok(Value::Real(r.to_f64().unwrap_or(f64::NAN)))
        }

        // Convert complex
        Value::Complex(box (r, i)) => {
            let real_inexact = inexact(r)?;
            let imag_inexact = inexact(i)?;
            Ok(make_rectangular(real_inexact, imag_inexact))
        }
    }
}
```

### 6. IEEE 754 Special Values

```rust
// Parsing
fn parse_number(s: &str) -> Option<Value> {
    match s {
        "+inf.0" => Some(Value::Real(f64::INFINITY)),
        "-inf.0" => Some(Value::Real(f64::NEG_INFINITY)),
        "+nan.0" => Some(Value::Real(f64::NAN)),
        "-nan.0" => Some(Value::Real(f64::NAN)),  // NaN has no sign in practice
        _ => {
            // Normal number parsing...
        }
    }
}

// Display
impl Display for Value {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Value::Real(r) if r.is_nan() => write!(f, "+nan.0"),
            Value::Real(r) if r.is_infinite() => {
                if r.is_sign_positive() {
                    write!(f, "+inf.0")
                } else {
                    write!(f, "-inf.0")
                }
            }
            Value::Real(r) => write!(f, "{}", r),
            // ... other cases
        }
    }
}

// Predicates
impl Value {
    pub fn is_finite(&self) -> bool {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => true,
            Value::Real(r) => r.is_finite(),
            Value::Complex(box (r, i)) => r.is_finite() && i.is_finite(),
        }
    }

    pub fn is_infinite(&self) -> bool {
        match self {
            Value::Real(r) => r.is_infinite(),
            Value::Complex(box (r, i)) => r.is_infinite() || i.is_infinite(),
            _ => false,
        }
    }

    pub fn is_nan(&self) -> bool {
        match self {
            Value::Real(r) => r.is_nan(),
            Value::Complex(box (r, i)) => r.is_nan() || i.is_nan(),
            _ => false,
        }
    }
}
```

## Implementation Plan (Simplified)

### Phase 1: Core Numeric Types (2-3 days)
1. ✅ Keep current Value enum (already correct!)
2. Add ratio normalization on division
3. Add overflow detection to arithmetic
4. Implement transparent BigInt promotion

### Phase 2: Division Semantics (1-2 days)
1. Fix `/` to return exact rationals
2. Implement `quotient`, `remainder`, `modulo`
3. Add division-by-zero error handling
4. Test with GCD algorithms

### Phase 3: Type Predicates (1 day)
1. Implement `exact?`, `inexact?`
2. Implement `exact-integer?`
3. Implement `finite?`, `infinite?`, `nan?`
4. Update `integer?`, `rational?`, `real?` to handle edge cases

### Phase 4: Exactness Conversion (1 day)
1. Implement `exact` procedure
2. Implement `inexact` procedure
3. Handle special float values properly

### Phase 5: Complex Numbers (2 days)
1. Change Complex to Box<(Value, Value)>
2. Implement `make-rectangular`, `make-polar`
3. Implement `real-part`, `imag-part`, `magnitude`, `angle`
4. Optimize away zero imaginary parts

### Phase 6: Parser Updates (1 day)
1. Parse `#e` and `#i` prefixes
2. Parse rational literals `3/5`
3. Parse `+inf.0`, `-inf.0`, `+nan.0`
4. Test parsing thoroughly

### Phase 7: Advanced Operations (2-3 days)
1. Implement rounding: `floor`, `ceiling`, `truncate`, `round`
2. Implement `numerator`, `denominator`
3. Implement `gcd`, `lcm`
4. Implement `exact-integer-sqrt`

**Total Estimated Effort**: 10-13 days (vs 12-16 days in original design)

## What Changed from Original Design

| Aspect | Original Design | Revised Design |
|--------|----------------|----------------|
| Exactness tracking | Separate variants | Type-implicit |
| Value enum size | 7 numeric variants | 5 numeric variants |
| Exactness check | Pattern match on 7 cases | Pattern match on 3 cases |
| Complex storage | `Complex(f64, f64)` | `Complex(Box<(Value, Value)>)` |
| Ratio handling | Unspecified | Normalized on creation |
| Division | Unclear | Separate `/` and `quotient` |
| Dependencies | Same | Same (no change) |

## Benefits of Revised Design

1. **Simpler** - 5 numeric variants instead of 7
2. **More elegant** - Exactness is intrinsic to type
3. **Chibi-proven** - Matches reference implementation
4. **Flexible complex** - Can have exact complex numbers
5. **Faster implementation** - Less code to write

## Comparison to Current Patina

### Current State (src/value/mod.rs)
```rust
Integer(i64)              // ✅ Already correct
BigInteger(BigInt)        // ✅ Already correct
Rational(BigRational)     // ✅ Already correct (but need normalization)
Real(f64)                 // ✅ Already correct
Complex(f64, f64)         // ❌ Should be Box<(Value, Value)>
```

**We're 80% there!** Just need to:
1. Change Complex representation
2. Add normalization to division
3. Implement the operations correctly

## Risks and Mitigations

### Risk 1: Complex representation change
**Impact**: Medium
**Mitigation**: Complex numbers not heavily used yet; early in project

### Risk 2: Division semantics change
**Impact**: High - breaks existing behavior
**Mitigation**:
- Add tests first
- Document expected behavior
- Update gradually

### Risk 3: Performance with boxed complex
**Impact**: Low
**Mitigation**: Complex numbers are rare; boxing cost is minimal

## Testing Strategy

### 1. Unit Tests
```rust
#[test]
fn test_exact_division() {
    // (/ 1 2) => 1/2 (exact rational)
    let result = divide(vec![
        Value::Integer(1),
        Value::Integer(2),
    ]).unwrap();
    assert_matches!(result, Value::Rational(_));
}

#[test]
fn test_quotient_vs_divide() {
    // (/ 1 2) => 1/2
    let div_result = divide(vec![Value::Integer(1), Value::Integer(2)]).unwrap();

    // (quotient 1 2) => 0
    let quot_result = quotient(Value::Integer(1), Value::Integer(2)).unwrap();

    assert_ne!(div_result, quot_result);
}
```

### 2. Chibi Compatibility Tests
Run same expressions in chibi and patina, compare results:
```bash
# chibi-scheme
(exact? (/ 1 2))  ; => #t
(/ 1 2)           ; => 1/2
(quotient 1 2)    ; => 0
```

### 3. R7RS Compliance Tests
Use tests from `reference/chibi-scheme/tests/r7rs-tests.scm`

## Dependencies (No Changes Needed!)

```toml
[dependencies]
num-bigint = "0.4"      # ✅ Perfect for BigInteger
num-rational = "0.4"    # ✅ Perfect for Rational (includes GCD!)
num-traits = "0.2"      # ✅ Useful trait abstractions
```

**No additional dependencies required!**

The `num-rational` crate already:
- Normalizes rationals automatically
- Provides `to_f64()` for conversion
- Implements all arithmetic operations
- Has `from_float()` for exact conversion

## Next Steps

1. **Get user approval** for this revised design
2. **Start with Phase 1**: Add overflow detection and ratio normalization
3. **Move to Phase 2**: Fix division semantics (critical for correctness)
4. **Continue through phases** sequentially

## Conclusion

The chibi-scheme analysis revealed a much simpler approach than our original design. By making exactness implicit in types, we:

- Reduce complexity (5 variants instead of 7)
- Follow proven design (chibi-scheme reference)
- Maintain R7RS compliance
- Keep implementation simple and clear

**Current Patina is already 80% structurally correct** - we just need to:
1. Fix Complex representation
2. Add proper division semantics
3. Implement the operations

This is a **much better path forward** than the original design!
