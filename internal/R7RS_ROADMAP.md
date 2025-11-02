# R7RS Bare Minimum Roadmap

This document outlines the path to bare minimum R7RS-small compliance for Patina, based on the chibi-scheme reference implementation and R7RS specification.

## Reference

Based on chibi-scheme test suite at `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`

The test suite is organized by R7RS spec sections:
- 4.1 Primitive expression types
- 4.2 Derived expression types
- 4.3 Macros
- 5.3 Variable definitions
- 5.5 Record-type definitions
- 6.x Standard library procedures

## Phase 1: Core Special Forms (4.1 Primitive Expressions)

Status: **In Progress**

### Already Implemented ✅
- [x] `quote` - Quote expressions
- [x] `if` - Conditional evaluation
- [x] `define` - Variable definition (basic)
- [x] `set!` - Variable mutation
- [x] `begin` - Sequential evaluation
- [x] `lambda` - Function definition (basic, needs closures)

### TODO
- [ ] **Fix lambda closures** - Currently doesn't properly capture environment
- [ ] **Variadic lambda** - `(lambda (x y . rest) ...)` rest parameters
- [ ] **Internal definitions** - `define` inside lambda bodies

### Test Coverage
From r7rs-tests.scm lines 43-91:
- Variable reference
- Quote forms
- Lambda (simple, variadic, rest parameters)
- If conditionals

## Phase 2: Derived Expression Types (4.2)

Status: **Not Started**

These can be implemented as special forms or macros:

### High Priority
- [ ] `cond` - Multi-branch conditionals
- [ ] `case` - Pattern matching on values
- [ ] `and` - Short-circuit logical AND
- [ ] `or` - Short-circuit logical OR
- [ ] `let` - Local bindings
- [ ] `let*` - Sequential local bindings
- [ ] `letrec` - Recursive local bindings
- [ ] `letrec*` - Sequential recursive bindings

### Medium Priority
- [ ] `let-values` - Multiple value binding
- [ ] `let*-values` - Sequential multiple value binding
- [ ] `when` - Conditional execution (like if without else)
- [ ] `unless` - Negated conditional
- [ ] `do` - Iteration construct
- [ ] `named let` - Named let for recursion

### Test Coverage
From r7rs-tests.scm lines 94-300+:
- cond (with else, =>)
- case (with else, =>)
- and/or short-circuiting
- let/let*/letrec scoping rules
- do loops

## Phase 3: Essential Primitives

### Numeric Operations (6.2.6)
**Already Implemented:**
- [x] `+`, `-`, `*`, `/` - Basic arithmetic
- [x] `=`, `<`, `>`, `<=`, `>=` - Comparisons

**TODO:**
- [ ] `quotient`, `remainder`, `modulo`
- [ ] `floor`, `ceiling`, `truncate`, `round`
- [ ] `numerator`, `denominator` (for rationals)
- [ ] `exact->inexact`, `inexact->exact`
- [ ] `number->string`, `string->number`
- [ ] Proper numeric tower coercion rules

### List Operations (6.4)
**Already Implemented:**
- [x] `cons`, `car`, `cdr`
- [x] `null?`, `pair?`

**TODO:**
- [ ] `list`, `list?`, `length`, `append`, `reverse`
- [ ] `list-ref`, `list-tail`, `list-set!`
- [ ] `memq`, `memv`, `member`
- [ ] `assq`, `assv`, `assoc`
- [ ] `caar`, `cadr`, `cdar`, `cddr`, ... (c[ad]{2,4}r)

### Boolean Operations (6.3)
**Already Implemented:**
- [x] `boolean?`

**TODO:**
- [ ] `not` (currently works but should verify)
- [ ] `boolean=?`

### Symbol Operations (6.5)
**Already Implemented:**
- [x] `symbol?`

**TODO:**
- [ ] `symbol->string`, `string->symbol`
- [ ] `symbol=?`

### Character Operations (6.6)
**TODO:**
- [ ] `char?`, `char=?`, `char<?`, etc.
- [ ] `char->integer`, `integer->char`
- [ ] `char-upcase`, `char-downcase`
- [ ] `char-alphabetic?`, `char-numeric?`, etc.

### String Operations (6.7)
**Already Implemented:**
- [x] `string?`

**TODO:**
- [ ] `make-string`, `string`, `string-length`
- [ ] `string-ref`, `string-set!`
- [ ] `string-append`, `substring`
- [ ] `string->list`, `list->string`
- [ ] `string=?`, `string<?`, etc.
- [ ] String comparison and case operations

### Vector Operations (6.8)
**TODO:**
- [ ] `make-vector`, `vector`, `vector?`, `vector-length`
- [ ] `vector-ref`, `vector-set!`
- [ ] `vector->list`, `list->vector`
- [ ] `vector-fill!`, `vector-copy`

### Control Features (6.10)
**Already Implemented:**
- [x] `apply` (basic version)

