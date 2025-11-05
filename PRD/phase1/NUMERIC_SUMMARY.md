# Numeric Tower - Summary and Recommendations

**Date**: 2025-11-04
**Status**: ⭐ **CANONICAL IMPLEMENTATION GUIDE** ⭐

> **This is the authoritative document for numeric tower implementation.**
> All other documents are supporting references.

## TL;DR

After analyzing the R7RS spec and chibi-scheme reference implementation, we discovered our **current Value enum is already 80% correct**. We just need to:

1. Fix Complex representation (use `Box<(Value, Value)>` instead of `(f64, f64)`)
2. Add proper division semantics (`/` returns exact rationals, not floats)
3. Add overflow detection and BigInt promotion
4. Implement the numeric operations

**No new dependencies needed!** Everything we need is already in `num-bigint`, `num-rational`, and `num-traits`.

## The Key Insight: Exactness is Type-Implicit

From chibi-scheme analysis:

```rust
// NO EXACTNESS FLAGS NEEDED!
pub enum Value {
    Integer(i64),           // Always exact
    BigInteger(BigInt),     // Always exact
    Rational(BigRational),  // Always exact
    Real(f64),              // Always inexact
    Complex(Box<(Value, Value)>),  // Depends on components
}

// Checking exactness is just pattern matching
fn is_exact(v: &Value) -> bool {
    matches!(v, Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_))
}
```

This eliminates the need for separate `ExactInteger`/`InexactInteger` variants!

## Critical Implementation Points

### 1. Division MUST Return Exact Results

```rust
// CORRECT (chibi-scheme way)
(/ 1 2)    => Rational(1/2)   // Exact!
(/ 6 3)    => Integer(2)      // Simplifies to integer
(/ 1 0)    => ERROR           // Exact zero divisor

// WRONG (naive float division)
(/ 1 2)    => Real(0.5)       // Lost exactness!
```

### 2. Quotient ≠ Division

```rust
/         -> exact division (returns Rational)
quotient  -> integer division (truncates toward zero)
remainder -> remainder after quotient
modulo    -> floor-remainder (always non-negative)
```

### 3. Overflow Must Promote to BigInt

```rust
fn multiply(a: i64, b: i64) -> Value {
    match a.checked_mul(b) {
        Some(result) => Value::Integer(result),
        None => Value::BigInteger(BigInt::from(a) * BigInt::from(b))
    }
}
```

### 4. Rationals Must Be Normalized

```rust
// When creating a rational
let ratio = BigRational::new(n, d);  // num-rational normalizes automatically!

// But we must simplify to integer if denominator is 1
if ratio.denom() == &BigInt::from(1) {
    Value::Integer(ratio.numer().to_i64().unwrap())
} else {
    Value::Rational(ratio)
}
```

## Documentation Created

### 1. Chibi-Scheme Analysis (in `internal/`)
- `README_NUMERIC_ANALYSIS.md` - Index and quick start
- `chibi_numeric_tower_analysis.md` - Complete analysis (643 lines)
- `numeric_implementation_guide.md` - Quick reference (194 lines)
- `rust_numeric_patterns.md` - Rust-specific patterns (508 lines)

### 2. Design Documents (in `PRD/phase1/`)
- `NUMERIC_TOWER_DESIGN.md` - Original design (comprehensive but complex)
- `NUMERIC_TOWER_REVISED.md` - **Recommended design** (simpler, chibi-based)
- `NUMERIC_SUMMARY.md` - This document

## Implementation Plan (10-13 days)

### Phase 1: Core Numeric Types (2-3 days)
- [x] Value enum already correct!
- [ ] Add ratio normalization on division
- [ ] Add overflow detection to arithmetic
- [ ] Implement transparent BigInt promotion

### Phase 2: Division Semantics (1-2 days)
- [ ] Fix `/` to return exact rationals
- [ ] Implement `quotient`, `remainder`, `modulo`
- [ ] Add division-by-zero error handling
- [ ] Enable GCD test cases

### Phase 3: Type Predicates (1 day)
- [ ] Implement `exact?`, `inexact?`
- [ ] Implement `exact-integer?`
- [ ] Implement `finite?`, `infinite?`, `nan?`

