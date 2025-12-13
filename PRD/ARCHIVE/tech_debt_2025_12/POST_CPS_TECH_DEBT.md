# Post-CPS Tech Debt (2025-12-12)

This document tracks technical debt introduced during the CPS (Continuation-Passing Style) transformation work. These issues were identified during a comprehensive code review of `patina-tree-walker` after CPS became the default evaluation mode.

**Related**: See `TECH_DEBT_CLEANUP.md` for pre-CPS tech debt items.

---

## Summary

| Priority | Count | Effort |
|----------|-------|--------|
| HIGH | 1 | ✅ COMPLETE |
| MEDIUM | 5 | Medium-Large (1 partially complete) |
| LOW | 2 | Large (needs profiling) |

---

## HIGH Priority

### 1. Remove Dead Code from CPS Migration ✅ COMPLETE (2025-12-12)

**Effort**: Small (2-3 hours)
**Crate**: patina-tree-walker
**Status**: ✅ COMPLETE

**Completed items**:

1. ✅ **Removed `new_with_cps()` in `backend.rs`**
   - CPS is now the only evaluation mode

2. ✅ **Removed `higher_order.rs`**
   - File deleted, module declaration removed from `mod.rs`
   - `map` and `for-each` remain in Scheme (`lib/scheme/base/higher_order.scm`)

3. ✅ **Audited `#[allow(dead_code)]` annotations in `registry.rs`**
   - All methods are intentional API for future help system
   - Methods have test coverage in unit tests
   - Annotations are correct - kept as-is with documentation

4. ✅ **Audited `ContValue::Captured` in `cps_eval.rs`**
   - Variant is matched in 3 places but never constructed
   - Needed for future continuation serialization support
   - Added clarifying comment and kept `#[allow(dead_code)]`

5. ✅ **Fixed incorrect annotation in `debug.rs`**
   - Removed `#[allow(dead_code)]` from `DebugConfig` struct (it IS used)
   - Kept annotation on `enabled_list()` method (truly unused, for future use)

6. ✅ **Removed `Procedure::Lambda` variant**
   - All lambdas now use `Procedure::CpsLambda`
   - Removed from `value.rs` enum definition
   - Updated `Display` impl in `value.rs`
   - Removed handling in `application.rs`
   - Removed error case in `cps_eval.rs`

7. ✅ **Removed legacy evaluation methods from `mod.rs`**
   - `eval_step_expanded`, `eval_step_impl_expanded`, `eval_list_impl_expanded`
   - `expand_all_macros`, `eval_expanded`, `eval_arguments_expanded`
   - `prepare_lambda_env`, `eval_lambda_body`, `resolve_tail_call_core_rc`
   - `extract_pair`, `expand_macro`

8. ✅ **Renamed `core_eval.rs` → `quasiquote.rs`**
   - File only contains quasiquote evaluation logic
   - Updated `mod.rs` module declaration
   - Updated `cps_eval.rs` to reference `super::quasiquote`
   - Removed `eval_core` export from `lib.rs`

---

## MEDIUM Priority

### 2. Clean Up Debug Output ✅ COMPLETE (2025-12-12)

**Effort**: Small (1-2 hours)
**Crate**: patina-tree-walker

**Problem**: Debug statements left in code that should use tracing.

**Resolution**:
- Replaced 5 `eprintln!` calls in `cps_eval/mod.rs` with `tracing::debug!` calls
- `cps_eval/quasiquote.rs` had no debug statements (already clean)
- Debug output now controlled via `RUST_LOG=patina_tree_walker::eval::cps_eval=debug`

---

### 3. TODO Comments from CPS Work ✅ COMPLETE (2025-12-12)

**Effort**: Medium (varies)
**Crate**: patina-tree-walker

**Original 5 TODOs - all resolved**:

1. ✅ **Parameter converter** (`primitives/parameters.rs`)
   - Fixed: Converter now applied to initial value via `evaluator.apply()`

2. ✅ **Unicode category support** (`primitives/characters.rs`)
   - Resolved: Current Unicode 15.0 table is comprehensive; TODO converted to documentation note

3. 🔶 **Delimited continuation capture** - DEFERRED to `DELIMITED_CONTINUATIONS_DESIGN.md`
   - `cps_eval/continuation.rs`, `cps_eval/wind.rs`
   - Full design and implementation plan in that document

