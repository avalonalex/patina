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
//!
//! Each takes the context (#706). A value in a message is formatted by the
//! datum writer's context entry point (`format_display`), and the
//! predicates read an error object's kind alone (`Cx::exception_kind`).

use crate::primitives::io::datum_writer::format_display;
use patina_core::ExceptionKind;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

/// (error message obj ...) - Create and raise an error object
///
/// Creates an error object with the given message and irritants, then raises it.
/// The irritants can be any values.
pub(super) fn error<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    // R7RS 6.11 says the message *should* be a string — advice, not a
    // requirement, and code in the wild takes the latitude: the R6RS habit of
    // `(error 'who "what")` appears throughout SRFI reference implementations,
    // including the bundled SRFI 41. Rejecting it replaced every one of that
    // library's 53 diagnostics with a complaint about the argument; chibi and
    // Gauche both report the symbol as the message, and so do we now.
    let message = match cx.get_string_contents(args[0]) {
        Some(s) => s,
        None => format_display(cx, args[0]),
    };

    // Remaining arguments are irritants - serialize to display string
    let irritants_display = if args.len() > 1 {
        args[1..]
            .iter()
            .map(|tv| format_display(cx, *tv))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };

    // For now, just return an error that includes the exception info
    // When we have full exception handling, this will call raise
    Err(EvalError::SchemeException {
        kind: ExceptionKind::Error,
        message,
        irritants_display,
    })
}

/// (error-object? obj) - Returns #t if obj is an error object. An interrupt
/// is not one (#701): a guard that catches errors lets Ctrl-C through.
pub(super) fn error_object_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let is_error_object = cx
        .exception_kind(args[0])
        .is_some_and(|kind| kind != ExceptionKind::Interrupt);
    Ok(Value::boolean(is_error_object))
}

/// (interrupt? obj) - Returns #t if obj is the condition a posted interrupt,
/// Ctrl-C's, raises (#701). `(patina interrupt)` exports it.
pub(super) fn interrupt_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let is_interrupt = cx
        .exception_kind(args[0])
        .is_some_and(|kind| kind == ExceptionKind::Interrupt);
    Ok(Value::boolean(is_interrupt))
}

/// (error-object-message error-object) - Get the error message string
pub(super) fn error_object_message<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let message = match cx.get_exception(args[0]) {
        Some((_kind, message, _irritants)) => message,
        None => {
            return Err(EvalError::TypeError(
                "error-object-message: argument must be an error object".to_string(),
            ));
        }
    };
    Ok(cx.alloc_string(message))
}

/// (error-object-irritants error-object) - Get the list of irritants
pub(super) fn error_object_irritants<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let irritants = match cx.get_exception(args[0]) {
        Some((_kind, _message, irritants)) => irritants,
        None => {
            return Err(EvalError::TypeError(
                "error-object-irritants: argument must be an error object".to_string(),
            ));
        }
    };
    // The irritants are values: build a list directly
    Ok(cx.list_from_iter(irritants))
}

/// (file-error? obj) - Returns #t if obj is a file error
pub(super) fn file_error_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let is_file_error = cx
        .exception_kind(args[0])
        .is_some_and(|kind| kind == ExceptionKind::FileError);
    Ok(Value::boolean(is_file_error))
}

/// (read-error? obj) - Returns #t if obj is a read error
pub(super) fn read_error_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let is_read_error = cx
        .exception_kind(args[0])
        .is_some_and(|kind| kind == ExceptionKind::ReadError);
    Ok(Value::boolean(is_read_error))
}

/// (raise obj) - Raise an exception
///
/// Raises the given object as an exception. If it's already an exception object,
/// raises it directly. Otherwise, wraps it in an exception.
pub(super) fn raise<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "raise expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    // If it's already an exception object, extract its info using heap method
    {
        if let Some((kind, message, irritants)) = cx.get_exception(args[0]) {
            let irritants_display = irritants
                .iter()
                .map(|tv| format_display(cx, *tv))
                .collect::<Vec<_>>()
                .join(" ");
            return Err(EvalError::SchemeException {
                kind,
                message,
                irritants_display,
            });
        }
    }

    // Otherwise, raise it as a generic exception with the object as the message
    {
        let message = format_display(cx, args[0]);
        Err(EvalError::SchemeException {
            kind: ExceptionKind::Error,
            message,
            irritants_display: String::new(),
        })
    }
}

/// (raise-continuable obj) - Raise a continuable exception
///
/// Similar to raise, but allows the exception handler to return a value
/// that becomes the result of raise-continuable.
/// NOTE: This is currently a stub - proper implementation requires CPS integration.
pub(super) fn raise_continuable<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "raise-continuable expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    // For now, just raise like a non-continuable exception
    // Full implementation needs CPS exception handler support
    {
        let message = format!("continuable: {}", format_display(cx, args[0]));
        Err(EvalError::SchemeException {
            kind: ExceptionKind::Error,
            message,
            irritants_display: String::new(),
        })
    }
}

/// (with-exception-handler handler thunk) - Install an exception handler
///
/// Calls thunk with handler installed as the current exception handler.
/// If an exception is raised, handler is called with the exception object.
/// NOTE: This is currently a stub - proper implementation requires CPS integration.
pub(super) fn with_exception_handler<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    // Verify both are procedures
    if !cx.is_callable(args[0]) {
        return Err(EvalError::TypeError(
            "with-exception-handler: first argument must be a procedure".to_string(),
        ));
    }
    if !cx.is_callable(args[1]) {
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
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Library is patina.internal.errors to match the internal library declaration.
    // error - create and raise an error
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "error",
        Arity::Min(1),
        "Raises an exception with the given message and irritants.",
        error,
    ));

    // error-object? - predicate
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "error-object?",
        Arity::Exact(1),
        "Returns #t if obj is an error object.",
        error_object_p,
    ));

    // error-object-message - accessor
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "error-object-message",
        Arity::Exact(1),
        "Returns the message of an error object.",
        error_object_message,
    ));

    // error-object-irritants - accessor
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "error-object-irritants",
        Arity::Exact(1),
        "Returns the list of irritants of an error object.",
        error_object_irritants,
    ));

    // file-error? - predicate
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "file-error?",
        Arity::Exact(1),
        "Returns #t if obj is a file error.",
        file_error_p,
    ));

    // read-error? - predicate
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "read-error?",
        Arity::Exact(1),
        "Returns #t if obj is a read error.",
        read_error_p,
    ));

    // interrupt? - predicate, for (patina interrupt)
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "interrupt?",
        Arity::Exact(1),
        "Returns #t if obj is the condition an interrupt raises.",
        interrupt_p,
    ));

    // raise - raise an exception
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "raise",
        Arity::Exact(1),
        "Raises an exception.",
        raise,
    ));

    // raise-continuable - raise a continuable exception
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "raise-continuable",
        Arity::Exact(1),
        "Raises a continuable exception.",
        raise_continuable,
    ));

    // with-exception-handler - install an exception handler
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.errors",
        "with-exception-handler",
        Arity::Exact(2),
        "Installs an exception handler and calls a thunk.",
        with_exception_handler,
    ));
}
