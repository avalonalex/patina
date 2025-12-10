# Numeric Operations Refactoring Plan

**Status:** Planning
**Created:** 2024-12-04
**Goal:** Consolidate numeric operations into `patina-core` and eliminate the brittle `NumericValue` abstraction

## Problem Statement

The current `arithmetic.rs` file has grown to ~3100 lines with several architectural issues:

1. **Parallel type hierarchy**: `NumericValue` duplicates `Value`'s numeric variants
2. **Constant conversion overhead**: Every operation does `Value` → `NumericValue` → `Value`
3. **Brittle exactness tracking**: Each function manually tracks `has_inexact` with variations
4. **Inconsistent NaN propagation**: Ad-hoc checks scattered throughout
5. **Bug in `is_inexact()`**: Treats all Complex as inexact, but Complex with exact parts should be exact

## Solution Overview

Move numeric operations to `patina-core` as methods on `Value`, eliminating `NumericValue` entirely.

### Design Principles

1. **Single source of truth**: Numeric semantics defined once, on `Value`
2. **No conversion overhead**: Operations work directly on `Value`
3. **Centralized policies**: Exactness contagion and NaN propagation in one place
4. **Thin primitives**: Tree-walker primitives just validate arity and delegate

## Implementation Plan

### Phase 1: Create Numeric Module in patina-core

Create `patina-core/src/numeric/` module structure:

```
patina-core/src/numeric/
├── mod.rs              # Public API, re-exports
├── error.rs            # NumericError type
├── predicates.rs       # is_exact, is_nan, is_finite, is_infinite, is_zero
├── exactness.rs        # to_exact, to_inexact, exactness contagion logic
├── arithmetic.rs       # add, sub, mul, div (with type promotion)
├── comparison.rs       # numeric_eq, lt, gt, le, ge
├── complex.rs          # real_part, imag_part, magnitude, angle, make_rectangular, make_polar
├── rounding.rs         # floor, ceiling, truncate, round
└── transcendental.rs   # sin, cos, tan, exp, log, sqrt, expt
```

### Phase 2: Define NumericError

```rust
// patina-core/src/numeric/error.rs

#[derive(Debug, Clone, PartialEq)]
pub enum NumericError {
    /// Operation requires a number but got something else
    NotANumber { got: String },

    /// Operation requires a real number (not complex)
    NotReal { got: String },

    /// Operation requires an integer
    NotInteger { got: String },

    /// Operation requires exact number
    NotExact { got: String },

    /// Division by zero
    DivisionByZero,

    /// Result would be undefined (e.g., 0/0, log of negative)
    Undefined { operation: &'static str, reason: &'static str },

    /// Overflow in exact arithmetic (shouldn't happen with BigInt, but just in case)
    Overflow,
}
```

### Phase 3: Implement Predicates

```rust
// patina-core/src/numeric/predicates.rs

impl Value {
    /// Returns true if this is any numeric type
    pub fn is_number(&self) -> bool;

    /// Returns true if this is a real number (not complex, or complex with exact zero imaginary)
    pub fn is_real(&self) -> bool;

    /// Returns true if this is an integer (exact or inexact)
    pub fn is_integer_value(&self) -> bool;

    /// Returns true if this is an exact number
    pub fn is_exact(&self) -> bool;

    /// Returns true if this is an inexact number
    pub fn is_inexact(&self) -> bool;

    /// Returns true if this is NaN (only possible for Real or Complex with Real parts)
    pub fn is_nan(&self) -> bool;

    /// Returns true if this is +inf.0 or -inf.0
    pub fn is_infinite(&self) -> bool;

    /// Returns true if this is a finite number (not NaN, not infinite)
    pub fn is_finite(&self) -> bool;

    /// Returns true if this is zero (exact or inexact)
    pub fn is_zero(&self) -> bool;

    /// Returns true if this is negative
    pub fn is_negative(&self) -> bool;

    /// Returns true if this is positive
    pub fn is_positive(&self) -> bool;
}
```

### Phase 4: Implement Exactness Conversion

```rust
// patina-core/src/numeric/exactness.rs

impl Value {
    /// Convert to inexact representation
    /// - Integer/BigInteger/Rational → Real
    /// - Complex with exact parts → Complex with inexact parts
    /// - Already inexact → unchanged
    pub fn to_inexact(&self) -> Value;

    /// Convert to exact representation
    /// - Real → Rational (using continued fractions or exact representation)
    /// - Complex with inexact parts → Complex with exact parts
    /// - Already exact → unchanged
    /// Returns error for NaN or infinity (no exact representation)
    pub fn to_exact(&self) -> Result<Value, NumericError>;
}

/// Determine the result exactness for a binary operation
/// Rule: If any operand is inexact, result is inexact
pub fn result_exactness(a: &Value, b: &Value) -> Exactness;

pub enum Exactness {
    Exact,
    Inexact,
}
```

### Phase 5: Implement Core Arithmetic

