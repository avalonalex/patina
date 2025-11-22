# Arithmetic Primitives Refactoring Plan

**Status:** Research Complete, Implementation Pending
**Created:** 2025-11-22
**Priority:** Medium (Technical Debt)

## Executive Summary

The `crates/patina-tree-walker/src/eval/primitives/arithmetic.rs` file has grown to **2,193 lines** - 2.6x larger than the next largest primitive file. This document analyzes the complexity sources and proposes concrete refactoring strategies to improve maintainability, reduce brittleness, and minimize future bugs.

**Key Metrics:**
- **Lines of code:** 2,193 (vs. ~700 average for other primitive files)
- **Functions:** 50+ arithmetic primitives
- **Pattern duplication:** ~126 lines of duplicated match logic
- **Registration boilerplate:** 450 lines (21% of file)
- **Estimated code reduction potential:** 30-40% through refactoring

---

## 1. Problem Analysis

### 1.1 Root Cause: Dual Type System

The file defines an internal `NumericValue` enum that mirrors the `Value` enum from `patina-runtime`:

```rust
// Internal to arithmetic.rs (lines 19-26)
enum NumericValue {
    Integer(i64),
    BigInteger(BigInt),
    Rational(BigRational),
    Real(f64),
    Complex(Box<(NumericValue, NumericValue)>), // Recursive!
}
```

**Purpose:** Provide helper methods for arithmetic operations without modifying `patina-runtime::Value`.

**Cost:**
1. **Constant conversion overhead:** Every function converts `Value → NumericValue → operation → NumericValue → Value`
2. **Duplicated logic:** Methods like `is_zero()`, `is_exact()`, `to_f64()` exist on both types
3. **Maintenance burden:** Changes to numeric tower require updating both representations
4. **Dead code:** 5+ methods marked `#[allow(dead_code)]` because they're not always needed

### 1.2 Pattern Duplication: The 5-Way Match

Arithmetic operations (`add`, `subtract`, `multiply`) all share nearly identical 42-line match expressions:

```rust
// From add() - lines 249-291
fn add(self, other: Self) -> Self {
    match (self, other) {
        // Integer + Integer (with overflow check)
        (Integer(a), Integer(b)) => match a.checked_add(b) { ... },

        // Promote to BigInteger (4 patterns)
        (BigInteger(a), Integer(b)) | (Integer(b), BigInteger(a)) => ...
        (BigInteger(a), BigInteger(b)) => ...

        // Promote to Rational (4 patterns)
        (Rational(a), Rational(b)) => ...
        (Rational(a), Integer(b)) | (Integer(b), Rational(a)) => ...

        // Promote to Real (4 patterns)
        (Real(a), Real(b)) => ...
        (Real(a), Integer(b)) | (Integer(b), Real(a)) => ...

        // Complex (2 patterns)
        (Complex(parts1), Complex(parts2)) => ...
        (Complex(parts), other) | (other, Complex(parts)) => ...
    }
}
```

This pattern appears **identically** in:
- `add()` - 42 lines
- `subtract()` - 36 lines
- `multiply()` - 38 lines

**Total duplication:** ~126 lines of nearly identical code.

**Risk:** Bug fixes or type promotion changes must be applied to all three functions consistently.

### 1.3 Scattered Special Cases

Special value handling (NaN, infinity, zero) appears throughout the file:

| Special Case | Occurrences | Inconsistencies |
|--------------|-------------|-----------------|
| NaN handling | 7 locations | Different propagation rules |
| Infinity | 6 locations | Multiple detection strategies |
| "Essentially zero" | 3 locations | Two different thresholds (exact vs 1e-10) |
| Division by zero | 3 locations | Inexact→infinity, exact→error |

**Example inconsistency:**
- Line 1037: `if c.im.abs() < 1e-10` (complex64_to_value)
- Line 1606: `if imag.abs() < 1e-10` (make_polar)
- Line 621: `if x.is_zero()` (exact division)

These use different thresholds for the same conceptual check.

### 1.4 Function-Specific Duplication

**Rounding functions** (lines 912-1014):
- `floor()`, `ceiling()`, `truncate()`, `round()` - all have identical structure
- 4 functions × ~25 lines each = 102 lines
- Only difference: which method to call (`.floor()` vs `.ceil()` vs `.trunc()`)

**Type conversion helpers** (defined locally in multiple functions):
- `to_f64()` closure: appears in `divide()`, `atan()`, `log()`, `exact_integer_sqrt()`
- `to_rational()` closure: appears in `divide()` and elsewhere
- Same logic copy-pasted 4+ times

