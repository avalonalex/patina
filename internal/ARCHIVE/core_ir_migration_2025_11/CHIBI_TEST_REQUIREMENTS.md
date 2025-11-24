# Requirements for (import (chibi test))

**Goal:** Get chibi r7rs-tests.scm working by supporting `(import (chibi test))`

**Status:** Can be achieved incrementally - Start with minimal, add as needed

---

## What (chibi test) Needs

### Direct Dependencies (from test.sld)

```scheme
(import (scheme base)           ; ❌ Need to create
        (scheme case-lambda)    ; ❌ Need to create
        (scheme write)          ; ❌ Need to create
        (scheme complex)        ; ❌ Need to create
        (scheme process-context); ❌ Need to create
        (scheme time)           ; ❌ Need to create
        (chibi diff)            ; ❌ Chibi-specific
        (chibi term ansi)       ; ❌ Chibi-specific
        (chibi optional))       ; ❌ Chibi-specific
```

### Additional Features Used

From examining `test.scm`:
- `guard` (exception handling) - ❌ Not implemented
- `open-output-string`, `get-output-string` - ❌ Not implemented
- `current-error-port` - ❌ Not implemented
- Parameters (make-parameter) - ❌ Not implemented
- `case-lambda` - ✅ Already implemented!

---

## Implementation Strategy

### Strategy 1: Copy and Adapt (RECOMMENDED)

**Approach:** Copy `(chibi test)` to our codebase and adapt it to work with what we have.

**Steps:**
1. Copy chibi test files to `lib/chibi/`
2. Remove dependencies we don't have
3. Simplify to core functionality
4. Add missing primitives as we discover them

**Advantages:**
- ✅ Start working immediately
- ✅ Add features incrementally
- ✅ Control over implementation
- ✅ Can simplify for our needs

**Disadvantages:**
- ❌ Not original chibi test (but compatible subset)
- ❌ May need to update as we add features

### Strategy 2: Full R7RS Compliance First

**Approach:** Implement all standard libraries that chibi test needs.

**Steps:**
1. Create `(scheme base)` - 100+ exports
2. Create `(scheme write)` - display, write, newline
3. Create `(scheme case-lambda)` - re-export existing
4. Create `(scheme complex)` - complex number operations
5. Create `(scheme process-context)` - command-line-arguments, exit
6. Create `(scheme time)` - current-second, current-jiffy, etc.
7. Implement missing primitives (guard, parameters, ports)
8. Create chibi-specific libraries (diff, term, optional)

**Advantages:**
- ✅ Full R7RS compliance
- ✅ Works with unmodified chibi test
- ✅ Benefits all future code

**Disadvantages:**
- ❌ Much more work upfront (1-2 weeks)
- ❌ Chibi tests still won't work until ALL dependencies ready

---

## Recommended Approach: Hybrid

### Phase 1: Minimal Chibi Test (2-4 hours)

**Goal:** Get basic `test` macro working

Create simplified `lib/chibi/test-minimal.scm`:

```scheme
;; Simplified (chibi test) - minimal subset for basic testing

(define-library (chibi test)
  (export test test-group test-begin test-end)

  (import (scheme base)    ; We'll create minimal version
          (scheme write))  ; We'll create minimal version

  (begin
    ;; Test state
    (define *tests-passed* 0)
    (define *tests-failed* 0)
    (define *current-group* #f)

    ;; Simple test macro
    (define-syntax test
      (syntax-rules ()
        ((test expected actual)
         (test "" expected actual))
        ((test name expected actual)
         (let ((result actual))
           (if (equal? result expected)
               (begin
                 (set! *tests-passed* (+ *tests-passed* 1))
                 (display "PASS: ")
                 (display name)
                 (newline))
               (begin
                 (set! *tests-failed* (+ *tests-failed* 1))
                 (display "FAIL: ")
                 (display name)
                 (newline)
                 (display "  Expected: ")
                 (write expected)
                 (newline)
                 (display "  Got: ")
                 (write result)
                 (newline)))))))

    ;; Test grouping (stubbed for now)
    (define-syntax test-group
      (syntax-rules ()
        ((test-group name body ...)
         (begin
           (display "Group: ")
           (display name)
           (newline)
           body ...))))

    (define (test-begin name)
      (set! *tests-passed* 0)
      (set! *tests-failed* 0)
      (display "Running: ")
      (display name)
      (newline))

    (define (test-end)
      (display "Tests passed: ")
      (display *tests-passed*)
      (newline)
      (display "Tests failed: ")
      (display *tests-failed*)
      (newline))))
```

