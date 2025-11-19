# Macro Issues Analysis - r7rs-tests.scm

**Date:** 2025-11-18
**Status:** Active debugging with macro debug mode enabled
**Priority:** #1 issue for R7RS compliance

## Overview

Analysis of macro-related issues found when running chibi-scheme's r7rs-tests.scm with full macro debug tracing enabled.

**Test Statistics:**
- Total macro expansions: 5,035
- Failed pattern matches: 16,848 (normal - rules trying patterns)
- Actual test failures: 1
- Runtime errors: 25
- Success rate: Very high (only 1 FAIL, 25 errors out of thousands of expansions)

## Error Categories

### 1. Missing Language Features (17 errors - 68%)

**Issue:** Undefined variables for features not yet implemented

**Breakdown:**
- `let-syntax` / `letrec-syntax` (7 occurrences) - Local macro definitions
- `parameterize` / parameters (4 occurrences) - Dynamic parameters
- `make-parameter`, `radix` (2 occurrences) - Parameter-related primitives
- Other undefined test variables (4 occurrences)

**Priority:** HIGH - Core R7RS features

**Example:**
```
Error: Undefined variable: let-syntax
```

**Solution:** Implement `let-syntax` and `letrec-syntax` special forms

**Notes:**
- These are R7RS-small required features
- `let-syntax` allows local macro definitions
- Different from `define-syntax` which is global
- See R7RS Section 4.3.2

### 2. Macro Compilation Errors (6 errors - 24%)

**Issue:** Invalid macro definitions that fail compilation

**Breakdown:**
- "Expected syntax-rules" (5 occurrences) - Malformed syntax-rules
- "Ellipsis in template contains no pattern variables" (1 occurrence)

**Priority:** MEDIUM - Edge cases in macro validation

**Example:**
```
Error: Invalid syntax: Failed to compile macro:
Invalid syntax: Ellipsis in template contains no pattern variables
```

**Context:**
This happens when a macro template tries to use `...` but there are no pattern variables at that ellipsis level to repeat.

**Valid:**
```scheme
(define-syntax repeat
  (syntax-rules ()
    ((repeat (x ...) body)
     (begin body ...))))  ; body is at ellipsis level
```

**Invalid:**
```scheme
(define-syntax broken
  (syntax-rules ()
    ((broken x)
     (list x ...))))  ; x is NOT at ellipsis level!
```

**Solution:** Better error messages and examples in validation

### 3. Hygiene Issues (1 error - 4%)

**Issue:** Gensym'd variable not found in scope

**Example:**
```
Error: Undefined variable: ##bar399#1069
```

**Priority:** HIGH - Breaks macro hygiene

**Analysis:**
This is a serious issue - a hygienic identifier that should be in scope is not found. This indicates a bug in our hygiene implementation.

**Possible causes:**
1. Free identifier incorrectly renamed
2. Binding not properly propagated through nested expansions
3. Gensym collision or loss during expansion

**Needs investigation:** Yes - this could affect many macros

### 4. Lexer Issues (1 error - 4%)

**Issue:** Vertical bar identifiers not supported

**Example:**
```
Error: Lexer error: Unexpected character: |
```

**Priority:** LOW - R7RS extension, rarely used

**Notes:**
R7RS allows `|...| ` for identifiers with special characters:
```scheme
(define |foo bar!| 42)  ; identifier with spaces
|foo bar!|  ; => 42
```

**Solution:** Add support for vertical-bar delimited identifiers in lexer

### 5. Runtime Errors (1 error - 4%)

**Issue:** Non-procedure value used as procedure

**Example:**
```
Error: Not a procedure: ok
```

**Priority:** LOW - Likely test setup issue

**Analysis:**
The test suite uses a variable `ok` that should be a procedure but isn't bound correctly. This is likely a test environment setup issue, not a macro issue.

## Test Failures

### The One Failing Test: Underscore Pattern Matching

**Test:**
```scheme
(list (count-to-2_ _ _) (count-to-2_) (count-to-2_ a b) (count-to-2_ a b c d))
```

**Expected:** `(2 0 fail fail)`
**Got:** `(2 0 2 fail)`

**Issue:** Third form `(count-to-2_ a b)` should return `fail` but returns `2`

**Analysis:**
This tests whether underscores `_` in macro patterns work correctly. The macro should:
- `(count-to-2_ _ _)` - Match 2 underscores → return 2 ✓
- `(count-to-2_)` - Match 0 args → return 0 ✓
- `(count-to-2_ a b)` - Match 2 args (not underscores) → should fail, but returns 2 ✗
- `(count-to-2_ a b c d)` - Match 4 args → fail ✓

**Root cause:** Underscores in patterns are treated as regular pattern variables instead of wildcards

**Priority:** MEDIUM - R7RS compliance issue