**Complex number conversions:**
- Three different representations: `Value::Complex`, `Complex64`, `NumericValue::Complex`
- Conversion boilerplate in every trig function (sin, cos, tan, etc.)

### 1.5 Registration Boilerplate

Lines 1742-2192 (450 lines) consist of repetitive registration code:

```rust
registry.register(PrimitiveFn::new(
    "scheme.base",
    "+",
    Arity::Min(0),
    "Returns the sum of its arguments. With no arguments, returns 0.",
    |eval, args, _tail| add(eval, args).map(EvalResult::Value),
));
```

This pattern repeats **50+ times** with minimal variation. Could be:
- Generated from a macro
- Defined in a data structure
- Moved to a separate registration module

---

## 2. Comparison with Other Files

| File | Lines | Functions | Avg Lines/Fn | Strategy |
|------|-------|-----------|--------------|----------|
| **arithmetic.rs** | 2,193 | 50+ | 44 | Match sprawl, duplication |
| strings.rs | 920 | 30+ | 31 | Simpler patterns |
| vectors.rs | 840 | 35+ | 24 | Delegates to Value methods |
| lists.rs | 682 | 40+ | 17 | Simple pattern matching |
| predicates.rs | 364 | 22 | 17 | Uses `make_type_predicate` helper |
| conversion.rs | 430 | 25+ | 17 | Straightforward conversions |

**Observation:** Files that use helper/utility functions are much more concise.

**Reference Implementation Strategy (Chibi-scheme):**
- Only essential operations implemented in C
- Derived operations defined in Scheme using primitives
- Example: `(define (odd? n) (= (remainder n 2) 1))` in `init-7.scm`
- Single representation, no wrapper types

---

## 3. Brittleness and Bug Risks

### 3.1 Type Promotion Complexity

Type promotion rules are implicit in match patterns:

```rust
// What triggers Integer → BigInteger promotion?
(Integer(a), Integer(b)) => match a.checked_add(b) {
    Some(sum) => Integer(sum),
    None => BigInteger(Self::to_bigint(a) + Self::to_bigint(b)),
}
```

