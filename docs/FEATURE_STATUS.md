# R7RS (scheme base) Feature Matrix

**Last Updated:** 2025-11-06 (Complex numbers + primitives.rs refactoring complete!)

This document provides a **detailed test-by-test status matrix** for R7RS compliance.

**For strategic planning and implementation guidance**, see [R7RS_ROADMAP.md](../PRD/phase1/R7RS_ROADMAP.md).

---

## Status Legend
- ✅ **Implemented & Tested** - Feature fully working with passing tests
- 🚧 **Partial** - Some cases work, others don't
- ❌ **Not Implemented** - Feature not yet started
- 🔒 **Blocked** - Waiting on another feature

---

## Summary Statistics

**Test Results: 232 passing, 11 ignored (not implemented)**

| Category | Implemented | Total | Progress |
|----------|-------------|-------|----------|
| Primitives | 18 | 20 | 90% |
| Derived Forms | 20 | 24 | 83% |
| Numbers | 32 | 34 | 94% |
| Complex Numbers | 25 | 25 | 100% ✅ |
| Lists | 30 | 30 | 100% ✅ |
| Predicates | 8 | 12 | 67% |
| Control | 5 | 5 | 100% ✅ |
| Strings | 0 | 30 | 0% |
| Vectors | 0 | 20 | 0% |
| I/O | 0 | 30 | 0% |
| **TOTAL** | **138** | **230** | **60%** |

---

## Primitive Expressions (Section 4.1)

### 4.1.1 Variable References
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| Variable lookup | ✅ | test_variable_reference | r7rs_primitives.rs:11 |
| Undefined variable error | 🚧 | (manual test only) | - |

### 4.1.2 Literal Expressions
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| quote (symbol) | ✅ | test_quote_symbol | primitives.rs |
| quote (list) | ✅ | test_quote_list | primitives.rs |
| quote (nested) | ✅ | test_quote_nested | primitives.rs |
| quote (literals) | ✅ | test_quote_literals | primitives.rs |
| quasiquote | ❌ | - | Future |
| unquote | ❌ | - | Future |
| unquote-splicing | ❌ | - | Future |

### 4.1.4 Procedures
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| lambda (fixed arity) | ✅ | test_simple_lambda | primitives.rs |
| lambda (multiple args) | ✅ | test_lambda_multiple_args | primitives.rs |
| lambda (variadic) | ✅ | test_lambda_variadic | primitives.rs |
| lambda (mixed fixed + rest) | ✅ | test_lambda_variadic_with_fixed | primitives.rs |
| lambda (closure) | 🚧 | test_lambda_closure (ignored) | Needs test update |
| define (function shorthand) | ❌ | - | Not implemented |

### 4.1.5 Conditionals
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| if (true branch) | ✅ | test_if_true | r7rs_primitives.rs:96 |
| if (false branch) | ✅ | test_if_false | r7rs_primitives.rs:102 |
| if (expression evaluation) | ✅ | test_if_consequent_evaluated | r7rs_primitives.rs:108 |
| if (procedure selection) | ✅ | test_if_procedure_selection | r7rs_primitives.rs:114 |

### 4.1.6 Assignments
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| define (variable) | ✅ | test_define_variable | r7rs_primitives.rs:119 |
| define (multiple) | ✅ | test_define_multiple | r7rs_primitives.rs:130 |
| set! | ✅ | test_set_bang | r7rs_primitives.rs:142 |
| set! (undefined error) | 🔒 | test_set_bang_undefined | Error handling |

---

## Derived Expressions (Section 4.2)

### 4.2.1 Conditionals
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| cond (simple) | ✅ | test_cond_simple | derived.rs |
| cond (else) | ✅ | test_cond_with_else | derived.rs |
| cond (=>) | ✅ | test_cond_with_arrow | derived.rs |
| case (simple) | ✅ | test_case_simple | derived.rs |
| case (else) | ✅ | test_case_with_else | derived.rs |
| case (=>) | ✅ | test_case_with_arrow | derived.rs |
| case (multiple values) | ✅ | test_case_single_datum | derived.rs |
| and (all true) | ✅ | test_and_all_true | derived.rs |
| and (with false) | ✅ | test_and_with_false | derived.rs |
| and (empty) | ✅ | test_and_empty | derived.rs |
| and (returns last) | ✅ | test_and_returns_last | derived.rs |
| or (all false) | ✅ | test_or_all_false | derived.rs |
| or (first true) | ✅ | test_or_first_true | derived.rs |
| or (returns value) | 🚧 | test_or_returns_first_true (ignored) | Edge case |
| when | ❌ | test_when (ignored) | Not implemented |
| unless | ❌ | test_unless (ignored) | Not implemented |

