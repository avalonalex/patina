# Tagged Pointers Value Representation

**Status:** Design Phase - Requires VM Architecture First
**Priority:** High (when VM work begins)
**Related:** [VM_SPECIFICATION.md](./VM_SPECIFICATION.md)

## Overview

This document describes the design for reducing `Value` from ~32 bytes to 8 bytes using tagged pointers. This optimization is **critical for VM performance** but should only be implemented after the VM architecture is finalized.

> **Prerequisites (from Phases 1-4, now complete):**
> - [x] Box large variants (Procedure, Identifier)
> - [x] ScopeSet optimization with SmallVec
> - [x] Rc<CoreExpr> for efficient tail calls
> - [x] Value interning (symbols + small integers)
>
> See `PRD/ARCHIVE/VALUE_SIZE_OPTIMIZATION_PHASES_1_4.md` for completed work.

---

## Why Tagged Pointers Are Critical for patina-vm

VMs benefit **even more** than tree-walkers from tagged pointers:

| Aspect | Tree-Walker | VM (Bytecode) | Impact |
|--------|-------------|---------------|--------|
| Type dispatch frequency | Per expression | Per instruction (tight loop) | VM: 10-100x more |
| Stack operations | Function calls | Every instruction | VM: push/pop in hot loop |
| Cache pressure | Moderate | Extreme (stack + bytecode) | VM: must fit in L1 |
| Type check cost | Tolerable | Dominates runtime | VM: must be 1 instruction |

**VM Bytecode Loop (Hot Path):**
```rust
// This loop runs millions of times per second
loop {
    match bytecode[pc] {
        Op::Add => {
            let b = stack.pop();  // With 104-byte Value: cache miss
            let a = stack.pop();  // With 8-byte Value: stays in register

            // Type check - with tagged pointers: ONE bit test
            // Current Value: branch on discriminant, then match variant
            if a.is_fixnum() && b.is_fixnum() {
                stack.push(Value::fixnum(a.as_fixnum() + b.as_fixnum()));
            } else {
                // Slow path for bignums, etc.
            }
            pc += 1;
        }
        // ... hundreds more opcodes
    }
}
```

---

## Architecture Decision: Shared Value vs Backend-Specific

**Recommendation: Shared Tagged Value in patina-core**

```
patina-core
├── Value(u64)        ← Tagged pointer, used by ALL backends
├── CoreExpr
└── Environment

patina-tree-walker → uses Value (benefits from tagging)
patina-vm          → uses Value (optimal for bytecode)
patina-jit         → uses Value (can unbox in compiled code)
```

**Pros:**
- Single representation across codebase
- No conversion overhead at boundaries
- Tree-walker gets "free" performance boost
- Simpler mental model

**Cons:**
- Tagging scheme must satisfy all backends
- Can't optimize per-backend

**Decision:** Use shared Value. Patina is an educational project; simplicity and a unified model outweigh marginal per-backend gains.

---

## Tagging Scheme Design (VM-Ready)

Design the tagging scheme to support efficient VM operations:

```rust
#[repr(transparent)]
pub struct Value(u64);

// Tagging scheme optimized for VM dispatch
impl Value {
    // Low 3 bits for primary type tag (8 types immediate)
    const TAG_BITS: u64 = 3;
    const TAG_MASK: u64 = 0b111;

    // Immediate types (no heap allocation)
    const TAG_FIXNUM: u64   = 0b000;  // 61-bit signed integer
    const TAG_PAIR: u64     = 0b001;  // Pointer to cons cell
    const TAG_SYMBOL: u64   = 0b010;  // Pointer to interned symbol
    const TAG_OBJECT: u64   = 0b011;  // Pointer to heap object (with sub-tag)
    const TAG_SPECIAL: u64  = 0b100;  // #t, #f, (), eof, unspecified
    const TAG_CHAR: u64     = 0b101;  // Unicode codepoint (immediate)
    const TAG_FLOAT: u64    = 0b110;  // IEEE 754 double (NaN-boxed)
    const TAG_RESERVED: u64 = 0b111;  // Future: continuation, etc.

    // Fast type checks (single instruction)
    #[inline(always)]
    pub fn is_fixnum(&self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_FIXNUM
    }

    #[inline(always)]
    pub fn is_pair(&self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_PAIR
    }

    // Immediate extraction (no memory access)
    #[inline(always)]
    pub fn as_fixnum_unchecked(&self) -> i64 {
        (self.0 as i64) >> Self::TAG_BITS  // Arithmetic shift preserves sign
    }

    // Construction
    #[inline(always)]
    pub fn fixnum(n: i64) -> Self {
        debug_assert!(n >= -(1 << 60) && n < (1 << 60), "Fixnum overflow");
        Self(((n as u64) << Self::TAG_BITS) | Self::TAG_FIXNUM)
    }

    // Special values (all fit in 64 bits)
    pub const TRUE: Self = Self(0x10 | Self::TAG_SPECIAL);
    pub const FALSE: Self = Self(0x00 | Self::TAG_SPECIAL);
    pub const NULL: Self = Self(0x20 | Self::TAG_SPECIAL);
    pub const EOF: Self = Self(0x30 | Self::TAG_SPECIAL);
    pub const UNSPECIFIED: Self = Self(0x40 | Self::TAG_SPECIAL);
}

// Heap objects use a header word for sub-typing
#[repr(C)]
struct HeapObject {
    header: u64,  // Contains sub-type tag, GC bits, size
    // ... payload follows
}
```

