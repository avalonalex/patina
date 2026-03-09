# VM Value Architecture: Dual Representation Design

**Status:** Design Document
**Created:** 2025-12-12
**Related:** [VM_SPECIFICATION.md](./VM_SPECIFICATION.md), [TAGGED_POINTERS.md](./TAGGED_POINTERS.md)

---

## Overview

This document describes the architecture for supporting two value representations in Patina:

1. **`Value` enum** (~32 bytes) - Used by tree-walker, parser, macro system
2. **`TaggedValue(u64)`** (8 bytes) - Used internally by the VM for performance

The key insight is that **tagged pointers are only used inside the VM**, keeping the tree-walker simple and preserving it as a reference implementation.

---

## Design Goals

1. **Keep tree-walker unchanged** - Simple, tested, serves as reference
2. **VM gets full performance benefits** - 8-byte values, cache-friendly
3. **Clean API boundary** - Backend trait returns `Value`, conversion is internal
4. **No forced migration** - Existing code continues to work
5. **Incremental adoption** - VM can be developed alongside tree-walker

---

## Architecture Overview

```
                         patina-core (shared)
                    ┌────────────────────────────┐
                    │  Value (enum, ~32 bytes)   │
                    │  CoreExpr                  │
                    │  Environment               │
                    │  ScopeSet, Symbol          │
                    └────────────────────────────┘
                                 │
        ┌────────────────────────┴────────────────────────┐
        │                                                 │
        ▼                                                 ▼
┌───────────────────┐                         ┌───────────────────┐
│ patina-tree-walker│                         │    patina-vm      │
├───────────────────┤                         ├───────────────────┤
│ Uses Value enum   │                         │ TaggedValue(u64)  │
│ CPS evaluation    │                         │ VmHeap            │
│ Rc<RefCell<...>>  │                         │ Bytecode          │
│ Reference impl    │                         │ High performance  │
└───────────────────┘                         └───────────────────┘
        │                                                 │
        └────────────────────────┬────────────────────────┘
                                 │
                                 ▼
                    ┌────────────────────────────┐
                    │   patina-interpreter       │
                    │   Backend trait API        │
                    │   Returns: Value           │
                    └────────────────────────────┘
```

---

## Data Flow

### Tree-Walker Path (Unchanged)

```
Source → Parser → Value → Desugarer → CoreExpr → CPS Transform → CpsExpr → Result: Value
```

### VM Path (New)

```
Source → Parser → Value → Desugarer → CoreExpr
                                          │
                                          ▼
                                   ┌─────────────┐
                                   │  Compiler   │
                                   │  (CoreExpr  │
                                   │   → Bytecode)│
                                   └─────────────┘
                                          │
                          Converts Value → TaggedValue
                          Allocates on VmHeap
                                          │
                                          ▼
                                   ┌─────────────┐
                                   │   Bytecode  │
                                   │  (TaggedValue│
                                   │   constants) │
                                   └─────────────┘
                                          │
                                          ▼
                                   ┌─────────────┐
                                   │  VM Execute │
                                   │  (all ops on│
                                   │  TaggedValue)│
                                   └─────────────┘
                                          │
                                          ▼
                                   Result: TaggedValue
                                          │
                          Converts TaggedValue → Value
                                          │
                                          ▼
                                   Result: Value (API)
```

---

## TaggedValue Representation

### Tagging Scheme

```rust
/// 8-byte tagged value for VM operations
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TaggedValue(u64);

impl TaggedValue {
    // Low 3 bits encode primary type (8 possibilities)
    const TAG_BITS: u32 = 3;
    const TAG_MASK: u64 = 0b111;

    // Immediate types (no heap allocation)
    const TAG_FIXNUM: u64   = 0b000;  // 61-bit signed integer
    const TAG_SPECIAL: u64  = 0b001;  // #t, #f, (), eof, unspecified
    const TAG_CHAR: u64     = 0b010;  // Unicode codepoint

    // Pointer types (point into VmHeap)
    const TAG_PAIR: u64     = 0b011;  // Cons cell
    const TAG_VECTOR: u64   = 0b100;  // Vector
    const TAG_STRING: u64   = 0b101;  // String
    const TAG_CLOSURE: u64  = 0b110;  // Closure
    const TAG_OBJECT: u64   = 0b111;  // Other heap objects (sub-tag in header)

    // Special value encoding (when TAG_SPECIAL)
    pub const TRUE: Self        = Self(0x08 | Self::TAG_SPECIAL);  // 0b0_1000 | 001
    pub const FALSE: Self       = Self(0x00 | Self::TAG_SPECIAL);  // 0b0_0000 | 001
    pub const NULL: Self        = Self(0x10 | Self::TAG_SPECIAL);  // 0b1_0000 | 001
    pub const EOF: Self         = Self(0x18 | Self::TAG_SPECIAL);  // 0b1_1000 | 001
    pub const UNSPECIFIED: Self = Self(0x20 | Self::TAG_SPECIAL);  // 0b10_0000 | 001
}
```