4. 🔶 **Default prompt tag singleton** (`primitives/continuations.rs`)
   - Related to delimited continuations - will be addressed in `DELIMITED_CONTINUATIONS_DESIGN.md`

---

### 4. Split Large CPS Files

**Effort**: Medium (4-6 hours)
**Crate**: patina-tree-walker

**Note**: For macro-related large files (matcher.rs, compiler.rs, expander.rs), see `TECH_DEBT_CLEANUP.md` item 10.

**Files needing decomposition**:

1. **`cps_eval.rs`** - ~3,100 lines ✅ COMPLETE (2025-12-12)

   Split into modular `cps_eval/` directory structure:

   | Module | Lines | Purpose |
   |--------|-------|---------|
   | `mod.rs` | ~400 | CpsEvaluator struct, eval(), trampoline loop, eval_cps() |
   | `types.rs` | ~250 | ContValue, StepResult, PromptFrame, ExceptionHandler |
   | `step.rs` | ~460 | eval_one_step - main dispatch for all CpsExpr forms |
   | `application.rs` | ~790 | apply_cps_step, eval_primop, CPS-sensitive primitives |
   | `continuation.rs` | ~310 | capture/restore/reify continuations, invoke_continuation_step |
   | `environment.rs` | ~200 | eval_trivial, lookup_var, set_var, make_cps_closure |
   | `wind.rs` | ~230 | run_wind_handlers, force_promise_cps, apply_from_direct |
   | `exceptions.rs` | ~100 | maybe_route_error_through_cps |
   | `quasiquote.rs` | ~260 | Quasiquote evaluation (moved from eval/quasiquote.rs) |

   Also removed `eval/quasiquote.rs` since only cps_eval uses it.

2. **`io.rs`** - ~2,700 lines ✅ COMPLETE (2025-12-12)

   Split into modular `io/` directory structure:

   | Module | Lines | Purpose |
   |--------|-------|---------|
   | `mod.rs` | ~400 | Registration, tests, re-exports |
   | `datum_writer.rs` | ~300 | DatumLabelWriter for circular structures |
   | `ports.rs` | ~400 | Port predicates, string/bytevector ports, current ports |
   | `text_output.rs` | ~180 | display, write, write-shared, newline, write-char, write-string |
   | `text_input.rs` | ~120 | read-char, peek-char, char-ready?, read-line, read-string |
   | `binary.rs` | ~260 | read-u8, peek-u8, write-u8, read-bytevector, write-bytevector |
   | `read.rs` | ~240 | read S-expression parsing |
   | `file.rs` | ~270 | File I/O and higher-order file procedures |

3. **`arithmetic.rs`** - ~1,335 lines ✅ COMPLETE (2025-12-12)

   Split into modular `arithmetic/` directory structure:

   | Module | Lines | Purpose |
   |--------|-------|---------|
   | `mod.rs` | ~430 | Registration of all primitives, unit tests |
   | `helpers.rs` | ~60 | Error conversion, rational_to_value helper |
   | `basic.rs` | ~100 | add, subtract, multiply, divide |
   | `comparison.rs` | ~100 | =, <, >, <=, >= |
   | `division.rs` | ~110 | quotient, remainder, modulo, floor/, truncate/ |
   | `rounding.rs` | ~70 | floor, ceiling, truncate, round, abs, max, min |
   | `transcendental.rs` | ~110 | sqrt, square, expt, sin, cos, tan, asin, acos, atan, exp, log |
   | `predicates.rs` | ~50 | finite?, infinite?, nan? |
   | `complex.rs` | ~60 | real-part, imag-part, magnitude, angle, make-rectangular, make-polar |
   | `number_theory.rs` | ~200 | gcd, lcm, numerator, denominator, exact, inexact, exact-integer-sqrt, rationalize |

---

### 5. Excessive Unwrap/Expect Calls

**Effort**: Large (ongoing)
**Crate**: patina-tree-walker
**Status**: ✅ PHASE 1-4 COMPLETE - See `PRD/ARCHIVE/error_system_2025_12/ERROR_SYSTEM_DESIGN.md`

