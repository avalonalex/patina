//! Exception handling primitives (R7RS Section 6.11)
//!
//! Implements:
//! - `error` - Create and raise an error object
//! - `error-object?` - Test if value is an error object
//! - `error-object-message` - Get error message
//! - `error-object-irritants` - Get error irritants
//! - `file-error?` - Test if value is a file error
//! - `read-error?` - Test if value is a read error
//! - `raise` - Raise an exception
//! - `raise-continuable` - Raise a continuable exception
//! - `with-exception-handler` - Install an exception handler

use std::cell::RefCell;
use std::rc::Rc;

use super::super::Evaluator;
use super::super::error::{EvalError, SchemeExceptionKind};
use patina_core::ExceptionKind;
use patina_runtime::value::Value;

/// (error message obj ...) - Create and raise an error object
///
/// Creates an error object with the given message and irritants, then raises it.
/// The message must be a string. The irritants can be any values.
pub(super) fn error(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "error")?;

    // First argument must be a string (the message)
    let message = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "error: first argument must be a string".to_string(),
            ));
        }
    };

    // Remaining arguments are irritants - serialize to display string
    let irritants_display = if args.len() > 1 {
        args[1..]
            .iter()
            .map(|v| format!("{}", v))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };

    // For now, just return an error that includes the exception info
    // When we have full exception handling, this will call raise
    Err(EvalError::SchemeException {
        kind: SchemeExceptionKind::Error,
        message,
        irritants_display,
    })
}

/// (error-object? obj) - Returns #t if obj is an error object
pub(super) fn error_object_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Exception(_)))
}

/// (error-object-message error-object) - Get the error message string
pub(super) fn error_object_message(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "error-object-message")?;

    match &args[0] {
        Value::Exception(exc) => Ok(Value::String(Rc::new(RefCell::new(exc.message.clone())))),
        _ => Err(EvalError::TypeError(
            "error-object-message: argument must be an error object".to_string(),
        )),
    }
}

/// (error-object-irritants error-object) - Get the list of irritants
pub(super) fn error_object_irritants(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "error-object-irritants")?;

    match &args[0] {
        Value::Exception(exc) => Ok(evaluator.list_from_vec(exc.irritants.clone())),
        _ => Err(EvalError::TypeError(
            "error-object-irritants: argument must be an error object".to_string(),
        )),
    }
}

/// (file-error? obj) - Returns #t if obj is a file error
pub(super) fn file_error_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| match v {
        Value::Exception(exc) => exc.kind == ExceptionKind::FileError,
        _ => false,
    })
}

/// (read-error? obj) - Returns #t if obj is a read error
pub(super) fn read_error_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| match v {
        Value::Exception(exc) => exc.kind == ExceptionKind::ReadError,
        _ => false,
    })
}

/// (raise obj) - Raise an exception
///
/// Raises the given object as an exception. If it's already an exception object,
/// raises it directly. Otherwise, wraps it in an exception.
pub(super) fn raise(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "raise expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let obj = &args[0];

    // If it's already an exception object, extract its info
    if let Value::Exception(exc) = obj {
        let kind = match &exc.kind {
            ExceptionKind::Error => SchemeExceptionKind::Error,
            ExceptionKind::FileError => SchemeExceptionKind::FileError,
            ExceptionKind::ReadError => SchemeExceptionKind::ReadError,
            ExceptionKind::Custom(s) => SchemeExceptionKind::Custom(s.clone()),
        };
        let irritants_display = exc
            .irritants
            .iter()
            .map(|v| format!("{}", v))
            .collect::<Vec<_>>()
            .join(" ");
        return Err(EvalError::SchemeException {
            kind,
            message: exc.message.clone(),
            irritants_display,
        });
    }

    // Otherwise, raise it as a generic exception with the object as the message
    Err(EvalError::SchemeException {
        kind: SchemeExceptionKind::Error,
        message: format!("{}", obj),
        irritants_display: String::new(),
    })
}

