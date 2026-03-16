# Multiple Consecutive Ellipses in Templates

**Status**: Ready to implement
**Scope**: True SRFI-149 Cartesian product semantics for `... ...` in templates
**Prerequisite**: None (independent of tail patterns)
**Effort**: ~150-250 lines across 3 files
**Date**: 2026-03-15

## Problem

Patina already supports `... ...` in templates for the "flatten" case — where a depth-2 pattern variable like `step` from `((var init step ...) ...)` appears as `step ... ...` in the template. This flattens the nested branches into a single list.

What Patina does **not** support is the **Cartesian product** case: where pattern variables at **different depths** appear under more ellipses than their pattern depth. For example:

```scheme
(define-syntax cross
  (syntax-rules ()
    ((cross (a ...) (b ...))
     ((list a b) ... ...))))

(cross (1 2) (x y))
;; Should produce: ((list 1 x) (list 1 y) (list 2 x) (list 2 y))
```

Here `a` has pattern depth 1 and `b` has pattern depth 1, but the template uses 2 ellipses. The outer `...` iterates over `a`, the inner `...` iterates over `b`, producing all combinations.

Racket supports this natively. Chez does **not** support this in `syntax-rules` (only in `syntax-case` with explicit `with-syntax` bindings). We follow Racket's approach.

## Goal

Extend Patina's template expansion to support the Cartesian product semantics: when a template has N consecutive ellipses and contains variables at depths less than N, the "excess" ellipses create repetition (broadcasting) rather than requiring exact depth matching.

## Non-Goals

- **Tail patterns** (`x ... y z`). That is a separate feature — see `TAIL_PATTERNS.md`.
- **`syntax-case`**. This works within `syntax-rules` only.
- **Triple+ ellipsis** (`... ... ...`). Support up to double for now.

## Reference: Racket's Algorithm

Racket's approach (from `racket/src/ChezScheme/s/syntax.ss`, `gen-syntax`):

### Compile-time: Template to expansion code

The key function is `gen-syntax` which compiles templates. When it sees `(x dots . y)` where `dots` is `...`:

```scheme
((x dots . y)
 (ellipsis? (syntax dots))
 (let f ((y (syntax y))
         (k (lambda (maps)
              ;; Compile subtemplate at one deeper level
              (let-values ([(x maps)
                            (gen-syntax src (syntax x) r
                              (cons '() maps)  ; push new depth level
                              ellipsis? #f)])
                (if (null? (car maps))
                    (syntax-error src "extra ellipsis in syntax form")
                    (values (gen-map x (car maps))
                            (cdr maps)))))))
   (syntax-case y ()
     ((dots . y)
      (ellipsis? (syntax dots))
      ;; SECOND ELLIPSIS: wrap in another map+append
      (f (syntax y)
         (lambda (maps)
           (let-values ([(x maps) (k (cons '() maps))])
             (if (null? (car maps))
                 (syntax-error src "extra ellipsis in syntax form")
                 (values (gen-mappend x (car maps))
                         (cdr maps)))))))
     (_ ...))))
```

### The `maps` stack

The critical data structure is `maps` — a list of association lists, one per ellipsis depth. Each level tracks which variables drive iteration at that depth.

- Single `...`: pushes one level, uses `gen-map` → `(map (lambda (v) body) source)`
- Double `... ...`: pushes two levels, inner uses `gen-map`, outer uses `gen-mappend` → `(apply append (map (lambda (v) body) source))`

### Variable lookup across depths

When a pattern variable at depth 1 appears under 2 ellipses, Racket's `gen-ref` function handles the mismatch:

```scheme
(define gen-ref
  (lambda (src var level maps)
    (if (fx= level 0)
        (values var maps)
        (if (null? maps)
            (syntax-error src "missing ellipsis in syntax form")
            (let-values ([(outer-var outer-maps)
                          (gen-ref src var (fx- level 1) (cdr maps))])
              ;; outer-var is the variable at the outer level
              ;; Register it as driving iteration at this level
              (let ((b (assq outer-var (car maps))))
                (if b
                    (values (cdr b) maps)
                    (let ((inner-var (gen-var 'tmp)))
                      (values inner-var
                              (cons (cons (cons outer-var inner-var)
                                          (car maps))
                                    outer-maps))))))))))
```

**What this does**: For a depth-1 variable `a` appearing at template depth 2:
1. At depth 2: calls `gen-ref` with `level=2`
2. Recurses to depth 1: finds `a` is the iteration source for the outer `...`
3. At depth 2: `a` is treated as a **constant** (broadcast) across the inner `...` iterations
4. A fresh iteration variable is generated for the inner `...` from a different source

