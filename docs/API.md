# Patina API Reference

Public API for using Patina as a library in your Rust programs.

## Overview

Patina provides a simple API through the `Interpreter` struct. You can embed Patina in your Rust applications to evaluate Scheme code programmatically.

## Quick Example

```rust
use patina::Interpreter;

fn main() {
    let interp = Interpreter::new();

    // Evaluate single expression
    let result = interp.eval_str("(+ 1 2 3)").unwrap();
    println!("{}", result);  // Prints: 6

    // Define variables
    interp.eval_str("(define x 42)").unwrap();
    let result = interp.eval_str("x").unwrap();
    println!("{}", result);  // Prints: 42

    // Evaluate program (multiple expressions)
    let result = interp.eval_program(r#"
        (define factorial
          (lambda (n)
            (if (<= n 1)
                1
                (* n (factorial (- n 1))))))
        (factorial 5)
    "#).unwrap();
    println!("{}", result);  // Prints: 120
}
```

## Core Types

### `Interpreter`

The main interpreter struct. Maintains environment state across evaluations.

```rust
pub struct Interpreter {
    // Internal implementation
}
```

#### Methods

##### `new() -> Interpreter`

Creates a new interpreter with a fresh global environment.

```rust
let interp = Interpreter::new();
```

Each interpreter maintains its own environment, so variables defined in one won't affect another.

##### `eval_str(&self, input: &str) -> Result<Value, InterpreterError>`

Evaluates a single Scheme expression from a string.

```rust
let result = interp.eval_str("(+ 1 2)")?;
```

**Returns:**
- `Ok(Value)` - The result of evaluation
- `Err(InterpreterError)` - Parse or evaluation error

**Note:** The interpreter maintains state between calls:

```rust
interp.eval_str("(define x 10)")?;
let result = interp.eval_str("(* x 2)")?;  // Uses previously defined x
```

##### `eval_program(&self, input: &str) -> Result<Value, InterpreterError>`

Evaluates multiple Scheme expressions, returning the last result.

```rust
let result = interp.eval_program(r#"
    (define x 10)
    (define y 20)
    (+ x y)
"#)?;
// Returns: 30 (result of last expression)
```

Definitions like `(define x 10)` return `Value::Unspecified` and won't be the final result unless they're the last expression.

### `Value`

Represents a Scheme value.

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
    InputPort,
    OutputPort,
    Unspecified,
    Eof,
}
```

#### Display

All values implement `Display`:

```rust
let result = interp.eval_str("(+ 1 2)")?;
println!("{}", result);  // Prints: 3
```

#### Pattern Matching

```rust
use patina::Value;

