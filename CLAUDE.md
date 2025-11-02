# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Patina is a Scheme R7RS-small interpreter written in Rust. This is an educational project with ambitious goals: implementing a full R7RS-compliant Scheme interpreter, then extending it with gradual typing, reactive concurrency, and logic programming. Currently in Phase 1 (basic R7RS compliance).

## Documentation Organization

**Important**: Use the following directories for different types of documentation:

- **IMPORTANT** - Do not create markdown files unless the user explicitly states to do so. You may offer to create markdown files, but only do so with explicit user approval. Integrate any necessary notes as comments within the relevant code files, and keep comments succinct and on point.
- **`PRD/`** - Product Requirements Documents, project proposals, and high-level design documents. We should be aggressive in terms of purge outdated PRDs as they have high cognative overhead.
- **`internal/`** - Internal documentation, implementation notes, test results, and milestone records
- **`docs/`** - Oriented towards external documentations, how to use high level features and repl and tui tools.

## Reference Implementation

**Chibi-scheme** (R7RS reference implementation) is available at:
`~/Project/reference/chibi-scheme`

Key files to reference:

- `tests/r7rs-tests.scm` - Comprehensive R7RS test suite (2516 lines covering entire spec)
- `lib/init-7.scm` - Core R7RS procedures implemented in Scheme on top of primitives
- `eval.c` - Core evaluator implementation in C
- `lib/scheme/base.sld` - R7RS base library definition

Use chibi-scheme to:

1. Understand how features should work, borrow interpreter implemention when needed.
2. Test Patina's output against chibi's output for compatibility
3. Reference the comprehensive test suite for validation

**R7RS Specification**: The official R7RS-small specification LaTeX source is available at:
`spec/r7rs-small-spec/`

This includes the complete specification source code, useful for understanding precise language semantics and requirements.

## Development Commands

### Building and Running

```bash
# Build the project
cargo build --release

# Run the REPL
cargo run --release

# Run in debug mode (faster compile, slower execution)
cargo run
```

### Testing

```bash
# Run all tests
cargo test

# Run a specific test
cargo test test_compare_arithmetic

# Run tests without chibi-scheme comparison (if chibi not installed)
SKIP_CHIBI_TESTS=1 cargo test

# Run only integration tests
cargo test --test comparison_test
cargo test --test scheme_runner

# Run tests verbosely to see print statements
cargo test -- --nocapture
```

### Code Quality
```bash
# Check for errors without building
cargo check

# Run the linter
cargo clippy

# Format code
cargo fmt
```

## Architecture Overview

### Core Pipeline: Lexer � Parser � Evaluator

1. **Lexer** (`src/lexer/mod.rs`): Tokenizes Scheme source into tokens
2. **Parser** (`src/parser/mod.rs`): Builds AST (Value enum) from tokens
3. **Evaluator** (`src/eval/mod.rs`): Tree-walking interpreter that evaluates AST

### Value Representation (`src/value/mod.rs`)

The core `Value` enum represents all Scheme values:
- Numeric tower: `Integer`, `BigInteger`, `Rational`, `Real`, `Complex` (using num-bigint/num-rational)
- Data structures: `Pair`, `Null`, `Vector`, `Bytevector`
- Procedures: `Primitive` (built-in) or `Lambda` (user-defined)
- Uses `Rc<T>` for efficient sharing of immutable data (strings, symbols, pairs)

### Environment Model (`src/env/mod.rs`)

- Lexical scoping with parent environment chains
- Uses `Rc<RefCell<HashMap>>` for mutable bindings (required for `set!`)
- Global environment initialized with primitive procedures

### REPL (`src/repl/mod.rs`)

Rich terminal interface built with rustyline:
- `highlighter.rs`: Real-time syntax highlighting with nu-ansi-term
- `validator.rs`: Multi-line input validation (checks parenthesis balancing)
- `completer.rs`: Tab completion (TODO)
- History saved to `~/.patina_history`

### Public API (`src/lib.rs`)

The `Interpreter` struct provides the main programmatic interface:
- `eval_str(input: &str)`: Parse and evaluate a single expression
- `eval_program(input: &str)`: Evaluate multiple expressions, return last result

## Testing Strategy

### Test Organization (Reorganized 2025-11-02)

Tests are now organized to mirror the R7RS specification structure:

**Compliance Tests** (`tests/compliance.rs` + `tests/compliance/`):
- `primitives.rs` - Section 4.1: Primitive expressions (quote, lambda, if, define, set!)
- `derived.rs` - Section 4.2: Derived expressions (cond, case, let, and, or, etc.)
- `numbers.rs` - Section 6.2: Numeric operations and predicates
- `lists.rs` - Section 6.4: Pairs and lists
- `predicates.rs` - Type predicates and equality

**Integration Tests** (`tests/integration.rs` + `tests/integration/`):
- `comparison_test.rs` - Side-by-side comparison with chibi-scheme
- `scheme_runner.rs` - Infrastructure for running .scm test files
- `file_runner.rs` - Test file discovery and execution