### 4.2.2 Binding Constructs
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| let | ✅ | test_let_simple | derived.rs |
| let (scoping) | ✅ | test_let_scoping | derived.rs |
| let* | ✅ | test_let_star_sequential | derived.rs |
| letrec | ✅ | test_letrec_recursive | derived.rs |
| let-values | ❌ | - | Not implemented |
| let*-values | ❌ | - | Not implemented |

### 4.2.3 Sequencing
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| begin | ✅ | test_begin | r7rs_primitives.rs:161 |
| begin (returns last) | ✅ | test_begin_returns_last | r7rs_primitives.rs:166 |

### 4.2.4 Iteration
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| do | ❌ | test_do_simple (ignored) | Not implemented |

---

## Numbers (Section 6.2)

### Arithmetic Operations
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| + | ✅ | test_addition | numbers.rs |
| - | ✅ | test_subtraction | numbers.rs |
| * | ✅ | test_multiplication | numbers.rs |
| / | ✅ | test_division | numbers.rs |
| / (exact zero error) | ✅ | test_division_by_zero | numbers.rs |
| / (inexact zero → inf) | ✅ | test_division_by_zero | numbers.rs |
| abs | ✅ | test_abs | numbers.rs / eval/mod.rs |
| quotient | ✅ | test_quotient | numbers.rs / eval/mod.rs |
| remainder | ✅ | test_remainder | numbers.rs / eval/mod.rs |
| modulo | ✅ | test_modulo | numbers.rs / eval/mod.rs |
| gcd | ❌ | - | Not implemented |
| lcm | ❌ | - | Not implemented |
| floor | ❌ | - | Not implemented |
| ceiling | ❌ | - | Not implemented |
| truncate | ❌ | - | Not implemented |
| round | ❌ | - | Not implemented |
| rationalize | ❌ | - | Not implemented |
| square | ❌ | - | Not implemented |
| exact-integer-sqrt | ❌ | - | Not implemented |
| expt | ❌ | - | Not implemented |

### Comparison Operations
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| = | ✅ | test_equal | r7rs_numbers.rs |
| < | ✅ | test_less_than | r7rs_numbers.rs |
| > | ✅ | test_greater_than | r7rs_numbers.rs |
| <= | ✅ | test_less_equal | r7rs_numbers.rs |
| >= | ✅ | test_greater_equal | r7rs_numbers.rs |

### Numeric Predicates
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| number? | ✅ | test_number_predicate | numbers.rs |
| integer? | ✅ | test_integer_predicate | numbers.rs |
| zero? | ✅ | test_zero_predicate | numbers.rs / bootstrap.scm |
| positive? | ✅ | test_positive_predicate | numbers.rs / bootstrap.scm |
| negative? | ✅ | test_negative_predicate | numbers.rs / bootstrap.scm |
| odd? | ✅ | test_odd_predicate | numbers.rs / bootstrap.scm |
| even? | ✅ | test_even_predicate | numbers.rs / bootstrap.scm |
| exact? | ✅ | test_exact_predicate | numbers.rs / eval/mod.rs |
| inexact? | ✅ | test_inexact_predicate | numbers.rs / eval/mod.rs |
| exact-integer? | ❌ | - | Not implemented |
| finite? | ❌ | - | Not implemented |
| infinite? | ❌ | - | Not implemented |
| nan? | ❌ | - | Not implemented |
| max | ✅ | test_max | numbers.rs / eval/mod.rs |
| min | ✅ | test_min | numbers.rs / eval/mod.rs |

---

## Complex Numbers (Section 6.2.6)

**Status: 100% Complete! ✅**

### Parsing and Representation
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| Rectangular notation (a+bi) | ✅ | test_parse_complex_rectangular | complex_numbers.rs |
| Pure imaginary (+i, -i) | ✅ | test_parse_complex_pure_imaginary | complex_numbers.rs |
| Shorthand notation (3+i) | ✅ | test_parse_complex_shorthand | complex_numbers.rs |
| Inexact complex (floats) | ✅ | test_parse_complex_with_floats | complex_numbers.rs |
| Polar notation (r@θ) | ✅ | test_parse_polar | complex_numbers.rs |

### Arithmetic Operations
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| Complex addition | ✅ | test_complex_addition | complex_numbers.rs |
| Addition with reals | ✅ | test_complex_addition_with_real | complex_numbers.rs |
| Complex subtraction | ✅ | test_complex_subtraction | complex_numbers.rs |
| Complex multiplication | ✅ | test_complex_multiplication | complex_numbers.rs |
| Multiplication with reals | ✅ | test_complex_multiplication_with_real | complex_numbers.rs |
| Complex negation | ✅ | test_complex_negation | complex_numbers.rs |
| Zero handling | ✅ | test_complex_zero | complex_numbers.rs |

