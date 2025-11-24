# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Patina is a Scheme R7RS-small interpreter written in Rust. This is an educational project with ambitious goals: implementing a full R7RS-compliant Scheme interpreter, then extending it with gradual typing, reactive concurrency, and logic programming. Currently in Phase 1 (basic R7RS compliance).

**Architecture:** Modular workspace with 7 crates supporting multiple backends (tree-walker implemented, VM and JIT planned).

## Workspace Structure

Patina uses a Rust workspace with separate crates for different concerns:

```
patina/ (workspace root)
├── Cargo.toml              # Workspace configuration
├── lib/                    # Scheme standard library
│   └── bootstrap.scm       # Core macros (let, cond, case, etc.)
│
└── crates/
    ├── patina-runtime/     # Core types (Value, Environment)
    ├── patina-frontend/    # Lexer, Parser, Macro Expander
    ├── patina-tree-walker/ # Tree-walking interpreter backend
    ├── patina-interpreter/ # High-level Interpreter API
    ├── patina-ir/          # Core IR for nanopass (future)
    ├── patina-repl/        # REPL with executable binary
    └── patina-tests/       # All integration & compliance tests
```

### Crate Responsibilities

**patina-runtime** (`crates/patina-runtime/src/`)
- Core `Value` enum: all Scheme data types
- `Environment`: lexical scoping with parent chains
- Shared types used by all other crates

**patina-frontend** (`crates/patina-frontend/src/`)
- `lexer/`: Tokenizes Scheme source
- `parser/`: Builds AST (as Value) from tokens
- `macro_expander/`: Hygienic macro expansion (syntax-rules) with marks-and-ribs hygiene
- `desugarer/`: Converts macro-expanded AST to CoreExpr IR
- Unit tests: 61 tests for lexer/parser/macros, 15 tests for desugarer

**patina-tree-walker** (`crates/patina-tree-walker/src/eval/`)
- `mod.rs`: Core evaluator with TCO support
- `special_forms.rs`: Special form handlers
- `application.rs`: Procedure application
- `primitives/`: All built-in procedures
- Implements the tree-walking interpreter backend

**patina-interpreter** (`crates/patina-interpreter/src/`)
- High-level `Interpreter` API
- Combines frontend (parsing) + backend (evaluation)
- Simple interface: `eval_str()`, `eval_program()`
- Backend abstraction (currently uses tree-walker)

**patina-repl** (`crates/patina-repl/src/`)
- Rich terminal REPL with rustyline
- Syntax highlighting, history, multi-line input
- Binary executable: `target/release/patina`

**patina-tests** (`crates/patina-tests/tests/`)
- R7RS compliance tests (~285 tests)
- Integration tests (chibi comparison)
- Interpreter API tests
- All tests use `patina-interpreter` API

**patina-ir** (`crates/patina-ir/src/`)
- Core IR definition for future nanopass architecture
- Not yet integrated into main pipeline

### Dependency Flow

```
patina-repl → patina-interpreter → patina-tree-walker → patina-runtime
                                 ↗  patina-frontend    ↗
patina-tests → patina-interpreter
```

## Documentation Organization

**Important**: Use the following directories for different types of documentation:

- **IMPORTANT** - Do not create markdown files unless the user explicitly states to do so. You may offer to create markdown files, but only do so with explicit user approval. Integrate any necessary notes as comments within the relevant code files, and keep comments succinct and on point.

### 📁 Active Documentation (Frequently Updated)

- **`PRD/phase1/`** - Strategic planning and high-level design (current phase only)
  - **`IMPLEMENTATION_STATUS.md`** - ⭐ **CANONICAL** overall roadmap, priorities, and remaining work
  - **`NUMERIC_SUMMARY.md`** - ⭐ **CANONICAL** guide for numeric tower implementation
  - `STRING_OPTIMIZATION.md` - Future optimization plans (deferred)
  - `HELP_SYSTEM.md` - Future help system design (deferred)

- **`docs/`** - User-facing documentation and developer guides
  - **`FEATURE_STATUS.md`** - ⭐ **CANONICAL** detailed test-by-test R7RS compliance matrix
  - **`TEST_ORGANIZATION.md`** - ⭐ Test structure and running tests
  - **`HYGIENE_SYSTEM_DESIGN.md`** - Guide for swapping hygiene implementations
  - **`HYGIENE_COMPLIANCE_ANALYSIS.md`** - R7RS macro compliance measurements
  - **`MACRO_DEBUGGING.md`** - Comprehensive macro debugging guide
  - `API.md` - Public API reference
  - `GETTING_STARTED.md` - User guide for getting started
  - `README.md` - Project overview
  - `TESTING.md` - Test infrastructure and usage
  - `DEVELOPMENT.md` - Developer guide

- **`internal/`** - Implementation notes and progress tracking
  - **`MILESTONES.md`** - Historical achievements and progress milestones (keep updated)
  - `NESTED_ELLIPSIS_LIMITATION.md` - Future enhancement documentation
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

