# Running chibi-scheme's R7RS Test Suite

## Goal

Run chibi-scheme's comprehensive R7RS test suite (`tests/r7rs-tests.scm`) against Patina to validate R7RS compliance.

**Test File:** `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`
**Size:** 2,516 lines covering entire R7RS-small specification
**Coverage:** All procedures, syntax, and standard libraries

## Current Blockers

### 1. Standard Libraries Not Defined

The test suite requires these imports:
```scheme
(import (scheme base)            ; ❌ Not defined
        (scheme char)            ; ❌ Not defined
        (scheme lazy)            ; ❌ Not defined
        (scheme inexact)         ; ❌ Not defined
        (scheme complex)         ; ❌ Not defined
        (scheme time)            ; ❌ Not defined
        (scheme file)            ; ❌ Not defined
        (scheme read)            ; ❌ Not defined
        (scheme write)           ; ❌ Not defined
        (scheme eval)            ; ❌ Not defined
        (scheme process-context) ; ❌ Not defined
        (scheme case-lambda)     ; ❌ Not defined
        (scheme r5rs)            ; ❌ Not defined
        (chibi test))            ; ❌ Not available
```

**Current state:** All primitives available globally (R5RS style), but not organized into libraries.

### 2. Testing Framework Not Available

Requires either:
- `(chibi test)` - chibi's testing library
- `(srfi 64)` - Standard Scheme test framework

**Option 1:** Port `(chibi test)` to Patina
**Option 2:** Implement minimal `(srfi 64)` subset
**Option 3:** Create simple test shim

## Phased Implementation Plan

### Phase 1: Minimal Test Infrastructure (Quick Win)

**Goal:** Run chibi tests with minimal setup

**Steps:**
1. Create simple test shim (avoid full SRFI-64 for now)
   ```scheme
   ;; lib/test-shim.sld
   (define-library (test-shim)
     (export test-begin test-end test)
     (import (scheme base) (scheme write))
     (begin
       (define (test-begin name)
         (display "Test suite: ") (display name) (newline))
       (define (test-end . args) #t)
       (define-syntax test
         (syntax-rules ()
           ((test expected expr)
            (let ((result expr))
              (if (equal? result expected)
                  (begin (display "  PASS: ") (write 'expr) (newline))
                  (begin (display "  FAIL: ") (write 'expr)
                         (display " expected ") (write expected)
                         (display " got ") (write result) (newline)))))))))
   ```

2. Define `(scheme base)` as Rust library with all current primitives
   - Create `RustLibraryBuilder` function
   - Register in `RustLibraryLoader::with_standard_libraries()`
   - Export all implemented primitives

3. Stub out other required libraries
   ```scheme
   (scheme char)            → Re-export char functions from (scheme base)
   (scheme lazy)            → Empty (not implemented yet)
   (scheme inexact)         → Re-export numeric functions
   (scheme complex)         → Re-export complex functions
   (scheme time)            → Empty (not implemented)
   (scheme file)            → Empty (not implemented)
   (scheme read)            → Empty (not implemented)
   (scheme write)           → Re-export write/display
   (scheme eval)            → Empty (not implemented)
   (scheme process-context) → Empty (not implemented)
   (scheme case-lambda)     → Empty (not implemented)
   (scheme r5rs)            → Re-export everything from (scheme base)
   ```

4. Modify test file to use our shim instead of `(chibi test)`

**Result:** Can run test suite, see which tests pass/fail

**Effort:** 2-3 hours

### Phase 2: Organize Standard Libraries (Proper Implementation)

**Goal:** Proper R7RS library organization

**Steps:**

1. **Split primitives by library** (Reference: R7RS Section 6)

   **`(scheme base)`** - Core library (~100 exports)
   - Arithmetic: `+`, `-`, `*`, `/`, `<`, `>`, `=`, etc.
   - Lists: `car`, `cdr`, `cons`, `list`, `append`, `map`, `for-each`, etc.
   - Predicates: `number?`, `pair?`, `null?`, `boolean?`, etc.
   - Control: `if`, `cond`, `case`, `and`, `or`, `when`, `unless`
   - Definitions: `define`, `lambda`, `let`, `let*`, `letrec`, etc.
   - I/O: `display`, `write`, `newline` (basic)
   - Strings: `string`, `string-append`, `string-ref`, etc.
   - Vectors: `vector`, `vector-ref`, `vector-set!`, etc.

   **`(scheme char)`** - Character operations
   - `char-alphabetic?`, `char-numeric?`, `char-whitespace?`
   - `char-upcase`, `char-downcase`, `char-foldcase`
   - `digit-value`, `string-upcase`, `string-downcase`

   **`(scheme cxr)`** - cXXXr procedures
   - `caaar`, `caadr`, ..., `cddddr` (all 24 combinations)

   **`(scheme write)`** - Output
   - `display`, `write`, `write-shared`, `write-simple`

   **`(scheme read)`** - Input
   - `read`

   **`(scheme file)`** - File operations
   - `call-with-input-file`, `call-with-output-file`
   - `with-input-from-file`, `with-output-to-file`