**Test Fixtures** (`tests/fixtures/`):
- `r7rs/` - R7RS compliance test cases
- `examples/` - Example Scheme programs organized by feature

**Common Helpers** (`tests/common/mod.rs`):
- `assert_eval_to(expr, expected)` - Test expression evaluation
- `assert_program_eval_to(code, expected)` - Test multi-expression programs
- `assert_eval_error(expr)` - Test error cases

### Running Tests

```bash
# All tests
cargo test

# Compliance tests only (R7RS spec coverage)
cargo test --test compliance

# Integration tests only
cargo test --test integration

# Specific category
cargo test --test compliance primitives
cargo test --test compliance numbers

# Generate progress report
./scripts/test_report.sh
```

### Test Status Tracking

See `tests/FEATURE_MATRIX.md` for detailed feature-by-feature status tracking.

Current status: **44/93 tests passing (47%)**
- Primitives: 18/20 (90%) ✅
- Numbers: 11/23 (47%) ⚠️
- Lists: 6/19 (31%) ⚠️
- Predicates: 7/12 (58%) ✅
- Derived: 2/19 (10%) ❌

### Test Infrastructure Notes
- Tests can optionally compare output with chibi-scheme if installed
- Use `SKIP_CHIBI_TESTS=1` environment variable to skip chibi comparisons
- The interpreter maintains state between `eval_str` calls in the same `Interpreter` instance
- `Value::Unspecified` is returned by definitions and should not be displayed
- Run `./scripts/test_report.sh` for a visual progress report

## Implementation Status

### Currently Working (Updated 2025-11-02)

**Special Forms:**
- quote - Full support including shorthand `'expr`
- if - With and without else clause
- define - Variable definitions (function shorthand not yet implemented)
- set! - Mutation of existing bindings
- lambda - **Full support with closures!** (Fixed/variadic/mixed arity)
- begin - Sequential evaluation
- cond - Multi-branch conditionals with else

**Arithmetic:**
- +, -, *, / - Full support (integers only currently)
- =, <, >, <=, >= - Comparison operators (integers only)

**List Operations:**
- cons, car, cdr - Pair operations
- null?, pair? - Type predicates
- list - List constructor

**Type Predicates:**
- eq?, eqv?, equal? - Equality predicates
- boolean?, number?, integer?, string?, symbol? - Type checks

**Value Types:**
- All R7RS value types parsed: booleans, numbers (full tower), characters, strings, symbols, pairs, vectors, bytevectors

### High-Priority TODOs for R7RS Compliance

**Phase 1 (Next 2-3 weeks):**
1. **let, let*, letrec** - Binding constructs (blocks 23% of tests)
2. **and, or** - Short-circuit boolean operators
3. **apply** - Critical for higher-order functions
4. **map, for-each** - Common list operations
5. **Tail call optimization** - Required by R7RS

**Phase 2 (Weeks 4-5):**
6. **Numeric operations** - abs, quotient, remainder, modulo, predicates
7. **List operations** - length, append, reverse, list-ref
8. **case** - Pattern matching conditional

**Phase 3 (Weeks 6+):**
9. **String operations** - Full string manipulation suite
10. **Vector operations** - Vector manipulation suite
11. **I/O and ports** - display, write, file operations
12. **Exception handling** - guard, raise, with-exception-handler
13. **Hygienic macros** - syntax-rules, define-syntax
14. **Continuations** - call/cc (complex, lower priority)

See `tests/FEATURE_MATRIX.md` for complete feature-by-feature tracking.

## Code Organization Principles

### When Adding Features
- New primitives go in `eval/mod.rs` in the `install_primitives` function
- New special forms: check symbol name in `eval_list`, add handler method
- Value types: extend the `Value` enum in `value/mod.rs`
- Display formatting: implement `std::fmt::Display` for new Value variants

### Error Handling
- Use `EvalError` enum (defined in `eval/mod.rs`) for evaluation errors
- Use `ParseError` enum for parsing issues
- Use `LexError` enum for lexical issues
- Wrap in `InterpreterError` at the public API level

### Memory Management
- Use `Rc<T>` for immutable shared data (symbols, strings, pairs)
- Use `Rc<RefCell<T>>` when interior mutability is needed (environments)
- Avoid `clone()` when possible; prefer sharing via `Rc`

## Future Phases

Phase 2 will add gradual typing (Typed Racket-style). Phase 3 adds reactive streams (Project Reactor-style). Phase 4 adds miniKanren logic programming. Keep the core interpreter clean and modular to support these extensions.

## Notebook Mode (Future Feature)

The project has extensive design docs for a terminal-based notebook interface with S-expression format (see `docs/`). This is not yet implemented but is a key future direction. The design emphasizes:
- S-expression notebook format (`.scm.nb`) - notebooks as valid Scheme programs
- Three-tier system integration: native Scheme commands, table-based commands, shell fallback
- Cell-based editing with dependency tracking
