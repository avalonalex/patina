# Using Chibi-Scheme as Reference

Guide for leveraging chibi-scheme when implementing R7RS features in Patina.

## Location

Chibi-scheme reference implementation: `~/Project/reference/chibi-scheme`

## Key Files

### 1. Test Suite - What to Implement
**File:** `tests/r7rs-tests.scm` (2516 lines)

This is your specification. Each test shows:
- Expected behavior
- Edge cases
- Correct output format

**Example workflow:**
```bash
# View tests for lambda
grep -A 10 "test.*lambda" ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

# View tests for let
sed -n '/test-begin.*let/,/test-end/p' ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm
```

### 2. Standard Library - How to Implement
**File:** `lib/init-7.scm`

Shows which procedures are:
- **Primitives** (implemented in C)
- **Derived** (implemented in Scheme using primitives)

**Example - Understanding what needs to be primitive:**

```scheme
; From init-7.scm:

; These are implemented in Scheme (don't need primitives):
(define (caar x) (car (car x)))
(define (list . args) args)
(define (not x) (if x #f #t))

; These call primitives (car, cdr are primitives):
(define (length ls)
  (if (list? ls) (length* ls) (error "length: not a list" ls)))
```

**Takeaway:** If chibi implements it in init-7.scm, you can too! Just need the underlying primitives.

### 3. Evaluator - Implementation Details
**File:** `eval.c`

The C implementation shows:
- How special forms are handled
- Tail call optimization
- Closure representation
- Macro expansion

**Useful for:**
- Understanding closure capture
- Seeing how let/let*/letrec desugar
- Tail call mechanics

**Caution:** Chibi uses a bytecode VM. Patina is a tree-walking interpreter. Don't copy directly, understand the concepts.

## Practical Workflow

### When Implementing a New Feature

#### Step 1: Find the Tests
```bash
cd ~/Project/reference/chibi-scheme

# Find tests for the feature (e.g., 'cond')
grep -n "test.*cond" tests/r7rs-tests.scm | head -20
```

#### Step 2: Run Tests in Chibi
```bash
# Test a specific expression
chibi-scheme -e "(cond ((> 3 2) 'greater) ((< 3 2) 'less))"
# => greater

# Test edge cases
chibi-scheme -e "(cond (else 'default))"
# => default
```

#### Step 3: Check if It's a Primitive or Derived

```bash
# Search in init-7.scm for the implementation
grep -A 10 "define.*cond" lib/init-7.scm

# If not found, it's a primitive (in C)
grep -n "cond" eval.c | head -5
```

#### Step 4: Understand the Semantics
For special forms like `cond`:
1. Read the R7RS spec section
2. Look at chibi's eval.c to see how it handles it
3. Look at test cases to understand behavior

#### Step 5: Implement in Patina
```rust
// In src/eval/mod.rs

fn eval_list(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // ... existing code ...
    match sym.as_ref() {
        "quote" => return self.eval_quote(&cdr),
        "if" => return self.eval_if(&cdr, env),
        "cond" => return self.eval_cond(&cdr, env),  // Add this
        // ...
    }
}

fn eval_cond(&self, clauses: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // Implementation based on what you learned from chibi
}
```

#### Step 6: Test Against Chibi
```bash
# Test the same expression in both
chibi-scheme -e "(cond ((> 3 2) 'yes) (else 'no))"
# => yes

cargo run
# patina> (cond ((> 3 2) 'yes) (else 'no))
# Should output: yes
```

## Example: Implementing `let`

### 1. Find the tests
```bash
cd ~/Project/reference/chibi-scheme
grep -A 5 "test.*let" tests/r7rs-tests.scm | head -30
```

Output shows:
```scheme
(test 6 (let ((x 2) (y 3))
  (* x y)))

(test 35 (let ((x 2) (y 3))
  (let ((x 7)
        (z (+ x y)))  ; Note: uses outer x!
    (* z x))))
```

### 2. Check init-7.scm
```bash
grep -A 20 "define-syntax let" lib/init-7.scm
```

If it's not there, it's a primitive. Check eval.c.

### 3. Understand the semantics
From the test, we see:
- `let` creates new bindings
- Inner `let` shadows outer bindings
- But initialization uses OUTER scope

