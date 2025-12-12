# Unified Error System Design

**Status**: ✅ Phase 1-4 Complete (source locations deferred to SOURCE_INFO_PLAN.md)
**Created**: 2025-12-12
**Updated**: 2025-12-12
**Related**: `POST_CPS_TECH_DEBT.md` item 5, `TECH_DEBT_CLEANUP.md` item 14

## Overview

This document designs a unified error handling system for Patina that provides:
1. Clear separation between recoverable (Scheme-catchable) and unrecoverable (Rust-only) errors
2. Consistent error types across all compilation/interpretation phases
3. Proper R7RS exception compliance
4. Good developer experience with rich error context

---

## Current State Analysis

### Error Types by Crate

| Crate | Error Type | Purpose |
|-------|------------|---------|
| `patina-core` | `NumericError` | Numeric operation failures |
| `patina-core` | `ExceptionKind`, `ExceptionObject` | Scheme-level exceptions |
| `patina-frontend` | `FrontendError` | Lexing, parsing, macro errors |
| `patina-frontend` | `LexError` | Tokenization errors |
| `patina-frontend` | `ParseError` | Parsing errors |
| `patina-frontend` | `DesugarError` | AST to CoreExpr errors |
| `patina-macros` | `MacroError` | Macro expansion errors |
| `patina-runtime` | `RuntimeError` | General runtime errors (rarely used) |
| `patina-pipeline` | `PipelineError` | Orchestration errors |
| `patina-tree-walker` | `EvalError` | Evaluation errors |
| `patina-tree-walker` | `SchemeExceptionKind` | Duplicate of ExceptionKind |

### Current Error Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              Source Code                                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Lexer                            LexError                                    │
│   └── tokens                       │                                         │
│                                    ▼                                         │
│ Parser                           ParseError                                  │
│   └── Value AST                    │                                         │
│                                    ▼                                         │
│ Macro Expander                   MacroError                                  │
│   └── Expanded AST                 │                                         │
│                                    ▼                                         │
│ Desugarer                        DesugarError                                │
│   └── CoreExpr                     │                                         │
└─────────────────────────────────────────────────────────────────────────────┘
                     All wrapped as ──► FrontendError ──► PipelineError
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ CPS Evaluator                    EvalError                                   │
│   │                                │                                         │
│   ├── Primitives                   ├── TypeError                             │
│   │     └── NumericError ─────────►├── WrongArity                            │
│   │                                ├── DivisionByZero                        │
│   │                                ├── IndexOutOfBounds                      │
│   │                                ├── IOError                               │
│   │                                ├── UndefinedVariable                     │
│   │                                ├── NotAProcedure                         │
│   │                                ├── InvalidSyntax                         │
│   │                                ├── InternalError                         │
│   │                                ├── ContinuationEscape                    │
│   │                                └── SchemeException                       │
│   │                                                                          │
│   └── Exception Handlers ──────────► Value::Exception(ExceptionObject)       │
│         (only IOError, InvalidSyntax currently routed)                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Problems with Current Design

1. **Duplicate types**: `SchemeExceptionKind` in eval/error.rs duplicates `ExceptionKind` in patina-core
2. **Inconsistent routing**: Only `IOError` and `InvalidSyntax` go through Scheme exception handlers
3. **Lost context**: Errors lose source location and expansion trace when converted between types
4. **No unification**: Each crate defines its own error type with overlapping variants
5. **Unclear boundaries**: Not obvious which errors are catchable in Scheme

---

## Design Goals

1. **Single source of truth** for exception kinds
2. **All runtime errors catchable in Scheme** (except bugs)
3. **Rich error context** preserved through transformations
4. **Source locations** on all errors where available
5. **Clear Rust vs Scheme boundary**

---

## Proposed Architecture

