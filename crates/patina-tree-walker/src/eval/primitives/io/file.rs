//! File I/O operations
//!
//! This module implements R7RS file I/O procedures:
//! - open-input-file, open-output-file
//! - open-binary-input-file, open-binary-output-file
//! - file-exists?, delete-file
//! - call-with-input-file, call-with-output-file
//! - with-input-from-file, with-output-to-file

use super::ports::{
    get_current_input_port, get_current_output_port, set_current_input_port,
    set_current_output_port,
};
use crate::eval::EvalResult;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::Port;
use patina_runtime::value::Value;

/// (open-input-file filename) - Opens a file for reading and returns an input port
pub(super) fn open_input_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_input_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for reading: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-input-file expects a string filename".to_string(),
        )),
    }
}

/// (open-output-file filename) - Opens a file for writing and returns an output port
pub(super) fn open_output_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-output-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_output_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for writing: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-output-file expects a string filename".to_string(),
        )),
    }
}

/// (open-binary-input-file filename) - Opens a binary file for reading
pub(super) fn open_binary_input_file(
    _eval: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-binary-input-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_binary_input_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for binary reading: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-binary-input-file expects a string filename".to_string(),
        )),
    }
}

/// (open-binary-output-file filename) - Opens a binary file for writing
pub(super) fn open_binary_output_file(
    _eval: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-binary-output-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_binary_output_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for binary writing: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-binary-output-file expects a string filename".to_string(),
        )),
    }
}

/// (file-exists? filename) - Returns #t if file exists
pub(super) fn file_exists_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "file-exists? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            Ok(Value::Boolean(std::path::Path::new(&*path).exists()))
        }
        _ => Err(EvalError::TypeError(
            "file-exists? expects a string filename".to_string(),
        )),
    }
}

/// (delete-file filename) - Deletes the file
pub(super) fn delete_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "delete-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            std::fs::remove_file(&*path)
                .map_err(|e| EvalError::IOError(format!("Cannot delete '{}': {}", path, e)))?;
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "delete-file expects a string filename".to_string(),
        )),
    }
}

/// (call-with-input-file filename proc) - Opens file for reading, calls proc with port, closes
pub(super) fn call_with_input_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-input-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "call-with-input-file expects a string filename".to_string(),
            ));
        }
    };

    let proc = &args[1];

    // Open the file
    let port = Port::open_input_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Call the procedure with the port
    let result = eval.apply(proc.clone(), vec![Value::Port(port.clone())], false);

    // Close the port regardless of result
    port.close();

    // Return the result
    result
}

/// (call-with-output-file filename proc) - Opens file for writing, calls proc with port, closes
pub(super) fn call_with_output_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-output-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "call-with-output-file expects a string filename".to_string(),
            ));
        }
    };

    let proc = &args[1];

    // Open the file
    let port = Port::open_output_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Call the procedure with the port
    let result = eval.apply(proc.clone(), vec![Value::Port(port.clone())], false);

    // Close the port regardless of result (flush happens on close)
    port.close();

    // Return the result
    result
}

/// (with-input-from-file filename thunk) - Opens file, sets current-input-port, calls thunk
/// Dynamically rebinds current-input-port for the duration of the thunk call.
pub(super) fn with_input_from_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "with-input-from-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "with-input-from-file expects a string filename".to_string(),
            ));
        }
    };

    let thunk = &args[1];

    // Open the file
    let port = Port::open_input_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Save the old current-input-port
    let old_port = get_current_input_port();

    // Set the new current-input-port
    set_current_input_port(port.clone());

    // Call the thunk and capture the result
    let result = eval.apply(thunk.clone(), vec![], false);

    // Restore the old current-input-port (even on error)
    set_current_input_port(old_port);

    // Close the file port
    port.close();

    result
}

/// (with-output-to-file filename thunk) - Opens file, sets current-output-port, calls thunk
/// Dynamically rebinds current-output-port for the duration of the thunk call.
pub(super) fn with_output_to_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "with-output-to-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "with-output-to-file expects a string filename".to_string(),
            ));
        }
    };

    let thunk = &args[1];

    // Open the file
    let port = Port::open_output_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Save the old current-output-port
    let old_port = get_current_output_port();

    // Set the new current-output-port
    set_current_output_port(port.clone());

    // Call the thunk and capture the result
    let result = eval.apply(thunk.clone(), vec![], false);

    // Restore the old current-output-port (even on error)
    set_current_output_port(old_port);

    // Flush and close the file port
    port.flush().ok(); // Best effort flush
    port.close();

    result
}
