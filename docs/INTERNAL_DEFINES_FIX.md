# Internal Defines Fix — Detailed Write-up

**Date:** 2026-03-14
**Result:** VM chibi tests 1161/1163 → 1162/1163 (fixed 4.3 define scoping test)
**Files changed:** 6 compiler passes, 0 runtime/instruction changes

---

## 1. The Problem

R7RS Section 5.3.2 specifies that internal defines (defines inside a lambda body) behave like `letrec*` — they create local bindings scoped to the lambda body, not global bindings.

The VM's `Instruction::Define` unconditionally stored to `state.globals`. This meant:

```scheme
(let ((x 1))
  (let-syntax ()        ;; desugars to ((lambda () ...))
    (define x 2)        ;; VM: stores x=2 to GLOBALS
    #f)
  x)                    ;; expects 1, gets 2
```

The `(define x 2)` inside the lambda body clobbered the outer `x` because both wrote to the same global namespace.

## 2. The Approach

Convert internal defines to local variable bindings entirely at compile time. No new instructions or runtime changes needed — the existing `SetLocal`/`WriteLocalCell` and register machinery handles everything.

The key insight: at compile time, scan each lambda body for `Define` nodes. Treat those names as additional lambda-local bindings (like params). Allocate registers for them, initialize to `#<unspecified>`, and compile the `Define` as a local assignment. Only true top-level defines (not inside any lambda) continue emitting `Instruction::Define`.

## 3. The Subtlety: MutableCell Boxing

The initial implementation worked for simple cases but caused a massive regression (1161 → 1119 passing tests). The failure mode: "expected a procedure, got unspecified."

**Reproducer:**
```scheme
(define (f x y)
  (define (g a b)            ;; internal define: g is a procedure
    (if (vector? a)
        (g (vector-ref a 0)  ;; recursive self-call
           (vector-ref b 0))
        (equal? a b)))
  (g x y))

(f #(a) #(a))  ;; ERROR: expected a procedure, got unspecified
```

**What went wrong:** `g` is an internal define whose value is a lambda. That lambda references `g` itself (for recursion), making `g` a free variable of its own value expression. The compiler creates a closure for `g`'s value, and that closure needs to capture `g`.

Without boxing, the capture snapshots `g`'s *current value* at closure creation time. But at that point, `g` is still `#<unspecified>` — the define hasn't executed yet. When the closure later tries to call `g` recursively, it finds `#<unspecified>` instead of a procedure.

**The fix:** Mark all internal define names as "mutated" in Pass 1. This triggers the existing MutableCell boxing machinery:

1. The register for `g` gets wrapped in a `MutableCell` (heap-allocated box).
2. The closure captures a pointer to the cell, not the value.
3. When the define executes, it writes the procedure into the cell via `WriteCell`.
4. When the closure reads `g`, it reads through the cell pointer via `ReadCell`, getting the current value.

This is exactly `letrec*` semantics: all bindings exist (as cells) before any values are computed, and each value is stored into its cell in order.

**Why "mutated" is the right signal:** The existing boxing logic boxes a variable when it is both (a) mutated and (b) captured by a nested lambda. Internal defines are semantically mutated — they go from `#<unspecified>` to their defined value — even though there's no explicit `set!`. Marking them as mutated makes the existing machinery do the right thing without any new boxing logic.

## 4. Pass-by-Pass Changes

### Pre-pass: `alpha_rename.rs`

Internal define names must participate in alpha-renaming so that the rest of the pipeline sees unique names. Before renaming the body, scan for `Define` nodes and add their names to the lambda's rename scope frame:

```
Lambda body scan:
  (define x 28)  →  binding { name: "x", unique: "x__#7", is_simple: true }

Define arm:
  (define x 28)  →  (define x__#7 28)   // name resolved through env
```

This ensures that if a macro introduces a `define` with the same name as an outer binding, the two get distinct renamed names (hygiene preserved).

### Pass 1: `pass1_analysis.rs`

Three additions in the Lambda arm:

1. **Scan body for define names** via `collect_internal_define_names()` — looks at direct children and one level of `Begin` wrapping.

2. **Add to `bound` set** — so references to internal-define names from nested lambdas are reported as free vars of the nested lambda (correct), not as free vars of the current lambda (which would be wrong — the name is bound here).

3. **Mark as mutated** — add to both `info.all_mutated` (global set) and `entry.mutated_bindings` (per-lambda set). This is what triggers MutableCell boxing downstream.

