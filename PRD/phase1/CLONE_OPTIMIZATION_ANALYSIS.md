# Clone Optimization Analysis for Tree-Walker

This document analyzes clone() usage in the tree-walker crate and identifies optimization opportunities.

**Status**: Analysis complete, implementation deferred (needs profiling first)

---

## Current State

### Clone Count Summary

| Location | Clone Count |
|----------|-------------|
| `cps_eval/` directory | ~162 |
| `primitives/` directory | ~150 |
| Other eval code | ~66 |
| **Total tree-walker** | **~378** |

### Clone Categories in CPS Evaluator

| Category | Count | Type Being Cloned |
|----------|-------|-------------------|
| `env.clone()` | ~50 | `Rc<Environment>` |
| `global_env.clone()` | ~29 | `Rc<Environment>` |
| `.as_ref().clone()` | ~15 | Contents inside `Rc<T>` |
| `body.clone()` | ~8 | `Rc<CpsExpr>` |
| `dynamic_winds.clone()` | ~4 | `Vec<DynamicWindRecord>` |
| `param.clone()` | ~10 | `Rc<str>` |
| `name.clone()` | ~8 | `String` or `Rc<str>` |

---

## Analysis by Type

### 1. Environment Clones (`Rc<Environment>`) - LOW COST ✅

**Count**: ~79 clones (env + global_env)

**Current Implementation**: `Rc<Environment>` - cloning just increments reference count.

**Assessment**: **Already optimal**. These are cheap Rc clones (pointer + counter increment). No action needed.

### 2. CpsExpr Clones (`Rc<CpsExpr>`) - LOW COST ✅

**Count**: ~15 clones via `.as_ref().clone()` plus body clones

**Current Implementation**: CpsExpr is wrapped in `Rc<CpsExpr>` in most places.

**Problem Pattern**:
```rust
current_expr = body.as_ref().clone();  // Clones the CpsExpr, not the Rc
```

**Assessment**: These clone the inner `CpsExpr` rather than just the `Rc`. The trampoline loop owns the expression, so this is necessary for the current design. Could be optimized by storing `Rc<CpsExpr>` in `StepResult::Continue` instead of owned `CpsExpr`.

### 3. String/Name Clones (`Rc<str>`, `String`) - MEDIUM COST

**Count**: ~18 clones

**Pattern**: Parameter names, continuation names

**Current**: Mix of `String` and `Rc<str>`

**Opportunity**: Ensure all names use `Rc<str>` consistently. Most already do via `ScopedParam`.

### 4. Dynamic Winds (`Vec<DynamicWindRecord>`) - HIGH COST ⚠️

**Count**: ~4 direct clones, but these are expensive

**Current Implementation**:
```rust
pub struct DynamicWindRecord {
    pub id: u64,
    pub before: Value,  // Procedure
    pub after: Value,   // Procedure
}
```

**Problem**: Each `StepResult` variant carries `dynamic_winds: Vec<DynamicWindRecord>`. Cloning vectors is O(n).

**Opportunity**: Wrap in `Rc<Vec<...>>` or use persistent data structure (e.g., `im::Vector`).

### 5. ContValue Clones - MEDIUM-HIGH COST ⚠️

**Current Implementation**:
```rust
pub(super) enum ContValue {
    Local {
        param: Rc<str>,
        body: Rc<CpsExpr>,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,  // ← Expensive to clone
    },
    // ... other variants with Box<ContValue>
}
```

**Problem**: `cont_env: HashMap<Rc<str>, ContValue>` is cloned frequently. HashMap clone is O(n).

**Opportunity**:
1. Wrap `cont_env` in `Rc<RefCell<...>>` for sharing
2. Use persistent map (e.g., `im::HashMap`)

### 6. Exception Handlers (`Vec<ExceptionHandler>`) - MEDIUM COST

**Count**: Carried in every `StepResult` variant

**Opportunity**: Same as dynamic winds - wrap in `Rc` or use persistent structure.

### 7. Prompt Stack (`Vec<PromptFrame>`) - MEDIUM COST

**Count**: Carried in every `StepResult` variant

**Opportunity**: Same as above.

---

## Recommended Optimizations

### Priority 1: Wrap Shared State in Rc (Low Effort, High Impact)

Convert `StepResult` fields from owned to shared:

