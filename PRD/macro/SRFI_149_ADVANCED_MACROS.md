# SRFI-149: Advanced Macro Patterns (Future Enhancement)

**Status**: Future consideration
**Related**: Phase 2+ macro system enhancements
**Date**: 2025-11-22

## Overview

This document catalogs advanced `syntax-rules` patterns that were removed from Patina's test suite because they are **NOT part of R7RS-small**. These patterns are defined in **SRFI-149: Basic Syntax-rules Template Extensions** and supported by Racket and some other Scheme implementations.

While these patterns are not necessary for R7RS compliance, they enable powerful macro transformations and could be considered for future phases of Patina development.

## Background

During Phase 1 development, 13 macro tests were found to test non-standard patterns. These tests were verified against:
- **Chibi-Scheme 0.11** (reference R7RS implementation)
- **Gauche** (another popular R7RS implementation)

Both implementations explicitly reject these patterns, confirming they are language extensions.

## SRFI-149 Extensions

SRFI-149 introduces two key relaxations to R7RS `syntax-rules`:

### 1. Multiple Consecutive Ellipses (`... ...`)

Allows adjacent ellipses in templates to create Cartesian product-like operations.

**Example: Cartesian Product**
```scheme
(syntax-rules ()
  ((cartesian (a ...) (b ...))
   ((list a b) ... ...)))

;; Usage: (cartesian (1 2 3) (x y))
;; Result: ((1 x) (1 y) (2 x) (2 y) (3 x) (3 y))
```

**R7RS Error**: "too many ...'s"

### 2. Excess Ellipses in Templates

Allows pattern variables to have more `...` in templates than in their patterns.

**Example: Cross Sum**
```scheme
(syntax-rules ()
  ((cross-sum (a ...) (b ...))
   ((+ a b) ... ...)))

;; Usage: (cross-sum (1 2) (10 20 30))
;; Result: ((+ 1 10) (+ 1 20) (+ 1 30) (+ 2 10) (+ 2 20) (+ 2 30))
```

## Removed Pattern Catalog

### Category 1: Double Ellipsis (`... ...`) Patterns

These patterns use adjacent or sequential ellipses to create combinations.

**1. `cartesian` - Cartesian Product**
```scheme
(syntax-rules ()
  ((cartesian (a ...) (b ...))
   ((list a b) ... ...)))
```
- Creates all combinations of two lists
- Chibi: "too many ...'s"
- Gauche: "invalid application"

**2. `cross-sum` - Cross Operation with Computation**
```scheme
(syntax-rules ()
  ((cross-sum (a ...) (b ...))
   ((+ a b) ... ...)))
```
- Applies operation to all combinations
- Chibi: "too many ...'s"
- Gauche: "invalid application"

**3. `make-setters` - Generate Multiple Definitions**
```scheme
(syntax-rules ()
  ((make-setters (name field ...) ...)
   ((define (name val) (set! field val)) ... ...)))
```
- Generates multiple setter functions
- Chibi: Definition accepted but expansion fails
- Gauche: Definition accepted but can't execute

**4. `pair-product` - Complex Product with Pairs**
```scheme
(syntax-rules ()
  ((pair-product ((a . b) ...) ((c . d) ...))
   (((a . c) (b . d)) ... ...)))
```
- Cartesian product on dotted pairs
- Chibi: "too many ...'s"
- Gauche: "proper list required"

**5. `alternate` - Alternating Pattern Depths**
```scheme
(syntax-rules ()
  ((alternate (a ...) ((b c) ...))
   ((list a b c) ... ...)))
```
- Combines flat and nested patterns
- Chibi: "too many ...'s"
- Gauche: "invalid application"

**6. `matrix-flatten` - Flatten with Double Ellipsis**
```scheme
(syntax-rules ()
  ((matrix-flatten (((a ...) ...) ...))
   ((a ... ...) ...)))
```
- Partial flattening of nested structure
- Chibi: "non procedure application"
- Gauche: "invalid application"

### Category 2: Multiple Ellipsis at Same Level (`a ... b ...`)

These patterns have multiple ellipsis expressions at the same template level.

**7. `flatten-pairs` - Interleave Elements**
```scheme
(syntax-rules ()
  ((flatten-pairs ((a b) ...))
   (a ... b ...)))

;; Usage: (flatten-pairs ((1 2) (3 4) (5 6)))
;; Intended: (1 3 5 2 4 6)
```
- Chibi: "invalid application"
- Gauche: "invalid application"

**8. `interleave` - Similar Pattern**
```scheme
(syntax-rules ()
  ((interleave ((a b) ...))
   (a ... b ...)))
```
- Same issue as flatten-pairs
- Produces invalid code

### Category 3: Triple/Quadruple Nesting

Patterns with extreme nesting levels.

**9. `complex-nesting` (matrix) - Triple Nesting**
```scheme
(syntax-rules ()
  ((matrix ((item ...) ...) ...)
   (list (list (list item ...) ...) ...)))
```
- Chibi: "non procedure application"
- Gauche: "invalid application"