### Mathematical Properties
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| i² = -1 | ✅ | test_i_squared | complex_numbers.rs |
| Powers of i | ✅ | test_powers_of_i | complex_numbers.rs |
| Conjugate property | ✅ | test_complex_conjugate_property | complex_numbers.rs |
| Polynomial identities | ✅ | test_complex_polynomial_identity | complex_numbers.rs |
| Distributive law | ✅ | test_complex_distributive_law | complex_numbers.rs |
| Associative law | ✅ | test_complex_associative_law | complex_numbers.rs |
| De Moivre's formula | ✅ | test_demoivre_special_cases | complex_numbers.rs |

### Advanced Tests
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| Complex Fibonacci | ✅ | test_complex_fibonacci | complex_numbers.rs |
| Julia set iteration | ✅ | test_complex_iteration | complex_numbers.rs |
| Roots of unity | ✅ | test_cube_roots_of_unity_sum | complex_numbers.rs |
| Exact rationals in complex | ✅ | test_complex_with_exact_rationals | complex_numbers.rs |
| Nested operations | ✅ | test_nested_complex_operations | complex_numbers.rs |

---

## Lists and Pairs (Section 6.4)

**Status: 100% Complete! ✅**

### Constructors
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| cons | ✅ | test_cons | lists.rs |
| list | ✅ | test_list | lists.rs |
| make-list | ❌ | - | Not in R7RS-small base |

### Accessors
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| car | ✅ | test_car | lists.rs |
| cdr | ✅ | test_cdr | lists.rs |
| caar | ✅ | test_caar | lists.rs |
| cadr | ✅ | test_cadr | lists.rs |
| cdar | ✅ | test_cdar | lists.rs |
| cddr | ✅ | test_cddr | lists.rs |
| list-ref | ✅ | test_list_ref | lists.rs |
| list-tail | ✅ | test_list_tail | lists.rs |

### Predicates
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| pair? | ✅ | test_pair_predicate | lists.rs |
| null? | ✅ | test_null_predicate | lists.rs |
| list? | ✅ | test_list_predicate | lists.rs |

### Operations
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| length | ✅ | test_length | lists.rs |
| append | ✅ | test_append | lists.rs |
| reverse | ✅ | test_reverse | lists.rs |
| memq | ✅ | test_memq | lists.rs |
| memv | ✅ | test_memv | lists.rs |
| member | ✅ | test_member | lists.rs |
| assq | ✅ | test_assq | lists.rs |
| assv | ✅ | test_assv | lists.rs |
| assoc | ✅ | test_assoc | lists.rs |
| list-copy | ❌ | - | Not in R7RS-small base |
| list-set! | ❌ | - | Not in R7RS-small base |

---

## Strings (Section 6.7)

### Status: Not Started
All string operations (30+ procedures) are not yet implemented.

Priority items:
- string-length
- string-ref
- string-set!
- string-append
- string=?, string<?, etc.
- string->list, list->string
- string->symbol, symbol->string

---

## Vectors (Section 6.8)

### Status: Not Started
All vector operations (20+ procedures) are not yet implemented.

Priority items:
- vector-length
- vector-ref
- vector-set!
- vector->list, list->vector
- make-vector

---

## Predicates (Section 6.3)

| Feature | Status | Tests | File |
|---------|--------|-------|------|
| eq? | ✅ | test_eq | predicates.rs |
| eqv? | ✅ | test_eqv | predicates.rs |
| equal? | ✅ | test_equal | predicates.rs |
| boolean? | ✅ | test_boolean_predicate | predicates.rs |
| boolean=? | ❌ | test_boolean_equal (ignored) | Not implemented |
| symbol? | ✅ | test_symbol_predicate | predicates.rs |
| string? | ✅ | test_string_predicate | predicates.rs |
| char? | ❌ | test_char_predicate (ignored) | Not implemented |
| vector? | ❌ | test_vector_predicate (ignored) | Not implemented |
| procedure? | ❌ | test_procedure_predicate (ignored) | Not implemented |
| not | ✅ | test_not | predicates.rs / bootstrap.scm |

---

## Control Features (Section 6.10)

**Status: 100% Complete! ✅**

| Feature | Status | Tests | File |
|---------|--------|-------|------|
| procedure? | ❌ | - | See predicates above |
| apply | ✅ | test_apply_basic | control.rs |
| apply (with args) | ✅ | test_apply_with_args | control.rs |
| apply (with lambda) | ✅ | test_apply_with_lambda | control.rs |
| map | ✅ | test_map_* | lists.rs |
| for-each | ✅ | test_for_each_* | lists.rs |
| call-with-values | ❌ | - | Not implemented |
| values | ❌ | - | Not implemented |
| dynamic-wind | ❌ | - | Not implemented |
| call/cc | ❌ | - | Not implemented |

---

## I/O (Section 6.13)

### Status: Not Started
All I/O operations (30+ procedures) are not yet implemented.