2. **Create library builders**
   ```rust
   // crates/patina-tree-walker/src/stdlib/scheme_base.rs
   pub fn build_scheme_base(name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
       // Install all (scheme base) primitives
       env.define("+", Value::Primitive(...));
       env.define("-", Value::Primitive(...));
       // ... all base library primitives

       // Return export list
       vec!["+", "-", "*", "/", "car", "cdr", ...].iter()
           .map(|s| s.to_string()).collect()
   }
   ```

3. **Register in RustLibraryLoader**
   ```rust
   impl RustLibraryLoader {
       pub fn with_standard_libraries() -> Self {
           let mut loader = Self::new();
           loader.register(vec!["scheme", "base"], build_scheme_base);
           loader.register(vec!["scheme", "char"], build_scheme_char);
           loader.register(vec!["scheme", "cxr"], build_scheme_cxr);
           // ... etc
           loader
       }
   }
   ```

**Result:** Proper R7RS library organization

**Effort:** 1-2 days

### Phase 3: Implement Missing Features

As tests run, implement missing features:

**High Priority:**
- `(scheme read)` - `read` procedure
- `(scheme write)` - Full write functionality
- `(scheme file)` - File I/O
- `(scheme process-context)` - Command-line args, environment, exit
- `(scheme case-lambda)` - Variable arity lambdas
- `(scheme eval)` - `eval` procedure

**Medium Priority:**
- `(scheme lazy)` - Promises and delays
- `(scheme time)` - Current time functions

**Lower Priority:**
- Features not heavily used in tests

**Effort:** Ongoing, 1-3 weeks

### Phase 4: Full SRFI-64 or chibi test

Implement proper testing framework for better output and diagnostics.

**Effort:** 3-5 days

## Immediate Next Steps

### Option A: Quick Prototype (Recommended)

1. Create minimal `(scheme base)` library - 1 hour
2. Create test shim - 30 minutes
3. Try running a subset of tests - 30 minutes
4. See what breaks, iterate

**Total:** ~2-3 hours to first test results

### Option B: Proper Implementation

1. Plan library organization - 1 hour
2. Implement all standard libraries - 1-2 days
3. Run tests - 1 hour

**Total:** 1-2 days to first test results

## Files to Create

### Minimal Approach (Option A)

```
lib/scheme/base.sld          # Minimal (scheme base) that re-exports globals
lib/test-shim.sld            # Simple test framework
tests/run_chibi_tests.sh     # Script to run modified chibi tests
```

### Proper Approach (Option B)

```
crates/patina-tree-walker/src/stdlib/
  mod.rs                     # Module exports
  scheme_base.rs             # (scheme base) builder
  scheme_char.rs             # (scheme char) builder
  scheme_cxr.rs              # (scheme cxr) builder
  scheme_write.rs            # (scheme write) builder
  ... etc

lib/scheme/
  base.sld                   # Optional: Scheme extensions to base
  char.sld                   # Optional: Scheme extensions
  ... etc
```

## Success Criteria

**Phase 1 Complete:**
- Can execute: `patina tests/r7rs-tests.scm`
- See test output (pass/fail counts)
- Identify missing features

**Phase 2 Complete:**
- All standard libraries properly defined
- Can import libraries correctly
- Tests use real library system

**Phase 3 Complete:**
- >80% of chibi tests passing
- All core R7RS features working
- Known failures documented

**Final Success:**
- >95% of chibi r7rs-tests.scm passing
- Remaining failures are acceptable differences (e.g., implementation-defined behavior)

## Recommendation

**Start with Option A (Quick Prototype):**

1. Spend 2-3 hours to get *something* running
2. Learn what's actually needed vs. theoretical requirements
3. Then decide whether to go back and do proper library organization

This follows the principle: "Make it work, make it right, make it fast"

Would you like me to start with the minimal approach and create:
1. A simple `(scheme base)` library
2. A test shim
3. Try running a small subset of chibi tests?
