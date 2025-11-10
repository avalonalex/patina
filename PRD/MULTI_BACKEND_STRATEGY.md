# Multi-Backend Interpreter Strategy

**Goal:** Maintain multiple interpreter implementations with different trade-offs

**Philosophy:** Keep the simple tree-walking interpreter as a reference implementation while adding more sophisticated backends for performance.

---

## Why Multiple Backends?

### Tree-Walking Interpreter (Current)
**Strengths:**
- ✅ Simple, easy to understand
- ✅ Great for learning and teaching
- ✅ Easy to debug and modify
- ✅ Direct mapping to R7RS semantics
- ✅ Perfect for prototyping new features
- ✅ Pedagogical value - shows how Scheme works

**Limitations:**
- ❌ Slower execution
- ❌ No JIT optimization
- ❌ Higher memory overhead for deep recursion
- ❌ Stack-based (even with TCO trampolines)

### Future: Bytecode VM
**Strengths:**
- ✅ Faster execution (2-10x typically)
- ✅ Lower memory overhead
- ✅ Better for long-running programs
- ✅ Can add JIT later

**Trade-offs:**
- ❌ More complex implementation
- ❌ Harder to debug
- ❌ Compilation step adds latency

### Future: Native Code (LLVM/Cranelift)
**Strengths:**
- ✅ Maximum performance
- ✅ Can compete with compiled languages
- ✅ Advanced optimizations

**Trade-offs:**
- ❌ Very complex
- ❌ Long compilation times
- ❌ Much harder to debug

---

## Proposed Architecture

### Backend Abstraction

```rust
// crates/patina-core/src/backend/mod.rs
pub mod tree_walking;
pub mod bytecode;  // Future
pub mod native;    // Future

/// Common trait for all interpreter backends
pub trait Backend {
    /// Evaluate an expression to a value
    fn eval(&mut self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError>;

    /// Load and execute a library
    fn load_library(&mut self, name: &LibraryName) -> Result<(), EvalError>;

    /// Get backend name for debugging
    fn name(&self) -> &str;

    /// Get performance characteristics
    fn characteristics(&self) -> BackendCharacteristics;
}

#[derive(Debug, Clone)]
pub struct BackendCharacteristics {
    pub startup_time: StartupTime,
    pub execution_speed: ExecutionSpeed,
    pub memory_overhead: MemoryOverhead,
    pub debugging_support: DebuggingSupport,
}

pub enum StartupTime {
    Instant,      // Tree-walking
    Fast,         // Bytecode
    Slow,         // Native compilation
}

pub enum ExecutionSpeed {
    Interpreted,  // Tree-walking
    Bytecode,     // Bytecode VM
    Native,       // Compiled code
}
```

### Backend Selection

```rust
// crates/patina-core/src/lib.rs
pub enum BackendKind {
    TreeWalking,   // Default, always available
    Bytecode,      // Requires "bytecode" feature
    Native,        // Requires "native" feature
}

pub struct Interpreter {
    backend: Box<dyn Backend>,
    library_registry: LibraryRegistry,
    global_env: Rc<Environment>,
}

impl Interpreter {
    /// Create with default backend (tree-walking)
    pub fn new() -> Self {
        Self::with_backend(BackendKind::TreeWalking)
    }

    /// Create with specific backend
    pub fn with_backend(kind: BackendKind) -> Self {
        let backend: Box<dyn Backend> = match kind {
            BackendKind::TreeWalking => {
                Box::new(tree_walking::TreeWalkingBackend::new())
            }
            #[cfg(feature = "bytecode")]
            BackendKind::Bytecode => {
                Box::new(bytecode::BytecodeBackend::new())
            }
            #[cfg(feature = "native")]
            BackendKind::Native => {
                Box::new(native::NativeBackend::new())
            }
            #[cfg(not(feature = "bytecode"))]
            BackendKind::Bytecode => {
                panic!("Bytecode backend not enabled. Enable 'bytecode' feature.")
            }
            #[cfg(not(feature = "native"))]
            BackendKind::Native => {
                panic!("Native backend not enabled. Enable 'native' feature.")
            }
        };

        Self {
            backend,
            library_registry: LibraryRegistry::new(),
            global_env: Rc::new(Environment::new()),
        }
    }

    /// Evaluate with current backend
    pub fn eval(&mut self, expr: &Value) -> Result<Value, EvalError> {
        self.backend.eval(expr, &self.global_env)
    }

    /// Switch backend at runtime (advanced use case)
    pub fn switch_backend(&mut self, kind: BackendKind) {
        self.backend = match kind {
            BackendKind::TreeWalking => Box::new(tree_walking::TreeWalkingBackend::new()),
            // ... other backends
        };
    }
}
```

