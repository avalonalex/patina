# Numeric Tower Implementation Design

**Status**: ⚠️ DEPRECATED - See NUMERIC_TOWER_REVISED.md instead
**Date**: 2025-11-04
**Author**: Claude (with human review)

> **This document is superseded by the revised design based on chibi-scheme analysis.**
>
> **Use instead**: `NUMERIC_SUMMARY.md` (canonical guide) and `NUMERIC_TOWER_REVISED.md` (detailed design).
>
> This document is kept for historical reference showing the initial design approach
> before analyzing the chibi-scheme reference implementation.

## Executive Summary

This document outlines the complete design for implementing R7RS-compliant numeric tower in Patina. The numeric tower is fundamental to Scheme's mathematical operations and must be implemented correctly to ensure R7RS compliance.

## 1. Current State Analysis

### What We Have

**Value Types** (src/value/mod.rs):
```rust
Integer(i64)              // Fixed-size integers
BigInteger(BigInt)        // Unbounded integers (num-bigint)
Rational(BigRational)     // Exact rationals (num-rational)
Real(f64)                 // IEEE 754 double precision
Complex(f64, f64)         // Rectangular form (real, imag)
```

**Dependencies** (Cargo.toml):
- `num-bigint = "0.4"` - Arbitrary precision integers
- `num-rational = "0.4"` - Rational number support
- `num-traits = "0.2"` - Numeric trait abstractions

### What's Missing

1. **Exactness tracking** - No distinction between exact and inexact numbers
2. **Special float values** - No explicit handling of +inf.0, -inf.0, +nan.0
3. **Type predicates** - Missing: `rational?`, `exact?`, `inexact?`, `exact-integer?`
4. **Numeric operations** - Most operations not implemented
5. **Proper division semantics** - Exact vs inexact division not handled correctly
6. **Complex number operations** - No `make-rectangular`, `real-part`, etc.

## 2. The Numeric Tower Specification

### Type Hierarchy

```
number
├── complex
    ├── real
        ├── rational
            ├── integer
```

**Key principle**: This is a mathematical hierarchy. An integer IS a rational, IS a real, IS a complex number. Type predicates must reflect this.

### The Exactness Dimension

Exactness is **orthogonal** to the type hierarchy:

```
            Exact              Inexact
Integer:    42, 1000           42.0 (if exact-integer? false)
Rational:   1/3, 22/7          0.333...
Real:       (exact sqrt)       3.14159, +inf.0, -inf.0, +nan.0
Complex:    3+4i (exact parts)  3.0+4.0i
```

**Critical rule**: Inexactness is contagious
- If ANY operand is inexact, the result is inexact
- Exception: Operations provably unaffected (e.g., `(* exact-0 inexact)` may return exact 0)

## 3. Design Options for Exactness Tracking

### Option 1: Wrapped Types (Recommended)

Add exactness to each numeric variant:

```rust
pub enum Value {
    // Exact numbers
    ExactInteger(i64),
    ExactBigInteger(BigInt),
    ExactRational(BigRational),
    ExactComplex(BigRational, BigRational),  // exact real + imag parts

    // Inexact numbers
    InexactInteger(i64),    // e.g., from (inexact 42)
    InexactReal(f64),       // IEEE 754 floats
    InexactComplex(f64, f64),

    // ... other types
}
```

**Pros**:
- Type system enforces exactness
- Clear distinction at type level
- Pattern matching is straightforward

**Cons**:
- Doubles the number of numeric variants
- More code duplication in operations

### Option 2: Metadata Wrapper

Wrap numbers with exactness flag:

```rust
pub struct Number {
    value: NumericValue,
    exact: bool,
}

pub enum NumericValue {
    Integer(i64),
    BigInteger(BigInt),
    Rational(BigRational),
    Real(f64),
    Complex(ComplexValue),
}

pub enum Value {
    Number(Number),
    // ... other types
}
```

