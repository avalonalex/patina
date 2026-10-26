//! Port operations and predicates
//!
//! This module implements:
//! - Port predicates: port?, input-port?, output-port?, etc.
//! - String ports: open-input-string, open-output-string, get-output-string
//! - Bytevector ports: open-input-bytevector, open-output-bytevector, get-output-bytevector
//! - Current ports: current-input-port, current-output-port, current-error-port
//! - Port closing: close-port, close-input-port, close-output-port
//!
//! Each takes the context (#706), and finds a port argument through it
//! (`Cx::get_port`, an `Rc` of the port, holding no borrow of the heap).
use patina_core::{CurrentPort, Cx, Value};
use patina_runtime::EvalError;
use patina_runtime::{Port, PortDirection};
use std::rc::Rc;

// =============================================================================
// Helper functions for getting ports from arguments
// =============================================================================

/// Helper to get the output port from tagged args or use current-output-port
/// The output port at `arg_index`, or the current output port.
pub(super) fn get_output_port<'gc>(
    cx: &Cx<'gc>,
    args: &[Value<'gc>],
    arg_index: usize,
) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match cx.get_port(args[arg_index]) {
            Some(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected an output port".to_string()));
                }
                Ok(p)
            }
            None => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(cx.instance().current_port(CurrentPort::Output))
    }
}

/// The input port at `arg_index`, or the current input port.
pub(super) fn get_input_port<'gc>(
    cx: &Cx<'gc>,
    args: &[Value<'gc>],
    arg_index: usize,
) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match cx.get_port(args[arg_index]) {
            Some(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected an input port".to_string()));
                }
                Ok(p)
            }
            None => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(cx.instance().current_port(CurrentPort::Input))
    }
}

// =============================================================================
// Port Predicates
// =============================================================================

/// (port? obj) - Returns #t if obj is a port
pub(super) fn port_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::boolean(cx.get_port(args[0]).is_some()))
}

/// (input-port? obj) - Returns #t if obj is an input port
pub(super) fn input_port_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = cx.get_port(args[0]).map(|p| p.is_input()).unwrap_or(false);
    Ok(Value::boolean(result))
}

/// (output-port? obj) - Returns #t if obj is an output port
pub(super) fn output_port_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = cx.get_port(args[0]).map(|p| p.is_output()).unwrap_or(false);
    Ok(Value::boolean(result))
}

/// (textual-port? obj) - Returns #t if obj is a port.
///
/// Every port is textual, because the textual operations work on every port:
/// a binary port reads and writes text as UTF-8 (`decode_utf8_at` and
/// `Port::write_string` in `patina-core`'s `port.rs`). R7RS §6.13.1 leaves
/// whether the textual and binary port types are disjoint to the
/// implementation, and an implementation that reads and writes text through
/// a port cannot coherently call it non-textual. chibi 0.12 and Gauche 0.9.15
/// both answer `#t` for bytevector ports and for binary file ports (#404).
///
/// `binary-port?` also accepts file and standard ports regardless of opening
/// mode (#412), matching both references. String ports remain textual-only,
/// matching chibi where the references differ.
pub(super) fn textual_port_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "textual-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    port_p(cx, args)
}

/// (binary-port? obj) - Returns #t if obj is a binary port
pub(super) fn binary_port_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "binary-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = cx.get_port(args[0]).map(|p| p.is_binary()).unwrap_or(false);
    Ok(Value::boolean(result))
}

/// (input-port-open? port) - Returns #t if port is open for input
pub(super) fn input_port_open_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_port(args[0]) {
        Some(p) => {
            // R7RS 6.13.1: "#t if port is still open and capable of
            // performing input" — an output-only port is simply not, rather
            // than a type error. Larceny's file suite maps every port
            // predicate over a fresh binary port and expects (#f ...) here.
            if p.direction != PortDirection::Input {
                return Ok(Value::FALSE);
            }
            Ok(Value::boolean(p.is_open()))
        }
        None => Err(EvalError::TypeError(
            "input-port-open? expects a port".to_string(),
        )),
    }
}

