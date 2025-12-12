# Tech Debt Cleanup Before Phase 2

This document tracks technical debt that should be addressed before starting Phase 2 (VM backend). Cleaning up these items will make VM implementation smoother and prevent carrying architectural debt forward.

## Overview

Based on a comprehensive audit of the 5 major crates (frontend, macros, interpreter, runtime, tree-walker), we identified issues at three priority levels:

- **HIGH**: Blocking issues that will complicate VM implementation
- **MEDIUM**: Code quality issues to clean up progressively
- **LOW**: Nice-to-have improvements

---

## HIGH Priority (Must Fix Before Phase 2)

### 1. ✅ Unify Evaluation Paths (COMPLETED 2024-12)

**Crate**: patina-tree-walker
**Files**: `src/eval/mod.rs`, `src/eval/core_eval.rs`

**Problem**: Three parallel evaluation functions maintain similar logic:
- `eval_in_env()` - Value path
- `eval_expanded()` - CoreExpr-aware Value path
- `eval_core()` - CoreExpr path

Plus a fallback mechanism (lines 553-583 in mod.rs) that switches between paths based on whether forms are "CoreExpr-compatible".

**Resolution**: The fallback mechanism was already dead code! All forms (`let-syntax`, `letrec-syntax`, `case-lambda`, `expand`) were already handled by the CoreExpr pipeline via the desugarer. Testing confirmed all 1,113 tests pass with the fallback disabled.

**Cleanup performed**:
1. Removed `is_value_evaluator_only_form()` function
2. Removed `FallbackFormNeeded` error variant from `DesugarError`
3. Simplified `eval_list_impl()` to route directly through CoreExpr (removed ~100 lines)
4. Simplified `eval_with_core_routing()` to remove fallback logic
5. Removed unused `eval_arguments()` function

**Status**: ✅ COMPLETE - All forms now use unified CoreExpr evaluation path

---

### 2. ✅ Fix Lambda Body Representation (COMPLETED 2024-12)

**Crate**: patina-tree-walker
**Files**: `src/eval/core_eval.rs`, `crates/patina-core/src/value.rs`

**Problem**: Lambda closures stored bodies using `LambdaBody` enum with two variants:
- `LambdaBody::Values(Vec<Value>)` - legacy path requiring conversion
- `LambdaBody::Core(Vec<CoreExpr>)` - optimized path

**Resolution**: Investigation revealed that `LambdaBody::Values` was **never constructed** - all lambdas already used `LambdaBody::Core`. The enum wrapper was dead code.

**Cleanup performed**:
1. Changed `LambdaBody` from enum to type alias: `pub type LambdaBody = Vec<CoreExpr>`
2. Removed all `LambdaBody::Core(...)` wrapper constructions (now just `body.clone()`)
3. Removed all match arms for `LambdaBody::Values` (dead code)
4. Removed `eval_lambda_body_values()` function (dead code)
5. Removed `eval_with_core_routing()` function (became dead after cleanup)
6. Removed `eval_value_simple()` function (became dead after cleanup)
7. Simplified `eval_lambda_body()` to directly evaluate CoreExpr

**Status**: ✅ COMPLETE - Lambda bodies now stored directly as `Vec<CoreExpr>`

---

### 3. ✅ Route Debug Output Through Logging (COMPLETED 2024-12)

**Crate**: patina-tree-walker
**Files**: Throughout `src/eval/`

**Problem**: 30+ `eprintln!()` calls scattered through evaluation code for debug traces. These pollute stderr in normal operation.

**Impact on VM**: Debug infrastructure needs to work across backends. Ad-hoc eprintln won't scale.

**Resolution**: Added `tracing` crate and replaced all `eprintln!()` with structured tracing:
- Warnings use `tracing::warn!` with structured fields
- Debug traces use `tracing::debug!` with target-specific filtering
- Environment lookups use `tracing::trace!` (more verbose)

**Cleanup performed**:
1. Added `tracing` dependency to workspace and patina-tree-walker
2. Added `verbose-tracing` feature flag for future use
3. Replaced 24 `eprintln!()` calls in `eval/mod.rs` and `eval/application.rs`
4. Kept existing `DebugConfig` infrastructure - tracing only emits when debug stages enabled
5. Enable with: `RUST_LOG=patina_tree_walker=debug` or `RUST_LOG=patina_tree_walker=trace`

