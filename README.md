# Patina - A Scheme R7RS Interpreter in Rust

**A modular, multi-backend Scheme interpreter designed for experimentation**

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)]()
[![License](https://img.shields.io/badge/license-MIT-blue)]()

---

## ⚠️ An experiment in agent-driven development

**This repository is, very deliberately, an experiment in building software
with AI coding agents.** The overwhelming majority of its code, tests,
documentation, and planning documents were written by an AI agent (Claude
Code) working under human direction: the human sets the goals, reviews the
designs and diffs, merges every PR, and owns every decision — the agent does
the reading, writing, debugging, benchmarking, and bookkeeping. The history
is honest about this: nearly every commit carries a `Co-Authored-By: Claude`
trailer, and the `PRD/` directory contains the actual working documents the
human–agent pair planned with, warts and all.

Two consequences worth stating plainly:

- **Read it as an experiment.** It is a probe of what agent-driven
  development can produce on a substantial project (an interpreter with a
  bytecode VM, hygienic macros, and a full numeric tower) — not a hardened
  production Scheme.
- **It is tested like it matters anyway.** 1163/1163 of the chibi-scheme
  R7RS suite on both backends, ~1,400 integration tests, and differential
  testing between the two backends gate every change.

---

## Philosophy

Patina is an educational and experimental Scheme interpreter with ambitious goals:

### Full R7RS-small Compliance

Our primary focus is complete conformance to the R7RS-small specification. We validate against the comprehensive [Chibi Scheme](https://github.com/ashinn/chibi-scheme) test suite maintained by Alex Shinn, chairman of the R7RS Small Language committee.

**Current status**: 100% of chibi r7rs-tests.scm passing (1163/1163 tests) on both backends.

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
    fn eval(&self, expr: TaggedValue, env: &Rc<Environment>)
        -> Result<TaggedValue, Self::Error>;
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
patina-vm           →  Register-based bytecode VM (default backend)
patina-tree-walker  →  CPS tree-walking backend (--tree-walker)
patina-interpreter  →  High-level API
patina-repl         →  Terminal REPL
patina-tests        →  Integration tests (~1400 tests)
```

### Future Goals

The architecture is designed to support future exploration:

- `syntax-case` procedural macros
- Nanopass-style optimization passes
- Language extensions (gradual typing, reactive concurrency, logic programming)

### Known Limitations

- **Garbage collection is stop-the-world.** A non-moving mark-and-sweep
  collector runs on both backends (`(gc)` and `(gc-stats)` in
  `(patina debug)` give manual control). Pauses are unbounded by
  generational or incremental techniques — that work is staged in
  `docs/GC_DESIGN.md`.
- Performance is that of a young interpreter: far beyond a naive
  tree-walker and improving quickly (the VM gained 2–3× on arithmetic- and
  list-heavy code in the most recent optimization wave), but not yet
  competitive with mature Schemes like Chez.

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
./target/release/patina

# Run a Scheme script (uses VM backend by default)
./target/release/patina script.scm

# Use the tree-walking backend instead
./target/release/patina --tree-walker script.scm

# Disassemble bytecode / trace execution
./target/release/patina --dump script.scm
./target/release/patina --trace script.scm

# R7RS compliance suite (the canonical gate)
./scripts/run_chibi_tests.sh

# Rust unit + integration tests
cargo test --all --lib --tests
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

A minimal intermediate representation with 13 core forms:

```rust
enum CoreExprKind {
    Literal, Var, Quote, Quasiquote,   // Values
    Lambda, If, Set, Begin,            // Core forms
    Define, Import, Expand,            // Toplevel
    App, Apply,                        // Application
}
```

All derived forms (`let`, `cond`, `case`, `do`, `and`, `or`, `when`,
`define-record-type`, etc.) are `syntax-rules` macros written in Scheme
(`lib/scheme/`) that expand to these primitives.

### Hygienic Macros

Racket-style scope sets hygiene (based on "Binding as Sets of Scopes", Flatt 2016):

- Pattern matching with ellipsis
- Flip-scope algorithm for use-site vs introduced identifier discrimination
- No alpha-renaming needed

### R7RS Library System

Complete R7RS-compliant library system:

- **`.sld` files** - R7RS library definitions with exports and includes
- **Rust primitives** - Performance-critical operations in internal modules
- **Scheme implementations** - Macros and derived forms in `.scm` files
- **Full support** - `cond-expand`, `include`, `include-ci`, `include-library-declarations`

### Full Numeric Tower

Complete Scheme numeric hierarchy with automatic promotion:

```
Integer (i64) → BigInteger → Rational → Real (f64) → Complex
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [VM Decisions](docs/VM_DECISIONS.md) | Settled VM architecture decisions (master reference) |
| [VM ISA](docs/VM_ISA.md) | Instruction set architecture and semantics |
| [VM Compiler](docs/VM_COMPILER.md) | The 5-pass compiler pipeline |
| [VM Runtime](docs/VM_RUNTIME.md) | Execution loop and control primitives |
| [Macro System](docs/MACRO_SYSTEM.md) | Scope-set hygiene and the flip-scope algorithm |
| [Test Organization](docs/TEST_ORGANIZATION.md) | Test structure and running tests |

The `PRD/` directory holds the living planning documents — including
`PRD/TRACK_P_PERFORMANCE_PRD.md` with the measured performance progress log.

---

## Project Structure

```
patina/
├── crates/
│   ├── patina-runtime/      # Core types, Backend trait, internal primitives
│   ├── patina-ir/           # CoreExpr IR
│   ├── patina-frontend/     # Lexer, Parser, Desugarer
│   ├── patina-macros/       # Macro expansion
│   ├── patina-pipeline/     # Pipeline orchestration
│   ├── patina-vm/           # Register-based bytecode VM (default)
│   ├── patina-tree-walker/  # CPS tree-walking backend
│   ├── patina-interpreter/  # High-level API
│   ├── patina-repl/         # Terminal REPL
│   └── patina-tests/        # Integration tests (~1400 tests)
├── lib/                     # Scheme standard library
│   └── scheme/              # R7RS .sld libraries and implementations
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

## Third-Party Code

- `scheme_tests/chibi/r7rs-tests.scm` is from
  [chibi-scheme](https://github.com/ashinn/chibi-scheme) (BSD 3-Clause);
  see `scheme_tests/README.md` for attribution and the full license text.
- `crates/patina-tests/bench_programs/{nboyer,sboyer}.scm` are the classic
  Boyer benchmarks (Public Domain, original headers retained), vendored via
  [ecraven/r7rs-benchmarks](https://github.com/ecraven/r7rs-benchmarks).

## License

MIT License - See [LICENSE](LICENSE) file for details

---

## Acknowledgments

- R7RS editors and contributors for the excellent specification
- Alex Shinn and the Chibi Scheme project
- The Scheme community for decades of language design wisdom
