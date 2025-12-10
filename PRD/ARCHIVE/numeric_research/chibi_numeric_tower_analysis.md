# Chibi-Scheme Numeric Tower Implementation Analysis

## Executive Summary

Chibi-Scheme implements a sophisticated numeric tower following R7RS specification with support for:
- **Fixnums** (tagged immediate values - most efficient)
- **Bignums** (arbitrary precision integers)
- **Flonums** (IEEE 754 doubles)
- **Ratios** (exact rational numbers)
- **Complex** (complex numbers with real/imag parts)

Key design insight: Uses **tagged integers** for fixnums to avoid allocation while supporting unlimited precision through bignum promotion.

---

## 1. Core Numeric Type Representation

### Header Structure (include/chibi/sexp.h)

```c
// All Scheme values are pointers to tagged structures
typedef struct sexp_struct *sexp;

struct sexp_struct {
  sexp_tag_t tag;           // Type identifier
  char markedp;
  unsigned int immutablep:1;
  // ... other flags ...
  union {
    double flonum;                    // IEEE 754 double
    struct {
      signed char sign;
      sexp_uint_t length;            // Array length in words
    } bignum;
    struct {
      sexp numerator;                 // Exact integer
      sexp denominator;               // Exact positive integer
    } ratio;
    struct {
      sexp real;                      // Real part
      sexp imag;                      // Imaginary part
    } complex;
    // ... other types ...
  } value;
};

enum sexp_types {
  SEXP_FIXNUM,    // Tagged immediate (not allocated)
  SEXP_NUMBER,    // Supertype for dispatch
  SEXP_FLONUM,    // Allocated double
  SEXP_BIGNUM,    // Arbitrary precision integer
  SEXP_RATIO,     // Exact rational (optional)
  SEXP_COMPLEX,   // Complex numbers (optional)
  // ...
};
```

### Fixnum Representation (Clever Tagging)

```c
// Fixnums use low bit as a tag (bit pattern: ...x1)
#define SEXP_FIXNUM_BITS 1
#define SEXP_FIXNUM_TAG 1
#define SEXP_FIXNUM_MASK 1

#define sexp_fixnump(x)  (((sexp_uint_t)(x) & SEXP_FIXNUM_MASK) == SEXP_FIXNUM_TAG)

// Boxing/unboxing is simple bit manipulation
#define sexp_make_fixnum(x)  ((sexp)(((sexp_sint_t)(x)<<SEXP_FIXNUM_BITS) + SEXP_FIXNUM_TAG))
#define sexp_unbox_fixnum(x) (((sexp_sint_t)(x)) >> SEXP_FIXNUM_BITS)

// Example: fixnum 5
// Stored as: (5 << 1) | 1 = 0b01011 = 11 (in pointer representation)
// Unbox: 11 >> 1 = 5
```

**Key advantage**: No allocation for small integers, instant type checking.

### Bignum Structure

```c
// Bignum layout:
// [header: tag, length, sign] [data array...]

#define sexp_bignump(x)     (sexp_check_tag(x, SEXP_BIGNUM))
#define sexp_bignum_sign(x) (sexp_field(x, bignum, SEXP_BIGNUM, sign))
#define sexp_bignum_length(x) (sexp_field(x, bignum, SEXP_BIGNUM, length))
#define sexp_bignum_data(x) sexp_flexible_array_field(x, bignum, sexp_uint_t)

// Sign: -1, 0, or +1
struct {
  signed char sign;      // -1 = negative, 0 = zero, 1 = positive
  sexp_uint_t length;    // Number of sexp_uint_t words
} bignum;

// Data is stored as unsigned words in little-endian order
// Example: 123456789 might be stored as [0x75BCD15, 0x00000007, ...]
```

### Flonum Representation (Two Modes)

**Mode 1: Immediate Flonums (on 64-bit)**
```c
#if SEXP_USE_IMMEDIATE_FLONUMS
#define sexp_flonump(x) (((sexp_uint_t)(x) & SEXP_EXTENDED_MASK) == SEXP_IFLONUM_TAG)
// Encodes single-precision float (32-bit) in lower bits of pointer
// Trade-off: Single precision but no allocation
#else
// Mode 2: Allocated Flonums
#define sexp_flonump(x) (sexp_check_tag(x, SEXP_FLONUM))
#define sexp_flonum_value(f) ((f)->value.flonum)  // Full double precision
#endif
```

