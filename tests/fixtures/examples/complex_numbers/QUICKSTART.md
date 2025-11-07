# Complex Numbers Quick Start

## Syntax

### Rectangular Notation (a + bi)
```scheme
3+4i           ; 3 + 4i
5-2i           ; 5 - 2i
-1+3i          ; -1 + 3i
-2-5i          ; -2 - 5i
```

### Pure Imaginary
```scheme
+i             ; 0 + 1i
-i             ; 0 - 1i
+5i            ; 0 + 5i
-3i            ; 0 - 3i
```

### Shorthand (imaginary part = ±1)
```scheme
3+i            ; 3 + 1i
7-i            ; 7 - 1i
```

### Polar Notation (r@θ)
```scheme
1@0            ; magnitude 1, angle 0 => 1+0i
3@1.5708       ; magnitude 3, angle π/2 => 0+3i
```

### With Rationals (exact complex)
```scheme
1/2+3/4i       ; exact: 1/2 + 3/4 i
3/5-2/3i       ; exact: 3/5 - 2/3 i
```

## Arithmetic

### Addition
```scheme
(+ 1+2i 3+4i)         ; => 4+6i
(+ 5 2+3i)            ; => 7+3i  (real + complex)
```

### Subtraction
```scheme
(- 5+7i 2+3i)         ; => 3+4i
(- 10+5i 10+5i)       ; => 0
```

### Multiplication
```scheme
(* 2+3i 1-1i)         ; => 5+i
(* +i +i)             ; => -1  (i² = -1)
(* 3+4i 3-4i)         ; => 25  (conjugate = real!)
```

### Negation
```scheme
(- 3+4i)              ; => -3-4i
(- +i)                ; => -i
```

## Key Properties

### Powers of i
```scheme
+i                    ; i¹ = i
(* +i +i)             ; i² = -1
(* +i +i +i)          ; i³ = -i
(* +i +i +i +i)       ; i⁴ = 1
```

### Complex Conjugate
```scheme
; (a+bi)(a-bi) = a² + b²  (always real!)
(* 3+4i 3-4i)         ; => 25  (9 + 16)
(* 5+12i 5-12i)       ; => 169 (25 + 144)
```

### Zero
```scheme
(+ 3+4i -3-4i)        ; => 0
(- 5+2i 5+2i)         ; => 0
(* 0 3+4i)            ; => 0
```

## Quick Examples

### Define and use variables
```scheme
(define z 3+4i)
(define w 1+2i)
(+ z w)               ; => 4+6i
(* z w)               ; => -5+10i
```

### Recursive functions
```scheme
(define (complex-power z n)
  (if (= n 0)
      1
      (* z (complex-power z (- n 1)))))

(complex-power 1+i 2)  ; => +2i
(complex-power +i 4)   ; => 1
```

### Iteration
```scheme
(define (iterate-square z n)
  (if (= n 0)
      z
      (iterate-square (* z z) (- n 1))))

(iterate-square 2+0i 3)  ; => 256  (2 → 4 → 16 → 256)
```

## Common Patterns

### Check if number is pure imaginary
```scheme
; If real part is 0
(define z 0+5i)
; In the future: (= (real-part z) 0)
```

### Check if result is real
```scheme
; If imaginary part is 0
(* 1+i 1-i)  ; => 2  (real result)
```

### Magnitude squared (for distance)
```scheme
; |z|² = z * conjugate(z)
(define z 3+4i)
(* z 3-4i)   ; => 25 = 3² + 4²
```

## Try These!

Copy and paste into the REPL:

```scheme
; Powers of i cycle through 4 values
(* +i +i)              ; -1
(* +i +i +i)           ; -i
(* +i +i +i +i)        ; 1

; Complex conjugate gives real result
(* 3+4i 3-4i)          ; 25

; Fibonacci with imaginary seeds
(define (fib n a b)
  (if (= n 0) a
      (if (= n 1) b
          (fib (- n 1) b (+ a b)))))

(fib 5 0+0i 0+1i)      ; +5i

; Julia set iteration
(define (julia z c n)
  (if (= n 0) z
      (julia (+ (* z z) c) c (- n 1))))

(julia 2+0i 0+0i 3)    ; 256
```

## Resources

- **Full examples**: See other `.scm` files in this directory
- **Mathematical theory**: See `README.md` for detailed explanations
- **Test cases**: See `tests/complex_numbers.rs` for comprehensive examples
