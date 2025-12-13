# Patina Known Limitations

This document tracks known limitations and edge cases in Patina's R7RS implementation.

**Last Updated:** 2025-12-13

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

## Status: ✅ FIXED (as of 2025-12-13)

**Added:** 2025-12-04
**Fixed:** 2025-12-13

This edge case has been fixed. Patina now correctly implements `bound-identifier=?` semantics for literal matching.

## The Edge Case (Now Fixed)

When a literal identifier is bound BEFORE the macro is defined, and that same binding is used in the macro invocation, both should match because they refer to the SAME binding:

```scheme
;; Case A: Binding exists BEFORE macro definition
(let ((k 999))                              ; k bound here
  (let-syntax ((n (syntax-rules (k)         ; k used as literal
                    ((n k) 'matched)
                    ((n x) 'no-match))))
    (n k)))                                  ; k from outer let

;; chibi-scheme / Gauche / Patina: 'matched ✅
```

Compare to:

```scheme
;; Case B: Binding created AFTER macro definition
(let-syntax ((n (syntax-rules (k)           ; k used as literal (unbound)
                  ((n k) 'matched)
                  ((n x) 'no-match))))
  (let ((k 999))                            ; k bound AFTER macro
    (n k)))                                  ; different binding

;; chibi-scheme / Gauche / Patina: 'no-match ✅ (all correct)
```

## How It Was Fixed

The fix involved implementing proper `bound-identifier=?` semantics by tracking binding context at macro compile time:

### 1. New `LiteralBinding` Type

Added `LiteralBinding` struct in `patina-core/src/compiled_macro.rs` to capture both the literal name and its binding scopes:

```rust
pub struct LiteralBinding {
    pub name: Rc<str>,
    /// None = unbound at definition time
    /// Some(scopes) = bound with these scopes
    pub binding_scopes: Option<ScopeSet>,
}
```

### 2. Updated `CompiledMacro`

Changed `literals: Vec<Rc<str>>` to `literals: Vec<LiteralBinding>` to store binding information with each literal.

### 3. Compiler Captures Binding Context

The macro compiler now checks `shadowed_names` (which includes lambda parameters) when resolving literals. This allows it to treat lambda parameters as "bound" even though they're not yet in the environment:

```rust
fn resolve_literal_bindings(
    literal_names: &[Rc<str>],
    env: Option<&Rc<Environment>>,
    definition_scopes: &ScopeSet,
    shadowed_names: &HashSet<Rc<str>>,  // NEW: includes lambda params
) -> Vec<LiteralBinding>
```

### 4. Matcher Compares Bindings

The pattern matcher now compares binding contexts instead of just checking if a name is shadowed:

- If literal is bound at definition time AND use-site has the same scopes → MATCH
- If literal is unbound at definition time AND use-site is bound → NO MATCH
- If literal is bound at definition time AND use-site has different scopes → NO MATCH

## Test Cases

The fix is verified by the following tests in `tests/hygiene.rs`:

```rust
#[test]
fn test_literal_bound_before_macro_definition() {
    // Case A: k bound BEFORE macro - should match
    // ...
    assert_eq!(result.unwrap().to_string(), "matched");
}

#[test]
fn test_literal_binding_before_vs_after() {
    // Contrasts Case A (before) vs Case B (after)
    // ...
}
```

## References

- R7RS Section 4.3.2 - Pattern Language, `bound-identifier=?` semantics
- `crates/patina-core/src/compiled_macro.rs` - `LiteralBinding` struct
- `crates/patina-macros/src/macro_expander/compiler/mod.rs` - `resolve_literal_bindings()`
- `crates/patina-macros/src/macro_expander/matcher/literal.rs` - `is_literal_shadowed()`
- `crates/patina-tests/tests/hygiene.rs` - Test cases

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

`case-lambda` is now a **pure macro** via SRFI-16, loaded from a `.sld` file:
- Implementation: `lib/scheme/case-lambda.sld` (R7RS library definition format)
- 21 tests passing in `crates/patina-tests/tests/case_lambda.rs`
- All clause types work: fixed arity, pure variadic, mixed variadic
- **First library using .sld loading path** - exercises SchemeLibraryLoader

**Code Removed:**
- `CoreExpr::CaseLambda` variant removed from IR
- `Procedure::CaseLambda` variant removed from runtime
- `desugar_case_lambda()` removed from desugarer
- Procedure dispatch code removed from `application.rs` and `core_eval.rs`
- `crates/patina-runtime/src/stdlib/scheme_case_lambda.rs` deleted
- Rust library builder registration removed from evaluator

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
- `lib/scheme/case-lambda.sld` - Working .sld library implementation
