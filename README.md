# Patina - A Scheme R7RS Interpreter in Rust

**A modular, multi-backend Scheme interpreter designed for experimentation**

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)]()
[![License](https://img.shields.io/badge/license-MIT-blue)]()

---

## Philosophy

Patina is an educational and experimental Scheme interpreter with ambitious goals:

### Full R7RS-small Compliance

Our primary focus is complete conformance to the R7RS-small specification. We validate against the comprehensive [Chibi Scheme](https://github.com/ashinn/chibi-scheme) test suite maintained by Alex Shinn, chairman of the R7RS Small Language committee.

### Modular Architecture

Patina separates concerns into independent crates with two key abstractions:

**Pipeline** - Orchestrates the entire evaluation flow:
```
Source Code → Parse → Expand → Desugar → Evaluate → Result
```

Different pipelines can compose phases differently (e.g., adding optimization passes, bytecode compilation).

**Backend** - Handles evaluation of parsed expressions:
```rust
trait Backend {
    fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, Error>;
}
```

This separation allows swapping evaluation strategies (tree-walker, VM, JIT) without changing the pipeline, or adding pipeline stages without touching the backend.

### Workspace Structure

```
patina-frontend     →  Lexer, Parser, Desugarer
patina-ir           →  CoreExpr intermediate representation
patina-macros       →  Hygienic macro expansion (scope sets)
patina-runtime      →  Core types, Backend trait, Library system
patina-pipeline     →  Pipeline orchestration
patina-tree-walker  →  Tree-walking backend (current)
patina-interpreter  →  High-level API
patina-repl         →  Terminal REPL
patina-tests        →  Integration tests
```

### Experimental Goals

The architecture is designed to support future exploration:

- Alternative backends (bytecode VM, JIT compilation)
- Nanopass-style optimization passes
- Language extensions (gradual typing, reactive concurrency, logic programming)

### Clean, Understandable Code

Patina prioritizes clarity over cleverness:

- Well-documented Rust with clear separation of concerns
- Educational value - learn both Scheme and interpreter design
- Comprehensive test suite
- Reference implementations studied: Chibi Scheme, Chez Scheme

---

## Quick Start

```bash
# Build and run the REPL
cargo build --release
cargo run --release

# Run a Scheme script
./target/release/patina script.scm

# Run tests
cargo test
```

```scheme
patina> (define factorial
          (lambda (n)
            (if (<= n 1)
                1
                (* n (factorial (- n 1))))))

patina> (factorial 10)
3628800

patina> (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))

patina> (when (> 3 2) (display "yes\n"))
yes
```

---

## Architecture Highlights

### CoreExpr IR

A minimal intermediate representation with 9 core forms:

```rust
enum CoreExpr {
    Literal, Var, Quote,           // Values
    Lambda, If, Set, Define,       // Core forms
    Begin, App,                    // Control flow
}
```

All derived forms (`let`, `cond`, `case`, `do`, `and`, `or`, etc.) are macros that expand to these primitives.

### Hygienic Macros

Racket-style scope sets hygiene (based on "Binding as Sets of Scopes", Flatt 2016):

- Pattern matching with ellipsis
- Flip-scope algorithm for use-site vs introduced identifier discrimination
- No alpha-renaming needed

### Dual-Loader Library System

Balances performance with flexibility:

- **Rust libraries** - Performance-critical primitives compiled into the interpreter
- **Scheme libraries** - Derived functions and macros loaded from `.scm` files

### Full Numeric Tower

Complete Scheme numeric hierarchy with automatic promotion:

```
Integer (i64) → BigInteger → Rational → Real (f64) → Complex
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [Getting Started](docs/GETTING_STARTED.md) | Installation and first steps |
| [Development Guide](docs/DEVELOPMENT.md) | Architecture and contributing |
| [API Reference](docs/API.md) | Using Patina as a library |
| [Feature Status](docs/FEATURE_STATUS.md) | Detailed R7RS compliance matrix |
| [Test Organization](docs/TEST_ORGANIZATION.md) | Test structure and running tests |

---

## Project Structure

```
patina/
├── crates/
│   ├── patina-runtime/      # Core types, Backend trait
│   ├── patina-ir/           # CoreExpr IR
│   ├── patina-frontend/     # Lexer, Parser, Desugarer
│   ├── patina-macros/       # Macro expansion
│   ├── patina-pipeline/     # Pipeline orchestration
│   ├── patina-tree-walker/  # Tree-walking backend
│   ├── patina-interpreter/  # High-level API
│   ├── patina-repl/         # Terminal REPL
│   └── patina-tests/        # Integration tests
├── lib/                     # Scheme standard library
│   └── scheme/              # R7RS library implementations
├── docs/                    # User documentation
├── PRD/                     # Planning documents
└── spec/                    # R7RS specification
```

---

## Resources

### Specifications

- [R7RS Small Specification](http://www.scheme-reports.org/)
- Local copy in `spec/r7rs-small-spec/`

### Learning Materials

- [Structure and Interpretation of Computer Programs (SICP)](https://mitpress.mit.edu/sites/default/files/sicp/index.html)
- [The Scheme Programming Language (4th ed)](https://www.scheme.com/tspl4/)

### Reference Implementations

- [Chibi Scheme](https://github.com/ashinn/chibi-scheme) - R7RS reference implementation
- [Chez Scheme](https://cisco.github.io/ChezScheme/) - High-performance Scheme

---

## License

MIT License - See LICENSE file for details

---

## Acknowledgments

- R7RS editors and contributors for the excellent specification
- Alex Shinn and the Chibi Scheme project
- The Scheme community for decades of language design wisdom