```rust
// patina-core/src/numeric/arithmetic.rs

impl Value {
    /// Add two numbers with automatic type promotion
    /// Returns NaN if either operand is NaN (NaN propagation)
    /// Applies inexact contagion
    pub fn numeric_add(&self, other: &Value) -> Result<Value, NumericError>;

    /// Subtract: self - other
    pub fn numeric_sub(&self, other: &Value) -> Result<Value, NumericError>;

    /// Multiply two numbers
    pub fn numeric_mul(&self, other: &Value) -> Result<Value, NumericError>;

    /// Divide: self / other
    /// Returns error for division by exact zero
    /// Returns infinity for division by inexact zero
    pub fn numeric_div(&self, other: &Value) -> Result<Value, NumericError>;

    /// Negate: -self
    pub fn numeric_neg(&self) -> Result<Value, NumericError>;

    /// Absolute value
    pub fn numeric_abs(&self) -> Result<Value, NumericError>;
}

// Helper for type promotion during arithmetic
enum PromotedPair {
    Integers(i64, i64),
    BigIntegers(BigInt, BigInt),
    Rationals(BigRational, BigRational),
    Reals(f64, f64),
    Complex(Box<(Value, Value)>, Box<(Value, Value)>),
}

fn promote_pair(a: &Value, b: &Value) -> Result<PromotedPair, NumericError>;
```

### Phase 6: Implement Comparison

```rust
// patina-core/src/numeric/comparison.rs

impl Value {
    /// Numeric equality (=)
    /// NaN is not equal to anything, including itself
    pub fn numeric_eq(&self, other: &Value) -> Result<bool, NumericError>;

    /// Less than (<)
    /// Returns error for complex numbers
    /// Returns false if either is NaN
    pub fn numeric_lt(&self, other: &Value) -> Result<bool, NumericError>;

    /// Greater than (>)
    pub fn numeric_gt(&self, other: &Value) -> Result<bool, NumericError>;

    /// Less than or equal (<=)
    pub fn numeric_le(&self, other: &Value) -> Result<bool, NumericError>;

    /// Greater than or equal (>=)
    pub fn numeric_ge(&self, other: &Value) -> Result<bool, NumericError>;

    /// Find maximum, with NaN propagation
    pub fn numeric_max(&self, other: &Value) -> Result<Value, NumericError>;

    /// Find minimum, with NaN propagation
    pub fn numeric_min(&self, other: &Value) -> Result<Value, NumericError>;
}
```

### Phase 7: Implement Complex Operations

```rust
// patina-core/src/numeric/complex.rs

impl Value {
    /// Extract real part (works for any number)
    pub fn real_part(&self) -> Result<Value, NumericError>;

    /// Extract imaginary part (0 for real numbers)
    pub fn imag_part(&self) -> Result<Value, NumericError>;

    /// Compute magnitude (|z|)
    pub fn magnitude(&self) -> Result<Value, NumericError>;

    /// Compute angle (argument) in radians
    pub fn angle(&self) -> Result<Value, NumericError>;
}

/// Construct complex from rectangular coordinates
pub fn make_rectangular(real: &Value, imag: &Value) -> Result<Value, NumericError>;

/// Construct complex from polar coordinates
pub fn make_polar(magnitude: &Value, angle: &Value) -> Result<Value, NumericError>;
```

### Phase 8: Implement Rounding

```rust
// patina-core/src/numeric/rounding.rs

impl Value {
    /// Floor: largest integer <= x
    pub fn numeric_floor(&self) -> Result<Value, NumericError>;

    /// Ceiling: smallest integer >= x
    pub fn numeric_ceiling(&self) -> Result<Value, NumericError>;

    /// Truncate: round toward zero
    pub fn numeric_truncate(&self) -> Result<Value, NumericError>;

    /// Round: round to nearest, ties to even (banker's rounding)
    pub fn numeric_round(&self) -> Result<Value, NumericError>;
}
```

### Phase 9: Implement Transcendental Functions

```rust
// patina-core/src/numeric/transcendental.rs

impl Value {
    pub fn numeric_sin(&self) -> Result<Value, NumericError>;
    pub fn numeric_cos(&self) -> Result<Value, NumericError>;
    pub fn numeric_tan(&self) -> Result<Value, NumericError>;
    pub fn numeric_asin(&self) -> Result<Value, NumericError>;
    pub fn numeric_acos(&self) -> Result<Value, NumericError>;
    pub fn numeric_atan(&self) -> Result<Value, NumericError>;
    pub fn numeric_atan2(&self, x: &Value) -> Result<Value, NumericError>;
    pub fn numeric_exp(&self) -> Result<Value, NumericError>;
    pub fn numeric_log(&self) -> Result<Value, NumericError>;
    pub fn numeric_log_base(&self, base: &Value) -> Result<Value, NumericError>;
    pub fn numeric_sqrt(&self) -> Result<Value, NumericError>;
    pub fn numeric_expt(&self, exponent: &Value) -> Result<Value, NumericError>;
}
```

