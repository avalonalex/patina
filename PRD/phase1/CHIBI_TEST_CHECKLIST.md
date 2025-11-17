# Chibi R7RS Test Compliance Checklist

**Last Updated:** 2025-11-16
**Current Status:** 85/126 tests passing (67.5%)
**Goal:** 100% chibi r7rs-tests.scm compliance

## Current Progress

- ✅ **85 passing** (67.5%)
- ❌ **0 failing** (proper failures)
- ⚠️ **41 errors** (32.5% - crashes before assertions)

## Remaining Work (41 errors to fix)

### High Priority (Quick Wins)

#### 1. String/Number Conversion Primitives (~2+ tests)
**Effort:** 2-4 hours
**Impact:** High (commonly used)

- [ ] `number->string` - Convert number to string with optional radix
- [ ] `string->number` - Parse string to number with optional radix
- [ ] Add to `primitives/strings.rs` or create `primitives/conversion.rs`
- [ ] Tests in `crates/patina-tests/tests/compliance/strings.rs`

**R7RS Reference:** Section 6.6 (Characters), 6.2.6 (Numerical operations)

---

#### 2. let-syntax / letrec-syntax (~5+ tests)
**Effort:** 1-2 days
**Impact:** High (macro system completeness)

- [ ] Implement `let-syntax` - Local macro definitions
- [ ] Implement `letrec-syntax` - Recursive local macros
- [ ] Add to `special_forms.rs` or `macro_expander`
- [ ] Handle hygiene for locally-scoped macros
- [ ] Tests in `crates/patina-tests/tests/compliance/macros.rs`

**R7RS Reference:** Section 4.3.1 (Binding constructs for syntactic keywords)

**Notes:**
- Similar to `let`/`letrec` but for macros instead of values
- Need to extend macro expander environment model
- Macros should be locally scoped and shadowed properly

---

#### 3. (scheme case-lambda) Library (~3+ tests)
**Effort:** 4-6 hours
**Impact:** Medium (dedicated library)

- [ ] Implement `case-lambda` special form/macro
- [ ] Pattern match on argument count and dispatch
- [ ] Create `lib/scheme/case-lambda-extras.scm` with macro
- [ ] Create `crates/patina-runtime/src/stdlib/scheme_case_lambda.rs`
- [ ] Register library in system
- [ ] Tests in `crates/patina-tests/tests/case_lambda.rs`

**R7RS Reference:** Section 4.2.9, R7RS library (scheme case-lambda)

**Implementation approach:**
```scheme
(define-syntax case-lambda
  (syntax-rules ()
    ((case-lambda
      (formals body ...) ...)
     ;; Dispatch based on argument count
     )))
```

---

### Medium Priority

#### 4. Parameter Objects (~2+ tests)
**Effort:** 6-8 hours
**Impact:** Medium (dynamic scoping)

- [ ] Add `Parameter` variant to `Value` enum
- [ ] Implement `make-parameter` primitive
- [ ] Implement `parameterize` special form
- [ ] Handle dynamic binding with proper scoping/unwinding
- [ ] Tests in `crates/patina-tests/tests/compliance/parameters.rs`

**R7RS Reference:** Section 4.2.6 (Dynamic bindings)

**Notes:**
- Parameters are like dynamic variables (different from lexical bindings)
- `parameterize` creates temporary binding that unwinds on exit
- Need to maintain parameter binding stack

---

### Lower Priority (Edge Cases)

#### 5. Escaped Identifiers in Lexer (~1 test)
**Effort:** 2-3 hours
**Impact:** Low (rare usage)

- [ ] Support `|identifier with spaces|` syntax
- [ ] Modify lexer to handle vertical bars
- [ ] Allow any characters except `|` inside
- [ ] Tests in `crates/patina-tests/tests/compliance/symbols.rs`

**R7RS Reference:** Section 2.1 (Identifiers), Section 7.1.1 (Lexical structure)

