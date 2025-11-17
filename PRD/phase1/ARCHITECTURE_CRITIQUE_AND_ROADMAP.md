# Architecture Critique & Multi-Backend Roadmap

**Date:** 2025-11-16
**Author:** Technical Review from Compiler/Interpreter Engineering Perspective
**Purpose:** Critique current architecture and provide roadmap for multi-backend, debuggable, extensible system

---

## Executive Summary

**Current State:** ✅ **Solid foundation, production-ready for single backend**
- Clean crate separation
- Working tree-walker with TCO
- Good test coverage (435 tests)
- 57.9% R7RS compliance

**Critical Issues for Multi-Backend Vision:**
- ❌ **No IR layer** - AST is directly evaluated, makes VM/JIT backends impossible
- ❌ **Value enum too large** - Performance cliff when moving to VM
- ⚠️ **No source location tracking** - Debugging nightmare
- ⚠️ **Environment representation inefficient** - Will bottleneck VM
- ⚠️ **Macro expansion happens too early** - Can't debug original code

**Recommended Path:**
1. **Phase 2A (2-3 weeks):** Add source locations + improve error messages (immediate value!)
2. **Phase 2B (3-4 weeks):** Introduce IR layer (enables multi-backend)
3. **Phase 2C (4-6 weeks):** Build bytecode VM (performance + easier debugging)
4. **Phase 3 (8-12 weeks):** JIT compiler (performance ceiling)

---

## 1. Current Architecture Analysis

### 1.1 What's Working Well ✅

**Excellent Crate Separation:**
```
patina-runtime      → Core types (Value, Environment)
patina-frontend     → Lexer, Parser, Macro Expander
patina-tree-walker  → Tree-walking interpreter
patina-interpreter  → High-level API
patina-repl         → User interface
```

**Strengths:**
- Clear boundaries and responsibilities
- `patina-runtime` is backend-agnostic (good!)
- Frontend can be reused across backends
- Test isolation works well

**Strong Foundations:**
- ✅ Proper tail call optimization (trampoline pattern)
- ✅ Hygienic macro system
- ✅ Library system with multi-namespace support
- ✅ Comprehensive test coverage
- ✅ Good documentation

### 1.2 Critical Architecture Issues ❌

#### Issue 1: No IR Layer (CRITICAL for multi-backend)

**Problem:**
```rust
// Current flow:
Source → Lexer → Parser → Value (AST) → Tree-Walker eval() → Result
                                ↑
                          This IS the AST!
```

**Why this blocks multi-backend:**
- VM needs bytecode, not AST
- JIT needs typed IR, not dynamic Value enum
- Can't optimize without IR passes
- Can't have multiple backends share compilation pipeline

**What's needed:**
```rust
// Desired flow:
Source → Lexer → Parser → AST
                            ↓
                       Macro Expand
                            ↓
                         IR (typed)
                         ↙    ↓    ↘
              Tree-Walk  Bytecode  JIT
                (debug)   (fast)   (fastest)
```

**Impact:** **BLOCKS** all multi-backend work. Must fix for Phase 2.

---

#### Issue 2: Value Enum Too Large (Performance Cliff)

**Current Value enum:**
```rust
pub enum Value {
    Boolean(bool),           // 1 byte
    Integer(i64),            // 8 bytes
    BigInteger(BigInt),      // heap allocated
    Rational(BigRational),   // heap allocated
    Real(f64),               // 8 bytes
    Complex(f64, f64),       // 16 bytes
    String(Rc<RefCell<...>>) // 16 bytes (ptr + refcount)
    Symbol(Rc<str>),         // 16 bytes
    Pair(Rc<...>),           // 16 bytes
    Vector(Rc<RefCell<...>>) // 16 bytes
    Procedure(Procedure),    // Large nested enum
    Macro { ... },           // 32+ bytes
    // ... 15+ variants total
}
```

**Problems:**

1. **Size:** `sizeof(Value)` is likely **48-64 bytes** (enum discriminant + largest variant)
   - Every small integer takes 64 bytes!
   - Stack frames are huge
   - Cache misses everywhere

2. **Clone cost:** Cloning a Value clones the entire enum even if it's just an integer
   - Tree-walker does this constantly

3. **Pattern matching cost:** Compiler generates large match jump tables