### Ratio Structure

```c
struct {
  sexp numerator;        // Exact integer (fixnum or bignum)
  sexp denominator;      // Exact positive integer
} ratio;

#define sexp_ratiop(x) (sexp_check_tag(x, SEXP_RATIO))
#define sexp_ratio_numerator(q) (sexp_pred_field(q, ratio, sexp_ratiop, numerator))
#define sexp_ratio_denominator(q) (sexp_pred_field(q, ratio, sexp_ratiop, denominator))

// Invariants maintained:
// - Denominator is always positive and > 1
// - Numerator and denominator are coprime (normalized)
```

### Complex Structure

```c
struct {
  sexp real;   // Can be: fixnum, bignum, ratio, or flonum
  sexp imag;   // Same types as real
} complex;

#define sexp_complexp(x) (sexp_check_tag(x, SEXP_COMPLEX))
#define sexp_complex_real(q) (sexp_pred_field(q, complex, sexp_complexp, real))
#define sexp_complex_imag(q) (sexp_pred_field(q, complex, sexp_complexp, imag))

// Optimization: If imaginary part is zero, represent as real number alone
```

---

## 2. Exactness Tracking

Chibi tracks exactness **implicitly through type**:

```c
// Type-based exactness predicates (no metadata needed!)
#define sexp_exact_integerp(x) (sexp_fixnump(x) || sexp_bignump(x))

#define sexp_exactp(x) (sexp_exact_integerp(x) || sexp_ratiop(x))

#define sexp_flonump(x) (...)  // Implies inexact

#define sexp_realp(x) (sexp_exact_integerp(x) || sexp_flonump(x) || sexp_ratiop(x))

#define sexp_numberp(x) (sexp_realp(x) || sexp_complexp(x))

// Exact vs Inexact:
// - Fixnum: exact
// - Bignum: exact
// - Ratio: exact
// - Flonum: inexact (always IEEE 754 double)
// - Complex: hybrid (type depends on real/imag parts)
```

**No flag is needed**: exactness is determined solely by the type! This is elegant.

---

## 3. Number Parsing with Exactness

From `sexp_read_number()` in sexp.c:2933:

```c
sexp sexp_read_number (sexp ctx, sexp in, int base, int exactp) {
  sexp_sint_t val = 0;
  int c, digit, negativep = 0, inexactp = 0;
  sexp_gc_var2(res, den);

  // Parse prefix markers: #e (exact), #i (inexact), #b, #o, #d, #x (radix)
  c = sexp_read_char(ctx, in);
  if (c == '#') {
    switch (sexp_tolower(sexp_read_char(ctx, in))) {
      case 'b': base = 2; break;
      case 'o': base = 8; break;
      case 'd': base = 10; break;
      case 'x': base = 16; break;
      case 'i': inexactp = 1; break;    // #i forces inexact
      case 'e': exactp = 1; break;      // #e forces exact
    }
  }

  // Parse digits into fixnum val
  for (; sexp_isxdigit(c); c=sexp_read_char(ctx, in)) {
    digit = digit_value(c);
    if (digit >= base) break;
    tmp = val * base + digit;
    
    // OVERFLOW DETECTION: Promote to bignum if needed
#if SEXP_USE_BIGNUMS
    if ((SEXP_MAX_FIXNUM / base < val) || (tmp > SEXP_MAX_FIXNUM)) {
      return sexp_read_bignum(ctx, in, val, (negativep ? -1 : 1), base);
    }
#endif
    val = tmp;
  }

  // Parse fractional/rational/complex suffixes
  if (exactp && is_precision_indicator(c)) {
    // Exponent notation: 1.5e10 -> parsed as exact number * 10^exponent
    den = sexp_read_number(ctx, in, base, 0);
    res = sexp_mul(ctx, res, sexp_expt(ctx, SEXP_TEN, den));
  }
  else if (c == '/') {
    // Rational: 3/5 (exact), creates ratio if SEXP_USE_RATIOS enabled
    den = sexp_read_number(ctx, in, base, exactp);
    res = sexp_make_ratio(ctx, res, den);
    res = sexp_ratio_normalize(ctx, res, in);
  }
  else if (c == '.' || is_precision_indicator(c)) {
    // Float: 3.14 -> becomes flonum (inexact)
    return sexp_read_float_tail(ctx, in, val, negativep);
  }

  // Return appropriate type based on input format
  return inexactp ? sexp_make_flonum(ctx, (double)val)
                  : sexp_make_fixnum(val);
}
```

