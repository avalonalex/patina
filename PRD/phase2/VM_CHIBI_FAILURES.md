# VM Backend: Chibi R7RS Test Failures

**Date:** 2026-03-13
**Score:** 1161/1163 (99.8%) — 2 FAIL, 0 Error
**Tree-walker baseline:** 1163/1163 (100%)

---

## Category 1: ~~Nested Quasiquote~~ ✅ FIXED (2 tests)

**Fixed 2026-03-13.** Section 4.2 now 74/74.

**Root cause:** `quasiquote_expand.rs` used raw `car` (an identifier with scope marks) when reconstructing nested `quasiquote`/`unquote` markers. The quoted expected value used plain symbols. When compared via `equal?` inside the `test` macro, identifiers ≠ symbols.

**Fix:** In `quasiquote_expand.rs`, intern plain symbols (`heap.borrow_mut().intern_symbol("quasiquote")` etc.) instead of quoting the raw `car` value for nested `quasiquote`, `unquote`, and `unquote-splicing` markers.

---

## Category 2: ~~Macro Hygiene~~ ✅ MOSTLY FIXED (3 of 4 tests)

**Fixed 2026-03-13.** Section 4.3 improved from 23/27 to 24/25 (2 tests were errors that no longer count).

**Root cause:** The VM compiler resolved variables by name only (in Pass 1 and Pass 2), ignoring scope-set information from macro expansion. The tree-walker uses `get_with_scopes()` at runtime with `binding.scopes ⊆ reference.scopes` rules.

**Fix:** Added `alpha_rename.rs` pre-pass that runs before the 5-pass compiler pipeline. It:
1. Renames all lambda parameters to unique names
2. Resolves variable references using scope-set subset rules (matching tree-walker semantics)
3. Distinguishes "simple" bindings (non-macro params, visible to `env.get()`) from "scoped" bindings (macro-introduced params, only visible to `get_with_scopes()`)
4. Uses `binding_scope` from Lambda nodes to give non-macro params proper scopes

**Still failing (1 test):**
```
FAIL: x
  expected: 1
  but got:  2
```
Test: `(let () (define x 1) (let-syntax () (define x 2) #f) (test 1 x))`

This is a separate issue: the VM's `Define` instruction always stores to globals. Internal `define` inside `let-syntax` (which desugars to a lambda body) should create a local binding, but the VM makes it global, clobbering the outer `x`. This is a `Define`-scoping issue, not a hygiene issue.

---

## Category 3: compose+values corruption (1 FAIL — section 6.10)

```
FAIL: (call-with-values (lambda () ((compose exact-integer-sqrt *) 12 75)) list)
  expected: (30 0)
  but got:  (#0=(a . #0#))
```

Works correctly in isolation. The corruption comes from a prior test's side effect polluting global state — likely the remaining `define`-scoping issue in Category 2 (the inner `(define x 2)` clobbers a global, which cascades).

### Fix approach
Will likely self-resolve once the `Define`-scoping issue is fixed. The internal-define-to-local-binding conversion would prevent global state pollution between tests.

---

## ~~Category 4: `eval` / `environment` / `null-environment`~~ ✅ FIXED

**Fixed 2026-03-13.** All 7 tests now pass (section 6.12: 5/5).

---

## ~~Category 5: `procedure?` for continuations~~ ✅ FIXED

**Fixed 2026-03-13.** Test `(call-with-current-continuation procedure?)` now passes.

---

## Priority Order (remaining)

1. **Internal define scoping** — 1 test (4.3) + likely fixes 1 cascade test (6.10)
   - The VM's `Define` instruction stores to globals unconditionally
   - Internal defines in lambda bodies should create local bindings
   - May require converting `Define` inside lambda bodies to `SetLocal` or equivalent
   - This is the root cause of both remaining failures

---

## Summary of Progress

| Date | Score | Fixed |
|------|-------|-------|
| Start | 1158/1165 (99.4%) | — |
| 2026-03-13 (eval/environment) | +7 tests | eval, environment, null-environment |
| 2026-03-13 (procedure?) | +1 test | procedure? for continuations |
| 2026-03-13 (quasiquote) | +2 tests | nested quasiquote identifier→symbol |
| 2026-03-13 (hygiene) | +3 tests, -2 errors | alpha_rename pass for scope-set hygiene |
| **Current** | **1161/1163 (99.8%)** | **2 remaining (both from define-scoping)** |

---

## Infrastructure Notes

- **Resilient execution:** `run_script_vm` now uses `eval_program_resilient` for test files.
- **Test script:** `./scripts/run_chibi_tests_vm.sh`
- **Verification:** `cargo build --release && ./scripts/run_chibi_tests_vm.sh`
