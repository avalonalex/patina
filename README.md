# Patina - A Scheme R7RS Interpreter in Rust

An educational Scheme R7RS-small interpreter written in Rust, designed for learning both Scheme and interpreter implementation.

## Project Goals

1. **Phase 1: Basic R7RS-small Interpreter** (Current)
   - Implement core Scheme R7RS-small specification
   - Full lexical scoping and proper tail calls
   - Complete numeric tower (integers, rationals, reals, complex)
   - Proper hygenic macros (syntax-rules)
   - R7RS-small compliance testing

2. **Phase 2: Gradual Typing** (Future)
   - Add optional type annotations
   - Static type inference where possible
   - Runtime type checking with graceful degradation
   - Inspired by Typed Racket

3. **Phase 3: Reactive Concurrency** (Future)
   - Project Reactor-style reactive streams
   - Asynchronous programming primitives
   - Backpressure handling
   - Integration with Rust's async/await

4. **Phase 4: Logic Programming** (Future)
   - miniKanren-style relational programming
   - Constraint solving
   - Integration with the main Scheme interpreter

## Current Status (2025-11-02)

**Phase 1: R7RS Compliance** - 47% complete

### Recently Implemented (NEW!)
- ✅ **Lambda with full closures!**
  - Fixed arity: `(lambda (x y) body)`
  - Variadic: `(lambda args body)`
  - Mixed: `(lambda (x . rest) body)`
  - Proper environment capture
  - Higher-order functions working!

### Implemented
- ✅ Lexer with full R7RS token support
- ✅ Parser for S-expressions, vectors, bytevectors
- ✅ Tree-walking evaluator with environment model
- ✅ Special forms: `quote`, `if`, `define`, `set!`, `lambda`, `begin`, `cond`
- ✅ Arithmetic: `+`, `-`, `*`, `/`, `=`, `<`, `>`, `<=`, `>=`
- ✅ Lists: `cons`, `car`, `cdr`, `list`, `null?`, `pair?`
- ✅ Predicates: `eq?`, `eqv?`, `equal?`, `boolean?`, `number?`, `integer?`, etc.
- ✅ **Rich REPL** with:
  - Syntax highlighting
  - Multi-line editing with parenthesis balancing
  - Persistent history (`~/.patina_history`)
  - Emacs keybindings

### Next Priorities
- 🚧 `let`, `let*`, `letrec` - Local bindings (blocks 23% of tests!)
- 🚧 `and`, `or` - Boolean operators
- 🚧 `apply`, `map`, `for-each` - Higher-order functions
- 🚧 More list operations: `length`, `append`, `reverse`
- 🚧 Numeric operations: `abs`, `quotient`, `remainder`, predicates
- ❌ String operations
- ❌ Vector operations
- ❌ I/O operations
- ❌ Tail call optimization
- ❌ Macros (syntax-rules)

**See [docs/FEATURE_STATUS.md](docs/FEATURE_STATUS.md) for complete feature matrix.**

## R7RS-small Compliance Testing

### Test Suites

The most comprehensive and widely-used R7RS-small test suite is from **Chibi Scheme**, which is maintained by Alex Shinn (the chairman of the R7RS Small Language committee).