**Issues:**
- No central documentation of promotion rules
- Easy to miss a case (e.g., forgot overflow check in one operation)
- Inconsistent across operations (some check overflow, some don't)

### 3.2 Division By Zero Asymmetry

```rust
// Inexact division by zero - produces infinity (lines 595-603)
if args.len() == 1 {
    let x = to_f64(&args[0])?;
    return Ok(Value::Real(1.0 / x));  // Can produce +inf.0
}

// Exact division by zero - error (lines 620-623)
if x.is_zero() {
    return Err(EvalError::DivisionByZero);
}
```

This is **correct per R7RS** but fragile:
- Easy to accidentally merge paths
- No comment explaining why they differ
- Could break if someone "fixes" the inconsistency

### 3.3 Comparison Function String Matching

Lines 466-476 match on operator name as a string:

```rust
let result = match op_name {
    "<" => a_f64 < b_f64,
    ">" => a_f64 > b_f64,
    "<=" => a_f64 <= b_f64,
    ">=" => a_f64 >= b_f64,
    _ => return Err(...),
};
```

**Problems:**
- String matching instead of using the provided closure
- Duplicates the operation logic
- Harder to extend (what if we add `<>` or other operators?)

### 3.4 Error Message Inconsistency

Each function has unique error formatting:
- `"+ expects numbers"` (no type name)
- `"/ expects numbers, got {}"` (includes type)
- `"finite? expects a number, got {}"` (different wording)

Users see inconsistent error messages across similar operations.

---

## 4. Proposed Refactoring Strategy

### Phase 1: Quick Wins (Low Risk, High Impact)

**Goal:** Reduce code by 30% without major architectural changes.

#### 4.1 Extract Common Conversion Helpers

Create a `helpers` module with standard conversions:

```rust
// crates/patina-tree-walker/src/eval/primitives/arithmetic/helpers.rs
pub(super) fn to_f64(v: &Value) -> Result<f64, EvalError> {
    match v {
        Value::Integer(n) => Ok(*n as f64),
        Value::BigInteger(n) => Ok(n.to_f64().unwrap_or(f64::INFINITY)),
        Value::Rational(r) => Ok(r.to_f64().unwrap_or(f64::INFINITY)),
        Value::Real(f) => Ok(*f),
        other => Err(EvalError::TypeError(format!(
            "Expected number, got {}", other.type_name()
        ))),
    }
}

pub(super) fn to_rational(v: &Value) -> Result<BigRational, EvalError> { ... }
pub(super) fn to_complex64(v: &Value) -> Result<Complex64, EvalError> { ... }
```

**Impact:** Eliminates 4+ duplicate closure definitions.

#### 4.2 Generalize Binary Operations

Extend the existing `binary_int_op` pattern to all numeric types:

```rust
fn binary_numeric_op<F>(
    a: &Value,
    b: &Value,
    op_name: &str,
    op: F,
) -> Result<Value, EvalError>
where
    F: Fn(NumericValue, NumericValue) -> NumericValue,
{
    let a_num = NumericValue::from_value(a.clone())?;
    let b_num = NumericValue::from_value(b.clone())?;
    Ok(op(a_num, b_num).into_value(evaluator))
}
```

**Usage:**
```rust
pub(super) fn add(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    args.into_iter()
        .try_fold(NumericValue::Integer(0), |acc, arg| {
            let num = NumericValue::from_value(arg)?;
            Ok(acc.add(num))
        })
        .map(|result| result.into_value(evaluator))
}
```

**Impact:** Simplifies add/subtract/multiply from ~40 lines each to ~8 lines each.

#### 4.3 Consolidate Rounding Functions

Create a generic rounding helper:

```rust
fn generic_round<FExact, FInexact>(
    eval: &Evaluator,
    args: Vec<Value>,
    name: &str,
    exact_op: FExact,
    inexact_op: FInexact,
) -> Result<Value, EvalError>
where
    FExact: Fn(&BigRational) -> BigInt,
    FInexact: Fn(f64) -> f64,
{
    eval.check_arity_exact(&args, 1, name)?;
    match &args[0] {
        Value::Integer(n) => Ok(Value::Integer(*n)),
        Value::BigInteger(n) => Ok(Value::BigInteger(n.clone())),
        Value::Rational(r) => Ok(Value::BigInteger(exact_op(r))),
        Value::Real(f) => Ok(Value::Real(inexact_op(*f))),
        other => Err(EvalError::TypeError(format!(
            "{} expects a real number, got {}", name, other.type_name()
        ))),
    }
}
```

**Usage:**
```rust
pub(super) fn floor(eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    generic_round(eval, args, "floor",
        |r| r.floor().numer().clone(),
        |f| f.floor())
}
```

**Impact:** Reduces 4 functions (102 lines) to ~40 lines total.

#### 4.4 Unified Special Value Handling

Create a `special_values` module:

```rust
pub(super) const ESSENTIALLY_ZERO_THRESHOLD: f64 = 1e-10;

pub(super) fn is_nan(v: &Value) -> bool {
    match v {
        Value::Real(f) => f.is_nan(),
        Value::Complex(r, i) => r.is_nan() || i.is_nan(),
        _ => false,
    }
}

pub(super) fn is_infinite(v: &Value) -> bool {
    match v {
        Value::Real(f) => f.is_infinite(),
        Value::Complex(r, i) => r.is_infinite() || i.is_infinite(),
        _ => false,
    }
}

pub(super) fn handle_nan_propagation(args: &[Value]) -> Option<Value> {
    if args.iter().any(is_nan) {
        Some(Value::Real(f64::NAN))
    } else {
        None
    }
}
```

**Impact:** Single source of truth for special value logic.

**Estimated reduction from Phase 1:** ~30% (2,193 → ~1,500 lines)

---

### Phase 2: Structural Refactoring (Medium Risk, High Maintainability)

**Goal:** Improve modularity and organization.

#### 4.5 Split Into Multiple Files

```
crates/patina-tree-walker/src/eval/primitives/arithmetic/
├── mod.rs              # Public API and registration
├── helpers.rs          # NumericValue, conversion utilities
├── special_values.rs   # NaN, infinity, zero handling
├── basic_ops.rs        # +, -, *, /
├── comparison.rs       # =, <, >, <=, >=
├── integer_ops.rs      # quotient, remainder, modulo, gcd, lcm
├── rounding.rs         # floor, ceiling, truncate, round
├── transcendental.rs   # sin, cos, tan, asin, acos, atan, exp, log
├── complex_ops.rs      # make-rectangular, magnitude, angle, etc.
└── rational_ops.rs     # numerator, denominator, rationalize
```

**Benefits:**
- Easier to navigate (each file 150-300 lines)
- Clearer separation of concerns
- Independent testing of each module
- Easier to onboard new contributors

#### 4.6 Macro-Based Registration

Replace 450 lines of registration boilerplate with a declarative macro:

```rust
macro_rules! register_arithmetic_primitives {
    ($registry:expr, [
        $(
            $namespace:literal :: $name:literal
            ( $arity:expr )
            => $func:ident
            , $doc:literal
        ),* $(,)?
    ]) => {
        $(
            $registry.register(PrimitiveFn::new(
                $namespace,
                $name,
                $arity,
                $doc,
                |eval, args, _tail| $func(eval, args).map(EvalResult::Value),
            ));
        )*
    };
}

// Usage
register_arithmetic_primitives!(registry, [
    "scheme.base" :: "+" (Arity::Min(0)) => add,
        "Returns the sum of its arguments. With no arguments, returns 0.",
    "scheme.base" :: "-" (Arity::Min(1)) => subtract,
        "Subtracts subsequent arguments from the first. With one argument, returns its negation.",
    // ...
]);
```

**Impact:** Registration code reduced from 450 lines to ~100 lines.

**Estimated total reduction after Phase 2:** ~40% (2,193 → ~1,300 lines)

---

### Phase 3: Architectural Changes (Higher Risk, Long-term)

**Goal:** Eliminate root causes of complexity.

#### 4.7 Move Methods to Value Enum

**Current situation:** Helper methods split between `Value` and `NumericValue`.

**Proposal:** Add to `patina-runtime/src/value/mod.rs`:

```rust
impl Value {
    pub fn is_exact(&self) -> bool {
        matches!(self,
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_))
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Value::Integer(n) => *n == 0,
            Value::BigInteger(n) => n.is_zero(),
            Value::Rational(r) => r.is_zero(),
            Value::Real(f) => *f == 0.0,
            Value::Complex(r, i) => *r == 0.0 && *i == 0.0,
            _ => false,
        }
    }

    pub fn to_f64(&self) -> Result<f64, ConversionError> { ... }
    pub fn to_rational(&self) -> Result<BigRational, ConversionError> { ... }
}
```

**Benefits:**
- Eliminates `NumericValue` wrapper entirely
- Methods available to all code, not just arithmetic.rs
- Reduces conversion overhead
- Single source of truth

**Risks:**
- Requires changes to `patina-runtime` crate
- Affects all consumers of `Value`
- Needs careful consideration of public API

#### 4.8 Defer to Scheme Implementations

Following chibi-scheme's strategy, move non-primitive operations to `lib/bootstrap.scm`:

```scheme
; Current: implemented in Rust
(define (odd? n) (= (remainder n 2) 1))
(define (even? n) (= (remainder n 2) 0))

; Potential candidates:
(define (square x) (* x x))
(define (cube x) (* x x x))
```

**Candidates for Scheme implementation:**
- `square` (currently 7 lines of Rust)
- `odd?`, `even?` (if not already in bootstrap.scm)
- Potentially `max`, `min` (though performance may matter)

**Benefits:**
- Reduces Rust code
- Demonstrates Scheme's expressiveness
- Easier for Scheme developers to understand
- Dogfooding: uses your own primitives

**Tradeoffs:**
- Slight performance cost (interpreted vs native)
- Longer stack traces on errors

#### 4.9 Trait-Based Numeric Operations

Define a trait for numeric operations:

```rust
pub trait NumericOp {
    fn add(&self, other: &Value) -> Result<Value, EvalError>;
    fn subtract(&self, other: &Value) -> Result<Value, EvalError>;
    fn multiply(&self, other: &Value) -> Result<Value, EvalError>;
    fn is_exact(&self) -> bool;
    fn to_f64(&self) -> f64;
}

impl NumericOp for Value {
    fn add(&self, other: &Value) -> Result<Value, EvalError> {
        // Type promotion and dispatch logic
    }
    // ...
}
```

**Benefits:**
- Clear contract for numeric operations
- Extensible (can add new numeric types)
- Type-safe dispatch

**Risks:**
- More complex than current approach
- May not play well with Scheme's dynamic typing
- Requires significant refactoring

**Status:** Defer until after Phase 1-2 complete. Evaluate if still needed.

---

## 5. Implementation Roadmap

### Milestone 1: Quick Wins (1-2 days)
- [ ] Extract conversion helpers (`to_f64`, `to_rational`, `to_complex64`)
- [ ] Extract special value helpers (`is_nan`, `is_infinite`, `ESSENTIALLY_ZERO_THRESHOLD`)
- [ ] Consolidate rounding functions using `generic_round`
- [ ] Test: all existing arithmetic tests pass

**Success criteria:** 20-30% code reduction, no new bugs

### Milestone 2: Binary Operation Abstraction (1 day)
- [ ] Create `binary_numeric_op` helper
- [ ] Refactor `add`, `subtract`, `multiply` to use helper
- [ ] Test: numeric tower tests pass, property-based tests if available

**Success criteria:** Eliminate 100+ lines of duplicate match logic

### Milestone 3: File Split (1-2 days)
- [ ] Create `arithmetic/` subdirectory structure
- [ ] Move functions to appropriate modules
- [ ] Update imports and module structure
- [ ] Test: full test suite passes

**Success criteria:** No single file > 500 lines

### Milestone 4: Registration Simplification (half day)
- [ ] Create registration macro
- [ ] Migrate all registrations to macro
- [ ] Test: all primitives still accessible

**Success criteria:** Registration code reduced to ~100 lines

### Milestone 5: Evaluation and Planning (half day)
- [ ] Measure total code reduction
- [ ] Document remaining complexity
- [ ] Decide if Phase 3 is needed
- [ ] Update this document with lessons learned

**Total estimated time:** 4-6 days

---

## 6. Testing Strategy

### Regression Testing
- [ ] All existing arithmetic tests must pass
- [ ] Chibi comparison tests must pass
- [ ] R7RS compliance tests must pass

### New Tests
- [ ] Edge cases for special values (NaN, infinity)
- [ ] Overflow behavior tests
- [ ] Type promotion tests
- [ ] Error message consistency tests

### Performance Testing
- [ ] Benchmark before/after refactoring
- [ ] Ensure no regression in numeric operation performance
- [ ] Profile to find any conversion overhead

---

## 7. Success Metrics

### Quantitative Goals
- **Code reduction:** 30-40% (2,193 → ~1,300 lines)
- **Largest file size:** < 500 lines
- **Registration boilerplate:** < 100 lines
- **Pattern duplication:** < 10% (currently ~15%)

### Qualitative Goals
- **Readability:** New contributor can understand numeric tower in < 1 hour
- **Maintainability:** Adding new operation requires < 50 lines of code
- **Consistency:** All operations use same conversion helpers
- **Documentation:** Clear explanation of type promotion rules

---

## 8. Risks and Mitigation

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Introduce numeric bugs | High | Medium | Extensive regression testing |
| Break existing tests | High | Low | Run tests after each change |
| Performance regression | Medium | Low | Benchmark before/after |
| Scope creep (Phase 3) | Medium | Medium | Stick to Phase 1-2 initially |
| Merge conflicts | Low | Medium | Work in feature branch |

---

## 9. Open Questions

1. **Should `NumericValue` be eliminated entirely?**
   - Pro: Removes dual type system
   - Con: Requires changes to `patina-runtime`
   - Decision: Defer to Phase 3, evaluate after Phase 1-2

2. **Should we implement numeric operations as a trait?**
   - Pro: Type-safe, extensible
   - Con: Complex, may not fit Scheme's dynamism
   - Decision: Defer, revisit if complexity remains

3. **Which operations should be moved to Scheme?**
   - Candidates: `square`, `odd?`, `even?`
   - Decision: Start with `square` as proof-of-concept

4. **Should we use a different threshold for "essentially zero"?**
   - Current: 1e-10 in some places, exact in others
   - Decision: Document current behavior, unify in Phase 1

---

## 10. References

- **Main file:** `crates/patina-tree-walker/src/eval/primitives/arithmetic.rs`
- **R7RS spec:** Section 6.2 (Numbers)
- **Chibi reference:** `internal/reference_impls/CHIBI_REFERENCE.md`
- **Numeric tower design:** `PRD/phase1/NUMERIC_SUMMARY.md`
- **Related files:**
  - `crates/patina-runtime/src/value/mod.rs` - Value enum definition
  - `crates/patina-tree-walker/src/eval/primitives/mod.rs` - Primitive registration

---

## 11. Conclusion

The arithmetic primitives file has grown organically to 2,193 lines through incremental feature additions. The complexity is **accidental** (from architecture choices), not **essential** (inherent to numeric tower).

**Phase 1-2 refactoring** can reduce this to ~1,300 lines with:
- Better abstractions (generic helpers)
- Eliminated duplication (consolidate patterns)
- Improved organization (file splitting)

**Phase 3 architectural changes** are optional and should only be pursued if:
- Phase 1-2 doesn't sufficiently reduce complexity
- Team has bandwidth for larger refactoring
- Benefits clearly outweigh risks

The proposed approach balances **pragmatism** (quick wins first) with **vision** (architectural improvements later).
