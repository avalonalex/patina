# R7RS (scheme base) Feature Matrix

**Last Updated:** 2025-11-02 (post-lambda implementation)

## Status Legend
- ✅ **Implemented & Tested** - Feature fully working with passing tests
- 🚧 **Partial** - Some cases work, others don't
- ❌ **Not Implemented** - Feature not yet started
- 🔒 **Blocked** - Waiting on another feature

---

## Summary Statistics

| Category | Implemented | Total | Progress |
|----------|-------------|-------|----------|
| Primitives | 18 | 25 | 72% |
| Derived Forms | 2 | 30 | 7% |
| Numbers | 11 | 60 | 18% |
| Lists | 6 | 35 | 17% |
| Strings | 0 | 30 | 0% |
| Vectors | 0 | 20 | 0% |
| Control | 0 | 15 | 0% |
| I/O | 0 | 30 | 0% |
| **TOTAL** | **37** | **245** | **15%** |

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
| quote (symbol) | ✅ | test_quote_symbol | r7rs_primitives.rs:23 |
| quote (list) | ✅ | test_quote_list | r7rs_primitives.rs:29 |
| quote (nested) | ✅ | test_quote_nested | r7rs_primitives.rs:35 |
| quote (literals) | ✅ | test_quote_literals | r7rs_primitives.rs:41 |
| quasiquote | ❌ | - | - |
| unquote | ❌ | - | - |
| unquote-splicing | ❌ | - | - |

### 4.1.4 Procedures
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| lambda (fixed arity) | ✅ | test_simple_lambda | r7rs_primitives.rs:50 |
| lambda (multiple args) | ✅ | test_lambda_multiple_args | r7rs_primitives.rs:55 |
| lambda (variadic) | ✅ | test_lambda_variadic | r7rs_primitives.rs:81 |
| lambda (mixed fixed + rest) | ✅ | test_lambda_variadic_with_fixed | r7rs_primitives.rs:86 |
| lambda (closure) | 🔒 | test_lambda_closure | Blocked by `let` |
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
| cond (simple) | ✅ | test_cond_simple | r7rs_derived.rs |
| cond (else) | ✅ | test_cond_with_else | r7rs_derived.rs |
| cond (=>) | ❌ | test_cond_with_arrow | Not implemented |
| case | ❌ | test_case_simple | Not implemented |
| case (else) | ❌ | test_case_with_else | Not implemented |
| and | ❌ | test_and_* | Not implemented |
| or | ❌ | test_or_* | Not implemented |
| when | ❌ | test_when | Not implemented |
| unless | ❌ | test_unless | Not implemented |

### 4.2.2 Binding Constructs
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| let | ❌ | test_let_simple | Not implemented |
| let (scoping) | ❌ | test_let_scoping | Not implemented |
| let* | ❌ | test_let_star_sequential | Not implemented |
| letrec | ❌ | test_letrec_recursive | Not implemented |
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
| do | ❌ | test_do_simple | Not implemented |

---

## Numbers (Section 6.2)

### Arithmetic Operations
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| + | ✅ | test_addition | r7rs_numbers.rs |
| - | ✅ | test_subtraction | r7rs_numbers.rs |
| * | ✅ | test_multiplication | r7rs_numbers.rs |
| / | ✅ | test_division | r7rs_numbers.rs |
| / (zero check) | ✅ | test_division_by_zero | r7rs_numbers.rs |
| abs | ❌ | test_abs | Not implemented |
| quotient | ❌ | test_quotient | Not implemented |
| remainder | ❌ | test_remainder | Not implemented |
| modulo | ❌ | test_modulo | Not implemented |
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
| number? | ✅ | test_number_predicate | r7rs_numbers.rs |
| integer? | ✅ | test_integer_predicate | r7rs_numbers.rs |
| zero? | ❌ | test_zero_predicate | Not implemented |
| positive? | ❌ | test_positive_predicate | Not implemented |
| negative? | ❌ | test_negative_predicate | Not implemented |
| odd? | ❌ | test_odd_predicate | Not implemented |
| even? | ❌ | test_even_predicate | Not implemented |
| exact? | ❌ | - | Not implemented |
| inexact? | ❌ | - | Not implemented |
| exact-integer? | ❌ | - | Not implemented |
| finite? | ❌ | - | Not implemented |
| infinite? | ❌ | - | Not implemented |
| nan? | ❌ | - | Not implemented |
| max | ❌ | test_max | Not implemented |
| min | ❌ | test_min | Not implemented |

---

## Lists and Pairs (Section 6.4)

### Constructors
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| cons | ✅ | test_cons | r7rs_lists.rs |
| list | ❌ | test_list | Not implemented |
| make-list | ❌ | - | Not implemented |

### Accessors
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| car | ✅ | test_car | r7rs_lists.rs |
| cdr | ✅ | test_cdr | r7rs_lists.rs |
| caar, cadr, cdar, cddr | ❌ | test_caar, etc. | Not implemented |
| list-ref | ❌ | test_list_ref | Not implemented |
| list-tail | ❌ | test_list_tail | Not implemented |

### Predicates
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| pair? | ✅ | test_pair_predicate | r7rs_lists.rs |
| null? | ✅ | test_null_predicate | r7rs_lists.rs |
| list? | ❌ | test_list_predicate | Not implemented |

### Operations
| Feature | Status | Tests | File |
|---------|--------|-------|------|
| length | ❌ | test_length | Not implemented |
| append | ❌ | test_append | Not implemented |
| reverse | ❌ | test_reverse | Not implemented |
| list-tail | ❌ | test_list_tail | Not implemented |
| memq, memv, member | ❌ | test_memq | Not implemented |
| assq, assv, assoc | ❌ | test_assq | Not implemented |
| list-copy | ❌ | - | Not implemented |
| list-set! | ❌ | - | Not implemented |

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

## Control Features (Section 6.10)

| Feature | Status | Notes |
|---------|--------|-------|
| procedure? | ❌ | Type predicate |
| apply | ❌ | High priority - needed for many functions |
| map | ❌ | High priority |
| for-each | ❌ | High priority |
| call-with-values | ❌ | Multiple values support |
| values | ❌ | Multiple values support |
| dynamic-wind | ❌ | Low priority |
| call/cc | ❌ | Low priority - complex |

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

Based on this matrix, the recommended implementation order:

### Phase 1: Core Language (Next 2-3 weeks)
1. ✅ ~~lambda~~ (DONE!)
2. let, let*, letrec (blocking many tests)
3. and, or, not (simple, widely used)
4. apply (needed for higher-order functions)
5. map, for-each (common operations)

### Phase 2: Numbers (Week 4-5)
6. abs, quotient, remainder, modulo
7. zero?, positive?, negative?, odd?, even?
8. max, min

### Phase 3: Lists (Week 5-6)
9. length, append, reverse
10. list-ref, list-tail
11. list?, list-copy

### Phase 4: Strings & Vectors (Week 7-8)
12. Basic string operations
13. Basic vector operations

### Phase 5: I/O (Week 9+)
14. display, write, newline
15. Port operations

---

**Note:** This matrix should be updated after each feature implementation to track progress.