**Phase 1 completed (2025-12-12)**:
- ✅ Created unified `ErrorDetail`, `ErrorKind`, `SourceLocation` in `patina-core/src/error.rs`
- ✅ Removed duplicate `SchemeExceptionKind` from `patina-tree-walker`
- ✅ Added `From<DesugarError>` for `InterpreterError` (fixes TECH_DEBT item 14)
- ✅ Added conversion methods: `EvalError::to_error_kind()`, `EvalError::to_error_detail()`

**Phase 2 partial (2025-12-12)**:
- ✅ Added `FrontendError::to_error_kind()`, `to_error_detail()`, `From<FrontendError> for ErrorDetail`
- ✅ Added `DesugarError::to_error_kind()`, `to_error_detail()`, `From<DesugarError> for ErrorDetail`
- ✅ Added `patina-core` dependency to `patina-frontend`
- ⏳ Source location tracking deferred (requires SOURCE_INFO_PLAN.md implementation)

**Phase 3 completed (2025-12-12)**:
- ✅ Expanded `maybe_route_error_through_cps` to handle ALL catchable error types
- ✅ Updated `apply_cps_step` to route `NotAProcedure` and arity errors
- ✅ Updated `CpsExpr::Var` handling to route `UndefinedVariable` errors
- ✅ Updated `CpsExpr::Continue` to route errors from `eval_trivial`
- ✅ Added 8 comprehensive tests for error catching with `guard`

**Analysis (2025-12-12)**:

The original estimate of 443 unwrap/expect calls was inflated by test code. Actual production code has ~15 unwraps:

| Category | Count | Risk |
|----------|-------|------|
| Safe (after arity check) | 4 | ✅ None |
| Safe (stack invariants) | 2 | ✅ None |
| Risky (needs fixing) | 3 | ⚠️ Medium |
| Internal/startup only | 2 | ⚠️ Low |

**Risky locations to fix** (deferred, low priority):
- `io/read.rs:57` - `remaining.unwrap()` without prior check
- `number_theory.rs:196-197` - `BigRational::from_f64().unwrap()`

**Phase 4 completed (2025-12-12)**:
- ✅ Added `file-error?` and `read-error?` predicate tests
- ✅ Added `error-object-irritants` test
- ✅ All 35 CPS feature tests passing (7 ignored for known bugs)

**Remaining work (see ARCHIVE/error_system_2025_12/ERROR_SYSTEM_DESIGN.md)**:
- ⏳ Source location tracking (blocked by SOURCE_INFO_PLAN.md)
- ⏳ Risky unwrap calls (low priority, 2 locations)

---

### 6. Test Coverage for CPS Code ✅ COMPLETE (2025-12-12)

**Effort**: Medium (6-8 hours)
**Crate**: patina-tree-walker
**Status**: ✅ COMPLETE - Comprehensive integration tests added

**CPS-specific integration tests**: ✅ COMPLETE
- `crates/patina-tests/tests/cps_features.rs` with 43 tests
- **36 tests passing**, 7 ignored (document known bugs)

| Category | Tests | Status |
|----------|-------|--------|
| Exception handler stack | 6 | ✅ 5 pass, 1 ignored (raise-continuable bug) |
| Dynamic-wind nested | 5 | ✅ 4 pass, 1 ignored (exception/after bug) |
| Continuation capture | 7 | ✅ All pass |
| Guard macro | 4 | ✅ All pass |
| Exception + dynamic-wind | 3 | ✅ 1 pass, 2 ignored (same after-thunk bug) |
| Error objects | 3 | ✅ All pass |
| Complex control flow | 2 | 2 ignored (continuation + exception interaction) |
| Runtime error routing | 11 | ✅ All pass |
| Backtracking (amb-style) | 1 | ✅ Pass |

**Known bugs documented via ignored tests** (7 total):
1. `raise-continuable` handler consumed after first use
2. `dynamic-wind` after thunk not run on exception propagation
3. `dynamic-wind` after thunk not run on call/cc escape
4. Before/after thunks not run on continuation re-entry

**Note**: Unit tests for individual modules (application.rs, library_support.rs, mod.rs) are lower priority since integration tests provide good coverage of CPS behavior.

---

## LOW Priority

### 7. Clone Optimization in CPS Evaluator