**Pros**:
- Single exactness flag location
- Easier to query exactness
- Less code duplication

**Cons**:
- Extra indirection
- More complex pattern matching
- Runtime overhead for exactness checks

### Option 3: Implicit Exactness (Current approach - INCORRECT)

Use type to imply exactness:
- Integer/BigInteger/Rational = exact
- Real/Complex = inexact

**Pros**:
- Simplest implementation
- No additional storage

**Cons**:
- ❌ VIOLATES R7RS - Can't represent inexact integers (e.g., `#i42`)
- ❌ Can't represent exact complex numbers
- ❌ Can't convert between exact/inexact properly

### Recommendation: **Option 1** (Wrapped Types)

This is the most R7RS-compliant approach. While it requires more variants, it provides:
- Type safety at compile time
- Clear semantics
- Easy to implement exact/inexact operations correctly

## 4. Type Predicate Implementation

### Required Predicates (R7RS base)

```rust
// Type hierarchy predicates
number?          // All numeric values
complex?         // All numeric values (same as number? in R7RS-small)
real?            // Not complex with non-zero imaginary
rational?        // Excludes irrationals, infinities, NaN
integer?         // Integer values (mathematically)

// Exactness predicates
exact?           // Is the number exact?
inexact?         // Is the number inexact?
exact-integer?   // Both exact AND integer

// Special value predicates (R7RS library - optional)
finite?          // Not infinity or NaN
infinite?        // +inf.0 or -inf.0
nan?             // +nan.0 or -nan.0
```

### Implementation Strategy

```rust
impl Value {
    pub fn is_number(&self) -> bool {
        matches!(self,
            Value::ExactInteger(_) | Value::ExactBigInteger(_) |
            Value::ExactRational(_) | Value::ExactComplex(_, _) |
            Value::InexactInteger(_) | Value::InexactReal(_) |
            Value::InexactComplex(_, _)
        )
    }

    pub fn is_real(&self) -> bool {
        match self {
            Value::ExactInteger(_) | Value::ExactBigInteger(_) |
            Value::ExactRational(_) | Value::InexactInteger(_) |
            Value::InexactReal(_) => true,
            Value::ExactComplex(_, i) => i.is_zero(),
            Value::InexactComplex(_, i) => *i == 0.0,
            _ => false,
        }
    }

    pub fn is_rational(&self) -> bool {
        match self {
            Value::ExactInteger(_) | Value::ExactBigInteger(_) |
            Value::ExactRational(_) => true,
            Value::InexactReal(r) => r.is_finite() && !r.is_nan(),
            Value::InexactInteger(_) => true, // Inexact integers are still rational
            _ => false,
        }
    }

    pub fn is_integer(&self) -> bool {
        match self {
            Value::ExactInteger(_) | Value::ExactBigInteger(_) |
            Value::InexactInteger(_) => true,
            Value::ExactRational(r) => r.denom().is_one(),
            Value::InexactReal(r) => r.is_finite() && r.fract() == 0.0,
            Value::ExactComplex(r, i) => i.is_zero() && r.denom().is_one(),
            Value::InexactComplex(r, i) => *i == 0.0 && r.fract() == 0.0,
            _ => false,
        }
    }

    pub fn is_exact(&self) -> bool {
        matches!(self,
            Value::ExactInteger(_) | Value::ExactBigInteger(_) |
            Value::ExactRational(_) | Value::ExactComplex(_, _)
        )
    }

    pub fn is_inexact(&self) -> bool {
        matches!(self,
            Value::InexactInteger(_) | Value::InexactReal(_) |
            Value::InexactComplex(_, _)
        )
    }

    pub fn is_exact_integer(&self) -> bool {
        self.is_exact() && self.is_integer()
    }
}
```

## 5. Numeric Operations Implementation

### Arithmetic Operations

