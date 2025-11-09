# R7RS Exception Handling Implementation Guide

**Status:** Not yet started (0%)
**Priority:** MEDIUM-HIGH (needed for I/O and error reporting)
**Estimated Effort:** 3-5 days
**Last Updated:** 2025-11-09

---

## Overview

R7RS requires a comprehensive exception handling system. We currently have **NONE** - we only have Rust-level `EvalError` that bubbles up to the top level.

R7RS exception handling is based on **dynamic exception handlers** similar to try/catch but more flexible.

---

## Current Status

### What We Have ✅
```rust
// src/eval/error.rs
pub enum EvalError {
    UndefinedVariable(String),
    NotAProcedure(String),
    WrongArity { expected: String, actual: usize },
    InvalidSyntax(String),
    TypeError(String),
    DivisionByZero,
    IndexOutOfBounds(String),
}
```

**Problem:** Errors propagate to Rust, not catchable in Scheme!

### What We Need ❌

```scheme
; User should be able to catch errors in Scheme
(guard (exn
         ((error-object? exn)
          (display "Error: ")
          (display (error-object-message exn))
          'recovered))
  (/ 1 0))  ; Should catch division by zero
; => recovered
```

---

## R7RS Requirements

### 1. Exception Handler Infrastructure (3 procedures)

**Priority:** HIGH (foundation for everything)
**Difficulty:** Medium (need dynamic environment support)

```scheme
(with-exception-handler handler thunk)
; Install handler as current exception handler, call thunk
; handler: (lambda (obj) ...) - one argument
; thunk: (lambda () ...) - zero arguments

(raise obj)
; Raise exception by calling current handler
; Handler is called, if it returns, raises secondary exception

(raise-continuable obj)
; Like raise, but if handler returns, its values become raise-continuable's values
```

**Key Concept:** Current exception handler is maintained in **dynamic environment**

---

### 2. Guard Syntax (1 special form)

**Priority:** HIGH (user-facing exception handling)
**Difficulty:** Medium-Hard (complex syntax, uses cond-like matching)

```scheme
(guard (variable
         clause1
         clause2
         ...)
  body ...)

; Example:
(guard (exn
         ((file-error? exn) 'file-error)
         ((read-error? exn) 'read-error)
         (else 'other-error))
  (open-input-file "nonexistent.txt"))
; => file-error
```

**Semantics:**
1. Evaluate body with exception handler installed
2. If exception raised, bind it to variable
3. Evaluate clauses like `cond` to determine result
4. If no clause matches, re-raise exception

---

### 3. Error Procedure (1 procedure)

**Priority:** HIGH (user error signaling)
**Difficulty:** Easy

```scheme
(error message obj ...)
; Create error object and raise it
; message: string describing the error
; obj ...: "irritants" - values related to error

(error "division by zero" 1 0)
; Raises error with message and irritants
```

---

### 4. Error Object Predicates (4 procedures)

**Priority:** MEDIUM (needed for guard clauses)
**Difficulty:** Easy

```scheme
(error-object? obj)
; Is obj an error object created by error?

(error-object-message error-object)
; Get the error message string

(error-object-irritants error-object)
; Get list of irritants

(file-error? obj)
; Is obj a file I/O error?

(read-error? obj)
; Is obj a read/parse error?
```

---

## Exception Handling Flow

### Without Guard (Current Behavior)
```
Scheme code
  ↓ error occurs
  ↓
Rust EvalError
  ↓
Propagates to top
  ↓
Program terminates
```

### With R7RS Exception Handling
```
Scheme code
  ↓ (error "boom!")
  ↓
Current exception handler called
  ↓
Handler decides what to do:
  - Return a value (if raised with raise-continuable)
  - Raise another exception
  - Jump to guard rescue clause
```

---

## Implementation Strategy

### Phase 1: Exception Infrastructure (2 days)

**Goal:** Basic exception raising and handling

**Step 1: Add Exception Value Types**
```rust
// src/value/mod.rs
pub enum Value {
    // ... existing variants ...

    // Exception object
    Exception(Rc<ExceptionObject>),
}

pub struct ExceptionObject {
    kind: ExceptionKind,
    message: String,
    irritants: Vec<Value>,
}

pub enum ExceptionKind {
    Error,           // Created by (error ...)
    FileError,       // File I/O errors
    ReadError,       // Parse/read errors
    Custom(String),  // User-defined
}
```

**Step 2: Add Dynamic Exception Handler**
```rust
// src/eval/mod.rs
pub struct Evaluator {
    global_env: Rc<Environment>,
    debug: Rc<DebugConfig>,
    exception_handler_stack: Rc<RefCell<Vec<Value>>>, // NEW!
}

impl Evaluator {
    fn current_exception_handler(&self) -> Value {
        self.exception_handler_stack.borrow()
            .last()
            .cloned()
            .unwrap_or_else(|| default_exception_handler())
    }

    fn push_exception_handler(&self, handler: Value) {
        self.exception_handler_stack.borrow_mut().push(handler);
    }

    fn pop_exception_handler(&self) {
        self.exception_handler_stack.borrow_mut().pop();
    }
}

fn default_exception_handler() -> Value {
    // Returns a lambda that prints the error and exits
    // (lambda (obj) (display "Unhandled exception: ") (write obj) (newline))
}
```

