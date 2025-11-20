# How Chibi-Scheme Handles Parameterize + Tail Calls

## The Solution: `dynamic-wind`

Chibi-scheme solves the parameterize/tail-call interaction problem by implementing `parameterize` as a **macro that expands to `dynamic-wind`**.

## Implementation (No-Threads Version)

```scheme
(define-syntax parameterize
  (syntax-rules ()
    ((parameterize ("step")
                   ((param value p old new) ...)
                   ()
                   body)
     (let ((p param) ...)
       (let ((old (p)) ...                          ; Save old values
             (new (parameter-convert p value)) ...) ; Convert new values
         (dynamic-wind
          (lambda () (p new) ...)                   ; BEFORE: Set new values
          (lambda () . body)                        ; BODY: Evaluate body
          (lambda () (p old) ...)))))               ; AFTER: Restore old values
    ; ... pattern matching rules ...
    ((parameterize ((param value) ...) . body)
     (parameterize ("step") () ((param value) ...) body))))
```

## Key Insights

### 1. **Macro Expansion**
`parameterize` is not a special form - it's a **macro** that expands to code using `dynamic-wind`.

### 2. **`dynamic-wind` Guarantees**
`dynamic-wind` has special semantics (R7RS section 6.10):
- The "before" thunk is called when entering
- The "body" thunk is evaluated
- The "after" thunk is **ALWAYS** called when leaving, even if:
  - The body does a tail call
  - A continuation is invoked (call/cc)
  - An exception is raised

### 3. **Parameter Mutation**
Notice that chibi uses **parameter mutation**: `(p new)` calls the parameter with one argument to SET its value, and `(p)` calls it with zero arguments to GET the value.

This is different from our stack approach - chibi mutates the parameter object directly, then relies on `dynamic-wind` to ensure restoration.

## Comparison to Our Implementation

### Our Approach (Stack-Based)
```rust
// Push new value onto stack
values.borrow_mut().push(converted_val);

// Evaluate body (NOT in tail position)
result = evaluator.eval_in_env(&pair.0, env)?;

// Pop stack to restore
values.borrow_mut().pop();
```

**Problem:** Can't use tail call optimization because we need to pop after body completes.

### Chibi's Approach (dynamic-wind)
```scheme
(dynamic-wind
  (lambda () (p new-val))    ; Set parameter
  (lambda () <body>)         ; Body CAN be tail-called!
  (lambda () (p old-val)))   ; Restoration guaranteed by dynamic-wind
```

**Advantage:** The VM/evaluator handles ensuring the "after" thunk runs, even for tail calls!

## How `dynamic-wind` Enables Tail Calls

The key is that `dynamic-wind` is implemented at the **evaluator/VM level**, not as user code:

1. When evaluating `dynamic-wind`, the VM:
   - Records the "after" thunk in a **winding stack** or similar structure
   - Calls the "before" thunk
   - Evaluates the body (possibly with tail call optimization)
   - Ensures "after" thunk is called when control leaves (even via tail call)

2. The VM's tail call implementation must:
   - Check if there are pending "after" thunks
   - Call them before transferring control to the tail call target
   - Or track them across tail calls and call them when eventually returning

## What We Need to Implement

To properly support tail calls in `parameterize`, we need to:

1. **Implement `dynamic-wind`** as a primitive or special form
2. **Track winding stack** in the evaluator
3. **Integrate with tail call optimization** to ensure cleanup thunks run
4. **Reimplement `parameterize` as a macro** that expands to `dynamic-wind`

This is a significant undertaking but is the **proper R7RS-compliant solution**.

## Alternative: Keep Current Approach

For now, our stack-based approach with disabled tail calls is:
- ✅ Correct (all tests pass)
- ✅ Simple to understand
- ✅ Easy to maintain
- ❌ Doesn't support tail calls in parameterize bodies

This is an acceptable trade-off until we implement `dynamic-wind`.

## References

- Chibi-scheme implementation: `~/Projects/reference/chibi-scheme/lib/srfi/39/syntax-no-threads.scm`
- R7RS section 6.10: `dynamic-wind`
- R7RS section 4.2.6: Dynamic bindings (parameters)