```rust
// Addition - preserve exactness, promote types as needed
fn add(a: Value, b: Value) -> Result<Value, EvalError> {
    match (a, b) {
        // Exact + Exact = Exact
        (Value::ExactInteger(a), Value::ExactInteger(b)) => {
            // Check overflow, promote to BigInt if needed
            a.checked_add(b)
                .map(Value::ExactInteger)
                .or_else(|| Some(Value::ExactBigInteger(
                    BigInt::from(a) + BigInt::from(b)
                )))
        }

        // Inexact + anything = Inexact
        (Value::InexactReal(a), Value::ExactInteger(b)) => {
            Ok(Value::InexactReal(a + b as f64))
        }

        // Rational arithmetic
        (Value::ExactRational(a), Value::ExactRational(b)) => {
            Ok(Value::ExactRational(a + b))
        }

        // Complex arithmetic
        (Value::ExactComplex(r1, i1), Value::ExactComplex(r2, i2)) => {
            Ok(Value::ExactComplex(r1 + r2, i1 + i2))
        }

        // Mixed exactness -> inexact
        (Value::ExactInteger(a), Value::InexactReal(b)) => {
            Ok(Value::InexactReal(a as f64 + b))
        }

        // ... more cases
    }
}
```

### Division - Critical Semantics

```rust
fn divide(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.len() {
        0 => Err(EvalError::ArityMismatch),
        1 => {
            // Reciprocal: (/ z) => 1/z
            match &args[0] {
                Value::ExactInteger(n) if *n == 0 => {
                    Err(EvalError::DivisionByZero)
                }
                Value::ExactInteger(n) => {
                    Ok(Value::ExactRational(
                        BigRational::new(BigInt::from(1), BigInt::from(*n))
                    ))
                }
                Value::InexactReal(r) if *r == 0.0 => {
                    Ok(Value::InexactReal(f64::INFINITY))
                }
                // ... more cases
            }
        }
        _ => {
            // Left-associative division
            let mut result = args[0].clone();
            for arg in &args[1..] {
                // Check for exact zero divisor
                if arg.is_exact() && arg.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                result = divide_two(result, arg.clone())?;
            }
            Ok(result)
        }
    }
}
```

### Integer Division Operations

R7RS requires TWO division strategies:

```rust
// Floor division (rounds toward -infinity)
fn floor_quotient(n1: Value, n2: Value) -> Result<Value, EvalError>;
fn floor_remainder(n1: Value, n2: Value) -> Result<Value, EvalError>;
fn floor_div(n1: Value, n2: Value) -> Result<Value, EvalError> {
    // Returns (values quotient remainder)
}

// Truncate division (rounds toward zero)
fn truncate_quotient(n1: Value, n2: Value) -> Result<Value, EvalError>;
fn truncate_remainder(n1: Value, n2: Value) -> Result<Value, EvalError>;
fn truncate_div(n1: Value, n2: Value) -> Result<Value, EvalError>;

// Legacy (for R7RS compatibility)
// quotient = truncate-quotient
// remainder = truncate-remainder
// modulo = floor-remainder
```

**Example**:
```scheme
(floor-quotient 5 2)     => 2
(floor-remainder 5 2)    => 1
(floor-quotient -5 2)    => -3   ; rounds toward -infinity
(floor-remainder -5 2)   => 1    ; always non-negative

(truncate-quotient -5 2) => -2   ; rounds toward zero
(truncate-remainder -5 2) => -1  ; same sign as dividend
```

## 6. Special Float Values

### IEEE 754 Special Values

```rust
// Constants
const POS_INF: f64 = f64::INFINITY;
const NEG_INF: f64 = f64::NEG_INFINITY;
const NAN: f64 = f64::NAN;

// Detection
impl Value {
    pub fn is_infinity(&self) -> bool {
        match self {
            Value::InexactReal(r) => r.is_infinite(),
            Value::InexactComplex(r, i) => r.is_infinite() || i.is_infinite(),
            _ => false,
        }
    }

    pub fn is_nan(&self) -> bool {
        match self {
            Value::InexactReal(r) => r.is_nan(),
            Value::InexactComplex(r, i) => r.is_nan() || i.is_nan(),
            _ => false,
        }
    }

    pub fn is_finite(&self) -> bool {
        match self {
            Value::ExactInteger(_) | Value::ExactBigInteger(_) |
            Value::ExactRational(_) => true,
            Value::InexactReal(r) => r.is_finite(),
            Value::InexactComplex(r, i) => r.is_finite() && i.is_finite(),
            Value::ExactComplex(_, _) => true,
            _ => false,
        }
    }
}
```

