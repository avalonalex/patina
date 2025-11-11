# Patina Scheme Examples

This directory contains example Scheme programs that demonstrate Patina's features. These examples serve both as documentation and as integration tests.

## Directory Structure

### 📊 `arithmetic/`
Basic arithmetic operations and comparisons.
- Addition, subtraction, multiplication, division
- Numeric comparisons (<, >, =, <=, >=)
- Integer and floating-point operations

### 🔢 `complex_numbers/` ⭐
**Advanced complex number demonstrations** - see [complex_numbers/README.md](complex_numbers/README.md)

Interesting mathematical programs including:
- **Complex Fibonacci sequences** with imaginary seeds
- **Julia set iteration** (fractal mathematics)
- **Mandelbrot set membership** testing
- **Roots of unity** calculations
- **De Moivre's formula** demonstrations
- Polynomial identities with complex numbers

These examples showcase non-trivial algorithms and mathematical properties.

### 🔒 `closures/` ⭐
**Comprehensive closure examples** - see [closures/README.md](closures/README.md)

Complete demonstrations of lexical closures and advanced functional patterns:
- **Basic closures** - Variable capture and lexical scoping
- **Stateful closures** - Mutable state with set! (counters, bank accounts)
- **Higher-order functions** - Composition, currying, partial application
- **Multiple captures** - Objects with properties, nested scopes
- **Practical examples** - Memoization, timers, stack implementation
- **Nested closures** - Deep nesting, recursive closures, mutual recursion

These examples demonstrate R7RS-compliant closure semantics with full environment sharing.

### 🎛️ `control/`
Control flow constructs.
- `if` conditionals
- `begin` sequential evaluation
- Boolean logic

### 📝 `lists/`
List operations and pair manipulation.
- List construction and traversal
- `car`, `cdr`, `cons` operations
- List predicates

### 🔧 `simple/`
Simple, self-contained examples.
- Basic arithmetic expressions
- Lambda expressions and closures

### 🧪 `harness/`
Test harness utilities and helper functions.

## Running Examples

### Run a single example:
```bash
cargo run < tests/fixtures/examples/arithmetic/basic.scm
```

### Run complex number examples:
```bash
# Complex Fibonacci with imaginary seeds
cargo run < tests/fixtures/examples/complex_numbers/complex_fibonacci.scm

# Julia set iteration
cargo run < tests/fixtures/examples/complex_numbers/complex_fractals.scm

# Roots of unity
cargo run < tests/fixtures/examples/complex_numbers/complex_roots.scm
```

### Run all integration tests:
```bash
cargo test --test integration
```

## Example Highlights

### Complex Fibonacci (imaginary seeds)
```scheme
(define (complex-fib n a b)
  (if (= n 0) a
      (if (= n 1) b
          (complex-fib (- n 1) b (+ a b)))))

(complex-fib 5 0+0i 0+1i)  ; => +5i (pure imaginary result!)
```

### Julia Set Iteration
```scheme
(define (julia-iterate z c iterations)
  (if (= iterations 0)
      z
      (julia-iterate (+ (* z z) c) c (- iterations 1))))

(julia-iterate 2+0i 0+0i 3)  ; => 256 (exponential growth)
```

### Polynomial Identity
```scheme
; (z - w)(z + w) = z² - w²
(define z 3+4i)
(define w 1+2i)
(* (- z w) (+ z w))  ; left side
(- (* z z) (* w w))  ; right side (same result!)
```

## Adding New Examples

When adding new examples:
1. Create a subdirectory for related examples
2. Add a README.md explaining what the examples demonstrate
3. Include `.expected` files if you want to verify exact output
4. Consider adding corresponding tests in `tests/`

## Testing Philosophy

These examples serve dual purposes:
1. **Documentation**: Show users how to use Patina features
2. **Integration Testing**: Verify that features work in realistic scenarios

The complex number examples especially demonstrate this - they're not just "does 3+4i parse?" but rather "can we implement interesting algorithms with complex numbers?"