**Status**: ✅ COMPLETE - All debug output now uses tracing infrastructure

---

### 4. ✅ Fix Interpreter Documentation (COMPLETED 2024-12)

**Crate**: patina-interpreter
**Files**: `src/lib.rs`

**Problem**: Multiple documentation issues:
1. `USE_CORE_EXPR` env var documented but never implemented (was removed after migration)
2. Contradictory statements about "primary" vs "default" evaluation path
3. `eval_program()` docs said "legacy Value-based" but code uses CoreExpr

**Resolution**: Updated documentation to reflect the unified CoreExpr evaluation path:

**Cleanup performed**:
1. Removed `USE_CORE_EXPR` env var documentation from `eval_str_core()`
2. Removed "experimental" and "Phase 2/Phase 3" references
3. Updated `eval_program()` doc - removed "legacy Value-based evaluator" reference
4. Clarified that `eval_str_core()` and `eval_program_core()` are for direct desugarer access

**Status**: ✅ COMPLETE - Documentation now accurately reflects unified CoreExpr path

---

### 5. ✅ Complete DEFINE_SYNTAX_ELIMINATION (COMPLETED 2024-12)

**Crate**: patina-macros, patina-frontend, patina-core
**Files**: Multiple files across macro system

**Problem**: `CoreExpr::DefineSyntax` existed as a compile-time artifact in the runtime IR, requiring the evaluator to handle macro compilation.

**Resolution**: The `DEFINE_SYNTAX_ELIMINATION.md` work was completed (archived in `PRD/ARCHIVE/phase1_completed/`):
- `CoreExpr::DefineSyntax` variant removed entirely
- All macro definitions (`define-syntax`) now compiled immediately during desugaring
- Macros installed in environment at desugar time, not eval time
- `shadowed_names` approach for R7RS 4.3.2 compliance is the correct design (not a workaround)

**Cleanup performed**:
1. Removed stale TODO comments referencing `DEFINE_SYNTAX_ELIMINATION.md`
2. The `shadowed_names` pattern is intentional for literal shadowing compliance

**Status**: ✅ COMPLETE - No `DefineSyntax` in CoreExpr, macros compiled at desugar time

---

## MEDIUM Priority (Clean Up Progressively)

### 6. ✅ Implement Stub Libraries (COMPLETED 2025-12)

**Status**: ✅ COMPLETE - All 7 libraries fully implemented

**Crate**: patina-runtime
**File**: `src/stdlib/scheme_stubs.rs` (removed - no longer needed)

**Problem**: Originally 7 libraries returned empty exports, causing silent failures.

**Resolution**: Implemented all libraries:

**Completed**:
- ✅ `(scheme time)` - DONE 2025-12: current-second, current-jiffy, jiffies-per-second
- ✅ `(scheme process-context)` - DONE 2025-12: command-line, exit, emergency-exit, get-environment-variable, get-environment-variables
- ✅ `(scheme write)` - DONE 2025-12: display, write, write-shared, write-simple
- ✅ `(scheme read)` - DONE 2025-12: read
- ✅ `(scheme file)` - DONE 2025-12: open-input-file, open-output-file, open-binary-input-file, open-binary-output-file, call-with-input-file, call-with-output-file, with-input-from-file, with-output-to-file, file-exists?, delete-file
- ✅ `(scheme eval)` - DONE 2025-12: eval, environment (in `patina-tree-walker/src/eval/primitives/eval.rs`)
- ✅ `(scheme r5rs)` - DONE 2025-12: null-environment, scheme-report-environment, plus all R5RS re-exports

**Files created/updated**:
- `src/stdlib/scheme_write.rs` - (scheme write) library
- `src/stdlib/scheme_read.rs` - (scheme read) library
- `src/stdlib/scheme_file.rs` - (scheme file) library (enhanced with binary ops)
- `patina-tree-walker/src/eval/primitives/eval.rs` - eval, environment, null-environment, scheme-report-environment

---

### 7. ✅ Fix Primitive Library Organization (COMPLETED 2025-12)

**Crate**: patina-runtime, patina-tree-walker
**Files**: `src/stdlib/scheme_base.rs`, `src/eval/primitives/predicates.rs`, `src/eval/primitives/debug.rs`

