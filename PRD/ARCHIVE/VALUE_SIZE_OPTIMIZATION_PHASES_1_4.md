# Value and CoreExpr Size Optimization (Phases 1-4)

**Date:** 2025-11-25
**Status:** ✅ COMPLETE (Phases 1-4) - Archived 2025-11-25
**Priority:** Medium-High (Performance Foundation)

> **Archive Note:** Phases 1-4 have been implemented:
> - **Phase 1**: Box large variants (Procedure, Identifier) ✅
> - **Phase 2**: ScopeSet optimization with SmallVec ✅
> - **Phase 3**: Reduce clone frequency with Rc<CoreExpr> ✅
> - **Phase 4**: Value interning (symbols + small integers) ✅
>
> **Phase 5 (Tagged Pointers)** has been moved to Phase 2 VM design:
> See [`PRD/phase2/TAGGED_POINTERS.md`](../phase2/TAGGED_POINTERS.md)

## Executive Summary

Patina's `Value` and `CoreExpr` types are significantly larger than optimal, causing performance degradation through excessive memory usage and clone overhead. This document presents findings from size analysis, identifies bottlenecks in the parse→desugar→evaluate pipeline, and provides a phased roadmap for optimization.

**Key Findings:**
- `Value`: **104 bytes** (target: 8-16 bytes)
- `CoreExpr`: **168 bytes** (target: 24-32 bytes)
- Each cons cell: **232 bytes** (target: 24-32 bytes)
- Frequent cloning in evaluator hot paths

**Impact:**
- ~10x memory overhead for common values
- Poor cache locality
- Significant clone costs in evaluation loops

---

## 1. Current State Analysis

### 1.1 Type Size Measurements

Measured using `RUSTFLAGS="-Zprint-type-sizes" cargo +nightly build`:

| Type | Actual Size | Optimal Size | Overhead |
|------|-------------|--------------|----------|
| `Value` | 104 bytes | 8-16 bytes | 6-13x |
| `CoreExpr` | 168 bytes | 24-32 bytes | 5-7x |
| `Procedure` | 104 bytes | 8 bytes (boxed) | 13x |
| `ScopeSet` | 48 bytes | 8-16 bytes | 3-6x |
| `LambdaBody` | 32 bytes | 8 bytes (Rc) | 4x |
| `(Value, Value)` in RefCell | 216 bytes | 16-24 bytes | 9-13x |
| Full cons cell with Rc | 232 bytes | 24-32 bytes | 7-10x |

### 1.2 Value Enum Breakdown

```rust
pub enum Value {
    // Small variants (could fit in 8-16 bytes)
    Boolean(bool),           // 1 byte data
    Integer(i64),            // 8 bytes data
    Real(f64),               // 8 bytes data
    Complex(f64, f64),       // 16 bytes data
    Character(char),         // 4 bytes data
    Null,                    // 0 bytes data
    Unspecified,             // 0 bytes data
    Eof,                     // 0 bytes data

    // Medium variants (pointer + metadata)
    Symbol(Rc<str>),         // 16 bytes (fat pointer)
    String(Rc<RefCell<String>>), // 8 bytes
    Pair(Rc<RefCell<(Value, Value)>>), // 8 bytes
    Vector(Rc<RefCell<Vec<Value>>>),   // 8 bytes
    Bytevector(Rc<RefCell<Vec<u8>>>),  // 8 bytes
    BigInteger(BigInt),      // heap allocated
    Rational(BigRational),   // heap allocated
    Macro(Rc<CompiledMacro>), // 8 bytes
    Library(Rc<Library>),    // 8 bytes
    Promise(Rc<RefCell<PromiseState>>), // 8 bytes
    InputPort, OutputPort,   // 0 bytes (unit variants)

    // LARGE variants (drive total enum size)
    Identifier {             // 64 bytes!
        name: Rc<str>,       // 16 bytes
        scopes: ScopeSet,    // 48 bytes
    },
    Procedure(Procedure),    // 104 bytes! (largest)
    Parameter {              // ~32 bytes
        values: Rc<RefCell<Vec<Value>>>,
        converter: Option<Box<Value>>,
    },
    Values(Vec<Value>),      // 24 bytes
}
```