**Industry standard (V8, SpiderMonkey, Chez Scheme):**
- Tagged pointers: 64-bit value encodes type + data
- Small integers: encoded in pointer bits (no allocation)
- `sizeof(Value)` = 8 bytes always
- Clone is memcpy (cheap)

**Recommendation:**
```rust
// Tagged pointer representation (NaN boxing or pointer tagging)
#[repr(transparent)]
pub struct Value(u64);

// Small integers, bools, chars: encoded directly in u64
// Everything else: pointer to heap allocation
```

**Impact:** 8x memory reduction, 5-10x performance improvement for VM

**Urgency:** Medium - Tree-walker works fine with current design, but VM will need this

---

#### Issue 3: No Source Location Tracking (Debuggability Crisis)

**Current error messages:**
```
Error: Undefined variable: foo
```

**What's missing:**
- No line numbers
- No column numbers
- No source file
- No call stack trace
- No snippet of source code

**Industry standard:**
```
Error: Undefined variable 'foo'
  --> myfile.scm:42:10
   |
42 |   (define (bar) (+ foo 1))
   |                    ^^^ undefined variable
   |
Stack trace:
  at bar (myfile.scm:42:10)
  at main (myfile.scm:50:3)
```

**What's needed:**

1. **Source locations in AST:**
```rust
pub struct Span {
    file: Arc<str>,
    start: usize,  // byte offset
    end: usize,
}

pub enum Expr {
    Symbol { name: String, span: Span },
    List { items: Vec<Expr>, span: Span },
    // ... all variants have spans
}
```

2. **Call stack tracking:**
```rust
pub struct CallFrame {
    name: String,
    location: Span,
    locals: HashMap<String, Value>,
}

pub struct CallStack {
    frames: Vec<CallFrame>,
}
```

3. **Source map for macros:**
   - Track original source before macro expansion
   - Show both expanded and original code in errors

**Urgency:** **HIGH** - This is user-facing and hurts adoption

**Effort:** 1-2 weeks to add spans everywhere, 1 week for call stack

---

#### Issue 4: Environment Representation (Performance Bottleneck)

**Current:**
```rust
pub struct Environment {
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    parent: Option<Rc<Environment>>,
}
```

**Problems:**

1. **RefCell runtime overhead:** Every variable access checks borrow rules at runtime
2. **HashMap overhead:** O(log n) lookup per variable access (robin hood hashing)
3. **Clone cost:** Cloning environment clones Rc (cheap) but HashMap access is slow
4. **Cache unfriendly:** Hash map jumps around memory

**For tree-walker:** This is fine! Not a bottleneck.

**For VM:** This will be **disastrous** - VMs do millions of variable accesses/sec

**VM needs:**
```rust
// Compile-time environment (for compiler)
pub struct StaticEnv {
    locals: Vec<(String, LocalIndex)>,  // Vector for fast lookup
    parent: Option<Rc<StaticEnv>>,
}

// Runtime environment (for VM)
pub struct Frame {
    locals: Vec<Value>,  // Direct array indexing - O(1)
    parent: Option<usize>, // Index into call stack, not Rc!
}
```

**Recommendation:**
- Keep current Environment for tree-walker
- Add StaticEnv + Frame for VM backend
- Don't try to unify them (different needs)

**Urgency:** Low for now, critical for VM

---

#### Issue 5: Macro Expansion Timing (Debuggability)

**Current flow:**
```
Parser → Value (with macros) → Macro Expand → Value (expanded) → Eval
                                    ↑
                            Happens here, source lost!
```

**Problem:** Once macros are expanded, original source code is gone
- Can't show user their original code in errors
- Can't step through macro expansion in debugger
- Can't implement macro-expand-1 for debugging

**Better flow:**
```
Parser → AST (with macros) → eval
                              ↓
                      (expand lazily during eval)
                              ↓
                    Keep source map: expanded → original
```

**Alternative:** Keep macro expansion early but track provenance
```rust
pub struct Expr {
    kind: ExprKind,
    span: Span,           // Where is this in source?
    macro_origin: Option<MacroExpansion>, // Was this macro-generated?
}

pub struct MacroExpansion {
    macro_name: String,
    original_call: Span,  // Where was the macro called?
    expansion_id: usize,
}
```

**Urgency:** Medium - Nice for debugging, not critical

---

### 1.3 Minor Issues ⚠️

**1. Parser Creates Value Directly**
- Should create proper AST type
- Value is runtime representation, not compile-time

**2. No Type Inference/Analysis**
- Can't optimize without knowing types
- JIT needs type feedback