**Problem**: Spec non-compliance in library organization:
- `sqrt` duplicated in (scheme base) and (scheme inexact) - should only be in inexact
- `real-part`, `imag-part` duplicated in (scheme base) and (scheme complex) - should only be in complex
- `library?` is Patina extension mixed with standard library

**Resolution**:
1. Removed `sqrt` from `(scheme base)` - now only in `(scheme inexact)`
2. Removed `real-part`, `imag-part` from `(scheme base)` - now only in `(scheme complex)`
3. Moved `library?` from `(scheme base)` to `(patina debug)` library
4. Updated test to import `(patina debug)` before using `library?`

**Status**: ✅ COMPLETE

---

### 8. ✅ Eliminate Duplicated Code (COMPLETED 2024-12)

**Crates**: patina-frontend, patina-interpreter

**Problem A** (frontend): Two near-identical functions for syntax-rules compilation:
- `compile_syntax_rules()` - without scopes
- `compile_syntax_rules_with_scopes()` - with scopes
~90% code duplication.

**Problem B** (interpreter): Duplicated parse-eval loops in:
- `eval_program()`
- `eval_program_core()`

**Resolution**:
- **Problem A**: `compile_syntax_rules()` was dead code (marked `#[allow(dead_code)]`, never called). Removed entirely - only `compile_syntax_rules_with_scopes()` is needed.
- **Problem B**: Not actual duplication - `eval_program()` uses backend directly while `eval_program_core()` uses the desugarer for direct CoreExpr access. They serve different purposes.

**Status**: ✅ COMPLETE - Dead code removed, no actual duplication remained

---

### 9. ✅ Fix Unsafe Unwrap Calls (COMPLETED 2024-12)

**Crate**: patina-macros
**File**: `src/macro_expander/matcher.rs`

**Problem**: HashMap access uses `.unwrap()` assuming keys exist:
```rust
branches.get_mut(&pvref).unwrap().push(match_value.clone());
```

**Resolution**: Replaced `.get_mut().unwrap()` with `.entry().or_default()` which is safer and more idiomatic Rust. The `.entry()` API handles missing keys gracefully instead of panicking.

**Cleanup performed**:
1. Line 437: Changed `branches.get_mut(&pvref).unwrap()` to `branches.entry(pvref).or_default()`
2. Line 585: Changed `branches.get_mut(pvref).unwrap()` to `branches.entry(*pvref).or_default()`

**Status**: ✅ COMPLETE - No more unsafe unwrap calls in HashMap access

---

### 10. Split Large Files

**Crates**: patina-tree-walker, patina-macros

**Problem**: Several files exceed 1300 lines.

**Progress**:
- ✅ `core_eval.rs` renamed to `quasiquote.rs` (~500 lines) - now only contains quasiquote logic
- ✅ Legacy eval methods removed from `mod.rs` (reduced from 1,690 to ~1,200 lines)
- ✅ `matcher.rs` split into `matcher/` directory (2025-12):
  - `matcher/mod.rs` - Matcher struct and core matching (~430 lines)
  - `matcher/error.rs` - MatchError enum (~100 lines)
  - `matcher/list_match.rs` - List/ellipsis matching (~300 lines)
  - `matcher/literal.rs` - Literal matching and hygiene (~130 lines)
  - `matcher/debug.rs` - Debug printing (~55 lines)
- ✅ `compiler.rs` split into `compiler/` directory (2025-12):
  - `compiler/mod.rs` - Compiler struct, constructors, compile_macro (~200 lines)
  - `compiler/pattern.rs` - Pattern compilation (~260 lines)
  - `compiler/template.rs` - Template compilation (~250 lines)
  - `compiler/escape.rs` - Ellipsis escape handling (~180 lines)
  - `compiler/helpers.rs` - Helper methods (~170 lines)
  - `compiler/tests.rs` - Unit tests (~300 lines)
- ✅ `expander.rs` split into `expander/` directory (2025-12):
  - `expander/mod.rs` - Expander struct, expand(), expand_impl() (~200 lines)
  - `expander/error.rs` - ExpandError enum (~70 lines)
  - `expander/list.rs` - List, vector, dotted list expansion (~130 lines)
  - `expander/ellipsis.rs` - Ellipsis expansion (single and double) (~420 lines)
  - `expander/hygiene.rs` - Identifier renaming and hygiene (~140 lines)
  - `expander/tests.rs` - Unit tests (~500 lines)

**Remaining (patina-macros)**: ✅ All large files split!