---

## Directory Structure

```
crates/patina-core/src/
├── lib.rs
├── runtime/              # Shared runtime (Value, Environment, etc.)
│   ├── mod.rs
│   ├── value.rs
│   ├── environment.rs
│   └── procedure.rs
│
├── frontend/             # Shared frontend (Lexer, Parser)
│   ├── mod.rs
│   ├── lexer.rs
│   └── parser.rs
│
├── backend/              # Backend abstraction
│   ├── mod.rs           # Backend trait
│   │
│   ├── tree_walking/    # Tree-walking interpreter (always available)
│   │   ├── mod.rs
│   │   ├── evaluator.rs
│   │   ├── special_forms/
│   │   │   ├── mod.rs
│   │   │   ├── binding.rs
│   │   │   ├── control.rs
│   │   │   └── iteration.rs
│   │   ├── primitives/
│   │   │   ├── mod.rs
│   │   │   ├── arithmetic.rs
│   │   │   ├── lists.rs
│   │   │   └── vectors.rs
│   │   └── application.rs
│   │
│   ├── bytecode/        # Bytecode VM (feature-gated)
│   │   ├── mod.rs
│   │   ├── compiler.rs  # AST -> Bytecode
│   │   ├── vm.rs        # Bytecode interpreter
│   │   ├── opcodes.rs   # Instruction set
│   │   └── optimizer.rs # Bytecode optimizations
│   │
│   └── native/          # Native code generation (feature-gated)
│       ├── mod.rs
│       ├── codegen.rs   # Code generation
│       └── jit.rs       # JIT compilation
│
├── macro_system/         # Shared macro system
├── library/              # Shared module system
└── debug/                # Shared debugging
```

---

## Feature Flags

```toml
# crates/patina-core/Cargo.toml
[features]
default = ["tree-walking"]

# Backend features (mutually exclusive for default, but can coexist)
tree-walking = []
bytecode = ["dep:byteorder"]
native = ["dep:cranelift", "dep:cranelift-module"]

# All backends (for testing/comparison)
all-backends = ["tree-walking", "bytecode", "native"]

# Debug features
debug-trace = []
profiling = ["dep:flamegraph"]
```

---

## Tree-Walking Backend (Current Implementation)

### Preserve Current Structure

```rust
// crates/patina-core/src/backend/tree_walking/mod.rs
pub mod evaluator;
pub mod special_forms;
pub mod primitives;
pub mod application;

use super::Backend;

pub struct TreeWalkingBackend {
    debugger: Debugger,
}

impl Backend for TreeWalkingBackend {
    fn eval(&mut self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // Your current tree-walking evaluator
        self.evaluator.eval(expr, env)
    }

    fn name(&self) -> &str {
        "tree-walking"
    }

    fn characteristics(&self) -> BackendCharacteristics {
        BackendCharacteristics {
            startup_time: StartupTime::Instant,
            execution_speed: ExecutionSpeed::Interpreted,
            memory_overhead: MemoryOverhead::High,
            debugging_support: DebuggingSupport::Full,
        }
    }
}

impl TreeWalkingBackend {
    /// Your current evaluator implementation
    pub fn eval_impl(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // All your current eval logic stays here!
        match expr {
            Value::Integer(_) | Value::Boolean(_) | Value::String(_) => Ok(expr.clone()),
            Value::Symbol(s) => env.get(s.as_ref()).ok_or(...),
            Value::Pair(_) => self.eval_list(expr, env),
            // ... all your current logic
        }
    }
}
```

