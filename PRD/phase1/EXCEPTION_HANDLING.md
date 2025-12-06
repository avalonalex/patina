# R7RS Exception Handling Implementation Guide

**Status:** Not yet started (0%)
**Priority:** MEDIUM-HIGH (needed for error predicates in I/O)
**Estimated Effort:** 3-5 days
**Last Updated:** 2025-12-06

---

## Overview

R7RS requires a comprehensive exception handling system. We currently have **NONE** - we only have Rust-level `EvalError` that bubbles up to the top level.

R7RS exception handling is based on **dynamic exception handlers** similar to try/catch but more flexible.

**Dependent Features:**
- `file-error?`, `read-error?` predicates in [IO_IMPLEMENTATION.md](./IO_IMPLEMENTATION.md)

---

## Current Status

### What We Have ✅
```rust
// crates/patina-tree-walker/src/eval/error.rs
pub enum EvalError {
    UndefinedVariable(String),
    NotAProcedure(String),
    WrongArity { expected: String, actual: usize },
    InvalidSyntax(String),
    TypeError(String),
    DivisionByZero,
    IndexOutOfBounds(String),
    IOError(String),           // Added for I/O operations
    InternalError(String),     // For unexpected errors
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
// crates/patina-runtime/src/value/mod.rs
pub enum Value {
    // ... existing 26 variants ...

    // Exception object (NEW)
    Exception(Rc<ExceptionObject>),
}

pub struct ExceptionObject {
    kind: ExceptionKind,
    message: String,
    irritants: Vec<Value>,
}

pub enum ExceptionKind {
    Error,           // Created by (error ...)
    FileError,       // File I/O errors (maps from EvalError::IOError)
    ReadError,       // Parse/read errors (maps from FrontendError)
    Custom(String),  // User-defined
}
```

**Step 2: Add Dynamic Exception Handler**
```rust
// crates/patina-tree-walker/src/eval/mod.rs
pub struct Evaluator {
    pub global_env: Rc<Environment>,
    pub library_registry: Rc<RefCell<LibraryRegistry>>,
    pub loader_registry: Rc<RefCell<LibraryLoaderRegistry>>,
    pub primitive_registry: Rc<RefCell<PrimitiveRegistry>>,
    pub special_form_registry: Rc<RefCell<SpecialFormRegistry>>,
    pub debug: Rc<DebugConfig>,
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
// crates/patina-tree-walker/src/eval/special_forms/exception_handler.rs (NEW)
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
// crates/patina-tree-walker/src/eval/primitives/exceptions.rs (NEW FILE)
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
// crates/patina-tree-walker/src/eval/special_forms/guard.rs (NEW)
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
// crates/patina-tree-walker/src/eval/primitives/exceptions.rs
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

The codebase already has `EvalError::IOError` which is used throughout the I/O system
(in `crates/patina-tree-walker/src/eval/primitives/io.rs`). When exceptions are implemented,
these should be converted to Scheme exceptions:

```rust
// crates/patina-tree-walker/src/eval/core_eval.rs or mod.rs
impl Evaluator {
    fn eval_with_exception_handling(&self, ...) -> Result<Value, EvalError> {
        // Existing evaluation code...

        // Convert certain EvalErrors to Scheme exceptions:
        match result {
            Err(EvalError::DivisionByZero) => {
                // Create error object
                let error_obj = Value::Exception(Rc::new(ExceptionObject {
                    kind: ExceptionKind::Error,
                    message: "division by zero".to_string(),
                    irritants: vec![],
                }));
                self.raise_scheme_exception(error_obj)
            }
            Err(EvalError::IOError(msg)) => {
                // Create file-error object
                let error_obj = Value::Exception(Rc::new(ExceptionObject {
                    kind: ExceptionKind::FileError,
                    message: msg,
                    irritants: vec![],
                }));
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
- ✅ I/O system - For `file-error?` (COMPLETE - see [IO_IMPLEMENTATION.md](./IO_IMPLEMENTATION.md))
- ✅ `read` - For `read-error?` (COMPLETE - `read` is in `(scheme read)` library)

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
- Rust-level `EvalError` with IOError, TypeError, etc.
- No Scheme-level exception handling
- Errors propagate to Rust and terminate the Scheme program

### What We Need ✅

**Minimum (3 days):**
1. Exception value type (`Value::Exception`)
2. `error` procedure
3. Error predicates (`error-object?`, `error-object-message`, `error-object-irritants`)
4. Simple `guard` (without call/cc)
5. Convert `EvalError::IOError` to `file-error?`-compatible exceptions
6. Convert `FrontendError` parse errors to `read-error?`-compatible exceptions

**Complete (5 days):**
7. `with-exception-handler`
8. `raise` / `raise-continuable`
9. Full `guard` with call/cc
10. `file-error?` / `read-error?` predicates

### Current Codebase State (as of 2025-12-06)

**I/O is COMPLETE** - See [IO_IMPLEMENTATION.md](./IO_IMPLEMENTATION.md):
- All 6 phases of I/O implemented
- `EvalError::IOError` is already used throughout
- `read` procedure exists in `(scheme read)` library
- Only missing: `file-error?`, `read-error?` predicates (blocked on this document)

### Next Steps

1. **Phase 1**: Add `Value::Exception` type, `error` procedure, basic predicates
2. **Phase 2**: Add `with-exception-handler`, `raise`
3. **Phase 3**: Add `guard` (simple version without call/cc)
4. **Phase 4**: Add `file-error?`, `read-error?` (connects to I/O system)

---

## References

- **R7RS Spec:** Section 6.11 (Exceptions)
- **R7RS Spec:** Section 4.2.7 (Guard syntax)
- **Chibi Tests:** Exception handling examples in r7rs-tests.scm
