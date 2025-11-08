# Macro System Research & Implementation Plan

**Date:** 2025-11-07
**Status:** Research Phase
**Goal:** Design and implement R7RS-compliant hygienic macro system

---

## Executive Summary

Macros are one of the most powerful features in Scheme, enabling syntactic abstraction and code transformation at compile-time. R7RS-small requires support for **hygienic macros** via `syntax-rules` and `define-syntax`.

**Current Status:**
- 242/245 tests passing (99% of non-macro features)
- 3 tests blocked by missing macro support: `when`, `unless`, `do`
- Ready to implement macro system as final major R7RS feature

**Complexity:** High - requires:
- Pattern matching on syntax
- Hygiene (preventing variable capture)
- Macro expansion phase
- Integration with evaluator

---

## Part 1: R7RS Specification Analysis

### 1.1 Core Macro Forms (R7RS Section 4.3)

#### `syntax-rules` (Section 4.3.2)
The primary macro definition mechanism in R7RS-small.

**Syntax:**
```scheme
(syntax-rules (<literal> ...) <syntax-rule> ...)
```

**Example from spec:**
```scheme
(define-syntax when
  (syntax-rules ()
    ((when test result1 result2 ...)
     (if test
         (begin result1 result2 ...)))))
```

**Key requirements:**
- Pattern matching against s-expressions
- Template-based expansion with pattern variables
- Hygiene: prevent accidental variable capture
- Support for ellipsis (`...`) for repetition
- Literal identifiers (must match exactly)

#### `define-syntax` (Section 4.3.1)
Binds a macro transformer to a name.

**Syntax:**
```scheme
(define-syntax <keyword> <transformer-spec>)
```

**Example:**
```scheme
(define-syntax unless
  (syntax-rules ()
    ((unless condition body ...)
     (if (not condition)
         (begin body ...)))))
```

### 1.2 Required Features for R7RS-small

**Minimum requirements:**
1. ✅ `define-syntax` - Macro definition
2. ✅ `syntax-rules` - Pattern-based transformers
3. ✅ Hygiene - Prevent variable capture
4. ✅ Ellipsis (`...`) - Repetition in patterns/templates
5. ✅ Literal matching - Keywords that must match exactly
6. ⚠️ `let-syntax` - Local macro bindings (optional for our 3 tests)
7. ⚠️ `letrec-syntax` - Recursive local macros (optional)

**Not required for R7RS-small:**
- ❌ `syntax-case` (advanced macro system, R6RS)
- ❌ `identifier-syntax` (advanced)
- ❌ Low-level macro primitives

### 1.3 Hygiene Requirements

**What is hygiene?**
Hygienic macros prevent accidental variable capture. Compare:

**Unhygienic (bad):**
```scheme
;; Broken macro - captures 'it'
(define-syntax my-when
  (lambda (test . body)
    `(let ((it ,test))
       (if it ,@body))))