**Key insights**:
1. Exactness is determined by *syntax*, not metadata
2. `#i3/5` forces flonum output
3. `#e3.0` parses as exact integer 3
4. Rational literals automatically create `ratio` type
5. Overflow during parsing immediately promotes to bignum

---

## 4. Arithmetic Operations

### Fixnum Fast Path (sexp.h:1537-1544)

```c
// Ultra-optimized fixnum arithmetic using bit manipulation
// These work because fixnums have low bit set to 1

#define sexp_fx_add(a, b) ((sexp)(((sexp_sint_t)a)+((sexp_sint_t)b)-SEXP_FIXNUM_TAG))
// Why: (a-1 + b-1) + 1 = a + b - 1 -> shift adjustment

#define sexp_fx_sub(a, b) ((sexp)(((sexp_sint_t)a)-((sexp_sint_t)b)+SEXP_FIXNUM_TAG))
// Why: (a-1) - (b-1) + 1 = a - b - 1 -> shift adjustment

#define sexp_fx_mul(a, b) ((sexp)((((sexp_sint_t)a)-SEXP_FIXNUM_TAG) \
                                   * (((sexp_sint_t)b)>>SEXP_FIXNUM_BITS)) \
                          + SEXP_FIXNUM_TAG))
// Why: ((a-1)/2) * ((b-1)/2) = (a*b - a - b + 1)/4

#define sexp_fx_div(a, b) (sexp_make_fixnum(sexp_unbox_fixnum(a) / sexp_unbox_fixnum(b)))
#define sexp_fx_rem(a, b) (sexp_make_fixnum(sexp_unbox_fixnum(a) % sexp_unbox_fixnum(b)))
#define sexp_fx_neg(a) (sexp_make_fixnum(-(sexp_unbox_fixnum(a))))
#define sexp_fx_abs(a) ((((sexp_sint_t)a) < 0) ? sexp_fx_neg(a) : a)
```

### Flonum Operations (sexp.h:1548-1551)

```c
#define sexp_fp_add(x,a,b) (sexp_make_flonum(x, sexp_flonum_value(a) + sexp_flonum_value(b)))
#define sexp_fp_sub(x,a,b) (sexp_make_flonum(x, sexp_flonum_value(a) - sexp_flonum_value(b)))
#define sexp_fp_mul(x,a,b) (sexp_make_flonum(x, sexp_flonum_value(a) * sexp_flonum_value(b)))
#define sexp_fp_div(x,a,b) (sexp_make_flonum(x, sexp_flonum_value(a) / sexp_flonum_value(b)))
```

### VM ADD Operation (vm.c:1763-1791)

**With bignums enabled:**
```c
case SEXP_OP_ADD:
  tmp1 = _ARG1, tmp2 = _ARG2;
  if (sexp_fixnump(tmp1) && sexp_fixnump(tmp2)) {
    j = sexp_unbox_fixnum(tmp1) + sexp_unbox_fixnum(tmp2);
    // Check bounds
    if ((j < SEXP_MIN_FIXNUM) || (j > SEXP_MAX_FIXNUM)) {
      // Overflow: promote first arg to bignum, then add
      _ARG1 = sexp_add(ctx, 
                       tmp1=sexp_fixnum_to_bignum(ctx, tmp1), 
                       tmp2);
    } else {
      _ARG1 = sexp_make_fixnum(j);
    }
  }
  else {
    // Mixed types: fixnum+flonum, bignum+ratio, etc.
    _ARG1 = sexp_add(ctx, tmp1, tmp2);
  }
  break;
```