### Display Formatting

```rust
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Value::InexactReal(r) => {
                if r.is_nan() {
                    write!(f, "+nan.0")
                } else if r.is_infinite() {
                    if r.is_sign_positive() {
                        write!(f, "+inf.0")
                    } else {
                        write!(f, "-inf.0")
                    }
                } else {
                    write!(f, "{}", r)
                }
            }
            // ... other cases
        }
    }
}
```

## 7. Complex Number Operations

### Constructor Functions

```rust
// make-rectangular: x y -> z where z = x + yi
fn make_rectangular(real: Value, imag: Value) -> Result<Value, EvalError> {
    match (real, imag) {
        (Value::ExactInteger(r), Value::ExactInteger(i)) => {
            if i == 0 {
                Ok(Value::ExactInteger(r))  // Simplify to real
            } else {
                Ok(Value::ExactComplex(
                    BigRational::from(BigInt::from(r)),
                    BigRational::from(BigInt::from(i))
                ))
            }
        }
        // If either part is inexact, result is inexact
        (real, imag) if real.is_inexact() || imag.is_inexact() => {
            let r = real.to_inexact_real()?;
            let i = imag.to_inexact_real()?;
            if i == 0.0 {
                Ok(Value::InexactReal(r))
            } else {
                Ok(Value::InexactComplex(r, i))
            }
        }
        // ... more cases
    }
}

// make-polar: r θ -> z where z = r * e^(iθ)
fn make_polar(magnitude: Value, angle: Value) -> Result<Value, EvalError> {
    // Convert to rectangular form
    let r = magnitude.to_real()?;
    let theta = angle.to_real()?;

    let real = r * theta.cos();
    let imag = r * theta.sin();

    // Result may be inexact even if inputs are exact (transcendental)
    Ok(Value::InexactComplex(real, imag))
}
```

### Accessor Functions

```rust
fn real_part(z: Value) -> Result<Value, EvalError> {
    match z {
        Value::ExactComplex(r, _) => Ok(Value::ExactRational(r)),
        Value::InexactComplex(r, _) => Ok(Value::InexactReal(r)),
        // Real numbers are their own real part
        real if real.is_real() => Ok(real),
        _ => Err(EvalError::TypeError("Expected number".into())),
    }
}

fn imag_part(z: Value) -> Result<Value, EvalError> {
    match z {
        Value::ExactComplex(_, i) => Ok(Value::ExactRational(i)),
        Value::InexactComplex(_, i) => Ok(Value::InexactReal(i)),
        // Real numbers have zero imaginary part (exact!)
        Value::ExactInteger(_) | Value::ExactBigInteger(_) |
        Value::ExactRational(_) => Ok(Value::ExactInteger(0)),
        Value::InexactReal(_) | Value::InexactInteger(_) => {
            Ok(Value::InexactReal(0.0))
        }
        _ => Err(EvalError::TypeError("Expected number".into())),
    }
}
```

## 8. Conversion Functions

### Exact/Inexact Conversion

