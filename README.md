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

## Current Status

### Implemented
- ✅ Lexer with full R7RS token support
- ✅ Parser for S-expressions, vectors, bytevectors
- ✅ Basic evaluator with environment model
- ✅ Core special forms: `quote`, `if`, `define`, `set!`, `begin`
- ✅ Basic primitives: arithmetic, list operations, predicates
- ✅ **Rich REPL** with:
  - Syntax highlighting (keywords, builtins, strings, numbers)
  - Multi-line editing with parenthesis balancing
  - Persistent history across sessions
  - History-based hints
  - Emacs-style keybindings

### TODO for R7RS-small Compliance
- [ ] Complete special forms: `lambda`, `let`, `let*`, `letrec`, `cond`, `case`, `and`, `or`
- [ ] Proper closures with lexical scoping
- [ ] Tail call optimization
- [ ] Full numeric tower (currently only integers and floats)
- [ ] Hygenic macros (syntax-rules, let-syntax, letrec-syntax)
- [ ] Complete standard library procedures
- [ ] String and character operations with full Unicode support
- [ ] Port and I/O operations
- [ ] Exception handling (guard, raise)
- [ ] Continuations (call/cc, dynamic-wind)
- [ ] Record types (define-record-type)
- [ ] Libraries and modules (define-library, import, export)

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
# TODO: Add test runner once implemented
# Will support running Chibi test suite
cargo test
```

## Project Structure

```
src/
├── main.rs          # Entry point
├── repl/            # Rich REPL implementation
│   ├── mod.rs           # Main REPL loop
│   ├── highlighter.rs   # Syntax highlighting
│   ├── validator.rs     # Multi-line validation
│   └── completer.rs     # Auto-completion (TODO)
├── lexer/           # Tokenization
│   └── mod.rs
├── parser/          # Parse tokens into AST
│   └── mod.rs
├── value/           # Scheme value representation
│   └── mod.rs
├── env/             # Environment and bindings
│   └── mod.rs
└── eval/            # Evaluator and primitives
    └── mod.rs

docs/
├── REPL_FEATURES.md     # REPL documentation
└── NOTEBOOK_DESIGN.md   # Notebook mode design

tests/
└── basic_tests.scm      # Example test cases
```

## Building and Running

```bash
# Build the project
cargo build --release

# Run the REPL
cargo run --release

# Run tests
cargo test
```

## REPL Features

Patina includes a rich, modern REPL inspired by Chez Scheme's Expeditor:

- **Syntax Highlighting** - Color-coded as you type
- **Multi-line Editing** - Intelligent parenthesis balancing
- **Persistent History** - Saved to `~/.patina_history`
- **History Search** - Ctrl+R for reverse search
- **Smart Hints** - Suggestions from history
- **Emacs Keybindings** - Ctrl+A, Ctrl+E, Ctrl+K, etc.

See [docs/REPL_FEATURES.md](docs/REPL_FEATURES.md) for detailed documentation.

### Future: Notebook Mode

We're designing a terminal-based notebook interface (like Jupyter for Scheme). See [docs/NOTEBOOK_DESIGN.md](docs/NOTEBOOK_DESIGN.md) for the vision.

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
