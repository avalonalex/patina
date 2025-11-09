# Implementing the `do` Loop Construct

## Status: ✅ IMPLEMENTED (2025-11-09)

The `do` loop is an R7RS iteration construct that is now **fully implemented** in Patina as a special form.

**Implementation:** `src/eval/special_forms.rs:1157-1294`
**Tests:** `tests/compliance/derived.rs:179-285` (10 tests, all passing)
**Total test coverage:** 395 tests passing

## What is `do`?

The `do` construct is a general iteration loop with:
- Loop variables with initial values and step expressions
- A test condition that determines when to exit
- Optional result expressions evaluated when exiting
- Optional commands executed on each iteration

### Syntax

```scheme
(do ((variable1 init1 step1)
     (variable2 init2 step2)
     ...)
    (test result-expr ...)
  command ...)
```

### Example from the Test

```scheme
(do ((i 0 (+ i 1))
     (sum 0 (+ sum i)))
    ((> i 5) sum))
; => 15

; Explanation:
; i starts at 0, increments by 1 each iteration
; sum starts at 0, adds i each iteration
; When i > 5, return sum
; Iterations: sum = 0+0+1+2+3+4+5 = 15
```

## R7RS Specification

From R7RS Section 4.2.4:

### Semantics

1. **Initialization:**
   - Evaluate all `init` expressions (in unspecified order)
   - Bind variables to fresh locations
   - Store init values in the bindings

2. **Iteration:**
   - Evaluate `test`
   - If test is **false**:
     - Evaluate `command` expressions in order (for side effects)
     - Evaluate all `step` expressions (in unspecified order)
     - Bind variables to fresh locations
     - Store step values
     - Begin next iteration
   - If test is **true**:
     - Evaluate result expressions left to right
     - Return value of last result expression
     - If no result expressions, return unspecified

3. **Scoping:**
   - Variables are visible in: test, commands, steps, results
   - Variables are NOT visible in their own init expressions
   - Step expressions can reference all variables (including self)

4. **Optional Steps:**
   - If `step` is omitted, variable doesn't change between iterations
   - Equivalent to: `(variable init variable)` as the step

### R7RS Examples

```scheme
;; Example 1: Build a vector
(do ((vec (make-vector 5))
     (i 0 (+ i 1)))
    ((= i 5) vec)
  (vector-set! vec i i))
; => #(0 1 2 3 4)

;; Example 2: Sum a list
(let ((x '(1 3 5 7 9)))
  (do ((x x (cdr x))
       (sum 0 (+ sum (car x))))
      ((null? x) sum)))
; => 25

;; Example 3: No step (variable doesn't change)
(do ((i 5))
    ((= i 5) 'done))
; => done (i stays 5)
```

## Implementation Options

### Option 1: Special Form (Direct Implementation)

Implement `do` as a special form in the evaluator.

**Pros:**
- Direct control over evaluation order
- Clear semantics
- Good error messages

**Cons:**
- More code in evaluator
- Harder to maintain

**Implementation location:** `src/eval/special_forms.rs`

```rust
pub(super) fn eval_do(
    &mut self,
    bindings: &[(Rc<str>, Value, Option<Value>)], // (var, init, step?)
    test_clause: &(Value, Vec<Value>),            // (test, results)
    commands: &[Value],
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    // 1. Evaluate inits and create loop environment
    let loop_env = Rc::new(Environment::new_with_parent(env.clone()));
    for (var, init, _) in bindings {
        let init_val = self.eval_in_env(init, env)?;
        loop_env.define(var.clone(), init_val);
    }

    // 2. Loop
    loop {
        // Evaluate test
        let test_result = self.eval_in_env(&test_clause.0, &loop_env)?;

        if is_true(&test_result) {
            // Exit: evaluate result expressions
            let mut last = Value::Unspecified;
            for expr in &test_clause.1 {
                last = self.eval_in_env(expr, &loop_env)?;
            }
            return Ok(last);
        }

        // Execute commands
        for cmd in commands {
            self.eval_in_env(cmd, &loop_env)?;
        }

        // Evaluate steps and update bindings
        let mut new_values = Vec::new();
        for (var, _, step_opt) in bindings {
            let new_val = if let Some(step) = step_opt {
                self.eval_in_env(step, &loop_env)?
            } else {
                loop_env.get(var).unwrap() // Use current value
            };
            new_values.push((var.clone(), new_val));
        }

        // Update all bindings
        for (var, val) in new_values {
            loop_env.set(var, val)?;
        }
    }
}
```

### Option 2: Macro (Bootstrap Implementation)

Implement `do` as a macro in `lib/bootstrap.scm` using `letrec` and named let.

**Pros:**
- No evaluator changes needed
- Demonstrates macro power
- Easy to maintain

**Cons:**
- Complex macro
- Harder to debug
- Nested ellipsis needed (currently not supported!)

**Implementation:** Would look something like this:

```scheme
(define-syntax do
  (syntax-rules ()
    ((do ((var init step ...) ...)
         (test expr ...)
       command ...)
     (letrec ((loop (lambda (var ...)
                      (if test
                          (begin expr ...)
                          (begin
                            command ...
                            (loop step ... ...))))))
       (loop init ...)))))
```

