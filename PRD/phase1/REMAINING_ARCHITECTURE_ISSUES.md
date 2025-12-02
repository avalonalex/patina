# Remaining Architecture Issues

**Date:** 2025-12-01
**Status:** Active (consolidated from previous documents)
**Purpose:** Track architectural improvements that are still relevant

This document consolidates remaining issues from previous architecture reviews:
- `PRD/ARCHIVE/phase1_completed/ARCHITECTURE_CRITIQUE_AND_ROADMAP.md`
- `PRD/ARCHIVE/ARITHMETIC_REFACTORING.md`

---

## Summary of What's Been Completed

| Item | Status | Notes |
|------|--------|-------|
| CoreExpr IR Layer | ✅ COMPLETE | Macros expanded during desugaring, tree-walker uses CoreExpr |
| Value Size Optimization (Ph 1-4) | ✅ COMPLETE | Value: 104→64 bytes, Procedure/Identifier boxed, interning done |
| Hygienic Macro System | ✅ COMPLETE | Scope-set based hygiene working |
| Library System | ✅ COMPLETE | Dual loader architecture |
| TCO (Tail Call Optimization) | ✅ COMPLETE | Trampoline pattern |

---

## Remaining Issues (By Priority)

### High Priority

#### 1. Source Location Tracking
**Status:** Not Started (has detailed plan in `SOURCE_INFO_PLAN.md`)
**Impact:** Error messages don't show file:line:column

**Current:**
```
Error: Undefined variable: foo
```

**Goal:**
```
Error: Undefined variable 'foo'
  --> test.scm:42:10
   |
42 |   (define (bar) (+ foo 1))
   |                    ^^^ undefined variable
```

**Effort:** 1-2 weeks
**Reference:** `PRD/phase1/SOURCE_INFO_PLAN.md`

---

### Medium Priority

#### 2. Arithmetic.rs Refactoring (Technical Debt)
**Status:** Not Started
**Impact:** 2,192 line file with significant duplication

**Problems:**
- Dual type system (`NumericValue` mirrors `Value`)
- Pattern duplication (~126 lines in add/subtract/multiply)
- 450 lines of registration boilerplate
- Scattered special case handling (NaN, infinity, zero)

**Quick Wins (Phase 1, 1-2 days):**
- Extract conversion helpers (`to_f64`, `to_rational`, `to_complex64`)
- Extract special value helpers (`is_nan`, `is_infinite`)
- Consolidate rounding functions using generic helper

**Structural Refactoring (Phase 2, 2-3 days):**
- Split into multiple files (~150-300 lines each)
- Create registration macro (reduce 450→100 lines)

**Estimated Reduction:** 30-40% (2,192 → ~1,300 lines)

**Reference:** `PRD/ARCHIVE/ARITHMETIC_REFACTORING.md`

---

### Low Priority (Future Phases)

#### 3. Value Size - Tagged Pointers (Phase 5)
**Status:** Deferred to VM phase
**Current:** Value is 64 bytes
**Goal:** Value is 8 bytes (for VM performance)

Tagged pointer representation allows:
- Small integers encoded in pointer bits
- 8x memory reduction
- Better cache locality

**When:** Only needed when building bytecode VM
**Reference:** `PRD/phase2/TAGGED_POINTERS.md`

---

#### 4. Environment Representation for VM
**Status:** Deferred to VM phase
**Current:** `HashMap<String, Value>` with parent chain
**Impact:** Fine for tree-walker, too slow for VM

**VM needs:**
- Static environment (compile-time)
- Frame-based locals (O(1) array indexing)
- No HashMap overhead

**When:** Only needed when building bytecode VM
**Reference:** `PRD/ARCHIVE/phase1_completed/ARCHITECTURE_CRITIQUE_AND_ROADMAP.md` (Section 4)

---

#### 5. Bytecode VM
**Status:** Future phase
**Goal:** 10-100x faster than tree-walker

**Design considerations:**
- Register-based (not stack-based)
- Study Chez/Guile IR designs first
- Multi-level IR (HIR → MIR → LIR)

**Reference:**
- `PRD/ARCHIVE/phase1_completed/ARCHITECTURE_CRITIQUE_AND_ROADMAP.md` (Phase 2C)
- `PRD/phase2/VM_SPECIFICATION.md`

---

#### 6. JIT Compiler
**Status:** Far future
**Goal:** Near-native performance

**Options:**
- Cranelift (recommended - Rust-based)
- LLVM (best perf, slow compile)
- Custom backend

**When:** Only if VM benchmarks justify it

---

## Current Performance Baseline

| Metric | Current | Target (VM) | Target (JIT) |
|--------|---------|-------------|--------------|
| Value size | 64 bytes | 8 bytes | 8 bytes |
| CoreExpr size | 120 bytes | N/A (bytecode) | N/A |
| Tree-walker speed | ~1000x slower than C | N/A | N/A |
| VM speed | N/A | 10-100x faster | N/A |
| JIT speed | N/A | N/A | ~C level |

---

## Recommended Order of Work

1. **R7RS Feature Completion** (current focus)
   - Continue improving compliance (currently 60.8% pass, 36.2% crash)
   - Implement missing primitives

2. **Source Location Tracking** (when ready)
   - High user impact
   - Follow plan in `SOURCE_INFO_PLAN.md`

3. **Arithmetic Refactoring** (optional cleanup)
   - Technical debt reduction
   - Not blocking other work

4. **VM/Tagged Pointers** (future phase)
   - Only when performance matters
   - Study Chez/Guile first

---

## References

- `PRD/ARCHIVE/phase1_completed/ARCHITECTURE_CRITIQUE_AND_ROADMAP.md` - Original architecture review
- `PRD/ARCHIVE/ARITHMETIC_REFACTORING.md` - Arithmetic refactoring plan
- `PRD/ARCHIVE/VALUE_SIZE_OPTIMIZATION_PHASES_1_4.md` - Value size work (completed)
- `PRD/phase1/SOURCE_INFO_PLAN.md` - Source location plan
- `PRD/phase2/TAGGED_POINTERS.md` - Phase 5 tagged pointers
- `PRD/phase2/VM_SPECIFICATION.md` - VM design
