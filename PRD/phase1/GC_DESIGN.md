# Garbage Collection Design for Tree-Walker

This document covers the design for adding proper garbage collection to handle circular structures in the tree-walker backend.

**Status**: Deferred from Phase 1 tech debt (TECH_DEBT_CLEANUP.md Item 12)

---

## Problem Statement

Current memory management uses `Rc<RefCell<T>>` which cannot collect cycles:

```scheme
;; This creates a cycle that Rc can't collect - memory leak
(define x (cons 1 2))
(set-cdr! x x)  ;; x now points to itself
```

R7RS allows `set-car!` and `set-cdr!` to create circular structures. Without cycle collection, these leak memory indefinitely.

---

## Impact Analysis

**Affected Types** (all use `Rc<RefCell<T>>`):
- `Value::Pair` - cons cells, most common
- `Value::Vector` - mutable vectors
- `Value::Bytevector` - byte vectors
- `Value::String` - mutable strings
- `Environment` - lexical environments (can have cycles via closures)

**Crates Affected**:
- `patina-core` - Value definition
- `patina-runtime` - Environment, library system
- `patina-tree-walker` - All evaluation code
- `patina-interpreter` - API layer
- `patina-tests` - Test utilities

---

## GC Options Evaluated

| Crate | Type | Handles Cycles | Maturity | Notes |
|-------|------|----------------|----------|-------|
| **gc** (rust-gc) | Mark-and-sweep | Yes | High | Best fit for interpreter |
| **bacon-rajan-cc** | Cycle-collecting RC | Yes | Medium | Drop-in for Rc |
| **shredder** | Concurrent GC | Yes | Medium | Overkill for tree-walker |
| **bumpalo** | Arena (no GC) | No | High | No cycle handling |

---

## Recommended Approach: `rust-gc`

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

---

## Migration Plan

### Phase 1: Add GC Infrastructure
1. Add `gc` dependency to `patina-core`
2. Create `GcValue` type alias or wrapper
3. Derive `Trace` and `Finalize` on Value and related types

### Phase 2: Migrate Heap Types
1. Replace `Rc<RefCell<T>>` with `Gc<GcCell<T>>` for:
   - Pairs
   - Vectors
   - Bytevectors
   - Strings
2. Keep `Rc` for immutable shared data (symbols, compiled macros)

### Phase 3: Environment Migration
1. Migrate `Environment` to use `Gc`
2. Handle closure cycles properly
3. Update all environment creation sites

### Phase 4: Testing
1. Add cycle-creation tests
2. Verify no memory leaks with valgrind/heaptrack
3. Benchmark performance impact

---

## Trade-offs

**Pros:**
- Correct R7RS semantics for circular structures
- Drop-in replacement API (`Gc` works like `Rc`)
- Mature crate (used by Servo)
- Enables `write-shared` to work correctly

**Cons:**
- Requires `#[derive(Trace, Finalize)]` on all GC'd types
- Stop-the-world collection (acceptable for tree-walker)
- Some runtime overhead vs pure Rc
- Touches many files (mechanical but tedious)

---

## Effort Estimate

- **Total**: 3-4 days
- **Risk**: Low (mechanical changes, well-understood pattern)

---

## Future Considerations

The VM backend (Phase 2) may use a different GC strategy (generational, concurrent). This design is specifically for the tree-walker backend where correctness matters more than optimal performance.

---

## Success Criteria

- [ ] Circular pairs don't leak memory
- [ ] `write-shared` correctly detects cycles
- [ ] No performance regression > 20% on benchmarks
- [ ] All existing tests pass

---

## References

- [rust-gc crate](https://crates.io/crates/gc)

---

## Related Documents

- `TECH_DEBT_CLEANUP.md` - Item 12 deferred to this doc
