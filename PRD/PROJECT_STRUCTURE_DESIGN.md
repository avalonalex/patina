# Patina Project Structure Design (Revised 2025-11-11)

**Vision:** Educational multi-backend Scheme interpreter with nanopass optimization architecture

**Core Architecture Philosophy:**
1. **Unified Frontend** → Lexer + Parser + Macro Expansion → **Minimal Scheme Syntax (Core IR)**
2. **Nanopass Optimization** → Multiple small, composable transformation passes
3. **Multi-Backend Execution** → Tree-walker (educational) + VM (practical) + JIT (future)
4. **Workspace Organization** → Clean separation enabling independent development

**Design Inspiration:**
- **Chez Scheme**: Nanopass compiler architecture, self-hosting design
- **Rust Workspace**: Multi-crate organization for modularity
- **Educational Focus**: Keep each component understandable and well-documented

**Philosophy:** "Simple, correct reference implementation with paths to optimization"

---

## Architectural Vision

### Three-Stage Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│                       FRONTEND                               │
│  Source → Lexer → Parser → Macro Expander → Core IR         │
│                                                               │
│  Output: Minimal Scheme syntax (no macros, all special      │
│          forms expanded to core forms)                       │
└───────────────────────┬─────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                  NANOPASS OPTIMIZER                          │
│  Pass 1: Assignment conversion (set! → box)                 │
│  Pass 2: CPS conversion (optional, for call/cc)            │
│  Pass 3: Closure conversion (lambda → closure)              │
│  Pass 4: Optimize known calls                               │
│  Pass 5: Inline primitives                                  │
│  Pass N: Backend-specific optimizations                     │
│                                                               │
│  Output: Optimized IR (still Core Scheme, but simpler)      │
└───────────────────────┬─────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                    BACKEND SELECTION                         │
│                                                               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │ Tree-Walker │  │  Bytecode   │  │  Native/JIT │         │
│  │             │  │     VM      │  │   (future)  │         │
│  │  (default)  │  │  (faster)   │  │  (fastest)  │         │
│  └─────────────┘  └─────────────┘  └─────────────┘         │
│                                                               │
│  All backends receive same optimized IR                      │
└─────────────────────────────────────────────────────────────┘
```

### Core IR (Minimal Scheme Syntax)

After frontend processing, all code is reduced to:

**Core Forms:**
- `quote` - Literal data
- `lambda` - Function abstraction
- `if` - Conditional (ternary only)
- `set!` - Mutation
- `begin` - Sequencing
- `define` - Top-level binding
- Primitive calls: `(+ 1 2)`, `(cons a b)`, etc.
- Application: `(f x y)`

**Eliminated Forms:**
- ❌ `cond`, `case` → expanded to nested `if`
- ❌ `and`, `or` → expanded to `if`
- ❌ `let`, `let*`, `letrec` → expanded to `lambda` + application
- ❌ `do` → expanded to named `let` → `lambda` recursion
- ❌ All macros → fully expanded

**Benefits:**
- ✅ **Simplicity**: Backend only handles ~7 core forms
- ✅ **Uniformity**: All macros/derived forms processed once, up front
- ✅ **Optimization**: Passes operate on minimal syntax
- ✅ **Correctness**: Easier to reason about semantics

---

## Current State Analysis

### What's Working Well ✅

**Rust Code Organization:**
```
src/
├── eval/          # Evaluator with good separation
│   ├── mod.rs
│   ├── error.rs
│   ├── special_forms.rs
│   ├── application.rs
│   ├── primitives/
│   └── debug.rs
├── macro_system/  # Clean macro implementation
├── repl/          # Well-organized REPL
├── value/         # Core value types
├── parser/
├── lexer/
└── env/
```

**Documentation:**
- Excellent PRD/ structure with phase organization
- Good internal/ for historical tracking
- Clear CLAUDE.md for AI guidance

### What Needs Improvement ⚠️

1. **No library organization** - Will need `lib/scheme/` for R7RS libraries
2. **Monolithic files** - Some files getting large (special_forms.rs ~1500 lines)
3. **No workspace structure** - Could benefit from crates separation
4. **Limited debugging infrastructure** - Basic debug module, could expand
5. **Test organization** - Tests mixed with source, could be clearer

---

## Proposed Project Structure (Nanopass Architecture)

### Top-Level Layout

```
patina/
├── crates/                      # Workspace for multiple crates
│   ├── patina-frontend/        # Unified frontend (Lexer + Parser + Macro)
│   ├── patina-ir/              # Core IR definitions (shared)
│   ├── patina-nanopass/        # Optimization passes
│   ├── patina-backend-tree/    # Tree-walking interpreter
│   ├── patina-backend-vm/      # Bytecode VM (future)
│   ├── patina-runtime/         # Shared runtime (Value, Environment)
│   ├── patina-cli/             # CLI and REPL (binary)
│   ├── patina-stdlib/          # Standard library (mixed Rust/Scheme)
│   └── patina-debugger/        # Debug server (future)
├── lib/                         # Scheme standard libraries
│   ├── scheme/               # R7RS standard libraries
│   │   ├── base.sld
│   │   ├── char.sld
│   │   ├── write.sld
│   │   └── ...
│   ├── srfi/                 # SRFI implementations
│   │   ├── 1.sld            # List library
│   │   └── ...
│   ├── patina/               # Patina-specific extensions
│   │   ├── debug.sld
│   │   ├── reactive.sld     # Future: reactive extensions
│   │   └── notebook.sld     # Future: notebook support
│   └── bootstrap.scm         # Core bootstrap code
├── tests/                     # Integration tests
│   ├── compliance/           # R7RS compliance tests
│   ├── performance/          # Benchmarks
│   ├── regression/           # Regression tests
│   └── chibi/                # Chibi test suite
├── examples/                  # Example programs
├── docs/                      # User documentation
├── internal/                  # Implementation notes
├── PRD/                       # Design documents
├── spec/                      # R7RS specification
└── Cargo.toml                 # Workspace configuration
```

---

## Detailed Crate Structure (Nanopass Architecture)

### 1. `patina-runtime` (Shared Foundation)

**Purpose:** Core value types and environment model shared by all components

```
crates/patina-runtime/
├── Cargo.toml
└── src/
    ├── lib.rs                 # Public API
    ├── value.rs              # Value enum (integers, pairs, procedures, etc.)
    ├── environment.rs        # Lexical environment
    ├── procedure.rs          # Procedure representation
    ├── port.rs               # I/O ports (future)
    ├── error.rs              # Error types
    └── display.rs            # Display formatting