**3. No Module Compilation Cache**
- Re-parse and re-macro-expand on every run
- Should cache to `.pbc` (Patina Bytecode) files

**4. Test Framework**
- Great coverage, but slow (435 tests take ~2 seconds)
- VM will be 10-100x faster for tests

**5. String Representation**
- `String(Rc<RefCell<String>>)` is correct for R7RS but slow
- Consider immutable strings by default (most strings never mutate)

---

## 2. Path to Multi-Backend

### Phase 2A: Foundation (2-3 weeks) - **START HERE**

**Goal:** Improve debuggability, prepare for IR

**Tasks:**

**1. Add Source Locations (1 week)**
```rust
// New AST type (separate from Value)
pub enum Expr {
    Symbol(String, Span),
    Number(Number, Span),
    List(Vec<Expr>, Span),
    // ...
}

pub struct Span {
    source_id: usize,       // Index into source map
    start: usize,
    end: usize,
}

pub struct SourceMap {
    files: Vec<SourceFile>,
}

pub struct SourceFile {
    path: PathBuf,
    content: String,
    lines: Vec<usize>,  // Byte offsets of line starts
}
```

**2. Separate AST from Value (1 week)**
- Parser returns `Expr`, not `Value`
- Evaluator converts `Expr → Value` during evaluation
- Keeps compilation and runtime separated

**3. Add Call Stack Tracking (3-5 days)**
```rust
pub struct CallStack {
    frames: Vec<CallFrame>,
    max_depth: usize,
}

pub struct CallFrame {
    name: String,
    location: Span,
    env: Rc<Environment>,
}
```

**4. Improve Error Messages (2-3 days)**
- Show source location
- Show call stack
- Show code snippet with highlighting

**Benefits:**
- ✅ Massively better debugging (user-facing value!)
- ✅ Prepares for IR layer
- ✅ Better error messages → better developer experience

**Effort:** 2-3 weeks
**Value:** **HIGH** - Immediate user impact

---

### Phase 2B: IR Layer (3-4 weeks)

**Goal:** Introduce typed IR to enable multiple backends

**Design:**

```rust
// High-level IR (desugared Scheme)
pub enum HIR {
    Const(Constant, Span),
    Var(String, Span),
    Lambda {
        params: Vec<String>,
        variadic: Option<String>,
        body: Vec<HIR>,
        span: Span,
    },
    Apply {
        func: Box<HIR>,
        args: Vec<HIR>,
        tail_call: bool,  // TCO marker
        span: Span,
    },
    If {
        cond: Box<HIR>,
        then_branch: Box<HIR>,
        else_branch: Box<HIR>,
        span: Span,
    },
    Define(String, Box<HIR>, Span),
    Set(String, Box<HIR>, Span),
    Begin(Vec<HIR>, Span),
    // All special forms desugared to core forms
}

// Later: Medium IR (typed, optimized)
pub enum MIR {
    // After type inference, optimizations
    // Suitable for VM bytecode generation
}

// Later: Low IR (bytecode or machine code)
pub enum LIR {
    // Bytecode instructions or assembly
}
```

**Compilation Pipeline:**
```
Source → Lexer → Parser → Expr (AST)
                            ↓
                      Macro Expand → Expr (expanded)
                            ↓
                      AST → HIR (desugar)
                            ↓
                  HIR → MIR (optimize, infer types)
                            ↓
                        Backend Choice:
                       ↙        ↓        ↘
                Tree-Walk   Bytecode    JIT
                 (HIR)       (LIR)      (Machine)
```

**Key Decisions:**

1. **Keep tree-walker on HIR:** Don't convert everything to bytecode yet
   - Tree-walker can work directly with HIR
   - Simpler, easier to debug
   - Good for REPL

2. **Add VM backend on LIR:** Once IR is stable
   - Compile HIR → Bytecode (LIR)
   - Run bytecode in register VM
   - 10-100x faster than tree-walker

3. **IR is immutable:** All transforms create new IR
   - Easier to debug
   - Enables parallel compilation later

**Benefits:**
- ✅ Enables multiple backends
- ✅ Cleaner separation of concerns
- ✅ Optimization opportunities
- ✅ Type inference possible

**Effort:** 3-4 weeks
**Value:** **CRITICAL** for multi-backend vision

---

### Phase 2C: Bytecode VM (4-6 weeks)