```rust
fn exact(z: Value) -> Result<Value, EvalError> {
    match z {
        // Already exact - return unchanged
        Value::ExactInteger(_) | Value::ExactBigInteger(_) |
        Value::ExactRational(_) | Value::ExactComplex(_, _) => Ok(z),

        // Convert inexact to exact
        Value::InexactInteger(n) => Ok(Value::ExactInteger(n)),

        Value::InexactReal(r) => {
            if r.is_infinite() || r.is_nan() {
                // Can't make infinity/NaN exact
                Err(EvalError::ImplementationRestriction(
                    "Cannot convert infinity/NaN to exact".into()
                ))
            } else {
                // Convert float to rational approximation
                // Use num-rational's from_float
                BigRational::from_float(r)
                    .map(Value::ExactRational)
                    .ok_or_else(|| EvalError::ConversionError(
                        format!("Cannot convert {} to exact", r)
                    ))
            }
        }

        Value::InexactComplex(r, i) => {
            let real_exact = exact(Value::InexactReal(r))?;
            let imag_exact = exact(Value::InexactReal(i))?;
            make_rectangular(real_exact, imag_exact)
        }

        _ => Err(EvalError::TypeError("Expected number".into())),
    }
}

fn inexact(z: Value) -> Result<Value, EvalError> {
    match z {
        // Already inexact - return unchanged
        Value::InexactInteger(_) | Value::InexactReal(_) |
        Value::InexactComplex(_, _) => Ok(z),

        // Convert exact to inexact
        Value::ExactInteger(n) => Ok(Value::InexactReal(n as f64)),
        Value::ExactBigInteger(n) => {
            // May lose precision for very large integers
            Ok(Value::InexactReal(n.to_f64().unwrap_or(f64::INFINITY)))
        }
        Value::ExactRational(r) => {
            Ok(Value::InexactReal(r.to_f64().unwrap_or(f64::NAN)))
        }
        Value::ExactComplex(r, i) => {
            let real = r.to_f64().unwrap_or(f64::NAN);
            let imag = i.to_f64().unwrap_or(f64::NAN);
            Ok(Value::InexactComplex(real, imag))
        }

        _ => Err(EvalError::TypeError("Expected number".into())),
    }
}
```

## 9. Implementation Plan

### Phase 1: Restructure Value Enum (Breaking Change)
**Effort**: 2-3 days
**Risk**: High (breaks existing code)

1. Add exact/inexact variants to Value enum
2. Update Display implementation
3. Update parser to recognize #e/#i prefixes
4. Fix all existing code to use new variants

### Phase 2: Type Predicates
**Effort**: 1 day
**Risk**: Low

1. Implement `number?`, `complex?`, `real?`, `rational?`, `integer?`
2. Implement `exact?`, `inexact?`, `exact-integer?`
3. Add tests for all predicates

### Phase 3: Basic Numeric Operations
**Effort**: 2-3 days
**Risk**: Medium

1. Update `+`, `-`, `*`, `/` to handle exactness correctly
2. Implement rounding: `floor`, `ceiling`, `truncate`, `round`
3. Implement `abs`, `min`, `max`
4. Add comprehensive tests

### Phase 4: Integer Division
**Effort**: 1-2 days
**Risk**: Low

1. Implement `floor-quotient`, `floor-remainder`, `floor/`
2. Implement `truncate-quotient`, `truncate-remainder`, `truncate/`
3. Alias `quotient`, `remainder`, `modulo` for compatibility
4. Test with GCD algorithms

### Phase 5: Complex Numbers
**Effort**: 2 days
**Risk**: Low

1. Implement `make-rectangular`, `make-polar`
2. Implement `real-part`, `imag-part`, `magnitude`, `angle`
3. Update arithmetic to handle complex properly

### Phase 6: Conversions and Special Values
**Effort**: 1 day
**Risk**: Low

1. Implement `exact`, `inexact`
2. Handle `+inf.0`, `-inf.0`, `+nan.0` in parser and display
3. Implement `finite?`, `infinite?`, `nan?`

### Phase 7: Advanced Operations (Lower Priority)
**Effort**: 3-4 days
**Risk**: Low

1. Transcendental functions (library): `exp`, `log`, `sin`, `cos`, etc.
2. `numerator`, `denominator`
3. `rationalize`
4. `exact-integer-sqrt`

