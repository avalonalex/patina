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

---

# 3. Complex Ellipsis Patterns in Variadic Formals

## Status: ✅ RESOLVED (as of 2025-12-06)

**Added:** 2025-12-06
**Resolved:** 2025-12-06

The official SRFI-16 reference implementation works in Patina! The key insight is that SRFI-16 uses a clever approach with literal markers (`"CLAUSE"` and `"IMPROPER"`) that avoids the problematic ellipsis patterns.

## What Works

The SRFI-16 reference implementation (https://srfi.schemers.org/srfi-16/srfi-16.html) is now implemented in `lib/scheme/case-lambda-extras.scm`:

```scheme
(define-syntax case-lambda
  (syntax-rules ()
    ;; Entry: dispatch to "CLAUSE" processing
    ((case-lambda (?a1 ?e1 ...) ?clause1 ...)
     (lambda args
       (let ((l (length args)))
         (case-lambda "CLAUSE" args l (?a1 ?e1 ...) ?clause1 ...))))

    ;; Fixed arity: ((?a1 ...) ?e1 ...)
    ((case-lambda "CLAUSE" ?args ?l ((?a1 ...) ?e1 ...) ?clause1 ...)
     (if (= ?l (length '(?a1 ...)))
         (apply (lambda (?a1 ...) ?e1 ...) ?args)
         (case-lambda "CLAUSE" ?args ?l ?clause1 ...)))

    ;; Variadic: ((?a1 . ?ar) ?e1 ...) - transition to "IMPROPER"
    ((case-lambda "CLAUSE" ?args ?l ((?a1 . ?ar) ?e1 ...) ?clause1 ...)
     (case-lambda "IMPROPER" ?args ?l 1 (?a1 . ?ar) (?ar ?e1 ...) ?clause1 ...))

    ;; ... more clauses for IMPROPER handling
    ))
```

The SRFI-16 approach:
1. Uses literal string markers (`"CLAUSE"`, `"IMPROPER"`) instead of helper macros
2. Recursively processes dotted pair patterns by peeling off one element at a time
3. Counts fixed parameters with an accumulator `?k`
4. Preserves the original formals pattern `?al` for use in the final `lambda`

## What Still Doesn't Work

The R7RS spec's more direct pattern and chibi's `%case` helper approach still fail:

```scheme
;; R7RS pattern - fails with "x2 at level 1 used at level 0"
((case-lambda "clause" args n ((x1 x2 ... . r) body0 ...) clause ...)
 ...)

;; Chibi %case accumulator - fails with "p at level 1 used at level 0"
((%case args len "variadic" n (p ...) (r . body) . rest)
 (apply (lambda (p ... . r) . body) args))
```

These patterns require ellipsis level tracking that allows accumulator patterns `(p ... x)`.

## Current Implementation

`case-lambda` is now a **pure macro** via SRFI-16:
- Implementation: `lib/scheme/case-lambda-extras.scm`
- 21 tests passing in `crates/patina-tests/tests/case_lambda.rs`
- All clause types work: fixed arity, pure variadic, mixed variadic

**Code Removed:**
- `CoreExpr::CaseLambda` variant removed from IR
- `Procedure::CaseLambda` variant removed from runtime
- `desugar_case_lambda()` removed from desugarer
- Procedure dispatch code removed from `application.rs` and `core_eval.rs`

The special form infrastructure has been completely removed. `case-lambda` is now purely implemented as a macro that expands to a regular lambda with internal argument count dispatch.

## Tests

All 21 case-lambda tests now pass:
- Basic fixed arity clauses
- Variadic (rest) parameters
- Mixed fixed + variadic: `((x . rest) ...)`
- Multiple clauses with dispatch
- Closures capturing environment
- Tail call optimization

## References

- SRFI-16: https://srfi.schemers.org/srfi-16/srfi-16.html
- R7RS Section 4.2.9 - case-lambda specification
- `lib/scheme/case-lambda-extras.scm` - Working implementation