/// (output-port-open? port) - Returns #t if port is open for output
pub(super) fn output_port_open_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_port(args[0]) {
        Some(p) => {
            // Mirror of input-port-open?: an input-only port is not capable
            // of output, so #f, not an error.
            if p.direction != PortDirection::Output {
                return Ok(Value::FALSE);
            }
            Ok(Value::boolean(p.is_open()))
        }
        None => Err(EvalError::TypeError(
            "output-port-open? expects a port".to_string(),
        )),
    }
}

// =============================================================================
// String Ports
// =============================================================================

/// (open-input-string string) - Create an input port from a string
pub(super) fn open_input_string<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_string_contents(args[0]) {
        Some(content) => Ok(cx.alloc_port(Port::new_input_string(content))),
        None => Err(EvalError::TypeError(
            "open-input-string expects a string".to_string(),
        )),
    }
}

/// (open-output-string) - Create an output port that accumulates to a string
pub(super) fn open_output_string<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-string expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(cx.alloc_port(Port::new_output_string()))
}

/// (get-output-string port) - Get the accumulated string from an output string port
pub(super) fn get_output_string<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_port(args[0]) {
        Some(p) => match p.get_output_string() {
            Ok(s) => Ok(cx.alloc_string(s)),
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        None => Err(EvalError::TypeError(
            "get-output-string expects an output string port".to_string(),
        )),
    }
}

// =============================================================================
// Bytevector Ports (Binary I/O)
// =============================================================================

/// (open-input-bytevector bytevector) - Create an input port from a bytevector
pub(super) fn open_input_bytevector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_bytevector_bytes(args[0]) {
        Some(content) => Ok(cx.alloc_port(Port::new_input_bytevector(content))),
        None => Err(EvalError::TypeError(
            "open-input-bytevector expects a bytevector".to_string(),
        )),
    }
}

/// (open-output-bytevector) - Create an output port that accumulates to a bytevector
pub(super) fn open_output_bytevector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-bytevector expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(cx.alloc_port(Port::new_output_bytevector()))
}

/// (get-output-bytevector port) - Get the accumulated bytevector from an output bytevector port
pub(super) fn get_output_bytevector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_port(args[0]) {
        Some(p) => match p.get_output_bytevector() {
            Ok(bv) => Ok(cx.alloc_bytevector(bv)),
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        None => Err(EvalError::TypeError(
            "get-output-bytevector expects an output bytevector port".to_string(),
        )),
    }
}

// =============================================================================
// Current Ports
// =============================================================================

// These three are **parameter objects**, as R7RS §6.13.1 requires, not plain
// procedures: each reads with no argument and installs with one.
//
// `parameterize` (`lib/scheme/base/parameters.scm`) drives a parameter through
// the object itself — `(p)` to read the current value, `(p v)` to install one,
// `(p old)` from `dynamic-wind`'s after-thunk to restore it. So accepting the
// setter arity is not a shortcut around the dynamic binding: the interpreter's
// slot (`Instance::current_port`, #618) *is* the binding, and writing it is
// what every other primitive that falls back to the current port observes. A
// Scheme-level rebinding that left the slot alone would be the broken
// version, because `display` with no port argument reads the slot and would
// ignore it.
//
// Written out three times rather than shared: each has to exist anyway as its
// own `fn` (the registry takes bare fn pointers, not closures), so factoring
// the body out moved it behind three function-pointer parameters without
// removing a function. The setter arm defers to this file's existing port
// validator for the right direction, whose "argument absent, use the current
// port" branch cannot be reached from here.

