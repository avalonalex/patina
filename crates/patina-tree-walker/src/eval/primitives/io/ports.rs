//! Port operations and predicates
//!
//! This module implements:
//! - Port predicates: port?, input-port?, output-port?, etc.
//! - String ports: open-input-string, open-output-string, get-output-string
//! - Bytevector ports: open-input-bytevector, open-output-bytevector, get-output-bytevector
//! - Current ports: current-input-port, current-output-port, current-error-port
//! - Port closing: close-port, close-input-port, close-output-port

use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;
use patina_runtime::{Port, PortDirection};
use std::cell::RefCell;
use std::rc::Rc;

// =============================================================================
// Thread-local storage for current ports
// =============================================================================

thread_local! {
    static CURRENT_INPUT_PORT: RefCell<Rc<Port>> = RefCell::new(Port::stdin());
    static CURRENT_OUTPUT_PORT: RefCell<Rc<Port>> = RefCell::new(Port::stdout());
    static CURRENT_ERROR_PORT: RefCell<Rc<Port>> = RefCell::new(Port::stderr());
}

/// Get the current input port
pub fn get_current_input_port() -> Rc<Port> {
    CURRENT_INPUT_PORT.with(|p| p.borrow().clone())
}

/// Get the current output port
pub fn get_current_output_port() -> Rc<Port> {
    CURRENT_OUTPUT_PORT.with(|p| p.borrow().clone())
}

/// Get the current error port
pub fn get_current_error_port() -> Rc<Port> {
    CURRENT_ERROR_PORT.with(|p| p.borrow().clone())
}

/// Set the current input port (for dynamic rebinding)
pub fn set_current_input_port(port: Rc<Port>) {
    CURRENT_INPUT_PORT.with(|p| *p.borrow_mut() = port);
}

/// Set the current output port (for dynamic rebinding)
pub fn set_current_output_port(port: Rc<Port>) {
    CURRENT_OUTPUT_PORT.with(|p| *p.borrow_mut() = port);
}

// =============================================================================
// Helper functions for getting ports from arguments
// =============================================================================

/// Helper to get the output port from args or use current-output-port
pub(super) fn get_output_port(args: &[Value], arg_index: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match &args[arg_index] {
            Value::Port(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected an output port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(get_current_output_port())
    }
}

/// Helper to get the input port from args or use current-input-port
pub(super) fn get_input_port(args: &[Value], arg_index: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match &args[arg_index] {
            Value::Port(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected an input port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(get_current_input_port())
    }
}

// =============================================================================
// Port Predicates
// =============================================================================

/// (port? obj) - Returns #t if obj is a port
pub(super) fn port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Boolean(matches!(args[0], Value::Port(_))))
}

/// (input-port? obj) - Returns #t if obj is an input port
pub(super) fn input_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_input(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (output-port? obj) - Returns #t if obj is an output port
pub(super) fn output_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_output(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (textual-port? obj) - Returns #t if obj is a textual port
pub(super) fn textual_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "textual-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_textual(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (binary-port? obj) - Returns #t if obj is a binary port
pub(super) fn binary_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "binary-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_binary(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (input-port-open? port) - Returns #t if port is open for input
pub(super) fn input_port_open_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if p.direction != PortDirection::Input {
                return Err(EvalError::TypeError(
                    "input-port-open? expects an input port".to_string(),
                ));
            }
            Ok(Value::Boolean(p.is_open()))
        }
        _ => Err(EvalError::TypeError(
            "input-port-open? expects a port".to_string(),
        )),
    }
}

/// (output-port-open? port) - Returns #t if port is open for output
pub(super) fn output_port_open_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if p.direction != PortDirection::Output {
                return Err(EvalError::TypeError(
                    "output-port-open? expects an output port".to_string(),
                ));
            }
            Ok(Value::Boolean(p.is_open()))
        }
        _ => Err(EvalError::TypeError(
            "output-port-open? expects a port".to_string(),
        )),
    }
}

// =============================================================================
// String Ports
// =============================================================================

/// (open-input-string string) - Create an input port from a string
pub(super) fn open_input_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let content = s.borrow().clone();
            Ok(Value::Port(Port::new_input_string(content)))
        }
        _ => Err(EvalError::TypeError(
            "open-input-string expects a string".to_string(),
        )),
    }
}