(let ((it 5))
  (my-when #t it))  ; ERROR: 'it' gets shadowed!
```

**Hygienic (correct):**
```scheme
;; syntax-rules ensures hygiene automatically
(define-syntax my-when
  (syntax-rules ()
    ((my-when test body ...)
     (if test (begin body ...)))))

(let ((it 5))
  (my-when #t it))  ; Works correctly!
```

**R7RS requirement:** All introduced identifiers must be fresh (renamed).

---

## Part 2: Reference Implementation Analysis

### 2.1 Chibi-Scheme Analysis

**Location:** `~/Project/reference/chibi-scheme/`

**Research tasks:**
1. [ ] How are macros represented internally?
   - Files to examine: `eval.c`, `lib/init-7.scm`
   - Look for `sexp_analyze`, `sexp_expand`

2. [ ] How is pattern matching implemented?
   - Pattern syntax tree structure
   - Ellipsis handling algorithm

3. [ ] How is hygiene maintained?
   - Identifier renaming strategy
   - Scope tracking mechanism

4. [ ] How is expansion triggered?
   - Pre-evaluation expansion phase?
   - Lazy expansion?

**Commands to investigate:**
```bash
cd ~/Project/reference/chibi-scheme
grep -r "syntax-rules" lib/
grep -r "define-syntax" lib/
grep -r "expand" *.c
```

### 2.2 Example Analysis from chibi-scheme

**Test suite examination:**
```bash
# Find macro tests in chibi
grep -A 10 "syntax-rules\|define-syntax" tests/r7rs-tests.scm | head -50
```

**Bootstrap library analysis:**
```bash
# See how chibi implements when/unless
grep -A 5 "define-syntax when\|define-syntax unless" lib/init-7.scm
```

### 2.3 Chez Scheme (Optional Deep Dive)

Chez Scheme has one of the most sophisticated macro systems.

**Research if time permits:**
- How does Chez implement syntax-case?
- What optimizations are possible?
- How does it integrate with compiler?

---

## Part 3: Implementation Strategy

### 3.1 Phased Approach

#### Phase 1: Pattern Matching Foundation (Week 1)
**Goal:** Implement pattern matching without hygiene

**Tasks:**
1. Define `Pattern` enum
   ```rust
   enum Pattern {
       Literal(Value),        // Exact match (symbols, literals)
       Variable(String),      // Pattern variable
       List(Vec<Pattern>),    // List pattern
       Ellipsis(Box<Pattern>),// ... repetition
       Any,                   // _ wildcard
   }
   ```

2. Implement pattern matcher
   ```rust
   fn match_pattern(pattern: &Pattern, value: &Value) -> Option<Bindings>
   ```

3. Test pattern matching in isolation
   - Test literal matching
   - Test variable capture
   - Test ellipsis (repetition)

**Success criteria:**
- Can match `(when test body ...)` against `(when #t (+ 1 2) (* 3 4))`
- Captures `test` = `#t`, `body` = `[(+ 1 2), (* 3 4)]`

#### Phase 2: Template Expansion (Week 2)
**Goal:** Generate code from templates

**Tasks:**
1. Define `Template` structure
   ```rust
   enum Template {
       Literal(Value),
       Variable(String),
       List(Vec<Template>),
       Ellipsis(Box<Template>),
   }
   ```

2. Implement template expander
   ```rust
   fn expand_template(template: &Template, bindings: &Bindings) -> Value
   ```

3. Test template expansion
   - Substitute pattern variables
   - Handle ellipsis expansion (repetition)

**Success criteria:**
- Can expand `(if test (begin body ...))` with captured bindings
- Produces correct output AST

#### Phase 3: Basic Macro System (Week 3)
**Goal:** Working `define-syntax` + `syntax-rules` without hygiene

**Tasks:**
1. Add `Macro` variant to `Value` enum
   ```rust
   enum Value {
       // ...
       Macro {
           name: String,
           patterns: Vec<(Pattern, Template)>,
       }
   }
   ```

2. Implement `define-syntax` special form
3. Integrate macro expansion into evaluator
4. Add macro environment (separate from runtime environment)

**Success criteria:**
- `when` macro works correctly
- `unless` macro works correctly
- Basic `do` loop works (if possible without full hygiene)

#### Phase 4: Hygiene Implementation (Week 4)
**Goal:** Full R7RS-compliant hygienic macros

**Tasks:**
1. Research hygiene algorithms
   - Renaming strategy (alpha-conversion)
   - Scope sets / syntax objects

2. Implement identifier renaming
   ```rust
   struct Identifier {
       name: String,
       scope_id: usize,  // Unique scope identifier
   }
   ```

3. Track macro invocation scopes
4. Rename introduced identifiers

**Success criteria:**
- Pass all R7RS hygiene tests
- Variable capture impossible

#### Phase 5: Advanced Features (If Needed)
**Optional enhancements:**
- [ ] `let-syntax` (local macros)
- [ ] `letrec-syntax` (recursive local macros)
- [ ] Better error messages for pattern match failures
- [ ] Macro expansion debugging/tracing

### 3.2 Data Structures

#### Macro Definition Storage
```rust
struct MacroDefinition {
    name: String,
    literals: Vec<String>,  // Literal identifiers
    rules: Vec<SyntaxRule>,
}

struct SyntaxRule {
    pattern: Pattern,
    template: Template,
}
```

#### Macro Environment
Separate from runtime environment:
```rust
struct MacroEnvironment {
    macros: HashMap<String, MacroDefinition>,
    parent: Option<Rc<MacroEnvironment>>,
}
```

### 3.3 Integration with Evaluator

**Current evaluator flow:**
```
Input → Lexer → Parser → Evaluator → Value
```

**New flow with macros:**
```
Input → Lexer → Parser → Macro Expander → Evaluator → Value
                            ↑
                    Macro Environment
```

**Key question:** When to expand?
- Option A: Expand all macros before evaluation (simpler)
- Option B: Expand on-demand during evaluation (lazier)
- **Recommendation:** Start with Option A (pre-expansion pass)

---

## Part 4: Research Questions

### 4.1 Critical Questions to Answer

1. **Pattern Matching:**
   - How to handle nested ellipsis? `((a ...) ...)`
   - How to match improper lists? `(a . b)`
   - How to handle `_` wildcard in patterns?

2. **Hygiene:**
   - What algorithm should we use? (Syntactic closures? Syntax-case? Explicit renaming?)
   - How to generate unique identifiers?
   - How to track identifier bindings across macro expansion?

3. **Performance:**
   - Cache expanded macros?
   - Memoize pattern matching results?
   - Impact on compilation time?

4. **Error Handling:**
   - How to report pattern match failures?
   - How to show macro expansion steps for debugging?
   - Source location tracking through macro expansion?

### 4.2 Investigation Plan

**Week 1: Specification Study**
- [ ] Read R7RS Section 4.3 in detail
- [ ] Study all macro examples in R7RS spec
- [ ] List all required behaviors
- [ ] Create test cases from spec

**Week 2: Reference Implementation Study**
- [ ] Analyze chibi-scheme macro implementation
- [ ] Document chibi's pattern matching algorithm
- [ ] Document chibi's hygiene strategy
- [ ] Extract key insights

**Week 3: Algorithm Design**
- [ ] Choose hygiene algorithm
- [ ] Design pattern matching algorithm
- [ ] Design template expansion algorithm
- [ ] Create detailed pseudocode

**Week 4: Prototype**
- [ ] Implement minimal pattern matcher
- [ ] Test against simple examples
- [ ] Iterate on design
- [ ] Document findings

---

## Part 5: Test Strategy

### 5.1 Test Suite from R7RS

Extract macro tests from chibi's test suite:
```bash
grep -B 2 -A 10 "define-syntax" \
  ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm \
  > /tmp/macro_tests.scm
```

### 5.2 Our Test Cases

#### Basic Pattern Matching
```scheme
;; Test 1: Simple when
(define-syntax when
  (syntax-rules ()
    ((when test result)
     (if test result))))

(when #t 42)  ; => 42
(when #f 42)  ; => #<unspecified>
```

#### Ellipsis Handling
```scheme
;; Test 2: Multiple body forms
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

(when #t 1 2 3)  ; => 3
```

#### Hygiene Test
```scheme
;; Test 3: No variable capture
(define-syntax swap
  (syntax-rules ()
    ((swap a b)
     (let ((temp a))
       (set! a b)
       (set! b temp)))))

(let ((temp 1) (x 2) (y 3))
  (swap x y)
  (list temp x y))  ; => (1 3 2)  NOT (2 3 2)!
```

### 5.3 Incremental Testing

Create test files:
- `tests/fixtures/examples/macros/01_basic_when.scm`
- `tests/fixtures/examples/macros/02_unless.scm`
- `tests/fixtures/examples/macros/03_do_loop.scm`
- `tests/fixtures/examples/macros/04_hygiene.scm`

---

## Part 6: Resources

### 6.1 Papers & Articles

**Essential reading:**
1. "Macros That Work" - Clinger & Rees (1991)
   - Original hygiene algorithm for Scheme
2. "Syntactic Abstraction in Scheme" - Dybvig et al. (1992)
   - Syntax-case system design
3. "Keeping it Clean with Syntactic Closures" - Bawden & Rees (1988)
   - Alternative hygiene approach

### 6.2 Reference Implementations

- **Chibi-Scheme:** `~/Project/reference/chibi-scheme/`
- **Chez Scheme:** (Optional) Advanced implementation
- **Racket:** (Optional) Modern macro system

### 6.3 R7RS Specification

**Key sections:**
- Section 4.3 - Macros
- Section 4.3.1 - Binding constructs for syntactic keywords
- Section 4.3.2 - Pattern language

**Files:**
```
~/Project/patina/spec/r7rs-small-spec/sem.tex
~/Project/patina/spec/r7rs-small-spec/stdmod.tex
```

---

## Part 7: Success Criteria

### Milestone 1: Basic Macros (No Hygiene)
- ✅ `when` macro works
- ✅ `unless` macro works
- ✅ Simple `do` loop works
- ✅ Pattern matching functional
- ✅ Template expansion functional

### Milestone 2: Hygiene
- ✅ Variable capture impossible
- ✅ All R7RS hygiene tests pass
- ✅ Nested macro expansion works

### Milestone 3: Full R7RS Compliance
- ✅ All 3 ignored tests pass
- ✅ `define-syntax` + `syntax-rules` complete
- ✅ 100% R7RS-small compliance (245/245 tests)

---

## Part 8: Timeline Estimate

**Conservative estimate:** 4-6 weeks

| Phase | Duration | Milestone |
|-------|----------|-----------|
| Research & Design | 1 week | Algorithm chosen |
| Pattern Matching | 1 week | Patterns work |
| Template Expansion | 1 week | Templates work |
| Basic Macros | 1 week | when/unless/do pass |
| Hygiene | 1-2 weeks | Full compliance |
| Polish & Testing | 1 week | Production ready |

**Aggressive estimate:** 3 weeks (if we skip deep hygiene)

---

## Next Steps

1. **This Week:**
   - [ ] Read R7RS Section 4.3 completely
   - [ ] Analyze chibi-scheme macro implementation
   - [ ] Document pattern matching algorithm
   - [ ] Create test cases

2. **Next Week:**
   - [ ] Design data structures
   - [ ] Implement pattern matcher
   - [ ] Test pattern matching

3. **Week 3:**
   - [ ] Implement template expander
   - [ ] Integrate with evaluator
   - [ ] Get `when` working

---

## Appendix: Example Macros to Support

### A.1 when/unless (Required for tests)
```scheme
(define-syntax when
  (syntax-rules ()
    ((when test result1 result2 ...)
     (if test (begin result1 result2 ...)))))

(define-syntax unless
  (syntax-rules ()
    ((unless test result1 result2 ...)
     (if (not test) (begin result1 result2 ...)))))
```

### A.2 do (Required for tests)
```scheme
(define-syntax do
  (syntax-rules ()
    ((do ((var init step ...) ...)
         (test expr ...)
       command ...)
     (letrec
       ((loop
          (lambda (var ...)
            (if test
                (begin expr ...)
                (begin
                  command ...
                  (loop (do "step" var step ...) ...))))))
       (loop init ...)))))
```

### A.3 Other Common Macros (For reference)
```scheme
;; and/or (we already have these as special forms)
;; cond (we already have this)
;; case (we already have this)

;; let-values (useful for future)
;; parameterize (useful for future)
```

---

**Status:** Ready to begin research phase!
**Next Action:** Start with R7RS spec reading and chibi-scheme analysis.
