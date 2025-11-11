# Evaluator Module Refactoring Plan

**Status:** Draft
**Created:** 2025-11-05
**Author:** Research by Claude Code
**Goal:** Break down the monolithic `src/eval/mod.rs` (2,711 lines) into smaller, maintainable modules

---

## Executive Summary

The current `src/eval/mod.rs` has grown to 2,711 lines containing 80+ functions handling everything from core evaluation logic to primitive operations. This document proposes a phased refactoring to improve:

- **Maintainability** - Smaller modules are easier to understand and modify
- **Testability** - Isolated modules can be tested independently
- **Navigability** - Clear organization makes features easier to find
- **Future extensibility** - Cleaner structure for Phase 2+ features (gradual typing, reactive concurrency)

**Comparison:**
- Current: 1 file, 2,711 lines, 80 functions
- chibi-scheme: eval.c is 2,777 lines (similar scale, but C)
- vonuvoli-scheme: 104 Rust files, evaluator.rs is only 2,303 lines with primitives split into 20+ separate files

---

## Current Structure Analysis

### Size Breakdown by Category

| Category | Lines | Functions | % of File | Complexity |
|----------|-------|-----------|-----------|------------|
| Core Evaluation | 69 | 3 | 3% | High |
| Special Forms | 1,033 | 17 | 38% | High |
| Application/Dispatch | 407 | 4 | 15% | Medium |
| Arithmetic Primitives | 516 | 15 | 19% | Medium |
| List Primitives | 386 | 12 | 14% | Medium |
| Higher-Order Functions | 121 | 2 | 4% | High |
| Type Predicates | 147 | 12 | 5% | Low |
| Equality Operations | 107 | 6 | 4% | Medium |
| Multiple Values | 35 | 2 | 1% | Medium |
| Installation | 60 | 1 | 2% | Low |
| **Total** | **2,711** | **80** | **100%** | - |

### Current Organization (Flat)

```
src/eval/
└── mod.rs (2,711 lines)
    ├── EvalError enum (27 lines)
    ├── Evaluator struct + new() (44 lines)
    ├── Core evaluation (eval, eval_in_env, eval_list) (69 lines)
    ├── 17 Special form evaluators (1,033 lines)
    ├── Application logic (apply, check_arity, etc.) (407 lines)
    ├── 51 Primitive implementations (1,431 lines)
    └── install_primitives() (65 lines)
```

---

## Reference Implementation Analysis

### chibi-scheme (R7RS Reference)
- **Structure:** eval.c (2,777 lines), similar monolithic approach
- **Note:** C language, different organization patterns
- **Takeaway:** Even reference implementations have large eval modules, but we can do better in Rust

### vonuvoli-scheme (Modern Rust)
- **Structure:** 104+ source files, highly modular
- **Key organization:**
  - `evaluator.rs` - Core evaluator (2,303 lines)
  - `expressions.rs` - Expression enum (374 lines)
  - `builtins_*.rs` - 20+ files for different primitive categories
    - `builtins_arithmetic.rs`, `builtins_lists.rs`, `builtins_strings.rs`, etc.
  - `primitives_*.rs` - 20+ files for primitive dispatchers
    - `primitives_arithmetic.rs`, `primitives_lists.rs`, etc.
- **Takeaway:** Aggressive modularization works well for large projects

### Best Practices from Research
1. **Separation of concerns** - Special forms, primitives, and evaluation logic should be separate
2. **Functional grouping** - Group related operations (arithmetic, lists, I/O)
3. **Clear module hierarchy** - Use `mod.rs` for re-exports, implementation in submodules
4. **Trait-based design** - vonuvoli uses `evaluator_trait.rs` for abstraction

---

## Proposed Refactoring Options

### Option A: Aggressive Split (Recommended for Long-term)

**Target structure:**
```
src/eval/
├── mod.rs                      (~150 lines) - Public API, re-exports
├── error.rs                    (~30 lines)  - EvalError enum
├── evaluator.rs                (~100 lines) - Evaluator struct, eval(), eval_in_env()
├── dispatcher.rs               (~80 lines)  - eval_list() with special form routing
├── application.rs              (~150 lines) - apply(), eval_arguments(), check_arity()
│
├── special_forms/
│   ├── mod.rs                  (~50 lines)  - Re-exports
│   ├── core.rs                 (~250 lines) - quote, if, define, set!, lambda
│   ├── bindings.rs             (~450 lines) - let, let*, letrec, letrec*, let-values
│   ├── conditionals.rs         (~200 lines) - cond, case
│   ├── boolean.rs              (~100 lines) - and, or
│   └── apply.rs                (~60 lines)  - apply special form
│
└── primitives/
    ├── mod.rs                  (~150 lines) - apply_primitive(), install_primitives()
    ├── arithmetic.rs           (~550 lines) - +, -, *, /, =, <, >, abs, max, min, etc.
    ├── pairs.rs                (~60 lines)  - cons, car, cdr
    ├── lists.rs                (~400 lines) - length, append, reverse, list-ref, etc.
    ├── predicates.rs           (~150 lines) - Type predicates (12 functions)
    ├── equality.rs             (~110 lines) - eq?, eqv?, equal? with helpers
    ├── higher_order.rs         (~130 lines) - map, for-each
    └── values.rs               (~40 lines)  - values, call-with-values
```