**Remaining (patina-tree-walker)** - See `POST_CPS_TECH_DEBT.md` item 4 for details:
- `cps_eval.rs` (~3,100 lines) → split into core, continuations, exceptions, prompts
- `io.rs` (~2,700 lines) → split into ports, read, write
- `arithmetic.rs` (1,334 lines) → split by operation type

**Effort**: Medium (2-3 days)

---

### 11. ✅ Remove Incomplete CoreExpr Forms (COMPLETED 2025-12)

**Crate**: patina-core, patina-ir
**Status**: ✅ COMPLETE

**Problem**: Two CoreExpr forms existed but were never generated by the desugarer:
- `CoreExpr::PrimCall` - optimization for direct primitive calls
- `CoreExpr::Let` - optimization for let expressions

These were marked as "Optional optimized forms (added by passes)" but no optimization pass existed.

**Resolution**: Removed entirely:
1. Removed `CoreExpr::PrimCall` and `CoreExpr::Let` variants from `patina-core/src/core_expr.rs`
2. Removed `Primitive` enum from `patina-core/src/core_expr.rs`
3. Removed `transform_primcall`, `convert_primitive`, `transform_let` from `patina-ir/src/cps_transform.rs`
4. Updated `is_tail_position`, `kind()`, `map_children`, and `Display` impl
5. Removed `Primitive` from re-exports in both patina-core and patina-ir

**Notes**: If these optimizations are needed in the future, they can be easily re-added with proper CPS support.

---

### 12. Add Garbage Collection for Cycle Handling

**Crate**: patina-runtime (affects all crates using Value)
**Files**: `src/value/mod.rs`, all code using `Rc<RefCell<T>>`

**Problem**: Current memory management uses `Rc<RefCell<T>>` which cannot collect cycles:

```scheme
;; This creates a cycle that Rc can't collect - memory leak
(define x (cons 1 2))
(set-cdr! x x)  ;; x now points to itself
```

R7RS allows `set-car!` and `set-cdr!` to create circular structures. Without cycle collection, these leak memory indefinitely.

**GC Options Evaluated**:

| Crate | Type | Handles Cycles | Maturity | Notes |
|-------|------|----------------|----------|-------|
| **gc** (rust-gc) | Mark-and-sweep | Yes | High | Best fit for interpreter |
| **bacon-rajan-cc** | Cycle-collecting RC | Yes | Medium | Drop-in for Rc |
| **shredder** | Concurrent GC | Yes | Medium | Overkill for tree-walker |
| **bumpalo** | Arena (no GC) | No | High | No cycle handling |

**Recommended**: Use `rust-gc` crate for tree-walker:

```rust
use gc::{Gc, GcCell, Trace, Finalize};

#[derive(Trace, Finalize)]
pub enum Value {
    Integer(i64),
    Pair(Gc<GcCell<(Value, Value)>>),  // Was: Rc<RefCell<...>>
    Vector(Gc<GcCell<Vec<Value>>>),
    // ...
}
```

**Migration Path**:
1. Add `gc` dependency to patina-runtime
2. Derive `Trace` and `Finalize` on Value and related types
3. Replace `Rc<RefCell<T>>` with `Gc<GcCell<T>>` for heap-allocated values
4. Keep `Rc` for immutable shared data (symbols, compiled macros)
5. Update all crates that pattern-match on Value

**Trade-offs**:
- **Pro**: Correct R7RS semantics for circular structures
- **Pro**: Drop-in replacement API (`Gc` works like `Rc`)
- **Pro**: Mature crate (used by Servo)
- **Con**: Requires `#[derive(Trace, Finalize)]` on all GC'd types
- **Con**: Stop-the-world collection (acceptable for tree-walker)
- **Con**: Some runtime overhead vs pure Rc

**Note**: VM backend (Phase 2) may use different GC strategy (generational, concurrent). Tree-walker just needs correctness, not optimal performance.

**Effort**: Medium-High (3-4 days) - touches many files but mechanical changes

---

## LOW Priority (Nice to Have)

### 13. Reduce Clone() Calls

**All crates**: 400+ clone() calls identified across codebase.

**Solution**: Profile hot paths, use Rc/Gc more aggressively. Note that GC adoption (item 12) may reduce some cloning by enabling more sharing.

### 14. Add Missing Error Conversions

**Crate**: patina-interpreter
**Status**: ✅ COMPLETE (2025-12-12)