let result = interp.eval_str("(+ 1 2)")?;
match result {
    Value::Integer(n) => println!("Got integer: {}", n),
    Value::Boolean(b) => println!("Got boolean: {}", b),
    _ => println!("Got other value"),
}
```

### `InterpreterError`

Error type for interpretation failures.

```rust
pub enum InterpreterError {
    LexError(LexError),
    ParseError(ParseError),
    EvalError(EvalError),
}
```

All errors implement `Display` and `Error`:

```rust
match interp.eval_str("(+ 1") {
    Ok(val) => println!("Result: {}", val),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Usage Patterns

### REPL-style Interaction

```rust
let interp = Interpreter::new();

loop {
    let input = read_user_input();  // Your input function

    match interp.eval_str(&input) {
        Ok(Value::Unspecified) => {
            // Don't print unspecified (from define, set!, etc.)
        }
        Ok(value) => println!("{}", value),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

### Batch Processing

```rust
let interp = Interpreter::new();

for file in scheme_files {
    let code = std::fs::read_to_string(file)?;
    match interp.eval_program(&code) {
        Ok(result) => println!("{}: {}", file, result),
        Err(e) => eprintln!("{}: Error: {}", file, e),
    }
}
```

### Testing Helper

```rust
fn assert_scheme_eq(code: &str, expected: &str) {
    let interp = Interpreter::new();
    let result = interp.eval_str(code).unwrap();
    assert_eq!(result.to_string(), expected);
}

assert_scheme_eq("(+ 1 2)", "3");
assert_scheme_eq("(cons 1 2)", "(1 . 2)");
```

### Sandboxed Evaluation

Each interpreter is isolated:

```rust
let interp1 = Interpreter::new();
let interp2 = Interpreter::new();

interp1.eval_str("(define x 10)")?;
interp2.eval_str("(define x 20)")?;

let r1 = interp1.eval_str("x")?;  // 10
let r2 = interp2.eval_str("x")?;  // 20
```

## Error Handling

### Common Errors

```rust
use patina::InterpreterError;

match interp.eval_str(code) {
    Ok(value) => { /* ... */ },
    Err(InterpreterError::LexError(e)) => {
        // Tokenization error (invalid syntax)
        eprintln!("Syntax error: {}", e);
    },
    Err(InterpreterError::ParseError(e)) => {
        // Parse error (malformed expression)
        eprintln!("Parse error: {}", e);
    },
    Err(InterpreterError::EvalError(e)) => {
        // Evaluation error (undefined variable, type error, etc.)
        eprintln!("Runtime error: {}", e);
    },
}
```

### Error Messages

Errors provide helpful messages:

```rust
interp.eval_str("undefined-var");
// Error: Undefined variable: undefined-var

interp.eval_str("(+ 1 #t)");
// Error: Type error: + expects numbers

interp.eval_str("(car 42)");
// Error: Type error: car expects a pair
```

## Thread Safety

**Note:** `Interpreter` is **not** thread-safe. Each thread should have its own interpreter instance.

```rust
use std::thread;

// ✅ Good: Each thread gets its own interpreter
let handles: Vec<_> = (0..4).map(|i| {
    thread::spawn(move || {
        let interp = Interpreter::new();
        interp.eval_str(&format!("(+ {} 1)", i)).unwrap()
    })
}).collect();

// ❌ Bad: Sharing interpreter across threads
// let interp = Interpreter::new();  // DON'T DO THIS
// thread::spawn(move || interp.eval_str("...")); // NOT SAFE
```

## Performance Tips

### Use Release Builds

```rust
// Development: cargo build
// Production: cargo build --release
```

Release builds are significantly faster for evaluation.

### Reuse Interpreter

```rust
// ✅ Good: Reuse interpreter for related evaluations
let interp = Interpreter::new();
for expr in expressions {
    interp.eval_str(expr)?;
}

// ❌ Bad: Creating new interpreter each time
for expr in expressions {
    let interp = Interpreter::new();  // Unnecessary overhead
    interp.eval_str(expr)?;
}
```

### Batch Definitions

```rust
// ✅ Better: Load all definitions at once
interp.eval_program(r#"
    (define x 10)
    (define y 20)
    (define z 30)
"#)?;

// ❌ Slower: Individual eval_str calls
interp.eval_str("(define x 10)")?;
interp.eval_str("(define y 20)")?;
interp.eval_str("(define z 30)")?;
```

## Advanced Usage

### Custom REPL

See `src/repl/mod.rs` for the built-in REPL implementation as a reference.

### Extending the Interpreter

Currently, primitives are hardcoded. Future versions may support custom primitives via plugins.

## API Stability

**Current Status**: Phase 1 (R7RS implementation)

The API is evolving. Future changes may include:
- Custom primitive registration
- Module system support
- Better error types with source locations
- Async evaluation support

## Examples

See `examples/` directory for complete examples:
- `test_lambda.rs` - Lambda and closure demonstrations
- More examples coming soon

## Next Steps

- Read [DEVELOPMENT.md](DEVELOPMENT.md) for architecture details
- See [FEATURE_STATUS.md](FEATURE_STATUS.md) for what's implemented
- Check [TESTING.md](TESTING.md) for writing tests

---

**Questions?** See the [main docs](README.md) or check the source code with `cargo doc --open`.