Also stores `internal_defines: Vec<Symbol>` in `LambdaInfo` for later passes to consume.

### Pass 2: `pass2_closure.rs`

Two changes:

**Lambda arm:** After building param `VarLoc` entries, add internal-define names as additional local entries (`VarLoc::Local` or `VarLoc::LocalBoxed` depending on whether they're in `boxed_params`). The boxed_params computation now includes internal defines alongside params.

**Define arm:** Instead of always emitting `ClosedExpr::Define`, check `ctx.lookup(name)`:
- `VarLoc::Local` → emit `ClosedExpr::SetLocal` (plain register assignment)
- `VarLoc::LocalBoxed` → emit `ClosedExpr::WriteLocalCell` (write through MutableCell)
- `VarLoc::Global` → emit `ClosedExpr::Define` (true top-level define, unchanged)

No new `ClosedExpr` variants needed.

### Pass 3: `pass3_tail.rs`

Propagates `internal_defines: Vec<Symbol>` from `ClosedLambda` to `TailedLambda`. One field addition, one line in the Lambda arm.

### Pass 4: `pass4_registers.rs`

In `allocate_lambda()`, after binding param registers (r0..r(n-1)), allocate additional registers for internal defines:

```
params:           r0, r1, ..., r(p-1)
internal defines: r(p), r(p+1), ..., r(p+d-1)
temporaries:      r(p+d), ...
```

Stores `internal_define_regs: Vec<u16>` in `RegLambda` so Pass 5 knows which registers need UNSPECIFIED initialization.

### Pass 5: `pass5_codegen.rs`

In the lambda prologue (before body instructions), emit initialization:

- **Boxed internal defines:** `LoadImmediate UNSPECIFIED` then `AllocCell` (the existing boxed_params loop handles AllocCell, but we need to pre-initialize the register since unlike params, no caller-supplied value exists).
- **Non-boxed internal defines:** `LoadImmediate UNSPECIFIED` only.

The body instructions then contain `SetLocal` or `WriteLocalCell` for each internal define, which overwrites the UNSPECIFIED value with the actual defined value.

## 5. Execution Example

```scheme
(define (f x)
  (define (g y) (+ x y))   ;; internal define, g captures x and itself
  (g 10))
```

**Compiled code for f's lambda (simplified):**
```
; prologue
r0 = <param x>                     ; from caller
r1 = UNSPECIFIED                    ; internal define 'g' init
AllocCell r1 ← r1                  ; box g (it's mutated + captured)

; body: (define (g y) (+ x y))
r2 = MakeClosure(g_code, [r1, r0]) ; captures g (boxed cell) and x
WriteCell r1 ← r2                  ; g = <closure>

; body: (g 10)
r3 = ReadCell r1                   ; load g from cell
r4 = 10
TailCall r3, [r4]
```

The key: `MakeClosure` captures a pointer to `r1`'s cell. When `g` later calls itself, it reads through its closure's cell pointer and finds the procedure — not `#<unspecified>`.

## 6. Edge Cases Handled

| Case | How handled |
|------|-------------|
| Forward references between internal defines | All names in scope before any values computed; cells initialized to UNSPECIFIED |
| Recursive self-reference | Name marked mutated → boxed → closure captures cell pointer |
| Mutual recursion between internal defines | Same: both boxed, both capture each other's cells |
| `set!` on an internal define | Already marked mutated; normal SetLocal/WriteLocalCell path |
| Internal define not captured | Not boxed (no nested lambda uses it); plain register + SetLocal |
| Top-level defines | `ctx.lookup()` returns `Global` → unchanged `Instruction::Define` path |
| `Begin`-wrapped defines | `collect_internal_define_names()` scans one level of Begin |
| `(define (f x) ...)` syntax | Already desugared by frontend to `(define f (lambda ...))` |
| Library body defines | Compiled as standalone top-level expressions, not inside lambdas |

## 7. What Didn't Work (First Attempt)

The first implementation did everything above except marking internal defines as mutated. This produced correct results for simple cases:

```scheme
(define (f x) (define y (+ x 1)) y)  ;; WORKS: y not captured, plain register
```

But failed catastrophically for any internal define captured by a nested lambda:

```scheme
(define (f x)
  (define (g y) (+ x y))  ;; BROKEN: g captures itself, gets #<unspecified>
  (g 10))
```

The regression was severe (1161 → 1119 passing tests) because many Scheme libraries use recursive internal defines — the `test-equal?` function in the chibi test framework itself had recursive internal defines, so nearly every test errored out.