### Cartesian product semantics

For template `((list a b) ... ...)` with `a` at depth 1, `b` at depth 1:

```
;; Generated expansion code (conceptual):
(apply append
  (map (lambda (a-iter)      ;; outer ... iterates over a
    (map (lambda (b-iter)    ;; inner ... iterates over b
      (list a-iter b-iter))
    b-source))
  a-source))
```

The outer `...` iterates `a`, the inner `...` iterates `b`. Since each `a` value is paired with every `b` value, this produces the Cartesian product.

### Broadcast semantics

When only ONE variable drives iteration at a given depth, the other variables at shallower depths are **broadcast** (repeated):

```scheme
(define-syntax repeat
  (syntax-rules ()
    ((repeat x (y ...))
     ((list x y) ...))))

(repeat 42 (a b c))
;; → ((list 42 a) (list 42 b) (list 42 c))
;; x (depth 0) is broadcast across y (depth 1)
```

This is actually already supported by most implementations because `x` at depth 0 is simply looked up as a scalar while iterating.

## Current State in Patina

Patina's template compiler (`compiler/template.rs`) already:
1. Counts consecutive ellipses via `count_consecutive_ellipses()` → stored as `nesting: u8`
2. Compiles the subtemplate at `level + nesting` depth
3. Collects vars via `collect_template_vars_at_level(&subtemplate, level + 1)`

Patina's template expander (`expander/ellipsis.rs`) already:
1. Handles `nesting == 1` (single ellipsis) — iterates and collects
2. Handles `nesting == 2` (double ellipsis) — nested iteration + flatten
3. Uses `get_iteration_count_at_level()` for depth navigation

**The gap**: The current double-ellipsis expansion assumes all vars are at depth `level+2` (the "flatten" case). For the Cartesian product, vars at depth `level+1` under double ellipsis need to be treated as the **outer** iteration source, while vars at depth `level+1` from a *different* subexpression drive the **inner** iteration. The key missing piece is variable-to-depth assignment when vars have mixed depths.

## Test Cases

### Cartesian product

```scheme
(define-syntax cartesian
  (syntax-rules ()
    ((cartesian (a ...) (b ...))
     ((list a b) ... ...))))

(cartesian (1 2 3) (x y))
;; → ((list 1 x) (list 1 y) (list 2 x) (list 2 y) (list 3 x) (list 3 y))
```

### Cross operation

```scheme
(define-syntax cross-sum
  (syntax-rules ()
    ((cross-sum (a ...) (b ...))
     ((+ a b) ... ...))))

(cross-sum (1 2) (10 20 30))
;; → ((+ 1 10) (+ 1 20) (+ 1 30) (+ 2 10) (+ 2 20) (+ 2 30))
```

### Broadcast (depth-0 variable under ellipsis)

```scheme
(define-syntax broadcast
  (syntax-rules ()
    ((broadcast x (y ...))
     ((list x y) ...))))

(broadcast 42 (a b c))
;; → ((list 42 a) (list 42 b) (list 42 c))
```

### Flatten (existing behavior — must not regress)

```scheme
(define-syntax flatten-step
  (syntax-rules ()
    ((flatten-step ((var init step ...) ...))
     (list step ... ...))))

(flatten-step ((i 0 (+ i 1)) (j 10)))
;; → (list (+ i 1))  ;; step from first clause, none from second
```

### Interleave (flatten-pairs)

```scheme
(define-syntax flatten-pairs
  (syntax-rules ()
    ((flatten-pairs ((a b) ...))
     (list a ... b ...))))

(flatten-pairs ((1 2) (3 4) (5 6)))
;; → (list 1 3 5 2 4 6)
```

### Multiple definitions

```scheme
(define-syntax make-setters
  (syntax-rules ()
    ((make-setters (name field ...) ...)
     (begin (define (name val) (set! field val)) ... ...))))
```

### Error: no variable to drive iteration

```scheme
(define-syntax bad
  (syntax-rules ()
    ((bad x) (list x ... ...))))
;; → compile-time error: x is at depth 0, cannot drive double ellipsis
```

## Implementation Plan

### Step 1: Classify variables by depth relative to ellipsis

**File**: `crates/patina-macros/src/macro_expander/compiler/template.rs`

In `compile_ellipsis_template()`, when `nesting >= 2`, partition the collected vars:

```rust
// Current: collect all vars above base_level
let vars = collect_template_vars_at_level(&subtemplate, level + 1);

// New: also track which vars are at which depth
// For nesting=2 with level=0:
//   "outer vars" = vars at depth 1 (drive outer iteration)
//   "inner vars" = vars at depth 2 (drive inner iteration)
//   If ALL vars are at depth 2 → flatten case (existing behavior)
//   If vars at depth 1 exist → Cartesian product case (new)
```

Store this classification in the `Template::Ellipsis` variant. Options:
- Add an `outer_vars: Vec<PVRef>` field to `Template::Ellipsis`
- Or add a `mode: EllipsisMode` enum (`Flatten` vs `CartesianProduct`)

### Step 2: Cartesian product expansion

**File**: `crates/patina-macros/src/macro_expander/expander/ellipsis.rs`

Add a new expansion path in `expand_double_ellipsis()`:

```
Current behavior (flatten):
  outer_count = iteration count at level-1 (from depth-2 vars)
  for each outer:
    inner_count = iteration count at level (from depth-2 vars)
    expand subtemplate inner_count times
  flatten all results

New behavior (Cartesian product):
  outer_count = iteration count from outer_vars (depth-1 vars)
  inner_count = iteration count from inner_vars (depth-1 vars from different source)
  for each outer_idx in 0..outer_count:
    for each inner_idx in 0..inner_count:
      expand subtemplate with both indices set
  flatten all results
```

The key difference: in the Cartesian case, `inner_count` is constant across all outer iterations (it comes from a different source list), while in the flatten case, `inner_count` varies per outer iteration.

### Step 3: Variable broadcast

**File**: `crates/patina-macros/src/macro_expander/expander/mod.rs`

When expanding a `Template::Var(pvref)` and the pvref's level is less than the current ellipsis depth, the variable should resolve to its scalar value (broadcast). This likely already works because `expand_impl` looks up vars via `env.get(pvref, indices)` and shallow vars ignore deeper indices. Verify and add tests.

### Step 4: Improved error messages

**File**: `crates/patina-macros/src/macro_expander/compiler/template.rs`

In `verify_ellipsis_nesting()`, improve the error when nesting exceeds what variables can support. Current check is too permissive — it only requires *any* var above `base_level`. For Cartesian products, we need at least one var per ellipsis depth.

## Files Changed

| File | Change |
|------|--------|
| `crates/patina-core/src/compiled_macro.rs` | Optionally add `outer_vars` or `mode` to `Template::Ellipsis` |
| `crates/patina-macros/src/macro_expander/compiler/template.rs` | Classify vars by depth in `compile_ellipsis_template()` |
| `crates/patina-macros/src/macro_expander/expander/ellipsis.rs` | Add Cartesian product path in `expand_double_ellipsis()` |
| `crates/patina-tests/` | Add Cartesian product and broadcast test cases |

Pattern matching (`matcher/`) requires **no changes** — the patterns are standard; only the template expansion semantics differ.

## Design Decisions

### Why Racket's approach over Chez's

Chez does not support Cartesian products in `syntax-rules`. Racket does. Since Patina already has the `nesting` field and double-ellipsis infrastructure from the `do` macro work, extending it to handle the Cartesian case is natural. We adopt Racket's semantics: excess ellipses create repetition/broadcasting.

### Flatten vs Cartesian detection

The two cases are distinguished by variable depths:
- **Flatten**: all driving vars have depth equal to total ellipsis depth → nested iteration with varying inner counts
- **Cartesian**: driving vars have depth less than total ellipsis depth → nested iteration with constant inner counts from independent sources

This can be detected at compile time and stored in the `Template::Ellipsis` variant, avoiding runtime overhead.

### No new pattern forms needed

Unlike tail patterns, this feature is purely a template expansion change. The pattern `(a ...)` still compiles the same way — it's only the template `(a b) ... ...` that gains new semantics.

## Verification

```bash
# Existing tests still pass (especially do macro which uses ... ...)
cargo build --release && ./scripts/run_chibi_tests.sh

# New Cartesian product tests
cargo test --package patina-tests -k cartesian

# Lint
cargo clippy --all-targets --all-features -- -D warnings
```

## References

- Racket `syntax.ss` `gen-syntax` function (lines 6074-6250): compile-time template expansion with maps stack
- Racket `syntax.ss` `gen-ref` function: variable-to-depth mapping for excess ellipses
- Racket `syntax/template.rkt` `make-template-map`: `ellipses` struct with `count` field
- Existing Patina catalog: `PRD/future/SRFI_149_ADVANCED_MACROS.md`
- Patina's current double-ellipsis: `expander/ellipsis.rs` `expand_double_ellipsis()`