**Files:** 18 modules (~150 lines average)
**Pros:**
- Maximum maintainability
- Easy to navigate and extend
- Clear separation of concerns
- Easy to add new primitive categories (strings, vectors, I/O)
- Follows vonuvoli-scheme pattern

**Cons:**
- More files to manage
- More complex import structure
- Larger refactoring effort

---

### Option B: Moderate Split (Recommended for Phase 1)

**Target structure:**
```
src/eval/
├── mod.rs                      (~150 lines) - Public API, Evaluator, core eval
├── error.rs                    (~30 lines)  - EvalError enum
├── special_forms.rs            (~1,100 lines) - All special forms + helpers
├── application.rs              (~150 lines) - apply(), eval_arguments(), check_arity()
│
└── primitives/
    ├── mod.rs                  (~150 lines) - apply_primitive(), install_primitives()
    ├── arithmetic.rs           (~550 lines) - All arithmetic operations
    ├── lists.rs                (~550 lines) - Pairs, lists, higher-order functions
    └── core.rs                 (~300 lines) - Predicates, equality, values
```

**Files:** 8 modules (~340 lines average)
**Pros:**
- Balanced complexity reduction
- Smaller refactoring effort
- Still provides clear organization
- Easy to further split later

**Cons:**
- `special_forms.rs` still quite large (1,100 lines)
- Less granular than Option A

---

### Option C: Conservative Split (Quick Win)

**Target structure:**
```
src/eval/
├── mod.rs                      (~250 lines) - Evaluator, core eval, error
├── special_forms.rs            (~1,100 lines) - All special forms
├── application.rs              (~150 lines) - Application logic
└── primitives.rs               (~1,300 lines) - All primitives + dispatcher
```

**Files:** 4 modules (~675 lines average)
**Pros:**
- Minimal disruption
- Quick to implement (1-2 hours)
- Still reduces complexity significantly

**Cons:**
- Files still quite large
- Limited benefit for long-term maintenance
- Will likely need further splitting

---

## Recommended Approach: Phased Refactoring

### Phase 1: Quick Win (Week 1) - **Start Here**
Implement **Option C** to get immediate benefits:

1. **Extract `error.rs`** (~30 lines)
   - Move `EvalError` enum
   - Update imports in `mod.rs`
   - **Risk:** Very low
   - **Benefit:** Clean separation, easy to extend errors

2. **Extract `special_forms.rs`** (~1,100 lines)
   - Move all `eval_*` methods for special forms
   - Keep dispatcher (`eval_list`) in `mod.rs`
   - **Risk:** Low (self-contained functions)
   - **Benefit:** Immediate 40% size reduction

3. **Extract `application.rs`** (~150 lines)
   - Move `apply()`, `eval_arguments()`, `check_arity()`
   - **Risk:** Low
   - **Benefit:** Clear separation of concerns

4. **Extract `primitives.rs`** (~1,300 lines)
   - Move `apply_primitive()`, `install_primitives()`
   - Move all `primitive_*` functions
   - **Risk:** Low (self-contained)
   - **Benefit:** Immediate 48% size reduction

**Result:** `mod.rs` reduced from 2,711 → ~250 lines (91% reduction!)

---

### Phase 2: Primitive Split (Week 2-3)
Further split `primitives.rs` following **Option B**:

1. Create `primitives/` directory
2. Split into:
   - `mod.rs` - Dispatcher and installation
   - `arithmetic.rs` - All numeric operations
   - `lists.rs` - Lists, pairs, higher-order
   - `core.rs` - Predicates, equality, values

**Result:** Largest file ~550 lines (manageable)

---

### Phase 3: Special Forms Split (Week 4+)
Further split `special_forms.rs` following **Option A**:

1. Create `special_forms/` directory
2. Split into:
   - `core.rs` - quote, if, define, set!, lambda
   - `bindings.rs` - let variants
   - `conditionals.rs` - cond, case
   - `boolean.rs` - and, or

**Result:** Full modular structure achieved

---

## Implementation Guidelines

### Module Visibility Rules

1. **Public API** (in `mod.rs`):
   ```rust
   pub struct Evaluator { ... }
   pub fn eval(&self, expr: &Value) -> Result<Value, EvalError>
   ```

