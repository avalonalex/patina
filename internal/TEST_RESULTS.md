# R7RS Test Results

## Test Suite Status

Created comprehensive R7RS test suite based on chibi-scheme's r7rs-tests.scm.

### Test Organization

```
tests/
├── test_helpers.rs        - Helper functions (assert_eval_to, etc.)
├── r7rs_primitives.rs     - Section 4.1: Primitive expressions (20 tests)
├── r7rs_derived.rs        - Section 4.2: Derived expressions (19 tests)
├── r7rs_numbers.rs        - Section 6.2: Numbers (23 tests)
├── r7rs_lists.rs          - Section 6.4: Lists and pairs (19 tests)
└── r7rs_predicates.rs     - Section 6.3: Booleans and predicates (12 tests)

Total: 93 tests
```

## Current Test Results (r7rs_primitives)

### Passing Tests ✅ (11/20)

1. **Variable reference** - `(define x 28) x` → `28`
2. **Quote symbol** - `'a` → `a`
3. **Quote list** - `'(+ 1 2)` → `(+ 1 2)`
4. **Quote nested** - `''a` → `(quote a)`
5. **Quote literals** - Numbers, strings, booleans
6. **Define variable** - Basic define
7. **Define multiple** - Multiple defines in sequence
8. **Set!** - Variable mutation
9. **Begin** - Sequential evaluation
10. **Begin returns last** - Returns last expression value
11. **If procedure selection** - `((if #f + *) 3 4)` → `12`

### Failing Tests ❌ (5/20)

1. **test_if_true** - `(if (> 3 2) 'yes 'no)`
   - **Error**: `Undefined variable: >`
   - **Fix needed**: Add `>`, `<=`, `>=` comparison operators

2. **test_if_false** - `(if (> 2 3) 'yes 'no)`
   - **Error**: Same as above

3. **test_if_consequent_evaluated** - `(if (> 3 2) (- 3 2) (+ 3 2))`
   - **Error**: Same as above

4. **test_simple_lambda** - `((lambda (x) (+ x x)) 4)`
   - **Error**: `lambda not yet implemented`
   - **Fix needed**: Implement lambda special form

5. **test_lambda_multiple_args** - `(lambda (x y) (- y x))`
   - **Error**: Same as above

### Ignored Tests (4/20)

1. **test_lambda_closure** - Requires closures (let + lambda)
2. **test_lambda_variadic** - `(lambda x x)` - variadic args
3. **test_lambda_variadic_with_fixed** - `(lambda (x y . z) z)`
4. **test_set_bang_undefined** - Should error on undefined variable

## Immediate Fixes Needed

### 1. Add Missing Comparison Operators

**File**: `src/eval/mod.rs`

**Current**:
```rust
fn install_primitives(env: &Rc<Environment>) {
    let primitives = [
        // ...
        ("=", Arity::Min(2)),
        ("<", Arity::Min(2)),
        // Missing: >, <=, >=
    ];
}
```

**Need to add**:
- `>` - greater than
- `<=` - less than or equal
- `>=` - greater than or equal

Implementation should follow same pattern as `primitive_less_than`.

### 2. Implement Lambda (Critical)

**File**: `src/eval/mod.rs:191`

**Current**:
```rust
fn eval_lambda(&self, _args: &Value, _env: &Rc<Environment>) -> Result<Value, EvalError> {
    // TODO: Implement lambda
    Err(EvalError::InvalidSyntax(
        "lambda not yet implemented".to_string(),
    ))
}
```

**Requirements**:
1. Parse parameter list
2. Parse body (one or more expressions)
3. Create `Procedure::Lambda` with params and body
4. **Critical**: Capture environment for closures
5. Support variadic parameters (`(lambda (x . rest) ...)`)

**Reference**: `~/Project/reference/chibi-scheme/eval.c` for closure handling

## Test Summary by Category

### Primitives (r7rs_primitives.rs)
- ✅ Passing: 11/20 (55%)
- ❌ Failing: 5/20 (25%)
- ⏸️ Ignored: 4/20 (20%)

### Derived Forms (r7rs_derived.rs)
- All 19 tests marked `#[ignore]` - not yet implemented
- Need: cond, case, and, or, let, let*, letrec, do, when, unless

### Numbers (r7rs_numbers.rs)
- Basic arithmetic passing (7 tests)
- Comparisons partially working (need >, <=, >=)
- 16 tests ignored - need quotient, remainder, modulo, abs, max, min, predicates

### Lists (r7rs_lists.rs)
- Basic cons, car, cdr, null?, pair? passing (5 tests)
- 14 tests ignored - need list, length, append, reverse, list-ref, caar/cadr, assq, memq

### Predicates (r7rs_predicates.rs)
- boolean?, symbol?, string? passing (3 tests)
- 9 tests ignored - need not, eq?, eqv?, equal?, char?, vector?, procedure?

## Next Steps (Priority Order)

1. **Add `>`, `<=`, `>=` operators** (< 30 min)
   - Will make 3 more primitive tests pass
   - Simple addition to install_primitives and apply_primitive

2. **Implement basic lambda** (2-4 hours)
   - Parse parameters and body
   - Create Lambda procedure
   - Will make 2 more primitive tests pass
   - **Don't worry about closures yet** - just get basic lambda working

3. **Implement closures** (4-8 hours)
   - Add environment capture to Lambda
   - Update apply to handle Lambda procedures
   - Will make closure test pass
   - Required for almost everything else

4. **Implement let/let*/letrec** (4-6 hours)
   - Can desugar to lambda
   - Critical for idiomatic Scheme
   - Needed by many other features

5. **Implement cond, case, and, or** (2-4 hours)
   - Straightforward special forms
   - Used everywhere in R7RS code

## How to Run Tests

```bash
# Run all R7RS primitive tests
cargo test --test r7rs_primitives

# Run with output
cargo test --test r7rs_primitives -- --nocapture

# Run a specific test
cargo test --test r7rs_primitives test_if_true

# Run all tests including ignored
cargo test --test r7rs_primitives -- --include-ignored

# Run all R7RS tests
cargo test r7rs_

# Run only passing tests (exclude ignored)
cargo test --test r7rs_primitives -- --skip ignored
```

## Test Helper API

```rust
// Basic assertion
assert_eval_to("(+ 1 2)", "3");

// Multi-expression program
assert_program_eval_to(r#"
    (define x 10)
    (+ x 5)
"#, "15");

// Expect error
assert_eval_error("(/ 1 0)");
```

## Progress Tracking

As features are implemented:
1. Remove `#[ignore]` from relevant tests
2. Run test suite
3. Fix any failures
4. Update this document
5. Move to next feature

**Goal**: Get all r7rs_primitives tests passing (100% coverage of Section 4.1)

## Comparison with Chibi

All test cases are based on chibi-scheme's r7rs-tests.scm. When in doubt about expected behavior:

```bash
# Test in chibi
chibi-scheme -e "(your-expression)"

# Compare with Patina
cargo run
patina> (your-expression)
```

Expected output should match (modulo formatting differences like `#t` vs `true`).
