# Closure Examples

This directory contains comprehensive examples demonstrating Patina's full support for **lexical closures**, one of the most powerful features in Scheme.

## What are Closures?

A **closure** is a function that "remembers" the environment in which it was created. It can access variables from its enclosing scope even after that scope has finished executing.

## Examples Overview

### 01_basic_closure.scm
**Concepts**: Variable capture, lexical scoping, independent environments

Simple examples showing:
- Capturing variables from `let` bindings
- Creating functions that return functions
- Each closure maintaining its own environment

**Try it:**
```bash
cargo run < tests/fixtures/examples/closures/01_basic_closure.scm
```

### 02_stateful_closure.scm
**Concepts**: Mutable state, `set!`, state encapsulation

Shows how closures can maintain and modify private state:
- Counter pattern
- Bank account with balance
- Multiple independent stateful objects

**Try it:**
```bash
cargo run < tests/fixtures/examples/closures/02_stateful_closure.scm
```

### 03_higher_order.scm
**Concepts**: Functions as values, composition, currying

Advanced patterns:
- Function composition (`compose`)
- Partial application (currying)
- Functions returning different closures based on input

**Try it:**
```bash
cargo run < tests/fixtures/examples/closures/03_higher_order.scm
```

### 04_multiple_captures.scm
**Concepts**: Capturing multiple variables, nested scopes

Demonstrates:
- Objects with multiple properties (points, rectangles)
- Capturing from multiple nesting levels
- Complex state management

**Try it:**
```bash
cargo run < tests/fixtures/examples/closures/04_multiple_captures.scm
```

### 05_practical_examples.scm
**Concepts**: Real-world patterns, data structures

Practical implementations:
- Memoization (caching)
- Timer/stopwatch
- Stack data structure

**Try it:**
```bash
cargo run < tests/fixtures/examples/closures/05_practical_examples.scm
```

### 06_nested_closures.scm
**Concepts**: Deep nesting, recursive closures, mutual recursion

Advanced topics:
- Closures returning closures returning closures
- Closures sharing mutable state
- Recursive closures (factorial)
- Mutually recursive closures (even?/odd?)

**Try it:**
```bash
cargo run < tests/fixtures/examples/closures/06_nested_closures.scm
```

## Key Closure Features in Patina

### ✅ Lexical Scoping
Variables are resolved based on where the function was **defined**, not where it's called:
```scheme
(define x 10)
(define f (lambda () x))
(define g (lambda (x) (f)))
(g 20)  ; => 10 (not 20!)
```

### ✅ Variable Capture
Closures capture variables from their surrounding environment:
```scheme
(define make-adder
  (lambda (n)
    (lambda (x) (+ x n))))  ; 'n' is captured

(define add5 (make-adder 5))
(add5 3)  ; => 8
```

### ✅ Mutable State with set!
Captured variables can be mutated:
```scheme
(define make-counter
  (lambda ()
    (let ((count 0))
      (lambda ()
        (set! count (+ count 1))
        count))))
```

### ✅ Environment Sharing
Multiple closures from the same creation context share state:
```scheme
(let ((x 0))
  (define inc (lambda () (set! x (+ x 1)) x))
  (define dec (lambda () (set! x (- x 1)) x)))
(inc)  ; => 1
(dec)  ; => 0 (same x!)
```

### ✅ Independent Environments
Each invocation creates a new environment:
```scheme
(define c1 (make-counter))
(define c2 (make-counter))
(c1)  ; => 1
(c2)  ; => 1 (independent!)
```

## Testing Against chibi-scheme

All examples have been verified against chibi-scheme (the R7RS reference implementation):

```bash
# Test with Patina
cargo run < tests/fixtures/examples/closures/01_basic_closure.scm

# Compare with chibi-scheme
chibi-scheme < tests/fixtures/examples/closures/01_basic_closure.scm
```

Both should produce identical output.

## R7RS Compliance

These examples demonstrate full R7RS compliance for:
- Section 4.1.4 - Procedures (lambda)
- Section 4.2.2 - Binding constructs (let, let*, letrec)
- Section 4.1.6 - Assignments (set!)

All closure semantics match the R7RS specification exactly.

## Common Patterns

### Factory Pattern
```scheme
(define (make-thing arg)
  (lambda (msg)
    ;; operate on arg
    ))
```

### Object-Oriented Style
```scheme
(define (make-object x y)
  (lambda (method . args)
    (cond
      ((eq? method 'get-x) x)
      ((eq? method 'set-x!) (set! x (car args)))
      ...)))
```

### Partial Application
```scheme
(define (curry f)
  (lambda (x)
    (lambda (y)
      (f x y))))
```

## Performance Notes

Closures in Patina use `Rc<Environment>` for efficient memory sharing:
- Multiple closures share the same environment (O(1) cloning)
- Mutations are visible across all closures from the same context
- Automatic memory management via reference counting

## Further Reading

- R7RS Specification Section 4.1.4 (Procedures)
- [SICP Chapter 3.2](https://mitpress.mit.edu/sites/default/files/sicp/full-text/book/book-Z-H-21.html) - Environment Model
- Wikipedia: [Closure (computer programming)](https://en.wikipedia.org/wiki/Closure_(computer_programming))
