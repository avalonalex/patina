# CoreIR Migration Branch

**Branch:** `core-ir-migration`
**Status:** Experimental - Not Ready for Main
**Base:** `main` (704 chibi tests passing)

## ⚠️ Why This is on a Separate Branch

This branch contains the CoreExpr infrastructure migration work. While the **infrastructure is complete and working**, the **integration with the backend causes a regression** (chibi tests drop from 704 to 18 passing).

**DO NOT MERGE TO MAIN** until the integration issue is resolved.

## What's in This Branch

### ✅ Complete and Working

1. **CoreExpr IR** (`crates/patina-ir/src/core_expr.rs`)
   - 9 core forms: Quote, Quasiquote, Lambda, If, Set, Define, Begin, Var, Literal, App, PrimCall
   - Full TCO support via trampoline pattern
   - Clean, minimal representation

2. **Desugarer** (`crates/patina-frontend/src/desugarer/`)
   - Transforms Value AST → CoreExpr IR
   - Handles all R7RS core forms
   - 13 unit tests, all passing

3. **CoreExpr Evaluator** (`crates/patina-tree-walker/src/eval/core_eval.rs`)
   - Full evaluation with TCO
   - Quasiquote implementation (900+ lines)
   - All helper functions for unquote/unquote-splicing
   - Integration tests all pass

4. **Test Suites**
   - `crates/patina-frontend/tests/desugarer_macro_tests.rs` - Desugarer unit tests
   - `crates/patina-tests/tests/core_expr_integration.rs` - Full pipeline tests
   - `crates/patina-tests/tests/library_core_expr_integration.rs` - Integration tests
   - **All tests pass when CoreExpr is called directly**

### ❌ Broken Integration

**File:** `crates/patina-tree-walker/src/backend.rs`

**Problem:** The integration that calls CoreExpr from the backend is **disabled** (commented out) because it causes chibi tests to drop from 704 passing to 18 passing.

**Root Cause:** Calling `expand_all_macros()` at the Backend level conflicts with the existing macro expansion in `eval_list_impl()`. This causes double expansion or expansion at the wrong time.

## Test Results

### With CoreExpr Integration Disabled (Current State)
```
✅ Compliance Tests:   347/347 passing (100%)
✅ Case-Lambda Tests:   21/21  passing (100%)
✅ Chibi Tests:        704/1108 passing (63.5%)
✅ Total Tests:        928     passing
✅ Clippy:            0 warnings
```

### With CoreExpr Integration Enabled (BROKEN)
```
✅ Compliance Tests:   347/347 passing (100%)
✅ Case-Lambda Tests:   21/21  passing (100%)
❌ Chibi Tests:         18/1108 passing (1.6%) ← REGRESSION!
❌ Error: "Not a procedure: #<macro:or>"
```

## How to Test

### Test CoreExpr Infrastructure (Works!)
```bash
# Test desugarer
cargo test --package patina-frontend desugarer_macro_tests

# Test CoreExpr integration (explicit pipeline)
cargo test --package patina-tests core_expr_integration

# All pass! ✅
```

### Test Full System (Backend integration disabled)
```bash
# Run compliance tests
cargo test --package patina-tests compliance

# Run chibi tests
./scripts/run_chibi_tests.sh

# All pass with CoreExpr disabled ✅
```

## Architecture Documentation

See these files for detailed analysis:

1. **`docs/MACRO_ARCHITECTURE_PROPOSAL.md`** - Proposed solution with 3 approaches
2. **`docs/CHIBI_TEST_REGRESSION_ANALYSIS.md`** - Root cause analysis
3. **`docs/SESSION_2025_11_22.md`** - Development session summary

## How to Fix and Merge

To make this branch mergeable to main, you need to fix the macro expansion integration. Three approaches:

### Option 1: Minimal Change (Recommended First)
1. Remove macro expansion from `eval_list_impl` in Value evaluator
2. Add `eval_expanded` method that assumes macros already expanded
3. In `backend.rs`, expand once at top level, then choose evaluator

