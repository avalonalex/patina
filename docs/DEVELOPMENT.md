# Development Guide

Guide to understanding Patina's architecture and contributing to the project.

## Architecture Overview

### Core Pipeline

```
Source Code
    ↓
[Lexer] → Tokens
    ↓
[Parser] → AST (Value enum)
    ↓
[Evaluator] → Result
```

### Directory Structure

```
src/
├── lexer/          # Tokenization
│   └── mod.rs      # Token stream from source
├── parser/         # AST construction
│   └── mod.rs      # Build Value tree from tokens
├── eval/           # Evaluation engine
│   └── mod.rs      # Tree-walking interpreter
├── value/          # Value types
│   └── mod.rs      # Scheme value representation
├── env/            # Environments
│   └── mod.rs      # Variable scoping
├── repl/           # REPL interface
│   ├── mod.rs      # Main REPL loop
│   ├── highlighter.rs   # Syntax highlighting
│   ├── validator.rs     # Multi-line validation
│   └── completer.rs     # Tab completion (TODO)
├── lib.rs          # Public API
└── main.rs         # CLI entry point
```

## Key Components

### 1. Value Representation (`src/value/mod.rs`)

All Scheme values are represented by the `Value` enum:

```rust
pub enum Value {
    Boolean(bool),
    Integer(i64),
    BigInteger(BigInt),
    Rational(BigRational),
    Real(f64),
    Complex(f64, f64),
    Character(char),
    String(Rc<String>),
    Symbol(Rc<str>),
    Pair(Rc<(Value, Value)>),
    Null,
    Vector(Rc<Vec<Value>>),
    Bytevector(Rc<Vec<u8>>),
    Procedure(Procedure),
    // ... more
}
```

**Key Design Choices:**
- `Rc<T>` for immutable sharing (symbols, strings, pairs)
- No copying - values are shared via reference counting
- Numeric tower fully represented (even if not all implemented)

### 2. Environment Model (`src/env/mod.rs`)

Lexical scoping with parent environment chains:

```rust
pub struct Environment {
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    parent: Option<Rc<Environment>>,
}
```

**Why `Rc<RefCell<>>`?**
- `Rc` - Shared ownership (closures capture environments)
- `RefCell` - Interior mutability (needed for `set!`)

**Operations:**
- `define(name, value)` - Create new binding in current env
- `set(name, value)` - Update existing binding (searches parents)
- `get(name)` - Lookup value (searches parents)

### 3. Evaluator (`src/eval/mod.rs`)

Tree-walking interpreter with special form handling:

```rust
fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    match expr {
        Value::Symbol(name) => env.get(name),  // Variable lookup
        Value::Pair(_) => self.eval_list(expr, env),  // Function call or special form
        _ => Ok(expr.clone()),  // Self-evaluating
    }
}
```

**Special Forms** (hardcoded in `eval_list`):
- `quote` - Return unevaluated
- `if` - Conditional
- `define` - Binding
- `set!` - Mutation
- `lambda` - Function creation **with closure support!**
- `begin` - Sequential evaluation
- `cond` - Multi-branch conditional

**Procedure Application:**
- Primitives: Rust functions (fast)
- Lambdas: Evaluate body in extended environment (with closure)

### 4. Lexer (`src/lexer/mod.rs`)

Converts source text to token stream:

```
"(+ 1 2)" → [LParen, Symbol("+"), Number(1), Number(2), RParen]
```

**Features:**
- Handles all R7RS literals
- Comment handling
- Multi-line strings
- Datum labels (TODO)

### 5. Parser (`src/parser/mod.rs`)

Builds AST from tokens:

```
[LParen, Symbol("+"), Number(1), Number(2), RParen]
    ↓
Pair(Symbol("+"), Pair(Integer(1), Pair(Integer(2), Null)))
```

**Note:** AST IS the `Value` enum - no separate AST type!

### 6. REPL (`src/repl/mod.rs`)

Rich terminal interface with `rustyline`:

- **Highlighter** (`highlighter.rs`) - Real-time syntax coloring
- **Validator** (`validator.rs`) - Multi-line input detection
- **Completer** (`completer.rs`) - Tab completion (TODO)

## Adding Features

### Adding a New Primitive

1. **Add to `install_primitives`** in `src/eval/mod.rs`:

```rust
fn install_primitives(env: &Rc<Environment>) {
    let primitives = [
        // ... existing primitives
        ("my-new-primitive", Arity::Exact(2)),  // Takes exactly 2 args
    ];
    // ...
}
```

2. **Implement the function** in `src/eval/mod.rs`:

```rust
fn primitive_my_new(& self, args: Vec<Value>) -> Result<Value, EvalError> {
    // args are already evaluated
    // Implement your logic
    Ok(Value::Integer(42))
}
```

3. **Add to dispatch** in `apply_primitive`:

```rust
fn apply_primitive(&self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
    match name {
        // ... existing cases
        "my-new-primitive" => self.primitive_my_new(args),
        _ => Err(EvalError::InvalidSyntax(format!("Unknown primitive: {}", name))),
    }
}
```

4. **Write tests** in `tests/compliance/`:

```rust
#[test]
fn test_my_new_primitive() {
    assert_eval_to("(my-new-primitive 1 2)", "42");
}
```

### Adding a New Special Form

1. **Add case in `eval_list`** in `src/eval/mod.rs`:

```rust
fn eval_list(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    let (car, cdr) = self.extract_pair(expr)?;

    if let Value::Symbol(ref sym) = car {
        match sym.as_ref() {
            // ... existing special forms
            "my-special-form" => return self.eval_my_special(&cdr, env),
            _ => {}
        }
    }
    // ... rest of function
}
```

2. **Implement handler**:

```rust
fn eval_my_special(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // Parse args
    let (first, rest) = self.extract_pair(args)?;

    // Evaluate as needed (special forms control evaluation!)
    let value = self.eval_in_env(&first, env)?;

    // Return result
    Ok(value)
}
```

3. **Write tests**:

```rust
#[test]
fn test_my_special_form() {
    assert_eval_to("(my-special-form ...)", "expected");
}
```

### Adding a New Value Type

1. **Extend `Value` enum** in `src/value/mod.rs`:

```rust
pub enum Value {
    // ... existing variants
    MyNewType(MyData),
}
```

2. **Implement `Display`** in `src/value/mod.rs`:

```rust
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // ... existing cases
            Value::MyNewType(data) => write!(f, "#<my-type: {}>", data),
        }
    }
}
```

3. **Update evaluator** if self-evaluating:

```rust
fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    match expr {
        // ... existing self-evaluating
        Value::MyNewType(_) => Ok(expr.clone()),
        // ...
    }
}
```

## Code Organization Principles

### Error Handling

Use the appropriate error type:

```rust
// Evaluation errors
return Err(EvalError::UndefinedVariable("x".to_string()));
return Err(EvalError::TypeError("Expected number".to_string()));
return Err(EvalError::WrongArity { expected: "2", actual: 1 });

// Parse errors
return Err(ParseError::UnexpectedToken(token));

// Wrap at API boundary
pub fn eval_str(&self, input: &str) -> Result<Value, InterpreterError> {
    // Converts internal errors to InterpreterError
}
```

### Memory Management

**Prefer sharing over cloning:**

```rust
// ✅ Good: Share via Rc
let sym = Rc::from("my-symbol");
Value::Symbol(sym.clone())  // Just increments ref count

// ❌ Bad: Unnecessary clone
let s = "my-symbol".to_string();
Value::Symbol(Rc::from(s.clone()))  // Clones string unnecessarily
```

**When to use what:**
- `Rc<T>` - Immutable shared data (most values)
- `Rc<RefCell<T>>` - Mutable shared data (environments only)
- Clone only when necessary

### Style Guidelines

```rust
// Use Result for errors, not panic!
// ✅ Good
fn divide(a: i64, b: i64) -> Result<i64, EvalError> {
    if b == 0 {
        return Err(EvalError::DivisionByZero);
    }
    Ok(a / b)
}

// ❌ Bad
fn divide(a: i64, b: i64) -> i64 {
    a / b  // Panics on zero!
}

// Use meaningful names
// ✅ Good
fn eval_if(&self, args: &Value, env: &Rc<Environment>)

// ❌ Bad
fn eval_i(&self, a: &Value, e: &Rc<Environment>)

// Extract complex logic to helpers
// ✅ Good
fn eval_lambda(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    let (params, variadic) = self.parse_lambda_params(&params_expr)?;
    let body = self.collect_list_items(&rest)?;
    // ...
}
```