/// (open-output-string) - Create an output port that accumulates to a string
pub(super) fn open_output_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-string expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(Port::new_output_string()))
}

/// (get-output-string port) - Get the accumulated string from an output string port
pub(super) fn get_output_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => match p.get_output_string() {
            Ok(s) => Ok(Value::String(Rc::new(RefCell::new(s)))),
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        _ => Err(EvalError::TypeError(
            "get-output-string expects an output string port".to_string(),
        )),
    }
}

// =============================================================================
// Bytevector Ports (Binary I/O)
// =============================================================================

/// (open-input-bytevector bytevector) - Create an input port from a bytevector
pub(super) fn open_input_bytevector(
    _eval: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Bytevector(bv) => {
            let content = bv.borrow().clone();
            Ok(Value::Port(Port::new_input_bytevector(content)))
        }
        _ => Err(EvalError::TypeError(
            "open-input-bytevector expects a bytevector".to_string(),
        )),
    }
}

/// (open-output-bytevector) - Create an output port that accumulates to a bytevector
pub(super) fn open_output_bytevector(
    _eval: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-bytevector expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(Port::new_output_bytevector()))
}

/// (get-output-bytevector port) - Get the accumulated bytevector from an output bytevector port
pub(super) fn get_output_bytevector(
    _eval: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => match p.get_output_bytevector() {
            Ok(bv) => Ok(Value::Bytevector(Rc::new(RefCell::new(bv)))),
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        _ => Err(EvalError::TypeError(
            "get-output-bytevector expects an output bytevector port".to_string(),
        )),
    }
}

// =============================================================================
// Current Ports
// =============================================================================

/// (current-input-port) - Returns the current input port
pub(super) fn current_input_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "current-input-port expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(get_current_input_port()))
}

/// (current-output-port) - Returns the current output port
pub(super) fn current_output_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "current-output-port expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(get_current_output_port()))
}

/// (current-error-port) - Returns the current error port
pub(super) fn current_error_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "current-error-port expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(get_current_error_port()))
}

// =============================================================================
// Port Operations
// =============================================================================

/// (close-port port) - Close a port
pub(super) fn close_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            p.close();
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "close-port expects a port".to_string(),
        )),
    }
}

/// (close-input-port port) - Close an input port
pub(super) fn close_input_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-input-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if !p.is_input() {
                return Err(EvalError::TypeError(
                    "close-input-port expects an input port".to_string(),
                ));
            }
            p.close();
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "close-input-port expects a port".to_string(),
        )),
    }
}

/// (close-output-port port) - Close an output port
pub(super) fn close_output_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-output-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if !p.is_output() {
                return Err(EvalError::TypeError(
                    "close-output-port expects an output port".to_string(),
                ));
            }
            p.close();
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "close-output-port expects a port".to_string(),
        )),
    }
}

// =============================================================================
// EOF Handling
// =============================================================================

/// (eof-object? obj) - Returns #t if obj is the EOF object
pub(super) fn eof_object_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "eof-object? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Boolean(matches!(args[0], Value::Eof)))
}

/// (eof-object) - Returns an EOF object
pub(super) fn eof_object(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "eof-object expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Eof)
}

/// (flush-output-port [port]) - Flushes the output port
pub(super) fn flush_output_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "flush-output-port expects 0 or 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 0)?;
    port.flush()
        .map_err(|e| EvalError::IOError(format!("flush failed: {}", e)))?;
    Ok(Value::Unspecified)
}

// =============================================================================
// call-with-port
// =============================================================================

use crate::eval::EvalResult;

/// (call-with-port port proc) - Calls proc with port, then closes the port
pub(super) fn call_with_port(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-port expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = match &args[0] {
        Value::Port(p) => p.clone(),
        _ => {
            return Err(EvalError::TypeError(
                "call-with-port expects a port as first argument".to_string(),
            ));
        }
    };

    let proc = &args[1];

    // Call the procedure with the port
    let result = eval.apply(proc.clone(), vec![Value::Port(port.clone())], false);

    // Close the port regardless of result
    port.close();

    // Return the result
    result
}