# Build specific crate
cargo build --package patina-frontend

# Run in debug mode (faster compile, slower execution)
cargo run
```

### Testing

```bash
# Run ALL tests (435 tests across workspace)
cargo test

# Run only integration tests
cargo test --package patina-tests

# Run specific test suite
cargo test --package patina-tests --test compliance
cargo test --package patina-tests --test integration

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

**Current status (as of quasiquote implementation):**
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
```

## Architecture Deep Dive

### Core Value Representation

**Location:** `crates/patina-runtime/src/value/mod.rs`

The `Value` enum represents all Scheme values:
- **Numeric tower**: `Integer`, `BigInteger`, `Rational`, `Real`, `Complex`
  - Uses `num-bigint`, `num-rational` crates
  - Automatic promotion on overflow
  - See `PRD/phase1/NUMERIC_SUMMARY.md` for details
- **Strings**: `String(Rc<RefCell<String>>)` - UTF-8 with O(n) character indexing (R7RS compliant)
  - See `PRD/phase1/STRING_OPTIMIZATION.md` for future optimization plans
- **Data structures**: `Pair`, `Null`, `Vector`, `Bytevector`
- **Procedures**: `Primitive` (built-in) or `Lambda` (user-defined)
- **Macros**: `Macro { name, data }` - Hygienic macro transformers
- Uses `Rc<T>` for efficient sharing of immutable data
- Uses `Rc<RefCell<T>>` for mutable data (environments)

### Environment Model

**Location:** `crates/patina-runtime/src/environment.rs`

- Lexical scoping with parent environment chains
- Uses `Rc<RefCell<HashMap>>` for mutable bindings (required for `set!`)
- Global environment initialized with primitive procedures

### Frontend Pipeline

**Location:** `crates/patina-frontend/src/`

1. **Lexer** (`lexer/mod.rs`):
   - Tokenizes Scheme source into tokens
   - Handles R7RS token types including vectors, bytevectors, characters
   - Rejects reserved characters `[`, `]`, `{`, `}`

2. **Parser** (`parser/mod.rs`):
   - Builds AST (as `Value` enum) from tokens
   - Handles quote shorthands (`'`, `` ` ``, `,`, `,@`)
   - Parses numeric literals with full numeric tower support

3. **Macro Expander** (`macro_expander/`):
   - Implements R7RS `syntax-rules` hygienic macros
   - Pattern matching and template expansion
   - Marks-and-ribs hygiene algorithm (Chez Scheme-style)
   - Core macros defined in `lib/bootstrap.scm`

4. **Desugarer** (`desugarer/mod.rs`):
   - Converts macro-expanded AST to CoreExpr IR
   - Macro-aware: expands macros on-demand during desugaring
   - Handles all special forms and converts to core primitives
   - Produces simplified IR for evaluation

### Tree-Walking Interpreter

**Location:** `crates/patina-tree-walker/src/eval/`

**CoreExpr Evaluator** (`core_eval.rs`) - PRIMARY PATH:
- Evaluates CoreExpr IR from desugarer
- Simplified evaluation logic with fewer cases
- Full TCO support via trampoline pattern
- No macro expansion needed (already desugared)

**Legacy Evaluator** (`mod.rs`):
- Direct AST evaluation (bypasses desugarer)
- `eval_step_impl()` - Main evaluation loop
- Handles special forms, macros, and procedure calls
- Loads `lib/bootstrap.scm` on initialization
- Still used for REPL and backwards compatibility

**Special Forms** (`special_forms.rs`):
- `quote`, `if`, `define`, `set!`, `lambda`, `begin`
- `cond`, `case`, `do` (with TCO)
- Binding forms: `let`, `let*`, `letrec`, `letrec*`
- Multiple values: `let-values`, `let*-values`
- Boolean: `and`, `or` (short-circuit)
- Higher-order: `apply`
- Macros: `define-syntax`, `syntax-rules`

**Primitives** (`primitives/mod.rs` and subdirectories):
- Organized by category (arithmetic, lists, strings, vectors, etc.)
- ~100+ R7RS procedures implemented
- See `primitives/` subdirectories for organization
- TCO support for `call-with-values` via special primitive type

**Error Handling** (`error.rs`):
- `EvalError` enum for all evaluation errors
- Converts `FrontendError` from macro expansion

### High-Level API

**Location:** `crates/patina-interpreter/src/lib.rs`

The `Interpreter` struct provides the main programmatic interface:

```rust
use patina_interpreter::Interpreter;

let interp = Interpreter::new();
let result = interp.eval_str("(+ 1 2 3)").unwrap();  // Single expression
let result = interp.eval_program("(define x 10) x").unwrap();  // Multiple expressions
```

Methods:
- `new()` - Create interpreter with fresh environment
- `eval_str(input: &str)` - Parse and evaluate a single expression
- `eval_program(input: &str)` - Evaluate multiple expressions, return last result
- `evaluator()` - Access underlying evaluator (for advanced use)

