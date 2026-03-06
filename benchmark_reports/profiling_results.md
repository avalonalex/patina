# Patina Profiling Results

**Generated:** 2024-12-28
**Benchmark:** `fib(25)`
**Tool:** cargo-flamegraph (Instruments on macOS)

## Executive Summary

**The CPS evaluator's HashMap cloning/dropping accounts for ~37% of execution time.**

This confirms the predictions in `CLONE_OPTIMIZATION_ANALYSIS.md`. The fix is straightforward:
wrap `cont_env` in `Rc<...>` to avoid deep cloning.

## Top Hotspots

| Function | Samples | % of Time | Category |
|----------|---------|-----------|----------|
| `<RawTable as Drop>::drop` | 856 | 21.0% | HashMap cleanup |
| `<ContValue as Clone>::clone` | 667 | 16.3% | Continuation cloning |
| `<RawTable as Clone>::clone` | 653 | 16.0% | HashMap cloning |
| `PrimitiveRegistry::apply` | 787 | 19.3% | Primitive dispatch |
| `lookup_var` | 180 | 4.4% | Variable lookup |
| `less_than` | 37 | 0.9% | Actual comparison |
| `add` | 2 | 0.05% | Actual addition |

## Analysis

### Problem: HashMap Clone/Drop Overhead

The CPS evaluator passes `cont_env: HashMap<Rc<str>, ContValue>` through `StepResult`:

```rust
pub enum StepResult {
    Continue {
        cont_env: HashMap<Rc<str>, ContValue>,  // Cloned every step!
        // ...
    },
    ApplyProc {
        cont_env: HashMap<Rc<str>, ContValue>,  // Cloned every step!
        // ...
    },
    // ...
}
```

Each evaluation step:
1. **Clones** the HashMap when creating StepResult (~16%)
2. **Drops** the old HashMap (~21%)

For `fib(25)` with ~240K recursive calls, this means ~480K HashMap clone+drop operations.

### Breakdown by Category

| Category | % of Time | Description |
|----------|-----------|-------------|
| **HashMap operations** | 37% | Clone + Drop overhead |
| **CPS machinery** | 42% | eval_one_step dispatch |
| **Continuation handling** | 23% | invoke_continuation_step |
| **Primitive dispatch** | 19% | Registry lookup + call |
| **Actual work** | <2% | Arithmetic, comparison |

## Recommended Fixes

### Priority 1: Rc-wrap cont_env (Expected: 30-40% improvement)

```rust
// Before
pub struct StepResult {
    cont_env: HashMap<Rc<str>, ContValue>,
}

// After
pub struct StepResult {
    cont_env: Rc<HashMap<Rc<str>, ContValue>>,
}
```

This changes O(n) clone to O(1) Rc increment.

### Priority 2: Consider persistent data structure

Use `im::HashMap` for O(log n) structural sharing:

```rust
use im::HashMap;

pub struct StepResult {
    cont_env: im::HashMap<Rc<str>, ContValue>,
}
```

### Priority 3: Avoid ContValue cloning

ContValue contains:
```rust
pub enum ContValue {
    Local {
        cont_env: HashMap<Rc<str>, ContValue>,  // Recursive!
        // ...
    },
    // ...
}
```

The nested `cont_env` causes cascading clones. Rc-wrapping fixes this too.

## Validation Plan

1. Implement Rc-wrap for cont_env
2. Re-run `fib(25)` benchmark
3. Expected: HashMap clone/drop should drop from 37% to <5%
4. Re-profile to find next bottleneck

## Flamegraph

See `/tmp/patina_flamegraph.svg` for the full interactive flamegraph.

To regenerate:
```bash
CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --bin patina -o flamegraph.svg -- /tmp/profile_fib.scm
```

## Update: Optimization Implemented (2024-12-28)

### Changes Made

Wrapped `cont_env: HashMap<Rc<str>, ContValue>` in `Rc<...>` across all CPS evaluator files:

1. **types.rs**: Changed `ContValue::Local` and all `StepResult` variants
2. **step.rs**: Updated `eval_one_step` signature and LetCont handling
3. **continuation.rs**: Updated all continuation functions
4. **application.rs**: Updated `apply_cps_step` and all helper methods
5. **mod.rs**: Updated eval loop initialization
6. **wind.rs**: Updated `force_promise_cps` and `apply_from_direct`
7. **exceptions.rs**: Updated `maybe_route_error_through_cps`
8. **environment.rs**: Updated `eval_trivial`

### Results

**Benchmark: `fib(25)`**

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| Execution time | ~4.9s | ~1.63s | **66.5%** |
| HashMap clone/drop | 37% of time | <5% | Eliminated |

This exceeds the predicted 30-40% improvement because:
1. HashMap clone/drop was O(n) per step, now O(1) Rc increment
2. ContValue::Local also contained HashMap, causing cascading clones
3. Deep recursion (240K calls) amplified the O(n) vs O(1) difference

