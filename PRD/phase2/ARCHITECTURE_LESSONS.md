# Architecture Lessons from Language Implementation Comparison

**Status:** Reference Document
**Created:** 2025-12-13
**Purpose:** Capture architectural insights from comparing Patina to other language implementations

---

## Overview

This document captures architectural lessons learned from comparing Patina's design against other small-to-medium language implementations (Lua, Chibi Scheme, Guile, V8, Chez Scheme, CPython, PyPy). These insights inform Phase 2 VM design decisions.

---

## Table of Contents

1. [Value Representation](#1-value-representation)
2. [Environment and Closures](#2-environment-and-closures)
3. [Memory Management](#3-memory-management)
4. [String and Symbol Handling](#4-string-and-symbol-handling)
5. [Dispatch and Primitives](#5-dispatch-and-primitives)
6. [Summary of Recommendations](#6-summary-of-recommendations)

---

## 1. Value Representation

### Current Patina Design

```rust
// ~64 bytes enum with 26 variants
pub enum Value {
    Integer(i64),
    BigInteger(Rc<BigInt>),
    Rational(Rc<Ratio<BigInt>>),
    Real(f64),
    Complex(Box<(Value, Value)>),
    Pair(Rc<RefCell<(Value, Value)>>),
    // ... 20+ more variants
}
```

**Characteristics:**
- Large enum size (~64 bytes due to largest variants)
- All heap types use `Rc<RefCell<T>>` for interior mutability
- Simple and type-safe, but not cache-friendly

### What Other Implementations Do

| Implementation | Technique | Value Size | Notes |
|----------------|-----------|------------|-------|
| **Lua 5.4** | Tagged union | 16 bytes | Compact, good cache locality |
| **V8 (JS)** | NaN-boxing | 8 bytes | Doubles as float storage |
| **Chez Scheme** | Tagged pointers | 8 bytes | 3-bit tags, 61-bit payload |
| **Guile** | SCM (tagged ptr) | 8 bytes | Conservative GC friendly |
| **Chibi Scheme** | Tagged union | ~24 bytes | Simple, moderate size |
| **CPython** | PyObject* | 8 bytes | Everything is heap object |

### Recommended Approach for VM

**Use tagged pointers (8 bytes) inside the VM:**

```rust
#[repr(transparent)]
pub struct TaggedValue(u64);

impl TaggedValue {
    // Low 3 bits for type tag (pointers are 8-byte aligned)
    const TAG_FIXNUM: u64   = 0b000;  // 61-bit signed integer
    const TAG_SPECIAL: u64  = 0b001;  // #t, #f, (), eof
    const TAG_CHAR: u64     = 0b010;  // Unicode codepoint
    const TAG_PAIR: u64     = 0b011;  // Heap pair
    const TAG_VECTOR: u64   = 0b100;  // Heap vector
    const TAG_STRING: u64   = 0b101;  // Heap string
    const TAG_CLOSURE: u64  = 0b110;  // Heap closure
    const TAG_OBJECT: u64   = 0b111;  // Other heap (sub-tagged)
}
```

**Benefits:**
- 8x smaller than current `Value`
- Immediates (fixnums, bools, chars) require no allocation
- Better cache utilization
- Enables efficient type checking: `(v & 0b111) == TAG_FIXNUM`

**Trade-offs:**
- More complex encoding/decoding
- Limited fixnum range (61 bits vs 64 bits)
- Need separate VM heap (documented in VM_VALUE_ARCHITECTURE.md)

### Key Insight: Dual Representation

Keep both representations:
1. **`Value` (tree-walker):** Simple, safe, for parser/macros/reference impl
2. **`TaggedValue` (VM):** Fast, compact, for execution hot path

Convert at VM entry/exit boundaries. This is exactly what VM_VALUE_ARCHITECTURE.md describes.

---

## 2. Environment and Closures

### Current Patina Design

```rust
pub struct Environment {
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    scoped_bindings: Rc<RefCell<HashMap<String, Vec<ScopedBinding>>>>,
    parent: Option<Rc<Environment>>,
}
```

**Lookup:** Hash lookup per variable access, parent chain traversal

### What Other Implementations Do

| Implementation | Technique | Lookup Cost |
|----------------|-----------|-------------|
| **Lua** | Flat upvalue array | O(1) index |
| **V8** | Context chain + slots | O(1) index |
| **Chez Scheme** | Display closures | O(1) index |
| **CPython** | LOAD_FAST opcode | O(1) index |
| **Guile** | Lexical env + slots | O(1) index |

**Key observation:** All fast implementations compute variable indices at compile time.

### Recommended Approach: Indexed Variable Access

**Phase 1: Add variable indices to IR**

```rust
// Current CoreExpr
pub enum CoreExpr {
    Var(String),  // Lookup by name at runtime
    // ...
}

// New VmCoreExpr (for VM)
pub enum VmCoreExpr {
    LocalVar { index: u16 },           // Direct register access
    ClosureVar { index: u16 },         // Closure slot access
    GlobalVar { name: Symbol },        // Only globals by name
    // ...
}
```

**Phase 2: Flat closure representation**

```rust
// Current closure
pub struct Closure {
    params: Vec<String>,
    body: Vec<CoreExpr>,
    env: Rc<Environment>,  // Captures entire parent environment
}

// New VM closure
pub struct VmClosure {
    code: CodeObjectId,           // Pointer to bytecode
    free_vars: Vec<TaggedValue>,  // Only captured variables, flat array
}
```

**Compilation process:**

```
Lambda source → Free variable analysis → Assign indices → VmCoreExpr
                                                              ↓
                                    LocalVar(0), LocalVar(1), ClosureVar(0), etc.
```

**Benefits:**
- 2-5x speedup for variable-heavy code
- No hash table lookup on hot path
- Cache-friendly sequential access
- Smaller closure objects (only capture what's needed)

**Integration with hygiene:**
- Scope sets are used during macro expansion (compile time)
- By the time we reach VmCoreExpr, all bindings are resolved
- Variable indices are stable after desugaring

---

## 3. Memory Management

### Current Patina Design

```rust
// Pervasive Rc<RefCell<T>> pattern
Value::Pair(Rc<RefCell<(Value, Value)>>)
Value::Vector(Rc<RefCell<Vec<Value>>>)
Value::String(Rc<RefCell<Vec<char>>>)
```

**Characteristics:**
- Reference counting (Rc)
- Interior mutability via RefCell
- Cannot handle cycles (e.g., `(set-cdr! x x)` leaks memory)
- Many small allocations

### What Other Implementations Do

| Implementation | GC Type | Handles Cycles | Notes |
|----------------|---------|----------------|-------|
| **Chez** | Generational | Yes | Very fast, complex |
| **Guile** | BDW (Boehm) | Yes | Conservative, simple |
| **Chibi** | Mark-sweep | Yes | Simple, stop-world |
| **Lua** | Incremental tri-color | Yes | Good latency |
| **CPython** | RC + cycle detector | Yes | Hybrid approach |

### Recommended Approach: rust-gc for VM Heap

**Use rust-gc crate for VM heap:**

```rust
use gc::{Gc, GcCell};

// VM heap objects
pub struct VmPair {
    car: Gc<TaggedValue>,
    cdr: Gc<TaggedValue>,
}

pub struct VmVector {
    elements: GcCell<Vec<TaggedValue>>,
}
```

**Why rust-gc:**
1. Simple integration with Rust
2. Handles cycles correctly
3. Mark-and-sweep (sufficient for interpreter)
4. Derive macro for automatic tracing
5. Well-maintained, production-tested

**Alternative: Arena + manual cycle detection**

For simpler cases, use arena allocation per expression:

```rust
pub struct EvalArena {
    pairs: typed_arena::Arena<(TaggedValue, TaggedValue)>,
    vectors: typed_arena::Arena<Vec<TaggedValue>>,
    // Reset after each top-level expression
}
```

**Recommended strategy:**
1. **Short-term:** Arena allocation for evaluation temporaries
2. **Medium-term:** rust-gc for full heap (Phase 2B)
3. **Long-term:** Consider generational GC if performance critical

---

## 4. String and Symbol Handling

> **Detailed Design:** See [STRING_ABSTRACTION_DESIGN.md](./STRING_ABSTRACTION_DESIGN.md) for a comprehensive design allowing swappable string implementations via feature flags.

### Current Patina Design

```rust
// 4 bytes per character (for O(1) indexing)
Value::String(Rc<RefCell<Vec<char>>>)

// Symbol interning (good!)
thread_local! {
    static SYMBOL_CACHE: RefCell<HashMap<String, Rc<str>>> = ...;
}
```

**Trade-off:** R7RS requires O(1) `string-ref`, hence `Vec<char>`.

### What Other Implementations Do

| Implementation | String Type | Symbol |
|----------------|-------------|--------|
| **Lua** | Interned UTF-8 | Interned |
| **Guile** | UTF-8 + index cache | Interned |
| **Chez** | UTF-8 | Unique + interned |
| **Racket** | UTF-8 | Interned |
| **V8** | Multiple reps | Internalized |

**Observation:** Most use UTF-8 with caching or accept O(n) character access.

### Recommended Improvements

**1. Small string optimization:**

```rust
pub enum SchemeString {
    // Inline for strings ≤ 23 bytes (fits in 24-byte struct)
    Inline { len: u8, data: [u8; 23] },
    // Heap for larger strings
    Heap(Gc<StringData>),
}

pub struct StringData {
    utf8: String,
    // Sparse character index (every 64 chars)
    char_offsets: Vec<usize>,
}
```

**2. UTF-8 with character index cache:**

```rust
pub struct VmString {
    data: Vec<u8>,          // UTF-8 bytes
    char_count: usize,      // Cached length in characters
    // Index every N characters for O(1)-ish access
    index_cache: Option<Vec<usize>>,
}

impl VmString {
    fn char_at(&self, idx: usize) -> char {
        if let Some(ref cache) = self.index_cache {
            // O(1) lookup via cache
            let block = idx / CACHE_INTERVAL;
            let offset = cache[block];
            // Linear scan within block
            self.scan_from(offset, idx % CACHE_INTERVAL)
        } else {
            // O(n) for uncached strings
            self.data.chars().nth(idx).unwrap()
        }
    }
}
```

**3. Symbol improvements:**

Current symbol interning is good. Enhancement: share interned symbols between tree-walker and VM to avoid conversion overhead.

---

## 5. Dispatch and Primitives

### Current Patina Design

```rust
// Dynamic dispatch via registries
pub struct SpecialFormRegistry {
    forms: HashMap<&'static str, Box<dyn SpecialForm>>,
}

pub struct PrimitiveRegistry {
    primitives: HashMap<&'static str, PrimitiveFn>,
}
```

**Runtime:** Hash lookup per primitive call

### What Other Implementations Do

| Implementation | Technique | Notes |
|----------------|-----------|-------|
| **Lua** | Opcode per operation | +, -, cons built into VM |
| **V8** | Inline caching | Polymorphic dispatch |
| **Chez** | Direct C calls | No dispatch overhead |
| **PyPy** | JIT-specialized | Traces through primitives |

### Recommended Approach: Specialized Opcodes

**Add specialized bytecode ops for hot primitives:**

```rust
pub enum Opcode {
    // Generic call (cold path)
    Call { func: Reg, args: RegSlice, dst: Reg },

    // Specialized arithmetic (hot path)
    FixnumAdd { dst: Reg, a: Reg, b: Reg, overflow: Label },
    FixnumSub { dst: Reg, a: Reg, b: Reg, overflow: Label },
    FixnumMul { dst: Reg, a: Reg, b: Reg, overflow: Label },
    FloatAdd { dst: Reg, a: Reg, b: Reg },

    // Specialized data structure ops
    Cons { dst: Reg, car: Reg, cdr: Reg },
    Car { dst: Reg, pair: Reg },
    Cdr { dst: Reg, pair: Reg },
    VectorRef { dst: Reg, vec: Reg, idx: Reg },

    // Specialized comparisons
    FixnumLt { dst: Reg, a: Reg, b: Reg },
    FixnumEq { dst: Reg, a: Reg, b: Reg },
}
```

**Benefits:**
- No function call overhead for common operations
- Branch-free type checking via guards
- Better inlining by CPU branch predictor
- Foundation for JIT specialization

**Integration with adaptive optimization:**

```rust
// Profiling version (used during warmup)
Opcode::Add { dst, a, b, profile_id }

// After profiling shows 99% fixnum:
// Optimizer rewrites to:
Opcode::FixnumAdd { dst, a, b, overflow: deopt_label }
```

---

## 6. Summary of Recommendations

### Must Have for Phase 2 VM

| Change | Impact | Effort | Section |
|--------|--------|--------|---------|
| **Tagged pointer values** | 2-3x speed, 8x memory | Medium | §1 |
| **Indexed variable access** | 2-5x speed | Medium | §2 |
| **Flat closure representation** | 2x speed, less memory | Medium | §2 |
| **Specialized opcodes** | 2-3x speed | Low | §5 |

### Should Have (Correctness/Quality)

| Change | Impact | Effort | Section |
|--------|--------|--------|---------|
| **GC (rust-gc)** | Correctness | Medium | §3 |
| **Arena allocation** | Memory churn | Low | §3 |
| **Small string optimization** | Memory | Low | §4 |

### Nice to Have (Polish)

| Change | Impact | Effort | Section |
|--------|--------|--------|---------|
| **UTF-8 + index cache** | Memory | Medium | §4 |
| **Inline caching** | Speed | High | §5 |
| **Generational GC** | Latency | High | §3 |

---

## Integration with Existing Design Docs

These recommendations integrate with:

1. **VM_VALUE_ARCHITECTURE.md:** Already describes dual representation (Value/TaggedValue) - this doc provides comparative justification

2. **VM_SPECIFICATION.md:** Should add:
   - Indexed variable access instructions (LoadLocal, LoadClosure)
   - More specialized opcodes section

3. **COMPILATION_DESIGN.md:** Should add:
   - Free variable analysis at compile time
   - Variable index assignment
   - Flat closure compilation

4. **TAGGED_POINTERS.md:** Already comprehensive - this doc validates the approach

---

## Implementation Order

Recommended order for Phase 2A (foundation):

1. **TaggedValue + VmHeap** (existing design in VM_VALUE_ARCHITECTURE.md)
2. **Indexed variables in VmCoreExpr** (new - add to COMPILATION_DESIGN.md)
3. **Flat closure compilation** (new - add to COMPILATION_DESIGN.md)
4. **Specialized opcodes for +, -, cons, car, cdr** (existing in VM_SPECIFICATION.md)
5. **Arena allocation for temporaries** (new)
6. **rust-gc integration** (Phase 2B)

This order builds foundation first, then adds optimizations incrementally.

---

## References

### Academic
- "Three Implementation Models for Scheme" - Dybvig (display closures)
- "Representing Type Information in Dynamically Typed Languages" - Gudeman (tagged pointers)
- "Efficient Implementation of the Smalltalk-80 System" - Deutsch & Schiffman (inline caching)

### Implementation References
- [Lua 5.4 Implementation](https://www.lua.org/doc/jucs05.pdf)
- [V8 Design](https://v8.dev/docs)
- [Chez Scheme Internals](https://cisco.github.io/ChezScheme/)
- [rust-gc crate](https://crates.io/crates/gc)

### Patina Docs
- [VM_VALUE_ARCHITECTURE.md](./VM_VALUE_ARCHITECTURE.md)
- [VM_SPECIFICATION.md](./VM_SPECIFICATION.md)
- [COMPILATION_DESIGN.md](./COMPILATION_DESIGN.md)
- [TAGGED_POINTERS.md](./TAGGED_POINTERS.md)