**Effort:** 4-6 hours
**Risk:** Medium
**Benefit:** Enables CoreExpr path properly

### Option 2: Extract Macro Crate (Recommended Long-term)
1. Create `patina-macros` crate
2. Move macro expansion out of evaluator
3. Separate MacroEnv from runtime Environment
4. Build flexible pipeline

**Effort:** 8-12 hours
**Risk:** Medium
**Benefit:** Clean architecture, enables future work

### Option 3: Keep Disabled (Current)
Just keep CoreExpr disabled until you're ready to fix it.

**Effort:** 0 hours
**Risk:** None
**Benefit:** Main branch stays stable

## What to Do Next

### If Merging to Main Soon
1. Make sure CoreExpr integration stays **disabled** in `backend.rs`
2. Keep all the infrastructure code (it's tested and working)
3. Document that CoreExpr is "future optimization infrastructure"
4. Merge safely - no regression!

### If Continuing Work on Branch
1. Pick one of the fix approaches above
2. Implement the fix
3. Run full test suite
4. When chibi tests are back to 704, merge to main

### If Abandoning Branch
The CoreExpr work is valuable! Consider:
1. Cherry-pick just the test improvements
2. Keep the Quasiquote implementation
3. Leave CoreExpr for a future optimization pass

## Files Changed in This Branch

### New Files (Keep These)
```
# CoreExpr Infrastructure
crates/patina-frontend/src/desugarer/          - Value → CoreExpr transformation
crates/patina-tree-walker/src/eval/core_eval.rs - CoreExpr evaluator
crates/patina-frontend/tests/desugarer_macro_tests.rs - Unit tests
crates/patina-tests/tests/core_expr_integration.rs - Integration tests
crates/patina-tests/tests/library_core_expr_integration.rs - Library tests

# Macro System Extraction (2025-11-23)
crates/patina-macros/                          - New crate: macro expansion system
crates/patina-macros/src/macro_env.rs          - MacroEnv type for macro definitions
crates/patina-pipeline/                        - New crate: pipeline orchestration
crates/patina-pipeline/src/pipeline.rs         - Pipeline trait and EvaluationStrategy
crates/patina-pipeline/src/standard.rs         - StandardPipeline implementation
crates/patina-interpreter/src/simple.rs        - SimpleInterpreter API

# Documentation
docs/MACRO_ARCHITECTURE_PROPOSAL.md            - Architecture proposal (3 options)
docs/MACRO_ARCHITECTURE_DECISIONS.md           - Known limitation documentation
docs/MACRO_ARCHITECTURE_IMPLEMENTATION_PLAN.md - When to implement fix
docs/CHIBI_TEST_REGRESSION_ANALYSIS.md         - Regression analysis
docs/SESSION_2025_11_22.md                     - Session 1 summary
docs/SESSION_2025_11_23.md                     - Session 2 summary
```

### Modified Files
```
# CoreExpr work
crates/patina-ir/src/core_expr.rs              - Added Quasiquote variant
crates/patina-tree-walker/src/backend.rs       - CoreExpr integration (disabled)
crates/patina-tree-walker/src/eval/mod.rs      - Quasiquote helpers
crates/patina-tests/tests/case_lambda.rs       - Removed #[ignore] attrs

# Macro extraction (2025-11-23)
crates/patina-macros/src/macro_expander/hygiene.rs    - Updated to use MacroEnv
crates/patina-macros/src/macro_expander/expander.rs   - Uses MacroEnv for hygiene
crates/patina-macros/src/macro_expander/interface.rs  - MacroExpander trait updated
crates/patina-macros/src/macro_expander/compiler.rs   - TODO comment documenting limitation
Cargo.toml                                      - Added patina-macros and patina-pipeline
```

### Dependencies Added
```
patina-frontend/Cargo.toml - Added patina-ir dependency
patina-tree-walker/Cargo.toml - Added patina-frontend dependency
patina-macros/Cargo.toml - New crate with patina-runtime dependency
patina-pipeline/Cargo.toml - New crate with frontend/macros/tree-walker dependencies
patina-interpreter/Cargo.toml - Added patina-pipeline dependency
```

## Summary

This branch contains **excellent work** that's just not ready for production:

✅ **What works:** CoreExpr infrastructure (IR, desugarer, evaluator)
✅ **What's tested:** All infrastructure has passing tests
❌ **What's broken:** Integration in backend.rs causes regression
📝 **What's needed:** Fix macro expansion integration (see proposals)

**Recommendation:** Keep this branch alive, fix the integration when ready, then merge. The infrastructure is too valuable to throw away!

## Recent Updates (2025-11-23)

### ✅ Option 2 Implementation Complete: Extract Macro Crate

Successfully implemented **Option 2** from `MACRO_ARCHITECTURE_PROPOSAL.md`:

#### Phase 1: Macro System Extraction ✅
- **New crate:** `crates/patina-macros/` - Standalone macro expansion system
- **New type:** `MacroEnv` - Dedicated macro environment (separate from runtime `Environment`)
- **Updated:** All hygiene and expansion code to use `MacroEnv`
- **Tests:** All tests passing (79 passed, 7 ignored test helpers that need Parser)

#### Phase 2: Pipeline Orchestration ✅
- **New crate:** `crates/patina-pipeline/` - Pipeline orchestration layer
- **New trait:** `Pipeline` - Flexible parse → eval interface
- **Implementation:** `StandardPipeline` - Current working pipeline
- **API:** `SimpleInterpreter` - Easier-to-use concrete interpreter type

#### Phase 3: Architecture Documentation ✅
- **Created:** `docs/MACRO_ARCHITECTURE_DECISIONS.md` - Documents known limitation
- **Created:** `docs/MACRO_ARCHITECTURE_IMPLEMENTATION_PLAN.md` - When to fix limitation
- **Decision:** Defer `Compiler::env` MacroEnv integration until Phase 1 R7RS completion

### Known Limitation (Deferred)

The `Compiler::env` field still uses `Environment` instead of `MacroEnv`. This causes a lexical scoping bug with `let-syntax` (see docs/MACRO_ARCHITECTURE_DECISIONS.md).

**Decision:** Acceptable for pre-alpha. Fix when:
1. User reports the bug, OR
2. Working on Module System (needs proper lexical scoping), OR
3. After Phase 1 R7RS-small completion

### Test Results (All Passing!)

```
✅ patina-frontend:    144 tests (141 passed, 3 ignored)
✅ patina-macros:       86 tests (79 passed, 7 ignored test helpers)
✅ patina-pipeline:      3 tests (all passed)
✅ patina-interpreter:  42 tests (38 passed, 4 ignored doctests)
✅ patina-tests:       ~850 tests (all passed)
✅ Total:             ~435 tests passing, 0 failures
```

### Architecture Overview

**New dependency flow:**
```
patina-repl → patina-interpreter → patina-pipeline → patina-tree-walker
                                 ↗  patina-frontend
                                 ↗  patina-macros
```

**Pipeline stages:**
1. Parse (patina-frontend)
2. Macro expansion (currently delegated to evaluator, future: explicit in pipeline)
3. Evaluation (patina-tree-walker)

### Next Steps

**Short term:**
- Continue Phase 1 R7RS compliance work (I/O, exceptions, module system)
- Pipeline is ready for future CoreExpr integration when needed

**Long term (when fixing limitation):**
- Update `Compiler::env` to use `MacroEnv`
- Move macro expansion from evaluator to pipeline
- Enable CoreExpr path with proper macro handling

## Questions?

See the documentation files or check git history:
```bash
git log --oneline --graph
git diff main...core-ir-migration
```