**The `Procedure` variant dominates** because it contains:
```rust
pub enum Procedure {
    Primitive {
        name: &'static str,      // 16 bytes
        arity: Arity,            // 24 bytes (enum)
        library: Vec<String>,    // 24 bytes
    },
    Lambda {
        params: Vec<String>,     // 24 bytes
        variadic: Option<String>, // 24 bytes
        body: LambdaBody,        // 32 bytes
        env: Rc<Environment>,    // 8 bytes
        binding_scope: Option<ScopeId>, // 16 bytes
    },
    CaseLambda { ... },
    Continuation,
}
```

### 1.3 CoreExpr Enum Breakdown

```rust
pub enum CoreExpr {
    // Small variants
    Literal(Value),          // 104 bytes (contains full Value!)
    Var(Symbol),             // 16 bytes
    Quote(Value),            // 104 bytes
    Quasiquote(Value),       // 104 bytes

    // Medium variants
    ScopedVar { name, scopes }, // 64 bytes
    Lambda { params, body, binding_scope }, // ~80 bytes
    If { test, then, else_ }, // 24 bytes (3 Box<CoreExpr>)
    Set { var, value },      // 24 bytes
    Begin(Vec<CoreExpr>),    // 24 bytes
    Define { name, value },  // 24 bytes
    App { func, args },      // 32 bytes
    Apply { func, args },    // 32 bytes

    // LARGE variant (drives total enum size)
    DefineSyntax {           // 168 bytes!
        name: Symbol,        // 16 bytes
        transformer: Value,  // 104 bytes (full Value!)
        definition_scopes: ScopeSet, // 48 bytes
    },

    // Other variants...
    Import { import_sets: Vec<Value> }, // 24 bytes
    Parameterize { bindings, body },    // 48 bytes
    CaseLambda { clauses },  // 24 bytes
    PrimCall { prim, args }, // 32 bytes
    Let { bindings, body },  // 32 bytes
    Expand { expr },         // 8 bytes
}
```

### 1.4 Clone Hotspots in Pipeline

**Desugarer (`crates/patina-frontend/src/desugarer/mod.rs`):**
```rust
// Line 175: Cloning Value for literals
CoreExpr::Literal(value.clone())  // 104 bytes copied

// Line 283: Cloning quoted data
Ok(CoreExpr::Quote(datum.clone()))  // 104 bytes copied
```

**Core Evaluator (`crates/patina-tree-walker/src/eval/core_eval.rs`):**
```rust
// Line 121: Trampoline loop clones expression
let mut current_expr = expr.clone();  // 168 bytes copied per iteration!

// Line 147: Literal evaluation
CoreExpr::Literal(v) => Ok(CoreEvalResult::Value(v.clone()))  // 104 bytes

// Line 162: Quote evaluation
CoreExpr::Quote(v) => Ok(CoreEvalResult::Value(v.clone()))  // 104 bytes

// Line 183-188: Lambda creation clones body
body: LambdaBody::Core(body.clone())  // Vec<CoreExpr> cloned

// Line 212-215: TailCall clones branch
Ok(CoreEvalResult::TailCall {
    expr: (**branch).clone(),  // 168 bytes!
    env,
})

// Line 360-365: Application evaluates and clones args
for arg in args {
    arg_vals.push(eval_non_tail(arg, env.clone(), evaluator)?);
}
```

---

## 2. Problem Analysis

### 2.1 Memory Impact

**Example: Simple list `(1 2 3 4 5)`**

Current:
- 5 cons cells × 232 bytes = **1,160 bytes**
- Plus 5 integers inline = already counted in Value
- Total: ~1.2 KB for 5 small integers

Optimal (tagged pointers):
- 5 cons cells × 24 bytes = 120 bytes
- 5 tagged integers = 0 additional (inline in pointer)
- Total: **120 bytes** (10x reduction)

**Example: Lambda `(lambda (x) (+ x 1))`**

Current:
- `Value::Procedure(Lambda {...})`: 104 bytes inline
- Plus heap allocations for params Vec, body Vec
- Environment Rc: 8 bytes
- Total Value: **104+ bytes**

Optimal:
- `Value::Procedure(Box<Procedure>)`: 8 bytes inline
- Procedure on heap: ~80 bytes
- Total Value: **8 bytes** (inline), 80 bytes heap