### Error Categories

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    SCHEME-CATCHABLE EXCEPTIONS                               │
│                    (Can be caught by guard/with-exception-handler)           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐              │
│  │   error         │  │   file-error    │  │   read-error    │              │
│  │   (default)     │  │   (I/O)         │  │   (parsing)     │              │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘              │
│           │                    │                    │                        │
│           ▼                    ▼                    ▼                        │
│  ┌─────────────────────────────────────────────────────────────────────────┐│
│  │                     Value::Exception(ExceptionObject)                   ││
│  │                                                                         ││
│  │  - TypeError (wrong type passed to procedure)                           ││
│  │  - ArityError (wrong number of arguments)                               ││
│  │  - DomainError (division by zero, sqrt of negative, etc.)               ││
│  │  - BoundsError (index out of bounds)                                    ││
│  │  - LookupError (undefined variable)                                     ││
│  │  - ApplicationError (not a procedure)                                   ││
│  │  - IOError (file not found, permission denied)                          ││
│  │  - ReadError (parse error, invalid syntax in input)                     ││
│  │  - SyntaxError (macro expansion, invalid form)                          ││
│  │  - UserError (raised by `error` procedure)                              ││
│  └─────────────────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                    RUST-ONLY ERRORS                                          │
│                    (Implementation bugs, control flow - NOT catchable)       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  - InternalError: Invariant violation, interpreter bug                       │
│  - ContinuationEscape: Control flow for call/cc (not a real error)          │
│  - ResourceExhausted: Stack overflow, out of memory (if we handle these)    │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Unified Error Type

Move error handling to `patina-core` as the single source of truth:

```rust
// In patina-core/src/error.rs (NEW FILE)

use std::rc::Rc;

/// Source location for error reporting
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// Source file or "<repl>" or "<string>"
    pub source: Rc<str>,
    /// 1-indexed line number
    pub line: u32,
    /// 1-indexed column number
    pub column: u32,
    /// Optional span length
    pub length: Option<u32>,
}

/// Detailed error information that can be converted to Scheme exception
#[derive(Debug, Clone)]
pub struct ErrorDetail {
    /// The kind of error
    pub kind: ErrorKind,
    /// Human-readable message
    pub message: String,
    /// Values related to the error (irritants in R7RS)
    pub irritants: Vec<Value>,
    /// Where the error occurred
    pub location: Option<SourceLocation>,
    /// Expansion trace for macro errors
    pub expansion_trace: Vec<ExpansionStep>,
}

/// Classification of errors
///
/// This determines:
/// 1. Which R7RS predicate matches (error-object?, file-error?, read-error?)
/// 2. Whether the error is catchable in Scheme
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    // === Catchable errors (become Scheme exceptions) ===

    /// Type mismatch: expected X, got Y
    Type,

    /// Wrong number of arguments
    Arity,

    /// Domain error (div by zero, invalid argument value)
    Domain,

    /// Index/key out of bounds
    Bounds,

    /// Variable not found
    Lookup,

    /// Tried to call non-procedure
    Application,

    /// File I/O error - maps to file-error? predicate
    FileIO,

    /// Read/parse error - maps to read-error? predicate
    Read,

    /// Syntax error (macro expansion, invalid special form)
    Syntax,

    /// User-raised error via (error ...)
    User,

    // === Non-catchable errors (stay in Rust) ===

    /// Internal interpreter bug - should never happen
    Internal,

    /// Control flow mechanism, not a real error
    ControlFlow,
}

impl ErrorKind {
    /// Is this error catchable by Scheme's guard/with-exception-handler?
    pub fn is_catchable(&self) -> bool {
        !matches!(self, ErrorKind::Internal | ErrorKind::ControlFlow)
    }

    /// Map to R7RS ExceptionKind for Scheme-level handling
    pub fn to_exception_kind(&self) -> ExceptionKind {
        match self {
            ErrorKind::FileIO => ExceptionKind::FileError,
            ErrorKind::Read => ExceptionKind::ReadError,
            _ => ExceptionKind::Error,
        }
    }
}

impl ErrorDetail {
    /// Convert to Scheme exception object
    pub fn to_exception(&self) -> ExceptionObject {
        ExceptionObject {
            kind: self.kind.to_exception_kind(),
            message: self.format_message(),
            irritants: self.irritants.clone(),
        }
    }

    /// Format message with location if available
    pub fn format_message(&self) -> String {
        match &self.location {
            Some(loc) => format!("{}:{}:{}: {}", loc.source, loc.line, loc.column, self.message),
            None => self.message.clone(),
        }
    }
}
```

