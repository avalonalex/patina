# R7RS Macro System Analysis

**Date:** 2025-11-08
**Status:** Research Complete - Week 1
**Next:** Chibi implementation analysis

---

## Executive Summary

Based on detailed analysis of the R7RS specification (Section 4.3) and chibi-scheme test suite, here are the key findings for implementing R7RS-compliant hygienic macros.

---

## Part 1: Core Requirements from R7RS

### 1.1 Fundamental Concepts

**Macro:** A program-defined expression type with syntax `(keyword datum ...)`

**Transformer:** The set of rules that specifies how a macro use is transcribed

**Hygiene:** Two mechanisms that prevent unintended conflicts:
1. **Identifier renaming:** If a macro inserts a binding, the identifier is renamed to avoid conflicts
2. **Lexical preservation:** If a macro inserts a free reference, it refers to the binding visible where the transformer was specified

### 1.2 Required Forms

#### `define-syntax` (Global macro binding)
```scheme
(define-syntax when
  (syntax-rules ()
    ((when test result1 result2 ...)
     (if test (begin result1 result2 ...)))))
```

#### `syntax-rules` (Pattern-based transformer)
```scheme
(syntax-rules (literal ...)
  (pattern template)
  ...)
```

#### `let-syntax` (Local macro binding - Optional for our 3 tests)
```scheme
(let-syntax ((keyword transformer) ...)
  body)
```

#### `letrec-syntax` (Recursive local macros - Optional)
Similar to `let-syntax` but allows self-reference

---

## Part 2: Pattern Matching Specification

### 2.1 Pattern Forms

A **pattern** can be:
1. **Identifier** - Can be:
   - `_` (underscore) - Matches anything, not a pattern variable
   - Literal identifier (from literals list) - Must match exactly
   - Pattern variable - Matches anything and captures it

2. **Constant** - Matches via `equal?`

3. **List patterns:**
   ```scheme
   (pattern ...)                              ; Fixed-length list
   (pattern ... . pattern)                     ; Improper list
   (pattern ... pattern ellipsis pattern ...)  ; With repetition
   ```

4. **Vector patterns:**
   ```scheme
   #(pattern ...)
   #(pattern ... pattern ellipsis pattern ...)
   ```

### 2.2 Pattern Matching Rules

**Formal matching definition** from R7RS:

Input expression `E` matches pattern `P` if and only if:

1. `P` is underscore (`_`) - Always matches
2. `P` is non-literal identifier - Always matches (pattern variable)
3. `P` is literal identifier AND `E` has same binding
4. `P` is list `(P1 ... Pn)` AND `E` is list of n elements matching each
5. `P` is improper list AND `E` matches with tail matching
6. **`P` has ellipsis** - Complex case (see below)
7. `P` is vector AND `E` matches element-wise
8. `P` is constant AND `E` equals it via `equal?`

### 2.3 Ellipsis (`...`) Handling

**Critical feature:** Ellipsis allows zero-or-more repetition

Example pattern: `(P1 ... Pk Pe ellipsis Pm+1 ... Pn)`
- First k elements match P1...Pk
- Next (m-k) elements each match Pe
- Remaining (n-m) elements match Pm+1...Pn

**Key insight:** The ellipsis can capture zero or more occurrences!

---

## Part 3: Template Expansion Specification

### 3.1 Template Forms

A **template** can be:
1. **Identifier:**
   - Pattern variable - Replaced by matched value
   - Ellipsis - Special marker
   - Literal identifier - Inserted (with hygiene)

2. **Constant** - Inserted as-is

3. **List templates:**
   ```scheme
   (element ...)
   (element ... . template)
   (ellipsis template)  ; Escape - template treated literally
   ```

4. **Vector templates:**
   ```scheme
   #(element ...)
   ```

where `element` = `template` optionally followed by `ellipsis`

### 3.2 Expansion Rules

**Pattern variable substitution:**
- Simple pattern variables → replaced by matched value
- Pattern variables with ellipsis → replaced by ALL matched values

**Critical constraint:** Pattern variables occurring in subpatterns with N ellipses can only appear in subtemplates with N ellipses!

**Literal identifier insertion:**
- Free identifier → refers to binding where `syntax-rules` appears
- Bound identifier → renamed to prevent capture (HYGIENE!)

### 3.3 Ellipsis Escaping

Special form: `(ellipsis template)` - ellipses in template have no meaning

Example:
```scheme
(define-syntax be-like-begin
  (syntax-rules ()
    ((be-like-begin name)
     (define-syntax name
       (syntax-rules ()
         ((name expr (... ...))    ; (... ...) escapes ellipsis!
          (begin expr (... ...)))))))
```

Result: `(... ...)` in template produces actual `...` in output

---

## Part 4: Hygiene Requirements

### 4.1 Hygiene Examples from R7RS

**Test case 1:** Local variable shadowing doesn't break macro
```scheme
(let ((=> #f))
  (cond (#t => 'ok)))  ; => 'ok (not error!)
```