### 2.2 Performance Impact

**Clone Cost Analysis:**

| Operation | Current Cost | Optimal Cost |
|-----------|--------------|--------------|
| Clone Value | 104 bytes memcpy | 8 bytes memcpy |
| Clone CoreExpr | 168 bytes memcpy | 24-32 bytes memcpy |
| TailCall iteration | 168 + 8 bytes | 32 bytes |
| Cons cell creation | 232 bytes alloc | 24 bytes alloc |

**Cache Impact:**
- L1 cache line: 64 bytes
- Current Value: spans 2 cache lines
- Current CoreExpr: spans 3 cache lines
- Optimal: fits in 1 cache line

### 2.3 Root Causes

1. **Large variants not boxed**: `Procedure`, `Identifier`, `DefineSyntax` inflate entire enum
2. **Value embedded in CoreExpr**: `Literal(Value)`, `Quote(Value)` carry 104-byte payload
3. **ScopeSet uses HashSet**: 48 bytes for what's often 0-3 scopes
4. **No value interning**: Symbols, small integers allocated repeatedly
5. **Defensive cloning**: Many `.clone()` calls could use references or Rc

---

## 3. Optimization Roadmap

### Phase 1: Box Large Variants (Low Risk, High Impact)

**Goal:** Reduce Value from 104 to ~24 bytes, CoreExpr from 168 to ~48 bytes

**Changes:**

```rust
// value.rs - Box the large variants
pub enum Value {
    // ... small variants unchanged ...

    // Box large variants
    Procedure(Box<Procedure>),      // 8 bytes instead of 104
    Identifier(Box<IdentifierData>), // 8 bytes instead of 64
    Parameter(Box<ParameterData>),   // 8 bytes instead of 32
}

pub struct IdentifierData {
    pub name: Rc<str>,
    pub scopes: ScopeSet,
}

pub struct ParameterData {
    pub values: Rc<RefCell<Vec<Value>>>,
    pub converter: Option<Box<Value>>,
}
```

```rust
// core_expr.rs - Box large variants and use Rc for Values
pub enum CoreExpr {
    Literal(Rc<Value>),           // 8 bytes instead of 104
    Quote(Rc<Value>),             // 8 bytes instead of 104
    Quasiquote(Rc<Value>),        // 8 bytes instead of 104

    // Box DefineSyntax
    DefineSyntax(Box<DefineSyntaxData>), // 8 bytes instead of 168

    // ... other variants ...
}

pub struct DefineSyntaxData {
    pub name: Symbol,
    pub transformer: Value,
    pub definition_scopes: ScopeSet,
}
```

**Expected Results:**
- Value: 104 → ~32 bytes (3x reduction)
- CoreExpr: 168 → ~48 bytes (3.5x reduction)

**Effort:** 1-2 days
**Risk:** Low (mechanical refactor)

---

### Phase 2: Optimize ScopeSet (Medium Risk, Medium Impact)

**Goal:** Reduce ScopeSet from 48 to 8-16 bytes

**Problem:** ScopeSet uses `HashSet<ScopeId>` which is 48 bytes, but typically holds 0-3 scopes.

**Options:**

**Option A: SmallVec-based (Recommended)**
```rust
use smallvec::SmallVec;

pub struct ScopeSet {
    // Inline storage for up to 3 scopes, heap for more
    scopes: SmallVec<[ScopeId; 3]>,  // 32 bytes
}
```

**Option B: Bitset for small scope IDs**
```rust
pub struct ScopeSet {
    // If all scopes fit in 64 bits, use inline bitset
    // Otherwise fall back to heap
    repr: ScopeSetRepr,
}

enum ScopeSetRepr {
    Inline(u64),           // Bitset for scopes 0-63
    Heap(Box<HashSet<ScopeId>>),
}
```

**Option C: Rc-shared scope sets**
```rust
// Most identifiers share the same scope set
pub struct ScopeSet(Rc<HashSetInner>);

// Interning: return existing Rc if scope set already exists
impl ScopeSet {
    pub fn intern(scopes: HashSet<ScopeId>) -> Self {
        SCOPE_SET_INTERNER.intern(scopes)
    }
}
```

**Expected Results:**
- ScopeSet: 48 → 16-32 bytes (1.5-3x reduction)
- Identifier variant: 64 → 24-40 bytes

