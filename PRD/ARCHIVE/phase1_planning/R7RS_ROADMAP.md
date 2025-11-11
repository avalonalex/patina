# R7RS Bare Minimum Roadmap

This document outlines the **strategic plan** for achieving bare minimum R7RS-small compliance for Patina.

**For current implementation status**, see [FEATURE_STATUS.md](../../docs/FEATURE_STATUS.md) which tracks test-by-test progress.

This roadmap focuses on **implementation strategy**, **phase planning**, and **rationale**.

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

Status: **COMPLETED** ✅

### Already Implemented ✅
- [x] `quote` - Quote expressions
- [x] `if` - Conditional evaluation
- [x] `define` - Variable definition
- [x] `set!` - Variable mutation
- [x] `begin` - Sequential evaluation
- [x] `lambda` - Function definition with full closures
- [x] **Lambda closures** - Proper environment capture ✅ (2025-11-02)
- [x] **Variadic lambda** - `(lambda (x y . rest) ...)` rest parameters ✅ (2025-11-02)
- [x] **Mixed arity lambda** - `(lambda (x . rest) ...)` ✅ (2025-11-02)

### TODO
- [ ] **Internal definitions** - `define` inside lambda bodies

### Test Coverage
From r7rs-tests.scm lines 43-91:
- Variable reference
- Quote forms
- Lambda (simple, variadic, rest parameters)
- If conditionals

## Phase 2: Derived Expression Types (4.2)

Status: **In Progress** - Core binding constructs complete! ✅

These can be implemented as special forms or macros:

### High Priority - COMPLETED ✅
- [x] `cond` - Multi-branch conditionals ✅ (2025-11-02)
- [x] `and` - Short-circuit logical AND ✅ (2025-11-02)
- [x] `or` - Short-circuit logical OR ✅ (2025-11-02)
- [x] `let` - Local bindings ✅ (2025-11-02)
- [x] `let*` - Sequential local bindings ✅ (2025-11-02)
- [x] `letrec` - Recursive local bindings ✅ (2025-11-02)
- [x] `letrec*` - Sequential recursive bindings ✅ (2025-11-02)

### High Priority - COMPLETED ✅
- [x] `case` - Pattern matching on values ✅ (2025-11-02)

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

**Already Implemented:**
- [x] `list` ✅

**Already Implemented:**
- [x] `list?`, `length`, `append`, `reverse` ✅ (2025-11-02)
- [x] `list-ref`, `list-tail` ✅ (2025-11-02)
- [x] `memq`, `memv`, `member` ✅ (2025-11-02)
- [x] `assq`, `assv`, `assoc` ✅ (2025-11-02)
- [x] `caar`, `cadr`, `cdar`, `cddr`, ... (c[ad]{2,4}r) ✅ (2025-11-02)

**TODO:**
- [ ] `list-set!` (mutation)

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
- [x] `apply` - Full implementation with variadic args ✅ (2025-11-02)

**TODO:**
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

1. ✅ **Fix closures** (critical for everything else) - COMPLETED 2025-11-02
   - ✅ Lambda captures its defining environment
   - ✅ Added `env` field to `Procedure::Lambda`
   - ✅ Referenced chibi's `eval.c` closure handling

2. ✅ **Implement let/let*/letrec** (builds on closures) - COMPLETED 2025-11-02
   - ✅ Implemented as special forms
   - ✅ All scoping rules working correctly

3. ✅ **Add cond, case, and, or** (frequently used) - COMPLETED 2025-11-02
   - ✅ Implemented as special forms
   - ✅ Short-circuit evaluation working
   - ✅ case with => syntax support

4. **Complete list operations** - IN PROGRESS
   - Many can be written in Scheme (see chibi's init-7.scm)
   - Some need to be primitives (cons, car, cdr already done)

5. ✅ **Add apply** - COMPLETED 2025-11-02, **map/for-each** - TODO
   - ✅ apply working with variadic arguments
   - 🔲 map, for-each - Next priority

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
- ✅ Quote, if, define, set!, begin, lambda (with closures!) - DONE
- ✅ let, let*, letrec, cond, and, or - DONE
- ✅ apply - DONE
- 🔲 case (pattern matching)
- 🔲 List operations: list ✅, append, reverse, map, for-each
- ✅ Essential predicates: eq?, eqv?, equal? - DONE
- 🔲 Numeric operations: full arithmetic ✅ basic, comparisons ✅
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

## Current Status Summary (Updated 2025-11-03)

**Test Coverage:** 92/115 compliance tests passing (80%) - See [FEATURE_STATUS.md](../../docs/FEATURE_STATUS.md) for details

**Fully Complete Categories:**
- ✅ **Lists (100%)** - All 30 list operations working
- ✅ **Control (100%)** - apply, map, for-each complete
- ✅ **Derived forms (83%)** - let, cond, case, and, or complete

**Remaining Work (23 ignored tests):**
- Simple predicates: `not`, `zero?`, `positive?`, `negative?`, `odd?`, `even?`
- Simple numeric ops: `abs`, `quotient`, `remainder`, `modulo`, `max`, `min`
- Edge cases: `when`, `unless`, `do`, division by zero

**Quick Wins:** Implementing the 9 simple predicates/operations above would reach **88% test coverage** with ~1-2 hours of work.

**Estimated effort to 95%+ compliance:**
- 🔲 Quick wins (predicates + simple numeric): 1-2 hours
- 🔲 Remaining numeric ops: 2-3 hours
- 🔲 `when`, `unless`, `do`: 2-4 hours
- **Total: ~1 day of focused work to reach 95%+ compliance**

## Post Phase 1: Usability & Tooling

After achieving bare-minimum R7RS compliance, focus shifts to usability:

**High Priority:**
- 🔲 Built-in help system - See [HELP_SYSTEM.md](./HELP_SYSTEM.md)
- 🔲 Better error messages with stack traces
- 🔲 Debugger/stepper support
- 🔲 REPL enhancements (history search, multi-line editing improvements)

## References

1. **R7RS Spec**: http://www.scheme-reports.org/
2. **Chibi tests**: `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`
3. **Chibi init**: `~/Project/reference/chibi-scheme/lib/init-7.scm`
4. **Chibi eval**: `~/Project/reference/chibi-scheme/eval.c`