**Why this works:**
- `cond` macro recognizes `=>` as local variable (not keyword)
- Expands to: `(if #t (begin => 'ok))` not `(if #t ('ok #t))`

**Test case 2:** Macro-inserted bindings are renamed
```scheme
(let-syntax ((when (syntax-rules ()
                     ((when test stmt1 stmt2 ...)
                      (if test (begin stmt1 stmt2 ...))))))
  (let ((if #t))
    (when if (set! if 'now))
    if))  ; => now
```

**Why this works:**
- The `if` in the macro template is DIFFERENT from user's `if` variable
- Macro's `if` refers to global `if`, user's `if` is local variable

**Test case 3:** Free references use lexical scope
```scheme
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    (let ((x 'inner))
      (m))))  ; => outer (not inner!)
```

**Why this works:**
- Macro's `x` refers to binding visible where `syntax-rules` was defined
- NOT the binding at use site!

### 4.2 Hygiene Algorithm Hints

From the spec, we need:
1. **Rename introduced bindings** - Each macro expansion gets fresh names
2. **Preserve lexical references** - Free vars refer to definition-site bindings
3. **Detect literal vs. binding** - `=>` as literal vs variable

**Implementation strategy (to investigate):**
- Attach "marks" or "scope sets" to identifiers
- Track binding context for each identifier
- Rename on conflict

---

## Part 5: Test Cases from Specification

### 5.1 Basic Macro

```scheme
(define-syntax when
  (syntax-rules ()
    ((when test result1 result2 ...)
     (if test (begin result1 result2 ...)))))

(when #t 1 2 3)  ; => 3
```

**Tests:**
- ✅ Simple pattern matching
- ✅ Ellipsis in pattern
- ✅ Ellipsis in template
- ✅ Basic expansion

### 5.2 Recursive Macro with Ellipsis

```scheme
(letrec-syntax
  ((my-or (syntax-rules ()
            ((my-or) #f)
            ((my-or e) e)
            ((my-or e1 e2 ...)
             (let ((temp e1))
               (if temp temp (my-or e2 ...)))))))
  (my-or #f #f 7 #f))  ; => 7
```

