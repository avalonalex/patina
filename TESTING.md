# Patina Testing Infrastructure

This document describes the comprehensive integration test suite for Patina, a Scheme R7RS interpreter.

## Overview

The testing infrastructure provides:
1. **Programmatic API** - Library interface for testing
2. **Integration tests** - Rust-based test harness
3. **R7RS compliance testing** - Comparison with chibi-scheme reference implementation
4. **Snapshot testing** - Expected outputs for regression testing
5. **Platform independence** - Tests work with or without chibi-scheme

## Reproducibility

See [SETUP.md](SETUP.md) for detailed setup instructions including:
- Docker-based reproducible environments
- CI/CD configuration
- Platform-specific installation guides
- Fallback testing without chibi-scheme

## Architecture

### Library Structure

Patina is structured as both a library and binary:
- **Library** (`src/lib.rs`) - Exposes `Interpreter` API for programmatic use
- **Binary** (`src/main.rs`) - REPL application

This allows tests to directly invoke the interpreter without spawning processes.

### Test Organization

```
tests/
├── scheme_runner.rs          # Integration tests (Patina-only)
├── file_runner.rs            # File-based tests (chibi-scheme comparison)
├── basic_tests.scm           # Legacy test file
└── schemes/                  # R7RS-compliant test suite
    ├── arithmetic/
    │   ├── basic.scm
    │   ├── basic.expected    # chibi-scheme reference output
    │   └── comparisons.scm
    ├── lists/
    │   └── basic.scm
    ├── control/
    │   ├── if.scm
    │   └── begin.scm
    └── harness/
        └── test_lib.scm      # Shared test utilities
```

## Running Tests

### All Tests (requires chibi-scheme)
```bash
cargo test
```

### Core Tests Only (no chibi-scheme required)
```bash
cargo test --lib                     # Unit tests
cargo test --test scheme_runner      # Patina integration tests
```

### Compliance Tests (requires chibi-scheme)
```bash
cargo test --test file_runner
```

### Skip chibi-scheme Tests
```bash
SKIP_CHIBI_TESTS=1 cargo test
```

### Generate Snapshots
```bash
cargo test --test file_runner -- --ignored
```

### Using Docker (fully reproducible)
```bash
docker-compose up patina-test
```

## Test Categories

### 1. Unit Tests (8 tests)
Located in individual modules:
- `src/lexer/mod.rs` - Lexer tests
- `src/parser/mod.rs` - Parser tests
- `src/env/mod.rs` - Environment tests
- `src/lib.rs` - Interpreter API tests

### 2. Integration Tests (7 tests)
Located in `tests/scheme_runner.rs`:
- Basic arithmetic operations
- Numeric comparisons
- List operations (cons, car, cdr)
- Conditionals (if)
- Definitions (define)
- Control flow (begin)

### 3. File-Based Tests (3 tests)
Located in `tests/file_runner.rs`:
- chibi-scheme availability check
- Arithmetic tests comparison
- List tests comparison

### 4. Doc Tests (2 tests)
Embedded in `src/lib.rs` documentation

## Using the Library API

```rust
use patina::Interpreter;

// Create interpreter
let interp = Interpreter::new();

// Evaluate single expression
let result = interp.eval_str("(+ 1 2 3)").unwrap();

// Evaluate program (multiple expressions)
let result = interp.eval_program(r#"
    (define x 10)
    (define y 20)
    (+ x y)
"#).unwrap();
```

## R7RS Compliance Testing

Test files in `tests/schemes/` are valid R7RS code that run with both:
- **chibi-scheme** (reference implementation)
- **Patina** (when features are implemented)

### Test File Format

```scheme
(import (scheme base)
        (scheme write))

(display (+ 1 2 3)) (newline)  ; Expected: 6
(display (null? '())) (newline) ; Expected: #t
```

### Generating Reference Outputs

```bash
# Generate .expected files from chibi-scheme
cargo test --test file_runner generate_snapshots -- --ignored

# Compare Patina output with snapshots (TODO)
```

## Current Test Coverage

### Implemented Features ✓
- [x] Basic arithmetic (+, -, *, /)
- [x] Numeric comparisons (=, <)
- [x] Lists (cons, car, cdr, list)
- [x] Predicates (null?, pair?)
- [x] Conditionals (if)
- [x] Definitions (define)
- [x] Sequencing (begin)
- [x] Quoting (')

### Not Yet Implemented ⏳
- [ ] Lambda expressions
- [ ] Let bindings
- [ ] Cond expressions
- [ ] More numeric predicates (>, <=, >=)
- [ ] More list functions (length, append, etc.)
- [ ] I/O procedures (display, write, newline)
- [ ] String operations
- [ ] Vector operations

## Demo Files

### `examples/demo.scm`
REPL-oriented demo file (comments show expected output)
- Designed for interactive use
- Cannot be run as standalone script

### `examples/demo_r7rs.scm`
Complete R7RS program
- Runs with both Patina and chibi-scheme
- Uses `(import ...)` and I/O procedures
- Shows all currently implemented features

```bash
# Run with chibi-scheme
chibi-scheme examples/demo_r7rs.scm

# Run with Patina (TODO: requires import/display/write)
```

## Future Enhancements

1. **Snapshot Testing**
   - Automatically compare Patina output with chibi-scheme
   - Regression detection

2. **R7RS Coverage Tracking**
   - Map tests to R7RS specification sections
   - Generate compliance reports

3. **Property-Based Testing**
   - Use QuickCheck-style testing for primitives
   - Generate random valid Scheme programs

4. **Performance Benchmarking**
   - Compare with chibi-scheme performance
   - Track performance regressions

5. **Error Message Quality**
   - Test error conditions
   - Compare error messages with reference implementation

## Contributing

When adding new features:
1. Add unit tests in the relevant module
2. Add integration tests in `tests/scheme_runner.rs`
3. Create R7RS test files in `tests/schemes/`
4. Update this document

## Test Statistics

```
Total Tests: 20
- Unit Tests: 8
- Integration Tests: 7
- File Tests: 3
- Doc Tests: 2

Pass Rate: 100% (20/20)
```

Last updated: 2025-10-27