### Fast Type Checks

```rust
impl TaggedValue {
    #[inline(always)]
    pub fn is_fixnum(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_FIXNUM
    }

    #[inline(always)]
    pub fn is_pair(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_PAIR
    }

    #[inline(always)]
    pub fn is_immediate(self) -> bool {
        matches!(
            self.0 & Self::TAG_MASK,
            Self::TAG_FIXNUM | Self::TAG_SPECIAL | Self::TAG_CHAR
        )
    }

    #[inline(always)]
    pub fn is_heap_pointer(self) -> bool {
        !self.is_immediate()
    }
}
```

### Fixnum Operations

```rust
impl TaggedValue {
    /// Create a fixnum (61-bit signed integer)
    #[inline(always)]
    pub fn fixnum(n: i64) -> Self {
        debug_assert!(
            n >= -(1i64 << 60) && n < (1i64 << 60),
            "Fixnum overflow: {n}"
        );
        // Shift left 3 bits, TAG_FIXNUM is 0 so no OR needed
        Self((n as u64) << Self::TAG_BITS)
    }

    /// Extract fixnum value (unchecked)
    #[inline(always)]
    pub fn as_fixnum_unchecked(self) -> i64 {
        // Arithmetic right shift preserves sign
        (self.0 as i64) >> Self::TAG_BITS
    }

    /// Safe fixnum extraction
    #[inline(always)]
    pub fn as_fixnum(self) -> Option<i64> {
        if self.is_fixnum() {
            Some(self.as_fixnum_unchecked())
        } else {
            None
        }
    }
}
```

---

## VmHeap Design

The VM uses its own heap, separate from tree-walker's `Rc<RefCell<...>>`:

```rust
/// VM heap with arena allocation
pub struct VmHeap {
    /// Pair storage: (car, cdr) tuples
    pairs: Vec<(TaggedValue, TaggedValue)>,

    /// Vector storage
    vectors: Vec<VmVector>,

    /// String storage (immutable after creation in VM)
    strings: Vec<String>,

    /// Closure storage
    closures: Vec<VmClosure>,

    /// Generic heap objects (records, ports, etc.)
    objects: Vec<HeapObject>,

    /// Free lists for reuse (simple GC)
    free_pairs: Vec<HeapIndex>,
    free_vectors: Vec<HeapIndex>,
}

pub type HeapIndex = u32;

pub struct VmVector {
    elements: Vec<TaggedValue>,
}

pub struct VmClosure {
    code: CodeObjectId,
    free_vars: Vec<TaggedValue>,
}

/// Header for generic heap objects
pub struct HeapObject {
    /// Sub-type tag (bigint, rational, complex, record, port, etc.)
    tag: HeapObjectTag,
    /// Object data
    data: HeapObjectData,
}

pub enum HeapObjectTag {
    BigInt,
    Rational,
    Complex,
    Real,       // Boxed f64 (when NaN-boxing not used)
    Record,
    Port,
    Promise,
    Continuation,
    // ... extensible
}
```

### Pointer Encoding