```

**Dependencies:** `num-bigint`, `num-rational`, `num-complex`

**Philosophy:** Minimal, stable foundation. Changes rarely.

---

### 2. `patina-ir` (Core Intermediate Representation)

**Purpose:** Defines the Core IR that all passes operate on

```
crates/patina-ir/
├── Cargo.toml
└── src/
    ├── lib.rs                # Public API
    ├── expr.rs               # Core expression types
    ├── syntax.rs             # Surface syntax (before macro expansion)
    ├── builder.rs            # Builder functions for constructing IR
    └── visitor.rs            # Visitor trait for IR traversal
```

**Core IR Types:**

```rust
// Core expressions after macro expansion
pub enum CoreExpr {
    // Literals
    Literal(Value),

    // Variables
    Var(Symbol),

    // Core forms
    Quote(Value),
    Lambda {
        params: Formals,
        body: Vec<CoreExpr>,
    },
    If {
        test: Box<CoreExpr>,
        then: Box<CoreExpr>,
        else_: Box<CoreExpr>,
    },
    Set {
        var: Symbol,
        value: Box<CoreExpr>,
    },
    Begin(Vec<CoreExpr>),
    Define {
        name: Symbol,
        value: Box<CoreExpr>,
    },

    // Application
    App {
        func: Box<CoreExpr>,
        args: Vec<CoreExpr>,
    },

    // After optimization passes
    PrimCall {
        prim: Primitive,
        args: Vec<CoreExpr>,
    },
    Let {
        bindings: Vec<(Symbol, CoreExpr)>,
        body: Box<CoreExpr>,
    },
}

// Visitor pattern for passes
pub trait ExprVisitor {
    type Output;
    fn visit_expr(&mut self, expr: &CoreExpr) -> Self::Output;
    fn visit_lambda(&mut self, params: &Formals, body: &[CoreExpr]) -> Self::Output;
    // ... visit methods for each variant
}
```

**Dependencies:** `patina-runtime`

---

### 3. `patina-frontend` (Unified Frontend)

**Purpose:** Lexer + Parser + Macro Expansion → Core IR

```
crates/patina-frontend/
├── Cargo.toml
└── src/
    ├── lib.rs                # Public API
    │
    ├── lexer/
    │   ├── mod.rs
    │   ├── token.rs          # Token types
    │   └── error.rs          # Lexer errors
    │
    ├── parser/
    │   ├── mod.rs            # Parse tokens → Surface syntax
    │   └── error.rs          # Parse errors
    │
    ├── macro_expander/
    │   ├── mod.rs            # Macro expansion orchestrator
    │   ├── expander.rs       # syntax-rules expansion
    │   ├── pattern.rs        # Pattern matching
    │   ├── template.rs       # Template expansion
    │   ├── hygiene.rs        # Hygienic renaming
    │   └── environment.rs    # Macro environment
    │
    ├── desugar/
    │   ├── mod.rs            # Surface syntax → Core IR
    │   ├── derived.rs        # Expand derived forms (cond, case, let, etc.)
    │   └── validate.rs       # Semantic validation
    │
    └── pipeline.rs           # Frontend orchestration
```

**Pipeline:**

```rust
pub struct Frontend {
    macro_env: MacroEnvironment,
}

impl Frontend {
    /// Full pipeline: source → Core IR
    pub fn compile(&mut self, source: &str) -> Result<CoreExpr, FrontendError> {
        // 1. Lex: source → tokens
        let tokens = lexer::tokenize(source)?;

        // 2. Parse: tokens → surface syntax
        let syntax = parser::parse(tokens)?;

        // 3. Macro expand: surface syntax → expanded syntax
        let expanded = self.macro_expander.expand(syntax)?;

        // 4. Desugar: expanded syntax → Core IR
        let core_ir = desugar::desugar(expanded)?;

        Ok(core_ir)
    }
}
```

**Dependencies:** `patina-runtime`, `patina-ir`

**Key Insight:** Frontend produces **Core IR** with NO macros, NO derived forms. Backend never sees `cond`, `let`, `and`, etc.

---

### 4. `patina-nanopass` (Optimization Passes)

**Purpose:** Multiple small transformation passes on Core IR

```
crates/patina-nanopass/
├── Cargo.toml
└── src/
    ├── lib.rs                # Public API
    ├── pipeline.rs           # Pass orchestration
    │
    ├── passes/
    │   ├── mod.rs
    │   ├── normalize.rs      # Normalize to simpler forms
    │   ├── assignment.rs     # Convert set! to box operations
    │   ├── closure.rs        # Closure conversion
    │   ├── inline.rs         # Inline primitives and small functions
    │   ├── constant_fold.rs  # Constant folding
    │   ├── dead_code.rs      # Dead code elimination
    │   ├── cps.rs            # CPS conversion (optional, for call/cc)
    │   └── optimize_calls.rs # Optimize known procedure calls
    │
    └── util/
        ├── free_vars.rs      # Free variable analysis
        ├── used_vars.rs      # Variable usage analysis
        └── inline_cost.rs    # Inlining cost model
```

**Example Pass:**

```rust
// passes/constant_fold.rs
pub struct ConstantFoldingPass;