### Phase 10: Implement Integer Division

```rust
// patina-core/src/numeric/integer_div.rs

impl Value {
    /// quotient: truncated division
    pub fn numeric_quotient(&self, divisor: &Value) -> Result<Value, NumericError>;

    /// remainder: sign follows dividend
    pub fn numeric_remainder(&self, divisor: &Value) -> Result<Value, NumericError>;

    /// modulo: sign follows divisor
    pub fn numeric_modulo(&self, divisor: &Value) -> Result<Value, NumericError>;

    /// floor-quotient
    pub fn numeric_floor_quotient(&self, divisor: &Value) -> Result<Value, NumericError>;

    /// floor-remainder
    pub fn numeric_floor_remainder(&self, divisor: &Value) -> Result<Value, NumericError>;

    /// truncate-quotient
    pub fn numeric_truncate_quotient(&self, divisor: &Value) -> Result<Value, NumericError>;

    /// truncate-remainder
    pub fn numeric_truncate_remainder(&self, divisor: &Value) -> Result<Value, NumericError>;

    /// gcd of two numbers
    pub fn numeric_gcd(&self, other: &Value) -> Result<Value, NumericError>;

    /// lcm of two numbers
    pub fn numeric_lcm(&self, other: &Value) -> Result<Value, NumericError>;

    /// numerator of rational
    pub fn numerator(&self) -> Result<Value, NumericError>;

    /// denominator of rational
    pub fn denominator(&self) -> Result<Value, NumericError>;
}
```

### Phase 11: Refactor Tree-Walker Primitives

After the numeric module is complete, refactor `arithmetic.rs` to be thin wrappers:

```rust
// patina-tree-walker/src/eval/primitives/arithmetic.rs (after refactoring)

pub(super) fn add(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Ok(Value::Integer(0));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_add(arg).map_err(numeric_to_eval_error)?;
    }
    Ok(result)
}

pub(super) fn max(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "max")?;

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_max(arg).map_err(numeric_to_eval_error)?;
    }
    Ok(result)
}

fn numeric_to_eval_error(e: NumericError) -> EvalError {
    match e {
        NumericError::NotANumber { got } => EvalError::TypeError(format!("expected number, got {}", got)),
        NumericError::DivisionByZero => EvalError::DivisionByZero,
        // ... etc
    }
}
```

### Phase 12: Split arithmetic.rs into Submodules

After Phase 11, the remaining code in tree-walker can be organized:

```
patina-tree-walker/src/eval/primitives/
├── arithmetic/
│   ├── mod.rs           # Re-exports and register()
│   ├── basic.rs         # +, -, *, /, abs
│   ├── comparison.rs    # =, <, >, <=, >=, max, min
│   ├── integer_div.rs   # quotient, remainder, modulo, floor-/, truncate-/
│   ├── rounding.rs      # floor, ceiling, truncate, round
│   ├── exactness.rs     # exact, inexact, rationalize, exact-integer-sqrt
│   ├── transcendental.rs # sin, cos, exp, log, sqrt, expt, etc.
│   └── complex.rs       # real-part, imag-part, magnitude, angle, make-rectangular, make-polar
```

## Migration Strategy

1. **Phase 1-10**: Build the new numeric module in parallel with existing code
2. **Add comprehensive tests**: Test new module against R7RS requirements
3. **Phase 11**: Gradually migrate primitives one-by-one
4. **Phase 12**: Split and clean up tree-walker after migration complete
5. **Delete `NumericValue`**: Remove after all uses are migrated

## Testing Strategy

1. **Unit tests in patina-core**: Test each method independently
2. **Property-based tests**: Test algebraic properties (commutativity, associativity)
3. **Edge cases**: NaN propagation, infinity handling, exactness contagion
4. **Regression tests**: All existing tests must continue to pass
5. **Chibi compatibility**: Run chibi r7rs-tests.scm after each phase

## Risk Mitigation

1. **Incremental migration**: Don't rewrite everything at once
2. **Feature flags**: Can disable new code path if issues found
3. **Parallel implementation**: Keep old code working during transition
4. **Comprehensive tests**: Catch regressions early

## Success Criteria

1. All existing tests pass
2. Chibi r7rs-tests.scm compatibility maintained or improved
3. `NumericValue` eliminated from codebase
4. `arithmetic.rs` reduced from ~3100 lines to ~500 lines
5. Clear separation between numeric semantics (patina-core) and primitive glue (tree-walker)

## Dependencies

- `num-bigint`: BigInt operations
- `num-rational`: Rational number operations
- `num-traits`: Numeric trait bounds
- `num-complex`: Complex number operations (for transcendental functions)

## Future Considerations

1. **Performance**: The new design should be faster (no conversion overhead)
2. **Multiple backends**: Numeric semantics shared across tree-walker, VM, JIT
3. **Exactness optimization**: Could cache exactness flag to avoid recomputation
4. **SIMD**: Future vectorization of numeric operations