```rust
impl TaggedValue {
    /// Create a pair pointer
    pub fn pair(index: HeapIndex) -> Self {
        Self(((index as u64) << Self::TAG_BITS) | Self::TAG_PAIR)
    }

    /// Extract heap index from pointer
    #[inline(always)]
    pub fn heap_index(self) -> HeapIndex {
        debug_assert!(self.is_heap_pointer());
        (self.0 >> Self::TAG_BITS) as HeapIndex
    }
}

impl VmHeap {
    /// Allocate a new pair
    pub fn alloc_pair(&mut self, car: TaggedValue, cdr: TaggedValue) -> TaggedValue {
        let index = if let Some(free) = self.free_pairs.pop() {
            self.pairs[free as usize] = (car, cdr);
            free
        } else {
            let index = self.pairs.len() as HeapIndex;
            self.pairs.push((car, cdr));
            index
        };
        TaggedValue::pair(index)
    }

    /// Access pair (unchecked)
    #[inline(always)]
    pub fn get_pair(&self, ptr: TaggedValue) -> (TaggedValue, TaggedValue) {
        self.pairs[ptr.heap_index() as usize]
    }

    /// Mutate pair (for set-car!/set-cdr!)
    #[inline(always)]
    pub fn set_car(&mut self, ptr: TaggedValue, value: TaggedValue) {
        self.pairs[ptr.heap_index() as usize].0 = value;
    }
}
```

---

## Value Conversion

### Value → TaggedValue (at compilation/entry)

```rust
impl TaggedValue {
    /// Convert from patina-core Value to TaggedValue
    /// Allocates heap objects on VmHeap as needed
    pub fn from_value(value: &Value, heap: &mut VmHeap) -> Self {
        match value {
            // Immediates - direct conversion
            Value::Integer(n) if Self::fits_fixnum(*n) => Self::fixnum(*n),
            Value::Boolean(true) => Self::TRUE,
            Value::Boolean(false) => Self::FALSE,
            Value::Null => Self::NULL,
            Value::Character(c) => Self::character(*c),
            Value::Eof => Self::EOF,
            Value::Unspecified => Self::UNSPECIFIED,

            // Heap types - allocate on VmHeap
            Value::Integer(n) => {
                // Overflow to bigint
                heap.alloc_bigint(BigInt::from(*n))
            }
            Value::BigInteger(n) => heap.alloc_bigint(n.clone()),
            Value::Rational(r) => heap.alloc_rational(r.clone()),
            Value::Real(f) => Self::float(*f),  // Or heap if not NaN-boxing
            Value::Complex(c) => {
                let real = Self::from_value(&c.0, heap);
                let imag = Self::from_value(&c.1, heap);
                heap.alloc_complex(real, imag)
            }

            Value::Pair(p) => {
                let (car, cdr) = &*p.borrow();
                let car_tagged = Self::from_value(car, heap);
                let cdr_tagged = Self::from_value(cdr, heap);
                heap.alloc_pair(car_tagged, cdr_tagged)
            }

            Value::Vector(v) => {
                let elements: Vec<_> = v.borrow()
                    .iter()
                    .map(|e| Self::from_value(e, heap))
                    .collect();
                heap.alloc_vector(elements)
            }

            Value::String(s) => heap.alloc_string(s.borrow().clone()),

            Value::Symbol(s) => heap.intern_symbol(s),

            Value::Procedure(p) => {
                // Convert closure to VM closure
                heap.alloc_closure_from_procedure(p, heap)
            }

            // ... other types
        }
    }

    fn fits_fixnum(n: i64) -> bool {
        n >= -(1i64 << 60) && n < (1i64 << 60)
    }
}
```

### TaggedValue → Value (at exit)

```rust
impl TaggedValue {
    /// Convert back to patina-core Value for API compatibility
    pub fn to_value(self, heap: &VmHeap) -> Value {
        if self.is_fixnum() {
            Value::Integer(self.as_fixnum_unchecked())
        } else if self == Self::TRUE {
            Value::Boolean(true)
        } else if self == Self::FALSE {
            Value::Boolean(false)
        } else if self == Self::NULL {
            Value::Null
        } else if self.is_char() {
            Value::Character(self.as_char_unchecked())
        } else if self == Self::EOF {
            Value::Eof
        } else if self == Self::UNSPECIFIED {
            Value::Unspecified
        } else if self.is_pair() {
            let (car, cdr) = heap.get_pair(self);
            Value::Pair(Rc::new(RefCell::new((
                car.to_value(heap),
                cdr.to_value(heap),
            ))))
        } else if self.is_vector() {
            let vec = heap.get_vector(self);
            Value::Vector(Rc::new(RefCell::new(
                vec.iter().map(|e| e.to_value(heap)).collect()
            )))
        } else if self.is_string() {
            Value::String(Rc::new(RefCell::new(heap.get_string(self).clone())))
        } else if self.is_object() {
            // Handle sub-tagged objects
            match heap.get_object_tag(self) {
                HeapObjectTag::BigInt => {
                    Value::BigInteger(heap.get_bigint(self).clone())
                }
                HeapObjectTag::Rational => {
                    Value::Rational(heap.get_rational(self).clone())
                }
                // ... other object types
            }
        } else {
            panic!("Unknown TaggedValue type: {:064b}", self.0)
        }
    }
}
```