---

#### 6. Macro System Edge Cases (~6 tests)
**Effort:** Variable (investigation needed)
**Impact:** Low (edge cases)

**Issues to investigate:**
- [ ] 5 "Invalid syntax: Expected syntax-rules" errors - validation too strict?
- [ ] 1 nested ellipsis limitation (already documented in `NESTED_ELLIPSIS_LIMITATION.md`)
- [ ] 1 gensym hygiene bug (`##bar399#1069` - collision or scope issue?)

**Action items:**
- [ ] Review error messages from chibi test output
- [ ] Identify specific test cases failing
- [ ] Determine if bugs or unimplemented edge cases
- [ ] Fix or document as known limitations

---

#### 7. Test Infrastructure Issues (~20+ tests)
**Effort:** Will be resolved by implementing case-lambda!
**Impact:** HIGH - These tests depend on case-lambda

**Undefined variables in tests:**
- `any-arity`, `rest-arity`, `dead-clause`
- `sequence1`, `sequence2`, `sequence3`
- `part-2x`, `mad-hatter`
- Others...

**Root cause identified:**
These are test helper functions defined using `case-lambda` in the test suite:
```scheme
(define any-arity
  (case-lambda
    (() 'zero)
    ((x) x)
    ((x y) (cons x y))
    ((x y z) (list x y z))
    (args (cons 'many args))))
```

**Resolution:**
- ✅ Not a test infrastructure issue!
- ✅ These tests will pass once we implement `case-lambda` (item #3)
- ✅ This increases the priority of case-lambda implementation

**Impact:** Implementing case-lambda will likely unlock most/all of these ~20 tests!

---

## Implementation Order (Recommended)

**Priority 1: Maximum Impact (1-2 days)**
1. **case-lambda library** (4-6 hours) ⭐⭐⭐
   - Will unlock ~20+ test helper functions!
   - Expected: ~23 tests unlocked (current 3 + 20 helpers)
2. **String/number conversion** (2-4 hours) ⭐⭐⭐
   - Quick win, commonly used primitives
   - Expected: ~2 tests unlocked

**Priority 2: Core Language Features (3-4 days)**
3. **let-syntax / letrec-syntax** (1-2 days) ⭐⭐⭐
   - Essential for macro system completeness
   - Expected: ~5 tests unlocked
4. **Parameter objects** (6-8 hours) ⭐⭐
   - Dynamic scoping support
   - Expected: ~2 tests unlocked

**Priority 3: Edge Cases (1-2 days)**
5. **Escaped identifiers** (2-3 hours) ⭐
   - Expected: ~1 test unlocked
6. **Macro edge cases** (variable effort) ⭐
   - Fix syntax-rules validation issues
   - Expected: ~5 tests unlocked

**Expected outcome after all items:** 110+ passing tests (85 + ~25 = 110+/126 = 87%+)
**Stretch goal:** 95%+ compliance (120+/126 tests)

---

## Current Gaps Summary

| Feature | Tests Blocked | Effort | Priority |
|---------|---------------|--------|----------|
| **case-lambda** | **~23** (3 direct + 20 helpers!) | Low | ⭐⭐⭐ |
| let-syntax/letrec-syntax | ~5 | Medium | ⭐⭐⭐ |
| number->string/string->number | ~2 | Low | ⭐⭐⭐ |
| Parameter objects | ~2 | Medium | ⭐⭐ |
| Macro edge cases | ~6 | Variable | ⭐ |
| Escaped identifiers | ~1 | Low | ⭐ |

**Total remaining:** ~39 tests (41 current - 2 already counted in case-lambda helpers)

---

## Tracking Progress

After each feature implementation:
1. Run `./scripts/run_chibi_tests.sh`
2. Update this checklist with new passing count
3. Remove completed items
4. Re-categorize remaining errors

**Goal:** Check off all items and reach 100% compatibility! 🎯