**Tests:**
- ✅ Multiple rules (pattern matching tries each)
- ✅ Recursive macro expansion
- ✅ Hygiene (temp doesn't capture)

### 5.3 Hygiene Test

```scheme
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    (let ((x 'inner))
      (m))))  ; => outer
```

**Tests:**
- ✅ Free variable hygiene
- ✅ Lexical scoping preservation

### 5.4 Literal Matching

```scheme
(define-syntax cond
  (syntax-rules (else =>)
    ...))

(let ((=> #f))
  (cond (#t => 'ok)))  ; => ok
```

**Tests:**
- ✅ Literal identifier detection
- ✅ Local binding doesn't match literal

### 5.5 Ellipsis Escaping

```scheme
(define-syntax be-like-begin
  (syntax-rules ()
    ((be-like-begin name)
     (define-syntax name
       (syntax-rules ()
         ((name expr (... ...))
          (begin expr (... ...))))))))

(be-like-begin sequence)
(sequence 1 2 3)  ; => 3
```

**Tests:**
- ✅ Nested macro definition
- ✅ Ellipsis escaping with `(... ...)`

---

## Part 6: Chibi-Scheme Test Suite Analysis

### 6.1 Key Test Patterns

From `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`:

**1. Underscore pattern** (line 497-507)
```scheme
(define-syntax underscore
  (syntax-rules ()
    ((underscore _ _) 'ok)))

(underscore 1 2)  ; => ok
```

**2. Pattern with ellipsis** (line 508-515)
```scheme
(define-syntax count-to-2
  (syntax-rules ()
    ((_ a b) 'ok)))

(count-to-2 1 2)  ; => ok
```

**3. Part matching** (line 477-495)
Multiple patterns, first matching wins

### 6.2 Edge Cases to Test

1. **Empty ellipsis match:** `((when test) (if test #t))` - test with 0 body forms
2. **Nested ellipsis:** `((x ...) ...)` - matrix patterns
3. **Improper lists:** `(a b . rest)` patterns
4. **Vector patterns:** `#(a b c ...)`
5. **Literal in different position:** `(op => result)` vs `(test => proc)`

---

## Part 7: Implementation Phases (Revised)

Based on spec analysis, here's the implementation order:

### Phase 1: Pattern Matching (Week 1-2)
**Goal:** Match patterns against input without expansion

**Tasks:**
1. Define `Pattern` enum (literals, variables, lists, ellipsis, vectors)
2. Implement `match_pattern(pattern, input) -> Option<Bindings>`
3. Handle ellipsis (0-or-more matching)
4. Handle underscore (wildcard)
5. **Test:** Verify all R7RS matching rules work

**Success criteria:**
- Can match `(when test body ...)` against `(when #t 1 2 3)`
- Captures: `test` = `#t`, `body` = `[1, 2, 3]`
- Handles empty ellipsis: `(when #t)` with 0 body forms

### Phase 2: Template Expansion (Week 2-3)
**Goal:** Generate output from template + bindings

**Tasks:**
1. Define `Template` enum (variables, lists, ellipsis, escape)
2. Implement `expand_template(template, bindings) -> Value`
3. Handle pattern variable substitution
4. Handle ellipsis expansion (repeat for each captured value)
5. Handle ellipsis escaping `(... template)`
6. **Test:** Verify template expansion without hygiene

**Success criteria:**
- Can expand `(if test (begin body ...))` with bindings
- Produces correct AST
- Handles `(... ...)` escaping

### Phase 3: Basic Non-Hygienic Macros (Week 3-4)
**Goal:** Working `define-syntax` + `syntax-rules` without hygiene

**Tasks:**
1. Add `Macro` to `Value` enum
2. Implement `define-syntax` special form
3. Integrate macro expansion into evaluator
4. Separate macro environment from runtime environment
5. **Test:** `when`, `unless` work (may fail hygiene tests)

**Success criteria:**
- 3 ignored tests pass (when, unless, do)
- Basic macros work
- May have hygiene violations (acceptable for this phase)

### Phase 4: Hygiene (Week 4-6)
**Goal:** Full R7RS hygiene

**Tasks:**
1. Research hygiene algorithms (syntactic closures vs marks)
2. Implement identifier renaming
3. Track macro invocation scopes
4. Distinguish literal vs binding identifiers
5. **Test:** All hygiene tests pass

**Success criteria:**
- Pass all R7RS hygiene tests
- Variable capture impossible
- Free references work correctly

---

## Part 8: Key Insights

### 8.1 Design Decisions

**1. When to expand macros?**
- **Recommendation:** Expand at parse time (before eval)
- Alternative: Lazy expansion during eval
- **Reason:** Cleaner separation, easier debugging

**2. How to represent macros?**
```rust
enum Value {
    // ...
    Macro {
        name: String,
        literals: Vec<String>,
        rules: Vec<(Pattern, Template)>,
    }
}
```

**3. Separate macro environment?**
- **YES** - Macros live in different namespace than values
- Store in parallel to runtime environment
- Lookup order: Check macro env first, then value env

### 8.2 Critical Implementation Details

**1. Pattern matching is ORDERED**
- Try rules left-to-right
- First match wins
- This allows "base case then recursive" patterns

**2. Ellipsis depth must match**
- Pattern: `(... (x ...) ...)` - depth 2
- Template: Must have 2 ellipses too: `(... (x ...) ...)`
- **Error** if depths don't match!

**3. Literal matching uses binding equality**
- NOT string equality!
- `(let ((=> #f)) ...)` - `=>` is different binding than keyword `=>`

### 8.3 Minimal Implementation for Our 3 Tests

For `when`, `unless`, `do` - we need:
- ✅ `define-syntax` (global macros)
- ✅ `syntax-rules` (pattern/template)
- ✅ Ellipsis (`...`) in patterns and templates
- ✅ Basic hygiene (renamed bindings)
- ⚠️ `let-syntax` - NOT needed for our tests
- ⚠️ `letrec-syntax` - NOT needed for our tests
- ⚠️ Full hygiene - Partial OK initially

**Aggressive timeline:** 3 weeks for 99% pass rate!

---

## Part 9: Next Steps

### Week 2 Tasks (Next):
1. ✅ Read R7RS Section 4.3 - DONE
2. ✅ Study macro examples - DONE
3. ✅ List required behaviors - DONE
4. **TODO:** Create test cases from spec
5. **TODO:** Analyze chibi-scheme implementation (C code)

### Week 2 Focus:
- Examine chibi's C implementation of pattern matching
- Document their hygiene strategy
- Design our Pattern/Template data structures
- Create minimal test suite

---

## Appendix A: R7RS Section 4.3 Summary

**Location:** `~/Project/patina/spec/r7rs-small-spec/expr.tex:1443-1850`

**Key sections:**
1. **Macros intro** (1443-1480) - Hygiene definition
2. **Binding constructs** (1481-1590) - `let-syntax`, `letrec-syntax`
3. **Pattern language** (1591-1800) - `syntax-rules` specification
4. **Error signaling** (1801-1850) - `syntax-error`

**Most important:** Lines 1591-1750 define formal pattern matching/expansion

---

## Appendix B: Test Priority

**Priority 1 (Must have):**
1. `when` macro
2. `unless` macro
3. `do` loop (complex!)

**Priority 2 (Hygiene validation):**
1. Free variable test (outer/inner x)
2. Local binding test (if shadowing)
3. Literal matching test (=> as var vs keyword)

**Priority 3 (Advanced features):**
1. Nested macros
2. Ellipsis escaping
3. Vector patterns

---

**Status:** ✅ Week 1 Research Complete
**Next:** Chibi C code analysis + Pattern/Template design
**Timeline:** 2-3 more weeks to 99% test pass rate
