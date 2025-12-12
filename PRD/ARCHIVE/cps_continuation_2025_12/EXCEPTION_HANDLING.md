# R7RS Exception Handling Implementation Guide

**Status:** ✅ Complete (26/26 tests passing)
**Priority:** Done
**Last Updated:** 2025-12-12

---

## Overview

R7RS exception handling is fully implemented. The CPS evaluator has complete support for `with-exception-handler`, `raise`, `raise-continuable`, `error`, and the `guard` macro. I/O and read errors are now also routed through the CPS exception handler stack.

**Test Results:** 26/26 exception tests passing

**Overall Compliance:** 1159/1159 tests passing (100%)

---

## Current Implementation Status (2025-12-11)

### ✅ Completed

1. **`Value::Exception` type** (`crates/patina-core/src/value.rs`):
   - `ExceptionObject` struct with `kind`, `message`, `irritants`
   - `ExceptionKind` enum: `Error`, `FileError`, `ReadError`, `Custom`

2. **`EvalError::SchemeException`** variant (`crates/patina-tree-walker/src/eval/error.rs`):
   - Scheme-level exceptions propagate as this error variant
   - Contains `SchemeExceptionKind`, `message`, and serialized `irritants_display`

3. **Exception primitives** (`crates/patina-tree-walker/src/eval/primitives/exceptions.rs`):
   - ✅ `error` - Creates and raises error with message and irritants
   - ✅ `error-object?` - Predicate for error objects
   - ✅ `error-object-message` - Get error message
   - ✅ `error-object-irritants` - Get irritants
   - ✅ `file-error?` - Predicate for file errors
   - ✅ `read-error?` - Predicate for read errors
   - ✅ `raise` - Raise non-continuable exception (CPS-aware in CPS mode)
   - ✅ `raise-continuable` - Raise continuable exception (CPS-aware in CPS mode)
   - ✅ `with-exception-handler` - Install exception handler (CPS-aware in CPS mode)

4. **CPS infrastructure** (`crates/patina-tree-walker/src/eval/cps_eval.rs`):
   - ✅ `call/cc` - Full implementation
   - ✅ `dynamic-wind` - Full implementation with continuation re-entry
   - ✅ `ExceptionHandler` struct with handler stack
   - ✅ `exception_handlers` threaded through all `StepResult` variants
   - ✅ `ExceptionHandlerCleanup` continuation for normal completion
   - ✅ `RaiseHandlerReturn` continuation for handler returns

5. **`guard` macro** (`lib/scheme/base/exceptions.scm`):
   - ✅ Basic guard with else clause
   - ✅ Guard with condition clauses
   - ✅ Multiple body expressions

### ✅ All Features Implemented

All R7RS exception handling features are now fully working:

1. **`with-exception-handler`** - Install exception handlers in the CPS handler stack
2. **`raise`** - Non-continuable exceptions routed through CPS handlers
3. **`raise-continuable`** - Continuable exceptions with handler return value
4. **`error`** - Create and raise error objects with message and irritants
5. **`guard`** - Scheme macro for structured exception handling
6. **I/O errors** - File I/O errors (open, close, read, write, delete) are caught by handlers
7. **Read errors** - Parse errors from `read` are caught by handlers

The key implementation was wrapping primitive calls in `maybe_route_error_through_cps()` which converts
`IOError` and `InvalidSyntax` errors to CPS-routed exceptions when handlers are installed.

---

## The Core Challenge: CPS Integration

### Why Simple Implementation Doesn't Work

The current implementation raises exceptions as `EvalError::SchemeException`, which propagates through Rust's `Result` type. This means:

1. Exceptions bubble up to the Rust top-level
2. Scheme code cannot catch them
3. `with-exception-handler` cannot intercept exceptions

### What We Need

For `with-exception-handler` to work, exceptions must be caught **within the CPS evaluator** and routed to the installed handler. This requires:

1. **Exception handler stack** in CPS state
2. **Exception-aware continuation application**
3. **Handler invocation** when exception occurs

---

## Detailed Implementation Plan

### Step 1: Add Exception Handler Stack to CPS State

**File:** `crates/patina-tree-walker/src/eval/cps_eval.rs`