## Testing Strategy

**See `docs/TEST_ORGANIZATION.md` for comprehensive testing documentation.**

### Test Organization

**Unit Tests** - In component crates with `#[cfg(test)]`:
- `patina-frontend`: 61 tests (lexer, parser, macro patterns)
- `patina-runtime`: 2 tests (environment operations)

**Integration Tests** - In `crates/patina-tests/tests/`:
- `compliance/` - R7RS spec tests (~285 tests)
- `integration/` - Chibi comparison (~13 tests)
- `interpreter_api.rs` - API tests (6 tests)
- `tail_recursion.rs` - TCO tests (36 tests)
- `numeric_operations.rs` - Numeric tower (25 tests)

**Test Utilities** (`crates/patina-tests/tests/common/mod.rs`):
```rust
assert_eval_to(expr, expected)           // Test expression evaluation
assert_program_eval_to(code, expected)   // Test multi-expression programs
assert_eval_error(expr)                  // Test error cases
```

### Running Tests

See Development Commands section above for test commands.

**Total: 435 tests (392 passing, 43 ignored)**

## Code Organization Principles

### When Adding Features

**New primitives**:
- Add implementation in `crates/patina-tree-walker/src/eval/primitives/<category>.rs`
- Register in `primitives/mod.rs::install_primitives()`

**New special forms**:
- Add dispatch case in `crates/patina-tree-walker/src/eval/mod.rs::eval_step_impl()`
- Implement handler in `crates/patina-tree-walker/src/eval/special_forms.rs`

**Value types**:
- Extend `Value` enum in `crates/patina-runtime/src/value/mod.rs`
- Implement `std::fmt::Display` for new variants

**Parser features**:
- Extend lexer in `crates/patina-frontend/src/lexer/mod.rs`
- Extend parser in `crates/patina-frontend/src/parser/mod.rs`

### Error Handling

- Use `EvalError` (in `patina-tree-walker/src/eval/error.rs`) for evaluation errors
- Use `ParseError` (in `patina-frontend`) for parsing issues
- Use `LexError` (in `patina-frontend`) for lexical issues
- Use `FrontendError` (in `patina-frontend`) for general frontend errors
- Wrap in `InterpreterError` at the public API level (`patina-interpreter`)

### Module Organization

**Evaluator** (in `patina-tree-walker/src/eval/`):
- `mod.rs` - Core evaluation logic (keep minimal)
- `error.rs` - Error type definitions
- `special_forms.rs` - All special form implementations
- `application.rs` - Procedure application logic
- `primitives/` - All primitive implementations organized by category
- `debug.rs` - Debug tracing support

**Keep module boundaries clean:**
- Use `pub(crate)` for internal APIs within a crate
- Use `pub(super)` for module-private functions
- Only expose what's needed at crate boundaries

### Memory Management

- Use `Rc<T>` for immutable shared data (symbols, strings, pairs)
- Use `Rc<RefCell<T>>` when interior mutability is needed (environments)
- Avoid unnecessary `clone()`; prefer sharing via `Rc`
- All `Value` operations work with `Rc` to minimize copying

## Implementation Status

See `docs/FEATURE_STATUS.md` for detailed test-by-test compliance matrix (canonical source).

**Currently Implemented:**
- Core special forms (quote, if, define, set!, lambda, begin, cond, case, do)
- Binding forms (let, let*, letrec, letrec*, let-values, let*-values)
- Boolean operators (and, or)
- Full numeric tower (integers, bignums, rationals, reals, complex)
- List operations (cons, car, cdr, list, append, map, for-each, etc.)
- String operations (basic)
- Vector operations (basic)
- Type predicates and equality
- Tail call optimization
- Hygienic macros (syntax-rules)
- Multiple values (values, call-with-values)

**High-Priority TODOs:**
- I/O and ports (display, write, file operations)
- Exception handling (guard, raise)
- Module system
- Full string/vector suites
- Continuations (call/cc)

## Future Phases

**Phase 2**: Gradual typing (Typed Racket-style)
**Phase 3**: Reactive streams (Project Reactor-style)
**Phase 4**: miniKanren logic programming

The workspace structure supports this evolution:
- Multiple backends: Add `patina-vm/`, `patina-jit/` alongside `patina-tree-walker/`
- All backends implement same interface for `patina-interpreter`
- Tests automatically run against all backends
- See `PRD/MULTI_BACKEND_STRATEGY.md` for details

## Notebook Mode (Future Feature)

The project has extensive design docs for a terminal-based notebook interface with S-expression format (see `PRD/future/phase4/`). This is not yet implemented but is a key future direction. The design emphasizes:
- S-expression notebook format (`.scm.nb`) - notebooks as valid Scheme programs
- Three-tier system integration: native Scheme commands, table-based commands, shell fallback
- Cell-based editing with dependency tracking