**What this needs:**
- ✅ `(scheme base)` - minimal: define, lambda, if, set!, equal?
- ✅ `(scheme write)` - minimal: display, write, newline

### Phase 2: Create Minimal Standard Libraries (2-3 hours)

**lib/scheme/base.scm:**
```scheme
;; Minimal (scheme base) - just enough for chibi test

(define-library (scheme base)
  (export
    ;; Core syntax (special forms - already exist)
    lambda if define set! quote begin

    ;; Booleans
    not

    ;; Equivalence
    eq? eqv? equal?

    ;; Numbers (primitives we already have)
    + - * / = < > <= >=
    number? integer? exact? inexact?

    ;; Pairs and lists (primitives we already have)
    cons car cdr null? pair?
    list length append reverse

    ;; Symbols
    symbol? symbol->string string->symbol

    ;; Characters
    char? char=?

    ;; Strings
    string? string=? string-length

    ;; Vectors
    vector? vector-length vector-ref

    ;; Procedures
    procedure? apply

    ;; And anything else we discover is needed
  )

  (import (patina primitives))  ; Our Rust primitives

  (begin
    (define (not x) (if x #f #t))
    ;; Add other derived procedures as needed
  ))
```

**lib/scheme/write.scm:**
```scheme
;; Minimal (scheme write)

(define-library (scheme write)
  (export display write newline write-char)

  (import (patina primitives))  ; display, write, etc. are primitives

  (begin))  ; All primitives, nothing to define
```

### Phase 3: Test It! (30 minutes)

```scheme
;; test-minimal-test.scm
(import (chibi test))

(test-begin "Basic tests")

(test 4 (+ 2 2))
(test "addition" 4 (+ 2 2))
(test 3 (+ 1 2))

(test-group "Arithmetic"
  (test 6 (* 2 3))
  (test 8 (/ 16 2)))

(test-end)
```

**Expected output:**
```
Running: Basic tests
PASS:
PASS: addition
PASS:
Group: Arithmetic
PASS:
PASS:
Tests passed: 5
Tests failed: 0
```

---

## Dependencies Breakdown

### What We Already Have ✅

**Primitives (in Rust):**
- Arithmetic: +, -, *, /, =, <, >, <=, >=
- Lists: cons, car, cdr, null?, pair?, list, length, append, reverse
- Type predicates: number?, string?, symbol?, char?, vector?, procedure?
- Comparison: eq?, eqv?, equal?
- I/O: display, write, newline (basic versions)
- Special forms: lambda, if, define, set!, quote, begin, let, cond, case

**Language features:**
- ✅ Macros (syntax-rules)
- ✅ Case-lambda (implemented)
- ✅ Quasiquote (just fixed!)
- ✅ Tail call optimization

### What We're Missing ❌

**For (scheme base):**
- `guard` - Exception handling (complex)
- `make-parameter`, `parameterize` - Parameters (medium)
- `call-with-current-continuation` (call/cc) - Continuations (complex)
- `dynamic-wind` - Cleanup handlers (medium)
- Many list procedures: map, for-each, filter, etc. (easy - can write in Scheme!)

**For (scheme write):**
- `write-string`, `write-u8` - Extended I/O (easy)