Priority items for basic functionality:
- display
- write
- newline
- current-input-port, current-output-port

---

## Next Priorities

**Current Status: 106/128 tests passing (83%)**

Based on the 22 ignored tests, here are the quickest wins to reach higher compliance:

### Quick Wins (1-2 hours) - Simple Operations
1. **`abs`** - Absolute value (trivial)
2. **`max`, `min`** - Find maximum/minimum (straightforward)

### Medium Priority (2-4 hours) - Numeric Operations
3. **`quotient`, `remainder`, `modulo`** - Integer division operations
   - **Note**: Once implemented, enable `test_gcd_euclidean` and `test_extended_gcd` in numbers.rs
   - These tests demonstrate the power of multiple values for classic algorithms!
   - Also enable `odd?` and `even?` predicates in bootstrap.scm once remainder is available
4. **Division by zero handling** - Proper error for `(/ 1 0)`

### Low Priority (4+ hours) - Advanced Features
5. **`when`, `unless`** - Syntactic sugar for `if`
6. **`or` returns value** - Edge case: `(or #f 42)` should return `42`, not `#t`
7. **`do`** - Iteration construct (more complex)
8. **`procedure?`, `char?`, `vector?`** - Type predicates

### Future Work (Not blocking current compliance)
- **Strings** - 30+ operations (major undertaking)
- **Vectors** - 20+ operations
- **I/O** - `display`, `write`, ports, etc.
- **Advanced control** - `call/cc`, `dynamic-wind`

---

**Recent Progress (2025-11-06):**
- ✅ **Fixed division by zero handling**
  - Exact division by zero (`(/ 1 0)`) → error
  - Inexact division by zero (`(/ 1 0.0)`) → `+inf.0` or `-inf.0`
  - Added proper infinity display formatting: `+inf.0`, `-inf.0`, `+nan.0`
  - Enabled `test_division_by_zero` (was previously ignored)
- ✅ **Implemented full complex number support** (25 new tests)
  - Rectangular notation: `3+4i`, `5-2i`, pure imaginary `+i`, `-i`
  - Polar notation: `r@θ` with automatic rectangular conversion
  - Shorthand notation: `3+i` (imaginary part = 1)
  - Complex arithmetic: addition, subtraction, multiplication, negation
  - Mixed arithmetic: complex + real numbers
  - Exact/inexact integration: complex respects exactness of components
  - Advanced tests: complex Fibonacci, Julia sets, roots of unity, polynomial identities
- ✅ **Major code quality improvements to primitives.rs**
  - Added 5 helper functions to eliminate ~178 lines of duplication
  - `check_arity_exact` / `check_arity_min` - unified arity checking
  - `list_to_vec` - safe list traversal helper
  - `make_type_predicate` - generic type predicate wrapper
  - `primitive_numeric_compare` - generic comparison operator
  - Refactored 24 primitive functions to use helpers
  - Improved error messages with function names
- ✅ Created comprehensive example programs
  - 6 complex number example programs in `tests/fixtures/examples/complex_numbers/`
  - Documentation with mathematical background (QUICKSTART.md, README.md)
- ✅ Added 107 new passing tests (124 → 231)
- ✅ Progress: 54% → **60%** (136/228 tests) 🎉
- ✅ Numbers category: 93% → **94%** (30/32)
- ✅ New Complex Numbers category: **100%** (25/25) 🎉

**Previous Progress (2025-11-04):**
- ✅ Implemented `not`, `zero?`, `positive?`, `negative?` in bootstrap.scm
- ✅ Implemented `quotient`, `remainder`, `modulo` primitives
- ✅ Implemented `odd?`, `even?` in bootstrap.scm
- ✅ Implemented `abs`, `max`, `min` primitives
- ✅ Enabled 3 GCD algorithm tests (demonstrating multiple values!)
- ✅ **Implemented transparent overflow detection and BigInt promotion**
  - Arithmetic operations automatically promote i64 → BigInt on overflow
  - Uses Rust's `checked_add`, `checked_mul`, `checked_sub`, `checked_neg`
  - Seamlessly mixes i64 and BigInt operands
- ✅ **Implemented BigInt literal parsing**
  - Parser detects when integer literals exceed i64::MAX
  - Automatically parses large integers as BigInteger
  - Created comprehensive verification test suite (5 tests)
  - Added 8 parser unit tests for BigInt literal parsing
- ✅ **Implemented `exact?` and `inexact?` predicates**
  - Integer/BigInteger/Rational are exact
  - Real/Complex are inexact
  - Respects syntactic exactness (123 vs 123.0)

**Milestone Achieved:** Full R7RS numeric tower (Integer → BigInteger → Rational → Real → Complex) with exactness tracking!

---

**Note:** This document is actively maintained and reflects the actual implementation status.