---

## Backend Implementation

```rust
// In patina-vm/src/backend.rs

use patina_runtime::{Backend, Environment, Value};
use std::rc::Rc;

pub struct VM {
    heap: VmHeap,
    code_cache: CodeCache,
    profiler: Profiler,
    // ... other VM state
}

impl Backend for VM {
    type Error = VmError;

    fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, Self::Error> {
        // 1. Desugar to CoreExpr (same as tree-walker)
        let desugarer = Desugarer::with_env(env.clone());
        let core_expr = desugarer.desugar(expr)?;

        // 2. Compile to bytecode (converts Values to TaggedValues)
        let bytecode = self.compile(&core_expr, env)?;

        // 3. Execute (all internal ops use TaggedValue)
        let result: TaggedValue = self.execute(bytecode)?;

        // 4. Convert result back to Value for API compatibility
        Ok(result.to_value(&self.heap))
    }

    fn global_env(&self) -> &Rc<Environment> {
        &self.global_env
    }
}
```

---

## Primitives: Dual Implementation

Primitives need two implementations - one for tree-walker (using `Value`), one for VM (using `TaggedValue`):

### Tree-Walker Primitive (Existing)

```rust
// In patina-tree-walker/src/eval/primitives/arithmetic.rs (unchanged)

pub fn prim_add(args: &[Value]) -> Result<Value, EvalError> {
    let mut result = Value::Integer(0);
    for arg in args {
        result = numeric_add(&result, arg)?;
    }
    Ok(result)
}

fn numeric_add(a: &Value, b: &Value) -> Result<Value, EvalError> {
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => {
            match x.checked_add(*y) {
                Some(sum) => Ok(Value::Integer(sum)),
                None => {
                    // Promote to BigInt
                    let big_x = BigInt::from(*x);
                    let big_y = BigInt::from(*y);
                    Ok(Value::BigInteger(big_x + big_y))
                }
            }
        }
        (Value::Real(x), Value::Real(y)) => Ok(Value::Real(x + y)),
        // ... other numeric type combinations
    }
}
```

### VM Primitive (New, Optimized)

```rust
// In patina-vm/src/primitives/arithmetic.rs

/// Fast path for fixnum addition
#[inline(always)]
pub fn vm_add_fixnum(a: TaggedValue, b: TaggedValue) -> Option<TaggedValue> {
    debug_assert!(a.is_fixnum() && b.is_fixnum());

    let x = a.as_fixnum_unchecked();
    let y = b.as_fixnum_unchecked();

    // Check for overflow (61-bit arithmetic)
    match x.checked_add(y) {
        Some(sum) if TaggedValue::fits_fixnum(sum) => {
            Some(TaggedValue::fixnum(sum))
        }
        _ => None  // Overflow, need slow path
    }
}

/// General addition (handles all numeric types)
pub fn vm_add(a: TaggedValue, b: TaggedValue, heap: &mut VmHeap) -> TaggedValue {
    // Fast path: both fixnums
    if a.is_fixnum() && b.is_fixnum() {
        if let Some(result) = vm_add_fixnum(a, b) {
            return result;
        }
        // Overflow: promote to bigint
        let big_a = BigInt::from(a.as_fixnum_unchecked());
        let big_b = BigInt::from(b.as_fixnum_unchecked());
        return heap.alloc_bigint(big_a + big_b);
    }

    // Medium path: both floats
    if a.is_float() && b.is_float() {
        return TaggedValue::float(a.as_float_unchecked() + b.as_float_unchecked());
    }

    // Slow path: mixed types, bignums, rationals, complex
    vm_add_slow(a, b, heap)
}
```

### Bytecode Integration