impl ExprVisitor for ConstantFoldingPass {
    type Output = CoreExpr;

    fn visit_expr(&mut self, expr: &CoreExpr) -> CoreExpr {
        match expr {
            // Fold (+ 1 2) → 3
            CoreExpr::PrimCall { prim: Primitive::Add, args }
                if all_literals(args) => {
                    CoreExpr::Literal(eval_const_add(args))
                }

            // Fold (if #t a b) → a
            CoreExpr::If { test, then, else_ }
                if is_literal_true(test) => {
                    self.visit_expr(then)
                }

            // Recursively visit children
            _ => expr.map_children(|child| self.visit_expr(child))
        }
    }
}
```

**Pass Pipeline:**

```rust
pub struct NanopassPipeline {
    passes: Vec<Box<dyn Pass>>,
}

impl NanopassPipeline {
    pub fn optimize(&self, expr: CoreExpr) -> CoreExpr {
        let mut current = expr;

        // Run each pass in sequence
        for pass in &self.passes {
            current = pass.transform(current);
        }

        current
    }
}
```

**Dependencies:** `patina-runtime`, `patina-ir`

**Educational Value:** Each pass is ~100-300 lines, easy to understand in isolation.

---

### 5. `patina-backend-tree` (Tree-Walking Interpreter)

**Purpose:** Simple, educational tree-walking interpreter

```
crates/patina-backend-tree/
├── Cargo.toml
└── src/
    ├── lib.rs                # Public API
    ├── evaluator.rs          # Core eval loop
    ├── primitives/
    │   ├── mod.rs
    │   ├── arithmetic.rs
    │   ├── lists.rs
    │   ├── strings.rs
    │   ├── vectors.rs
    │   └── predicates.rs
    ├── application.rs        # Procedure application
    └── debug.rs              # Debug tracing
```

**Evaluator:**

```rust
pub struct TreeWalkingEvaluator {
    global_env: Rc<Environment>,
    debug: DebugConfig,
}

impl TreeWalkingEvaluator {
    /// Evaluate Core IR expression
    pub fn eval(&mut self, expr: &CoreExpr, env: &Rc<Environment>)
        -> Result<Value, EvalError>
    {
        match expr {
            CoreExpr::Literal(v) => Ok(v.clone()),
            CoreExpr::Var(s) => env.get(s),
            CoreExpr::Quote(v) => Ok(v.clone()),

            CoreExpr::Lambda { params, body } => {
                Ok(Value::Lambda {
                    params: params.clone(),
                    body: body.clone(),
                    env: Rc::clone(env),
                })
            }

            CoreExpr::If { test, then, else_ } => {
                let test_val = self.eval(test, env)?;
                if test_val.is_truthy() {
                    self.eval(then, env)
                } else {
                    self.eval(else_, env)
                }
            }

            CoreExpr::PrimCall { prim, args } => {
                let arg_vals = args.iter()
                    .map(|a| self.eval(a, env))
                    .collect::<Result<Vec<_>, _>>()?;
                primitives::apply_primitive(prim, &arg_vals)
            }

            CoreExpr::App { func, args } => {
                let func_val = self.eval(func, env)?;
                let arg_vals = args.iter()
                    .map(|a| self.eval(a, env))
                    .collect::<Result<Vec<_>, _>>()?;
                self.apply(func_val, arg_vals)
            }

            // ... other cases
        }
    }
}
```

**Dependencies:** `patina-runtime`, `patina-ir`

**Philosophy:** Simple, correct, educational. Performance is NOT a goal.

---

### 6. `patina-backend-vm` (Bytecode VM - Future)

**Purpose:** Bytecode compiler + virtual machine for faster execution

```
crates/patina-backend-vm/
├── Cargo.toml
└── src/
    ├── lib.rs                # Public API
    │
    ├── compiler/
    │   ├── mod.rs            # Core IR → Bytecode
    │   ├── codegen.rs        # Code generation
    │   ├── optimize.rs       # Bytecode-level optimizations
    │   └── constant_pool.rs  # Constant pool management
    │
    ├── vm/
    │   ├── mod.rs            # Virtual machine
    │   ├── interpreter.rs    # Bytecode interpreter
    │   ├── stack.rs          # Value stack
    │   └── call_frame.rs     # Call frames
    │
    ├── opcodes.rs            # Instruction set
    └── disassembler.rs       # Bytecode disassembler (debugging)
```

**Instruction Set:**

```rust
#[derive(Debug, Clone, Copy)]
pub enum OpCode {
    // Constants
    LoadConst(u16),      // Push from constant pool
    LoadNil,
    LoadTrue,
    LoadFalse,

    // Variables
    LoadLocal(u8),       // Load from local environment
    LoadUpvalue(u8),     // Load from closure
    StoreLocal(u8),
    StoreUpvalue(u8),

    // Arithmetic
    Add, Sub, Mul, Div,
    Eq, Lt, Gt,

    // Control flow
    Jump(i16),
    JumpIfFalse(i16),
    Return,

    // Calls
    Call(u8),            // Call with N arguments
    TailCall(u8),        // Tail call (reuses frame)

    // Lists
    Cons, Car, Cdr,

    // Closures
    MakeClosure { func: u16, upvalues: u8 },
}
```

**Compilation:**

```rust
pub struct BytecodeCompiler {
    code: Vec<OpCode>,
    constants: Vec<Value>,
    local_vars: HashMap<Symbol, u8>,
}

