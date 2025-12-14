# Patina VM Specification (Draft v0.1)

**Status:** Draft Design
**Target:** Phase 2 Implementation
**Design Goal:** Flexible architecture supporting incremental addition of advanced features

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Bytecode ISA Design](#bytecode-isa-design)
3. [Memory Model](#memory-model)
4. [Execution Model](#execution-model)
5. [Runtime Data Structures](#runtime-data-structures)
6. [Optimization Framework](#optimization-framework)
7. [Extensibility Points](#extensibility-points)
8. [Integration Strategy](#integration-strategy)
9. [Implementation Phases](#implementation-phases)

---

## Architecture Overview

### Core Design Principles

1. **Modularity:** Each optimization technique is an independent module
2. **Progressive Enhancement:** VM works correctly at each optimization level
3. **Backward Compatibility:** Can fall back to simpler modes
4. **Profiling Infrastructure:** Built-in from day one to support all optimizations
5. **Flexible ISA:** Bytecode can be extended without breaking existing code

### Component Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                     patina-interpreter                       │
│                    (High-level API)                          │
└─────────────────────┬───────────────────────────────────────┘
                      │
        ┌─────────────┴──────────────┐
        │                            │
┌───────▼────────┐          ┌────────▼────────┐
│ patina-frontend│          │  patina-vm      │
│ (Parsing, IR)  │          │  (This Spec)    │
└───────┬────────┘          └────────┬────────┘
        │                            │
        │                   ┌────────┴────────┐
        │                   │                 │
        │          ┌────────▼────────┐ ┌─────▼─────────┐
        │          │ VM Core         │ │ Optimizer     │
        │          │ (Interpreter)   │ │ (JIT/Trace)   │
        │          └────────┬────────┘ └─────┬─────────┘
        │                   │                 │
        └───────────────────┴─────────────────┘
                            │
                   ┌────────▼─────────┐
                   │  patina-runtime  │
                   │  (Value, Env)    │
                   └──────────────────┘
```

### Execution Modes

The VM supports multiple execution modes that can be switched dynamically:

```rust
pub enum ExecutionMode {
    /// Simple bytecode interpreter (baseline)
    Interpreter,

    /// Interpreter with profiling (preparation for optimization)
    ProfilingInterpreter,

    /// Interpreter with inline caching and specialization
    AdaptiveInterpreter,

    /// Meta-tracing JIT (traces hot paths to native code)
    TracingJIT,

    /// Debug mode (enables snapshots, time-travel)
    Debug,
}
```

**Flexibility:** Start with `Interpreter`, add capabilities incrementally without breaking changes.

---

## Bytecode ISA Design

### Design Philosophy

The ISA is designed to support:
- **Delimited continuations** as primitive (enables call/cc, exceptions, generators)
- **Type profiling** annotations on all operations
- **Guards** for speculative optimization
- **Multi-level representation** (can be lowered to simpler forms)
- **Indexed variable access** - variables resolved to indices at compile time (see [ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md) §2)

### Core Instruction Set

```rust
pub enum Opcode {
    // ─────────────────────────────────────────────────────
    // Constants & Literals
    // ─────────────────────────────────────────────────────
    LoadConst { dst: Register, constant_index: ConstantPoolIndex },
    LoadImmediate { dst: Register, value: TaggedValue },  // Small immediates inline

    // ─────────────────────────────────────────────────────
    // Variable Access (indexed for performance)
    // See ARCHITECTURE_LESSONS.md §2 for design rationale
    // ─────────────────────────────────────────────────────
    /// Load from local variable (parameter or let-bound)
    LoadLocal { dst: Register, index: u16 },

    /// Store to local variable
    StoreLocal { src: Register, index: u16 },

    /// Load from closure's captured variable array
    LoadClosure { dst: Register, slot: u16 },

    /// Store to closure's captured variable (for set! on captured vars)
    StoreClosure { src: Register, slot: u16 },

    /// Load global variable (only globals use name lookup)
    LoadGlobal { dst: Register, name: Symbol },

    /// Store global variable
    StoreGlobal { src: Register, name: Symbol },

    /// Move between registers
    Move { dst: Register, src: Register },

    // ─────────────────────────────────────────────────────
    // Arithmetic (with profiling metadata)
    // ─────────────────────────────────────────────────────
    Add {
        dst: Register,
        src1: Register,
        src2: Register,
        profile_id: ProfileId,  // ← For adaptive numeric tower
    },
    Sub { dst: Register, src1: Register, src2: Register, profile_id: ProfileId },
    Mul { dst: Register, src1: Register, src2: Register, profile_id: ProfileId },
    Div { dst: Register, src1: Register, src2: Register, profile_id: ProfileId },

    // Specialized arithmetic (generated by adaptive optimizer)
    FixnumAdd { dst: Register, src1: Register, src2: Register, overflow: Label },
    FloatAdd { dst: Register, src1: Register, src2: Register },

    // ─────────────────────────────────────────────────────
    // Comparison
    // ─────────────────────────────────────────────────────
    Eq { dst: Register, src1: Register, src2: Register },
    Lt { dst: Register, src1: Register, src2: Register },
    Gt { dst: Register, src1: Register, src2: Register },

    // ─────────────────────────────────────────────────────
    // Control Flow (Traditional)
    // ─────────────────────────────────────────────────────
    Jump { target: Label },
    JumpIf { condition: Register, target: Label },
    JumpUnless { condition: Register, target: Label },

    // Backward jumps (triggers hotness profiling for tracing JIT)
    JumpBack { target: Label, loop_id: LoopId },

    // ─────────────────────────────────────────────────────
    // Function Calls
    // ─────────────────────────────────────────────────────
    Call {
        func: Register,
        args: RegisterSlice,  // Contiguous register range
        dst: Register,
        profile_id: ProfileId,  // Track callsite behavior
    },

    TailCall { func: Register, args: RegisterSlice },
    Return { value: Register },

    // ─────────────────────────────────────────────────────
    // Delimited Continuations (Primitive for all control effects)
    // ─────────────────────────────────────────────────────
    Reset {
        prompt_tag: PromptTag,
        body: Label,
        cleanup: Option<Label>,  // For dynamic-wind
    },

    Shift {
        prompt_tag: PromptTag,
        continuation_var: Register,
        body: Label
    },

    InvokeContinuation {
        continuation: Register,
        value: Register,
        dst: Register
    },

    // ─────────────────────────────────────────────────────
    // High-Level Control (desugar to Reset/Shift)
    // ─────────────────────────────────────────────────────
    Try { body: Label, handler: Label, exception_var: Register },
    Raise { value: Register },

    Yield { value: Register },  // For generators
    Resume { generator: Register, dst: Register },

    // Full continuation (desugars to top-level reset/shift)
    CallCC { func: Register, dst: Register },

    // ─────────────────────────────────────────────────────
    // Guards (for speculative optimization)
    // ─────────────────────────────────────────────────────
    GuardType {
        reg: Register,
        expected_type: TypeTag,
        deopt_target: Label,
        guard_id: GuardId,  // For tracking guard success rate
    },

    GuardFixnum { reg: Register, deopt_target: Label, guard_id: GuardId },
    GuardFloat { reg: Register, deopt_target: Label, guard_id: GuardId },
    GuardPair { reg: Register, deopt_target: Label, guard_id: GuardId },

    // ─────────────────────────────────────────────────────
    // Data Structure Operations
    // ─────────────────────────────────────────────────────
    Cons { dst: Register, car: Register, cdr: Register },
    Car { dst: Register, pair: Register },
    Cdr { dst: Register, pair: Register },
    SetCar { pair: Register, value: Register },
    SetCdr { pair: Register, value: Register },

    MakeVector { dst: Register, length: Register, fill: Register },
    VectorRef { dst: Register, vector: Register, index: Register },
    VectorSet { vector: Register, index: Register, value: Register },

    // ─────────────────────────────────────────────────────
    // Closures & Environments
    // Flat closure design - see ARCHITECTURE_LESSONS.md §2
    // ─────────────────────────────────────────────────────

    /// Create closure with captured free variables
    /// free_vars contains values to capture (computed at call site)
    MakeClosure {
        dst: Register,
        code: CodeObjectId,
        free_vars: RegisterSlice,  // Contiguous registers with captured values
    },

    // ─────────────────────────────────────────────────────
    // Profiling & Instrumentation (can be no-ops in production)
    // ─────────────────────────────────────────────────────
    ProfileEnter { function_id: FunctionId },  // Track function entries
    ProfileExit { function_id: FunctionId },

    // Snapshot support (for time-travel debugging)
    Snapshot { snapshot_id: SnapshotId },  // Take heap snapshot

    // ─────────────────────────────────────────────────────
    // Tracing Support
    // ─────────────────────────────────────────────────────
    StartTrace { trace_id: TraceId },  // Begin recording trace
    AbortTrace { trace_id: TraceId },   // Trace failed, abort
    GuardTrace { trace_id: TraceId, guards: Vec<GuardId> },  // Enter compiled trace

    // ─────────────────────────────────────────────────────
    // Debugging
    // ─────────────────────────────────────────────────────
    Breakpoint { breakpoint_id: BreakpointId },

    // No-op (for patching/alignment)
    Nop,
}
```

### Register Architecture

**Design:** Register-based (not stack-based) for easier optimization

```rust
pub struct RegisterFile {
    registers: Vec<Value>,
    num_locals: usize,      // Fixed per function
    num_temporaries: usize, // Used for intermediate results
}

pub type Register = u16;  // 65k registers should be enough
```

**Why registers over stack:**
- Easier to analyze dataflow
- Guards can refer to specific values
- Better for SSA-style optimizations
- Traced code works with values in known locations

### Constant Pool

```rust
pub struct ConstantPool {
    values: Vec<Value>,
    code_objects: Vec<CodeObject>,
}

pub type ConstantPoolIndex = u32;
pub type CodeObjectId = u32;
```

### Prompt Tags (for Delimited Continuations)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptTag {
    /// Top-level prompt for call/cc
    TopLevel,

    /// Exception handling
    Exception(ExceptionHandlerId),

    /// Generator/coroutine yield point
    Generator(GeneratorId),

    /// Async/await
    Async(AsyncTaskId),

    /// User-defined prompt
    Custom(Symbol),
}
```

**Flexibility:** New control abstractions = new prompt tags, no ISA changes needed.

---

## Memory Model

### Heap Layout

The heap supports both **immediate operations** and **persistent snapshots** (for time-travel debugging):

```rust
pub enum HeapMode {
    /// Traditional mutable heap (fast, no snapshots)
    Mutable,

    /// Copy-on-write persistent heap (enables time-travel)
    Persistent,
}

pub struct Heap {
    mode: HeapMode,
    storage: HeapStorage,
}

pub enum HeapStorage {
    /// Simple vector-based heap
    Simple(Vec<HeapObject>),

    /// Persistent data structure (im::HashMap for structural sharing)
    Persistent(PersistentHeap),
}

pub struct PersistentHeap {
    root: Arc<HeapNode>,
    generation: u64,
}

pub struct HeapNode {
    objects: im::HashMap<ObjectId, HeapObject>,
    parent: Option<Arc<HeapNode>>,
}
```

**Flexibility:**
- Start with `Mutable` mode (fast, simple)
- Add `Persistent` mode later for time-travel debugging
- Switch modes dynamically per REPL session

### Value Representation

Extend existing `Value` enum with VM-specific variants:

```rust
pub enum Value {
    // ─── Existing from patina-runtime ───
    Fixnum(i64),
    BigInt(Rc<BigInt>),
    Rational(Rc<Ratio<BigInt>>),
    Real(f64),
    Complex(Rc<Complex<f64>>),
    String(Rc<RefCell<String>>),
    Pair(Rc<(Value, Value)>),
    Vector(Rc<RefCell<Vec<Value>>>),
    // ...

    // ─── VM-specific additions ───

    /// Delimited continuation (captures stack slice)
    DelimitedContinuation(Rc<DelimitedContinuation>),

    /// Full continuation (legacy call/cc support)
    FullContinuation(Rc<FullContinuation>),

    /// Generator state
    Generator(Rc<RefCell<Generator>>),

    /// Promise (for async/await)
    Promise(Rc<RefCell<Promise>>),

    /// Compiled trace entry point (JIT support)
    CompiledTrace(TraceId, NativeCodePtr),

    // ─── For symbolic execution (optional future feature) ───
    Symbolic(Rc<SymbolicValue>),
    Mixed(Box<Value>, Rc<SymbolicValue>),  // Concrete + symbolic
}
```

**Flexibility:** Can add new value types without breaking existing code.

### Continuation Representation

```rust
pub struct DelimitedContinuation {
    /// Stack frames captured up to prompt
    frames: Vec<Frame>,

    /// Registers at capture time
    registers: Vec<Value>,

    /// Prompt tag this continuation was captured under
    prompt_tag: PromptTag,

    /// Dynamic wind entries (for proper before/after execution)
    dynamic_wind_entries: Vec<DynamicWindEntry>,

    /// Metadata for optimization
    metadata: ContinuationMetadata,
}

pub struct ContinuationMetadata {
    /// How many times has this continuation been invoked?
    invocation_count: AtomicU32,

    /// Was this continuation stored (escaped)?
    escaped: bool,

    /// Effect classification (for optimization)
    effect_kind: EffectKind,
}

pub enum EffectKind {
    /// Used for early exit (exception-like) - can optimize to longjmp
    Exception,

    /// Used for generators (single resume) - use stack copying
    Generator,

    /// Full power continuation - no optimization
    Full,

    /// Unknown (conservative)
    Unknown,
}
```

**Flexibility:** Effect classification can be refined over time without changing core representation.

---

## Execution Model

### VM State

```rust
pub struct VM {
    /// Current execution mode
    mode: ExecutionMode,

    /// Register file
    registers: RegisterFile,

    /// Call stack
    call_stack: CallStack,

    /// Prompt stack (for delimited continuations)
    prompt_stack: PromptStack,

    /// Dynamic-wind stack (for before/after thunks)
    dynamic_wind_stack: Vec<DynamicWindEntry>,

    /// Heap
    heap: Heap,

    /// Global environment
    globals: Arc<RefCell<Environment>>,

    /// Code cache (bytecode + compiled traces)
    code_cache: CodeCache,

    /// Profiling data
    profiler: Profiler,

    /// Optimizer (optional, activated in adaptive modes)
    optimizer: Option<Box<dyn Optimizer>>,

    /// Snapshot manager (for time-travel debugging)
    snapshot_manager: Option<SnapshotManager>,
}
```

### Call Stack

```rust
pub struct CallStack {
    frames: Vec<Frame>,
}

pub struct Frame {
    /// Code being executed
    code: CodeObjectId,

    /// Program counter within code
    pc: usize,

    /// Base of register window for this frame
    register_base: usize,

    /// Lexical environment (for closure captures)
    environment: Arc<RefCell<Environment>>,

    /// Return address
    return_pc: usize,

    /// Metadata
    metadata: FrameMetadata,
}

pub struct FrameMetadata {
    /// Source location (for debugging)
    source_location: Option<SourceLocation>,

    /// Profiling data
    execution_count: u32,

    /// Is this frame part of a traced loop?
    in_trace: bool,
}
```

### Prompt Stack

```rust
pub struct PromptStack {
    prompts: Vec<Prompt>,
}

pub struct Prompt {
    /// Unique tag for this prompt
    tag: PromptTag,

    /// Call stack depth when prompt was established
    stack_depth: usize,

    /// Register base for continuation restoration
    register_base: usize,

    /// Dynamic-wind depth (for proper cleanup)
    dynamic_wind_depth: usize,
}
```

### Execution Loop

```rust
impl VM {
    pub fn run(&mut self) -> Result<Value, VMError> {
        loop {
            // Fetch instruction
            let instruction = self.fetch_instruction()?;

            // Check for trace entry point
            if let Some(trace) = self.check_trace_entry() {
                // Execute compiled trace instead of bytecode
                match self.execute_trace(trace) {
                    Ok(result) => return Ok(result),
                    Err(TraceDeopt) => {
                        // Guard failed, continue in interpreter
                        continue;
                    }
                }
            }

            // Profile if in profiling mode
            if self.mode.is_profiling() {
                self.profiler.record_instruction(&instruction);
            }

            // Execute instruction
            match self.execute_instruction(instruction)? {
                ExecutionResult::Continue => continue,
                ExecutionResult::Return(value) => return Ok(value),
                ExecutionResult::Yield(value) => {
                    // Generator yield - capture continuation
                    return Ok(value);
                }
                ExecutionResult::StartTrace => {
                    // Switch to trace recording mode
                    self.begin_trace_recording()?;
                    continue;
                }
            }
        }
    }

    fn execute_instruction(&mut self, op: Opcode) -> Result<ExecutionResult, VMError> {
        match op {
            Opcode::Add { dst, src1, src2, profile_id } => {
                let v1 = self.get_register(src1);
                let v2 = self.get_register(src2);

                // Record type profile
                self.profiler.record_types(profile_id, &v1, &v2);

                // Execute (may be specialized based on profile)
                let result = self.execute_add(v1, v2, profile_id)?;
                self.set_register(dst, result);

                Ok(ExecutionResult::Continue)
            }

            Opcode::JumpBack { target, loop_id } => {
                // Check hotness
                let hotness = self.profiler.increment_loop_counter(loop_id);

                if hotness > TRACE_THRESHOLD && self.can_trace() {
                    // Start tracing this hot loop
                    return Ok(ExecutionResult::StartTrace);
                }

                self.pc = target;
                Ok(ExecutionResult::Continue)
            }

            Opcode::Reset { prompt_tag, body, cleanup } => {
                self.execute_reset(prompt_tag, body, cleanup)
            }

            Opcode::Shift { prompt_tag, continuation_var, body } => {
                self.execute_shift(prompt_tag, continuation_var, body)
            }

            // ... other opcodes
        }
    }
}
```

**Flexibility:** Execution loop can switch between interpretation and traced execution transparently.

---

## Runtime Data Structures

### Code Objects

```rust
pub struct CodeObject {
    /// Unique identifier
    id: CodeObjectId,

    /// Function name (for debugging)
    name: Option<Symbol>,

    /// Bytecode instructions
    instructions: Vec<Opcode>,

    /// Constant pool (local to this function)
    constants: Vec<Value>,

    /// Number of registers needed
    num_registers: usize,

    /// Parameter information
    params: ParamInfo,

    /// Source location mapping
    source_map: SourceMap,

    /// Optimization metadata
    metadata: CodeMetadata,
}

pub struct CodeMetadata {
    /// Times this code has been executed
    execution_count: AtomicU32,

    /// Has this been traced/compiled?
    compiled_traces: Vec<TraceId>,

    /// Profiling data for this function
    type_profiles: HashMap<ProfileId, TypeProfile>,

    /// Effect classification (if analyzed)
    effect_summary: Option<EffectSummary>,
}
```

### Flat Closures

Closures use a flat representation for O(1) access to captured variables.
See [ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md) §2 for design rationale.

```rust
/// Runtime closure object
pub struct VmClosure {
    /// Pointer to compiled code
    code: CodeObjectId,

    /// Captured free variables (flat array, indexed access)
    /// Variables are copied at closure creation time
    free_vars: Vec<TaggedValue>,
}

impl VmClosure {
    /// Access captured variable by index (O(1))
    #[inline(always)]
    pub fn get_free_var(&self, index: u16) -> TaggedValue {
        self.free_vars[index as usize]
    }

    /// Mutate captured variable (for set! on captured vars)
    #[inline(always)]
    pub fn set_free_var(&mut self, index: u16, value: TaggedValue) {
        self.free_vars[index as usize] = value;
    }
}
```

**Comparison with tree-walker:**

| Aspect | Tree-Walker | VM |
|--------|-------------|-----|
| Closure structure | `env: Rc<Environment>` | `free_vars: Vec<TaggedValue>` |
| Variable lookup | Hash table + parent chain | Direct index |
| Capture strategy | Captures entire environment | Only captured variables |
| Memory overhead | High (parent pointers) | Low (flat array) |
| Lookup cost | O(depth) worst case | O(1) always |

**Mutable captured variables:**

For variables that are `set!` after capture, we use a box indirection:

```rust
/// Mutable cell for captured variables that are set!
pub struct MutableCell {
    value: TaggedValue,
}

// Compiler detects set! on captured vars and wraps them
// At capture: free_vars[i] = box_cell(initial_value)
// At access: unbox_cell(free_vars[i])
// At set!: set_cell(free_vars[i], new_value)
```

### Profiling Infrastructure

```rust
pub struct Profiler {
    /// Type observations at each profile point
    type_profiles: HashMap<ProfileId, TypeProfile>,

    /// Loop hotness counters
    loop_counters: HashMap<LoopId, LoopProfile>,

    /// Callsite information
    callsite_profiles: HashMap<ProfileId, CallsiteProfile>,

    /// Guard success rates
    guard_stats: HashMap<GuardId, GuardStats>,
}

pub struct TypeProfile {
    observations: HashMap<TypeSignature, u32>,
    total_observations: u32,
}

pub struct TypeSignature {
    operand_types: Vec<TypeTag>,
}

pub enum TypeTag {
    Fixnum,
    BigInt,
    Rational,
    Real,
    Complex,
    String,
    Pair,
    Vector,
    Procedure,
    Other,
}

pub struct LoopProfile {
    /// Number of times loop back-edge taken
    iteration_count: AtomicU32,

    /// Is this loop stable (good candidate for tracing)?
    stable: bool,

    /// Detected patterns
    patterns: Vec<LoopPattern>,
}

pub enum LoopPattern {
    /// Tail-recursive accumulation
    TailRecursiveAccumulation,

    /// Numeric iteration
    NumericIteration { induction_var: Register },

    /// List traversal
    ListTraversal,
}
```

**Flexibility:** Rich profiling enables multiple optimization strategies.

---

## Optimization Framework

### Optimizer Interface

```rust
pub trait Optimizer: Send + Sync {
    /// Analyze code and profiling data
    fn analyze(&mut self, code: &CodeObject, profile: &Profiler) -> AnalysisResult;

    /// Generate optimized version
    fn optimize(&mut self, code: &CodeObject, analysis: &AnalysisResult)
        -> Result<OptimizedCode, OptimizationError>;

    /// Check if optimization is valid (guards passed)
    fn validate(&self, optimized: &OptimizedCode) -> bool;

    /// Deoptimize (fall back to interpreter)
    fn deoptimize(&mut self, optimized: &OptimizedCode) -> DeoptInfo;
}

pub struct OptimizedCode {
    kind: OptimizationKind,
    code: OptimizedCodeVariant,
}

pub enum OptimizationKind {
    /// Specialized bytecode (adaptive numeric tower)
    SpecializedBytecode,

    /// Traced loop compiled to native
    CompiledTrace,

    /// Inlined function
    InlinedFunction,
}

pub enum OptimizedCodeVariant {
    /// Modified bytecode with guards and specialized ops
    Bytecode(Vec<Opcode>),

    /// Native code from tracing JIT
    NativeCode(NativeCodePtr),
}
```

### Adaptive Numeric Optimizer

```rust
pub struct AdaptiveNumericOptimizer {
    specialization_threshold: u32,
}

impl Optimizer for AdaptiveNumericOptimizer {
    fn analyze(&mut self, code: &CodeObject, profile: &Profiler) -> AnalysisResult {
        let mut specializations = vec![];

        for (profile_id, type_profile) in &code.metadata.type_profiles {
            if type_profile.is_monomorphic(0.95) {
                // 95%+ same type -> specialize
                let dominant = type_profile.dominant_type();
                specializations.push(Specialization {
                    profile_id: *profile_id,
                    specialized_type: dominant,
                });
            }
        }

        AnalysisResult { specializations }
    }

    fn optimize(&mut self, code: &CodeObject, analysis: &AnalysisResult)
        -> Result<OptimizedCode, OptimizationError>
    {
        let mut optimized_bytecode = code.instructions.clone();

        for spec in &analysis.specializations {
            // Replace generic Add with specialized FixnumAdd
            self.replace_with_specialized_op(&mut optimized_bytecode, spec)?;
        }

        Ok(OptimizedCode {
            kind: OptimizationKind::SpecializedBytecode,
            code: OptimizedCodeVariant::Bytecode(optimized_bytecode),
        })
    }
}
```

### Meta-Tracing JIT Optimizer

```rust
pub struct MetaTracingJIT {
    /// Cranelift JIT module
    jit_module: JITModule,

    /// Active traces
    traces: HashMap<TraceId, Trace>,

    /// Trace recording state
    recorder: Option<TraceRecorder>,
}

pub struct Trace {
    /// Bytecode instructions in trace
    operations: Vec<TracedOp>,

    /// Guards that must pass for trace to be valid
    guards: Vec<Guard>,

    /// Compiled native code (if compiled)
    compiled: Option<NativeCodePtr>,

    /// Performance counters
    stats: TraceStats,
}

impl Optimizer for MetaTracingJIT {
    fn analyze(&mut self, code: &CodeObject, profile: &Profiler) -> AnalysisResult {
        // Identify hot loops
        let mut hot_loops = vec![];

        for (loop_id, loop_profile) in &profile.loop_counters {
            if loop_profile.iteration_count.load(Ordering::Relaxed) > TRACE_THRESHOLD {
                hot_loops.push(*loop_id);
            }
        }

        AnalysisResult { hot_loops }
    }

    fn optimize(&mut self, code: &CodeObject, analysis: &AnalysisResult)
        -> Result<OptimizedCode, OptimizationError>
    {
        // This would trigger trace recording on next execution
        // Actual compilation happens in the trace recorder

        for loop_id in &analysis.hot_loops {
            self.mark_for_tracing(*loop_id);
        }

        // Return marker to start tracing
        Ok(OptimizedCode {
            kind: OptimizationKind::CompiledTrace,
            code: OptimizedCodeVariant::StartTracing,
        })
    }
}
```

**Flexibility:** Each optimizer is independent, can be added/removed modularly.

---

## Extensibility Points

### 1. New Control Abstractions

**Add via prompt tags:**

```rust
// New async/await support
pub enum PromptTag {
    // ... existing tags ...

    /// Async task suspension point
    Async(AsyncTaskId),
}

// VM execution handles it automatically via Reset/Shift
```

**No ISA changes needed!**

### 2. New Optimizers

**Implement `Optimizer` trait:**

```rust
pub struct MyCustomOptimizer;

impl Optimizer for MyCustomOptimizer {
    fn analyze(&mut self, code: &CodeObject, profile: &Profiler) -> AnalysisResult {
        // Your optimization analysis
    }

    fn optimize(&mut self, code: &CodeObject, analysis: &AnalysisResult)
        -> Result<OptimizedCode, OptimizationError>
    {
        // Your optimization transformation
    }
}

// Register with VM
vm.register_optimizer(Box::new(MyCustomOptimizer));
```

### 3. New Value Types

**Extend Value enum:**

```rust
pub enum Value {
    // ... existing variants ...

    /// Custom user-defined value type
    Extension(Box<dyn ValueExtension>),
}

pub trait ValueExtension {
    fn type_name(&self) -> &str;
    fn display(&self) -> String;
    fn equals(&self, other: &dyn ValueExtension) -> bool;
}
```

### 4. New Bytecode Instructions

**Bytecode is versioned:**

```rust
pub struct CodeObject {
    /// Bytecode version (enables evolution)
    version: BytecodeVersion,

    instructions: Vec<Opcode>,
}

pub enum BytecodeVersion {
    V1,  // Initial version
    V2,  // Added symbolic execution opcodes
    V3,  // Added ... future features
}
```

VM can interpret old bytecode versions via compatibility layer.

---

## Integration Strategy

### Phase 1: Baseline VM (No Existing Code Changes)

**New crate:** `crates/patina-vm/`

```
patina-vm/
├── src/
│   ├── lib.rs                    # Public API
│   ├── opcode.rs                 # Opcode definitions
│   ├── vm.rs                     # Core VM implementation
│   ├── register.rs               # Register file
│   ├── call_stack.rs             # Call stack management
│   ├── prompt_stack.rs           # Prompt stack (continuations)
│   ├── heap.rs                   # Heap management
│   ├── execution.rs              # Execution loop
│   ├── profiler.rs               # Profiling infrastructure
│   ├── optimizer/
│   │   ├── mod.rs                # Optimizer trait
│   │   ├── adaptive_numeric.rs   # Adaptive numeric tower
│   │   ├── tracing_jit.rs        # Meta-tracing JIT
│   │   └── effect_typing.rs      # Effect-typed continuations
│   ├── codegen/
│   │   ├── mod.rs                # Bytecode generation
│   │   └── ir_to_bytecode.rs     # Lower IR to bytecode
│   └── debug/
│       ├── snapshot.rs           # Snapshot management
│       └── time_travel.rs        # Time-travel debugging
└── Cargo.toml
```

**Dependencies:**
```toml
[dependencies]
patina-runtime = { path = "../patina-runtime" }
patina-frontend = { path = "../patina-frontend" }

# For persistent heap (optional)
im = { version = "15", optional = true }

# For JIT compilation (optional)
cranelift = { version = "0.104", optional = true }
cranelift-jit = { version = "0.104", optional = true }

# For SMT solving (very optional)
z3 = { version = "0.12", optional = true }

[features]
default = []
persistent-heap = ["im"]
tracing-jit = ["cranelift", "cranelift-jit"]
symbolic-execution = ["z3"]
```

### Integration with patina-interpreter

```rust
// In crates/patina-interpreter/src/lib.rs

pub struct Interpreter {
    backend: InterpreterBackend,
}

pub enum InterpreterBackend {
    TreeWalker(TreeWalkerEvaluator),
    VM(patina_vm::VM),
}

impl Interpreter {
    pub fn new() -> Self {
        // Default: tree-walker (stable)
        Interpreter {
            backend: InterpreterBackend::TreeWalker(TreeWalkerEvaluator::new()),
        }
    }

    pub fn with_vm() -> Self {
        // Opt-in to VM backend
        Interpreter {
            backend: InterpreterBackend::VM(patina_vm::VM::new()),
        }
    }

    pub fn eval_str(&self, input: &str) -> Result<Value, Error> {
        match &self.backend {
            InterpreterBackend::TreeWalker(eval) => eval.eval_str(input),
            InterpreterBackend::VM(vm) => {
                // Parse to IR
                let ast = parse(input)?;

                // Compile to bytecode
                let bytecode = vm.compile(ast)?;

                // Execute
                vm.execute(bytecode)
            }
        }
    }
}
```

**Flexibility:** Tree-walker remains default, VM is opt-in during Phase 2.

---

## Implementation Phases

### Phase 2A: Foundation (4-6 weeks)

**Goal:** Working bytecode VM without optimizations

**Deliverables:**
- ✅ Core VM with register machine
- ✅ Bytecode ISA (basic opcodes)
- ✅ Compilation from AST to bytecode
- ✅ Basic profiling infrastructure
- ✅ Test suite (all R7RS tests pass)

**Features:**
- Simple interpreter mode
- No optimizations yet
- Establishes architecture for future work

### Phase 2B: Adaptive Numeric Tower (2-3 weeks)

**Goal:** 5-10x speedup on numeric code

**Deliverables:**
- ✅ Type profiling at arithmetic operations
- ✅ Specialized fixnum/float opcodes
- ✅ Guard insertion and deoptimization
- ✅ Benchmarks showing speedup

**Features:**
- ProfilingInterpreter mode
- AdaptiveNumericOptimizer
- Numeric benchmarks (sum, fibonacci, etc.)

### Phase 2C: Effect-Typed Continuations (3-4 weeks)

**Goal:** Make call/cc practical

**Deliverables:**
- ✅ Reset/Shift primitives
- ✅ Effect inference
- ✅ Optimized exception-style call/cc
- ✅ Generator support

**Features:**
- Delimited continuation support
- Exception fast path
- Generator/coroutine primitives

### Phase 2D: Meta-Tracing JIT (Optional, 6-8 weeks)

**Goal:** 10-100x speedup on hot loops

**Deliverables:**
- ✅ Trace recording
- ✅ Cranelift integration
- ✅ Guard compilation
- ✅ Deoptimization

**Features:**
- TracingJIT mode
- Automatic hot loop detection
- Native code generation

### Phase 2E: Persistent Heap (Optional, 4-6 weeks)

**Goal:** Time-travel debugging

**Deliverables:**
- ✅ Copy-on-write heap
- ✅ Snapshot management
- ✅ REPL integration (rewind/replay)

**Features:**
- Debug mode
- Snapshot save/restore
- Time-travel commands

---

## Success Criteria

### Correctness
- ✅ All R7RS compliance tests pass
- ✅ No regressions vs tree-walker
- ✅ Deterministic behavior

### Performance
- ✅ Baseline VM: 2-5x faster than tree-walker
- ✅ With adaptive numeric: 5-10x faster on numeric code
- ✅ With tracing JIT: 10-100x faster on hot loops

### Maintainability
- ✅ Clear module boundaries
- ✅ Each optimizer is independent
- ✅ Comprehensive test coverage
- ✅ Good documentation

### Flexibility
- ✅ Can add new optimizations without refactoring
- ✅ Can switch execution modes at runtime
- ✅ Backward compatible bytecode

---

## References

### Architecture & Design
- [Architecture Lessons](./ARCHITECTURE_LESSONS.md) - Comparative analysis with other language implementations
- [VM Value Architecture](./VM_VALUE_ARCHITECTURE.md) - Dual representation (Value/TaggedValue)
- [Compilation Design](./COMPILATION_DESIGN.md) - Bytecode compiler design
- [Tagged Pointers](./TAGGED_POINTERS.md) - TaggedValue implementation details

### Research Documents
- [Meta-Tracing](./01_META_TRACING.md)
- [Effect Continuations](./02_EFFECT_CONTINUATIONS.md)
- [Adaptive Numeric Tower](./03_ADAPTIVE_NUMERIC.md)
- [Persistent Heap](./04_PERSISTENT_HEAP.md)
- [Delimited Continuations ISA](./05_DELIMITED_CONTINUATIONS.md)
- [Self-Optimizing AST](./06_SELF_OPTIMIZING_AST.md)
- [Symbolic Execution](./07_SYMBOLIC_EXECUTION.md)

---

## Conclusion

This VM specification provides:

1. **Solid Foundation:** Register-based bytecode VM with profiling infrastructure
2. **Incremental Path:** Each phase adds capabilities without breaking existing work
3. **Multiple Strategies:** Can pursue different optimizations independently
4. **Future-Proof:** Extensible design accommodates research ideas

**Recommended starting point:** Phase 2A + 2B (Foundation + Adaptive Numeric)
- Gets working VM quickly (6-9 weeks)
- Delivers measurable performance win
- Establishes profiling for future optimizations
- Proves architecture before committing to complex features

**Next steps after 2A+2B:**
- If performance critical → 2D (Tracing JIT)
- If UX critical → 2E (Persistent Heap)
- If research interests → 2C (Effect Continuations)

The architecture supports all paths! 🚀