**Effort:** 2-3 days
**Risk:** Medium (semantics must be preserved)

---

### Phase 3: Reduce Clone Frequency (Medium Risk, High Impact)

**Goal:** Eliminate unnecessary clones in hot paths

**3.1 Use Rc<CoreExpr> in TailCall:**
```rust
pub(crate) enum CoreEvalResult {
    Value(Value),
    TailCall {
        expr: Rc<CoreExpr>,  // Share instead of clone
        env: Rc<Environment>,
    },
}
```

**3.2 Evaluate by reference where possible:**
```rust
// Instead of:
fn eval_core(expr: &CoreExpr, ...) {
    let mut current_expr = expr.clone();  // 168 bytes!
    loop {
        match eval_core_step(&current_expr, ...) { ... }
    }
}

// Use Rc sharing:
fn eval_core(expr: Rc<CoreExpr>, ...) {
    let mut current_expr = expr;  // Just Rc bump
    loop {
        match eval_core_step(&current_expr, ...) { ... }
    }
}
```

**3.3 Lazy desugaring for lambda bodies:**
```rust
pub enum LambdaBody {
    // Don't desugar until first call
    Lazy(Rc<Vec<Value>>),
    // Cached desugared form
    Desugared(Rc<Vec<CoreExpr>>),
}
```

**Expected Results:**
- Trampoline loop: 168 bytes → 8 bytes per iteration
- Lambda creation: avoid body cloning entirely

**Effort:** 3-4 days
**Risk:** Medium (must preserve semantics)

---

### Phase 4: Value Interning (Medium Risk, Medium Impact)

**Goal:** Share common values, reduce allocations

**4.1 Symbol interning:**
```rust
thread_local! {
    static SYMBOL_INTERNER: RefCell<HashMap<String, Rc<str>>> = ...;
}

impl Value {
    pub fn symbol(name: &str) -> Value {
        SYMBOL_INTERNER.with(|interner| {
            let mut map = interner.borrow_mut();
            let rc = map.entry(name.to_string())
                .or_insert_with(|| Rc::from(name))
                .clone();
            Value::Symbol(rc)
        })
    }
}
```

**4.2 Small integer caching:**
```rust
// Pre-allocate common integers
lazy_static! {
    static ref SMALL_INTEGERS: [Value; 256] = {
        let mut arr = [Value::Unspecified; 256];
        for i in -128..128 {
            arr[(i + 128) as usize] = Value::Integer(i as i64);
        }
        arr
    };
}

impl Value {
    pub fn integer(n: i64) -> Value {
        if n >= -128 && n < 128 {
            SMALL_INTEGERS[(n + 128) as usize].clone()
        } else {
            Value::Integer(n)
        }
    }
}
```

**Expected Results:**
- Symbol allocation: eliminated for repeated symbols
- Integer allocation: eliminated for -128..127

**Effort:** 2-3 days
**Risk:** Low

---

### Phase 5: Tagged Pointers (High Risk, High Impact) - VM-Ready Design

**Goal:** Reduce Value to 8 bytes using NaN-boxing or tagged pointers, designed for reuse in `patina-vm`.

#### Prerequisites

This phase should **NOT** begin until:
1. ✅ Phases 1-4 complete (Value ≤ 32 bytes, clone overhead reduced)
2. ✅ Tree-walker performance baselined with benchmarks
3. ✅ `patina-vm` architecture designed (bytecode format, stack layout)
4. ✅ Benchmarks demonstrate Phase 1-4 gains plateau

**Rationale:** Tagged pointers are a fundamental representation change. Attempting this before simpler optimizations wastes effort and makes debugging harder. The VM design must inform the tagging scheme.

#### Why Tagged Pointers Are Critical for patina-vm

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

#### Architecture Decision: Shared Value vs Backend-Specific

**Option A: Shared Tagged Value in patina-core (Recommended)**

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

**Option B: Backend-Specific Values (Not Recommended for Patina)**

```
patina-core::BoxedValue    ← For API boundaries, FFI
patina-vm::VmValue         ← NaN-boxed for bytecode
patina-jit::JitValue       ← Register-optimized
```

**Pros:** Each backend fully optimized
**Cons:** Conversion overhead, code duplication, complexity