**Goal:** Build fast bytecode VM for production workloads

**Design:**

**1. Register-based VM (not stack-based)**
   - Modern VMs (Lua, Python 3.11+) use registers
   - Fewer instructions, better perf
   - Easier to optimize

```rust
pub enum Opcode {
    // Load/Store
    LoadConst(Register, ConstIndex),
    LoadGlobal(Register, SymbolIndex),
    StoreGlobal(SymbolIndex, Register),

    // Arithmetic
    Add(Register, Register, Register),  // r0 = r1 + r2
    Sub(Register, Register, Register),
    Mul(Register, Register, Register),

    // Control flow
    Jump(Label),
    JumpIfFalse(Register, Label),
    Call(Register, ArgCount, Register), // call r0 with n args, store in r_result
    TailCall(Register, ArgCount),
    Return(Register),

    // Closure
    MakeClosure(Register, FunctionIndex, CaptureList),

    // Type checks (for optimization)
    IsInteger(Register, Label),  // Jump if r is int

    // ...
}

pub struct Function {
    name: String,
    arity: Arity,
    bytecode: Vec<Opcode>,
    constants: Vec<Value>,
    num_registers: usize,
    source_map: Vec<(usize, Span)>,  // bytecode offset → source location
}

pub struct VM {
    call_stack: Vec<Frame>,
    globals: HashMap<Symbol, Value>,
    functions: Vec<Function>,
}

pub struct Frame {
    function: usize,       // Index into VM.functions
    pc: usize,             // Program counter
    registers: Vec<Value>, // Local registers
    caller_frame: usize,   // For stack traces
}
```

**2. Compiler: HIR → Bytecode**
```rust
pub struct BytecodeCompiler {
    current_function: Function,
    register_allocator: RegisterAllocator,
    env: StaticEnv,  // Compile-time env (maps vars → registers)
}

impl BytecodeCompiler {
    fn compile_hir(&mut self, hir: &HIR) -> Register {
        match hir {
            HIR::Const(c, _) => {
                let reg = self.alloc_register();
                let const_idx = self.add_constant(c);
                self.emit(Opcode::LoadConst(reg, const_idx));
                reg
            }
            HIR::Apply { func, args, tail_call, .. } => {
                let func_reg = self.compile_hir(func);
                let arg_regs: Vec<_> = args.iter()
                    .map(|arg| self.compile_hir(arg))
                    .collect();
                let result_reg = self.alloc_register();

                if *tail_call {
                    self.emit(Opcode::TailCall(func_reg, arg_regs.len()));
                } else {
                    self.emit(Opcode::Call(func_reg, arg_regs.len(), result_reg));
                }
                result_reg
            }
            // ...
        }
    }
}
```

**3. Optimizations:**
   - **Constant folding:** `(+ 1 2)` → `3` at compile time
   - **Type specialization:** Fast path for integer arithmetic
   - **Inline small functions:** Eliminate call overhead
   - **Dead code elimination:** Remove unused code

**Benefits:**
- ✅ 10-100x faster than tree-walker
- ✅ Lower memory usage
- ✅ Better for production workloads
- ✅ Still debuggable (source maps)

**Effort:** 4-6 weeks
**Value:** **HIGH** - Production performance

---

### Phase 3: JIT Compiler (8-12 weeks) - Future

**Goal:** Native code generation for maximum performance

**Options:**

**1. Cranelift (Recommended)**
- Rust-based code generator
- Used by Wasmtime, SpiderMonkey
- Good performance, reasonable compile times
- Safe, no C++ dependencies

**2. LLVM**
- Best performance (V8, Julia level)
- Slow compile times (JIT latency)
- Large dependency

**3. Custom x64 backend**
- Maximum control
- 8-12 weeks of work
- Hard to maintain

**Recommendation:** Start with Cranelift

```rust
// Simplified JIT design
pub struct JITCompiler {
    cranelift_ctx: Context,
    compiled_functions: HashMap<Symbol, *const u8>,
}

impl JITCompiler {
    fn compile_function(&mut self, hir: &HIR) -> *const u8 {
        // HIR → Cranelift IR → Machine code
        let clif_ir = self.hir_to_cranelift(hir);
        let optimized = optimize(clif_ir);
        let native_code = self.cranelift_ctx.compile(optimized);
        native_code.as_ptr()
    }
}
```

**Benefits:**
- ✅ Near-native performance (C/C++ level)
- ✅ Competitive with Chez, Racket, Gambit
- ✅ Can optimize hot loops