/// (raise-continuable obj) - Raise a continuable exception
///
/// Similar to raise, but allows the exception handler to return a value
/// that becomes the result of raise-continuable.
/// NOTE: This is currently a stub - proper implementation requires CPS integration.
pub(super) fn raise_continuable(
    _evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "raise-continuable expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    // For now, just raise like a non-continuable exception
    // Full implementation needs CPS exception handler support
    let obj = &args[0];
    Err(EvalError::SchemeException {
        kind: SchemeExceptionKind::Error,
        message: format!("continuable: {}", obj),
        irritants_display: String::new(),
    })
}

/// (with-exception-handler handler thunk) - Install an exception handler
///
/// Calls thunk with handler installed as the current exception handler.
/// If an exception is raised, handler is called with the exception object.
/// NOTE: This is currently a stub - proper implementation requires CPS integration.
pub(super) fn with_exception_handler(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "with-exception-handler")?;

    let _handler = &args[0];
    let thunk = &args[1];

    // Verify both are procedures
    if !matches!(_handler, Value::Procedure(_) | Value::Continuation(_)) {
        return Err(EvalError::TypeError(
            "with-exception-handler: first argument must be a procedure".to_string(),
        ));
    }
    if !matches!(thunk, Value::Procedure(_) | Value::Continuation(_)) {
        return Err(EvalError::TypeError(
            "with-exception-handler: second argument must be a procedure".to_string(),
        ));
    }

    // For now, just call the thunk without exception handling
    // Full implementation needs CPS integration to:
    // 1. Install the handler
    // 2. Run the thunk
    // 3. On exception, call the handler
    // 4. If handler returns, raise secondary exception (or continue for raise-continuable)

    // This is a stub that at least allows code to run
    Err(EvalError::InternalError(
        "with-exception-handler: not yet implemented - requires CPS integration".to_string(),
    ))
}

/// Register exception primitives
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Library is patina.internal.errors to match the internal library declaration.
    // error - create and raise an error
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "error",
        Arity::Min(1),
        "Raises an exception with the given message and irritants.",
        |eval, args, _tail| error(eval, args).map(EvalResult::Value),
    ));

    // error-object? - predicate
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "error-object?",
        Arity::Exact(1),
        "Returns #t if obj is an error object.",
        |eval, args, _tail| error_object_p(eval, args).map(EvalResult::Value),
    ));

    // error-object-message - accessor
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "error-object-message",
        Arity::Exact(1),
        "Returns the message of an error object.",
        |eval, args, _tail| error_object_message(eval, args).map(EvalResult::Value),
    ));

    // error-object-irritants - accessor
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "error-object-irritants",
        Arity::Exact(1),
        "Returns the list of irritants of an error object.",
        |eval, args, _tail| error_object_irritants(eval, args).map(EvalResult::Value),
    ));

    // file-error? - predicate
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "file-error?",
        Arity::Exact(1),
        "Returns #t if obj is a file error.",
        |eval, args, _tail| file_error_p(eval, args).map(EvalResult::Value),
    ));

    // read-error? - predicate
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "read-error?",
        Arity::Exact(1),
        "Returns #t if obj is a read error.",
        |eval, args, _tail| read_error_p(eval, args).map(EvalResult::Value),
    ));

    // raise - raise an exception
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "raise",
        Arity::Exact(1),
        "Raises an exception.",
        |eval, args, _tail| raise(eval, args).map(EvalResult::Value),
    ));

    // raise-continuable - raise a continuable exception
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "raise-continuable",
        Arity::Exact(1),
        "Raises a continuable exception.",
        |eval, args, _tail| raise_continuable(eval, args).map(EvalResult::Value),
    ));

    // with-exception-handler - install an exception handler
    registry.register(PrimitiveFn::new(
        "patina.internal.errors",
        "with-exception-handler",
        Arity::Exact(2),
        "Installs an exception handler and calls a thunk.",
        |eval, args, _tail| with_exception_handler(eval, args).map(EvalResult::Value),
    ));
}