## Testing Strategy

See [TESTING.md](TESTING.md) for complete guide.

**Quick checklist:**
- ✅ Write test before implementing feature
- ✅ Test edge cases (empty, zero, null, etc.)
- ✅ Test error conditions
- ✅ Use descriptive test names
- ✅ One assertion per test (usually)

## Code Quality

### Before Committing

```bash
# Format code
cargo fmt

# Run linter
cargo clippy

# Run tests
cargo test

# Check compilation
cargo check
```

### CI Requirements

All PRs must:
- Pass `cargo test`
- Pass `cargo clippy`
- Pass `cargo fmt --check`
- Not break existing tests

## Performance Considerations

### Current Approach

Patina prioritizes **correctness** over performance in Phase 1.

**What's NOT optimized yet:**
- No tail call optimization (coming soon!)
- Tree-walking interpreter (not bytecode)
- Ref-counted values (not arena-allocated)

**What IS efficient:**
- Rc sharing (minimal copying)
- Built-in primitives (direct Rust)

### Future Optimizations (Phase 1+)

1. **Tail Call Optimization** - Required by R7RS
2. **Bytecode compilation** - Optional, after R7RS complete
3. **Better number handling** - Avoid allocations for small integers

## Contributing

### Workflow

1. **Pick a feature** from [FEATURE_STATUS.md](FEATURE_STATUS.md)
2. **Write tests first** (TDD approach)
3. **Implement** following patterns above
4. **Run tests** (`cargo test`)
5. **Submit PR**

### Good First Issues

Easy features to start with:
- `not` - Boolean negation
- `abs` - Absolute value
- `zero?`, `positive?`, `negative?` - Numeric predicates
- `length` - List length
- `append` - List concatenation

See [FEATURE_STATUS.md](FEATURE_STATUS.md) for status.

### Getting Help

- Read existing code in `src/eval/mod.rs` for patterns
- Check tests in `tests/compliance/` for examples
- See R7RS spec in `spec/r7rs-small-spec/`
- Use chibi-scheme as reference (see [CHIBI_REFERENCE.md](CHIBI_REFERENCE.md))

## Debugging

### Using cargo run

```bash
# Add debug prints
println!("Debug: {:?}", value);

# Run specific test with output
cargo test test_name -- --nocapture
```

### Using rust-lldb/rust-gdb

```bash
# Build with debug info
cargo build

# Debug
rust-lldb target/debug/patina
```

### Tracing Evaluation

Add temporary debug output in `eval_in_env`:

```rust
fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    eprintln!("Evaluating: {}", expr);  // Temporary debug
    // ... rest of function
}
```

## Documentation

### Code Documentation

```rust
/// Evaluates a lambda expression and creates a closure.
///
/// # Arguments
/// * `args` - The lambda's parameter list and body
/// * `env` - The environment to capture (for closures)
///
/// # Returns
/// A `Value::Procedure` containing the lambda
fn eval_lambda(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // ...
}
```

### Generating Docs

```bash
cargo doc --open
```

## Project Phases

### Phase 1: R7RS-small (Current)

Focus: Correctness and compliance

**Status:** 47% complete
- ✅ Lambda with closures
- 🚧 let/let*/letrec
- 🚧 Core list operations
- ❌ Strings, vectors, I/O

### Future Phases

- **Phase 2:** Gradual typing (Typed Racket style)
- **Phase 3:** Reactive concurrency (Project Reactor style)
- **Phase 4:** Notebook system (Terminal-based)

See [../PRD/](../PRD/) for designs.

## Resources

- **R7RS Spec**: `spec/r7rs-small-spec/` (LaTeX source)
- **Chibi-scheme**: `~/Project/reference/chibi-scheme`
- **Test Suite**: `tests/compliance/`
- **Progress**: [FEATURE_STATUS.md](FEATURE_STATUS.md)

## Questions?

- Check [API.md](API.md) for public API
- See [TESTING.md](TESTING.md) for tests
- Read [FEATURE_STATUS.md](FEATURE_STATUS.md) for what's done
- Review existing code for patterns

---

**Happy hacking!** 🦀✨