```rust
// Add to CpsEvaluator struct or pass through StepResult
struct ExceptionHandler {
    handler: Value,           // The handler procedure
    escape_cont: ContValue,   // Continuation to escape to if handler returns
}

// In StepResult variants, add:
exception_handlers: Vec<ExceptionHandler>,
```

The exception handler stack must be:
- Part of the dynamic extent (like `dynamic_winds`)
- Passed through all `StepResult` variants
- Saved/restored when capturing/invoking continuations

### Step 2: Implement `with-exception-handler` in CPS

**Location:** Add case in `apply_cps_step` for `with-exception-handler`

```rust
"with-exception-handler" => {
    // (with-exception-handler handler thunk)
    if args.len() != 2 {
        return Err(EvalError::WrongArity { ... });
    }

    let handler = args[0].clone();
    let thunk = args[1].clone();

    // Create a continuation that will:
    // 1. Pop the exception handler when thunk completes normally
    // 2. Pass the result to the original continuation
    let cleanup_cont = ContValue::ExceptionHandlerCleanup {
        original_cont: Box::new(cont),
    };

    // Create new exception handler entry
    let new_handler = ExceptionHandler {
        handler: handler.clone(),
        escape_cont: cont.clone(),  // For raise-continuable
    };

    // Push handler and call thunk
    let mut new_handlers = exception_handlers.clone();
    new_handlers.push(new_handler);

    Ok(StepResult::ApplyProc {
        proc: thunk,
        args: vec![],
        cont: cleanup_cont,
        env: self.evaluator.global_env.clone(),
        cont_env,
        prompt_stack,
        dynamic_winds,
        exception_handlers: new_handlers,  // NEW
    })
}
```

### Step 3: Add New ContValue Variant

```rust
enum ContValue {
    // ... existing variants ...

    /// Cleanup continuation for with-exception-handler
    /// Pops the exception handler when the body completes normally
    ExceptionHandlerCleanup {
        original_cont: Box<ContValue>,
    },
}
```

### Step 4: Modify `raise` to Use CPS Exception Handling

Instead of returning `Err(EvalError::SchemeException(...))`, `raise` should:

```rust
"raise" => {
    if args.len() != 1 {
        return Err(EvalError::WrongArity { ... });
    }

    let exception = args[0].clone();

    // Get current exception handler
    if let Some(handler_entry) = exception_handlers.last() {
        // Pop the handler (one-shot)
        let mut new_handlers = exception_handlers[..exception_handlers.len()-1].to_vec();

        // Call handler with exception
        // If handler returns, raise secondary exception (for non-continuable)
        let raise_secondary_cont = ContValue::RaiseSecondary {
            exception: exception.clone(),
        };

        Ok(StepResult::ApplyProc {
            proc: handler_entry.handler.clone(),
            args: vec![exception],
            cont: raise_secondary_cont,
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers: new_handlers,
        })
    } else {
        // No handler - propagate to Rust level
        Err(EvalError::SchemeException { ... })
    }
}
```

### Step 5: Add `RaiseSecondary` Continuation

```rust
ContValue::RaiseSecondary { exception } => {
    // Handler returned without escaping - this is an error for non-continuable raise
    Err(EvalError::SchemeException {
        kind: SchemeExceptionKind::Error,
        message: "exception handler returned".to_string(),
        irritants_display: format!("{}", exception),
    })
}
```

### Step 6: Handle `raise-continuable` Differently

For `raise-continuable`, if the handler returns, use that value:

```rust
"raise-continuable" => {
    // Similar to raise, but handler return value becomes result
    if let Some(handler_entry) = exception_handlers.last() {
        let mut new_handlers = exception_handlers[..exception_handlers.len()-1].to_vec();

        // For continuable, use original continuation
        // Handler's return value continues from raise-continuable
        Ok(StepResult::ApplyProc {
            proc: handler_entry.handler.clone(),
            args: vec![exception],
            cont: cont,  // Continue with raise-continuable's continuation
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers: new_handlers,
        })
    } else {
        Err(EvalError::SchemeException { ... })
    }
}
```

### Step 7: Update All StepResult Variants

Every `StepResult` variant that carries state needs to include `exception_handlers`:

```rust
enum StepResult {
    Done(Value),
    Eval {
        expr: Rc<CpsExpr>,
        env: Rc<Environment>,
        cont: ContValue,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,  // ADD
    },
    ApplyProc {
        proc: Value,
        args: Vec<Value>,
        cont: ContValue,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,  // ADD
    },
    InvokeCont {
        cont: ContValue,
        value: Value,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,  // ADD
    },
}
```

### Step 8: Implement `guard` as Scheme Macro

Once `with-exception-handler` works, add to `lib/scheme/base/exceptions.scm`:

```scheme
(define-syntax guard
  (syntax-rules (else)
    ;; Case with else clause
    ((guard (var clause ... (else result1 result2 ...)) body1 body2 ...)
     (call-with-current-continuation
       (lambda (guard-k)
         (with-exception-handler
           (lambda (condition)
             (guard-k
               (let ((var condition))
                 (cond clause ... (else result1 result2 ...)))))
           (lambda () body1 body2 ...)))))

    ;; Case without else clause - re-raise if no match
    ((guard (var clause ...) body1 body2 ...)
     (call-with-current-continuation
       (lambda (guard-k)
         (with-exception-handler
           (lambda (condition)
             (let ((var condition))
               (let ((result (cond clause ... (else #f))))
                 (if result
                     (guard-k result)
                     (raise condition)))))
           (lambda () body1 body2 ...)))))))
```

Update `lib/scheme/base.sld`:
```scheme
(include "base/exceptions.scm")
(export guard)
```

---

## Testing Strategy

After implementation, these tests should pass:

```scheme
;; Test 1: with-exception-handler + call/cc
(call-with-current-continuation
  (lambda (k)
    (with-exception-handler
      (lambda (x) (k 'caught))
      (lambda () (raise 'boom)))))
;; => caught

;; Test 2: raise-continuable
(with-exception-handler
  (lambda (con) 42)
  (lambda () (+ (raise-continuable "should be a number") 23)))
;; => 65

;; Test 3: guard with else
(guard (exn (else 'caught))
  (raise 'test))
;; => caught

;; Test 4: guard with specific clause
(guard (exn
        ((error-object? exn) (error-object-message exn))
        (else 'other))
  (error "test message"))
;; => "test message"

;; Test 5: guard re-raise
(guard (exn
        ((eq? exn 'specific) 'matched))
  (guard (exn
          (else 'inner-caught))
    (raise 'different)))
;; => inner-caught
```

---

## Estimated Remaining Work

| Task | Effort |
|------|--------|
| Add `exception_handlers` to StepResult | 0.5 day |
| Implement `with-exception-handler` in CPS | 0.5 day |
| Implement CPS-aware `raise`/`raise-continuable` | 0.25 day |
| Add `ExceptionHandlerCleanup` continuation | 0.25 day |
| Implement `guard` macro in Scheme | 0.25 day |
| Testing and debugging | 0.25 day |
| **Total** | **~2 days** |

---

## Alternative: Simpler Approach

If full CPS integration proves too complex, a simpler approach:

1. Keep exceptions as `EvalError::SchemeException`
2. Implement `guard` as a special form in Rust that wraps evaluation in a try-catch
3. This is not fully R7RS compliant but handles common cases

```rust
// Simplified guard in Rust
fn eval_guard(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // Evaluate body, catching SchemeException
    match self.eval_body(body, env) {
        Ok(val) => Ok(val),
        Err(EvalError::SchemeException { kind, message, .. }) => {
            // Convert to exception object and evaluate clauses
            let exc = create_exception(kind, message);
            self.eval_guard_clauses(clauses, exc, env)
        }
        Err(other) => Err(other),  // Re-raise other errors
    }
}
```

This approach:
- ✅ Works for common `guard` usage
- ✅ Doesn't require CPS changes
- ❌ Doesn't support `with-exception-handler`
- ❌ `raise-continuable` can't work properly
- ❌ Nested handlers don't work correctly

---

## References

- **R7RS Spec:** Section 6.11 (Exceptions)
- **R7RS Spec:** Section 4.2.7 (Guard syntax)
- **Chibi implementation:** `lib/init-7.scm` (exception handling)
- **CPS evaluator:** `crates/patina-tree-walker/src/eval/cps_eval.rs`
