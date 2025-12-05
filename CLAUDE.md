# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Patina is a Scheme R7RS-small interpreter written in Rust. This is an educational project with ambitious goals: implementing a full R7RS-compliant Scheme interpreter, then building a bytecode VM for performance, adding syntax-case for procedural macros, and ultimately extending with gradual typing, reactive concurrency, and logic programming. Currently in Phase 1 (basic R7RS compliance).

**Architecture:** Modular workspace with 9 crates supporting multiple backends (tree-walker implemented, VM and JIT planned). Features a sophisticated dual-loader library system and CoreExpr IR-based evaluation pipeline.

## Workspace Structure

Patina uses a Rust workspace with 9 crates organized by concern:

```
patina/ (workspace root)
├── Cargo.toml              # Workspace configuration
├── lib/                    # Scheme standard library
│   ├── scheme/             # R7RS library implementations
│   │   ├── base-extras.scm # Core macros and derived procedures for (scheme base)
│   │   └── lazy-extras.scm # Lazy evaluation support for (scheme lazy)
│   └── chibi/              # Chibi scheme test suite
│       └── test-extras.scm # Testing utilities
│
└── crates/
    ├── patina-runtime/     # Core types, Backend trait, Library system
    ├── patina-ir/          # CoreExpr IR definition (7 core forms)
    ├── patina-frontend/    # Lexer, Parser, Desugarer
    ├── patina-macros/      # Macro expansion (syntax-rules with scope sets hygiene)
    ├── patina-pipeline/    # Pipeline orchestration (pluggable strategies)
    ├── patina-tree-walker/ # Tree-walking interpreter backend
    ├── patina-interpreter/ # High-level Interpreter API
    ├── patina-repl/        # Rich terminal REPL
    └── patina-tests/       # All integration & compliance tests (~423 tests)
```

### Crate Responsibilities

**patina-runtime** (`crates/patina-runtime/src/`)
- Core `Value` enum: all 26 Scheme data types
- `Environment`: lexical scoping with parent chains
- `Backend` trait: abstraction for multiple evaluation strategies
- Library system: `LibraryRegistry`, `LibraryLoaderRegistry`, `RustLibraryLoader`
- Standard library builders: `stdlib/scheme_base.rs`, `stdlib/scheme_char.rs`, etc.
- Shared types used by all other crates

**patina-ir** (`crates/patina-ir/src/`)
- `CoreExpr` enum: 9 core forms + 4 additional forms
- Minimal IR for backend evaluation
- Foundation for future nanopass architecture

**patina-frontend** (`crates/patina-frontend/src/`)
- `lexer/`: Tokenizes Scheme source
- `parser/`: Builds AST (as Value) from tokens
- `desugarer/`: Converts macro-expanded AST to CoreExpr IR
  - **Macro-aware**: Expands macros on-demand during desugaring
  - Checks environment for macro bindings
  - No separate pre-expansion phase needed

**patina-macros** (`crates/patina-macros/src/`)
- Hygienic macro expansion (syntax-rules)
- Racket-style scope sets hygiene (based on "Binding as Sets of Scopes", Flatt 2016)
- Flip-scope algorithm for distinguishing use-site vs introduced identifiers
- Pattern matching and template expansion
- Full ellipsis support including nested (`... ...`) and escaping (`(... template)`)
- Separated into own crate for modularity
- **See `docs/MACRO_SYSTEM.md` for comprehensive architecture documentation**

**patina-pipeline** (`crates/patina-pipeline/src/`)
- Pipeline orchestration with pluggable evaluation strategies
- `Pipeline` trait: eval(), eval_program(), strategy()
- `StandardPipeline`: Default parse → eval pipeline
- `EvaluationStrategy` enum: Direct, CoreExpr, Bytecode (future), Jit (future)

**patina-tree-walker** (`crates/patina-tree-walker/src/eval/`)
- Implements `Backend` trait from patina-runtime
- `TreeWalker`: Wraps Evaluator and implements Backend trait
- `Evaluator`: Core evaluation engine with registries
  - `global_env`: Global environment with all primitives
  - `library_registry`: Manages loaded libraries
  - `loader_registry`: Coordinates multiple library loaders
  - `primitive_registry`: Registry of primitive procedures
  - `special_form_registry`: Registry of special forms
- `core_eval.rs`: CoreExpr evaluator (primary path)
- `mod.rs`: Legacy Value evaluator (fallback for let-syntax, case-lambda)
- `special_forms/`: All special form implementations
- `application.rs`: Procedure application logic
- `primitives/`: All built-in procedures organized by category