**Without bignums (flonum fallback):**
```c
if (sexp_fixnump(tmp1) && sexp_fixnump(tmp2))
  _ARG1 = sexp_fx_add(tmp1, tmp2);
else if (sexp_flonump(tmp1) && sexp_flonump(tmp2))
  _ARG1 = sexp_fp_add(ctx, tmp1, tmp2);
else if (sexp_flonump(tmp1) && sexp_fixnump(tmp2))
  // Promote fixnum to flonum
  _ARG1 = sexp_make_flonum(ctx, sexp_flonum_value(tmp1) + (double)sexp_unbox_fixnum(tmp2));
else if (sexp_fixnump(tmp1) && sexp_flonump(tmp2))
  _ARG1 = sexp_make_flonum(ctx, (double)sexp_unbox_fixnum(tmp1) + sexp_flonum_value(tmp2));
else
  sexp_raise("+: not a number", sexp_list2(ctx, tmp1, tmp2));
```

### Division Operation (vm.c:1850-1897)

**Key semantic difference from typical implementations:**
```c
case SEXP_OP_DIV:  // `/` operator
  tmp1 = _ARG1, tmp2 = _ARG2;
  if (tmp2 == SEXP_ZERO) {
    sexp_raise("divide by zero", SEXP_NULL);
  }
  else if (sexp_fixnump(tmp1) && sexp_fixnump(tmp2)) {
#if SEXP_USE_RATIOS
    // Exact division: return ratio, automatically normalized
    _ARG1 = sexp_make_ratio(ctx, tmp1, tmp2);
    _ARG1 = sexp_ratio_normalize(ctx, _ARG1, SEXP_FALSE);
#else
    // Inexact fallback: convert to flonum
    _ARG1 = sexp_make_flonum(ctx, (double)sexp_unbox_fixnum(tmp1) / 
                                   (double)sexp_unbox_fixnum(tmp2));
#endif
  }
  break;

// quotient / remainder operations (floor division)
case SEXP_OP_QUOTIENT:  // quotient procedure
  if (sexp_fixnump(tmp1) && sexp_fixnump(tmp2)) {
    _ARG1 = sexp_fx_div(tmp1, tmp2);  // Integer division, truncation
  }
  break;
```

**Important semantic choices**:
- `/` returns ratio if ratios enabled, flonum otherwise
- `quotient` returns integer truncated toward zero
- `remainder` uses Euclidean division semantics

---

## 5. Type Predicates & Promotion

### Number Type Predicates

```c
// Exact integer: fixnum or bignum
#define sexp_exact_integerp(x) (sexp_fixnump(x) || sexp_bignump(x))

// Integer (exact or flonum that's integer-valued)
#define sexp_integerp(x) (sexp_exact_integerp(x) || \
                          (sexp_flonump(x) && sexp_flonum_value(x) == floor(sexp_flonum_value(x))))

// Exact number (integer, ratio, or complex with exact parts)
#define sexp_exactp(x) (sexp_exact_integerp(x) || sexp_ratiop(x))

// Real number
#define sexp_realp(x) (sexp_exact_integerp(x) || sexp_flonump(x) || sexp_ratiop(x))

// Any number
#define sexp_numberp(x) (sexp_realp(x) || sexp_complexp(x))

// Check for special float values
#define sexp_infp(x) (sexp_flonump(x) && isinf(sexp_flonum_value(x)))
#define sexp_nanp(x) (sexp_flonump(x) && isnan(sexp_flonum_value(x)))
```

### Sign/Polarity Predicates

```c
#define sexp_exact_negativep(x) (sexp_fixnump(x) ? (sexp_unbox_fixnum(x) < 0) \
                                 : (sexp_bignump(x) && sexp_bignum_sign(x) < 0))

#define sexp_exact_positivep(x) (sexp_fixnump(x) ? (sexp_unbox_fixnum(x) > 0) \
                                 : (sexp_bignump(x) && sexp_bignum_sign(x) > 0))

#define sexp_negativep(x) (sexp_exact_negativep(x) ||                   \
                           (sexp_flonump(x) && sexp_flonum_value(x) < 0))

// Special case: negative zero in IEEE 754
#define sexp_pedantic_negativep(x) (...similar but checks for -0.0...)

#define sexp_oddp(x) (sexp_fixnump(x) ? (sexp_unbox_fixnum(x) & 1) : \
                      (sexp_bignump(x) && (sexp_bignum_data(x)[0] & 1)))
```

### Exact/Inexact Conversion

From eval.c:1903-1932:

