# Post-CPS Tech Debt (2025-12-12)

This document tracks technical debt introduced during the CPS (Continuation-Passing Style) transformation work. These issues were identified during a comprehensive code review of `patina-tree-walker` after CPS became the default evaluation mode.

**Related**: See `TECH_DEBT_CLEANUP.md` for pre-CPS tech debt items.

---

## Summary

| Priority | Count | Effort |
|----------|-------|--------|
| HIGH | 1 | ✅ COMPLETE |
| MEDIUM | 5 | Medium-Large |
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

### 2. Clean Up Debug Output

**Effort**: Small (1-2 hours)
**Crate**: patina-tree-walker

**Problem**: Debug statements left in code that should use tracing:

**Locations**:
- `cps_eval.rs`: CPS debug output
- `quasiquote.rs`: Quasiquote evaluation debug output (formerly core_eval.rs)

**Solution**: Replace with `tracing::debug!()` or `tracing::trace!()` calls. Infrastructure already exists.

---

### 3. TODO Comments from CPS Work

**Effort**: Medium (varies)
**Crate**: patina-tree-walker

**Found 5 significant TODOs**:

1. **Parameter converter** (`primitives/parameters.rs`, line 75)
   ```rust
   // TODO: Apply converter to initial value
   ```
   **Impact**: Parameter converters may not work correctly on initialization

2. **Unicode category support** (`primitives/characters.rs`, line 326)
   ```rust
   // TODO: Consider using unicode-general-category or unic-ucd-category
   ```
   **Impact**: Character category predicates may be incomplete

3. **Delimited continuation capture** (`cps_eval.rs`, lines 1479, 2632)
   ```rust
   // TODO: Implement proper capture for these special continuations
   // TODO: Implement proper delimited continuation capture
   ```
   **Impact**: Shift/reset operators may not work correctly

4. **Default prompt tag singleton** (`primitives/continuations.rs`, line 209)
   ```rust
   // TODO: Use a thread-local singleton for the default tag
   ```
   **Impact**: Each call creates new tag instead of singleton

---

### 4. Split Large CPS Files

**Effort**: Medium (4-6 hours)
**Crate**: patina-tree-walker

**Files needing decomposition**:

1. **`cps_eval.rs`** - 3,174 lines
   - Core CPS evaluation loop
   - Continuation mechanics
   - Exception handling
   - Dynamic-wind
   - Prompts

   **Recommendation**: Split into:
   - `cps_eval/core.rs` - Main evaluation loop
   - `cps_eval/continuations.rs` - Continuation handling
   - `cps_eval/exceptions.rs` - Exception handling
   - `cps_eval/prompts.rs` - Prompt/reset handling

2. **`io.rs`** - 2,761 lines (90 functions)

   **Recommendation**: Split into:
   - `io/ports.rs` - Port management
   - `io/read.rs` - Read operations
   - `io/write.rs` - Write operations

---

### 5. Excessive Unwrap/Expect Calls

**Effort**: Large (ongoing)
**Crate**: patina-tree-walker

**Problem**: 443 `unwrap()`/`expect()` calls in eval module

**High-risk locations**:
- `quasiquote.rs`: Multiple unwraps in quasiquote evaluation (formerly core_eval.rs)
- `cps_eval.rs`: Several unwraps in continuation handling
- `application.rs`: Parameter converter handling

**Solution**: Audit critical paths and convert to proper error handling. Focus on:
1. Any unwrap in CPS continuation path
2. Any unwrap in lambda application
3. Any unwrap in library loading

---

### 6. Test Coverage for CPS Code

**Effort**: Medium (6-8 hours)
**Crate**: patina-tree-walker

**Files with insufficient unit tests**:
- `eval/application.rs` - No unit tests for application logic
- `eval/library_support.rs` - No unit tests for path resolution
- `eval/mod.rs` - No direct unit tests

**CPS-specific testing gaps**:
- Exception handler stack management
- Dynamic-wind with multiple nested levels
- Continuation capture edge cases
- Prompt tag discrimination

---

## LOW Priority

### 7. Clone Optimization in CPS Evaluator

**Effort**: Large (needs profiling first)
**Crate**: patina-tree-walker

**Problem**: 162 clone() calls in `cps_eval.rs` alone, 457 total in eval module

**High-impact areas identified**:
- Environment clones during lambda application
- Value clones during continuation capture
- CpsExpr clones in evaluation loop

**Recommendation**: Profile before optimizing. Consider:
- Reference passing where lifetime allows
- Rc sharing instead of cloning
- Arena allocation for short-lived values

---

### 8. Stub Functions (Intentional)

**Priority**: LOW (Intentional by design)
**Crate**: patina-tree-walker
**File**: `src/eval/primitives/continuations.rs`

**Stubs that error instead of implementing**:
- `call_cc_stub()` (line 125)
- `call_with_continuation_prompt_stub()` (line 216)
- `abort_current_continuation_stub()` (line 243)
- `dynamic_wind_stub()` (line 273)

**Analysis**: These are intentional - they exist for direct mode which is now deprecated. Could be removed once direct mode is fully removed from the codebase.

---

## Progress Tracking

| Item | Priority | Status | Notes |
|------|----------|--------|-------|
| 1. Remove dead CPS code | HIGH | ✅ COMPLETE | Completed 2025-12-12 |
| 2. Clean up debug output | MEDIUM | Not Started | 13+ statements |
| 3. TODO comments | MEDIUM | Not Started | 5 significant TODOs |
| 4. Split large CPS files | MEDIUM | Not Started | cps_eval.rs, io.rs |
| 5. Fix unwrap/expect calls | MEDIUM | Not Started | 443 calls |
| 6. Test coverage for CPS | MEDIUM | Not Started | Multiple files |
| 7. Clone optimization | LOW | Not Started | Needs profiling |
| 8. Stub functions | LOW | N/A | Intentional |

---

## Recommended Cleanup Order

1. ✅ **Quick wins** (2-3 hours): COMPLETE
   - ~~Remove `new_with_cps()`~~
   - ~~Remove `higher_order.rs`~~
   - Convert debug output to tracing (remaining)

2. **Code quality** (4-6 hours):
   - Address TODO comments
   - Add CPS tests

3. **Refactoring** (8-12 hours):
   - Split `cps_eval.rs` into modules
   - Split `io.rs` into modules

4. **Performance** (10+ hours):
   - Profile hot paths
   - Optimize clones based on profiling data

---

## Related Documents

- `TECH_DEBT_CLEANUP.md` - Pre-CPS tech debt (mostly complete)
- `PRD/ARCHIVE/cps_continuation_2025_12/` - Archived CPS implementation docs
