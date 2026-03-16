# Tail Patterns for syntax-rules

**Status**: Ready to implement
**Scope**: Add support for elements after `...` in `syntax-rules` patterns
**Effort**: ~200-300 lines across 4 files
**Date**: 2026-03-15

## Problem

R7RS-small `syntax-rules` only allows `...` at the end of a list (before an optional dotted tail). Patterns like `(x ... y z)` — where fixed elements appear *after* the ellipsis — are rejected. This limitation prevents common idioms:

```scheme
;; "rotate last to front" — not possible today
(define-syntax rotate
  (syntax-rules ()
    ((_ x ... y) (list y x ...))))

(rotate 1 2 3 4 5) → (5 1 2 3 4)
```

Chez Scheme supports these patterns natively. The algorithm is small and well-understood.

## Goal

Port Chez Scheme's `each+` tail-pattern matching to Patina's Rust macro expander. This adds one new pattern form and one new matching function. Template expansion requires no changes.

## Non-Goals

- **SRFI-149 Cartesian products** (`... ...` in templates). That is a separate, more complex feature. See `SRFI_149_ADVANCED_MACROS.md`.
- **`syntax-case`**. Tail patterns will work with `syntax-rules` only (Patina doesn't have `syntax-case` yet).
- **Multiple ellipses at the same list level** (`x ... y ...`). Chez explicitly rejects this — so do we.

## Reference: Chez Scheme's Algorithm

Chez implements tail patterns with three components:

### 1. Pattern compilation (`convert-pattern`)

When the compiler sees `(x dots y ... . z)` where `dots` is `...`:

```scheme
[(x dots y ... . z)
 (ellipsis? #'dots)
 (let-values ([(z ids) (cvt #'z n ids)])
   (let-values ([(y ids) (cvt* #'(y ...) n ids)])
     (let-values ([(x ids) (cvt #'x (fx+ n 1) ids)])
       (values `#(each+ ,x ,(reverse y) ,z) ids))))]
```

Produces `#(each+ x-pat (y-pats-reversed) z-pat)`:
- `x` compiled at depth `n+1` (it repeats)
- `y` patterns compiled at depth `n` (fixed elements), then **reversed**
- `z` compiled at depth `n` (the dotted tail)

### 2. Runtime matching (`match-each+`)

```scheme
(define match-each+
  (lambda (e x-pat y-pat z-pat w r)
    (let f ([e e] [w w])
      (cond
        [(pair? e)
         (let-values ([(xr* y-pat r) (f (cdr e) w)])
           (if r
               (if (null? y-pat)
                   (let ([xr (match (car e) x-pat w '())])
                     (if xr
                         (values (cons xr xr*) y-pat r)
                         (values #f #f #f)))
                   (values '() (cdr y-pat) (match (car e) (car y-pat) w r)))
               (values #f #f #f)))]
        [else (values '() y-pat (match e z-pat w r))]))))
```

The algorithm:
1. Recurse to the end of the input list
2. At the tail, match against `z-pat`
3. Walking back, consume elements for `y-pat` (reversed, so right-to-left)
4. Once all `y-pat` consumed, collect remaining elements into `x-pat` matches

### 3. Dispatch (`match*`)

```scheme
((each+)
 (let-values ([(xr* y-pat r)
               (match-each+ e (vector-ref p 1) (vector-ref p 2)
                 (vector-ref p 3) w r)])
   (and r (null? y-pat)
     (if (null? xr*)
         (match-empty (vector-ref p 1) r)
         (combine xr* r)))))
```

After matching, verify `y-pat` is fully consumed. If `x` matched zero elements, use `match-empty` to bind variables to empty lists.

## Test Cases (from Chez)

### Basic: `(_ x ... y)` — last element is fixed

```scheme
(define-syntax gp$a
  (syntax-rules ()
    [(_ x ... y) (list y x ...)]))

(gp$a 1 2 3 4 5)     → (5 1 2 3 4)
(gp$a 1)             → (1)          ; x...=(), y=1
(gp$a 1 2)           → (2 1)
(gp$a)               → ERROR        ; y is required
```

### Multiple tail elements: `(_ x ... y z . w)`

```scheme
(define-syntax gp$c
  (syntax-rules ()
    [(_ x ... y z . w) '((x ...) y z w)]))

(gp$c 1 2)           → (() 1 2 ())
(gp$c 1 2 3 4 5)     → ((1 2 3) 4 5 ())
(gp$c 1 2 . 3)       → (() 1 2 3)
(gp$c 1 2 3 4 5 . 6) → ((1 2 3) 4 5 6)
(gp$c)               → ERROR
(gp$c 1)             → ERROR
```

### Structured tail: `(_ x ... (y z) . #(foo w1 w2))`

```scheme
(define-syntax gp$d
  (syntax-rules (foo)
    [(_ x ... (y z) . #(foo w1 w2)) '((x ...) y z w1 w2)]))

(gp$d (4 5) . #(foo 6 7))       → (() 4 5 6 7)
(gp$d 1 (4 5) . #(foo 6 7))     → ((1) 4 5 6 7)
(gp$d 1 2 3 (4 5) . #(foo 6 7)) → ((1 2 3) 4 5 6 7)
```

### Tail with literal keyword: `(_ x ... . rats)`

```scheme
(define-syntax gp$e
  (syntax-rules (rats)
    [(_ x ... . rats) '(x ...)]))

(gp$e . rats)             → ()
(gp$e 1 . rats)           → (1)
(gp$e 1 2 3 4 5 . rats)   → (1 2 3 4 5)
(gp$e)                    → ERROR  ; missing literal "rats"
```

### Nested: `(_ (x ... y) ...)`

```scheme
(define-syntax gp$f
  (syntax-rules ()
    [(_ (x ... y) ...) '(x ... ... y ...)]))

(gp$f)                          → ()
(gp$f (1 2 3 4 5) (6 7 8))     → (1 2 3 4 6 7 5 8)
```

### Error: multiple ellipses at same level

```scheme
;; This must be a compile-time error
(define-syntax bad
  (syntax-rules ()
    [(_ x ... y ...) '(x ... y ...)]))
→ ERROR
```

## Implementation Plan

### Step 1: Extend `Pattern` enum

**File**: `crates/patina-core/src/compiled_macro.rs`

Add a new variant to distinguish tail patterns from regular ellipsis:

```rust
enum Pattern {
    // ... existing variants ...

    /// Ellipsis with fixed tail elements: (sub ... tail1 tail2 . rest)
    /// Chez's `#(each+ x-pat (y-pats-reversed) z-pat)`
    EllipsisTail {
        subpattern: Box<Pattern>,       // x-pat (the repeating part)
        level: u8,                      // ellipsis nesting depth
        tail_patterns: Vec<Pattern>,    // y-pats, stored REVERSED (right-to-left)
        rest: Box<Pattern>,             // z-pat (dotted tail, or implicit null)
        vars: Vec<PVRef>,               // all PVRefs in subpattern
        tail_vars: Vec<PVRef>,          // all PVRefs in tail_patterns + rest
    },
}
```

### Step 2: Update pattern compilation

**File**: `crates/patina-macros/src/macro_expander/compiler/pattern.rs`

In `compile_rule_list_pattern()` and `compile_pattern_items()`: when an ellipsis is found and there are elements after it, emit `EllipsisTail` instead of `Ellipsis`. The current `num_following` calculation already identifies this case — it just needs to capture the tail patterns explicitly rather than leaving them as separate list items.

Key logic:
```
if ellipsis found at position i:
    items_after = items[i+2..]  // elements after the "..."
    if items_after is empty:
        emit Ellipsis { ... }              // existing behavior
    else:
        emit EllipsisTail {
            subpattern: compile(items[i]),
            tail_patterns: compile(items_after).reverse(),
            rest: compile(dotted_tail_or_null),
            ...
        }