**Step 3: Implement `with-exception-handler`**
```rust
// src/eval/special_forms.rs
pub(super) fn eval_with_exception_handler(
    &self,
    args: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    // Parse: (with-exception-handler handler thunk)
    let (handler_expr, rest) = self.extract_pair(args)?;
    let (thunk_expr, end) = self.extract_pair(&rest)?;

    // Evaluate handler and thunk
    let handler = self.eval_in_env(&handler_expr, env)?;
    let thunk = self.eval_in_env(&thunk_expr, env)?;

    // Verify types
    match (&handler, &thunk) {
        (Value::Lambda(_), Value::Lambda(_)) => {},
        _ => return Err(EvalError::TypeError("...".to_string())),
    }

    // Install handler and call thunk
    self.push_exception_handler(handler);
    let result = self.apply(thunk, vec![]);
    self.pop_exception_handler();

    result
}
```

**Step 4: Implement `raise`**
```rust
// src/eval/primitives/exceptions.rs (NEW FILE)
pub fn raise(evaluator: &Evaluator, args: &[Value]) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity { ... });
    }

    let exception_obj = &args[0];
    let handler = evaluator.current_exception_handler();

    // Call handler with exception object
    let result = evaluator.apply(handler, vec![exception_obj.clone()])?;

    // If handler returns, raise secondary exception
    // (as per R7RS spec)
    Err(EvalError::SecondaryException(
        "Exception handler returned without handling exception".to_string()
    ))
}
```

---

### Phase 2: Guard Syntax (1-2 days)

**Goal:** Implement `guard` special form

**Challenge:** Guard is complex - it's like a combination of `with-exception-handler` and `cond`

```scheme
(guard (exn
         ((file-error? exn) 'file-error)
         ((read-error? exn) 'read-error)
         (else 'unknown))
  body)

; Expands conceptually to:
(call-with-current-continuation
  (lambda (guard-k)
    (with-exception-handler
      (lambda (condition)
        (guard-k
          (cond
            ((file-error? condition) 'file-error)
            ((read-error? condition) 'read-error)
            (else 'unknown))))
      (lambda () body))))
```

**Implementation:**
```rust
// src/eval/special_forms.rs
pub(super) fn eval_guard(
    &self,
    args: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    // Parse: (guard (variable clause ...) body ...)
    let (guard_clause, body_exprs) = self.extract_pair(args)?;

    // Parse guard clause: (variable clause ...)
    let (var_expr, clauses_list) = self.extract_pair(&guard_clause)?;
    let var_name = match var_expr {
        Value::Symbol(s) => s.clone(),
        _ => return Err(EvalError::InvalidSyntax("guard variable must be symbol".into())),
    };

    // Parse clauses (like cond)
    let clauses = self.parse_cond_clauses(&clauses_list)?;

    // Create exception handler that evaluates clauses
    // This is tricky - need to capture current continuation!
    // May need to implement call/cc first...

    // For now, simpler implementation without call/cc:
    // Try to evaluate body, catch exceptions in Rust,
    // convert to Scheme exception and evaluate clauses

    todo!("Guard requires call/cc for full R7RS compliance")
}
```

**Note:** Full `guard` implementation requires `call-with-current-continuation`!

---

### Phase 3: Error Procedures (1 day)

**Goal:** User-friendly error signaling

```rust
// src/eval/primitives/exceptions.rs
pub fn error(args: &[Value]) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity { ... });
    }

    let message = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => return Err(EvalError::TypeError("error message must be string".into())),
    };

    let irritants = args[1..].to_vec();

    let error_obj = Value::Exception(Rc::new(ExceptionObject {
        kind: ExceptionKind::Error,
        message,
        irritants,
    }));

    // Raise the error
    raise_exception(error_obj)
}

pub fn error_object_p(args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Boolean(matches!(args[0], Value::Exception(_))))
}

pub fn error_object_message(args: &[Value]) -> Result<Value, EvalError> {
    match &args[0] {
        Value::Exception(exc) => {
            Ok(Value::String(Rc::new(RefCell::new(exc.message.clone()))))
        }
        _ => Err(EvalError::TypeError("not an error object".into())),
    }
}

pub fn error_object_irritants(args: &[Value]) -> Result<Value, EvalError> {
    match &args[0] {
        Value::Exception(exc) => {
            Ok(list_from_vec(exc.irritants.clone()))
        }
        _ => Err(EvalError::TypeError("not an error object".into())),
    }
}
```

---

## Integration with Existing Error Handling

### Convert Rust Errors to Scheme Exceptions

```rust
// src/eval/mod.rs
impl Evaluator {
    pub fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>)
        -> Result<Value, EvalError>
    {
        // Existing evaluation code...

        // But now, certain errors should create Scheme exceptions:
        match result {
            Err(EvalError::DivisionByZero) => {
                // Create error object
                let error_obj = Value::Exception(Rc::new(ExceptionObject {
                    kind: ExceptionKind::Error,
                    message: "division by zero".to_string(),
                    irritants: vec![],
                }));

                // Call current exception handler
                self.raise_scheme_exception(error_obj)
            }
            other => other
        }
    }
}
```

