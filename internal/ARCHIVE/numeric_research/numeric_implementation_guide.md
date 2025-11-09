# Quick Reference: Chibi Numeric Tower Design Patterns

## File Locations in Chibi Source

| Component | Location | Lines |
|-----------|----------|-------|
| **Type definitions** | `include/chibi/sexp.h` | 438-620 (union definition) |
| **Fixnum macros** | `include/chibi/sexp.h` | 762, 1537-1544 |
| **Bignum macros** | `include/chibi/sexp.h` | 1531-1533 |
| **Ratio accessors** | `include/chibi/sexp.h` | 1269-1270 |
| **Complex accessors** | `include/chibi/sexp.h` | 1272-1273 |
| **Type predicates** | `include/chibi/sexp.h` | 1007-1020, 1044-1120 |
| **Number parsing** | `sexp.c` | 2933-3105 |
| **VM arithmetic** | `vm.c` | 1763-1950+ |
| **Exact/Inexact conversion** | `eval.c` | 1903-1978 |

## Critical Code Sections

### 1. Fixnum Type Checking (1 bitwise AND)
```c
#define sexp_fixnump(x) (((sexp_uint_t)(x) & SEXP_FIXNUM_MASK) == SEXP_FIXNUM_TAG)
// Result: O(1) type check
```

### 2. Overflow Detection in Addition
```c
j = sexp_unbox_fixnum(tmp1) + sexp_unbox_fixnum(tmp2);
if ((j < SEXP_MIN_FIXNUM) || (j > SEXP_MAX_FIXNUM))
  // Promote to bignum and retry
```

### 3. Fixnum to Flonum Conversion
```c
#define sexp_fixnum_to_flonum(ctx, x) (sexp_make_flonum(ctx, sexp_unbox_fixnum(x)))
// Loss of precision possible; user must be aware
```

### 4. Ratio Normalization (Must Happen)
```c
res = sexp_make_ratio(ctx, res, den);
res = sexp_ratio_normalize(ctx, res, in);
// Reduces to lowest terms, normalizes sign
```

### 5. Complex Simplification
```c
if (sexp_complex_imag(res) == SEXP_ZERO)
  res = sexp_complex_real(res);  // Return real if imag is zero
```

## Exactness Rules

### Implicit (No metadata needed)
- `Fixnum` = exact
- `Bignum` = exact
- `Ratio` = exact
- `Flonum` = **always** inexact
- `Complex` = depends on components

### Predicates
```c
sexp_exactp(x)   // x is fixnum, bignum, or ratio
sexp_inexactp(x) // NOT (sexp_exactp(x)) OR has flonum part
sexp_realp(x)    // fixnum, bignum, ratio, or flonum
sexp_numberp(x)  // any numeric type
```

### Syntax
- `3` = exact integer (fixnum)
- `3.0` = inexact real (flonum)
- `#e3.0` = exact (converts to integer 3)
- `#i3/5` = inexact (converts to flonum)
- `3/5` = exact (ratio, if enabled)

## Promotion Pipeline

```
User input
    |
    v
Parse with type markers (#e, #i)
    |
    +---> Small integer? --> Fixnum
    |
    +---> Too large?     --> Bignum
    |
    +---> Rational syntax? --> Ratio (if enabled)
    |                     --> Flonum (if not enabled)
    |
    +---> Decimal point?  --> Flonum
    |
    +---> Inexact marker? --> Always Flonum
```

## Arithmetic Overflow Path

```
Fixnum + Fixnum
    |
    +---> Fits in bounds? --> Return Fixnum
    |
    +---> Overflow?       --> Promote to Bignum
                          --> Use Bignum arithmetic
                          --> Return Bignum
```

## Division Semantics

| Operation | Input | Output | Notes |
|-----------|-------|--------|-------|
| `/` | int / int | Ratio (if enabled) | Exact division |
| `/` | int / int | Flonum (no ratios) | Inexact fallback |
| `/` | float / float | Flonum | IEEE 754 |
| `quotient` | int / int | Integer | Floor division, truncate toward zero |
| `remainder` | int / int | Integer | Euclidean semantics |
| `modulo` | int / int | Integer | Modulo semantics |

## IEEE 754 Special Values

```
+inf.0   = positive infinity (flonum)
-inf.0   = negative infinity (flonum)
+nan.0   = not-a-number (flonum)
-0.0     = negative zero (special: 1.0/(-0.0) = -inf.0)
```

Predicates:
```c
sexp_infp(x)  // isinf() check
sexp_nanp(x)  // isnan() check
```

## Complex Number Handling

### Storage
```c
struct {
  sexp real;  // Can be: fixnum, bignum, ratio, flonum
  sexp imag;  // Same types as real
} complex;
```

### Optimization
```c
// If creating complex with zero imaginary part:
if (sexp_complex_imag(res) == SEXP_ZERO)
  return sexp_complex_real(res);  // Just return real number
```

### Conversion Rules
```c
// exact->inexact: convert both parts
sexp_complex_real(res) = sexp_exact_to_inexact(ctx, ..., sexp_complex_real(i));
sexp_complex_imag(res) = sexp_exact_to_inexact(ctx, ..., sexp_complex_imag(i));

// inexact->exact: convert both parts, error if either non-integer
// Complex simplification: if imag == 0, return real part
```

## Implementation Checklist

For Patina numeric tower:

- [ ] **Type predicates** - Implement all type checks (exactness implicit)
- [ ] **Fixnum operations** - Addition, subtraction, multiplication
- [ ] **Overflow detection** - Check bounds before assignment
- [ ] **Bignum promotion** - Transparent upgrade on overflow
- [ ] **Number parsing** - Handle #e, #i, /, radix prefixes
- [ ] **Ratio support** - GCD normalization on creation
- [ ] **Flonum operations** - IEEE 754 compliant
- [ ] **Mixed-type arithmetic** - Promotion rules
- [ ] **Complex numbers** - Optimization for zero imaginary
- [ ] **Exact/inexact conversion** - Bidirectional with appropriate checks
- [ ] **Division semantics** - Ratio vs quotient vs modulo
- [ ] **Special values** - +inf, -inf, +nan, -0.0

## Performance Insights

### Fast Path (No allocation)
- Fixnum arithmetic: direct bit manipulation
- Type checking: single bitwise AND
- Argument passing: bare pointers

### Slow Path (Requires allocation)
- Bignum arithmetic: word-by-word operations
- Ratio creation: GCD computation
- Flonum boxing: heap allocation

### Optimization Opportunities
1. Cache GCD results for ratios
2. Pre-compute common constants (0, 1, -1, 2^k, etc.)
3. Inline VM operations
4. Specialize for common argument patterns