**Repository**: [chibi-scheme](https://github.com/ashinn/chibi-scheme)
**Test Location**: `tests/r7rs-tests.scm`

The Chibi test suite covers:
- All procedures and syntax in R7RS-small (except `delete-file`)
- Full Unicode support
- Complete numeric tower
- All standard libraries

### Other Test Resources
- **Larceny**: R7RS benchmarks and tests
- **Gauche, Chicken, Cyclone, Kawa**: Each maintains their own test suites
- You can download and adapt these tests to validate Patina's compliance

### Running Tests

```bash
# All tests
cargo test

# Compliance tests (R7RS spec-organized)
cargo test --test compliance

# Integration tests
cargo test --test integration

# Generate progress report
./scripts/test_report.sh
```

**See [docs/TESTING.md](docs/TESTING.md) for complete testing guide.**

## Quick Start

```bash
# Build and run
cargo build --release
cargo run --release

# Try it out
patina> (define factorial
...       (lambda (n)
...         (if (<= n 1)
...             1
...             (* n (factorial (- n 1))))))
patina> (factorial 5)
120
```

**See [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) for complete guide.**

## Documentation

- **[Getting Started](docs/GETTING_STARTED.md)** - Installation and first steps
- **[Feature Status](docs/FEATURE_STATUS.md)** - What's implemented (47% complete)
- **[Testing Guide](docs/TESTING.md)** - Running and writing tests
- **[Development Guide](docs/DEVELOPMENT.md)** - Architecture and contributing
- **[API Reference](docs/API.md)** - Using Patina as a library

**For future plans:**
- **[PRD/ROADMAP.md](PRD/ROADMAP.md)** - Development roadmap
- **[PRD/phase4/](PRD/phase4/)** - Notebook system design (Phase 4)

## Project Structure

```
src/
├── lexer/           # Tokenization
├── parser/          # AST construction
├── eval/            # Evaluation engine
├── value/           # Scheme values
├── env/             # Environments/scoping
├── repl/            # REPL interface
├── lib.rs           # Public API
└── main.rs          # CLI entry

docs/                # Current documentation
├── GETTING_STARTED.md
├── FEATURE_STATUS.md
├── TESTING.md
├── DEVELOPMENT.md
└── API.md

PRD/                 # Future plans & designs
├── ROADMAP.md
├── phase1/          # R7RS (current)
├── phase2/          # Gradual typing
├── phase3/          # Reactive
└── phase4/          # Notebook system

tests/
├── compliance/      # R7RS spec tests
├── integration/     # End-to-end tests
└── fixtures/        # Test data
```

## REPL Features

Patina includes a rich, modern REPL:

- **Syntax Highlighting** - Color-coded as you type
- **Multi-line Editing** - Intelligent parenthesis balancing
- **Persistent History** - Saved to `~/.patina_history`
- **History Search** - Ctrl+R for reverse search
- **Emacs Keybindings** - Ctrl+A, Ctrl+E, Ctrl+K, etc.

## Future: Notebook System (Phase 4)

Designed but not yet implemented - a terminal-based computational notebook with S-expression format.

**Key features (planned):**
- Notebooks as valid Scheme programs (`.scm.nb`)
- Cell-based editing in terminal
- Three-tier command system (Scheme/Tables/Shell)
- Dependency tracking
- Export to HTML/Markdown

**See [PRD/phase4/NOTEBOOK_OVERVIEW.md](PRD/phase4/NOTEBOOK_OVERVIEW.md) for complete design.**
- Composable with macros
- No JSON bloat

**Why three-tier commands?**
- Type-safe: `(file-info-size f)` not `(parse-string ...)`
- Cross-platform: Native ops work everywhere
- Composable: `(filter pred (ls))` just works
- Better than Jupyter's `!` and `%` magic

See documentation:
- [**THREE_TIER_SUMMARY.md**](docs/THREE_TIER_SUMMARY.md) - 🌟 Visual guide to system integration
- [NATIVE_COMMANDS.md](docs/NATIVE_COMMANDS.md) - Native Scheme commands
- [NOTEBOOK_FORMAT.md](docs/NOTEBOOK_FORMAT.md) - Complete S-expr format spec
- [SYSTEM_INTEGRATION.md](docs/SYSTEM_INTEGRATION.md) - Detailed command integration
- [TUI_IMPLEMENTATION.md](docs/TUI_IMPLEMENTATION.md) - Implementation guide
- [NOTEBOOK_DESIGN.md](docs/NOTEBOOK_DESIGN.md) - Original vision

Examples:
- [sample-notebook.scm.nb](examples/sample-notebook.scm.nb) - Tutorial
- [system-integration-demo.scm.nb](examples/system-integration-demo.scm.nb) - Three tiers in action

## Example Usage

```scheme
> (+ 1 2 3)
6

> (define x 10)
#<unspecified>

> (* x 5)
50

> (if (< x 20) "small" "large")
"small"

> (list 1 2 3 4)
(1 2 3 4)

> (define (factorial n)
    (if (= n 0)
        1
        (* n (factorial (- n 1)))))
#<unspecified>

> (factorial 5)
120
```

## Architecture Notes

### Value Representation
- Uses Rust enums for type-safe value representation
- `Rc<T>` for shared immutable data (strings, symbols, pairs)
- Numeric tower support via `num-bigint` and `num-rational` crates

### Environment Model
- Lexical scoping with parent environment chains
- `Rc<RefCell<HashMap>>` for mutable bindings (needed for `set!`)
- Separate environments for each scope

### Evaluation Strategy
- Tree-walking interpreter (Phase 1)
- TODO: Bytecode compiler and VM (Phase 1.5, optional)
- TODO: Tail call optimization (required for R7RS)

## Contributing

This is a learning project! Areas that need work:
1. Implementing remaining R7RS-small features
2. Adding comprehensive tests from Chibi Scheme
3. Performance optimizations
4. Better error messages and debugging support

## Resources

### R7RS Specification
- [R7RS Small Specification (PDF)](http://www.scheme-reports.org/2013/r7rs-small-spec.zip)
- [Official R7RS Website](http://www.scheme-reports.org/)

### Learning Materials
- [Structure and Interpretation of Computer Programs (SICP)](https://mitpress.mit.edu/sites/default/files/sicp/index.html)
- [The Scheme Programming Language (4th ed)](https://www.scheme.com/tspl4/)
- [Write Yourself a Scheme in 48 Hours](https://en.wikibooks.org/wiki/Write_Yourself_a_Scheme_in_48_Hours)

### Reference Implementations
- [Chibi Scheme](https://github.com/ashinn/chibi-scheme) - Reference R7RS implementation
- [Gauche](https://practical-scheme.net/gauche/) - Production-ready Scheme
- [Guile](https://www.gnu.org/software/guile/) - GNU's extensibility language

## License

MIT License - See LICENSE file for details

## Acknowledgments

- R7RS editors and contributors
- The Scheme community
- All the reference implementations that make learning possible