```rust
// In VM execution loop
match op {
    Op::Add { dst, src1, src2, profile_id } => {
        let a = self.get_register(src1);
        let b = self.get_register(src2);

        // Record types for adaptive optimization
        self.profiler.record_types(profile_id, a, b);

        let result = vm_add(a, b, &mut self.heap);
        self.set_register(dst, result);
    }

    // Specialized opcode (generated by optimizer)
    Op::FixnumAdd { dst, src1, src2, overflow_label } => {
        let a = self.get_register(src1);
        let b = self.get_register(src2);

        match vm_add_fixnum(a, b) {
            Some(result) => self.set_register(dst, result),
            None => {
                // Deoptimize: jump to overflow handler
                self.pc = overflow_label;
                continue;
            }
        }
    }
}
```

---

## Continuations

### Tree-Walker: CPS-Based (Unchanged)

The tree-walker uses CPS transformation and `CpsExpr`:

```rust
// Continuation is a CpsExpr that represents "what to do next"
pub enum CpsExpr {
    // ...
    LetCont { name, body, in_expr },
    AppCont { cont, value },
    CallCC { func, cont },
    // ...
}
```

### VM: Stack-Based

The VM uses a different continuation model based on stack frames:

```rust
pub struct VmContinuation {
    /// Stack frames captured up to prompt
    frames: Vec<VmFrame>,

    /// Register state at capture time
    registers: Vec<TaggedValue>,

    /// Prompt tag this was captured under
    prompt_tag: PromptTagId,

    /// Dynamic-wind entries for proper cleanup
    wind_entries: Vec<WindEntry>,
}

pub struct VmFrame {
    code: CodeObjectId,
    pc: usize,
    register_base: usize,
    // ...
}
```

**Why different models?**

| Aspect | Tree-Walker CPS | VM Stack |
|--------|-----------------|----------|
| Implementation | Transform to CpsExpr | Native stack frames |
| Overhead | CPS transform cost | None |
| call/cc | Trivial (cont is explicit) | Copy stack frames |
| Common case | Every call builds CpsExpr | Just push frame |
| Memory | Many small allocations | Contiguous stack |

The VM model is faster for the common case (no call/cc) while still supporting full continuations when needed.

---

## Conversion Cost Analysis

### When Conversion Happens

| Operation | Tree-Walker | VM |
|-----------|-------------|-----|
| Parse | → Value | → Value |
| Macro expand | Value → Value | Value → Value |
| Desugar | Value → CoreExpr | Value → CoreExpr |
| Compile | N/A | CoreExpr → Bytecode (Value→Tagged) |
| Execute | CpsExpr (Value) | Bytecode (TaggedValue) |
| Return | Value | TaggedValue → Value |

### Cost Per Operation

**Immediates (fixnum, bool, char, null):**
- Value → Tagged: ~3 instructions (match + shift + OR)
- Tagged → Value: ~3 instructions (mask + shift + construct)
- **Negligible cost**

**Simple heap objects (pairs, vectors):**
- Value → Tagged: O(n) where n = number of elements
- Tagged → Value: O(n)
- **Linear in size, but only at boundaries**

**Large structures:**
- Deep conversion could be expensive
- But: Most results are small (single values, short lists)
- Large data stays in VM during computation

### Mitigation Strategies

1. **Lazy conversion**: Only convert what's needed
   ```rust
   // Don't convert entire list, just head
   fn to_value_lazy(self, heap: &VmHeap, depth: usize) -> Value
   ```

2. **Result caching**: Cache converted results for repeated access

3. **Streaming output**: For large results, stream conversion

---

## Testing Strategy

### Correctness Tests

1. **Round-trip tests**: `Value → TaggedValue → Value` preserves semantics
   ```rust
   #[test]
   fn roundtrip_integer() {
       let value = Value::Integer(42);
       let mut heap = VmHeap::new();
       let tagged = TaggedValue::from_value(&value, &mut heap);
       let back = tagged.to_value(&heap);
       assert_eq!(value, back);
   }
   ```

2. **Edge cases**: Fixnum boundaries, special values, cyclic structures

3. **All R7RS tests pass with VM backend**

### Performance Tests

1. **Microbenchmarks**: Fixnum arithmetic, pair operations
2. **Macrobenchmarks**: fibonacci, tak, sorting
3. **Compare**: VM vs tree-walker for same inputs

---

## Implementation Phases

### Phase 1: TaggedValue Foundation

- [ ] Implement `TaggedValue` with tagging scheme
- [ ] Implement `VmHeap` with pair/vector/string allocation
- [ ] Implement `from_value` and `to_value` conversions
- [ ] Unit tests for all conversions