---

## Simplified Implementation (Without call/cc)

For **Phase 1 implementation** without continuations:

```rust
// Simplified guard that doesn't require call/cc
// but isn't fully R7RS compliant
pub(super) fn eval_guard_simple(
    &self,
    args: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    let (guard_clause, body_exprs) = self.extract_pair(args)?;
    let (var_expr, clauses_list) = self.extract_pair(&guard_clause)?;

    let var_name = extract_symbol(var_expr)?;
    let body = self.collect_list_items(&body_exprs)?;

    // Evaluate body with exception handler
    let result = (|| {
        for expr in &body {
            let val = self.eval_in_env(expr, env)?;
            // Last value
            if expr == body.last().unwrap() {
                return Ok(val);
            }
        }
        Ok(Value::Unspecified)
    })();

    // If error occurred, evaluate guard clauses
    match result {
        Ok(val) => Ok(val),
        Err(eval_err) => {
            // Convert EvalError to exception object
            let exception = self.error_to_exception(eval_err);

            // Bind exception to variable
            let guard_env = Rc::new(Environment::with_parent(env.clone()));
            guard_env.define(var_name.to_string(), exception);

            // Evaluate guard clauses (like cond)
            self.eval_guard_clauses(&clauses_list, &guard_env)
        }
    }
}
```

---

## Testing Strategy

```rust
// tests/compliance/exceptions.rs (NEW)

#[test]
fn test_error_creates_exception() {
    assert_program_eval_to(
        r#"
        (guard (exn
                 ((error-object? exn)
                  (error-object-message exn)))
          (error "test error" 1 2 3))
        "#,
        "\"test error\"",
    );
}

#[test]
fn test_guard_catches_division_by_zero() {
    assert_program_eval_to(
        r#"
        (guard (exn
                 (else 'caught))
          (/ 1 0))
        "#,
        "caught",
    );
}

#[test]
fn test_with_exception_handler() {
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (k)
            (with-exception-handler
              (lambda (x)
                (k 'caught))
              (lambda ()
                (error "boom!")))))
        "#,
        "caught",
    );
}

#[test]
fn test_file_error() {
    assert_program_eval_to(
        r#"
        (guard (exn
                 ((file-error? exn) 'file-error)
                 (else 'other))
          (open-input-file "/nonexistent/file.txt"))
        "#,
        "file-error",
    );
}
```

---

## Dependencies

### Required First
- ✅ None! Can implement basic exception handling now

### Enhanced (Optional)
- ❌ `call-with-current-continuation` - For full `guard` R7RS compliance
- ❌ I/O system - For `file-error?`
- ❌ `read` - For `read-error?`

---

## Implementation Phases

### Phase 1: Basic Error Raising (1-2 days)

**Can start now!**

```scheme
; Implement these:
(error message obj ...)
(error-object? obj)
(error-object-message error-obj)
(error-object-irritants error-obj)

; Test:
(error "test" 1 2)  ; Should signal error
```

---

### Phase 2: Exception Handlers (1-2 days)

```scheme
; Implement:
(with-exception-handler handler thunk)
(raise obj)
(raise-continuable obj)

; Test:
(with-exception-handler
  (lambda (e) (display "caught!") 42)
  (lambda () (error "boom!")))
; Should print "caught!" and... ? (depends on implementation)
```

**Note:** Without call/cc, behavior may differ from R7RS

---

### Phase 3: Guard (1 day - simplified, or 2-3 days with call/cc)

```scheme
; Implement:
(guard (var clause ...) body ...)

; Test:
(guard (exn (else 'caught))
  (error "test"))
; => caught
```

---

## Summary

### What We Have Now ❌
- Rust-level errors only
- No Scheme-level exception handling
- Errors terminate program

### What We Need ✅

**Minimum (3 days):**
1. Exception value type
2. `error` procedure
3. Error predicates
4. Simple `guard` (without call/cc)
5. Convert some EvalErrors to exceptions

**Complete (5 days):**
6. `with-exception-handler`
7. `raise` / `raise-continuable`
8. Full `guard` with call/cc
9. `file-error?` / `read-error?`

### Priority Decision

**Recommended:** Implement Phase 1-2 (basic error raising + handlers) **after I/O Phase 1**

**Rationale:**
1. I/O needs error handling (file-error?)
2. But I/O Phase 1 (string ports) doesn't need files yet
3. Can implement basic I/O first, then add exceptions, then file I/O

**Timeline:**
1. I/O Phase 1 (string ports, display/write) - 2-3 days
2. Exception Phases 1-2 (error + handlers) - 2-3 days
3. I/O Phase 2 (files + file-error?) - 2-3 days
4. Exception Phase 3 (full guard) - 1-2 days

**Total: ~10 days for I/O + Exceptions**

---

## References

- **R7RS Spec:** Section 6.11 (Exceptions)
- **R7RS Spec:** Section 4.2.7 (Guard syntax)
- **Chibi Tests:** Exception handling examples in r7rs-tests.scm