### Phase-Specific Errors

Each phase wraps `ErrorDetail` with phase-specific context:

```rust
// patina-frontend/src/error.rs

/// Frontend phase that produced the error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendPhase {
    Lexing,
    Parsing,
    MacroExpansion,
    Desugaring,
}

#[derive(Debug, Clone)]
pub struct FrontendError {
    pub phase: FrontendPhase,
    pub detail: ErrorDetail,
}

impl FrontendError {
    pub fn lex_error(msg: impl Into<String>, loc: Option<SourceLocation>) -> Self {
        Self {
            phase: FrontendPhase::Lexing,
            detail: ErrorDetail {
                kind: ErrorKind::Read,  // Lex errors are read errors
                message: msg.into(),
                irritants: vec![],
                location: loc,
                expansion_trace: vec![],
            },
        }
    }

    pub fn parse_error(msg: impl Into<String>, loc: Option<SourceLocation>) -> Self {
        Self {
            phase: FrontendPhase::Parsing,
            detail: ErrorDetail {
                kind: ErrorKind::Read,  // Parse errors are read errors
                message: msg.into(),
                irritants: vec![],
                location: loc,
                expansion_trace: vec![],
            },
        }
    }

    pub fn macro_error(msg: impl Into<String>, trace: Vec<ExpansionStep>) -> Self {
        Self {
            phase: FrontendPhase::MacroExpansion,
            detail: ErrorDetail {
                kind: ErrorKind::Syntax,
                message: msg.into(),
                irritants: vec![],
                location: None,
                expansion_trace: trace,
            },
        }
    }

    pub fn desugar_error(msg: impl Into<String>, loc: Option<SourceLocation>) -> Self {
        Self {
            phase: FrontendPhase::Desugaring,
            detail: ErrorDetail {
                kind: ErrorKind::Syntax,
                message: msg.into(),
                irritants: vec![],
                location: loc,
                expansion_trace: vec![],
            },
        }
    }
}
```

```rust
// patina-tree-walker/src/eval/error.rs

/// Evaluation errors
///
/// Most variants are catchable in Scheme. Only Internal and ContinuationEscape
/// propagate as Rust errors without going through Scheme handlers.
#[derive(Debug)]
pub enum EvalError {
    /// Catchable error with full context
    Exception(ErrorDetail),

    /// Internal interpreter bug (not catchable)
    Internal(String),

    /// Control flow for call/cc (not a real error)
    ContinuationEscape,
}

impl EvalError {
    // Convenience constructors

    pub fn type_error(msg: impl Into<String>, got: Value) -> Self {
        Self::Exception(ErrorDetail {
            kind: ErrorKind::Type,
            message: msg.into(),
            irritants: vec![got],
            location: None,
            expansion_trace: vec![],
        })
    }

    pub fn arity_error(expected: &str, actual: usize) -> Self {
        Self::Exception(ErrorDetail {
            kind: ErrorKind::Arity,
            message: format!("expected {} arguments, got {}", expected, actual),
            irritants: vec![Value::Integer(actual as i64)],
            location: None,
            expansion_trace: vec![],
        })
    }

    pub fn division_by_zero() -> Self {
        Self::Exception(ErrorDetail {
            kind: ErrorKind::Domain,
            message: "division by zero".into(),
            irritants: vec![],
            location: None,
            expansion_trace: vec![],
        })
    }

    pub fn undefined_variable(name: &str) -> Self {
        Self::Exception(ErrorDetail {
            kind: ErrorKind::Lookup,
            message: format!("undefined variable: {}", name),
            irritants: vec![Value::symbol(name)],
            location: None,
            expansion_trace: vec![],
        })
    }

    pub fn index_out_of_bounds(index: i64, len: usize) -> Self {
        Self::Exception(ErrorDetail {
            kind: ErrorKind::Bounds,
            message: format!("index {} out of bounds for length {}", index, len),
            irritants: vec![Value::Integer(index), Value::Integer(len as i64)],
            location: None,
            expansion_trace: vec![],
        })
    }

    pub fn not_a_procedure(value: &Value) -> Self {
        Self::Exception(ErrorDetail {
            kind: ErrorKind::Application,
            message: format!("not a procedure: {}", value),
            irritants: vec![value.clone()],
            location: None,
            expansion_trace: vec![],
        })
    }

    pub fn io_error(msg: impl Into<String>) -> Self {
        Self::Exception(ErrorDetail {
            kind: ErrorKind::FileIO,
            message: msg.into(),
            irritants: vec![],
            location: None,
            expansion_trace: vec![],
        })
    }

    /// Check if this error should be routed through Scheme exception handlers
    pub fn is_catchable(&self) -> bool {
        match self {
            EvalError::Exception(detail) => detail.kind.is_catchable(),
            EvalError::Internal(_) => false,
            EvalError::ContinuationEscape => false,
        }
    }

    /// Convert to Scheme exception (for routing through handlers)
    pub fn to_scheme_exception(&self) -> Option<Value> {
        match self {
            EvalError::Exception(detail) if detail.kind.is_catchable() => {
                Some(Value::Exception(Rc::new(detail.to_exception())))
            }
            _ => None,
        }
    }
}
```