2. **Internal methods** (in submodules):
   ```rust
   // Make visible to other eval modules
   pub(super) fn eval_if(...)
   pub(super) fn primitive_add(...)

   // Keep private to module
   fn collect_list_items(...)
   ```

3. **Re-exports** (in `mod.rs`):
   ```rust
   mod error;
   pub use error::EvalError;

   mod special_forms;
   mod primitives;
   mod application;
   ```

### Dependency Management

**Core dependencies:**
- `special_forms` → `mod.rs::eval_in_env()` (via `&self`)
- `primitives` → `application::apply()` (for higher-order functions)
- `application` → All primitive implementations

**Approach:**
- Keep `Evaluator` and `eval_in_env` in `mod.rs`
- Pass `&self` to all submodule functions that need recursion
- Use `pub(super)` for cross-module visibility within `eval/`

### Testing Strategy

After each phase:
1. **Run full test suite:**
   ```bash
   cargo test
   cargo test --test compliance
   cargo test --test integration
   ```

2. **Verify no behavior changes:**
   - All existing tests should pass
   - No new compiler warnings
   - `cargo clippy` should be clean

3. **Update documentation:**
   - Update `CLAUDE.md` with new structure
   - Add module-level documentation (`//!`) to each new file

---

## Migration Checklist

### Phase 1 - Extract error.rs
- [ ] Create `src/eval/error.rs`
- [ ] Move `EvalError` enum
- [ ] Add module docs
- [ ] Update `mod.rs` imports: `pub use error::EvalError;`
- [ ] Run tests: `cargo test`
- [ ] Run clippy: `cargo clippy`
- [ ] Commit: "refactor(eval): extract error types"

### Phase 1 - Extract special_forms.rs
- [ ] Create `src/eval/special_forms.rs`
- [ ] Move all `eval_*` special form methods (17 functions)
- [ ] Move helpers: `collect_list_items`, `parse_lambda_params`, `bind_values_to_formals`
- [ ] Keep in `mod.rs`: `eval_list()` dispatcher
- [ ] Add `pub(super)` visibility
- [ ] Add module docs
- [ ] Update imports in `mod.rs`
- [ ] Run tests
- [ ] Commit: "refactor(eval): extract special forms"

### Phase 1 - Extract application.rs
- [ ] Create `src/eval/application.rs`
- [ ] Move `apply()`, `eval_arguments()`, `check_arity()`
- [ ] Move helper: `list_from_vec()`
- [ ] Add `pub(super)` visibility
- [ ] Add module docs
- [ ] Update imports
- [ ] Run tests
- [ ] Commit: "refactor(eval): extract application logic"

### Phase 1 - Extract primitives.rs
- [ ] Create `src/eval/primitives.rs`
- [ ] Move `apply_primitive()` (52 lines)
- [ ] Move `install_primitives()` (60 lines)
- [ ] Move all 51 `primitive_*` functions
- [ ] Move equality helpers: `values_eq`, `values_eqv`, `values_equal`
- [ ] Add `pub(super)` visibility
- [ ] Add module docs
- [ ] Update imports
- [ ] Run tests
- [ ] Run full test suite with `cargo test --all`
- [ ] Commit: "refactor(eval): extract primitives"

### Phase 1 - Final Verification
- [ ] Run `cargo build --release`
- [ ] Run `cargo test --all`
- [ ] Run `cargo clippy --all-targets`
- [ ] Run `cargo fmt --check`
- [ ] Update `CLAUDE.md` with new structure
- [ ] Update this PRD status to "Completed - Phase 1"

---

## Future Considerations

### Phase 2+ Extensions

When implementing gradual typing (Phase 2):
- Add `src/eval/type_checker.rs`
- Extend `special_forms/` with `typed_define.rs`

When implementing reactive concurrency (Phase 3):
- Add `src/eval/reactive/` directory
- New primitives in `primitives/streams.rs`

When implementing logic programming (Phase 4):
- Add `src/eval/logic/` directory
- Unification in separate module

### Performance Considerations

- **No expected performance impact** - This is pure refactoring
- Function calls remain the same (methods → functions with `&self`)
- Inlining should work the same with `#[inline]` hints if needed
- Benchmark after Phase 1 to verify: `cargo bench` (if benchmarks exist)

### Documentation

Add module-level docs to each new file:

```rust
//! Special forms evaluation
//!
//! This module implements evaluation for Scheme special forms including:
//! - `quote` - Quote expressions
//! - `if` - Conditional evaluation
//! - `define` - Variable and function definition
//! - `set!` - Assignment
//! - `lambda` - Procedure creation
//! - `let` variants - Binding forms
//! - `cond` and `case` - Multi-way conditionals
//! - `and` and `or` - Boolean operators
```