```c
sexp sexp_exact_to_inexact (sexp ctx, sexp self, sexp_sint_t n, sexp i) {
  sexp_gc_var1(res);
  res = i;
  if (sexp_fixnump(i))
    res = sexp_fixnum_to_flonum(ctx, i);      // Fixnum -> double
  else if (sexp_flonump(i))
    res = i;                                   // Already inexact
  else if (sexp_bignump(i))
    res = sexp_make_flonum(ctx, sexp_bignum_to_double(i));  // Bignum -> double
  else if (sexp_ratiop(i))
    res = sexp_make_flonum(ctx, sexp_ratio_to_double(ctx, i));  // Ratio -> double
  else if (sexp_complexp(i)) {
    // Complex: convert both parts recursively
    res = sexp_make_complex(ctx, SEXP_ZERO, SEXP_ZERO);
    sexp_complex_real(res) = sexp_exact_to_inexact(ctx, self, 1, sexp_complex_real(i));
    sexp_complex_imag(res) = sexp_exact_to_inexact(ctx, self, 1, sexp_complex_imag(i));
  }
  return res;
}

sexp sexp_inexact_to_exact (sexp ctx, sexp self, sexp_sint_t n, sexp z) {
  if (sexp_exactp(z))
    return z;                    // Already exact
  else if (sexp_flonump(z)) {
    if (isinf(sexp_flonum_value(z)) || isnan(sexp_flonum_value(z)))
      return error("exact: not a finite number", z);
    else if (sexp_flonum_value(z) != trunc(sexp_flonum_value(z))) {
#if SEXP_USE_RATIOS
      return sexp_double_to_ratio_2(ctx, sexp_flonum_value(z));  // Convert to ratio
#else
      return error("exact: not an integer", z);
#endif
    }
    else if (flonum > SEXP_MAX_FIXNUM || flonum < SEXP_MIN_FIXNUM) {
      return sexp_double_to_bignum(ctx, sexp_flonum_value(z));   // Flonum -> bignum
    }
    else {
      return sexp_make_fixnum((sexp_sint_t)sexp_flonum_value(z));  // Flonum -> fixnum
    }
  }
  // ... similar for complex ...
}
```

---

## 6. Special Value Handling

### IEEE 754 Special Values

```c
// Infinities
#define sexp_infp(x) (sexp_flonump(x) && isinf(sexp_flonum_value(x)))
// Accessible as: +inf.0, -inf.0

// Not-a-Number
#define sexp_nanp(x) (sexp_flonump(x) && isnan(sexp_flonum_value(x)))
// Accessible as: +nan.0

// They support all arithmetic and comparison operations
// Special rule: +nan.0 != +nan.0 (IEEE 754 semantics)
```

### Numeric Equality (Important!)

```c
// Standard equality for most numbers
sexp a = sexp_make_fixnum(3);
sexp b = sexp_make_fixnum(3);
sexp_eq(a, b);  // Returns SEXP_TRUE

// For flonums, two representations of same number are equal
sexp f1 = sexp_make_flonum(ctx, 3.14);
sexp f2 = sexp_make_flonum(ctx, 3.14);
sexp_eqv(f1, f2);  // Returns SEXP_TRUE (value equality)

// IEEE special value handling
#if SEXP_USE_IEEE_EQV
#define sexp_flonum_eqv(x, y) (memcmp(sexp_flonum_bits(x), sexp_flonum_bits(y), sizeof(double)) == 0)
// Compares bit patterns, so -0.0 != +0.0 and NaN equality works
#else
#define sexp_flonum_eqv(x, y) (sexp_flonum_value(x) == sexp_flonum_value(y))
// Standard IEEE comparison, so -0.0 == +0.0 and NaN != NaN
#endif
```

---

## 7. Design Patterns & Clever Tricks

### 1. **Immediate Tagging for Fixnums**
- No allocation needed for small integers
- Type check is single bitwise AND
- Arithmetic uses clever bit manipulation
- Overflow detection + automatic promotion

### 2. **Type-Based Exactness**
- No need for exactness flags
- Determined solely by variant type
- Simpler reasoning about behavior
- Natural fit with union-based representation

### 3. **Lazy Normalization for Ratios**
```c
// After creating a ratio, normalize it
res = sexp_make_ratio(ctx, 6, 8);
res = sexp_ratio_normalize(ctx, res, SEXP_FALSE);
// Result: 3/4 (gcd-reduced, sign normalized)
```