```

### Step 3: Add `match_each_plus` matching

**File**: `crates/patina-macros/src/macro_expander/matcher/list_match.rs`

Port Chez's `match-each+` to Rust. The Rust version operates on `&[TaggedValue]` (already converted from the Scheme list) rather than recursing through pairs:

```
fn match_each_plus(
    input: &[TaggedValue],
    x_pat: &Pattern,
    y_pats: &[Pattern],     // reversed tail patterns
    z_pat: &Pattern,        // rest/dotted tail
    tail: TaggedValue,      // actual dotted tail value
    ...
) -> Result<bool, MacroError>
```

Algorithm (iterative, since we have random-access slices):
1. Match `tail` against `z_pat`
2. Match last `y_pats.len()` elements of `input` against `y_pats` (in reverse)
3. Match remaining leading elements against `x_pat` repeatedly
4. If `x` matched zero elements, call `match_empty` equivalent

### Step 4: Wire into matcher dispatch

**File**: `crates/patina-macros/src/macro_expander/matcher/list_match.rs`

Add `EllipsisTail` case to `match_list_tagged()` and `match_dotted_list_tagged()`. The dispatch mirrors Chez's `match*` for `each+`.

### Step 5: Validate at compile time

**File**: `crates/patina-macros/src/macro_expander/compiler/pattern.rs`

Reject patterns with **two** ellipses at the same list level:
```scheme
(_ x ... y ...)  → compile-time error
```

This is already implicitly prevented by the current code (only one ellipsis scan per list), but add an explicit error message.

## Files Changed

| File | Change |
|------|--------|
| `crates/patina-core/src/compiled_macro.rs` | Add `EllipsisTail` variant to `Pattern` |
| `crates/patina-macros/src/macro_expander/compiler/pattern.rs` | Emit `EllipsisTail` when items follow `...` |
| `crates/patina-macros/src/macro_expander/matcher/list_match.rs` | Add `match_each_plus()` and dispatch |
| `crates/patina-tests/` | Add tail pattern test cases |

Template expansion (`expander/`) requires **no changes** — `EllipsisTail` populates the same `MatchEnv` tree as `Ellipsis`, so the existing expansion logic works as-is.

## Verification

```bash
# Existing tests still pass
cargo build --release && ./scripts/run_chibi_tests.sh

# New tail-pattern tests
cargo test --package patina-tests -k tail_pattern

# Lint
cargo clippy --all-targets --all-features -- -D warnings
```

## References

- Chez Scheme `syntax.ss` lines 6447–6490 (pattern conversion), 7436–7453 (`match-each+`), 7518–7525 (dispatch)
- Chez test suite `mats/8.ms` lines 504–580 (generalized-pattern tests)
- Existing Patina SRFI-149 catalog: `PRD/future/SRFI_149_ADVANCED_MACROS.md`