**Problem**: No `From<DesugarError>` for `InterpreterError`.

**Resolution**: Added `From<DesugarError> for InterpreterError<E>` in `patina-interpreter/src/lib.rs`.
This was completed as part of the unified error system work (Phase 1 of ERROR_SYSTEM_DESIGN.md).

### 15. Improve Test Organization

**Crates**: patina-macros, patina-frontend
**Status**: ✅ REVIEWED (2025-12-12) - Current organization is acceptable

**Original concern**: 800+ lines of test code mixed in production modules.

**Analysis (2025-12-12)**:
- Total: ~483 unit tests inline, ~1,000+ integration tests in patina-tests
- Macro crate tests already partially extracted: `compiler/tests.rs`, `expander/tests.rs`
- High test ratios in macro code (interface.rs: 2.4x, matcher/mod.rs: 1.3x) are justified by domain complexity
- Frontend tests (parser, desugarer, lexer) test private internals - appropriate for inline

**Conclusion**: Current organization follows Rust conventions. High inline test ratios in macro code reflect the complexity of hygiene and pattern matching. See `docs/TEST_ORGANIZATION.md` for full analysis.

### 16. String Performance

**Crate**: patina-tree-walker

**Problem**: O(n) character indexing is spec-compliant but slow.

---

## Progress Tracking

| Item | Priority | Status | Notes |
|------|----------|--------|-------|
| 1. Unify evaluation paths | HIGH | ✅ DONE | Completed 2024-12: removed dead fallback code |
| 2. Fix lambda body representation | HIGH | ✅ DONE | Completed 2024-12: LambdaBody now Vec<CoreExpr> |
| 3. Route debug through logging | HIGH | ✅ DONE | Completed 2024-12: using tracing crate |
| 4. Fix interpreter documentation | HIGH | ✅ DONE | Completed 2024-12: removed stale USE_CORE_EXPR docs |
| 5. DEFINE_SYNTAX_ELIMINATION | HIGH | ✅ DONE | Completed 2024-12: DefineSyntax removed from CoreExpr |
| 6. Implement stub libraries | MEDIUM | ✅ DONE | All 7 done: time, process-context, write, read, file, eval, r5rs completed 2025-12 |
| 7. Fix primitive organization | MEDIUM | ✅ DONE | Completed 2025-12: sqrt, real-part, imag-part, library? moved |
| 8. Eliminate duplicated code | MEDIUM | ✅ DONE | Completed 2024-12: removed dead compile_syntax_rules |
| 9. Fix unsafe unwrap | MEDIUM | ✅ DONE | Completed 2024-12: using entry().or_default() |
| 10. Split large files | MEDIUM | ✅ DONE | Completed 2024-12: splitted multiple file with >= 1300 lines |
| 11. Remove incomplete CoreExpr | MEDIUM | ✅ DONE | Completed 2025-12: PrimCall, Let, Primitive removed |
| 12. Add GC for cycle handling | MEDIUM | Not Started | Use rust-gc crate |
| 13. Reduce clone() calls | LOW | Not Started | |
| 14. Add error conversions | LOW | ✅ DONE | Completed 2025-12-12 |
| 15. Improve test organization | LOW | ✅ REVIEWED | Current organization acceptable |
| 16. String performance | LOW | Not Started | |

---

## Definition of Done

Phase 1 tech debt cleanup is complete when:

1. ✅ All HIGH priority items are resolved (DONE 2024-12: 5/5 complete)
2. ✅ No fallback to Value-based evaluation in normal code paths (DONE 2024-12)
3. ✅ Lambda closures store CoreExpr bodies directly (DONE 2024-12)
4. ✅ Debug output uses proper logging infrastructure (DONE 2024-12)
5. ✅ Interpreter documentation accurately reflects implementation (DONE 2024-12)
6. ✅ At least 50% of MEDIUM priority items addressed (6/7 DONE: all stub libraries, primitive org, duplicated code, unsafe unwrap; 1 not started)

This positions the codebase for clean VM backend implementation in Phase 2.

---

## Related Documents

- **`POST_CPS_TECH_DEBT.md`** - Tech debt introduced during CPS transformation (2025-12-12)
  - Dead code removal, debug output cleanup, TODO comments
  - Large file splitting (cps_eval.rs, io.rs)
  - Unwrap/expect audit, clone optimization, test coverage