### Phase 2: Basic VM

- [ ] Bytecode compiler from CoreExpr
- [ ] Basic execution loop
- [ ] VM primitives (arithmetic, pairs, vectors)
- [ ] Backend trait implementation

### Phase 3: Integration

- [ ] Run R7RS test suite with VM backend
- [ ] Performance benchmarks
- [ ] Fix any semantic differences

### Phase 4: Optimization

- [ ] Profiling infrastructure
- [ ] Adaptive numeric specialization
- [ ] (Future) Tracing JIT

---

## Open Questions

1. **Float representation**: NaN-boxing vs heap-allocated?
   - NaN-boxing: More complex encoding, but floats are immediate
   - Heap: Simpler, but floats need allocation

2. **Symbol interning**: Shared with tree-walker or VM-local?
   - Shared: No conversion needed for symbols
   - VM-local: Cleaner separation

3. **Closure conversion**: How to handle existing `Value::Procedure`?
   - Need to compile lambda bodies to bytecode
   - Free variable capture

4. **GC strategy**: Reference counting vs tracing?
   - RC: Simpler, deterministic
   - Tracing: Better for cycles, less overhead

---

## GC Strategy Recommendation

Based on analysis in [ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md) §3, we recommend a phased approach:

### Phase 2A: Arena Allocation (Immediate)

Use typed arenas for evaluation temporaries:

```rust
use typed_arena::Arena;

pub struct EvalArena {
    pairs: Arena<(TaggedValue, TaggedValue)>,
    vectors: Arena<Vec<TaggedValue>>,
    strings: Arena<String>,
}

impl EvalArena {
    /// Reset arena after each top-level expression
    pub fn reset(&mut self) {
        // Arenas are dropped and recreated
        *self = Self::new();
    }
}
```

**Benefits:**
- Reduces allocation churn during evaluation
- Fast bump allocation
- No GC pauses for short-lived objects
- Simple implementation

### Phase 2B: rust-gc Integration (Medium-term)

For long-lived objects and cycle handling, integrate `rust-gc`:

```rust
use gc::{Gc, GcCell, Trace};

#[derive(Trace)]
pub struct GcPair {
    car: Gc<TaggedValue>,
    cdr: Gc<TaggedValue>,
}

#[derive(Trace)]
pub struct GcVector {
    elements: GcCell<Vec<TaggedValue>>,
}

impl VmHeap {
    pub fn alloc_pair(&mut self, car: TaggedValue, cdr: TaggedValue) -> TaggedValue {
        let pair = Gc::new(GcPair { car: Gc::new(car), cdr: Gc::new(cdr) });
        TaggedValue::gc_pair(pair)
    }
}
```

**Why rust-gc:**
- Handles cycles correctly (required for R7RS: `set-car!`, `set-cdr!`)
- Simple integration via derive macro
- Mark-and-sweep (sufficient for interpreter)
- Well-maintained crate

### Phase 3: Generational GC (Long-term, Optional)

If profiling shows GC is a bottleneck:
- Consider implementing generational collection
- Young generation for short-lived allocations
- Old generation for survivors
- More complex, only if needed

### Comparison with Other Implementations

| Implementation | GC Type | Handles Cycles | Complexity |
|----------------|---------|----------------|------------|
| Chez Scheme | Generational | Yes | High |
| Guile | Boehm conservative | Yes | Low |
| Chibi Scheme | Mark-sweep | Yes | Medium |
| Lua | Incremental tri-color | Yes | Medium |
| **Patina VM** | rust-gc mark-sweep | Yes | Low |

### Implementation Order

1. **Immediate:** Arena allocation for temporaries
2. **Phase 2B:** rust-gc for heap objects
3. **If needed:** Generational optimization

---

## References

- [ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md) - Comparative analysis with other implementations
- [TAGGED_POINTERS.md](./TAGGED_POINTERS.md) - Original tagged pointer design
- [VM_SPECIFICATION.md](./VM_SPECIFICATION.md) - Full VM specification
- [rust-gc crate](https://crates.io/crates/gc) - Rust garbage collection library
- "Binding as Sets of Scopes" (Flatt 2016) - Hygiene system
- "Three Implementation Models for Scheme" (Dybvig) - VM design