**Decision:** Use Option A. Patina is an educational project; simplicity and a unified model outweigh marginal per-backend gains.

#### Tagging Scheme Design (VM-Ready)

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

#### VM-Specific Considerations

**1. Stack Layout Compatibility:**
```rust
// VM stack is just Vec<Value> - each slot is 8 bytes
struct VmStack {
    slots: Vec<Value>,  // 8 bytes per slot, cache-friendly
    sp: usize,
}
```

**2. Bytecode Operand Encoding:**
```rust
// Bytecode can embed small constants directly
enum Bytecode {
    LoadImmediate(Value),  // 8-byte immediate fits in instruction
    LoadConst(u16),        // Index into constant pool for larger values
    Add, Sub, Mul, Div,    // Operate on stack
    // ...
}
```

**3. GC Integration Points:**
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

**4. Platform Considerations:**
- Assumes 64-bit platform (48-bit address space on x86_64, ARM64)
- Pointer tagging uses low bits (requires aligned allocations)
- Alternative: NaN-boxing for better float performance (trade-off: complex encoding)

#### Migration Path

1. **Create `patina-core::tagged` module** with new Value type
2. **Implement conversion traits** between old and new Value
3. **Migrate patina-tree-walker** first (simpler, validates correctness)
4. **Build patina-vm** using tagged Value from start
5. **Remove old Value** once migration complete

#### Expected Results

| Metric | Before Phase 5 | After Phase 5 |
|--------|----------------|---------------|
| `size_of::<Value>()` | ~32 bytes (after Phase 1) | 8 bytes |
| Cons cell total | ~80 bytes | 24 bytes |
| Type check | Branch + match | Single AND + compare |
| Stack slot | 32 bytes | 8 bytes |
| Fixnum arithmetic | Box/unbox overhead | Direct CPU ops |
| VM dispatch overhead | High | Minimal |

**Performance Targets:**
- `fibonacci(35)` tree-walker: 10x faster than Phase 4
- `fibonacci(35)` VM: 100x faster than current tree-walker
- Memory usage: 4x reduction for typical programs

#### Effort and Risk

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

## 4. Desired End State

### 4.1 Target Type Sizes

| Type | Current | Phase 1 | Phase 2 | Phase 5 |
|------|---------|---------|---------|---------|
| Value | 104 | 32 | 24 | 8 |
| CoreExpr | 168 | 48 | 40 | 32 |
| ScopeSet | 48 | 48 | 16 | 16 |
| Cons cell | 232 | 80 | 64 | 24 |

### 4.2 Performance Targets

| Metric | Current | Target |
|--------|---------|--------|
| fibonacci(30) | ~30s | <5s (Phase 4), <0.5s (with VM) |
| Memory per cons cell | 232 bytes | <32 bytes |
| Clone cost (Value) | 104 bytes | 8 bytes |
| Clone cost (CoreExpr) | 168 bytes | 32 bytes |
| Cache lines per Value | 2 | 1 |

### 4.3 Code Quality Targets

- No `clippy::large_enum_variant` warnings
- All hot path clones justified with comments
- Size regression tests in CI

---

## 5. Implementation Order

### Recommended Sequence

```
Phase 1 (Week 1): Box Large Variants
├── Box Value::Procedure → Value: 104 → 56 bytes
├── Box Value::Identifier → Value: 56 → 32 bytes
├── Use Rc<Value> in CoreExpr → CoreExpr: 168 → 48 bytes
└── Tests passing, benchmarks baseline

Phase 2 (Week 2): Optimize ScopeSet
├── Implement SmallVec-based ScopeSet
├── Update Identifier to use new ScopeSet
└── Verify macro hygiene still works

Phase 3 (Week 2-3): Reduce Clones
├── Rc<CoreExpr> in TailCall
├── Evaluate by Rc sharing
├── Profile and verify improvements
└── Document remaining necessary clones

Phase 4 (Week 3): Value Interning
├── Symbol interner
├── Small integer cache
└── Benchmark improvements

         ════════════════════════════════════════
         GATE: Phases 1-4 must be complete before Phase 5
         GATE: patina-vm architecture must be designed
         ════════════════════════════════════════

Phase 5 (Coordinated with patina-vm): Tagged Pointers
├── Prerequisite: patina-vm bytecode format designed
├── Prerequisite: VM stack layout determined
├── Design tagging scheme (VM-optimized)
├── Implement in patina-core::tagged module
├── Migrate tree-walker first (validation)
├── Build patina-vm using tagged Value
└── Remove old Value type
```

