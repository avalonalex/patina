# Patina Documentation

**Current implementation documentation** for Patina, a Scheme R7RS interpreter written in Rust.

For future plans and designs, see [../PRD/](../PRD/).

## Quick Links

- [Getting Started](GETTING_STARTED.md) - Installation and first steps
- [Feature Status](FEATURE_STATUS.md) - What's currently implemented
- [Testing](TESTING.md) - How to run and write tests
- [Development](DEVELOPMENT.md) - Architecture and contributing
- [API Reference](API.md) - Interpreter API documentation

## Documentation Index

### For Users

**[Getting Started](GETTING_STARTED.md)**
- Installation
- Running the REPL
- Basic usage
- Examples

**[Feature Status](FEATURE_STATUS.md)**
- Implemented features
- Work in progress
- Planned features
- R7RS compliance progress

### For Developers

**[Development Guide](DEVELOPMENT.md)**
- Architecture overview
- Code organization
- Adding new features
- Error handling
- Memory management

**[Testing Guide](TESTING.md)**
- Running tests
- Writing tests
- Test organization
- Using chibi-scheme for reference
- Progress tracking

**[API Reference](API.md)**
- `Interpreter` struct
- Public methods
- Usage examples
- Integration guide

**[Chibi Reference](CHIBI_REFERENCE.md)**
- Using chibi-scheme as reference
- Running comparison tests
- R7RS test suite

## Project Structure

```
patina/
├── src/                # Source code
│   ├── lexer/         # Tokenization
│   ├── parser/        # AST construction
│   ├── eval/          # Evaluation engine
│   ├── value/         # Value types
│   ├── env/           # Environment/scoping
│   └── repl/          # REPL interface
├── tests/             # Test suite
│   ├── compliance/    # R7RS compliance tests
│   ├── integration/   # End-to-end tests
│   └── fixtures/      # Test data
├── docs/              # This directory (current docs)
├── PRD/               # Future plans & designs
└── examples/          # Example programs
```

## Current Status (2025-11-02)

**Phase 1: R7RS-small Compliance** - 47% complete

- ✅ Lambda with closures
- ✅ Basic special forms (quote, if, define, set!, begin, cond)
- ✅ Arithmetic operations
- ✅ List operations
- ✅ Type predicates
- 🚧 Binding forms (let, let*, letrec)
- 🚧 Boolean operators (and, or)
- ❌ String operations
- ❌ Vector operations
- ❌ I/O operations

See [FEATURE_STATUS.md](FEATURE_STATUS.md) for detailed progress.

## Contributing

1. Read the [Development Guide](DEVELOPMENT.md)
2. Check [Feature Status](FEATURE_STATUS.md) for what needs work
3. See [Testing Guide](TESTING.md) for writing tests
4. Submit pull requests!

## Resources

- **R7RS Specification**: `spec/r7rs-small-spec/`
- **Chibi-scheme Reference**: `~/Project/reference/chibi-scheme`
- **Test Suite**: `tests/compliance/`
- **Feature Matrix**: `tests/FEATURE_MATRIX.md`

## Future Phases

This documentation covers Phase 1 (R7RS compliance). For future phases:

- **Phase 2**: Gradual typing - See `PRD/phase2/`
- **Phase 3**: Reactive concurrency - See `PRD/phase3/`
- **Phase 4**: Notebook system - See `PRD/phase4/`

---

**Questions?** Check the appropriate guide above or see the [main README](../README.md).
