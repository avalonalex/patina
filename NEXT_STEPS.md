# Next Steps for Patina Development

## Immediate Priorities

### 1. Lambda and Closures
The most critical missing feature. Without `lambda`, you can't define functions!

**Implementation tasks:**
- [ ] Parse lambda syntax: `(lambda (x y) body...)`
- [ ] Store environment in Lambda variant for closures
- [ ] Update eval_lambda in eval/mod.rs
- [ ] Add apply logic for Lambda procedures
- [ ] Test lexical scoping

**Example test cases:**
```scheme
((lambda (x) (* x 2)) 5)  ; => 10

(define make-adder
  (lambda (n)
    (lambda (x) (+ x n))))

(define add5 (make-adder 5))
(add5 10)  ; => 15
```

### 2. Derived Expression Forms
These can be implemented as macros or directly in the evaluator:

- [ ] `let` - local bindings
- [ ] `let*` - sequential bindings
- [ ] `letrec` - recursive bindings
- [ ] `cond` - multi-way conditional
- [ ] `case` - pattern matching on values
- [ ] `and`, `or` - boolean operators

**Example:**
```scheme
(let ((x 5)
      (y 10))
  (+ x y))  ; => 15
```

### 3. Tail Call Optimization
Required for R7RS compliance. Without this, recursive functions will blow the stack.

**Implementation approaches:**
- Trampoline pattern (simpler)
- Continuation-passing style
- Proper tail call tracking

### 4. More Primitives
Add missing standard procedures:

**Numeric:**
- [ ] `quotient`, `remainder`, `modulo`
- [ ] `abs`, `floor`, `ceiling`, `truncate`, `round`
- [ ] `sqrt`, `expt`
- [ ] `>`, `<=`, `>=`
- [ ] Numeric predicates: `even?`, `odd?`, `positive?`, `negative?`, `zero?`

**List:**
- [ ] `list?`, `length`, `append`, `reverse`
- [ ] `list-ref`, `list-tail`
- [ ] `map`, `for-each`, `filter`

**String:**
- [ ] `string?`, `string-length`, `string-ref`, `substring`
- [ ] `string-append`, `string->list`, `list->string`

**Character:**
- [ ] `char?`, `char=?`, `char<?`, etc.
- [ ] `char->integer`, `integer->char`

**Vector:**
- [ ] `make-vector`, `vector-length`, `vector-ref`, `vector-set!`
- [ ] `vector->list`, `list->vector`

**Type predicates:**
- [ ] `boolean?`, `symbol?`, `number?`, `string?`, `procedure?`

### 5. Better Number Parsing
Currently only handles simple integers and floats.

**Add support for:**
- [ ] Exact/inexact prefixes: `#e`, `#i`
- [ ] Radix prefixes: `#b`, `#o`, `#d`, `#x`
- [ ] Rational numbers: `1/3`, `22/7`
- [ ] Complex numbers: `3+4i`
- [ ] Scientific notation: `1e10`, `3.14e-2`

### 6. Hygenic Macros
One of the harder features to implement correctly.

- [ ] `syntax-rules` - pattern-based macros
- [ ] `let-syntax` - local macro bindings
- [ ] `letrec-syntax` - recursive macros
- [ ] Proper hygiene and scope handling

### 7. I/O and Ports
- [ ] Port types (text/binary, input/output)
- [ ] `read`, `write`, `display`
- [ ] `open-input-file`, `open-output-file`
- [ ] `with-input-from-file`, `with-output-to-file`
- [ ] String ports

### 8. Exception Handling
- [ ] `guard` - exception handling form
- [ ] `raise` - raise exceptions
- [ ] Standard exception types

### 9. Continuations
Advanced feature, but required for R7RS:

- [ ] `call-with-current-continuation` (call/cc)
- [ ] `dynamic-wind`
- [ ] Exception continuations

### 10. Libraries and Modules
Final piece for full R7RS-small compliance:

- [ ] `define-library` - module definition
- [ ] `import` - import bindings
- [ ] `export` - export bindings
- [ ] Standard R7RS libraries

## Testing Strategy

### Phase 1: Unit Tests
Create Rust unit tests for each component:
```rust
#[test]
fn test_lambda_closure() {
    let eval = Evaluator::new();
    let result = eval.eval_str("((lambda (x) (lambda (y) (+ x y))) 5)");
    // Test that the result is a closure...
}
```

### Phase 2: Integration Tests
Create `.scm` test files that can be run through the interpreter:
```bash
cargo run < tests/basic_tests.scm
```

### Phase 3: R7RS Compliance Tests
Download and adapt Chibi Scheme tests:
```bash
git clone https://github.com/ashinn/chibi-scheme
cp chibi-scheme/tests/r7rs-tests.scm tests/
# Adapt as needed
```

## Architecture Improvements

### Consider Later
- [ ] Bytecode compiler + VM (for performance)
- [ ] Better error messages with source locations
- [ ] Debugger/stepper
- [ ] Profiler
- [ ] Garbage collection improvements
- [ ] Native FFI

## Future Phases

### Gradual Typing (Phase 2)
- Research Typed Racket's approach
- Design type annotation syntax
- Implement type inference
- Add runtime type checking

### Reactive Concurrency (Phase 3)
- Study Project Reactor patterns
- Design Scheme API for streams
- Implement backpressure
- Integration with Tokio

### Logic Programming (Phase 4)
- Study miniKanren implementation
- Add relational operators
- Implement unification
- Constraint solving

## Learning Resources

**Books:**
- SICP Chapter 4 (Metalinguistic Abstraction)
- Lisp in Small Pieces by Christian Queinnec
- Essentials of Programming Languages (EOPL)

**Papers:**
- "Definitional Interpreters for Higher-Order Programming Languages" by Reynolds
- "An Incremental Approach to Compiler Construction" by Abdulaziz Ghuloum
- "miniKanren: A Fresh Name in Nominal Logic Programming" by Byrd & Friedman

**Implementation References:**
- Chibi Scheme source code
- Rust Scheme interpreters on GitHub
- Write Yourself a Scheme tutorial