**patina-interpreter** (`crates/patina-interpreter/src/`)
- High-level `Interpreter<B: Backend>` API (generic over backends)
- `TreeWalkInterpreter`: Type alias for `Interpreter<TreeWalker>`
- `SimpleInterpreter`: Simpler non-generic API using StandardPipeline
- Methods: `eval_str()`, `eval_program()`, `eval_program_resilient()`
- Backend-agnostic interface for easy backend swapping

**patina-repl** (`crates/patina-repl/src/`)
- Rich terminal REPL with rustyline
- Syntax highlighting, history, multi-line input
- Binary executable: `target/release/patina`
- Script mode: `patina script.scm`
- REPL mode: `patina` (interactive)

**patina-tests** (`crates/patina-tests/tests/`)
- R7RS compliance tests (~347 tests in scheme_base.rs)
- Integration tests (library loading, CoreExpr integration)
- API tests (interpreter_api.rs)
- Case-lambda tests (21 tests, currently ignored pending import support)
- Total: ~423 tests passing

### Dependency Flow

```
patina-repl → patina-interpreter → patina-tree-walker → patina-runtime
                                 ↗  patina-frontend    ↗
                                    patina-pipeline
                                    patina-macros
                                    patina-ir
patina-tests → patina-interpreter
```

## Documentation Organization

**Important**: Use the following directories for different types of documentation:

- **IMPORTANT** - Do not create markdown files unless the user explicitly states to do so. You may offer to create markdown files, but only do so with explicit user approval. Integrate any necessary notes as comments within the relevant code files, and keep comments succinct and on point.

### 📁 Active Documentation (Frequently Updated)

- **`PRD/phase1/`** - Strategic planning and high-level design (current phase only)
  - **`IMPLEMENTATION_STATUS.md`** - ⭐ **CANONICAL** overall roadmap, priorities, and remaining work
  - **`TECH_DEBT_CLEANUP.md`** - ⭐ Tech debt to address before Phase 2 (VM backend)
  - **`NUMERIC_SUMMARY.md`** - ⭐ **CANONICAL** guide for numeric tower implementation
  - **`VALUE_SIZE_OPTIMIZATION.md`** - ⭐ Value/CoreExpr size analysis and optimization roadmap
  - `STRING_OPTIMIZATION.md` - Future optimization plans (deferred)
  - `HELP_SYSTEM.md` - Future help system design (deferred)

- **`PRD/phase2/`** - Future enhancements
  - **`SYNTAX_CASE_DESIGN.md`** - ⭐ syntax-case implementation design (future macro system)

- **`docs/`** - User-facing documentation and developer guides
  - **`FEATURE_STATUS.md`** - ⭐ **CANONICAL** detailed test-by-test R7RS compliance matrix
  - **`MACRO_SYSTEM.md`** - ⭐ Comprehensive macro system architecture (syntax-case roadmap)
  - **`R7RS_LARGE_STATUS.md`** - R7RS-large editions tracking (Red, Tangerine, etc.)
  - **`TEST_ORGANIZATION.md`** - ⭐ Test structure and running tests
  - `API.md` - Public API reference
  - `GETTING_STARTED.md` - User guide for getting started
  - `README.md` - Project overview
  - `TESTING.md` - Test infrastructure and usage
  - `DEVELOPMENT.md` - Developer guide

- **`internal/`** - Implementation notes and progress tracking
  - **`MILESTONES.md`** - Historical achievements and progress milestones (keep updated)
  - `MACRO_SYSTEM_KNOWN_LIMITATIONS.md` - Macro system limitations (nested ellipsis now works!)
  - `reference_impls/` - Reference implementation notes (Chibi, Chez)

### 🗄️ Archived Documentation (Read-Only Reference)

- **`internal/ARCHIVE/`** - Completed research and historical docs (see `internal/ARCHIVE/README.md`)
  - `core_ir_migration_2025_11/` - CoreExpr migration work (✅ COMPLETE 2025-11-23)
  - `macro_research/` - Macro system research (✅ COMPLETE 2025-11-08)
  - `numeric_research/` - Numeric tower research (✅ 94% COMPLETE 2025-11-04)
  - `completed_features/` - Implementation docs for finished features (do loop, macros)
  - `historical/` - Resolved bugs and historical issues

**Note:** Consult archive only for historical context or when implementing related future features. Always use active docs above as primary source of truth.

