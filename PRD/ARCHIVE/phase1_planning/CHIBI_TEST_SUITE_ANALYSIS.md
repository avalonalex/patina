# Chibi-Scheme R7RS Test Suite Analysis

**Goal:** Run chibi-scheme's comprehensive R7RS test suite (`r7rs-tests.scm`) against Patina

**Test File:** `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`
- **Size:** 2,516 lines
- **Coverage:** All R7RS-small procedures and syntax (except `delete-file`)
- **Assumptions:** Full unicode, full numeric tower, all standard libraries

---

## Required Dependencies

### 1. Module System (CRITICAL)

The test file starts with:
```scheme
(import (scheme base) (scheme char) (scheme lazy)
        (scheme inexact) (scheme complex) (scheme time)
        (scheme file) (scheme read) (scheme write)
        (scheme eval) (scheme process-context) (scheme case-lambda)
        (scheme r5rs)
        (chibi test))
```

**What we need:**
- ✅ `(scheme base)` - Core language (we have most of this!)
- ❌ `(scheme char)` - Character library
- ❌ `(scheme lazy)` - Promises (delay/force)
- ❌ `(scheme inexact)` - Inexact numbers (floats)
- ✅ `(scheme complex)` - Complex numbers (we have this!)
- ❌ `(scheme time)` - Time operations
- ❌ `(scheme file)` - File I/O
- ❌ `(scheme read)` - Read procedures
- ❌ `(scheme write)` - Write procedures (display, write)
- ❌ `(scheme eval)` - Eval
- ❌ `(scheme process-context)` - Command line args, exit
- ❌ `(scheme case-lambda)` - Case-lambda syntax
- ❌ `(scheme r5rs)` - R5RS compatibility
- ❌ `(chibi test)` - Test framework (or SRFI-64)

**Module System Requirements:**
- `define-library` syntax
- `import` declarations
- `export` specifications
- Library name resolution
- Multiple file loading

**Estimated Effort:** 2-3 weeks (as noted in IMPLEMENTATION_STATUS.md)

---

### 2. Test Infrastructure (EASY)

The test suite uses a simple test framework that can be implemented as a library:

```scheme
(define (test-begin . o) #f)  ; Optional name parameter

(define (test-end . o) #f)    ; Optional name parameter

(define-syntax test
  (syntax-rules ()
    ((test expected expr)
     (let ((res expr))
       (cond
        ((not (equal? res expected))
         (display "FAIL: ")
         (write 'expr)
         (display ": expected ")
         (write expected)
         (display " but got ")
         (write res)
         (newline)))))))
```

**What we need:**
- ✅ `syntax-rules` - We have this!
- ✅ `equal?` - We have this!
- ❌ `display` - Need basic I/O
- ❌ `write` - Need basic I/O
- ❌ `newline` - Need basic I/O

**Estimated Effort:** 1-2 days with basic I/O, or can stub for now

---

### 3. I/O Procedures (MEDIUM)

**Minimal I/O needed for tests:**
- `display` - Display a value
- `write` - Write a value (with escapes)
- `newline` - Output newline

**From IO_IMPLEMENTATION.md, Phase 1 (2-3 days):**
- Port infrastructure (`Value::Port`)
- String ports (no filesystem needed initially)
- `display`, `write`, `newline`, `write-char`
- `read`, `read-char`, `eof-object`

**For full test suite:**
- File I/O (Phase 2, 2-3 days)
- Parameter objects for current ports (Phase 3, 1-2 days)

**Estimated Effort:** 2-3 days for minimal, 5-7 days for complete

---

### 4. Missing Language Features

Based on quick scan of the test file:

**High Priority (needed by many tests):**
- ❌ `inexact?` / `exact?` - Type predicates (we have partial)
- ❌ Floating point numbers (Real type exists but needs math ops)
- ❌ `floor`, `ceiling`, `truncate`, `round` - Rounding functions
- ❌ `sqrt`, `expt` - Math functions
- ❌ `char?`, `char=?`, `char<?`, etc. - Character operations
- ❌ `string->list`, `list->string` - Conversions
- ❌ `string-copy`, `string-fill!` - String operations
- ❌ `vector-fill!`, `vector-copy` - Vector operations
- ❌ `delay`, `force`, `promise?` - Lazy evaluation
- ❌ `case-lambda` - Variable arity syntax

**Medium Priority:**
- ❌ `dynamic-wind` - Control flow
- ❌ `call-with-current-continuation` (call/cc) - Continuations
- ❌ `eval` - Runtime evaluation
- ❌ `environment` - Environment reification
- ❌ `command-line`, `exit` - Process context
- ❌ `current-jiffy`, `jiffies-per-second` - Timing

**Estimated Effort:**
- Math functions: 3-5 days (from IMPLEMENTATION_STATUS.md)
- Character ops: 2-3 days
- String/vector ops: 2-3 days
- Lazy evaluation: 1-2 days
- case-lambda: 1 day
- **Total:** ~2-3 weeks

---

## Gap Analysis Summary

### What We Have ✅
- Core special forms (quote, if, define, lambda, set!, begin)
- Binding constructs (let, let*, letrec, letrec*, let-values, let*-values)
- Conditionals (cond, case, and, or, when, unless)
- Tail call optimization (100%)
- Macro system (syntax-rules, define-syntax)
- Complex numbers
- Integer arithmetic
- Basic list operations
- Basic vector operations
- Basic string operations
- Type predicates (most)
- Higher-order functions (apply, map, for-each)

### What We're Missing ❌

**Blockers (Cannot run tests without these):**
1. **Module system** - 2-3 weeks
2. **Basic I/O** (display, write, newline) - 2-3 days