**Effort:** 8-12 weeks
**Value:** Medium - Nice for benchmarks, overkill for most users

**Urgency:** LOW - VM is good enough for Phase 1

---

## 3. Debuggability Roadmap

### 3.1 Immediate (Phase 2A)

**Source Locations (1 week) - TOP PRIORITY**
```rust
Error: Undefined variable 'foo'
  --> test.scm:42:10
   |
42 |   (define (bar) (+ foo 1))
   |                    ^^^ undefined variable
```

**Call Stack Traces (3-5 days)**
```rust
Stack trace:
  at factorial (math.scm:15:5)
  at main (math.scm:25:3)
```

**Impact:** Massively better developer experience

---

### 3.2 Medium-term (Phase 2B-C)

**Interactive Debugger (2-3 weeks)**
```scheme
patina> (debug (factorial 5))
Breaking at math.scm:15:5
15|   (if (= n 0)

debug> step          ; Step into
debug> next          ; Step over
debug> continue      ; Continue
debug> print n       ; Inspect variable
debug> backtrace     ; Show stack
debug> frame 2       ; Switch stack frame
```

**Breakpoints (1 week)**
```scheme
(set-breakpoint! 'factorial)  ; Break on function entry
(set-breakpoint! "math.scm" 15)  ; Break on line
```

**Time-travel Debugging (3-4 weeks) - Advanced**
- Record all evaluation steps
- Step backwards in time
- Inspect any historical state
- Like rr for Scheme

**Impact:** Best-in-class debugging for Scheme

---

### 3.3 Long-term (Phase 3+)

**Profiler**
```scheme
(profile (run-benchmark))
; Shows:
; - Time per function
; - Call counts
; - Allocation hot spots
```

**Trace Viewer**
```scheme
(trace 'factorial)
(factorial 5)
; Prints:
; > (factorial 5)
;   > (factorial 4)
;     > (factorial 3)
;     < 6
;   < 24
; < 120
```

---

## 4. Extensibility & Plugin System

### 4.1 Current Extensibility

**What works:**
- ✅ Easy to add primitives (register in registry)
- ✅ Library system allows organized extensions
- ✅ Clean FFI boundary (Rust functions → Scheme)

**What's hard:**
- ❌ No way for users to add primitives without recompiling
- ❌ No dynamic loading of extensions
- ❌ No stable ABI

---

### 4.2 Plugin System Design

**Goal:** Allow users to extend Patina without forking

**Design 1: Rust Plugins (compile-time)**
```toml
# Cargo.toml
[dependencies]
patina-plugin-api = "0.1"

[lib]
crate-type = ["cdylib"]  # Dynamic library
```

```rust
// my-plugin/src/lib.rs
use patina_plugin_api::*;

#[plugin_export]
pub fn my_sqrt(args: &[Value]) -> Result<Value, Error> {
    let n = args[0].as_f64()?;
    Ok(Value::Real(n.sqrt()))
}

#[plugin_init]
pub fn init(registry: &mut PrimitiveRegistry) {
    registry.register("my-plugin/sqrt", my_sqrt, Arity::Exact(1));
}
```

**Loading:**
```scheme
(import (plugin "libmy_plugin.so"))
(my-plugin/sqrt 4.0)  ; => 2.0
```

**Benefits:**
- ✅ Native performance
- ✅ Safe (uses Rust safety)
- ✅ Easy to distribute (cargo package)

**Challenges:**
- ❌ Requires compilation
- ❌ Platform-specific binaries
- ❌ ABI stability

---

**Design 2: Wasm Plugins (portable)**
```rust
// Compile plugin to Wasm
wasm-pack build my-plugin

// Load in Patina
(import (wasm-plugin "my_plugin.wasm"))
```

**Benefits:**
- ✅ Platform-independent
- ✅ Sandboxed (safe)
- ✅ Easy to distribute

**Challenges:**
- ❌ Slower than native
- ❌ Wasm runtime overhead

---

**Recommendation:**
- Phase 2: Support Rust dynamic plugins (cdylib)
- Phase 3: Add Wasm support if needed
- Provide stable plugin API in `patina-plugin-api` crate

---

## 5. Performance Optimization Strategy

### 5.1 Current Performance

**Tree-walker:**
- ~1000x slower than C
- Fine for REPL, scripts, tests
- Not suitable for heavy computation