### Updated Exception Routing

```rust
// In cps_eval/exceptions.rs

impl<'a> CpsEvaluator<'a> {
    /// Route catchable errors through Scheme exception handlers
    pub(super) fn route_error(
        &self,
        err: EvalError,
        cont: ContValue,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // Non-catchable errors always propagate as Rust errors
        if !err.is_catchable() {
            return Err(err);
        }

        // No handlers installed - propagate as Rust error
        // (will become unhandled exception at top level)
        if exception_handlers.is_empty() {
            return Err(err);
        }

        // Convert to Scheme exception and route through handlers
        let exception = err.to_scheme_exception()
            .expect("catchable error must convert to exception");

        let handler_entry = exception_handlers.last().cloned().unwrap();
        let new_handlers = exception_handlers[..exception_handlers.len() - 1].to_vec();

        // Create continuation for when handler returns
        let raise_return_cont = ContValue::RaiseHandlerReturn {
            continuable: false,
            original_exception: Some(exception.clone()),
            original_cont: Box::new(cont),
        };

        Ok(StepResult::ApplyProc {
            proc: handler_entry.handler,
            args: vec![exception],
            cont: raise_return_cont,
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers: new_handlers,
        })
    }
}
```

---

## Migration Plan

### Phase 1: Unify Core Types (Low Risk) ✅ COMPLETE (2025-12-12)

1. ✅ Created `patina-core/src/error.rs` with `ErrorDetail`, `ErrorKind`, `SourceLocation`
2. ✅ `ExceptionKind` already aligned (Error, FileError, ReadError, Custom)
3. ✅ Removed duplicate `SchemeExceptionKind` from `patina-tree-walker/src/eval/error.rs`
4. ✅ Added conversion methods:
   - `EvalError::to_error_kind()` - Classify any EvalError
   - `EvalError::to_error_detail()` - Convert to rich error context
   - `From<ErrorDetail> for EvalError` - Convert back
   - `From<DesugarError> for InterpreterError` - Missing conversion added

### Phase 2: Update Frontend Errors (Medium Risk)

**Status**: 🔶 Partial (2025-12-12)

**Completed (without source tracking)**:
- ✅ Added `to_error_kind()` method to `FrontendError`
- ✅ Added `to_error_detail()` method to `FrontendError`
- ✅ Added `From<FrontendError> for ErrorDetail` conversion
- ✅ Added `to_error_kind()` method to `DesugarError`
- ✅ Added `to_error_detail()` method to `DesugarError`
- ✅ Added `From<DesugarError> for ErrorDetail` conversion
- ✅ Added `patina-core` dependency to `patina-frontend`

**Deferred (requires SOURCE_INFO_PLAN.md)**:
1. ⏳ Add source location tracking to lexer/parser
2. ⏳ Preserve macro expansion trace through error transformations
3. ⏳ Update errors to include `SourceLocation`

Source location tracking is a larger feature that requires implementing the
`Syntax` object system from `SOURCE_INFO_PLAN.md` first. The current changes
provide error kind categorization and ErrorDetail conversion without locations.

### Phase 3: Update Evaluation Errors (Medium Risk)