### Phase 4: Exactness Conversion (1 day)
- [ ] Implement `exact` procedure
- [ ] Implement `inexact` procedure
- [ ] Handle special float values

### Phase 5: Complex Numbers (2 days)
- [ ] Change Complex to `Box<(Value, Value)>`
- [ ] Implement `make-rectangular`, `make-polar`
- [ ] Implement `real-part`, `imag-part`, `magnitude`, `angle`

### Phase 6: Parser Updates (1 day)
- [ ] Parse `#e` and `#i` prefixes
- [ ] Parse rational literals `3/5`
- [ ] Parse `+inf.0`, `-inf.0`, `+nan.0`

### Phase 7: Advanced Operations (2-3 days)
- [ ] Implement rounding: `floor`, `ceiling`, `truncate`, `round`
- [ ] Implement `numerator`, `denominator`
- [ ] Implement `gcd`, `lcm`
- [ ] Implement `exact-integer-sqrt`

## What This Unlocks

Once the numeric tower is complete, we can:

1. **Enable GCD algorithms** - The test cases are already written!
   ```scheme
   (define (gcd a b)
     (if (= b 0)
         a
         (let-values (((q r) (values (quotient a b) (remainder a b))))
           (gcd b r))))
   ```

2. **Implement `odd?` and `even?`** - They need `remainder`
   ```scheme
   (define (odd? n) (= (remainder n 2) 1))
   (define (even? n) (= (remainder n 2) 0))
   ```

3. **Full arithmetic compliance** - Match chibi-scheme behavior exactly

4. **Enable more R7RS tests** - Many tests are blocked on numeric operations

## Current Status

**Patina's Value enum** (src/value/mod.rs):
```rust
Integer(i64)              // ✅ Correct
BigInteger(BigInt)        // ✅ Correct
Rational(BigRational)     // ✅ Correct (needs normalization)
Real(f64)                 // ✅ Correct
Complex(f64, f64)         // ⚠️ Should be Box<(Value, Value)>
```

**Test results**: 106/128 passing (83%)

**Blocked tests**:
- 2 tests for `odd?`/`even?` (need `remainder`)
- 3 tests for GCD algorithms (need `quotient`/`remainder`)
- 10+ tests for other numeric operations

## Recommendation

**Start with Phase 2** (Division Semantics) because:
1. It's the most critical for correctness
2. It unblocks the GCD tests (already written!)
3. It's relatively self-contained
4. It will reveal if we need Phase 1 changes first

**Quick win**: Just implementing `quotient`, `remainder`, and `modulo` would:
- Enable `odd?` and `even?` predicates
- Enable 3 GCD test cases
- Add 5 more passing tests
- Demonstrate the power of the numeric tower

## Document Hierarchy

**THIS DOCUMENT (NUMERIC_SUMMARY.md)** is the canonical guide.

**Supporting references** (consult when needed):
- `NUMERIC_TOWER_REVISED.md` - Detailed design with code examples
- `internal/rust_numeric_patterns.md` - Rust implementation patterns
- `internal/chibi_numeric_tower_analysis.md` - Deep dive into chibi source
- `internal/numeric_implementation_guide.md` - Quick code reference

**External references**:
- R7RS Spec: `spec/r7rs-small-spec/` (Section 6.2 - Numbers)
- Chibi-Scheme: `~/Project/reference/chibi-scheme`

**Deprecated**:
- `NUMERIC_TOWER_DESIGN.md` - Original design (superseded by REVISED version)

## Questions to Consider

1. **Start with Phase 2 (division) or Phase 1 (complex change)?**
   - Recommendation: Phase 2 - it's more self-contained

2. **Break Complex representation in one PR or gradually?**
   - Recommendation: One PR - complex numbers barely used yet

3. **How much testing before merging?**
   - Recommendation: Each phase should have tests and pass existing suite

4. **Should we implement all of Phase 7 or defer some?**
   - Recommendation: Defer transcendental functions, do integer operations first

## Next Action

**Awaiting your approval** to start implementation. Recommended sequence:

1. Review `NUMERIC_TOWER_REVISED.md` for full details
2. Decide on starting phase (recommend Phase 2)
3. Create feature branch for numeric tower work
4. Implement phase by phase with tests

**Estimated time to full numeric tower**: 10-13 days of focused work.