**Benchmark (fibonacci(30)):**
```
C:           0.03s
Chez:        0.05s (JIT)
Racket:      0.08s (JIT)
Patina:      ~30s (estimated, tree-walker)
```

---

### 5.2 Performance Roadmap

**Phase 1: Tree-walker optimizations (2-3 days)**
- Use `&Value` instead of `Value` where possible (avoid clones)
- Inline small primitives (+, -, *, /)
- Cache symbol lookups
- Expected gain: 2-3x faster

**Phase 2B: Bytecode VM (4-6 weeks)**
- 10-100x faster than tree-walker
- Expected: `fibonacci(30)` in ~3 seconds

**Phase 2C: Type specialization (2-3 weeks)**
- Fast paths for integer arithmetic
- Expected: Additional 5-10x on numeric code

**Phase 3: JIT (8-12 weeks)**
- Near-native performance
- Expected: `fibonacci(30)` in ~0.1 seconds (competitive with Chez)

---

### 5.3 Memory Optimization

**Current:**
- `sizeof(Value)` = 48-64 bytes
- Every integer is heap-allocated via Rc

**Optimized (NaN boxing):**
- `sizeof(Value)` = 8 bytes
- Small integers encoded in pointer
- 8x less memory, better cache locality

**Implementation:**
```rust
const TAG_INTEGER: u64 = 0x0001;
const TAG_BOOLEAN: u64 = 0x0002;
const TAG_CHAR: u64 = 0x0003;
const TAG_POINTER: u64 = 0xFFFC_0000_0000_0000;

#[repr(transparent)]
pub struct Value(u64);

impl Value {
    pub fn from_integer(n: i64) -> Self {
        if n >= i32::MIN as i64 && n <= i32::MAX as i64 {
            // Encode in place (no allocation!)
            Self((n as u64) << 16 | TAG_INTEGER)
        } else {
            // Large integer: heap allocate
            let ptr = Box::into_raw(Box::new(n));
            Self((ptr as u64) | TAG_POINTER)
        }
    }
}
```

**Effort:** 2-3 weeks
**Value:** HIGH for VM, medium for tree-walker

---

## 6. Recommended Roadmap

### Phase 1 (Current) - Complete R7RS ✅

**Timeline:** 2-3 weeks remaining
**Goals:**
- Lazy evaluation (delay/force)
- Parameter objects
- Case-lambda
- Let-syntax/letrec-syntax
- Advanced math functions
- Reach 80%+ R7RS compliance

**Status:** On track, library system done, test framework working

---

### Phase 2A - Debuggability Foundation ⭐ **START NEXT**

**Timeline:** 2-3 weeks
**Goals:**
- Add source locations to everything
- Separate AST from Value
- Call stack tracking
- Beautiful error messages

**Why first:**
- **Immediate user value** (better errors)
- Prepares for IR layer
- Makes all future work easier to debug
- Low risk, high reward

**Estimated effort:** 2-3 weeks
**Priority:** **HIGHEST**

---

### Phase 2B - IR Layer & Multi-Backend Foundation

**Timeline:** 3-4 weeks
**Goals:**
- Design and implement HIR
- Convert tree-walker to use HIR
- Macro expansion to HIR
- Basic optimizations (constant folding, dead code)

**Why second:**
- Enables all future backends
- Clean separation of concerns
- Foundation for VM

**Estimated effort:** 3-4 weeks
**Priority:** **HIGH**

---

### Phase 2C - Bytecode VM

**Timeline:** 4-6 weeks
**Goals:**
- Design register-based VM
- Implement bytecode compiler (HIR → bytecode)
- Build VM runtime
- 10-100x performance improvement

**Why third:**
- Massive performance gain
- Production-ready performance
- Better than most Scheme implementations

**Estimated effort:** 4-6 weeks
**Priority:** **MEDIUM-HIGH**

---

### Phase 2D - Interactive Debugger

**Timeline:** 2-3 weeks
**Goals:**
- Breakpoints
- Step through code
- Variable inspection
- REPL in debugger context

**Why fourth:**
- Completes debuggability story
- Best-in-class developer experience
- Enables complex development

**Estimated effort:** 2-3 weeks
**Priority:** **MEDIUM**

---

### Phase 3 - JIT Compiler (Future)

**Timeline:** 8-12 weeks
**Goals:**
- Cranelift integration
- Near-native performance
- Competitive with Chez, Racket