```rust
// Before
pub(super) enum StepResult {
    Continue {
        expr: CpsExpr,                           // Owned
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,   // Owned, expensive clone
        prompt_stack: Vec<PromptFrame>,          // Owned, expensive clone
        dynamic_winds: Vec<DynamicWindRecord>,   // Owned, expensive clone
        exception_handlers: Vec<ExceptionHandler>, // Owned, expensive clone
    },
    // ...
}

// After
pub(super) enum StepResult {
    Continue {
        expr: Rc<CpsExpr>,                       // Shared
        env: Rc<Environment>,
        cont_env: Rc<ContEnv>,                   // Shared (new type)
        prompt_stack: Rc<Vec<PromptFrame>>,      // Shared
        dynamic_winds: Rc<Vec<DynamicWindRecord>>, // Shared
        exception_handlers: Rc<Vec<ExceptionHandler>>, // Shared
    },
    // ...
}
```

**Caveat**: Need `Rc<RefCell<...>>` if mutation is needed, or use copy-on-write pattern.

### Priority 2: Use Persistent Data Structures (Medium Effort, High Impact)

For frequently modified collections that are also shared:

```toml
# Cargo.toml
[dependencies]
im = "15"  # Immutable/persistent collections
```

```rust
use im::HashMap as PersistentMap;
use im::Vector as PersistentVec;

pub(super) enum ContValue {
    Local {
        param: Rc<str>,
        body: Rc<CpsExpr>,
        env: Rc<Environment>,
        cont_env: PersistentMap<Rc<str>, ContValue>,  // O(log n) clone
    },
}
```

**Benefits**:
- `im::HashMap` clone is O(1) (structural sharing)
- Insert/remove is O(log n) but preserves old version
- Perfect for backtracking (continuations!)

### Priority 3: Avoid Rc Dereference Clones (Low Effort)

Change patterns like:
```rust
current_expr = body.as_ref().clone();  // Clones inner CpsExpr
```

To:
```rust
current_expr = Rc::clone(&body);  // Just clones the Rc
```

This requires changing the trampoline loop to work with `Rc<CpsExpr>` instead of owned `CpsExpr`.

---

## Types Already Optimized ✅

These already use `Rc` and are cheap to clone:

| Type | Current Wrapping | Clone Cost |
|------|------------------|------------|
| `Environment` | `Rc<Environment>` | O(1) |
| `Value` (most variants) | `Rc<RefCell<...>>` for mutable | O(1) |
| `CpsExpr` | `Rc<CpsExpr>` | O(1) |
| Symbol names | `Rc<str>` | O(1) |
| `CompiledMacro` | `Rc<CompiledMacro>` | O(1) |

---

## Implementation Plan

### Phase 1: Profile First
Before optimizing, profile to confirm which clones are hot:
```bash
cargo build --release
perf record ./target/release/patina benchmark.scm
perf report
```

Or use `flamegraph`:
```bash
cargo install flamegraph
cargo flamegraph -- ./target/release/patina benchmark.scm
```

### Phase 2: Quick Wins
1. Change `StepResult` collections to `Rc<Vec<...>>`
2. Store `Rc<CpsExpr>` instead of owned `CpsExpr` in trampoline

### Phase 3: Persistent Data Structures
1. Add `im` crate dependency
2. Convert `cont_env` to `im::HashMap`
3. Convert `dynamic_winds` to `im::Vector`

---

## Estimated Impact

| Optimization | Effort | Expected Speedup |
|--------------|--------|------------------|
| Rc-wrap StepResult fields | 2-3 hours | 5-15% |
| Persistent cont_env | 3-4 hours | 10-20% |
| Avoid .as_ref().clone() | 1-2 hours | 2-5% |

**Total estimated improvement**: 15-35% (needs profiling to confirm)

---

## Decision: Defer Until Profiling

This optimization work is deferred because:

1. **Premature optimization risk**: Without profiling, we might optimize cold paths
2. **GC work may change things**: If we switch to `rust-gc`, the Rc patterns will change
3. **VM backend coming**: Phase 2 will have different performance characteristics

**Recommendation**: Profile first with realistic benchmarks, then apply targeted optimizations.

---

## Related Documents

- `TECH_DEBT_CLEANUP.md` - Item 13 (Reduce Clone() Calls)
- `POST_CPS_TECH_DEBT.md` - Item 7 (Clone Optimization in CPS Evaluator)
- `GC_DESIGN.md` - GC work may affect this analysis