impl BytecodeCompiler {
    pub fn compile(&mut self, expr: &CoreExpr) -> Result<CompiledFunction, CompileError> {
        match expr {
            CoreExpr::Literal(v) => {
                let idx = self.add_constant(v.clone());
                self.emit(OpCode::LoadConst(idx));
            }

            CoreExpr::If { test, then, else_ } => {
                self.compile(test)?;
                let jump_false = self.emit_jump(OpCode::JumpIfFalse(0));

                self.compile(then)?;
                let jump_end = self.emit_jump(OpCode::Jump(0));

                self.patch_jump(jump_false);
                self.compile(else_)?;

                self.patch_jump(jump_end);
            }

            CoreExpr::App { func, args } if is_tail_position => {
                for arg in args {
                    self.compile(arg)?;
                }
                self.compile(func)?;
                self.emit(OpCode::TailCall(args.len() as u8));
            }

            // ... other cases
        }

        Ok(CompiledFunction {
            code: self.code.clone(),
            constants: self.constants.clone(),
        })
    }
}
```

**Virtual Machine:**

```rust
pub struct VM {
    stack: Vec<Value>,
    call_frames: Vec<CallFrame>,
    globals: Rc<Environment>,
}

impl VM {
    pub fn run(&mut self, func: &CompiledFunction) -> Result<Value, VMError> {
        let mut ip = 0;  // Instruction pointer

        loop {
            let opcode = func.code[ip];

            match opcode {
                OpCode::LoadConst(idx) => {
                    self.stack.push(func.constants[idx].clone());
                }

                OpCode::Add => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(add(a, b)?);
                }

                OpCode::TailCall(argc) => {
                    // Tail call optimization: reuse current frame
                    let args = self.pop_n(argc as usize);
                    let func = self.stack.pop().unwrap();

                    // Pop current frame
                    self.call_frames.pop();

                    // Push new frame (reuses stack space)
                    self.call_frames.push(CallFrame::new(func, args));
                    ip = 0;  // Jump to start of new function
                    continue;
                }

                OpCode::Return => {
                    let result = self.stack.pop().unwrap();
                    if self.call_frames.is_empty() {
                        return Ok(result);
                    }
                    let frame = self.call_frames.pop().unwrap();
                    ip = frame.return_address;
                    // Result already on stack
                }

                // ... other opcodes
            }

            ip += 1;
        }
    }
}
```

**Dependencies:** `patina-runtime`, `patina-ir`

**Advantages over tree-walking:**
- ✅ 3-10x faster execution
- ✅ Better memory usage
- ✅ Native TCO support (TailCall opcode)
- ✅ Can add JIT later

---

### 7. `patina-cli` (Binary Crate)

**Purpose:** Command-line interface and REPL

```
crates/patina-cli/
├── Cargo.toml
└── src/
    ├── main.rs                # Entry point
    ├── cli/                   # Command-line interface
    │   ├── mod.rs
    │   ├── args.rs           # Argument parsing (clap)
    │   └── commands.rs       # Subcommands
    ├── repl/                  # Interactive REPL
    │   ├── mod.rs
    │   ├── completer.rs      # Tab completion
    │   ├── highlighter.rs    # Syntax highlighting
    │   ├── validator.rs      # Input validation
    │   └── history.rs        # Command history
    └── config/                # Configuration
        ├── mod.rs
        └── settings.rs
```

**Features:**
- ✅ Subcommands: `patina run`, `patina repl`, `patina test`, `patina debug`
- ✅ Configuration file support (`~/.patina/config.toml`)
- ✅ Rich REPL with rustyline integration

---

### 3. `patina-stdlib` (Mixed Rust/Scheme)

**Purpose:** Standard library implementations

```
crates/patina-stdlib/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # Loads and exposes standard libraries
    ├── scheme/                # R7RS implementations in Rust
    │   ├── mod.rs
    │   ├── base.rs           # (scheme base) - Rust side
    │   ├── char.rs           # (scheme char)
    │   ├── write.rs          # (scheme write)
    │   └── file.rs           # (scheme file)
    ├── srfi/                  # SRFI implementations
    │   ├── mod.rs
    │   └── srfi_1.rs         # SRFI-1 list library
    └── patina/                # Patina extensions
        ├── mod.rs
        ├── debug.rs
        └── reactive.rs        # Future
```

**Philosophy:**
- **Performance-critical code in Rust** (math, string ops)
- **High-level logic in Scheme** (loaded from `lib/`)
- **Rust provides primitives, Scheme composes them**

Example:
```rust
// patina-stdlib/src/scheme/base.rs
pub fn install_base_primitives(env: &mut Environment) {
    // Low-level primitives in Rust
    env.define("+", Value::Primitive(...));
    env.define("car", Value::Primitive(...));

    // Load high-level functions from Scheme
    load_scheme_library("lib/scheme/base.sld", env);
}
```

---

### 4. `patina-debugger` (Future)

**Purpose:** Debug protocol server (LSP-like)

```
crates/patina-debugger/
└── src/
    ├── lib.rs
    ├── server/                # Debug server
    │   ├── mod.rs
    │   ├── protocol.rs       # Debug protocol
    │   └── session.rs
    ├── breakpoints/
    │   ├── mod.rs
    │   └── manager.rs
    └── inspector/             # Variable inspection
        └── mod.rs