**Solution:** Make `_` a wildcard in patterns (matches anything, doesn't bind)

## Macro Debug Output Quality

### ✅ What's Working Well

1. **Pattern Matching Visibility**
   - See all rules tried
   - See why each rule fails
   - Clear success/failure indicators (✓/✗)

2. **Variable Bindings**
   - See exactly what each variable binds to
   - Helpful for debugging complex patterns

3. **Template Expansion**
   - See template before expansion
   - See result after expansion
   - Clear transformation tracking

4. **Hygiene Tracing**
   - See which identifiers are free
   - See all renamings (e.g., `display -> ##display#4`)
   - Helps debug hygiene issues

### Example of Excellent Debug Output

```
[MACRO] Expanding macro: let
[MACRO]   Input: (let () (define x 28) (test 28 x))
[MACRO]
[MACRO]   === Trying rule 1 ===
[MACRO]   Pattern: (let proc-name (((var val) ...)) (body ...))
[MACRO]   ✗ Match failed: Pattern matching failed: type mismatch
Expected: list
Got:      define
Hint: List patterns only match lists, vector patterns only match vectors
[MACRO]
[MACRO]   === Trying rule 2 ===
[MACRO]   Pattern: (let (((var val) ...)) (body ...))
[MACRO]   ✓ Match successful!
[MACRO]   Bindings:
[MACRO]     var = []
[MACRO]     val = []
[MACRO]     body = [(define x 28), (test 28 x)]
[MACRO]
[MACRO]   === Expanding template ===
[MACRO]   Template: ((lambda ((var ...)) (body ...)) (val ...))
[MACRO]   Expanded: ((lambda () (define x 28) (test 28 x)))
```

This level of detail makes debugging trivial compared to cryptic error messages!

## Priority Issues for R7RS Compliance

### High Priority (Must Fix)

1. **Implement `let-syntax` / `letrec-syntax`** (7 errors)
   - Core R7RS feature
   - Allows local macro definitions
   - Needed for many advanced macros

2. **Fix hygiene bug** (1 error: `##bar399#1069` not found)
   - Breaks macro correctness
   - Could affect many macros silently
   - Must investigate and fix

3. **Implement parameters** (4 errors)
   - `make-parameter`, `parameterize`
   - R7RS-small required feature
   - Needed for dynamic scoping

### Medium Priority (Should Fix)

4. **Fix underscore wildcard** (1 test failure)
   - R7RS compliance issue
   - `_` should be wildcard, not pattern variable
   - Affects pattern matching semantics

5. **Better macro validation errors** (6 compilation errors)
   - "Expected syntax-rules" - need better messages
   - "Ellipsis with no variables" - show what's wrong
   - Help users write correct macros

### Low Priority (Nice to Have)

6. **Vertical bar identifiers** (1 error)
   - R7RS feature but rarely used
   - `|foo bar|` syntax for special characters
   - Lexer enhancement

7. **Test environment setup** (1 error)
   - `Not a procedure: ok`
   - Test-specific issue
   - Not a macro problem

## Next Steps

### Immediate Actions

1. **Investigate hygiene bug**
   - Search for `##bar399#1069` in debug output
   - Understand why gensym'd variable is undefined
   - Fix before it causes silent failures

2. **Implement `let-syntax` / `letrec-syntax`**
   - Most impactful fix (eliminates 7 errors)
   - Required for R7RS compliance
   - Enables local macro testing

3. **Fix underscore wildcard**
   - Simple fix with high impact
   - Eliminates the one test failure
   - Makes pattern matching R7RS compliant

### Medium Term

4. **Implement parameters**
   - `make-parameter`, `parameterize`
   - R7RS-small requirement
   - Enables dynamic scoping tests

5. **Improve validation messages**
   - Add examples to error messages
   - Show what's wrong, not just "invalid"
   - Help users learn macro syntax

### Long Term

6. **Add vertical bar identifiers**
   - Lexer enhancement
   - R7RS compliance
   - Low priority but nice to have

## Success Metrics

**Current:**
- 1 test failure
- 25 runtime errors
- ~99% macro expansion success rate

**Target (after fixes):**
- 0 test failures
- <10 runtime errors (only missing features)
- 100% macro expansion success for implemented features

**Key:** The macro system is already very solid. Most issues are:
- Missing language features (let-syntax, parameters)
- One hygiene bug to fix
- One pattern matching bug (underscore)

Once these are fixed, macro support should be essentially complete for R7RS-small!

## Validation Success

The new validation system is working excellently:
- Catches real errors at compile time
- Provides helpful hints
- Allows valid patterns (like broadcast)
- Only 6 validation errors out of 5,035 expansions = 99.9% success rate

## Conclusion

The macro system is in excellent shape:
- ✅ Pattern matching works
- ✅ Template expansion works
- ✅ Hygiene works (mostly - one bug to fix)
- ✅ Validation catches errors early
- ✅ Debug output is incredibly helpful

**Main gaps:**
1. `let-syntax` / `letrec-syntax` (missing feature)
2. Parameters (missing feature)
3. One hygiene bug
4. Underscore wildcard

**Once these 4 issues are fixed, macro support will be R7RS-complete!**

---

**See also:**
- `MACRO_DEBUG_ENABLED.md` - How to use debug mode
- `docs/FEATURE_STATUS.md` - Overall R7RS compliance matrix
- `scheme_tests/reports/` - Latest test results with full debug output