**Why later:**
- VM is good enough for most users
- JIT is complex, high maintenance
- Diminishing returns

**Priority:** **LOW** (nice-to-have)

---

## 7. Specific Technical Recommendations

### 7.1 Immediate Actions (This Week)

1. **Create AST type separate from Value** (1 day)
2. **Add Span to AST nodes** (2 days)
3. **Track source files in SourceMap** (1 day)
4. **Update errors to include spans** (1 day)

---

### 7.2 Short-term (Next Month)

1. **Design HIR** (3 days)
2. **Implement AST → HIR** (1 week)
3. **Update tree-walker to eval HIR** (1 week)
4. **Add basic optimizations** (3-5 days)

---

### 7.3 Medium-term (Next Quarter)

1. **Design bytecode format** (1 week)
2. **Implement bytecode compiler** (2-3 weeks)
3. **Build VM runtime** (2-3 weeks)
4. **Optimize and test** (1 week)

---

## 8. Risk Assessment

### High Risk ⚠️

**IR Design:**
- Wrong IR design will haunt you forever
- Recommendation: Study Chez, Guile, Racket IRs first
- Get this right before implementing

**NaN Boxing:**
- Subtle bugs, platform differences
- Test exhaustively
- Consider starting with simpler tagged pointers

### Medium Risk ⚠️

**VM Correctness:**
- Bytecode bugs are hard to debug
- Need comprehensive test suite
- Consider formal verification for critical opcodes

**JIT Complexity:**
- Cranelift is complex
- Budget 2-3x initial estimate
- Consider if really needed

### Low Risk ✅

**Source locations:**
- Well-understood problem
- Low risk, high value

**Call stack:**
- Simple to implement
- Tree-walker already has frames

---

## 9. Success Metrics

### Phase 2A (Debuggability)
- ✅ 100% of errors include source location
- ✅ Stack traces show 5+ frames
- ✅ Users report "best error messages in Scheme"

### Phase 2B (IR Layer)
- ✅ HIR representation covers all R7RS features
- ✅ Tree-walker performs ≥90% as fast on HIR as on AST
- ✅ IR design reviewed by experienced compiler engineers

### Phase 2C (VM)
- ✅ VM is 10-100x faster than tree-walker
- ✅ All tests pass on VM backend
- ✅ Fibonacci(30) runs in <5 seconds

### Phase 3 (JIT)
- ✅ JIT is 10x faster than VM on tight loops
- ✅ Fibonacci(30) runs in <0.5 seconds
- ✅ Competitive with Chez Scheme on benchmarks

---

## 10. Conclusion

**Current State:**
Patina has a **solid, well-architected foundation**. The tree-walker is production-ready for its use case. The crate separation is excellent. TCO works. Library system is great.

**Critical Gap:**
The lack of an IR layer is the **only blocker** for multi-backend. Everything else can be incrementally improved.

**Recommended Path:**

1. **Phase 2A (2-3 weeks):** Add source locations + better errors
   - **Immediate user value**
   - Prepares for IR
   - Low risk

2. **Phase 2B (3-4 weeks):** Introduce HIR
   - Enables multi-backend
   - Clean architecture
   - Medium risk (IR design critical)

3. **Phase 2C (4-6 weeks):** Build bytecode VM
   - Production performance
   - High value
   - Medium effort

4. **Phase 3 (later):** JIT if needed
   - Nice-to-have
   - Low priority

**Total timeline to production VM:** 10-14 weeks (~3 months)

**This is achievable and will result in a world-class Scheme implementation.** 🚀

---

## Appendix: References

**Recommended reading:**

1. **Chez Scheme Architecture** (Kent Dybvig)
   - Best-in-class Scheme compiler
   - Excellent IR design

2. **Crafting Interpreters** (Bob Nystrom)
   - Great VM design patterns
   - Bytecode compiler tutorial

3. **Modern Compiler Implementation in ML** (Andrew Appel)
   - IR design
   - Optimization passes

4. **Engineering a Compiler** (Cooper & Torczon)
   - Register allocation
   - Code generation

5. **Cranelift Documentation**
   - JIT code generation
   - Used by Wasmtime

**Reference implementations to study:**

- **Chez Scheme:** Best JIT compiler
- **Guile:** Good VM design
- **Racket:** Excellent tooling & debugging
- **ChezScheme:** IR passes
- **Lua 5.4:** Simple, fast bytecode VM
