# Numeric Operations Test Suite

This test file (`numeric_operations.rs`) contains **33 comprehensive test cases** for missing R7RS-small numeric operations. All tests are currently **ignored** and serve as a specification for future implementation.

## Test Summary by Category

### 1. Rounding Functions (4 tests)
- `test_floor` - Round down to integer
- `test_ceiling` - Round up to integer
- `test_truncate` - Round toward zero
- `test_round` - Round to nearest (banker's rounding)

**Priority:** ⭐⭐⭐ High - Fundamental operations, easy to implement

---

### 2. Rational Number Accessors (2 tests)
- `test_numerator` - Get numerator of rational
- `test_denominator` - Get denominator of rational

**Priority:** ⭐⭐⭐ High - Quick wins, we already have BigRational

---

### 3. Exactness Conversion (2 tests)
- `test_inexact_to_exact` - Convert inexact → exact (e.g., `3.0` → `3`)
- `test_exact_to_inexact` - Convert exact → inexact (e.g., `3` → `3.0`)

**Priority:** ⭐⭐ Medium - Important for numeric tower

---

### 4. Square Root and Exponentiation (3 tests)
- `test_sqrt` - Square root
- `test_expt` - Exponentiation (with integer and real exponents)
- `test_square` - Square a number (can be library function)

**Priority:** ⭐⭐ Medium - Common operations

---

### 5. Number Theory (3 tests)
- `test_gcd` - Greatest common divisor
- `test_lcm` - Least common multiple
- `test_exact_integer_sqrt` - Integer square root with remainder (returns multiple values)

**Priority:** ⭐⭐ Medium - Useful but not critical

---

### 6. Approximation (1 test)
- `test_rationalize` - Find simple rational approximation

**Priority:** ⭐ Low - Specialized use case

---

### 7. Float Predicates (4 tests)
- `test_finite_predicate` - Test if number is finite
- `test_infinite_predicate` - Test if number is infinite
- `test_nan_predicate` - Test if number is NaN
- `test_exact_integer_predicate` - Test if exact integer

**Priority:** ⭐⭐ Medium - Useful for robust numeric code

---

### 8. Complex Number Operations (5 tests)
- `test_real_part` - Extract real part
- `test_imag_part` - Extract imaginary part
- `test_magnitude` - Distance from origin
- `test_angle` - Angle in polar form
- `test_make_rectangular` - Construct from real+imaginary
- `test_make_polar` - Construct from magnitude+angle

**Priority:** ⭐ Low - Nice to have, complex numbers already work

---

### 9. Trigonometric Functions (6 tests)
- `test_sin`, `test_cos`, `test_tan` - Basic trig
- `test_asin`, `test_acos`, `test_atan` - Inverse trig

**Priority:** ⭐ Low - Can use Rust's f64 methods

---

### 10. Exponential and Logarithmic (2 tests)
- `test_exp` - e^x
- `test_log` - Natural log (and log with base)

**Priority:** ⭐ Low - Can use Rust's f64 methods

---

## Implementation Roadmap

### Phase 1: Core Numeric (Est. 2-3 hours)
1. **Rounding functions** - `floor`, `ceiling`, `truncate`, `round`
2. **Rational accessors** - `numerator`, `denominator`
3. **Exactness conversion** - `exact`, `inexact`

**Impact:** 8 tests enabled, completes core numeric tower operations

---

### Phase 2: Mathematical Functions (Est. 2-3 hours)
4. **Square root and power** - `sqrt`, `expt`, `square`
5. **Float predicates** - `finite?`, `infinite?`, `nan?`, `exact-integer?`
6. **Number theory** - `gcd`, `lcm`, `exact-integer-sqrt`

**Impact:** 10 tests enabled, adds mathematical operations

---

### Phase 3: Transcendental Functions (Est. 1-2 hours)
7. **Trigonometric** - `sin`, `cos`, `tan`, `asin`, `acos`, `atan`
8. **Exponential/Log** - `exp`, `log`

**Impact:** 8 tests enabled, completes R7RS numeric functions

---

### Phase 4: Advanced Complex (Est. 1-2 hours)
9. **Complex accessors** - `real-part`, `imag-part`, `magnitude`, `angle`
10. **Complex constructors** - `make-rectangular`, `make-polar`
11. **Rationalize** - `rationalize`

**Impact:** 6 tests enabled, completes advanced operations

---

## Running Tests

```bash
# Run all numeric operation tests (will show as ignored)
cargo test --test numeric_operations

# Run specific test category (when implemented)
cargo test --test numeric_operations test_floor
cargo test --test numeric_operations test_gcd

# Run all tests including ignored ones (will fail until implemented)
cargo test --test numeric_operations -- --include-ignored
```

---

## Implementation Notes

### Easy Implementations (Use Rust stdlib):
- Rounding: `f64::floor()`, `f64::ceil()`, `f64::trunc()`, `f64::round()`
- Trig: `f64::sin()`, `f64::cos()`, `f64::tan()`, etc.
- Exp/Log: `f64::exp()`, `f64::ln()`, `f64::log()`
- Predicates: `f64::is_finite()`, `f64::is_infinite()`, `f64::is_nan()`

### Medium Implementations:
- Rational accessors: Extract from existing `BigRational`
- GCD/LCM: Use `num-integer` crate methods
- `sqrt`/`expt`: Handle exact vs inexact cases

### Complex Implementations:
- `exact-integer-sqrt`: Returns multiple values
- `rationalize`: Continued fractions algorithm
- Exactness conversion: Rational approximation of floats

---

## Total Progress

**Current Status:**
- ✅ 232 tests passing
- ⏸️ 11 tests ignored (non-numeric features)
- 🔴 **33 tests ignored (numeric operations in this file)**

**After Implementation:**
- Projected: ~265 tests passing
- Numbers category: 94% → **100%** ✅
- Overall R7RS compliance: 60% → **~65%**
