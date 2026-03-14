# VM Backend: Chibi R7RS Test Failures

**Date:** 2026-03-14
**Score:** 1163/1163 (100%) — 0 FAIL, 0 Error ✅
**Tree-walker baseline:** 1163/1163 (100%)

---

## Category 1: ~~Nested Quasiquote~~ ✅ FIXED (2 tests)

**Fixed 2026-03-13.** Section 4.2 now 74/74.

**Root cause:** `quasiquote_expand.rs` used raw `car` (an identifier with scope marks) when reconstructing nested `quasiquote`/`unquote` markers. The quoted expected value used plain symbols. When compared via `equal?` inside the `test` macro, identifiers ≠ symbols.

**Fix:** In `quasiquote_expand.rs`, intern plain symbols (`heap.borrow_mut().intern_symbol("quasiquote")` etc.) instead of quoting the raw `car` value for nested `quasiquote`, `unquote`, and `unquote-splicing` markers.

---

## Category 2: ~~Macro Hygiene~~ ✅ FIXED (4 of 4 tests)

**Fixed 2026-03-13/14.** Section 4.3 now 25/25.

**Root cause:** The VM compiler resolved variables by name only (in Pass 1 and Pass 2), ignoring scope-set information from macro expansion. The tree-walker uses `get_with_scopes()` at runtime with `binding.scopes ⊆ reference.scopes` rules.

**Fix:** Added `alpha_rename.rs` pre-pass that runs before the 5-pass compiler pipeline. It:
1. Renames all lambda parameters to unique names
2. Resolves variable references using scope-set subset rules (matching tree-walker semantics)
3. Distinguishes "simple" bindings (non-macro params, visible to `env.get()`) from "scoped" bindings (macro-introduced params, only visible to `get_with_scopes()`)
4. Uses `binding_scope` from Lambda nodes to give non-macro params proper scopes

Internal define scoping (the `(let () (define x 1) (let-syntax () (define x 2) #f) (test 1 x))` test) was fixed separately via compile-time conversion of internal defines to local bindings.

---

## Category 3: ~~compose+values corruption~~ ✅ FIXED (1 test — section 6.10)

**Fixed 2026-03-14.** Section 6.10 now 34/34.

**Root cause:** Stale `value_buffer` from a prior `(values x)` call leaked into `call-with-values`. The VM's `Values` control primitive unconditionally sets `state.value_buffer = args.clone()` — even for single-value calls like `(test '(a . 4) (values x))`. A later `(set-cdr! x x)` mutated the pair into a circular list. When `call-with-values` ran its producer thunk (which returned a `#<values>` heap object from `exact-integer-sqrt`, bypassing the `values` intercept), the stale non-empty `value_buffer` was consumed instead of the actual producer result.

**Fix:** Clear `state.value_buffer` before running the producer thunk in `CallWithValues`, so only values produced by the current producer are seen.

---

## ~~Category 4: `eval` / `environment` / `null-environment`~~ ✅ FIXED

**Fixed 2026-03-13.** All 7 tests now pass (section 6.12: 5/5).

---

## ~~Category 5: `procedure?` for continuations~~ ✅ FIXED

**Fixed 2026-03-13.** Test `(call-with-current-continuation procedure?)` now passes.

---

## Summary of Progress

| Date | Score | Fixed |
|------|-------|-------|
| Start | 1158/1165 (99.4%) | — |
| 2026-03-13 (eval/environment) | +7 tests | eval, environment, null-environment |
| 2026-03-13 (procedure?) | +1 test | procedure? for continuations |
| 2026-03-13 (quasiquote) | +2 tests | nested quasiquote identifier→symbol |
| 2026-03-13 (hygiene) | +3 tests, -2 errors | alpha_rename pass for scope-set hygiene |
| 2026-03-14 (internal defines) | +1 test | internal define scoping (letrec* semantics) |
| 2026-03-14 (value_buffer) | +1 test | clear stale value_buffer in call-with-values |
| **Final** | **1163/1163 (100%)** | **All tests passing** ✅ |

---

## Infrastructure Notes

- **Resilient execution:** `run_script_vm` now uses `eval_program_resilient` for test files.
- **Test script:** `./scripts/run_chibi_tests_vm.sh`
- **Verification:** `cargo build --release && ./scripts/run_chibi_tests_vm.sh`