**Status**: ✅ COMPLETE (2025-12-12)

**Completed**:
- ✅ Expanded `maybe_route_error_through_cps` to handle ALL catchable errors:
  - `TypeError`, `WrongArity`, `DivisionByZero`, `IndexOutOfBounds`
  - `UndefinedVariable`, `NotAProcedure`, `IOError`, `InvalidSyntax`
  - `SchemeException` (pass-through)
- ✅ Updated `apply_cps_step` to route `NotAProcedure` and arity errors
- ✅ Updated `CpsExpr::Var` handling to route `UndefinedVariable` errors
- ✅ Updated `CpsExpr::Continue` to route errors from `eval_trivial`
- ✅ Added comprehensive tests in `cps_features.rs`:
  - `test_guard_catches_type_error`
  - `test_guard_catches_undefined_variable`
  - `test_guard_catches_arity_error`
  - `test_guard_catches_division_by_zero`
  - `test_guard_catches_bounds_error`
  - `test_guard_catches_application_error`
  - `test_with_exception_handler_catches_type_error`
  - `test_error_message_preserved`

**Deferred**:
- ⏳ Fix remaining risky `unwrap()` calls (low priority, 2 locations)

### Phase 4: End-to-End Testing

**Status**: ✅ COMPLETE (2025-12-12)

**Completed**:
- ✅ All R7RS exception tests pass (35 tests in cps_features.rs alone)
- ✅ `guard` catches all expected error types:
  - `test_guard_catches_type_error`
  - `test_guard_catches_undefined_variable`
  - `test_guard_catches_arity_error`
  - `test_guard_catches_division_by_zero`
  - `test_guard_catches_bounds_error`
  - `test_guard_catches_application_error`
  - `test_guard_catches_file_error` (file-error? predicate)
  - `test_guard_catches_read_error` (read-error? predicate)
- ✅ Error message preservation tested (`test_error_message_preserved`)
- ✅ Error irritants tested (`test_error_object_irritants`)

**Deferred** (requires SOURCE_INFO_PLAN.md):
- ⏳ Test error messages include source locations
- ⏳ Test macro expansion traces in error output

---

## Testing Strategy

### R7RS Compliance Tests

```scheme
;; TypeError should be catchable
(guard (ex ((error-object? ex) 'caught))
  (+ "not a number" 1))
;; => caught

;; ArityError should be catchable
(guard (ex ((error-object? ex) 'caught))
  ((lambda (x) x) 1 2 3))
;; => caught

;; DivisionByZero should be catchable
(guard (ex ((error-object? ex) 'caught))
  (/ 1 0))
;; => caught

;; UndefinedVariable should be catchable
(guard (ex ((error-object? ex) 'caught))
  undefined-variable)
;; => caught

;; file-error? predicate
(guard (ex ((file-error? ex) 'file-error))
  (open-input-file "/nonexistent/path"))
;; => file-error

;; read-error? predicate
(guard (ex ((read-error? ex) 'read-error))
  (read (open-input-string "(unclosed")))
;; => read-error
```

### Internal Error Tests

```rust
#[test]
fn internal_errors_not_catchable() {
    let code = r#"
        (guard (ex (#t 'caught))
          ; This would require triggering an InternalError
          ; which should NOT be caught
          ...)
    "#;
    // Internal errors should propagate past guard
}
```

---

## Benefits

1. **Consistency**: One `ErrorKind` enum for all error classification
2. **R7RS Compliance**: All user-facing errors catchable by `guard`
3. **Rich Context**: Source locations and expansion traces preserved
4. **Clear Boundaries**: Obvious which errors are Rust-only
5. **Better UX**: Error messages include file:line:column
6. **Debugging**: Macro expansion traces help debug macro errors

---

## Open Questions

1. **Stack traces**: Should we capture Scheme call stacks for errors?
2. **Conditions**: Should we support SRFI-35 style conditions?
3. **Restarts**: Should we support Common Lisp style restarts?
4. **Performance**: Is the overhead of `ErrorDetail` acceptable?

---

## References

- R7RS Section 6.11 (Exceptions)
- SRFI-35 (Conditions)
- Racket exception documentation
- Chibi-scheme error handling