**Problem:** This requires nested ellipsis `step ... ...` which we don't support!

### Option 3: Simpler Macro (Without Nested Ellipsis)

A less general macro that works with current limitations:

```scheme
;; This WON'T work for all cases but demonstrates the idea
(define-syntax do-2
  (syntax-rules ()
    ((do-2 ((var1 init1 step1)
            (var2 init2 step2))
           (test result)
       body ...)
     (let loop ((var1 init1) (var2 init2))
       (if test
           result
           (begin
             body ...
             (loop step1 step2)))))))
```

But this only works for exactly 2 variables!

## Recommended Approach

**Implement `do` as a special form** (Option 1)

**Reasons:**
1. ✅ Works immediately (no nested ellipsis needed)
2. ✅ Full R7RS compliance
3. ✅ Better error messages
4. ✅ Clearer semantics
5. ✅ Already have pattern for special forms

**Steps to implement:**

### Step 1: Add `do` to Special Forms List

In `src/eval/mod.rs`, add "do" to the special form dispatch:

```rust
"do" => self.eval_do(&args, env),
```

### Step 2: Parse `do` Syntax

In `src/eval/special_forms.rs`:

```rust
pub(super) fn eval_do(
    &mut self,
    expr: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    // Parse: (do ((var init step) ...) (test result ...) command ...)
    let (bindings_clause, rest) = extract_pair(expr)?;
    let (test_clause, commands) = extract_pair(rest)?;

    // Parse bindings: ((var init step) ...)
    let bindings = parse_do_bindings(bindings_clause)?;

    // Parse test clause: (test result ...)
    let (test_expr, result_exprs) = parse_test_clause(test_clause)?;

    // Parse commands
    let commands = collect_list_items(commands)?;

    // Execute loop (see implementation above)
    // ...
}
```

### Step 3: Implement Loop Logic

See the implementation sketch in Option 1 above.

### Step 4: Add Tests

```rust
#[test]
fn test_do_simple() {
    assert_eval_to(
        r#"(do ((i 0 (+ i 1))
              (sum 0 (+ sum i)))
             ((> i 5) sum))"#,
        "15",
    );
}

#[test]
fn test_do_with_commands() {
    assert_program_eval_to(
        r#"
        (define result '())
        (do ((i 0 (+ i 1)))
            ((= i 3) result)
          (set! result (cons i result)))
        "#,
        "(2 1 0)",
    );
}

#[test]
fn test_do_no_step() {
    // Variable without step doesn't change
    assert_eval_to(
        "(do ((x 5)) ((= x 5) 'done))",
        "done",
    );
}

#[test]
fn test_do_no_results() {
    // No result expressions returns unspecified
    assert_eval_to(
        "(do ((i 0 (+ i 1))) ((> i 5)))",
        "#<unspecified>",
    );
}
```

### Step 5: Update Documentation

Document `do` as an implemented feature.

## Estimated Effort

**Complexity:** Medium
**Effort:** 1-2 days
**Lines of code:** ~200-300 lines

**Breakdown:**
- Parsing: 0.5 day (~100 lines)
- Loop logic: 0.5 day (~100 lines)
- Testing: 0.5 day (~100 lines test code)
- Documentation: 0.5 day

## Why Not Implemented Yet?

The `do` construct is:
- Less commonly used than `let`, `letrec`, etc.
- Can be worked around with named let
- Not blocking other features
- Medium complexity to implement

**Priority:** Medium

Could implement after:
- Tail call optimization (TCO) - would make `do` more efficient
- More primitives needed for useful loops

## Workarounds (Current)

### Use Named Let

```scheme
;; Instead of:
(do ((i 0 (+ i 1))
     (sum 0 (+ sum i)))
    ((> i 5) sum))

;; Use:
(let loop ((i 0) (sum 0))
  (if (> i 5)
      sum
      (loop (+ i 1) (+ sum i))))
```

### Use letrec

```scheme
(letrec ((loop (lambda (i sum)
                 (if (> i 5)
                     sum
                     (loop (+ i 1) (+ sum i))))))
  (loop 0 0))
```

## Related R7RS Features

- **Named let** - Similar iteration construct (already implemented)
- **`do`** - General iteration (not implemented)
- TCO - Makes iteration efficient (not implemented)

## Decision

**Defer implementation for now** but document clearly.

Reasons:
1. Can use named let as workaround
2. Medium effort (1-2 days)
3. Not blocking other features
4. Less commonly used than other constructs

**Future:** Implement as special form when:
- More user demand
- After TCO implementation
- After more primitives added

## References

- R7RS Section 4.2.4: Iteration (`do`)
- R7RS Section 4.2.4: Named let (already implemented)
- Chibi-scheme implementation: `lib/init-7.scm` (may be macro or special form)

## Related Files

- `tests/compliance/derived.rs:179` - Test marked `#[ignore]`
- `src/eval/special_forms.rs` - Where to add implementation
- `src/eval/mod.rs` - Where to add dispatch

## Test to Enable

When implemented, enable:
```rust
#[test]
#[ignore] // TODO: Implement do
fn test_do_simple() { ... }
```

Located at: `tests/compliance/derived.rs:179`
