# Patina Known Limitations

This document tracks known limitations and edge cases in Patina's R7RS implementation.

**Last Updated:** 2025-12-04

---

# 1. Nested Ellipsis

## Status: ✅ IMPLEMENTED (as of 2025-12-04)

**Previous Status:** Not Implemented
**Current Status:** Working - all tests pass

Nested ellipsis patterns (e.g., `(expr ...) ...`) are now fully supported.

### Example (Works)

```scheme
(define-syntax multi-begin
  (syntax-rules ()
    ((multi-begin (expr ...) ...)
     (begin expr ... ...))))

;; Usage:
(multi-begin
  ((set! x 1) (set! x (+ x 1)))    ; First group
  ((set! x (+ x 10)) (set! x (+ x 20))))  ; Second group

;; Expands to:
(begin
  (set! x 1)
  (set! x (+ x 1))
  (set! x (+ x 10))
  (set! x (+ x 20)))
```

### Tests

- `crates/patina-tests/tests/compliance/macros_advanced.rs:348` - `test_nested_ellipsis` ✅
- `crates/patina-tests/tests/macro_expander_interface.rs:144` - `test_nested_ellipsis_macro` ✅

---

# 2. Literal Matching Edge Case: Binding Before Macro Definition

## Status: Known Limitation (Minor)

**Added:** 2025-12-04

There is an edge case in literal identifier matching where Patina differs from chibi-scheme and Gauche.

## The Edge Case

When a literal identifier is bound BEFORE the macro is defined, and that same binding is used in the macro invocation:

```scheme
;; Case A: Binding exists BEFORE macro definition
(let ((k 999))                              ; k bound here
  (let-syntax ((n (syntax-rules (k)         ; k used as literal
                    ((n k) 'matched)
                    ((n x) 'no-match))))
    (n k)))                                  ; k from outer let

;; chibi-scheme / Gauche: 'matched
;; Patina: 'no-match
```

Compare to:

```scheme
;; Case B: Binding created AFTER macro definition
(let-syntax ((n (syntax-rules (k)           ; k used as literal
                  ((n k) 'matched)
                  ((n x) 'no-match))))
  (let ((k 999))                            ; k bound AFTER macro
    (n k)))                                  ; different binding

;; chibi-scheme / Gauche / Patina: 'no-match (all correct)
```

## Why This Happens

Patina uses a `shadowed_names` approach to detect when literal identifiers have been shadowed by local bindings at the use-site. This correctly handles Case B (binding after macro), but cannot distinguish Case A because:

1. At the use-site `(n k)`, we see that `k` is bound locally
2. We add `k` to `shadowed_names` to prevent matching
3. But we don't track WHEN the binding was created relative to macro definition

In Case A, the `k` in the literal list and the `k` at the use-site actually refer to the **same binding** (the outer let), so they should match. Our `shadowed_names` doesn't capture this distinction.

## Technical Details

**R7RS Section 4.3.2** specifies that literal matching uses `bound-identifier=?` semantics:

> "A literal identifier matches an input identifier if both have the same binding, or both are unbound and have the same name."

The key phrase is "same binding" - in Case A, both the literal `k` in the pattern and the `k` at the use-site refer to the same binding (the outer let).

**What would be needed to fix:**

The macro expander would need to:
1. Track the actual binding (not just the name) when capturing literals
2. At use-site, compare the actual bindings rather than just checking if a name is in `shadowed_names`

This is conceptually similar to what chibi does with its `identifier-syntax` approach where identifiers carry their binding context.

## Practical Impact

**Very Low** - This pattern is rare in practice:

1. Most literals are reserved keywords (`else`, `=>`, `_`) that are never bound as variables
2. When literals ARE variable names, they're typically unbound (free identifiers)
3. Binding a variable, then using it as a macro literal, then using it in the macro is unusual

Common patterns that work correctly:

```scheme
;; Standard else literal - works fine
(cond (else 'default))

;; Standard => literal - works fine
(case x ((1 2) => handler))

;; Free identifier as literal - works fine
(let-syntax ((m (syntax-rules (foo)
                  ((m foo) 'matched))))
  (m foo))  ; => 'matched
```

## Priority

**Low** - Not prioritizing a fix because:
1. All 5 previously ignored macro tests now pass
2. The core literal matching logic is correct for common cases
3. This edge case requires an unusual code pattern
4. Fixing it would require significant changes to how we track bindings

## Test Cases

The following tests document the current behavior:

```scheme
;; In tests/hygiene.rs

;; This works (binding AFTER macro):
(let-syntax ((m (syntax-rules (k) ((m k) 'lit) ((m x) 'var))))
  (let ((k 1)) (m k)))  ; => 'var (correct - different binding)

;; This differs from chibi (binding BEFORE macro):
(let ((k 1))
  (let-syntax ((m (syntax-rules (k) ((m k) 'lit) ((m x) 'var))))
    (m k)))  ; Patina: 'var, chibi: 'lit
```

## References

- R7RS Section 4.3.2 - Pattern Language, literal matching semantics
- `crates/patina-macros/src/macro_expander/matcher.rs` - `literals_equal()` function
- `crates/patina-tests/tests/hygiene.rs` - Test cases for literal matching