**Total Estimated Effort**: 12-16 days for full numeric tower

## 10. Testing Strategy

### Unit Tests for Each Type

```rust
#[test]
fn test_exact_integer_addition() {
    let a = Value::ExactInteger(42);
    let b = Value::ExactInteger(8);
    let result = add(a, b).unwrap();
    assert_matches!(result, Value::ExactInteger(50));
}

#[test]
fn test_inexact_contagion() {
    let a = Value::ExactInteger(1);
    let b = Value::InexactReal(2.0);
    let result = add(a, b).unwrap();
    assert_matches!(result, Value::InexactReal(_));
}

#[test]
fn test_division_exact_zero_error() {
    let a = Value::ExactInteger(1);
    let b = Value::ExactInteger(0);
    assert!(divide(vec![a, b]).is_err());
}
```

### Compliance Tests from R7RS Spec

Use chibi-scheme as reference implementation:

```scheme
; Type predicate tests
(test (integer? 3.0) #t)      ; Inexact integer
(test (rational? +inf.0) #f)  ; Infinity is not rational
(test (exact? 1/3) #t)        ; Exact rational

; Exactness contagion
(test (exact? (+ 1 2.0)) #f)  ; Inexact result
(test (exact? (* 0 2.0)) #f)  ; Still inexact (conservative)

; Division semantics
(test (exact? (/ 1 2)) #t)    ; Exact division -> rational
(test (/ 1 2) 1/2)            ; Not 0!
```

## 11. Risks and Mitigations

### Risk 1: Breaking Existing Code
**Impact**: High
**Mitigation**:
- Create migration branch
- Update all existing code systematically
- Run full test suite after each change

### Risk 2: Performance Regression
**Impact**: Medium
**Mitigation**:
- Keep fast path for common cases (exact integers)
- Benchmark before/after
- Consider lazy promotion (start with i64, promote to BigInt only when needed)

### Risk 3: Incorrect Exactness Semantics
**Impact**: High
**Mitigation**:
- Extensive testing against chibi-scheme
- Review R7RS spec carefully for edge cases
- Test NaN, infinity, and exact zero division

## 12. Open Questions

1. **Should we support the full numeric tower or allow restrictions?**
   - R7RS allows omitting complex numbers
   - Recommendation: Support full tower for completeness

2. **How to handle very large exact integers?**
   - Current: Use BigInt
   - Alternative: Report implementation restriction at some limit?
   - Recommendation: Support arbitrary precision (R7RS strongly encourages this)

3. **Should we implement negative zero (-0.0)?**
   - R7RS says it's optional but recommended for IEEE 754
   - Recommendation: Yes, f64 supports it natively

4. **Precision of rational-to-float conversion?**
   - Use num-rational's built-in to_f64
   - Document precision limits

## 13. Dependencies

### Current (Sufficient)
- `num-bigint = "0.4"` - Arbitrary precision integers ✅
- `num-rational = "0.4"` - Rational numbers ✅
- `num-traits = "0.2"` - Numeric traits ✅

### Optional Additions
- `num-complex` - If we want complex number type instead of (f64, f64) tuple
  - Verdict: Not needed, tuple is simpler and matches R7RS semantics

### No Additional Dependencies Required!

Our current dependencies are perfect for implementing the R7RS numeric tower.

## 14. Conclusion

Implementing a correct R7RS numeric tower requires:

1. **Restructuring the Value enum** to track exactness explicitly
2. **Careful implementation** of type predicates respecting the mathematical hierarchy
3. **Exactness propagation** in all arithmetic operations
4. **Special handling** for division, infinities, and NaN
5. **Comprehensive testing** against the R7RS spec

The recommended approach is to use explicit exact/inexact variants in the Value enum (Option 1), which provides the strongest type safety and clearest semantics.

**Estimated total effort**: 12-16 days for complete implementation.

**Next step**: Get user approval for the design, then start with Phase 1 (Value enum restructuring).