**Major Gaps (Many tests will fail):**
3. **Floating point math** - 3-5 days
4. **Character operations** - 2-3 days
5. **String/vector utilities** - 2-3 days
6. **Lazy evaluation** (delay/force) - 1-2 days

**Advanced Features (Some tests will fail):**
7. **call/cc** - 2-3 weeks
8. **eval/environment** - 1 week
9. **Dynamic-wind** - 3-5 days
10. **Process context** - 1-2 days
11. **Timing** - 1-2 days

---

## Phased Approach to Run Chibi Tests

### Phase 1: Minimal Test Infrastructure (1 week)
**Goal:** Run a subset of tests without module system

**What to implement:**
1. **Stub test framework** (1-2 hours)
   - Define `test-begin`, `test-end`, `test` as macros/procedures
   - Use `equal?` for comparison
   - Print results to console (initially without display/write)

2. **Basic I/O** (2-3 days)
   - Implement `display`, `write`, `newline`
   - String ports for testing
   - Minimal port infrastructure

3. **Extract non-module tests** (1 day)
   - Create `patina-r7rs-tests.scm` without `import`
   - Comment out tests requiring missing features
   - Focus on core language tests

**Expected Pass Rate:** ~40-50% of tests

---

### Phase 2: Module System + Libraries (3-4 weeks)
**Goal:** Full module support to run complete test suite

**What to implement:**
1. **Module system** (2-3 weeks)
   - `define-library` syntax
   - `import`/`export` resolution
   - Multi-file loading
   - Library search paths

2. **Standard libraries** (1 week)
   - `(scheme base)` - Package core features
   - `(scheme char)` - Character operations
   - `(scheme write)` - Output procedures
   - `(scheme read)` - Input procedures (partial)
   - `(chibi test)` or minimal SRFI-64

3. **Missing core features** (1 week)
   - Floating point math operations
   - String/vector utilities
   - Character operations

**Expected Pass Rate:** ~70-80% of tests

---

### Phase 3: Advanced Features (2-4 weeks)
**Goal:** Maximum R7RS compliance

**What to implement:**
1. **File I/O** (2-3 days)
2. **Lazy evaluation** (delay/force) (1-2 days)
3. **Exception handling** (guard, raise) (3-5 days)
4. **Remaining math** (sqrt, trig, etc.) (2-3 days)
5. **eval/environment** (1 week)
6. **Process context** (command-line, exit) (1-2 days)
7. **Timing functions** (1-2 days)

**Expected Pass Rate:** ~90-95% of tests

---

### Phase 4: Complete R7RS (Optional, 3-4 weeks)
**Goal:** 100% test suite pass rate

**What to implement:**
1. **call/cc** (2-3 weeks)
2. **Dynamic-wind** (3-5 days)
3. **Binary I/O** (3-4 days)
4. **case-lambda** (1 day)
5. **Parameter objects** (2-3 days)

**Expected Pass Rate:** ~98-100% of tests

---

## Recommended Immediate Next Steps

Given your current progress (88% R7RS compliant, 100% TCO), here's what I recommend:

### Option A: Quick Win Approach (1-2 weeks)
1. Implement **basic I/O** (display, write, newline) - 2-3 days
2. Create **stub test framework** - 2 hours
3. **Extract and adapt** chibi tests into `patina-r7rs-tests.scm` - 1 day
4. Run subset without modules - **Immediate validation!**
5. Implement **missing math functions** - 3-5 days
6. Implement **string/char/vector utilities** - 3-4 days

**Result:** Can run ~50% of chibi tests, validates core language

### Option B: Full Module System First (3-4 weeks)
1. Implement **module system** - 2-3 weeks
2. Implement **basic I/O** - 2-3 days
3. Implement **test library** - 1-2 days
4. Run **full test suite** with expected failures
5. Iteratively fix gaps

**Result:** Can run 100% of tests, but many will fail initially

### Option C: Parallel Approach (Most Efficient)
1. **You** implement module system (2-3 weeks)
2. **Meanwhile**, implement:
   - Basic I/O (2-3 days)
   - Math functions (3-5 days)
   - Character ops (2-3 days)
   - String/vector utils (2-3 days)
3. When modules ready, everything else is done!

**Result:** Module system + features done in ~3 weeks total

---

## My Recommendation

**Start with Option A (Quick Win)** because:

1. ✅ **Immediate validation** - See how much works NOW
2. ✅ **Finds gaps early** - Discover what's actually blocking tests
3. ✅ **Motivating** - Watch pass rate climb as you add features
4. ✅ **Incremental** - Each feature adds more passing tests
5. ✅ **No module system needed** - Can start TODAY

**Then transition to module system** once you've validated core features.

The test suite will be incredibly valuable for finding edge cases and ensuring R7RS compliance!

---

## Estimated Timeline

**Conservative estimate to run chibi r7rs-tests.scm:**

- **Minimal (40% pass rate):** 1 week
- **Good (70% pass rate):** 5-6 weeks
- **Excellent (90% pass rate):** 8-10 weeks
- **Perfect (100% pass rate):** 12-14 weeks

**Your current pace:** Very fast! TCO took 2.5 hours vs estimated 2-3 days.

**Realistic timeline at your pace:** 6-8 weeks to 90%+ pass rate

---

## Next Action Items

If you want to start with **Option A (Quick Win)**:

1. ✅ Implement basic I/O (display, write, newline) - See `PRD/phase1/IO_IMPLEMENTATION.md`
2. ✅ Create stub test framework as Scheme library
3. ✅ Extract subset of chibi tests
4. ✅ Run and measure pass rate
5. ✅ Implement missing features based on failures

Would you like me to start implementing the basic I/O and test framework?
