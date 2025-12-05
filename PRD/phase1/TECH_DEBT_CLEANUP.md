# Tech Debt Cleanup Before Phase 2

This document tracks technical debt that should be addressed before starting Phase 2 (VM backend). Cleaning up these items will make VM implementation smoother and prevent carrying architectural debt forward.

## Overview

Based on a comprehensive audit of the 5 major crates (frontend, macros, interpreter, runtime, tree-walker), we identified issues at three priority levels:

- **HIGH**: Blocking issues that will complicate VM implementation
- **MEDIUM**: Code quality issues to clean up progressively
- **LOW**: Nice-to-have improvements

---

## HIGH Priority (Must Fix Before Phase 2)

### 1. Unify Evaluation Paths

**Crate**: patina-tree-walker
**Files**: `src/eval/mod.rs`, `src/eval/core_eval.rs`

**Problem**: Three parallel evaluation functions maintain similar logic:
- `eval_in_env()` - Value path
- `eval_expanded()` - CoreExpr-aware Value path
- `eval_core()` - CoreExpr path

Plus a fallback mechanism (lines 553-583 in mod.rs) that switches between paths based on whether forms are "CoreExpr-compatible".

**Impact on VM**: The VM will need a single, clean compilation target. Having dual paths means we'd need to support both in the VM or complete the migration anyway.

**Solution**:
1. Complete CoreExpr support for remaining fallback forms:
   - `let-syntax` / `letrec-syntax`
   - `case-lambda`
   - `expand` (debug form)
2. Remove Value-based evaluation path entirely
3. Remove fallback logic in backend.rs

**Effort**: Medium (2-3 days)

---

### 2. Fix Lambda Body Representation

**Crate**: patina-tree-walker
**Files**: `src/eval/core_eval.rs` (lines 876-1020)

**Problem**: Lambda closures store bodies as `Vec<Value>` even when created via CoreExpr path. This requires:
- `core_expr_to_value()` conversion when creating lambdas
- `value_to_core_simple()` conversion when applying lambdas
- `strip_identifiers_to_symbols()` cleanup pass

**Impact on VM**: The VM needs closures with compiled bytecode bodies, not Value ASTs. The current representation defeats the purpose of having an IR.

**Solution**:
1. Introduce `CoreClosure` type in patina-runtime:
   ```rust
   pub struct CoreClosure {
       pub params: Vec<String>,
       pub variadic: Option<String>,
       pub body: Vec<CoreExpr>,  // NOT Vec<Value>
       pub env: Rc<Environment>,
   }
   ```
2. Update `Procedure` enum to include `CoreLambda(CoreClosure)`
3. Evaluate CoreExpr bodies directly without conversion
4. Remove conversion functions

**Effort**: Medium-High (3-4 days)

---

### 3. Route Debug Output Through Logging

**Crate**: patina-tree-walker
**Files**: Throughout `src/eval/`

**Problem**: 30+ `eprintln!()` calls scattered through evaluation code for debug traces. These pollute stderr in normal operation.

**Impact on VM**: Debug infrastructure needs to work across backends. Ad-hoc eprintln won't scale.

**Solution**:
1. Add `tracing` crate dependency
2. Replace `eprintln!` with `tracing::debug!` or `tracing::trace!`
3. Add feature flag for verbose tracing
4. Update REPL to optionally enable tracing subscriber

**Effort**: Low (1 day)

---

### 4. Fix Interpreter Documentation

**Crate**: patina-interpreter
**Files**: `src/lib.rs`

**Problem**: Multiple documentation issues:
1. `USE_CORE_EXPR` env var documented (line 277) but never implemented
2. Contradictory statements about "primary" vs "default" evaluation path
3. `eval_program()` docs say "legacy Value-based" but code uses CoreExpr

**Impact on VM**: Confusing docs will make VM integration harder. Users won't know which API to use.

**Solution**:
1. Remove `USE_CORE_EXPR` documentation (or implement it)
2. Clarify that CoreExpr is now the default and only path
3. Update all method docs to reflect current behavior
4. Add migration notes for any API changes

**Effort**: Low (0.5 day)

---

### 5. Complete DEFINE_SYNTAX_ELIMINATION

**Crate**: patina-macros
**Files**: `src/macro_expander/mod.rs`, `src/macro_expander/matcher.rs`

**Problem**: Multiple TODOs reference `DEFINE_SYNTAX_ELIMINATION.md` for cleanup of literal shadowing logic. The current approach (passing `shadowed_names` through expansion) is a workaround for R7RS 4.3.2 compliance.

**Impact on VM**: Macro expansion happens before VM compilation. Cleaner macro infrastructure benefits all backends.

**Solution**:
1. Review the DEFINE_SYNTAX_ELIMINATION.md plan
2. Implement the cleaner approach if feasible
3. Or document the current approach as intentional and remove TODOs

**Effort**: Medium (2 days) - depends on complexity of cleaner approach

---

## MEDIUM Priority (Clean Up Progressively)

### 6. Implement Stub Libraries

**Crate**: patina-runtime
**File**: `src/stdlib/scheme_stubs.rs`

**Problem**: 7 libraries return empty exports, causing silent failures:
- `(scheme time)` - current-second, current-jiffy, jiffies-per-second
- `(scheme file)` - file I/O operations
- `(scheme read)` - read procedure
- `(scheme write)` - write-shared, write-simple (display/write in base)
- `(scheme eval)` - eval, environment
- `(scheme process-context)` - command-line, exit, get-environment-variable
- `(scheme r5rs)` - R5RS compatibility