**TODO:**
- [ ] `apply` - Full implementation with multiple args
- [ ] `map`, `for-each`
- [ ] `call-with-current-continuation` (call/cc)
- [ ] `values`, `call-with-values`
- [ ] `dynamic-wind`

## Phase 4: I/O Operations (6.13)

### Ports
**TODO:**
- [ ] `input-port?`, `output-port?`
- [ ] `current-input-port`, `current-output-port`, `current-error-port`
- [ ] `open-input-file`, `open-output-file`
- [ ] `close-input-port`, `close-output-port`
- [ ] `call-with-input-file`, `call-with-output-file`

### Input
**TODO:**
- [ ] `read`, `read-char`, `peek-char`
- [ ] `eof-object?`, `eof-object`
- [ ] `char-ready?`

### Output
**TODO:**
- [ ] `write`, `display`, `newline`
- [ ] `write-char`, `write-string`

## Phase 5: Hygienic Macros (4.3)

Status: **Not Started** (Complex, save for later)

**TODO:**
- [ ] `syntax-rules` - Pattern-based macro system
- [ ] `let-syntax`, `letrec-syntax` - Local macro bindings
- [ ] Proper hygiene and scope handling

### Why Later?
Macros are complex and require:
- Syntax object representation
- Hygiene algorithm (preventing variable capture)
- Macro expansion phase
- Can implement many features as special forms first, then convert to macros

## Phase 6: Exception Handling (6.11)

**TODO:**
- [ ] `error`, `error-object?`
- [ ] `raise`, `raise-continuable`
- [ ] `with-exception-handler`
- [ ] `guard` syntax

## Phase 7: Module System (5.2, 5.6)

Status: **Not Started** (Can work in global environment first)

**TODO:**
- [ ] `define-library`
- [ ] `import`, `export`
- [ ] Library path resolution
- [ ] Standard libraries (scheme base), (scheme write), etc.

## Implementation Strategy

### Recommended Order

1. **Fix closures** (critical for everything else)
   - Lambda needs to capture its defining environment
   - Add `env` field to `Procedure::Lambda`
   - Reference: chibi's `eval.c` closure handling

2. **Implement let/let*/letrec** (builds on closures)
   - These are used everywhere in R7RS code
   - Can implement as special forms or desugar to lambda

3. **Add cond, case, and, or** (frequently used)
   - Straightforward special forms
   - Enable more idiomatic Scheme code

4. **Complete list operations**
   - Many can be written in Scheme (see chibi's init-7.scm)
   - Some need to be primitives (cons, car, cdr already done)

5. **Add map, for-each, apply**
   - Critical for functional programming
   - Used throughout R7RS test suite

6. **Numeric tower completion**
   - Proper handling of exact/inexact
   - Rational and complex number operations

7. **String and character operations**
   - Needed for I/O and text processing
   - Unicode support per R7RS

8. **I/O operations**
   - Ports, read, write, display
   - File operations

9. **Exception handling**
   - error, raise, guard
   - Needed for robust programs

10. **Macros** (save for last)
    - Most complex feature
    - Many things can work without it initially

### Testing Approach

1. **Port chibi's test suite incrementally**
   - Start with section 4.1 tests (primitives)
   - Copy relevant tests from r7rs-tests.scm
   - Add to `tests/schemes/r7rs/` directory

2. **Use comparison tests**
   - Run same code through Patina and chibi
   - Compare outputs (already have infrastructure)

3. **Track progress**
   - Document which sections of r7rs-tests.scm pass
   - Update this roadmap as features are completed

## Bare Minimum Definition

For a "bare minimum R7RS interpreter", we need:

### Core (Must Have)
- ✅ Quote, if, define, set!, begin, lambda (with closures!)
- 🔲 let, let*, letrec, cond, case, and, or
- 🔲 List operations: list, append, reverse, map, for-each
- 🔲 Essential predicates: eq?, eqv?, equal?
- 🔲 Numeric operations: full arithmetic, comparisons
- 🔲 Basic I/O: read, write, display

### Nice to Have (For Practical Use)
- String operations
- Vector operations
- Error handling
- call/cc (continuations)

### Can Defer
- Hygienic macros (syntax-rules)
- Module system
- Full numeric tower (complex, exact rationals)
- call-with-values, dynamic-wind

## Current Status Summary

**Working:** ~10% of R7RS
- Basic arithmetic and comparisons
- Simple special forms
- Primitive predicates
- Basic list operations

**Next Priority:** Closures → let forms → cond/case → list operations

**Estimated effort to bare minimum:**
- Closures: 1-2 days
- Let forms: 1 day
- Cond/case/and/or: 1 day
- List ops: 2-3 days
- Map/for-each/apply: 1-2 days
- Predicates and utilities: 1-2 days
- **Total: ~2 weeks of focused work**

## References

1. **R7RS Spec**: http://www.scheme-reports.org/
2. **Chibi tests**: `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`
3. **Chibi init**: `~/Project/reference/chibi-scheme/lib/init-7.scm`
4. **Chibi eval**: `~/Project/reference/chibi-scheme/eval.c`