- **`spec/`** - Official R7RS specification (DO NOT add additional documentation here)

## Library System Architecture

Patina features a sophisticated **dual-loader library system** that balances performance (Rust-implemented) with flexibility (Scheme-implemented).

### Library Types

**1. Rust Libraries** - Performance-critical primitives
- **Location**: `crates/patina-runtime/src/stdlib/`
- **Examples**:
  - `scheme_base.rs` - Core (scheme base) primitives
  - `scheme_char.rs` - Character operations
  - `scheme_complex.rs` - Complex number support
  - `scheme_inexact.rs` - Inexact arithmetic
- **Registration**: Via `RustLibraryLoader` at evaluator initialization
- **Advantages**: Fast, compiled, type-safe

**2. Scheme Libraries** - Derived functions and macros
- **Location**: `lib/scheme/` directory
- **Examples**:
  - `base-extras.scm` - Derived functions (caar, cadr, not, zero?, etc.) and macros (let, cond, case, do, etc.)
  - `lazy-extras.scm` - Lazy evaluation support
- **Loading**: Automatically loaded as part of library initialization
- **Advantages**: Easy to modify, leverages Scheme's expressiveness

### Library Infrastructure

**LibraryRegistry** (`patina-runtime/src/library_registry.rs`)
- Manages loaded libraries with caching
- Tracks library search paths
- Detects circular dependencies
- Thread-safe with RefCell for interior mutability

**LibraryLoaderRegistry** (`patina-runtime/src/library_loader.rs`)
- Coordinates multiple loaders with priority order
- RustLibraryLoader (highest priority) → SchemeLibraryLoader
- Extensible: new loaders can be registered

**RustLibraryLoader** (`patina-runtime/src/library_loader.rs`)
- Simple hash map of library name → builder function
- Builder functions return `LibraryBuilder` with exports
- Zero overhead: just function calls