**Nothing changes** - just wrapped in a backend interface!

---

## Future: Bytecode Backend

### Example Implementation (Future)

```rust
// crates/patina-core/src/backend/bytecode/opcodes.rs
#[derive(Debug, Clone, Copy)]
pub enum OpCode {
    // Constants
    LoadConst(u16),      // Push constant from pool
    LoadNil,
    LoadTrue,
    LoadFalse,

    // Variables
    LoadGlobal(u16),     // Load from global env
    LoadLocal(u8),       // Load from stack frame
    StoreGlobal(u16),
    StoreLocal(u8),

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,

    // Comparison
    Eq,
    Lt,
    Gt,

    // Control flow
    Jump(i16),           // Unconditional jump
    JumpIfFalse(i16),    // Conditional jump

    // Calls
    Call(u8),            // Call with N args
    TailCall(u8),        // Tail call optimization
    Return,

    // Lists
    Cons,
    Car,
    Cdr,

    // Closure support
    MakeClosure(u16),    // Create closure
    LoadUpvalue(u8),     // Access closed-over variable
}

// crates/patina-core/src/backend/bytecode/compiler.rs
pub struct BytecodeCompiler {
    constants: Vec<Value>,
    code: Vec<OpCode>,
}

impl BytecodeCompiler {
    pub fn compile(&mut self, expr: &Value) -> Result<Vec<OpCode>, CompileError> {
        match expr {
            Value::Integer(n) => {
                let idx = self.add_constant(expr.clone());
                Ok(vec![OpCode::LoadConst(idx)])
            }
            Value::Pair(_) if is_special_form(expr, "if") => {
                self.compile_if(expr)
            }
            // ... compile all special forms
        }
    }

    fn compile_if(&mut self, expr: &Value) -> Result<Vec<OpCode>, CompileError> {
        // (if test then else)
        let (test, then_expr, else_expr) = parse_if(expr)?;

        let mut code = Vec::new();

        // Compile test
        code.extend(self.compile(&test)?);

        // Jump if false (backpatch later)
        let jump_to_else = code.len();
        code.push(OpCode::JumpIfFalse(0));

        // Compile then branch
        code.extend(self.compile(&then_expr)?);
        let jump_to_end = code.len();
        code.push(OpCode::Jump(0));

        // Patch else jump
        let else_start = code.len();
        code[jump_to_else] = OpCode::JumpIfFalse((else_start - jump_to_else) as i16);

        // Compile else branch
        code.extend(self.compile(&else_expr)?);

        // Patch end jump
        let end = code.len();
        code[jump_to_end] = OpCode::Jump((end - jump_to_end) as i16);

        Ok(code)
    }
}

// crates/patina-core/src/backend/bytecode/vm.rs
pub struct VM {
    stack: Vec<Value>,
    globals: Rc<Environment>,
    call_frames: Vec<CallFrame>,
    constants: Vec<Value>,
}

impl VM {
    pub fn run(&mut self, code: &[OpCode]) -> Result<Value, EvalError> {
        let mut ip = 0;  // Instruction pointer

        while ip < code.len() {
            match code[ip] {
                OpCode::LoadConst(idx) => {
                    self.stack.push(self.constants[idx as usize].clone());
                }
                OpCode::Add => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(add(a, b)?);
                }
                OpCode::JumpIfFalse(offset) => {
                    let test = self.stack.pop().unwrap();
                    if !test.is_truthy() {
                        ip = (ip as i16 + offset) as usize;
                        continue;
                    }
                }
                OpCode::TailCall(argc) => {
                    // Reuse current stack frame (TCO!)
                    let args: Vec<_> = (0..argc).map(|_| self.stack.pop().unwrap()).collect();
                    let proc = self.stack.pop().unwrap();
                    // ... handle tail call
                }
                // ... all other opcodes
            }
            ip += 1;
        }

        Ok(self.stack.pop().unwrap())
    }
}
```