This means: `let` ≈ `((lambda (x y) body) expr1 expr2)`

### 4. Implement
```rust
fn eval_let(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // (let ((x 1) (y 2)) body...)
    // Extract bindings and body
    // Evaluate all binding expressions in CURRENT env
    // Create NEW env with bindings
    // Evaluate body in NEW env
}
```

### 5. Test
Create test file `tests/schemes/control/let.scm`:
```scheme
; Basic let
(let ((x 2) (y 3)) (* x y))
; Expected: 6

; Nested let
(let ((x 2) (y 3))
  (let ((x 7) (z (+ x y)))
    (* z x)))
; Expected: 35
```

Run comparison test:
```rust
#[test]
fn test_let_forms() {
    compare("(let ((x 2) (y 3)) (* x y))");
    // etc.
}
```

## Common Patterns

### Pattern 1: Special Form Desugaring
Many special forms desugar to simpler forms:

```scheme
; let desugars to lambda application:
(let ((x 1) (y 2)) (+ x y))
; ≡
((lambda (x y) (+ x y)) 1 2)

; and desugars to nested ifs:
(and a b c)
; ≡
(if a (if b c #f) #f)
```

**Check chibi's eval.c** to see if it desugars or handles directly.

### Pattern 2: Self-Hosting
Many procedures are written in Scheme:

From init-7.scm:
```scheme
(define (caar x) (car (car x)))
(define (list . args) args)
(define (append . o) ...)
```

**For Patina:** Implement these in Scheme once you have enough primitives!

Store in `lib/base.scm`, load at startup.

### Pattern 3: Primitive vs Derived
**Primitives** (must be in Rust):
- `cons`, `car`, `cdr` - Data structure basics
- `+`, `-`, `*`, `/` - Arithmetic
- `eq?`, `eqv?` - Identity
- `apply1` - Function application

**Derived** (can be in Scheme):
- `caar`, `cadr`, etc. - Combinations of car/cdr
- `list` - Uses variadic args
- `append` - Uses cons and recursion
- `not` - Uses if

## Debugging Differences

### When output differs from chibi:

#### 1. Output Format
```bash
# Chibi might output:
#t, #f, '(), (1 2 3)

# Patina might output:
true, false, (), (1 2 3)
```

**Solution:** Check Patina's `Display` implementation for `Value`.

#### 2. Error Messages
```bash
# Chibi might give:
ERROR: undefined variable: foo

# Compare what Patina gives
```

**Solution:** Improve error messages to match.

#### 3. Behavior Differences
If behavior differs:
1. Re-read R7RS spec for that feature
2. Check chibi's test to confirm expected behavior
3. Debug Patina's implementation

## Quick Reference Commands

```bash
# Test an expression in chibi
chibi-scheme -e "(your-expression-here)"

# Run chibi's full test suite
cd ~/Project/reference/chibi-scheme
chibi-scheme tests/r7rs-tests.scm

# Search for a feature
grep -r "your-feature" lib/
grep -n "your-feature" eval.c

# View specific test section
sed -n '/test-begin "section-name"/,/test-end/p' tests/r7rs-tests.scm
```

## Integration with Patina Tests

### Strategy
1. Copy relevant tests from chibi's r7rs-tests.scm
2. Adapt to Patina's test format
3. Use comparison tests to verify compatibility

### Example

From chibi:
```scheme
(test 'greater (cond ((> 3 2) 'greater) ((< 3 2) 'less)))
```

In Patina's `tests/comparison_test.rs`:
```rust
#[test]
fn test_cond_basic() {
    compare("(cond ((> 3 2) 'greater) ((< 3 2) 'less))");
}
```

## Summary

**Golden Rules:**
1. **Tests first** - Let r7rs-tests.scm guide what to implement
2. **Understand primitives** - Check init-7.scm to see what's foundational
3. **Test incrementally** - Verify each feature against chibi
4. **Learn from eval.c** - Understand concepts, don't copy implementation
5. **Self-host when possible** - Write Scheme procedures in Scheme

**Quick Workflow:**
1. Pick feature from roadmap
2. Read tests in r7rs-tests.scm
3. Check init-7.scm (Scheme) or eval.c (C primitive)
4. Implement in Patina
5. Test against chibi output
6. Move to next feature
