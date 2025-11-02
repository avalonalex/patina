# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Patina is a Scheme R7RS-small interpreter written in Rust. This is an educational project with ambitious goals: implementing a full R7RS-compliant Scheme interpreter, then extending it with gradual typing, reactive concurrency, and logic programming. Currently in Phase 1 (basic R7RS compliance).

## Documentation Organization

**Important**: Use the following directories for different types of documentation:

- **`PRD/`** - Product Requirements Documents, project proposals, and high-level design documents
- **`internal/`** - Internal documentation, implementation notes, test results, and milestone records

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

### Test Organization
- `tests/comparison_test.rs`: Side-by-side comparison with chibi-scheme (R7RS reference)
- `tests/scheme_runner.rs`: Infrastructure for running .scm test files
- `tests/file_runner.rs`: Test file discovery and execution
- `tests/schemes/`: Directory containing Scheme test files organized by feature

### Test Infrastructure Notes
- Tests can optionally compare output with chibi-scheme if installed
- Use `SKIP_CHIBI_TESTS=1` environment variable to skip chibi comparisons
- The interpreter maintains state between `eval_str` calls in the same `Interpreter` instance
- `Value::Unspecified` is returned by definitions and should not be displayed

## Implementation Status

### Currently Working
- Self-evaluating literals (numbers, strings, booleans, characters)
- Quote, if, define, set!, lambda, begin special forms
- Arithmetic primitives: +, -, *, /
- Comparison operators: =, <, >, <=, >=
- List operations: cons, car, cdr, null?, pair?
- Basic predicates: eq?, boolean?, number?, integer?, string?, symbol?

### High-Priority TODOs for R7RS Compliance
1. **Closures**: Lambda currently doesn't capture environment properly
2. **Tail call optimization**: Required for R7RS, critical for recursive algorithms
3. **More special forms**: let, let*, letrec, cond, case, and, or
4. **Complete numeric tower**: Currently missing proper rational and complex arithmetic
5. **Hygienic macros**: syntax-rules, let-syntax, letrec-syntax
6. **String/character operations**: Full Unicode support per R7RS
7. **I/O and ports**: File operations, read, write, display
8. **Exception handling**: guard, raise, with-exception-handler
9. **Continuations**: call-with-current-continuation (call/cc)

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