---

## CLI Backend Selection

```rust
// crates/patina-cli/src/cli/args.rs
use clap::Parser;

#[derive(Parser)]
#[command(name = "patina")]
#[command(about = "A Scheme R7RS interpreter")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Select interpreter backend
    #[arg(long, short = 'b', global = true)]
    pub backend: Option<BackendChoice>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum BackendChoice {
    /// Tree-walking interpreter (default, simple, easy to debug)
    Tree,

    /// Bytecode VM (faster execution)
    #[cfg(feature = "bytecode")]
    Bytecode,

    /// Native code generation (fastest, longer startup)
    #[cfg(feature = "native")]
    Native,
}

impl Default for BackendChoice {
    fn default() -> Self {
        Self::Tree
    }
}

// Usage:
// patina repl                    # Default: tree-walking
// patina repl --backend tree     # Explicit tree-walking
// patina repl --backend bytecode # Bytecode VM
// patina run script.scm -b native # Native compilation
```

---

## Benchmarking Framework

```rust
// crates/patina-core/benches/backend_comparison.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkGroup};
use patina_core::{Interpreter, BackendKind};

fn fibonacci_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("fibonacci");

    let code = r#"
        (define (fib n)
          (if (<= n 1)
              n
              (+ (fib (- n 1)) (fib (- n 2)))))
        (fib 20)
    "#;

    // Tree-walking
    group.bench_function("tree-walking", |b| {
        let mut interp = Interpreter::with_backend(BackendKind::TreeWalking);
        b.iter(|| interp.eval_str(black_box(code)))
    });

    // Bytecode
    #[cfg(feature = "bytecode")]
    group.bench_function("bytecode", |b| {
        let mut interp = Interpreter::with_backend(BackendKind::Bytecode);
        b.iter(|| interp.eval_str(black_box(code)))
    });

    // Native
    #[cfg(feature = "native")]
    group.bench_function("native", |b| {
        let mut interp = Interpreter::with_backend(BackendKind::Native);
        b.iter(|| interp.eval_str(black_box(code)))
    });

    group.finish();
}

criterion_group!(benches, fibonacci_benchmark);
criterion_main!(benches);

// Run with:
// cargo bench --features all-backends
```

---

## Documentation Strategy

### User-Facing Documentation

```markdown
# Choosing an Interpreter Backend

Patina supports multiple interpreter backends with different performance
characteristics. Choose based on your use case:

## Tree-Walking (Default)

**Best for:**
- Learning Scheme
- Interactive development
- Debugging
- Small scripts

**Characteristics:**
- ✅ Instant startup
- ✅ Simple, predictable behavior
- ✅ Full debugging support
- ❌ Slower execution

**Usage:**
```bash
patina repl
patina run script.scm --backend tree
```

## Bytecode VM

**Best for:**
- Long-running programs
- Production deployments
- Server applications

**Characteristics:**
- ✅ 2-10x faster execution
- ✅ Lower memory usage
- ✅ Good startup time
- ❌ Limited debugging

**Usage:**
```bash
patina run script.scm --backend bytecode
```

## Native Compilation

**Best for:**
- CPU-intensive computations
- Maximum performance
- Ahead-of-time compilation

**Characteristics:**
- ✅ Maximum performance (near C speed)
- ✅ Advanced optimizations
- ❌ Slow compilation
- ❌ Debugging via external tools

**Usage:**
```bash
patina compile script.scm -o script
./script
```

## Performance Comparison

Based on our benchmarks:

| Benchmark | Tree | Bytecode | Native |
|-----------|------|----------|--------|
| Fibonacci(20) | 1x | 4x | 12x |
| Ackermann(3,8) | 1x | 5x | 15x |
| List processing | 1x | 3x | 8x |
| Macro expansion | 1x | 1x | 1x |
```