```

**Future integration** with VSCode, Emacs, etc.

---

## Scheme Library Organization

### Structure

```
lib/
├── scheme/                    # R7RS standard libraries
│   ├── base.sld              # Core language
│   ├── case-lambda.sld
│   ├── char.sld
│   ├── complex.sld
│   ├── cxr.sld
│   ├── eval.sld
│   ├── file.sld
│   ├── inexact.sld
│   ├── lazy.sld
│   ├── load.sld
│   ├── process-context.sld
│   ├── r5rs.sld
│   ├── read.sld
│   ├── repl.sld
│   ├── time.sld
│   └── write.sld
│
├── srfi/                      # Scheme Requests for Implementation
│   ├── 1.sld                 # List library
│   ├── 9.sld                 # Records
│   ├── 64.sld                # Testing
│   └── ...
│
├── patina/                    # Patina-specific libraries
│   ├── debug.sld             # Debugging facilities
│   │   # (import (patina debug))
│   │   # (trace expr), (break-at func), (profile expr)
│   │
│   ├── reactive.sld          # Reactive streams (future)
│   │   # (import (patina reactive))
│   │   # (make-stream), (subscribe), (publish)
│   │
│   ├── notebook.sld          # Notebook support (future)
│   │   # (import (patina notebook))
│   │   # (display-table), (plot)
│   │
│   └── ffi.sld               # Foreign function interface
│       # (import (patina ffi))
│       # (call-rust "function-name" args...)
│
└── bootstrap.scm              # Minimal bootstrap code
```

### Third-Party Library Organization

**Purpose:** Manage external libraries for testing and optional distribution

```
lib/
├── scheme/                    # Patina's R7RS implementations (first-party)
├── srfi/                      # Patina's SRFI implementations (first-party)
├── patina/                    # Patina extensions (first-party)
└── vendor/                    # Third-party libraries (NEW)
    ├── snow/                  # Snow package repository
    │   ├── srfi/              # SRFI reference implementations
    │   ├── chibi/             # Chibi-Scheme libraries
    │   └── ...
    └── README.md              # Attribution and licensing info
```

**Rationale:**

1. **Clear separation** - First-party vs third-party code
2. **Standard practice** - Similar to `node_modules`, `vendor/` in Go/Ruby
3. **Easy attribution** - Licensing and source tracking
4. **Flexible distribution** - Can bundle or exclude for releases

**Library Search Path Priority:**

```rust
pub struct LibrarySearchPaths {
    paths: Vec<PathBuf>,
}

impl Default for LibrarySearchPaths {
    fn default() -> Self {
        Self {
            paths: vec![
                PathBuf::from("lib/scheme"),      // R7RS standard (Patina)
                PathBuf::from("lib/patina"),      // Patina extensions
                PathBuf::from("lib/srfi"),        // SRFI (Patina + tested)
                PathBuf::from("lib/vendor/snow"), // Snow packages
                PathBuf::from("."),               // User's working directory
            ]
        }
    }
}
```

**Usage Example:**

```scheme
;; lib/vendor/snow/srfi/srfi-1.sld - List library from Snow
(define-library (srfi 1)
  (export filter fold map ...)
  (begin ...))