### 4. **Overflow Promotion Pipeline**
```
fixnum overflow -> bignum (same operation, different type)
bignum -> ratio (if division with SEXP_USE_RATIOS)
ratio -> flonum (if inexact forced or no ratio support)
```

### 5. **Complex Number Optimization**
```c
// Special case: if imaginary part is zero, return real number
if (sexp_complex_imag(res) == SEXP_ZERO)
  res = sexp_complex_real(res);  // Simplify 3+0i to 3
```

### 6. **Recursive Conversion for Complex**
```c
// exact->inexact for complex: convert both parts
sexp_complex_real(res) = sexp_exact_to_inexact(ctx, ..., sexp_complex_real(i));
sexp_complex_imag(res) = sexp_exact_to_inexact(ctx, ..., sexp_complex_imag(i));
```

---

## 8. Numeric Operations Summary

| Operation | Type | Implementation | Notes |
|-----------|------|------------------|-------|
| `+`, `-`, `*` | fixnum+fixnum | `sexp_fx_*` (bit manipulation) | Overflow -> promote to bignum |
| `/` | fixnum+fixnum | `sexp_make_ratio` (if ratio enabled) | Otherwise -> flonum |
| `/` | flonum+flonum | `sexp_fp_div` (IEEE 754) | Returns flonum |
| `quotient` | any integer | Floor division, truncation toward zero | |
| `remainder` | any integer | Euclidean semantics | |
| `modulo` | any integer | Modulo semantics | |
| `exact->inexact` | any exact | Recursive conversion through types | Special handling for complex |
| `inexact->exact` | flonum | Double to ratio (if supported) | Error if non-integer |
| Number? | any | Type check predicate | Checks all numeric types |
| Integer? | any | Type check + flonum value check | True for 3.0 |
| Exact? | any | Type check (no flags) | Based on type alone |

---

## 9. Key Takeaways for Patina Implementation

### 1. **Tagged Integer Design is Excellent**
- Consider using similar low-bit tagging for fixnums in Rust
- Allows zero-allocation for common case
- Type checking is O(1)

### 2. **Exactness Tracking Without Flags**
- Don't add exactness metadata
- Let type implicitly determine exactness
- Simpler mental model + less state to track

### 3. **Ratio + Flonum is Good Split**
- Ratios preserve exactness for rational arithmetic
- Flonums for transcendental functions
- Natural type promotion path

### 4. **Overflow Handling Strategy**
- Check bounds EARLY during operations
- Promote transparently to bignum
- User never sees the transition

### 5. **Complex Numbers = Pairs of Reals**
- Keep them separate: `(real, imag)`
- Allows mixed-type components
- Optimize away zero imaginary parts

### 6. **Consider Configuration Flags**
Chibi has `SEXP_USE_RATIOS`, `SEXP_USE_COMPLEX`, `SEXP_USE_BIGNUMS`:
- Allows minimal builds without ratio support
- Graceful degradation to flonum
- Good for embedded/restricted environments

---

## 10. Comparison to Patina's Current Implementation

### Current Patina (from CLAUDE.md)

Numeric tower:
```rust
pub enum Value {
    Integer,
    BigInteger,
    Rational,
    Real,
    Complex,
    // ...
}
```

**Issues vs Chibi's approach**:
1. All values allocated (no tagging)
2. No immediate type checking optimization
3. Exactness tracking unclear - may need metadata
4. Complex nested type handling

### Recommended Improvements

1. **Add Fixnum Tagging** (optional but valuable)
   - Reserve low bits for fixnum tag
   - Zero-allocation common case
   - But adds complexity in Rust

2. **Make Exactness Type-Implicit**
   - `Integer` and `Rational` = exact
   - `Real` and `Complex` = possibly exact
   - `Flonum` = always inexact
   - No separate `exact` flag needed

3. **Use Ratio Normalization**
   - Store gcd-reduced ratios only
   - Normalize on creation
   - Simplify equality checking

4. **IEEE 754 Compliance**
   - Support +inf.0, -inf.0, +nan.0
   - Handle negative zero correctly
   - Propagate NaN through operations

5. **Overflow Strategy**
   - Fast fixnum path
   - Automatic promotion to bignum
   - User-transparent behavior