---

## Maintenance Strategy

### 1. Keep Tree-Walking Simple

- **Never add complexity** for performance
- Focus on **correctness and clarity**
- Use as **reference implementation** for new features
- **Always test** new features here first

### 2. Shared Core Components

All backends share:
- ✅ Value types (`runtime/value.rs`)
- ✅ Environment (`runtime/environment.rs`)
- ✅ Lexer and Parser (`frontend/`)
- ✅ Macro system (`macro_system/`)
- ✅ Library system (`library/`)

### 3. Feature Parity

All backends must:
- ✅ Pass the same test suite
- ✅ Implement full R7RS-small
- ✅ Support same standard libraries
- ✅ Produce identical results

### 4. Testing Strategy

```rust
// tests/backend_parity.rs
#[test]
fn test_all_backends_produce_same_result() {
    let test_cases = vec![
        ("(+ 1 2)", "3"),
        ("(if #t 1 2)", "1"),
        // ... hundreds of test cases
    ];

    for (expr, expected) in test_cases {
        let tree_result = Interpreter::with_backend(BackendKind::TreeWalking)
            .eval_str(expr).unwrap();

        #[cfg(feature = "bytecode")]
        {
            let bytecode_result = Interpreter::with_backend(BackendKind::Bytecode)
                .eval_str(expr).unwrap();
            assert_eq!(tree_result, bytecode_result);
        }

        assert_eq!(tree_result.to_string(), expected);
    }
}
```

---

## Migration Timeline

### Phase 1: Extract Backend Interface (Now)
- Define `Backend` trait
- Wrap current tree-walking code
- No behavior changes
- **Effort:** 1-2 days

### Phase 2: Optimize Tree-Walking (Ongoing)
- Keep improving clarity
- Add better error messages
- Enhance debugging
- **Continuous improvement**

### Phase 3: Bytecode VM (Future - 6-8 weeks)
- Design instruction set
- Implement compiler
- Implement VM
- Optimize bytecode

### Phase 4: Native Backend (Far Future - 3-4 months)
- LLVM or Cranelift integration
- Type inference for optimization
- JIT compilation

---

## Advantages of This Approach

### For You (Developer)

✅ **Freedom to experiment** - Try new ideas in tree-walking first
✅ **Reference implementation** - Always have simple version to compare
✅ **Teaching tool** - Show students how interpreters work
✅ **Debugging aid** - Complex issues? Test in tree-walking first
✅ **No pressure** - Advanced backends are opt-in features

### For Users

✅ **Choice** - Pick backend for their needs
✅ **Compatibility** - All backends produce same results
✅ **Performance** - Can optimize later without changing code
✅ **Simplicity** - Default is still the simple tree-walking

### For the Project

✅ **Gradual evolution** - No big rewrites, incremental improvement
✅ **Risk mitigation** - New backends don't break existing code
✅ **Flexibility** - Can support multiple backends simultaneously
✅ **Future-proof** - Easy to add new backends (WASM, GPU, etc.)

---

## Example: Adding a New Feature

When adding a new R7RS feature:

1. **Implement in tree-walking** - Simple, correct version
2. **Write tests** - Based on tree-walking behavior
3. **Test other backends** - They must match tree-walking
4. **Optimize later** - Add bytecode/native support when needed

This ensures correctness first, performance second!

---

## Conclusion

Your tree-walking interpreter is valuable and should be preserved! The backend abstraction lets you:

- Keep it simple and pedagogical
- Add performance backends later
- Give users choice
- Maintain multiple implementations without conflict

**Recommendation:** Start with Phase 1 (extract backend interface) during the workspace migration. It's a small change now that enables big flexibility later!

Would you like me to help implement the backend abstraction layer?
