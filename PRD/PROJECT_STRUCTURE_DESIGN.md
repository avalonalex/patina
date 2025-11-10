# Patina Project Structure Design

**Goal:** Create a practical, production-quality project structure that:
- Leverages Rust's strengths (type safety, performance, tooling)
- Provides excellent debugging facilities
- Maintains clear separation of concerns
- Scales well as the project grows
- Balances pragmatism with R7RS compliance

**Philosophy:** "Rust where it helps, Scheme where it makes sense"

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

## Proposed Project Structure

### Top-Level Layout

```
patina/
├── crates/                    # Workspace for multiple crates
│   ├── patina-core/          # Core interpreter (library)
│   ├── patina-cli/           # CLI and REPL (binary)
│   ├── patina-stdlib/        # Standard library (mixed Rust/Scheme)
│   ├── patina-macros/        # Procedural macros if needed
│   └── patina-debugger/      # Debug server (future)
├── lib/                       # Scheme standard libraries
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

## Detailed Crate Structure

### 1. `patina-core` (Library Crate)

**Purpose:** Core interpreter engine, usable as a library

```
crates/patina-core/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # Public API
    │
    ├── runtime/               # Runtime system
    │   ├── mod.rs
    │   ├── value.rs          # Value types
    │   ├── environment.rs     # Environment/scope
    │   ├── procedure.rs       # Procedure representation
    │   └── port.rs            # I/O ports (future)
    │
    ├── frontend/              # Parsing pipeline
    │   ├── mod.rs
    │   ├── lexer.rs
    │   ├── token.rs
    │   ├── parser.rs
    │   └── syntax.rs          # AST types
    │
    ├── eval/                  # Evaluator
    │   ├── mod.rs
    │   ├── error.rs
    │   ├── evaluator.rs       # Main evaluator
    │   ├── special_forms/     # Split by category
    │   │   ├── mod.rs
    │   │   ├── binding.rs    # let, let*, letrec, etc.
    │   │   ├── control.rs    # if, cond, case
    │   │   ├── iteration.rs  # do
    │   │   └── definition.rs # define
    │   ├── primitives/        # Built-in procedures
    │   │   ├── mod.rs
    │   │   ├── arithmetic.rs
    │   │   ├── lists.rs
    │   │   ├── strings.rs
    │   │   ├── vectors.rs
    │   │   ├── io.rs         # Future
    │   │   └── predicates.rs
    │   └── application.rs     # Procedure application
    │
    ├── macro_system/          # Hygienic macros
    │   ├── mod.rs
    │   ├── expander.rs
    │   ├── pattern.rs
    │   ├── template.rs
    │   └── hygiene.rs
    │
    ├── library/               # Module system
    │   ├── mod.rs
    │   ├── types.rs           # Library, ImportSet
    │   ├── registry.rs        # Library loading
    │   ├── resolver.rs        # Import resolution
    │   ├── loader.rs          # File loading
    │   └── standard.rs        # Standard library binding
    │
    ├── debug/                 # Debugging support
    │   ├── mod.rs
    │   ├── trace.rs           # Execution tracing
    │   ├── backtrace.rs       # Call stack tracking
    │   ├── profiler.rs        # Performance profiling
    │   └── hooks.rs           # Debug hooks
    │
    └── util/                  # Utilities
        ├── mod.rs
        ├── display.rs         # Display formatting
        └── conversions.rs     # Type conversions
```

**Key Improvements:**
- ✅ `special_forms/` split by category (no more 1500-line files!)
- ✅ Dedicated `library/` module for module system
- ✅ Expanded `debug/` with tracing, profiling, backtraces
- ✅ Clear `runtime/` vs `frontend/` vs `eval/` separation
- ✅ All public API in `lib.rs`

---

### 2. `patina-cli` (Binary Crate)

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

## Migration Path

### Phase 1: Workspace Setup (1 day)

1. Create workspace structure
2. Split into `patina-core` and `patina-cli`
3. Update imports
4. Verify all tests pass

### Phase 2: Code Reorganization (2-3 days)

1. Split `special_forms.rs` into `special_forms/` directory
2. Split `primitives.rs` into organized modules
3. Create `library/` module structure
4. Update module paths

### Phase 3: Enhanced Debugging (1-2 days)

1. Expand `debug/` module with tracing
2. Add backtrace support
3. Implement profiler
4. Add debug hooks

### Phase 4: Scheme Libraries (1 day)

1. Create `lib/scheme/` structure
2. Move `bootstrap.scm`
3. Create placeholder `.sld` files
4. Update library loading paths

### Phase 5: Stdlib Crate (2-3 days)

1. Create `patina-stdlib` crate
2. Move primitive installation
3. Create mixed Rust/Scheme approach
4. Wire into core

**Total Migration:** 1-2 weeks (can be done incrementally!)

---

## Benefits of This Structure

### For Development

✅ **Modular** - Each crate has clear responsibility
✅ **Testable** - Easier to test individual components
✅ **Maintainable** - No monster files (kept under 500 lines)
✅ **Reusable** - Core can be used as library
✅ **Fast builds** - Workspace parallelism

### For Users

✅ **Clear library organization** - R7RS, SRFI, Patina extensions
✅ **Good debugging** - Tracing, profiling, backtraces
✅ **Excellent docs** - Rustdoc + user guides
✅ **Easy to extend** - Well-defined APIs

### For R7RS Compliance

✅ **Standard library structure** - Matches R7RS organization
✅ **Module system ready** - Clean place for library loading
✅ **Extensible** - Easy to add missing features

---

## Recommended Next Steps

1. **Start with workspace migration** (Phase 1, 1 day)
   - Low risk, immediate organizational benefits
   - Can be done without changing functionality

2. **Then split large files** (Phase 2, 2-3 days)
   - Makes code more maintainable
   - Easier to find and modify code

3. **Add enhanced debugging** (Phase 3, 1-2 days)
   - Immediate value for development
   - Helps with future debugging

4. **Create library structure** (Phase 4, 1 day)
   - Prepares for module system
   - Organizes existing Scheme code

This structure will serve you well through Phase 1 R7RS compliance and beyond into reactive/linear types/notebook features!

Would you like to start with the workspace migration?
