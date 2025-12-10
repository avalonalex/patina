# Chibi-Scheme Numeric Tower Analysis for Patina

**Status**: Supporting Reference Documentation

> **For implementation guidance**, start with `PRD/phase1/NUMERIC_SUMMARY.md` (canonical guide).
>
> These documents provide deep-dive analysis of chibi-scheme's implementation
> to inform design decisions and provide code patterns.

This directory contains comprehensive analysis of chibi-scheme's numeric tower implementation, extracted to help guide Patina's numeric design decisions.

## Documents Included

### 1. `chibi_numeric_tower_analysis.md` (643 lines)
**Comprehensive reference document** covering all aspects of chibi-scheme's numeric implementation:

- Core numeric type representation (fixnums, bignums, flonums, ratios, complex)
- Exactness tracking strategy (implicit through types, no metadata)
- Number parsing with exactness markers (#e, #i)
- Arithmetic operations and overflow handling
- Type predicates and promotion rules
- IEEE 754 special value handling
- Design patterns and clever tricks
- Performance insights
- Comparison to Patina's current approach
- Recommendations for improvement

**Best for**: Understanding how chibi implements everything end-to-end

### 2. `numeric_implementation_guide.md` (194 lines)
**Quick reference guide** for implementation patterns:

- File locations in chibi source with line numbers
- Critical code sections (copy-paste friendly)
- Exactness rules and predicates
- Promotion pipeline diagram
- Overflow handling path
- Division semantics (/, quotient, remainder)
- IEEE 754 special values
- Complex number handling
- Implementation checklist
- Performance insights

**Best for**: Quick lookups during coding

### 3. `rust_numeric_patterns.md` (508 lines)
**Rust-specific adaptation guide** showing how to apply chibi patterns to Patina:

- Type representation options
- Ratio implementation with normalization
- Exactness checking without metadata
- Overflow detection & promotion
- Division semantics in Rust
- Complex number handling
- Special IEEE 754 value parsing
- Type promotion rules
- Implementation checklist
- Testing strategy examples

**Best for**: Implementing numeric tower in Patina's Rust codebase

## Key Insights from Chibi-Scheme

### 1. Exactness is Type-Implicit
- No metadata flags needed
- Fixnum/BigInteger/Ratio = exact
- Flonum = always inexact
- Complex = depends on components
- This simplifies reasoning significantly

### 2. Tagged Integers (Fixnums)
- Chibi uses low-bit tagging for immediate values
- Zero allocation for common case
- Type checking is single bitwise AND
- Overflow automatically promotes to bignum

### 3. Ratio Normalization
- Ratios must be normalized on creation (GCD reduction)
- Denominator always positive
- Ensures canonical representation
- Simplifies equality checking

### 4. Type-Based Promotion
```
Integer overflow -> BigInteger -> Ratio (with /) -> Flonum
All transparent to user
```

### 5. Division Semantics Matter
- `/` returns exact result (ratio if enabled, else flonum)
- `quotient` returns integer division (toward zero)
- `remainder` uses Euclidean semantics
- Not equivalent operations!

### 6. Complex = Pairs of Reals
- Both real and imag can be any numeric type
- Simplifies representation
- Optimization: if imag is zero, just return real

### 7. IEEE 754 Compliance
- Support +inf.0, -inf.0, +nan.0
- Preserve negative zero (-0.0)
- NaN propagation
- Proper handling of infinity

## Recommended Implementation Priority for Patina

### Phase 1 (Core Numeric Behavior)
1. Make exactness implicit through types (no flags)
2. Implement ratio normalization on creation
3. Add overflow detection to integer operations
4. Implement transparent bignum promotion
5. Separate `/` from `quotient`/`remainder`

### Phase 2 (Parsing & Conversion)
1. Parse exactness markers (#e, #i)
2. Parse rational literals (3/5)
3. Implement exact->inexact conversion
4. Implement inexact->exact conversion
5. Handle IEEE 754 special values

### Phase 3 (Complex & Advanced)
1. Implement complex numbers as pairs
2. Optimize away zero imaginary part
3. Recursive conversion for complex
4. Full R7RS numeric compliance
5. Performance optimization

## Quick Start: Applying to Patina

1. **Read** `rust_numeric_patterns.md` sections 1-5 for basic patterns
2. **Reference** `numeric_implementation_guide.md` for specific code patterns
3. **Deep dive** `chibi_numeric_tower_analysis.md` for design rationale
4. **Check** the checklists in both guides for what to implement

## Key Files in Chibi-Scheme Source

| Task | File | Lines |
|------|------|-------|
| Type definitions | sexp.h | 438-620 |
| Fixnum operations | sexp.h | 1537-1544 |
| Type predicates | sexp.h | 1007-1020 |
| Number parsing | sexp.c | 2933-3105 |
| VM arithmetic | vm.c | 1763-1950+ |
| Conversions | eval.c | 1903-1978 |

Reference directory: `~/Project/reference/chibi-scheme`

## Important Design Decisions

### What Chibi Got Right
- Type-implicit exactness (elegant)
- Transparent overflow promotion (user-friendly)
- Separate division operations (semantically correct)
- Lazy ratio normalization (efficient)
- Complex as pairs (flexible)

### What Patina Should Consider
- Don't add exactness metadata - let type decide
- Do normalize ratios on creation (not lazily)
- Do handle overflow transparently
- Do support IEEE 754 special values
- Do optimize complex with zero imag part

### What to Avoid
- Don't make exactness a property of values
- Don't store unnormalized ratios
- Don't mix / and quotient semantics
- Don't lose precision in conversions

## Testing Notes from Chibi

- Comprehensive R7RS test suite in tests/r7rs-tests.scm (2516 lines)
- Tests all numeric combinations and edge cases
- Validates against R7RS spec strictly
- Can cross-reference for compatibility checking

## Performance Considerations

Fast paths:
- Fixnum arithmetic (bit manipulation)
- Type checking (single AND)

Slow paths:
- Bignum arithmetic (word-by-word)
- Ratio creation (GCD computation)
- Flonum boxing (allocation)

## Additional Resources

- R7RS-small spec: `~/Project/patina/spec/r7rs-small-spec/`
- Reference chibi: `~/Project/reference/chibi-scheme`
- Patina source: `~/Project/patina`
- Test suite example: `tests/r7rs-tests.scm` (2516 lines)

---

**Analysis Date**: November 4, 2025
**Source**: Chibi-Scheme reference implementation
**Status**: Ready for Patina implementation