---

## Success Metrics

### Phase 1 Success Criteria
- ✅ `mod.rs` reduced to < 300 lines (from 2,711)
- ✅ All tests pass (`cargo test`)
- ✅ No clippy warnings
- ✅ No performance regression
- ✅ Clear module boundaries established

### Phase 2 Success Criteria
- ✅ No file > 600 lines
- ✅ Primitives organized by category
- ✅ Easy to add new primitive categories

### Phase 3 Success Criteria
- ✅ No file > 500 lines
- ✅ Full modular structure
- ✅ Clear extension points for Phase 2+ features

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Breaking existing tests | High | Low | Run tests after each file extraction |
| Import complexity | Medium | Medium | Use `pub(super)` and clear re-exports |
| Merge conflicts | Medium | Medium | Do Phase 1 in one PR/session |
| Performance regression | Low | Very Low | Benchmark before/after |
| Over-modularization | Low | Low | Follow Option B/C initially, expand as needed |

---

## Alternatives Considered

### Alternative 1: Keep as monolithic file
- **Pros:** No refactoring effort, no import complexity
- **Cons:** Already difficult to maintain at 2,711 lines, will get worse with Phase 2+ features
- **Decision:** Rejected - Technical debt will compound

### Alternative 2: Split by syntax (special forms vs primitives only)
- **Pros:** Simple two-way split
- **Cons:** Primitives still too large (1,300 lines)
- **Decision:** Rejected - Doesn't go far enough

### Alternative 3: vonuvoli-style extreme modularity (20+ files)
- **Pros:** Maximum modularity
- **Cons:** Overkill for current project size
- **Decision:** Deferred to Phase 3+

---

## Conclusion

The current 2,711-line `src/eval/mod.rs` should be refactored in a phased approach:

1. **Phase 1 (Immediate):** Split into 4 files (Conservative - Option C)
   - Quick win, 91% size reduction in `mod.rs`
   - Low risk, high benefit
   - **Estimated effort:** 2-4 hours

2. **Phase 2 (Next 1-2 weeks):** Split primitives into 4 modules
   - Improved organization
   - Easier to extend
   - **Estimated effort:** 2-3 hours

3. **Phase 3 (Future):** Split special forms into 5 modules
   - Full modular structure
   - Ready for Phase 2+ features
   - **Estimated effort:** 2-3 hours

**Total effort:** ~8-10 hours spread over 3 phases

**Recommendation:** Start with Phase 1 immediately to get quick benefits and establish the pattern for future refactoring.

---

## Appendix: Detailed Function Inventory

### Special Forms (17 functions, 1,033 lines)

| Function | Lines | Category | Complexity |
|----------|-------|----------|------------|
| `eval_quote()` | 11 | Core | Low |
| `eval_if()` | 31 | Core | Medium |
| `eval_define()` | 68 | Core | High |
| `eval_set()` | 23 | Core | Low |
| `eval_lambda()` | 30 | Core | Medium |
| `parse_lambda_params()` | 49 | Core | High |
| `eval_begin()` | 13 | Sequencing | Low |
| `eval_let()` | 68 | Bindings | High |
| `eval_let_star()` | 60 | Bindings | High |
| `eval_letrec()` | 69 | Bindings | High |
| `eval_letrec_star()` | 68 | Bindings | High |
| `eval_let_values()` | 57 | Bindings | High |
| `eval_let_star_values()` | 69 | Bindings | High |
| `bind_values_to_formals()` | 106 | Bindings | High |
| `eval_cond()` | 70 | Conditionals | Medium |
| `eval_case()` | 97 | Conditionals | High |
| `collect_list_items()` | 19 | Helper | Low |
| `eval_and()` | 46 | Boolean | Medium |
| `eval_or()` | 46 | Boolean | Medium |
| `eval_apply()` | 58 | Application | Medium |

### Primitives (51 functions, 1,431 lines)

**Arithmetic (15):** add, subtract, multiply, divide, numeric_equal, less_than, greater_than, less_equal, greater_equal, quotient, remainder, modulo, abs, max, min

**Lists (12):** cons, car, cdr, list_p, length, append, reverse, list_ref, list_tail, memq, memv, member, assq, assv, assoc

**Predicates (12):** null_p, pair_p, number_p, integer_p, boolean_p, string_p, symbol_p, exact_p, inexact_p, (plus eq/eqv/equal covered in equality)

**Equality (6):** eq, eqv, equal, values_eq, values_eqv, values_equal

**Higher-Order (2):** map, for_each

**Multiple Values (2):** values, call_with_values

---

**Status:** Ready for implementation
**Next Steps:** Begin Phase 1 with error.rs extraction