### Phase Dependencies

```
                    ┌─────────────┐
                    │   Phase 1   │  Box Large Variants
                    │  (Week 1)   │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │   Phase 2   │  Optimize ScopeSet
                    │  (Week 2)   │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │   Phase 3   │  Reduce Clones
                    │ (Week 2-3)  │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │   Phase 4   │  Value Interning
                    │  (Week 3)   │
                    └──────┬──────┘
                           │
         ══════════════════╪════════════════════
                           │ GATE: Phases 1-4 complete
         ══════════════════╪════════════════════
                           │
    ┌──────────────────────┼──────────────────────┐
    │                      │                      │
    ▼                      ▼                      │
┌────────────┐      ┌─────────────┐              │
│ patina-vm  │      │   Phase 5   │◄─────────────┘
│  Design    │─────►│   Tagged    │  Requires VM design
│            │      │  Pointers   │  to inform tagging
└────────────┘      └─────────────┘
```

### Success Criteria

**Phase 1 Complete When:**
- [ ] `size_of::<Value>() <= 32`
- [ ] `size_of::<CoreExpr>() <= 48`
- [ ] All tests passing
- [ ] No performance regressions

**Phase 2 Complete When:**
- [ ] `size_of::<ScopeSet>() <= 24`
- [ ] Macro hygiene tests passing
- [ ] Memory usage reduced in macro-heavy code

**Phase 3 Complete When:**
- [ ] Trampoline loop clones < 32 bytes
- [ ] Profile shows reduced allocation pressure
- [ ] No unnecessary `.clone()` in hot paths

**Phase 4 Complete When:**
- [ ] Symbol interning active
- [ ] Small integer caching active
- [ ] Allocation benchmarks improved
- [ ] Baseline benchmarks documented for Phase 5 comparison

**Phase 5 Entry Criteria:**
- [ ] Phases 1-4 complete and stable
- [ ] `patina-vm` bytecode format finalized
- [ ] `patina-vm` stack layout determined
- [ ] Performance baseline established (tree-walker with Phases 1-4)
- [ ] Tagged pointer tagging scheme reviewed and approved

**Phase 5 Complete When:**
- [ ] `size_of::<Value>() == 8`
- [ ] All tree-walker tests passing with tagged Value
- [ ] `patina-vm` operational with tagged Value
- [ ] Performance targets met (see Phase 5 section)
- [ ] Old Value type removed from codebase

---

## 6. Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking macro hygiene | Medium | High | Extensive test coverage, incremental changes |
| Performance regression | Low | Medium | Benchmark before/after each phase |
| Increased complexity | Medium | Low | Document patterns, add comments |
| Memory leaks from interning | Low | Medium | Use weak references or bounded caches |
| Phase 5 tagging incompatible with VM | Medium | High | Design VM first, let VM inform tagging scheme |
| Platform-specific tagged pointer issues | Medium | Medium | CI on multiple platforms, abstract behind API |
| GC integration difficulties | Medium | High | Reserve tag bits for GC, design with GC in mind |

---

## 7. References

- `PRD/phase1/ARCHITECTURE_CRITIQUE_AND_ROADMAP.md` - Issue 2: Value Enum Too Large
- `PRD/ARCHIVE/ARCHITECTURE_REVIEW.md` - Issue 5: Value/Environment Cyclic References
- `crates/patina-core/src/value.rs` - Current Value implementation
- `crates/patina-core/src/core_expr.rs` - Current CoreExpr implementation
- `crates/patina-tree-walker/src/eval/core_eval.rs` - Evaluator hot paths

---

## Appendix A: Size Measurement Command

```bash
cd ~/Project/patina
RUSTFLAGS="-Zprint-type-sizes" cargo +nightly build --package patina-core 2>&1 \
  | grep -E "Value|CoreExpr|Procedure|ScopeSet|LambdaBody"
```

## Appendix B: Benchmark Command (TODO)

```bash
# Add benchmark suite for size optimization
cargo bench --package patina-tests -- value_size
```