**For (scheme process-context):**
- `command-line` - Get argv (medium - need Rust support)
- `exit` - Exit program (easy - Rust primitive)
- `get-environment-variable`, `get-environment-variables` - env vars (easy)

**For (scheme time):**
- `current-second` - Unix timestamp (easy - Rust primitive)
- `current-jiffy`, `jiffies-per-second` - High-res timer (easy - Rust primitive)

**Chibi-specific:**
- `(chibi diff)` - Diff algorithm (can stub or skip)
- `(chibi term ansi)` - ANSI colors (can stub)
- `(chibi optional)` - Optional arguments (can implement or skip)

---

## Concrete Action Plan

### Step 1: Create Patina Primitives Library (1 hour)

**Goal:** Export all Rust primitives as a library

**File:** `lib/patina/primitives.scm`

```scheme
(define-library (patina primitives)
  (export
    ; List ALL primitives we have in Rust
    ; This is just cataloging, not implementation
    + - * / = < > <= >=
    cons car cdr null? pair? list?
    ; ... ~150 more primitives
  )

  ; No body - primitives provided by Rust
  ; Special handling in SchemeLibraryLoader or custom RustLibraryLoader
)
```

**OR:** Implement `RustLibraryLoader` that directly exports primitives without needing a file.

### Step 2: Create Minimal Standard Libraries (2 hours)

**Files needed:**
1. `lib/scheme/base.scm` - Import primitives, add derived procedures
2. `lib/scheme/write.scm` - Re-export write primitives
3. `lib/scheme/case-lambda.scm` - Re-export case-lambda special form

### Step 3: Create Minimal Chibi Test (1 hour)

**File:** `lib/chibi/test.scm`

Simplified version with just `test`, `test-group`, `test-begin`, `test-end`.

### Step 4: Test Integration (30 minutes)

Create simple test file:
```scheme
(import (chibi test))
(test 4 (+ 2 2))
```

Fix any issues that come up.

### Step 5: Run Chibi Tests (30 minutes)

Try running actual chibi r7rs-tests.scm:
```bash
./scripts/run_chibi_tests.sh
```

See what breaks, fix incrementally.

---

## Missing Primitives Priority

### High Priority (needed for basic testing)

1. **apply** - Already have most of implementation, just needs wiring
2. **map** - Can implement in Scheme using recursion
3. **for-each** - Can implement in Scheme
4. **member**, **assoc** - Can implement in Scheme

### Medium Priority (nice to have)

1. **String procedures** - string-append, substring, etc.
2. **Port procedures** - current-output-port, current-error-port
3. **exit** - Exit with code
4. **command-line** - Get command-line arguments

### Low Priority (can stub)

1. **guard** - Exception handling (complex, can start with simple version)
2. **Parameters** - make-parameter, parameterize
3. **Time functions** - current-second, etc.

---

## Estimated Effort

### Minimal Working Version
- **Time:** 4-6 hours
- **Deliverable:** Basic `(chibi test)` with simple `test` macro works
- **Coverage:** Can run simple tests, report pass/fail

### Moderate Coverage
- **Time:** 1-2 days
- **Deliverable:** Standard libraries exist, most chibi tests parse
- **Coverage:** 50-70% of chibi r7rs-tests work

### Full Support
- **Time:** 1-2 weeks
- **Deliverable:** Complete R7RS standard libraries
- **Coverage:** 95%+ of chibi r7rs-tests pass

---

## Next Immediate Actions

1. **Create `lib/patina/primitives.scm`** - List all primitives we have
2. **Create `lib/scheme/base.scm`** - Import primitives, minimal exports
3. **Create `lib/scheme/write.scm`** - Re-export display, write, newline
4. **Create `lib/chibi/test.scm`** - Minimal test framework
5. **Test:** `(import (chibi test)) (test 4 (+ 2 2))`

**Want me to start implementing these?**