**Solution**: Implement or explicitly error on import attempt.

**Effort**: High (1-2 weeks for full implementation)

---

### 7. Fix Primitive Library Organization

**Crate**: patina-runtime
**File**: `src/stdlib/scheme_base.rs`

**Problem**: Spec non-compliance in library organization:
- `sqrt` duplicated in (scheme base) and (scheme inexact) - should only be in inexact
- `real-part`, `imag-part` duplicated in (scheme base) and (scheme complex) - should only be in complex
- `library?` is Patina extension mixed with standard library

**Solution**:
1. Remove sqrt from (scheme base), keep only in (scheme inexact)
2. Remove real-part/imag-part from (scheme base), keep only in (scheme complex)
3. Move `library?` to new `(patina debug)` or `(patina core)` library

**Effort**: Low (0.5 day)

---

### 8. Eliminate Duplicated Code

**Crates**: patina-frontend, patina-interpreter

**Problem A** (frontend): Two near-identical functions for syntax-rules compilation:
- `compile_syntax_rules()` - without scopes
- `compile_syntax_rules_with_scopes()` - with scopes
~90% code duplication.

**Problem B** (interpreter): Duplicated parse-eval loops in:
- `eval_program()`
- `eval_program_core()`

**Solution**: Extract common logic into helper functions.

**Effort**: Low (1 day)

---

### 9. Fix Unsafe Unwrap Calls

**Crate**: patina-macros
**File**: `src/macro_expander/matcher.rs` (lines 446, 591)

**Problem**: HashMap access uses `.unwrap()` assuming keys exist:
```rust
branches.get_mut(&pvref).unwrap().push(match_value.clone());
```

**Solution**: Use `.entry()` API or add explicit error handling.

**Effort**: Low (0.5 day)

---

### 10. Split Large Files

**Crates**: patina-tree-walker, patina-macros

**Problem**: Several files exceed 1300 lines:
- `tree-walker/src/eval/mod.rs` - 1,690 lines
- `tree-walker/src/eval/core_eval.rs` - 1,607 lines
- `tree-walker/src/eval/primitives/arithmetic.rs` - 1,334 lines
- `macros/src/macro_expander/matcher.rs` - 1,174 lines
- `macros/src/macro_expander/compiler.rs` - 1,386 lines
- `macros/src/macro_expander/expander.rs` - 1,377 lines

**Solution**:
- Extract library loading from mod.rs to separate module
- Extract quasiquote from core_eval.rs
- Split arithmetic.rs by operation type
- Consider splitting macro phases into sub-modules

**Effort**: Medium (2-3 days)

---

### 11. Remove Incomplete CoreExpr Forms

**Crate**: patina-tree-walker
**File**: `src/eval/core_eval.rs` (lines 510-517)

**Problem**: Two CoreExpr forms exist but return errors:
- `CoreExpr::PrimCall` - optimization for direct primitive calls
- `CoreExpr::Let` - optimization for let expressions

These are dead code paths that exist in the IR but can't execute.

**Solution**: Either implement these optimizations or remove from CoreExpr enum.

**Effort**: Low (remove) or Medium (implement)

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

**Problem**: No `From<DesugarError>` for `InterpreterError`.

### 15. Improve Test Organization

**Crates**: patina-macros, patina-frontend

**Problem**: 800+ lines of test code mixed in production modules.

### 16. String Performance

**Crate**: patina-tree-walker

**Problem**: O(n) character indexing is spec-compliant but slow.

---

## Progress Tracking

| Item | Priority | Status | Notes |
|------|----------|--------|-------|
| 1. Unify evaluation paths | HIGH | Not Started | Blocks VM work |
| 2. Fix lambda body representation | HIGH | Not Started | Blocks VM work |
| 3. Route debug through logging | HIGH | Not Started | |
| 4. Fix interpreter documentation | HIGH | Not Started | |
| 5. DEFINE_SYNTAX_ELIMINATION | HIGH | Not Started | Review plan first |
| 6. Implement stub libraries | MEDIUM | Not Started | Can be incremental |
| 7. Fix primitive organization | MEDIUM | Not Started | |
| 8. Eliminate duplicated code | MEDIUM | Not Started | |
| 9. Fix unsafe unwrap | MEDIUM | Not Started | |
| 10. Split large files | MEDIUM | Not Started | |
| 11. Remove incomplete CoreExpr | MEDIUM | Not Started | |
| 12. Add GC for cycle handling | MEDIUM | Not Started | Use rust-gc crate |
| 13. Reduce clone() calls | LOW | Not Started | |
| 14. Add error conversions | LOW | Not Started | |
| 15. Improve test organization | LOW | Not Started | |
| 16. String performance | LOW | Not Started | |

---

## Definition of Done

Phase 1 tech debt cleanup is complete when:

1. All HIGH priority items are resolved
2. No fallback to Value-based evaluation in normal code paths
3. Lambda closures store CoreExpr bodies directly
4. Debug output uses proper logging infrastructure
5. Interpreter documentation accurately reflects implementation
6. At least 50% of MEDIUM priority items addressed

This positions the codebase for clean VM backend implementation in Phase 2.