**Status**: 🔶 DEFERRED - See `CLONE_OPTIMIZATION_ANALYSIS.md`

**Effort**: Large (needs profiling first)
**Crate**: patina-tree-walker

**Analysis complete**: See `CLONE_OPTIMIZATION_ANALYSIS.md` for detailed breakdown.

**Key findings**:
- ~162 clone() calls in cps_eval directory
- Environment clones (Rc) are already cheap - no action needed
- `cont_env: HashMap` and `dynamic_winds: Vec` in StepResult are expensive
- Potential fix: Use `im` crate for persistent data structures (O(1) clone)

**Decision**: Defer until profiling confirms hot paths. Estimated 15-35% speedup if optimized.

---

### 8. Stub Functions ✅ COMPLETE (2025-12-12)

**Priority**: LOW
**Crate**: patina-tree-walker
**File**: `src/eval/primitives/continuations.rs`

**Resolution**: Removed all dead stub functions:
- `call_cc_stub()` - removed (call/cc handled via CpsExpr::CallCC)
- `call_with_continuation_prompt_stub()` - removed (handled via CpsExpr::Prompt)
- `abort_current_continuation_stub()` - removed (handled via CpsExpr::Abort)
- `dynamic_wind_stub()` - removed (handled specially in apply_cps_step)

Also removed their primitive registrations. These stubs only existed for "direct mode" which no longer exists since CPS is now the only evaluation mode.

Kept the useful primitives: `continuation?`, `make-continuation-prompt-tag`, `continuation-prompt-tag?`, `default-continuation-prompt-tag`.

---

## Progress Tracking

| Item | Priority | Status | Notes |
|------|----------|--------|-------|
| 1. Remove dead CPS code | HIGH | ✅ COMPLETE | Completed 2025-12-12 |
| 2. Clean up debug output | MEDIUM | ✅ COMPLETE | Replaced with tracing |
| 3. TODO comments | MEDIUM | ✅ COMPLETE | Fixed 2, deferred 2 to DELIMITED_CONTINUATIONS_DESIGN.md |
| 4. Split large CPS files | MEDIUM | ✅ COMPLETE | cps_eval.rs ✅, io.rs ✅, arithmetic.rs ✅ |
| 5. Fix unwrap/expect calls | MEDIUM | 🔶 DEFERRED | See ERROR_SYSTEM_DESIGN.md |
| 6. Test coverage for CPS | MEDIUM | ✅ COMPLETE | 36/43 tests pass, 7 bugs documented |
| 7. Clone optimization | LOW | 🔶 DEFERRED | See CLONE_OPTIMIZATION_ANALYSIS.md |
| 8. Stub functions | LOW | ✅ COMPLETE | Dead code removed |

---

## Recommended Cleanup Order

1. ✅ **Quick wins** (2-3 hours): COMPLETE
   - ~~Remove `new_with_cps()`~~
   - ~~Remove `higher_order.rs`~~
   - ~~Convert debug output to tracing~~ ✅ COMPLETE (2025-12-12)

2. **Code quality** (4-6 hours):
   - Address TODO comments
   - ~~Add CPS tests~~ ✅ PARTIAL (24 passing, 7 documenting bugs)

3. **Refactoring** (8-12 hours): ✅ COMPLETE
   - ~~Split `cps_eval.rs` into modules~~ ✅ COMPLETE (2025-12-12)
   - ~~Split `io.rs` into modules~~ ✅ COMPLETE (2025-12-12)
   - ~~Split `arithmetic.rs` into modules~~ ✅ COMPLETE (2025-12-12)

4. **Performance** (10+ hours):
   - Profile hot paths
   - Optimize clones based on profiling data

---

## Related Documents

- `TECH_DEBT_CLEANUP.md` - Pre-CPS tech debt (mostly complete)
- `GC_DESIGN.md` - Design doc for garbage collection (deferred from TECH_DEBT_CLEANUP.md)
- `DELIMITED_CONTINUATIONS_DESIGN.md` - Design doc for delimited continuations (deferred from this doc)
- `CLONE_OPTIMIZATION_ANALYSIS.md` - Clone analysis for tree-walker (deferred, needs profiling)
- `PRD/ARCHIVE/cps_continuation_2025_12/` - Archived CPS implementation docs