**SchemeLibraryLoader** (`patina-tree-walker/src/library_support.rs`)
- Parses `.sld` files (R7RS library definition format)
- **Stateless design**: Only parses, doesn't evaluate
- Eliminates circular dependencies (loader doesn't need evaluator)
- Searches configured library search paths

### Library Loading Flow

```
1. User code: (import (scheme char))
2. LibraryLoaderRegistry checks RustLibraryLoader first
3. If found: RustLibraryLoader returns pre-built library
4. If not found: SchemeLibraryLoader searches for scheme/char.sld
5. Library registered in LibraryRegistry for caching
6. Exports installed into current environment
```

### Current Library Status

**Implemented Libraries** (Rust):
- `(scheme base)` - Core primitives + base-extras.scm macros
- `(scheme char)` - Character operations
- `(scheme complex)` - Complex number support
- `(scheme inexact)` - Inexact arithmetic operations
- `(scheme lazy)` - Lazy evaluation (delay, force)
- `(scheme write)` - Output operations

**Partially Implemented**:
- `(scheme file)` - File I/O (basic support)
- `(scheme read)` - Input operations (basic support)

**Not Yet Implemented**:
- `(scheme case-lambda)` - Multi-arity procedures
- `(scheme cxr)` - Extended car/cdr
- `(scheme eval)` - Runtime evaluation
- `(scheme load)` - File loading
- `(scheme process-context)` - Process info
- `(scheme time)` - Time operations
- `(scheme r5rs)` - R5RS compatibility

## Architecture Deep Dive

### Evaluation Pipeline

The current evaluation pipeline uses **CoreExpr IR** with **macro-aware desugaring**:

```
Source Code
    ↓
[Lexer] → Tokens
    ↓
[Parser] → Value AST (homoiconic representation)
    ↓
[Backend.eval()]
    ↓
[Desugarer with env] → Checks for macros, expands on-demand
    ↓
[CoreExpr IR] → 9 core forms
    ↓
[CoreExpr Evaluator] → Result Value
```

**Key Innovation: Macro-Aware Desugaring**

The desugarer receives an `Environment` and checks for macro bindings:
1. When encountering a form, checks if it's a macro in the environment
2. If macro: Expands it and desugars the result
3. If special form: Desugars according to CoreExpr rules
4. If application: Desugars operator and operands

**Benefits**:
- No separate macro expansion phase needed
- Desugarer always works with the current macro environment
- Macros are expanded at the right time (after parsing, during desugaring)
- Clean separation: macro expander handles expansion, desugarer handles traversal

**Fallback Path**: Forms not yet in CoreExpr (let-syntax, case-lambda, expand) use the legacy Value evaluator.

### CoreExpr IR

**Location:** `crates/patina-ir/src/lib.rs`

CoreExpr is a minimal intermediate representation with 9 core forms:

```rust
pub enum CoreExpr {
    // Core forms (cannot be macros)
    Literal(Value),              // Self-evaluating values
    Var(String),                 // Variable reference
    Quote(Value),                // Literal data
    Lambda {                     // Function abstraction
        params: Vec<String>,
        variadic: Option<String>,
        body: Vec<CoreExpr>,
    },
    If {                         // Conditional (always ternary)
        test: Box<CoreExpr>,
        consequent: Box<CoreExpr>,
        alternate: Box<CoreExpr>,
    },
    Set {                        // Mutation
        var: String,
        value: Box<CoreExpr>,
    },
    Define {                     // Top-level binding
        var: String,
        value: Box<CoreExpr>,
    },
    Begin(Vec<CoreExpr>),        // Sequencing
    App {                        // Function application
        operator: Box<CoreExpr>,
        operands: Vec<CoreExpr>,
    },
}
```

**Design Philosophy**:
- Minimal set of forms that cannot be expressed as macros
- All derived forms (let, cond, and, or, case, do, etc.) are macros
- Simpler to evaluate than full Value AST (9 cases vs 26 variants)
- Foundation for future optimizations and alternative backends

**Not in CoreExpr**: The desugarer does NOT handle derived forms like `let`, `cond`, `and`, `or`, etc. These are **already macros** defined in `lib/scheme/base-extras.scm`. The macro expander transforms them before the desugarer sees them.

### Core Value Representation

**Location:** `crates/patina-runtime/src/value/mod.rs`

The `Value` enum represents all Scheme values (26 variants):

**Numeric Tower**:
- `Integer(i64)` - Small integers
- `BigInteger(BigInt)` - Arbitrary precision
- `Rational(Ratio<BigInt>)` - Exact fractions
- `Real(f64)` - Inexact reals
- `Complex(f64, f64)` - Complex numbers
- Automatic promotion on overflow
- See `PRD/phase1/NUMERIC_SUMMARY.md` for details

**Data Structures**:
- `Pair(Rc<RefCell<(Value, Value)>>)` - Cons cells
- `Null` - Empty list
- `Vector(Rc<RefCell<Vec<Value>>>)` - Vectors
- `Bytevector(Rc<RefCell<Vec<u8>>>)` - Byte vectors
- `String(Rc<RefCell<String>>>` - UTF-8 strings with O(n) indexing (R7RS compliant)

**Procedures**:
- `Procedure(Procedure)` where Procedure is:
  - `Primitive { name, arity, library }` - Built-in procedures
  - `Lambda { params, variadic, body, env }` - User-defined closures
  - `CaseLambda { clauses, env }` - Multi-arity dispatch
  - `Continuation(...)` - First-class continuations

**Hygiene Support**:
- `Symbol(Rc<str>)` - Regular symbols (special forms, built-ins)
- `Identifier { name, scopes }` - Hygienic identifier with scope set
  - Uses Racket-style scope sets for hygiene
  - Scopes track lexical context through macro expansions
  - Flip-scope algorithm toggles scopes to distinguish use-site vs introduced identifiers

**Special Values**:
- `Boolean(bool)` - #t and #f
- `Character(char)` - Characters
- `Macro { name, data }` - Macro transformers
- `Parameter(...)` - Dynamic parameters
- `Promise { ... }` - Lazy evaluation
- `Values(Vec<Value>)` - Multiple values
- `Library(...)` - Library objects
- `InputPort(...)`, `OutputPort(...)` - I/O ports
- `Unspecified` - Undefined return value
- `Eof` - End of file marker

**Memory Management**:
- Uses `Rc<T>` for immutable shared data
- Uses `Rc<RefCell<T>>` for mutable data (required for `set!`)
- Efficient sharing without unnecessary cloning

### Environment Model

**Location:** `crates/patina-runtime/src/environment.rs`

Environments implement lexical scoping with parent chains:

```rust
pub struct Environment {
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    parent: Option<Rc<Environment>>,
}
```

**Key operations**:
- `define(name, value)` - Create new binding in current environment
- `set(name, value)` - Mutate existing binding (searches parent chain)
- `get(name)` - Lookup variable (searches parent chain)
- `with_parent(parent)` - Create child environment

**Design**:
- `Rc<RefCell<HashMap>>` enables mutation required for `set!`
- Parent chain implements lexical scoping
- Global environment initialized with all primitives from loaded libraries

### Backend Abstraction

**Location:** `crates/patina-runtime/src/backend.rs`

The `Backend` trait enables multiple evaluation strategies:

```rust
pub trait Backend {
    type Error: std::error::Error + Send + Sync + 'static;

    fn eval(&self, expr: &Value, env: &Rc<Environment>)
        -> Result<Value, Self::Error>;

    fn global_env(&self) -> &Rc<Environment>;

    fn eval_global(&self, expr: &Value) -> Result<Value, Self::Error> {
        let global = self.global_env().clone();
        self.eval(expr, &global)
    }
}
```

**TreeWalker Backend** (`crates/patina-tree-walker/src/backend.rs`):

Implements Backend trait with hybrid evaluation:
1. **CoreExpr Path** (primary):
   - Creates macro-aware desugarer with environment
   - Desugars Value → CoreExpr (expands macros on-demand)
   - Evaluates CoreExpr with TCO support

2. **Value Path** (fallback):
   - For forms not yet in CoreExpr: let-syntax, letrec-syntax, case-lambda, expand
   - Uses legacy `Evaluator::eval_in_env()`

**Evaluation Result** (for TCO):
```rust
pub enum EvalResult {
    Value(Value),                              // Final result
    TailCall { expr: Value, env: Rc<Environment> },  // Continue trampolining
    TailCallPrimitive { proc: Value, args: Vec<Value> },  // Optimized primitive call
}
```

### Special Forms Registry

**Location:** `crates/patina-tree-walker/src/eval/special_forms/mod.rs`

Special forms are registered in a dynamic registry:

```rust
pub trait SpecialForm {
    fn name(&self) -> &'static str;
    fn help(&self) -> &'static str;
    fn eval(&self, evaluator: &Evaluator, args: &Value, env: &Rc<Environment>,
            in_tail_position: bool) -> Result<EvalResult, EvalError>;
    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError>;
}
```

**Registered Special Forms**:
- `quote`, `if`, `define`, `set!`, `lambda`, `begin`
- `define-syntax`, `let-syntax`, `letrec-syntax`
- `case-lambda` (R7RS scheme case-lambda)
- `expand` (debugging extension)

**Note**: Most "special forms" users think of (let, cond, case, do, and, or) are actually **macros** defined in base-extras.scm, not special forms.

### Primitives Organization

**Location:** `crates/patina-tree-walker/src/eval/primitives/`

Primitives are organized by category:

```
primitives/
├── mod.rs              # Registry and installation
├── arithmetic.rs       # +, -, *, /, quotient, remainder, modulo
├── comparison.rs       # =, <, >, <=, >=
├── lists.rs           # cons, car, cdr, list, append, length, reverse
├── vectors.rs         # vector, vector-ref, vector-set!, make-vector
├── strings.rs         # string-append, string-length, substring
├── chars.rs           # char=?, char<?, char-upcase, char-downcase
├── predicates.rs      # null?, pair?, number?, string?, symbol?
├── equivalence.rs     # eq?, eqv?, equal?
├── conversion.rs      # number->string, string->number, symbol->string
├── io.rs             # display, write, newline, read
└── control.rs        # apply, call-with-values, values
```

**Primitive Registry** (`PrimitiveRegistry`):
- Tracks which primitives belong to which library
- Enables selective export during library loading
- Pattern: `registry.register(library_name, primitive_name, function)`

### Macro System

**Location:** `crates/patina-macros/` (separate crate)

**Hygiene Algorithm**: Racket-style scope sets (based on "Binding as Sets of Scopes", Flatt 2016)
- `Identifier { name, scopes }` stores hygiene context via scope sets
- Each macro expansion creates a fresh scope
- **Flip-scope algorithm**:
  1. Before pattern matching: flip macro_scope on INPUT (adds scope to use-site identifiers)
  2. After template expansion: flip macro_scope on OUTPUT (removes from use-site, adds to introduced)
- Lookup uses subset matching: binding with `binding.scopes ⊆ reference.scopes` wins
- Most specific (largest scope set) binding shadows less specific ones
- No renaming needed - scopes provide discrimination

**Macro Expansion Integration**:
- Macros stored as `Value::Macro(Rc<CompiledMacro>)` (type-safe, no `dyn Any`)
- Desugarer checks environment for macro bindings
- When found, calls macro expander with flip-scope hygiene
- Expands recursively until a special form or application remains

**Core Macros** (in `lib/scheme/base-extras.scm`):
- Binding: `let`, `let*`, `letrec`, `letrec*`, `let-values`, `let*-values`
- Conditionals: `cond`, `case`, `when`, `unless`
- Boolean: `and`, `or`
- Iteration: `do` (with helper for optional step)
- Definition: `define-values`

## Reference Implementations

### Chibi-scheme (Primary R7RS Reference)

**Location:** `~/Project/reference/chibi-scheme`

**Key files to reference:**
- `tests/r7rs-tests.scm` - Comprehensive R7RS test suite (2516 lines covering entire spec)
- `lib/init-7.scm` - Core R7RS procedures implemented in Scheme on top of primitives
- `eval.c` - Core evaluator implementation in C
- `lib/scheme/base.sld` - R7RS base library definition

**Use chibi-scheme to:**
1. Understand how features should work, borrow interpreter implementation when needed
2. Test Patina's output against chibi's output for compatibility
3. Reference the comprehensive test suite for validation

**Documentation:** See `internal/reference_impls/CHIBI_REFERENCE.md` for detailed notes

### Other Reference Implementations

**Chez Scheme:** See `internal/reference_impls/CHEZ_REFERENCE.md` for notes on Chez implementation patterns

### R7RS Specification

**Location:** `spec/r7rs-small-spec/`

The official R7RS-small specification LaTeX source, useful for understanding precise language semantics and requirements.

## Development Commands

### Building and Running

```bash
# Build the workspace
cargo build --release

# Run the REPL
cargo run --release
# Binary location: ./target/release/patina

# Run a Scheme script
./target/release/patina script.scm

# Build specific crate
cargo build --package patina-frontend

# Run in debug mode (faster compile, slower execution)
cargo run
```

### Testing

```bash
# Run ALL tests (~423 tests across workspace)
cargo test

# Run only integration tests (patina-tests crate)
cargo test --package patina-tests

# Run specific test suite
cargo test --package patina-tests --test scheme_base
cargo test --package patina-tests --test interpreter_api

# Run specific crate's unit tests
cargo test --package patina-frontend
cargo test --package patina-runtime

# Run specific category
cargo test --package patina-tests primitives::
cargo test --package patina-tests numbers::

# Run tests verbosely
cargo test -- --nocapture

# Skip chibi comparison tests (if chibi not installed)
SKIP_CHIBI_TESTS=1 cargo test
```

### R7RS Compliance Testing

Run the comprehensive chibi-scheme r7rs test suite to check compliance:

```bash
# Run chibi-scheme's r7rs-tests.scm and generate compatibility report
./scripts/run_chibi_tests.sh

# View the report
cat scheme_tests/reports/compatibility.md

# View detailed results
cat scheme_tests/reports/results.txt
```

This runs the complete chibi-scheme R7RS test suite (~2500 lines) and generates:
- **Compatibility report** with pass/fail statistics
- **Detailed test output** showing what works and what doesn't

**Current status** (as of CoreExpr migration):
- ✅ 68/129 tests passing (52.7%)
- ❌ 4 tests failing (3.1%)
- ⚠️ 57 tests crashing (44.2%) - missing features
- Tests cover: primitives, macros, quasiquote, numeric tower, lists, strings, vectors, control flow

### Code Quality

```bash
# Check for errors without building
cargo check

# Check specific crate
cargo check --package patina-tree-walker

# Run the linter
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt

# Check formatting without making changes
cargo fmt -- --check
```

## Testing Strategy

**See `docs/TEST_ORGANIZATION.md` for comprehensive testing documentation.**

### Test Organization

**Unit Tests** - In component crates with `#[cfg(test)]`:
- `patina-frontend`: Lexer, parser, macro patterns
- `patina-runtime`: Environment operations, library loading
- `patina-macros`: Macro expansion, hygiene
- `patina-ir`: CoreExpr construction
- `patina-tree-walker`: Evaluator components

**Integration Tests** - In `crates/patina-tests/tests/`:
- `scheme_base.rs` - R7RS (scheme base) compliance (~347 tests)
- `interpreter_api.rs` - API tests (6 tests)
- `case_lambda.rs` - Case-lambda tests (21 tests, ignored pending import)
- `sld_file_loading.rs` - Library loading tests
- `library_core_expr_integration.rs` - CoreExpr integration with libraries
- `conversion.rs` - Type conversion tests
- `verify_bigint_promotion.rs` - Numeric tower promotion tests

**Test Utilities** (`crates/patina-tests/tests/common/mod.rs`):
```rust
assert_eval_to(expr, expected)           // Test expression evaluation
assert_program_eval_to(code, expected)   // Test multi-expression programs
assert_eval_error(expr)                  // Test error cases
```

### Running Tests

**Total: ~423 tests passing**

See Development Commands section above for test commands.

## Code Organization Principles

### When Adding Features

**New primitives**:
1. Add implementation in `crates/patina-tree-walker/src/eval/primitives/<category>.rs`
2. Register in `primitives/mod.rs::install_primitives()`
3. Add to appropriate library builder in `crates/patina-runtime/src/stdlib/<library>.rs`

**New special forms**:
1. Create implementation in `crates/patina-tree-walker/src/eval/special_forms/<name>.rs`
2. Implement `SpecialForm` trait
3. Register in `special_forms/mod.rs::build_registry()`

**New libraries**:
- **Rust library**: Create `crates/patina-runtime/src/stdlib/<name>.rs` and register in evaluator init
- **Scheme library**: Create `lib/scheme/<name>-extras.scm` and load in library builder

**Value types**:
- Extend `Value` enum in `crates/patina-runtime/src/value/mod.rs`
- Implement `std::fmt::Display` for new variants
- Add support in parser if needed (`crates/patina-frontend/src/parser/mod.rs`)

**CoreExpr forms**:
- Extend `CoreExpr` enum in `crates/patina-ir/src/lib.rs`
- Add desugaring logic in `crates/patina-frontend/src/desugarer/mod.rs`
- Add evaluation logic in `crates/patina-tree-walker/src/eval/core_eval.rs`

**Parser features**:
- Extend lexer in `crates/patina-frontend/src/lexer/mod.rs`
- Extend parser in `crates/patina-frontend/src/parser/mod.rs`

### Error Handling

Use appropriate error types for each layer:
- **Lexer**: `LexError` (in patina-frontend)
- **Parser**: `ParseError` (in patina-frontend)
- **Desugarer**: `DesugarError` (in patina-frontend)
- **Evaluator**: `EvalError` (in patina-tree-walker)
- **Runtime**: `RuntimeError` (in patina-runtime)
- **Interpreter API**: `InterpreterError<E>` (generic over backend error)
- **Pipeline**: `PipelineError` (in patina-pipeline)

### Module Organization

**Evaluator** (in `patina-tree-walker/src/eval/`):
- `mod.rs` - Core evaluation logic, registries, initialization
- `core_eval.rs` - CoreExpr evaluation (primary path)
- `error.rs` - Error type definitions
- `special_forms/` - All special form implementations
- `application.rs` - Procedure application logic
- `primitives/` - All primitive implementations organized by category
- `debug.rs` - Debug tracing support

**Keep module boundaries clean:**
- Use `pub(crate)` for internal APIs within a crate
- Use `pub(super)` for module-private functions
- Only expose what's needed at crate boundaries
- Avoid circular dependencies (use traits for abstraction)

### Memory Management

- Use `Rc<T>` for immutable shared data (symbols, strings, pairs)
- Use `Rc<RefCell<T>>` when interior mutability is needed (environments, vectors)
- Avoid unnecessary `clone()`; prefer sharing via `Rc`
- All `Value` operations work with `Rc` to minimize copying

## Implementation Status

See `docs/FEATURE_STATUS.md` for detailed test-by-test compliance matrix (canonical source).

**Currently Implemented:**
- Core special forms (quote, if, define, set!, lambda, begin)
- Full numeric tower (integers, bignums, rationals, reals, complex)
- List operations (cons, car, cdr, list, append, map, for-each, etc.)
- String operations (string-append, string-length, substring, etc.)
- Vector operations (vector, vector-ref, vector-set!, make-vector, etc.)
- Type predicates and equality (number?, string?, eq?, eqv?, equal?, etc.)
- Tail call optimization (via trampoline pattern)
- Hygienic macros (syntax-rules with scope sets hygiene)
- Multiple values (values, call-with-values)
- Library system (import, library loading, dual loaders)
- CoreExpr IR evaluation (primary path)

**Partially Implemented:**
- I/O operations (display, write, newline - basic support)
- Ports (input-port?, output-port? - basic support)

**High-Priority TODOs:**
- Full I/O suite (read, read-char, peek-char, file operations)
- Exception handling (guard, raise, error objects)
- Module system enhancements (export, rename, only, except)
- Continuations (call/cc, dynamic-wind)
- Case-lambda (implemented but needs import support)
- Full string/vector/bytevector operations

**Current Compliance:**
- **Internal tests**: ~423/423 passing (100%)
- **Chibi r7rs-tests.scm**: 68/129 passing (52.7%), 4 failing (3.1%), 57 crashing (44.2%)

## Future Phases

**Phase 2**: Bytecode VM backend
- Compile CoreExpr to bytecode for 5-10x speedup over tree-walking
- Essential foundation for performant type checking and macro expansion
- New crate: `patina-vm/` implementing the `Backend` trait
- Leverage existing CoreExpr IR as compilation source

**Phase 3**: syntax-case (procedural macros)
- Full syntax-case with `syntax->datum`, `datum->syntax`
- Builds on existing scope-set hygiene infrastructure
- Enables compile-time type annotation processing
- See `PRD/phase2/SYNTAX_CASE_DESIGN.md` for design

**Phase 4**: Gradual typing (Typed Racket-style)
- Type inference and checking at compile time
- Contracts at typed/untyped boundaries
- Requires VM (performance) and syntax-case (annotations)

**Phase 5**: Reactive streams (Project Reactor-style)
**Phase 6**: miniKanren logic programming

The workspace structure supports this evolution:
- Multiple backends: Add `patina-vm/`, `patina-jit/` alongside `patina-tree-walker/`
- All backends implement same Backend trait for `patina-interpreter`
- Tests automatically run against all backends
- See `PRD/MULTI_BACKEND_STRATEGY.md` for details

## Notebook Mode (Future Feature)

The project has extensive design docs for a terminal-based notebook interface with S-expression format (see `PRD/future/phase4/`). This is not yet implemented but is a key future direction. The design emphasizes:
- S-expression notebook format (`.scm.nb`) - notebooks as valid Scheme programs
- Three-tier system integration: native Scheme commands, table-based commands, shell fallback
- Cell-based editing with dependency tracking

## API Quick Reference

### High-Level API (Recommended)

```rust
use patina_interpreter::TreeWalkInterpreter;

// Create interpreter
let interp = TreeWalkInterpreter::new_tree_walker();

// Evaluate single expression
let result = interp.eval_str("(+ 1 2 3)").unwrap();

// Evaluate program (multiple expressions)
let result = interp.eval_program("(define x 10) (+ x 5)").unwrap();

// Resilient mode (continues on errors, for test suites)
let result = interp.eval_program_resilient(code);

// Access underlying evaluator
let eval = interp.evaluator();
let env = eval.global_env.clone();
```

### Pipeline API (Flexible)

```rust
use patina_pipeline::{StandardPipeline, Pipeline};

// Create pipeline
let pipeline = StandardPipeline::new();

// Evaluate with custom environment
let env = pipeline.evaluator().global_env.clone();
let result = pipeline.eval("(+ 1 2)", &env).unwrap();
```

### Backend API (Low-Level)

```rust
use patina_tree_walker::TreeWalker;
use patina_runtime::Backend;

// Create backend
let backend = TreeWalker::new();

// Evaluate expression
let result = backend.eval_global(&expr).unwrap();

// Custom environment
let custom_env = Rc::new(Environment::with_parent(backend.global_env().clone()));
let result = backend.eval(&expr, &custom_env).unwrap();
```

## Tips for Claude Code

1. **Check current state first**: This codebase evolves rapidly. Always read the relevant source files before making suggestions.

2. **Follow the architecture**: Respect the crate boundaries and don't mix concerns. Primitives go in tree-walker, types in runtime, parsing in frontend.

3. **Test thoroughly**: Run `cargo test --package patina-tests` after changes. Check that existing tests still pass.

4. **Consult reference implementations**: When unsure about semantics, check chibi-scheme or the R7RS spec.

5. **Library organization**: New primitives should be added to the appropriate library builder in `patina-runtime/src/stdlib/`, not just registered globally.

6. **CoreExpr vs Value**: The CoreExpr path is now primary. Forms that aren't in CoreExpr use the fallback Value evaluator.

7. **Macros vs special forms**: Most things users think of as "special forms" (let, cond, case) are actually macros. Only add special forms when absolutely necessary.

8. **Documentation**: Keep this file updated as the architecture evolves, but don't create new documentation files without asking the user first.
