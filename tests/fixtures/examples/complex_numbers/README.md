# Complex Number Examples

This directory contains interesting programs that demonstrate complex number arithmetic in Patina Scheme. These examples go beyond basic arithmetic to explore mathematical properties and algorithms.

## Mathematical Properties Tested

### 1. **Polynomial Identities** (`complex_polynomials.scm`)
- **(z - w)(z + w) = z² - w²** - Difference of squares with complex numbers
- **(a + bi)(a - bi) = a² + b²** - Complex conjugate multiplication (always real!)
- Demonstrates that polynomial algebra works seamlessly with complex numbers

### 2. **Powers of i**
The imaginary unit cycles through four values:
- **i¹ = i**
- **i² = -1** (fundamental definition)
- **i³ = -i**
- **i⁴ = 1** (returns to identity)
- **i⁵ = i** (cycle repeats)

### 3. **Roots of Unity** (`complex_roots.scm`)
The n-th roots of unity are solutions to z^n = 1:
- **Square roots**: ±1
- **Cube roots**: 1, (-1+√3i)/2, (-1-√3i)/2
- **Property**: The sum of all n-th roots of unity equals 0
  - Example: 1 + e^(2πi/3) + e^(4πi/3) = 0

### 4. **De Moivre's Formula** (`complex_demoivre.scm`)
**(cos θ + i sin θ)^n = cos(nθ) + i sin(nθ)**
- Tested with special angles where exact values are known
- (1+i)² = 2i
- (1-i)² = -2i
- Demonstrates Euler's formula in action

### 5. **Complex Fibonacci** (`complex_fibonacci.scm`)
The Fibonacci sequence F(n) = F(n-1) + F(n-2) works with complex seeds!

**With imaginary seeds** (0, i):
- F(0) = 0
- F(1) = i
- F(2) = i
- F(3) = 2i
- F(5) = 5i
- Pattern: Pure imaginary Fibonacci numbers!

**With mixed seeds** (1+i, 1-i):
- F(2) = 2 (becomes real!)
- F(5) = 8-2i (complex result)

### 6. **Julia Set Iteration** (`complex_fractals.scm`)
The Julia set iteration: **z_{n+1} = z_n² + c**
- With c = 0, this is repeated squaring
- With complex c, creates fractal patterns
- Example: Starting at 2, iterating with c=0:
  - z₁ = 4
  - z₂ = 16
  - z₃ = 256 (exponential growth!)

### 7. **Mandelbrot Set Test** (`complex_mandelbrot.scm`)
Tests if a point is in the Mandelbrot set by checking if the orbit remains bounded.
- Origin (0+0i): In the set ✓
- 0.25+0i: In the set ✓
- 1+1i: Escapes (not in set) ✗

## Field Properties

Complex numbers form a **field** - all these properties hold:

### Commutativity
- a + b = b + a ✓
- a × b = b × a ✓

### Associativity
- (a + b) + c = a + (b + c) ✓
- (a × b) × c = a × (b × c) ✓

### Distributivity
- a × (b + c) = a×b + a×c ✓

### Identities
- a + 0 = a ✓
- a × 1 = a ✓
- a × 0 = 0 ✓

All these properties are tested in `complex_numbers.rs`!

## Why These Tests Matter

1. **Correctness**: They verify that complex arithmetic follows mathematical laws
2. **Edge Cases**: They catch issues with zero, pure imaginary numbers, and pure real results
3. **Exactness**: They test interaction with Scheme's exact/inexact number system
4. **Real-World Use**: Fractals, signal processing, and quantum computing all use these patterns
5. **Fun**: They're mathematically beautiful and make the tests enjoyable to write!

## Running the Examples

```bash
# Run a specific example
cargo run < tests/fixtures/examples/complex_fibonacci.scm

# Run all complex number tests
cargo test --test complex_numbers

# Run with output
cargo test --test complex_numbers -- --nocapture
```

## Future Extensions

These examples will become even more interesting when we add:
- `sqrt` - for computing complex square roots
- `magnitude` and `angle` - for working with polar form
- `real-part` and `imag-part` - for extracting components
- `make-rectangular` and `make-polar` - for construction
- `exp`, `log`, `sin`, `cos` - for transcendental functions

Then we could implement:
- Fast Fourier Transform (FFT)
- Mandelbrot set visualization
- Solving quadratic equations with complex roots
- Signal processing algorithms