**10. `deep-nest` - Quadruple Nesting**
```scheme
(syntax-rules ()
  ((deep-nest ((((x ...) ...) ...) ...))
   (((x ...) ...) ...)))
```
- Chibi: "too few ...'s"
- Gauche: "Pattern variable x is used in wrong level"

### Category 4: Invalid Expansions

Patterns that expand to syntactically invalid code.

**11. `nested-map` - Function Application Issue**
```scheme
(syntax-rules ()
  ((nested-map f ((x ...) ...))
   ((f x ...) ...)))
```
- Produces code like `((list 1 2 3) ...)` which tries to call a list
- Chibi: "non procedure application"
- Gauche: "invalid application"

**12. `prefix-all` - Similar Issue**
```scheme
(syntax-rules ()
  ((prefix-all prefix (elem ...) ...)
   ((prefix elem ...) ...)))
```
- Same problem as nested-map
- Chibi: "undefined variable"
- Gauche: "invalid application"

**13. `broadcast` - Single Element Distribution**
```scheme
(syntax-rules ()
  ((broadcast x (y ...))
   ((list x y) ...)))
```
- Chibi: "invalid application"
- Gauche: "invalid application"

## Valid R7RS Pattern (Kept)

Only **one** pattern from the original tests is valid R7RS:

**`multi-list` - Simple Double Nesting**
```scheme
(syntax-rules ()
  ((multi-list (item ...) ...)
   (list (list item ...) ...)))

;; Usage: (multi-list (1 2) (3 4))
;; Result: ((1 2) (3 4))
```
- ✅ **PASS in Chibi**
- ✅ **PASS in Gauche**
- Status: Valid R7RS, kept as `test_nested_ellipsis_macro` (currently ignored)

This works because each ellipsis operates at a different nesting level.

## Implementation Support

### R7RS-small (Patina's Target)
- ❌ Does NOT support SRFI-149 patterns
- ✅ Supports simple double-nesting: `((item ...) ...)`
- Reference implementations (Chibi, Gauche) explicitly reject these patterns

### Racket
- ✅ Supports via special escaping syntax
- Use `(... ...)` to generate an inner ellipsis
- Use `(... <form>)` to escape ellipsis in templates
- See: https://docs.racket-lang.org/reference/stx-patterns.html

### SRFI-149 Implementations
- Chibi-Scheme (with SRFI-149 library loaded)
- Kawa
- Some other Scheme implementations
- See: https://srfi.schemers.org/srfi-149/srfi-149.html

### Why Not in R7RS

The core issue is **ellipsis scoping**: by convention, `...` belongs to the outermost macro layer. R7RS keeps this restriction for:
- **Simplicity**: Easier to understand and implement
- **Portability**: Consistent behavior across implementations
- **Clarity**: Unambiguous semantics

SRFI-149 relaxes these rules but adds complexity to the macro expander.

## Use Cases for SRFI-149

If implemented, these patterns would enable:

1. **Cartesian Products**: Generate all combinations of elements
2. **Cross Operations**: Apply operations across multiple sequences
3. **Matrix Transformations**: Flatten, reshape nested structures
4. **Code Generation**: Create multiple similar definitions
5. **Advanced Metaprogramming**: More expressive macro transformations

## Future Considerations

### Phase 2+ Enhancement

If Patina adds SRFI-149 support:

**Pros:**
- More powerful macro system
- Closer to Racket compatibility
- Enables advanced metaprogramming patterns

**Cons:**
- Adds complexity to macro expander
- Potential performance impact
- Diverges from strict R7RS compliance
- Edge cases and implementation challenges

**Recommendation**: Consider as **optional extension** in Phase 2 or later, not core functionality.

### Implementation Path

If pursuing SRFI-149:

1. Implement basic SRFI-149 patterns in macro expander
2. Add feature flag or library: `(import (srfi 149))`
3. Re-enable removed tests under SRFI-149 feature
4. Document as language extension, not R7RS feature
5. Ensure R7RS mode remains default

## References

- **SRFI-149 Specification**: https://srfi.schemers.org/srfi-149/srfi-149.html
- **Racket Pattern Matching**: https://docs.racket-lang.org/reference/stx-patterns.html
- **Stack Overflow Discussion**: https://stackoverflow.com/questions/68741400/nested-ellipsis-macro-doesnt-work-in-guile-and-racket
- **More Macro Issues**: https://stackoverflow.com/questions/52549740/more-macro-woes

## Verification Results

All patterns were tested on 2025-11-22 with:
- Chibi-Scheme 0.11
- Gauche (latest)

Test results confirmed none of these patterns work in standard R7RS implementations. Only the simple double-nesting pattern `((item ...) ...)` is valid R7RS.

See test script in commit history for reproduction steps.

---

**Decision**: Removed from Patina test suite. Documented here for future consideration if advanced macro features are desired in Phase 2+.
