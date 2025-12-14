//! Text output operations
//!
//! This module implements R7RS text output procedures:
//! - display, write, write-shared, write-simple
//! - newline, write-char, write-string

use super::datum_writer::{format_display, format_write, format_write_shared, format_write_simple};
use super::ports::get_output_port;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;

/// (display obj [port]) - Write obj in human-readable format
/// Handles circular structures using datum labels (#n= and #n#)
pub(super) fn display(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "display expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_display(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write obj [port]) - Write obj in machine-readable format
/// Handles circular structures using datum labels (#n= and #n#)
pub(super) fn write(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_write(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-shared obj [port]) - Write obj with datum labels for all shared structures
/// Labels both circular and shared (multiply-referenced) structures
pub(super) fn write_shared(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-shared expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_write_shared(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-simple obj [port]) - Write obj without datum labels
/// Does not handle circular structures (may loop infinitely)
pub(super) fn write_simple(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-simple expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_write_simple(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (newline [port]) - Write a newline character
pub(super) fn newline(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "newline expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 0)?;
    port.write_string("\n")
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-char char [port]) - Write a single character
pub(super) fn write_char(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-char expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let ch = match &args[0] {
        Value::Character(c) => *c,
        _ => {
            return Err(EvalError::TypeError(
                "write-char expects a character".to_string(),
            ));
        }
    };

    let port = get_output_port(&args, 1)?;
    port.write_char(ch)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-string string [port [start [end]]]) - Write a string
pub(super) fn write_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "write-string expects 1 to 4 arguments".to_string(),
            actual: args.len(),
        });
    }

    let chars = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "write-string expects a string".to_string(),
            ));
        }
    };

    let port = get_output_port(&args, 1)?;

    // Handle optional start/end indices
    let start = if args.len() > 2 {
        match &args[2] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "write-string: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() > 3 {
        match &args[3] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "write-string: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        chars.len()
    };

    // Get substring by character indices (not byte indices)
    if start > chars.len() || end > chars.len() || start > end {
        return Err(EvalError::IndexOutOfBounds(format!(
            "write-string: index {} out of range for string of length {}",
            if start > chars.len() { start } else { end },
            chars.len()
        )));
    }

    let substr: String = chars[start..end].iter().collect();
    port.write_string(&substr)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}