### All Tests Passing

- `cargo test` - All ~1400 tests pass
- Correctness verified

## Post-Optimization Profiling (2024-12-28)

After the Rc optimization, the new bottlenecks are:

### Top Functions by Sample Count

| Function | Samples | % of Time | Category |
|----------|---------|-----------|----------|
| `::apply` | 983 | 15.9% | Application dispatch |
| `PrimitiveRegistry::apply` | 856 | 13.8% | Primitive dispatch |
| `::eval_one_step` | 617 | 10.0% | CPS evaluation |
| `core::slice::memchr::memchr_aligned` | 514 | 8.3% | String/symbol ops |
| `Environment::get` | 424 | 6.9% | Variable lookup |
| `::drop_slow` | 338 | 5.5% | Rc drop handling |
| `::lookup_var` | 218 | 3.5% | Variable lookup |
| `core::hash::BuildHasher::hash_one` | 152 | 2.5% | Hashing |
| `_xzm_free` | 139 | 2.2% | Memory deallocation |
| `::clone` | 135 | 2.2% | Cloning |

### Breakdown by Category

| Category | % of Time | Description |
|----------|-----------|-------------|
| **Primitive dispatch** | 14% | PrimitiveRegistry lookup + call |
| **CPS machinery** | 10% | eval_one_step dispatch |
| **Environment lookup** | 10.5% | get + lookup_var |
| **String/hash ops** | 10.8% | memchr_aligned + hash_one |
| **Memory management** | 10% | drop_slow + free + clone |
| **Actual work** | <5% | Arithmetic, comparison |

### Key Observations

1. **HashMap clone/drop eliminated** - No longer in top hotspots
2. **Primitive dispatch is now dominant** - 14% in registry lookup
3. **Environment lookup significant** - ~10% of time
4. **Memory still matters** - drop_slow at 5.5%

### Recommended Next Optimizations

#### Priority 1: Optimize Primitive Dispatch (Expected: 5-10% improvement)

The `PrimitiveRegistry::apply` function does a HashMap lookup for every primitive call. For `fib(25)` with 240K calls, that's 240K+ hashmap lookups.

Options:
1. **Inline common primitives** - Special-case +, -, <, etc. in CPS evaluator
2. **Use array-based dispatch** - Index primitives by a numeric ID
3. **Cache primitive lookups** - Store primitives directly in the environment

#### Priority 2: Optimize Environment Lookup (Expected: 3-5% improvement)

Variable lookup is ~10% of time. Options:
1. **Use de Bruijn indices** - Convert variable names to indices at compile time
2. **Flatten environment chains** - Reduce parent-chain traversal
3. **Use array-based environments** - For fixed-size scopes

#### Priority 3: Tagged Pointers (Expected: 10-20% improvement)

The `Value` enum is still 64 bytes. Tagged pointers would:
1. Reduce allocation pressure (immediate fixnums, chars, bools)
2. Improve cache utilization (8 bytes vs 64 bytes)
3. Eliminate Rc overhead for common values

See `PRD/phase1/TAGGED_POINTER_DESIGN.md` for detailed design.

## Inline Primitives Optimization (2024-12-28)

### Problem

After the Rc optimization, `PrimitiveRegistry::apply` was 14% of execution time. Each call to `+`, `-`, `<` etc. went through:
1. Environment lookup for the procedure
2. HashMap lookup in primitive registry
3. Function dispatch through `evaluator.apply()`

### Solution

Inlined hot primitives directly in `apply_cps_step` when `Procedure::Primitive` is matched:
- `+`, `-`, `*`, `/` - call `Value::numeric_add/sub/mul/div` directly
- `<`, `>`, `=`, `<=`, `>=` - call `Value::numeric_lt/gt/eq/le/ge` directly
- Proper error routing through CPS exception handlers

### Results

**Benchmark: `fib(25)`**

| Stage | Time | Improvement from Previous |
|-------|------|--------------------------|
| Original | ~4.9s | - |
| After Rc optimization | ~1.63s | 66.5% |
| After inline primitives | ~0.92s | **43.6%** |
| **Total improvement** | - | **81.2%** |

### Code Changes

Modified `apply_cps_step` in `application.rs` to handle hot primitives inline:
- Added `apply_inline_primitive` helper for error routing
- Added `inline_add`, `inline_sub`, `inline_mul`, `inline_div` helpers
- Added `inline_comparison` helper for `<`, `>`, `=`, `<=`, `>=`

## Next Steps

1. [x] Implement `Rc<HashMap>` for `cont_env` in `StepResult`
2. [x] Benchmark improvement (66.5% improvement achieved!)
3. [x] Profile again to find next bottleneck
4. [x] Inline common primitives (+, -, <) in CPS evaluator (43.6% additional improvement!)
5. [ ] Consider tagged pointers for Value (major refactor)
6. [ ] Profile again to find next bottleneck