/// (current-input-port) / (current-input-port port)
pub(super) fn current_input_port<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    match args {
        [] => {
            let port = cx.instance().current_port(CurrentPort::Input);
            Ok(cx.alloc_port(port))
        }
        [_] => {
            let port = get_input_port(cx, args, 0)?;
            cx.instance().set_current_port(CurrentPort::Input, port);
            Ok(Value::UNSPECIFIED)
        }
        _ => Err(EvalError::WrongArity {
            expected: "current-input-port expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        }),
    }
}

/// (current-output-port) / (current-output-port port)
pub(super) fn current_output_port<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    match args {
        [] => {
            let port = cx.instance().current_port(CurrentPort::Output);
            Ok(cx.alloc_port(port))
        }
        [_] => {
            let port = get_output_port(cx, args, 0)?;
            cx.instance().set_current_port(CurrentPort::Output, port);
            Ok(Value::UNSPECIFIED)
        }
        _ => Err(EvalError::WrongArity {
            expected: "current-output-port expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        }),
    }
}

/// (current-error-port) / (current-error-port port)
pub(super) fn current_error_port<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    match args {
        [] => {
            let port = cx.instance().current_port(CurrentPort::Error);
            Ok(cx.alloc_port(port))
        }
        [_] => {
            let port = get_output_port(cx, args, 0)?;
            cx.instance().set_current_port(CurrentPort::Error, port);
            Ok(Value::UNSPECIFIED)
        }
        _ => Err(EvalError::WrongArity {
            expected: "current-error-port expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        }),
    }
}

// =============================================================================
// Port Operations
// =============================================================================

/// (close-port port) - Close a port
pub(super) fn close_port<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_port(args[0]) {
        Some(p) => {
            p.close();
            Ok(Value::UNSPECIFIED)
        }
        None => Err(EvalError::TypeError(
            "close-port expects a port".to_string(),
        )),
    }
}

/// (close-input-port port) - Close an input port
pub(super) fn close_input_port<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-input-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_port(args[0]) {
        Some(p) => {
            if !p.is_input() {
                return Err(EvalError::TypeError(
                    "close-input-port expects an input port".to_string(),
                ));
            }
            p.close();
            Ok(Value::UNSPECIFIED)
        }
        None => Err(EvalError::TypeError(
            "close-input-port expects a port".to_string(),
        )),
    }
}

/// (close-output-port port) - Close an output port
pub(super) fn close_output_port<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-output-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_port(args[0]) {
        Some(p) => {
            if !p.is_output() {
                return Err(EvalError::TypeError(
                    "close-output-port expects an output port".to_string(),
                ));
            }
            p.close();
            Ok(Value::UNSPECIFIED)
        }
        None => Err(EvalError::TypeError(
            "close-output-port expects a port".to_string(),
        )),
    }
}

// =============================================================================
// EOF Handling
// =============================================================================

/// (eof-object? obj) - Returns #t if obj is the EOF object
pub(super) fn eof_object_p<'gc>(
    _cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "eof-object? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    // The eof object is an immediate
    Ok(Value::boolean(args[0] == Value::EOF))
}

/// (eof-object) - Returns an EOF object
pub(super) fn eof_object<'gc>(
    _cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "eof-object expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::EOF)
}

/// (flush-output-port [port]) - Flushes the output port
pub(super) fn flush_output_port<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "flush-output-port expects 0 or 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let port = if args.is_empty() {
        cx.instance().current_port(CurrentPort::Output)
    } else {
        match cx.get_port(args[0]) {
            Some(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected an output port".to_string()));
                }
                p
            }
            None => return Err(EvalError::TypeError("expected a port".to_string())),
        }
    };
    port.flush()
        .map_err(|e| EvalError::IOError(format!("flush failed: {}", e)))?;
    Ok(Value::UNSPECIFIED)
}

// =============================================================================
// call-with-port
// =============================================================================

/// (call-with-port port proc) - Calls proc with port, then closes the port
pub(super) fn call_with_port<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn crate::apply_context::ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-port expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract port for close() call
    let port = cx.get_port(args[0]).ok_or_else(|| {
        EvalError::TypeError("call-with-port expects a port as first argument".to_string())
    })?;

    // Reached only by importing `(patina internal io)`: the `(scheme base)`
    // version runs its procedure as a frame (#471).
    let result = ctx.apply_in(cx, args[1], &[args[0]]);

    // R7RS 6.13.1: the port is closed if `proc` returns, and "must not be
    // closed automatically" if it does not. An `Err` here is a raise or a
    // continuation escape leaving `proc`, and the code that receives it — a
    // `guard` clause runs after the escape, in the guard's own extent — may
    // still hold the port. Closing on the way out handed such a clause a
    // closed port.
    if result.is_ok() {
        port.close();
    }

    // Return the result
    result
}