;; Your code can import it
(import (srfi 1))
(filter odd? '(1 2 3 4 5))  ; => (1 3 5)
```

**Licensing and Attribution:**

Create `lib/vendor/README.md`:

```markdown
# Third-Party Libraries

This directory contains libraries from external sources used for testing
and optionally bundled with Patina releases.

## Snow Packages
- **Source**: https://snow-fort.org/
- **License**: Various (see individual package LICENSE files)
- **Usage**: Testing R7RS compatibility, optional standard library

## Included Packages
- srfi/* - Scheme Request for Implementation reference implementations
- chibi/* - Chibi-Scheme library implementations

See LICENSE files in subdirectories for specific licensing terms.
```

**Git Configuration:**

Recommend checking vendor libraries into git for:
- ✅ Reproducible builds
- ✅ No external dependencies at build time
- ✅ Consistent testing across environments

If you prefer NOT to check them in, add to `.gitignore`:
```gitignore
lib/vendor/
```

**CLI Integration:**

```bash
# Use only builtin libraries (skip vendor/)
patina --no-vendor repl

# Add custom library search path
patina --library-path ~/my-scheme-libs repl

# Show library search paths
patina --show-library-paths
```

**Implementation in `crates/patina-core/src/library/loader.rs`:**

```rust
pub struct LibraryLoader {
    search_paths: LibrarySearchPaths,
    allow_vendor: bool,
}

impl LibraryLoader {
    pub fn find_library(&self, name: &LibraryName) -> Result<PathBuf, LibraryError> {
        for path in &self.search_paths.paths {
            // Skip vendor path if --no-vendor flag set
            if !self.allow_vendor && path.starts_with("lib/vendor") {
                continue;
            }

            let lib_path = self.resolve_library_path(name, path)?;
            if lib_path.exists() {
                return Ok(lib_path);
            }
        }
        Err(LibraryError::NotFound(name.clone()))
    }
}
```

**Benefits:**

- ✅ Clear separation of first-party vs third-party code
- ✅ Easy to track licensing and attribution
- ✅ Flexible for both testing and distribution
- ✅ Follows standard practices from other ecosystems
- ✅ Configurable via CLI flags
- ✅ Works seamlessly with module system (when implemented)

---

### Library File Format

**Standard R7RS format (.sld):**
```scheme
(define-library (scheme write)
  (import (scheme base))
  (export display write newline write-char write-string)
  (begin
    ;; Implementations or (include "write-impl.scm")
    ))
```

**Mixed approach (Rust + Scheme):**
```scheme
(define-library (scheme base)
  (import (patina primitives))  ; Rust-implemented primitives
  (export + - * / car cdr cons map fold ...)
  (begin
    ;; High-level functions built on primitives
    (define (fold f init lst)
      (if (null? lst)
          init
          (fold f (f (car lst) init) (cdr lst))))))
```

---

## Debugging Infrastructure

### Current Debug Module

```rust
// src/eval/debug.rs - Current basic implementation
pub struct DebugConfig {
    pub enabled_stages: HashSet<DebugStage>,
}

pub enum DebugStage {
    Eval,    // Expression evaluation
    Apply,   // Procedure application
    Expand,  // Macro expansion
}
```

### Enhanced Debug System

```rust
// crates/patina-core/src/debug/mod.rs
pub mod trace;
pub mod backtrace;
pub mod profiler;
pub mod hooks;

pub struct Debugger {
    tracing: TraceConfig,
    breakpoints: BreakpointManager,
    backtrace: BacktraceCollector,
    profiler: Profiler,
    hooks: HookManager,
}

// crates/patina-core/src/debug/trace.rs
pub struct TraceConfig {
    pub trace_eval: bool,
    pub trace_apply: bool,
    pub trace_macro: bool,
    pub trace_import: bool,
    pub output: TraceOutput,
}

pub enum TraceOutput {
    Stderr,
    File(PathBuf),
    Callback(Box<dyn Fn(&TraceEvent)>),
}

pub struct TraceEvent {
    pub timestamp: Instant,
    pub depth: usize,
    pub kind: TraceKind,
    pub expr: Value,
    pub env_id: usize,
}

// crates/patina-core/src/debug/backtrace.rs
pub struct CallStack {
    frames: Vec<CallFrame>,
}

pub struct CallFrame {
    pub function_name: Option<String>,
    pub expr: Value,
    pub env: Rc<Environment>,
    pub source_location: Option<SourceLocation>,  // Future
}

impl CallStack {
    pub fn push(&mut self, frame: CallFrame) { }
    pub fn pop(&mut self) { }
    pub fn format_backtrace(&self) -> String { }
}

// crates/patina-core/src/debug/profiler.rs
pub struct Profiler {
    samples: HashMap<String, ProfilingSample>,
}

pub struct ProfilingSample {
    pub call_count: u64,
    pub total_time: Duration,
    pub self_time: Duration,
}

// crates/patina-core/src/debug/hooks.rs
pub trait DebugHook: Send + Sync {
    fn on_eval_enter(&self, expr: &Value, env: &Rc<Environment>);
    fn on_eval_exit(&self, expr: &Value, result: &Result<Value, EvalError>);
    fn on_apply(&self, proc: &Value, args: &[Value]);
}

pub struct HookManager {
    hooks: Vec<Box<dyn DebugHook>>,
}
```

### Usage Examples

```rust
// Enable comprehensive debugging
let mut debugger = Debugger::new();
debugger.trace_config.trace_eval = true;
debugger.trace_config.trace_apply = true;

// Add custom hook
debugger.hooks.add(Box::new(CustomDebugHook));

// Profile code
let profiler = Profiler::new();
profiler.start("my-function");
evaluator.eval(expr)?;
profiler.stop("my-function");
profiler.report();
```

---

## Build and Development Workflow

### Cargo Workspace Configuration

```toml
# Root Cargo.toml
[workspace]
members = [
    "crates/patina-core",
    "crates/patina-cli",
    "crates/patina-stdlib",
]
resolver = "2"

[workspace.dependencies]
# Shared dependencies
num-bigint = "0.4"
num-rational = "0.4"
num-complex = "0.4"

[profile.dev]
opt-level = 1  # Faster debug builds

[profile.release]
lto = true
codegen-units = 1
```

```toml
# crates/patina-core/Cargo.toml
[package]
name = "patina-core"
version = "0.1.0"
edition = "2021"

[dependencies]
num-bigint = { workspace = true }
num-rational = { workspace = true }
num-complex = { workspace = true }
thiserror = "1.0"

[features]
default = ["std"]
std = []
debug-trace = []
profiling = ["dep:flamegraph"]
```

### Development Commands

```bash
# Build everything
cargo build --workspace

# Build specific crate
cargo build -p patina-core

# Run REPL
cargo run -p patina-cli -- repl

# Run with debug tracing
PATINA_DEBUG=eval,apply cargo run -p patina-cli

# Profile
cargo build --release --features profiling

# Test
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format
cargo fmt --all
```

---

## Testing Strategy

### Test Organization

```
tests/
├── compliance/                # R7RS compliance
│   ├── primitives.rs
│   ├── numbers.rs
│   ├── lists.rs
│   └── macros.rs
│
├── integration/               # Integration tests
│   ├── library_loading.rs
│   ├── import_resolution.rs
│   └── repl_session.rs
│
├── performance/               # Benchmarks
│   ├── fibonacci.rs
│   ├── ackermann.rs
│   └── macro_expansion.rs
│
├── regression/                # Regression tests
│   ├── issue_001.rs
│   └── issue_002.rs
│
└── chibi/                     # Chibi test suite
    ├── r7rs-tests.scm
    └── runner.rs
```

### Test Utilities

```rust
// tests/common/mod.rs
pub fn assert_eval_to(expr: &str, expected: &str) {
    let interpreter = Interpreter::new();
    let result = interpreter.eval_str(expr).unwrap();
    assert_eq!(result.to_string(), expected);
}

pub fn assert_eval_error(expr: &str) {
    let interpreter = Interpreter::new();
    assert!(interpreter.eval_str(expr).is_err());
}

pub fn with_debug_trace<F>(test: F)
where
    F: FnOnce(&mut Interpreter),
{
    let mut interpreter = Interpreter::new();
    interpreter.debugger.enable_tracing();
    test(&mut interpreter);
    interpreter.debugger.dump_trace();
}
```

---

## Documentation Strategy

### User Documentation (docs/)

```
docs/
├── guide/
│   ├── getting-started.md
│   ├── language-reference.md
│   ├── standard-library.md
│   └── debugging.md
├── tutorials/
│   ├── your-first-program.md
│   └── writing-libraries.md
└── api/                       # Generated rustdoc
```

### Implementation Documentation (internal/)

```
internal/
├── ARCHITECTURE.md            # Overall architecture
├── EVALUATOR_DESIGN.md       # Evaluator internals
├── MACRO_SYSTEM.md           # Macro implementation
├── DEBUGGING.md              # Debugging system
└── ARCHIVE/                   # Historical docs
```

### Code Documentation

```rust
/// Evaluates a Scheme expression in the given environment.
///
/// # Arguments
///
/// * `expr` - The expression to evaluate
/// * `env` - The environment for variable lookups
///
/// # Examples
///
/// ```
/// let env = Environment::new();
/// let expr = parse("(+ 1 2)");
/// let result = evaluator.eval(&expr, &env)?;
/// assert_eq!(result, Value::Integer(3));
/// ```
///
/// # Errors
///
/// Returns `EvalError` if evaluation fails.
pub fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // Implementation
}
```

---

## Migration Path to Nanopass Architecture

### Overview

This migration can be done **incrementally** while keeping the current interpreter working. Each phase delivers value independently.

**Strategy:** "Extract, don't rewrite"
- Keep current code working
- Extract components into new structure
- Gradually wire new components together
- Old code can coexist with new during migration

---

### Phase 1: Foundation - Runtime & IR (1-2 weeks)

**Goal:** Extract stable foundation that won't change

**Tasks:**
1. Create `crates/patina-runtime/`
   - Extract `Value` enum from `src/value/mod.rs`
   - Extract `Environment` from `src/env/mod.rs`
   - Extract error types
   - Add comprehensive tests

2. Create `crates/patina-ir/`
   - Define `CoreExpr` enum (7 core forms)
   - Define `SurfaceSyntax` (pre-macro expansion)
   - Implement builder functions
   - Implement visitor trait

3. Update `Cargo.toml` for workspace
   - Configure workspace members
   - Set up shared dependencies

**Deliverable:** Stable foundation for all other crates

**Status:** Can continue using old evaluator during this phase

---

### Phase 2: Frontend Extraction (2-3 weeks)

**Goal:** Unified frontend that produces Core IR

**Tasks:**
1. Create `crates/patina-frontend/`
   - Move `src/lexer/` → `patina-frontend/src/lexer/`
   - Move `src/parser/` → `patina-frontend/src/parser/`
   - Move `src/macro_system/` → `patina-frontend/src/macro_expander/`

2. Implement desugar module
   - `cond` → nested `if`
   - `case` → nested `if`
   - `and` → `if`
   - `or` → `if`
   - `let`/`let*`/`letrec` → `lambda` + application
   - Validate all tests pass

3. Wire frontend pipeline
   - `Frontend::compile(source)` → `CoreExpr`
   - Integration tests comparing old vs new

**Deliverable:** Frontend that produces minimal Core IR

**Status:** Old evaluator still works; new frontend can be tested independently

---

### Phase 3: Backend Extraction (1-2 weeks)

**Goal:** Tree-walking backend using Core IR

**Tasks:**
1. Create `crates/patina-backend-tree/`
   - Extract `src/eval/mod.rs` → evaluator for Core IR only
   - Move `src/eval/primitives/` → `patina-backend-tree/src/primitives/`
   - Simplify evaluator (only handles 7 core forms!)

2. Wire together
   - `Frontend::compile(source)` → `CoreExpr`
   - `TreeWalkingEvaluator::eval(core_ir)` → `Value`

3. Verify parity
   - Run all existing tests
   - Compare output with old implementation
   - Fix any discrepancies

**Deliverable:** Complete working pipeline (Frontend → Backend)

**Status:** Can delete old evaluator after verification

---

### Phase 4: Nanopass Infrastructure (1-2 weeks)

**Goal:** Optimization pass framework

**Tasks:**
1. Create `crates/patina-nanopass/`
   - Implement `Pass` trait
   - Implement `NanopassPipeline`
   - Add basic passes:
     - Constant folding
     - Dead code elimination
     - Primitive call recognition

2. Wire into pipeline
   - `Frontend::compile()` → Core IR
   - `NanopassPipeline::optimize()` → Optimized IR
   - `Backend::eval()` → Value

3. Benchmarks
   - Compare performance with/without optimization
   - Measure compilation overhead

**Deliverable:** Working nanopass optimization framework

**Status:** Optional passes, can be disabled for debugging

---

### Phase 5: VM Backend (Future - 4-6 weeks)

**Goal:** Bytecode compiler + VM

**Tasks:**
1. Create `crates/patina-backend-vm/`
   - Design instruction set
   - Implement bytecode compiler
   - Implement VM interpreter
   - Add TCO support (TailCall opcode)

2. Integration
   - Same frontend and nanopass pipeline
   - Backend selection at runtime

3. Performance validation
   - Benchmark suite
   - Ensure 3-10x speedup over tree-walking

**Deliverable:** Production-ready bytecode backend

**Status:** Fully independent from tree-walking backend

---

### Phase 6: CLI Modernization (1 week)

**Goal:** Modern CLI with backend selection

**Tasks:**
1. Update `crates/patina-cli/`
   - Add `--backend` flag (tree/vm)
   - Add `--optimize` flag (enable/disable passes)
   - Add `--dump-ir` for debugging
   - Add `--dump-bytecode` for VM debugging

2. REPL enhancements
   - Show optimization stats
   - Backend switching at runtime
   - IR/bytecode inspection commands

**Deliverable:** Production-quality CLI

---

## Timeline Summary

| Phase | Duration | Effort | Status |
|-------|----------|--------|--------|
| Phase 1: Foundation | 1-2 weeks | Medium | Can start now |
| Phase 2: Frontend | 2-3 weeks | High | After Phase 1 |
| Phase 3: Backend | 1-2 weeks | Medium | After Phase 2 |
| Phase 4: Nanopass | 1-2 weeks | Medium | After Phase 3 |
| Phase 5: VM | 4-6 weeks | High | Future (optional) |
| Phase 6: CLI | 1 week | Low | After Phase 4 |

**Total (Phases 1-4):** 6-9 weeks to complete nanopass architecture
**Total (Phases 1-6):** 10-16 weeks for full multi-backend system

**Incremental value:**
- After Phase 1: Stable foundation
- After Phase 2: Clean frontend with desugaring
- After Phase 3: Full working system with new architecture
- After Phase 4: Optimization framework (educational value!)
- After Phase 5: Fast VM backend (production value!)
- After Phase 6: Professional CLI

---

## Risk Mitigation

### Keep Old Code During Migration

```
src/
├── (old code stays here during migration)
├── lexer/ → eventually deleted
├── parser/ → eventually deleted
├── eval/ → eventually deleted

crates/
├── patina-frontend/ → new
├── patina-backend-tree/ → new
└── ...
```

**Tests use new code, old code as reference for verification.**

### Gradual Feature Flag Migration

```rust
// During migration
#[cfg(feature = "new-architecture")]
use patina_frontend::Frontend;

#[cfg(not(feature = "new-architecture"))]
use crate::old_evaluator::Evaluator;
```

### Continuous Testing

- Run full test suite after each phase
- Compare output with reference implementation (Chibi)
- Performance regression tests

**Total Migration:** 6-16 weeks depending on scope, but **incremental and low-risk**!

---

## Benefits of Nanopass Architecture

### For Education 📚

✅ **Understandable** - Each pass is ~100-300 lines, easy to comprehend
✅ **Teachable** - Can show students how compilers work step-by-step
✅ **Debuggable** - Can dump IR after each pass to see transformations
✅ **Extensible** - Adding new passes is straightforward
✅ **Reference quality** - Clean implementation suitable for learning

### For Development 🛠️

✅ **Modular** - Each crate/pass has single responsibility
✅ **Testable** - Test frontend, passes, backends independently
✅ **Maintainable** - Small, focused modules (no 1500-line files!)
✅ **Flexible** - Can add/remove/reorder passes
✅ **Fast iteration** - Workspace parallelism, incremental compilation

### For Performance ⚡

✅ **Optimization-ready** - Nanopass framework enables sophisticated optimizations
✅ **Multi-backend** - Choose best backend for use case (tree/VM/JIT)
✅ **Minimal IR** - Backend only handles ~7 core forms (simple & fast)
✅ **Proven approach** - Chez Scheme uses this architecture for production

### For R7RS Compliance ✅

✅ **Clean separation** - Macro expansion happens once, up front
✅ **Correct semantics** - Desugaring follows R7RS exactly
✅ **Testable** - Each transformation can be tested independently
✅ **Future-proof** - Easy to add R7RS-large features later

### For Future Features 🚀

✅ **Gradual typing** (Phase 2) - Add type inference pass
✅ **Reactive concurrency** (Phase 3) - Add stream transformation pass
✅ **Logic programming** (Phase 4) - Add miniKanren pass
✅ **Call/cc** - Optional CPS conversion pass
✅ **JIT compilation** - Bytecode VM → native code

---

## Comparison: Current vs Nanopass Architecture

| Aspect | Current (Monolithic) | Nanopass Architecture |
|--------|---------------------|----------------------|
| **Frontend** | Mixed eval + macros | Unified: Lex → Parse → Macro → Desugar |
| **IR** | AST (complex) | Core IR (~7 forms, minimal) |
| **Optimization** | Ad-hoc in evaluator | Composable passes |
| **Backends** | Single (tree-walker) | Multiple (tree/VM/JIT) |
| **Macro handling** | Evaluator checks macros | Frontend expands all macros |
| **Derived forms** | Special form or macro | Frontend desugars to core |
| **TCO** | Manual trampolines | Native in VM backend |
| **Code size** | Large files (1500+ lines) | Small modules (100-300 lines) |
| **Testability** | Integration tests mainly | Unit test each component |
| **Educational value** | Good (simple) | Excellent (shows compiler pipeline) |
| **Performance** | Adequate | Excellent (with VM) |

---

## Educational Impact

This architecture makes Patina an **excellent teaching tool** for:

### Compiler Construction Courses

Students can learn:
1. **Lexing & Parsing** - Classic tokenization and parsing
2. **Macro Systems** - Hygienic macro expansion (real R7RS implementation!)
3. **Desugaring** - How high-level forms reduce to core forms
4. **IR Design** - Why minimal IRs simplify backends
5. **Optimization** - How passes compose to optimize code
6. **Code Generation** - Tree-walking vs bytecode vs native
7. **Virtual Machines** - Stack-based bytecode interpreter

### Each component is:
- ✅ **Self-contained** (~100-500 lines per module)
- ✅ **Well-documented** with examples and tests
- ✅ **Incrementally buildable** (students can implement passes)
- ✅ **Real-world** (not toy examples, full R7RS!)

---

## Recommended Next Steps

### Option A: Start Immediately (Aggressive)

**Start with Phase 1 (Foundation) NOW:**
1. Create `crates/patina-runtime/` - Extract Value & Environment
2. Create `crates/patina-ir/` - Define CoreExpr enum
3. Set up workspace Cargo.toml

**Benefits:**
- Foundation for all future work
- Low risk (old code still works)
- ~1-2 weeks

### Option B: Finish Phase 1 R7RS First (Conservative)

**Complete current R7RS goals:**
1. Advanced math functions (sin, cos, exp, log, sqrt)
2. Basic I/O (display, write, read)
3. Exception handling (guard, raise)
4. File I/O

**Then start nanopass migration.**

**Benefits:**
- Feature-complete R7RS implementation
- More stable foundation for migration
- ~3-4 weeks, then migration

### Recommendation: **Option A (Start Foundation Now)**

**Rationale:**
1. Foundation work is independent of R7RS features
2. Can extract Value/Environment while continuing R7RS work
3. Gets workspace benefits immediately
4. Enables parallel development (some work on R7RS, some on architecture)

**Concrete first steps:**
```bash
# 1. Create workspace structure
mkdir -p crates/patina-runtime/src
mkdir -p crates/patina-ir/src

# 2. Set up root Cargo.toml with workspace
# 3. Extract Value & Environment to patina-runtime
# 4. Define CoreExpr in patina-ir
# 5. Update existing code to use workspace crates
# 6. Verify all tests still pass
```

**Timeline:** Can have foundation working in 1-2 weeks while continuing R7RS work!

This architecture will serve Patina through all future phases: R7RS compliance, gradual typing, reactive programming, logic programming, and beyond! 🎉
