//! Binary I/O operations
//!
//! This module implements R7RS binary I/O procedures:
//! - read-u8, peek-u8, u8-ready?
//! - write-u8
//! - read-bytevector, read-bytevector!
//! - write-bytevector

use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::Port;
use patina_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Helper to get a binary input port from args
fn get_binary_input_port(args: &[Value], idx: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > idx {
        match &args[idx] {
            Value::Port(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected input port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        // No port specified - for binary operations, we need an explicit port
        Err(EvalError::TypeError(
            "binary I/O operations require an explicit port".to_string(),
        ))
    }
}

/// Helper to get a binary output port from args
fn get_binary_output_port(args: &[Value], idx: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > idx {
        match &args[idx] {
            Value::Port(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected output port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        // No port specified - for binary operations, we need an explicit port
        Err(EvalError::TypeError(
            "binary I/O operations require an explicit port".to_string(),
        ))
    }
}

/// (read-u8 [port]) - Read a single byte from a binary input port
pub(super) fn read_u8(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(&args, 0)?;
    match port.read_u8() {
        Ok(Some(byte)) => Ok(Value::Integer(byte as i64)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-u8 [port]) - Peek at next byte without consuming it
pub(super) fn peek_u8(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(&args, 0)?;
    match port.peek_u8() {
        Ok(Some(byte)) => Ok(Value::Integer(byte as i64)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (u8-ready? [port]) - Check if a byte is ready to be read
pub(super) fn u8_ready_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "u8-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(&args, 0)?;
    match port.u8_ready() {
        Ok(ready) => Ok(Value::Boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (write-u8 byte [port]) - Write a byte to a binary output port
pub(super) fn write_u8(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-u8 expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let byte = match &args[0] {
        Value::Integer(n) if *n >= 0 && *n <= 255 => *n as u8,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "write-u8: byte must be an exact integer in [0, 255]".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "write-u8: first argument must be an exact integer".to_string(),
            ));
        }
    };

    let port = get_binary_output_port(&args, 1)?;
    match port.write_u8(byte) {
        Ok(()) => Ok(Value::Unspecified),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector k [port]) - Read up to k bytes from a binary input port
pub(super) fn read_bytevector(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let k = match &args[0] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "read-bytevector: k must be a non-negative integer".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "read-bytevector: k must be an integer".to_string(),
            ));
        }
    };

    let port = get_binary_input_port(&args, 1)?;
    match port.read_bytevector(k) {
        Ok(Some(bytes)) => Ok(Value::Bytevector(Rc::new(RefCell::new(bytes)))),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector! bytevector [port [start [end]]]) - Read into existing bytevector
pub(super) fn read_bytevector_bang(
    _eval: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector! expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    let bv = match &args[0] {
        Value::Bytevector(bv) => bv.clone(),
        _ => {
            return Err(EvalError::TypeError(
                "read-bytevector!: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let port = if args.len() > 1 {
        get_binary_input_port(&args, 1)?
    } else {
        return Err(EvalError::TypeError(
            "read-bytevector!: port argument required".to_string(),
        ));
    };

    let bv_len = bv.borrow().len();

    let start = if args.len() > 2 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: start out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() > 3 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len && (*n as usize) >= start => {
                *n as usize
            }
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: end out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        bv_len
    };

    let mut bv_mut = bv.borrow_mut();
    match port.read_bytevector_into(&mut bv_mut, start, end) {
        Ok(Some(n)) => Ok(Value::Integer(n as i64)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (write-bytevector bytevector [port [start [end]]]) - Write bytevector to port
pub(super) fn write_bytevector(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "write-bytevector expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    let bv = match &args[0] {
        Value::Bytevector(bv) => bv.clone(),
        _ => {
            return Err(EvalError::TypeError(
                "write-bytevector: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let port = if args.len() > 1 {
        get_binary_output_port(&args, 1)?
    } else {
        return Err(EvalError::TypeError(
            "write-bytevector: port argument required".to_string(),
        ));
    };

    let bv_borrowed = bv.borrow();
    let bv_len = bv_borrowed.len();

    let start = if args.len() > 2 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "write-bytevector: start out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "write-bytevector: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() > 3 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len && (*n as usize) >= start => {
                *n as usize
            }
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "write-bytevector: end out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "write-bytevector: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        bv_len
    };

    match port.write_bytevector(&bv_borrowed[start..end]) {
        Ok(()) => Ok(Value::Unspecified),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}
