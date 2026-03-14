# VM Backend: Chibi R7RS Test Failures

**Date:** 2026-03-13
**Score:** 1152/1165 (98.9%) — 6 FAIL, 7 Error (some errors produce multiple cascading errors in output)
**Tree-walker baseline:** 1159/1159 (100%)

---

## Category 1: Nested Quasiquote (2 FAIL — section 4.2)

Nested quasiquote with unquoting produces wrong output. The VM's `quasiquote_expand.rs` doesn't handle nested levels correctly.

### Failures

```
FAIL: `(a `(b ,(+ 1 2) ,(foo ,(+ 1 3) d) e) f)
  expected: (a `(b ,(+ 1 2) ,(foo 4 d) e) f)
  but got:  (a `(b ,(+ 1 2) ,(foo 4 d) e) f)
```
Note: expected and got look identical in text — the mismatch is likely in the quasiquote structure representation (symbols vs syntax objects).

```
FAIL: `(a `(b ,,name1 ,',name2 d) e)
  expected: (a `(b ,x ,'y d) e)
  but got:  (a `(b ,x ,'y d) e)
```
Same — visual match but structural mismatch (symbol identity / hygiene marks).

### Root cause
`quasiquote_expand.rs` strips identifiers to plain symbols during expansion. At nested quasiquote levels, the structure must preserve the quasiquote/unquote markers with correct nesting. The tree-walker handles this via the `Quasiquote` CoreExpr variant which the evaluator processes at runtime, while the VM expands at compile time.

### Fix approach
Compare the VM's compile-time qq expansion output with the tree-walker's runtime qq handling for nested cases. The issue is likely in how `quasiquote_expand` handles depth > 1.

---

## Category 2: Macro Hygiene (2 FAIL + 2 Error — section 4.3)

### Failures

```
FAIL: (let ((x 'outer))
        (let-syntax ((m (syntax-rules () ((m) x))))
          (let ((x 'inner)) (m))))
  expected: outer
  but got:  inner
```
Classic hygiene test — `(m)` should refer to `x` from the definition site (outer), not use site (inner). The VM resolves `x` to the wrong binding.

```
FAIL: x  [in letrec-syntax my-or test]
  expected: 1
  but got:  2
```
A `letrec-syntax` test where `my-or` macro introduces `let`, `if`, `temp` bindings that should be hygienic (not captured by user bindings of the same name). The VM leaks bindings.

### Errors (from same section)
```
Error: Type error: remainder: expected integer, got non-numeric
Error: Type error: =: expected number, got non-numeric
```
These cascade from the `letrec-syntax` hygiene failure — the macro expands incorrectly, binding `odd?`/`even?` to non-procedure values, then arithmetic ops are called on them.

### Root cause
The VM compiler resolves variable references at compile time. When a macro introduces a binding, the scope-set hygiene system should ensure the introduced name doesn't collide with user-site names. The tree-walker resolves at runtime using environments that carry hygiene information. The VM's static resolution loses this context.

### Fix approach
This is a fundamental issue with how the VM compiler resolves variable bindings in macro-expanded code. May require:
1. Carrying scope-set information through to the compiler's variable resolution pass
2. Or deferring name resolution for macro-introduced bindings to runtime (via globals)

This is the hardest category to fix — may need architectural changes to the compiler.

---

## Category 3: Continuations and `procedure?` (2 FAIL — section 6.10)

### Failures

```
FAIL: (call-with-current-continuation procedure?)
  expected: #t
  but got:  #f
```
R7RS requires continuations to satisfy `procedure?`. VM continuations are stored in side tables on `VmState` (as `VmContinuationRef(u64)` opaque handles), not as heap objects that `Heap::is_procedure()` can recognize.

```
FAIL: (call-with-values (lambda () ((compose exact-integer-sqrt *) 12 75)) list)
  expected: (30 0)
  but got:  (#0=(a . #0#))
```
The compose+call-with-values test returns a circular list instead of `(30 0)`. When tested in isolation, this works correctly. The corruption likely comes from a prior test's side effect polluting state (possibly the `letrec-syntax`/`my-or` error above that corrupts bindings).

### Fix approach

**`procedure?` for continuations:**
Option A: Add `VmContinuation` / `VmDelimitedContinuation` variants to `HeapObjectData` and store them on the heap (big refactor, removes side tables).
Option B: Add a VM-specific `procedure?` primitive override that checks both `Heap::is_procedure()` and the VmState side tables. Requires the primitive to have access to VmState.
Option C: Store a thin wrapper closure on the heap that captures the continuation ref. When called, it dispatches to `try_invoke_continuation`.

**compose corruption:**
Investigate whether the `letrec-syntax` error 2 tests earlier corrupts shared state (e.g. a global binding or heap object). May resolve itself once macro hygiene is fixed.

---

## Category 4: `eval` / `environment` / `null-environment` (5 Error + 2 FAIL — section 6.12)

### Errors
```
Error: Cannot load scheme base: load_scheme_library not yet supported in VM  (x5)
Error: eval: expected environment, got null  (x5+)
Error: expected a procedure, got null
```

### Failures
```
FAIL: (inexact (eval '(sin 0) (environment '(scheme inexact))))
  expected: 0.0
  but got:  ()

FAIL: (let ((f (eval '(lambda (f x) (f x x)) (null-environment 5)))) (f + 10))
  expected: 20
  but got:  ()
```

### Root cause
The VM's `eval` primitive works but `environment` and `null-environment` call `load_scheme_library` which is not implemented for the VM backend. The functions return null/error instead of a usable environment object.

### Fix approach
Implement `load_scheme_library` for VmBackend, or make `environment`/`null-environment` use the existing library registry to construct environment objects without re-loading. The tree-walker already does this — the VM needs equivalent plumbing.

---

## Priority Order

1. **`eval`/`environment`** (Category 4) — 7 tests, likely straightforward plumbing
2. **`procedure?` for continuations** (Category 3, first failure) — 1 test, several approaches available
3. **Nested quasiquote** (Category 1) — 2 tests, localized to `quasiquote_expand.rs`
4. **Macro hygiene** (Category 2) — 4 tests, hardest fix, may need compiler architecture changes
5. **compose corruption** (Category 3, second failure) — 1 test, may self-resolve with other fixes

---

## Infrastructure Notes

- **Resilient execution:** `run_script_vm` now uses `eval_program_resilient` for test files (matching tree-walker behavior), so errors no longer abort the entire suite.
- **Test script:** `./scripts/run_chibi_tests_vm.sh` runs the chibi suite against the VM backend, generating `scheme_tests/reports/results_vm.txt` and `compatibility_vm.md`.
- **Verification:** `cargo build --release && ./scripts/run_chibi_tests_vm.sh`