---

## VM-Specific Considerations

### 1. Stack Layout Compatibility
```rust
// VM stack is just Vec<Value> - each slot is 8 bytes
struct VmStack {
    slots: Vec<Value>,  // 8 bytes per slot, cache-friendly
    sp: usize,
}
```

### 2. Bytecode Operand Encoding
```rust
// Bytecode can embed small constants directly
enum Bytecode {
    LoadImmediate(Value),  // 8-byte immediate fits in instruction
    LoadConst(u16),        // Index into constant pool for larger values
    Add, Sub, Mul, Div,    // Operate on stack
    // ...
}
```

### 3. GC Integration Points
```rust
impl Value {
    // GC needs to identify pointers
    #[inline(always)]
    pub fn is_heap_pointer(&self) -> bool {
        matches!(
            self.0 & Self::TAG_MASK,
            Self::TAG_PAIR | Self::TAG_SYMBOL | Self::TAG_OBJECT
        )
    }

    // Extract raw pointer for GC traversal
    #[inline(always)]
    pub unsafe fn as_ptr(&self) -> *const HeapObject {
        debug_assert!(self.is_heap_pointer());
        (self.0 & !Self::TAG_MASK) as *const HeapObject
    }
}
```

### 4. Platform Considerations
- Assumes 64-bit platform (48-bit address space on x86_64, ARM64)
- Pointer tagging uses low bits (requires aligned allocations)
- Alternative: NaN-boxing for better float performance (trade-off: complex encoding)

---

## Migration Path

1. **Create `patina-core::tagged` module** with new Value type
2. **Implement conversion traits** between old and new Value
3. **Migrate patina-tree-walker** first (simpler, validates correctness)
4. **Build patina-vm** using tagged Value from start
5. **Remove old Value** once migration complete

---

## Expected Results

| Metric | Current (after Phase 4) | After Tagged Pointers |
|--------|-------------------------|----------------------|
| `size_of::<Value>()` | ~32 bytes | 8 bytes |
| Cons cell total | ~80 bytes | 24 bytes |
| Type check | Branch + match | Single AND + compare |
| Stack slot | 32 bytes | 8 bytes |
| Fixnum arithmetic | Box/unbox overhead | Direct CPU ops |
| VM dispatch overhead | High | Minimal |

**Performance Targets:**
- `fibonacci(35)` tree-walker: 10x faster than Phase 4
- `fibonacci(35)` VM: 100x faster than current tree-walker
- Memory usage: 4x reduction for typical programs

---

## Effort and Risk

**Effort:** 2-3 weeks (longer if GC integration needed)

**Risk:** High
- Breaking change to fundamental type
- Platform-specific behavior (pointer width, alignment)
- Must maintain correctness for all 22 Value variants
- Requires extensive test coverage

**Mitigations:**
- Comprehensive property-based tests (quickcheck)
- Parallel implementation (don't remove old Value until verified)
- CI testing on multiple platforms
- Benchmark suite to catch performance regressions

---

## Entry Criteria

This work should **NOT** begin until:
- [ ] `patina-vm` bytecode format finalized (see [VM_SPECIFICATION.md](./VM_SPECIFICATION.md))
- [ ] `patina-vm` stack layout determined
- [ ] Performance baseline established (tree-walker with Phases 1-4)
- [ ] Tagged pointer tagging scheme reviewed and approved
