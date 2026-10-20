//! Binary I/O operations
//!
//! This module implements R7RS binary I/O procedures:
//! - read-u8, peek-u8, u8-ready?
//! - write-u8
//! - read-bytevector, read-bytevector!
//! - write-bytevector
//!
//! Each takes the context (#706).

use super::ports::{get_input_port, get_output_port};
use patina_core::{Cx, Value};
use patina_runtime::EvalError;
use patina_runtime::Port;
use std::rc::Rc;

/// Get an explicit binary input port, or the current one
fn get_binary_input_port<'gc>(
    cx: &Cx<'gc>,
    args: &[Value<'gc>],
    idx: usize,
) -> Result<Rc<Port>, EvalError> {
    let port = get_input_port(cx, args, idx)?;
    if !port.is_binary() {
        return Err(EvalError::TypeError("expected binary port".to_string()));
    }
    Ok(port)
}

/// Get an explicit binary output port, or the dynamically current one (#533).
fn get_binary_output_port<'gc>(
    cx: &Cx<'gc>,
    args: &[Value<'gc>],
    idx: usize,
) -> Result<Rc<Port>, EvalError> {
    let port = get_output_port(cx, args, idx)?;
    if !port.is_binary() {
        return Err(EvalError::TypeError("expected binary port".to_string()));
    }
    Ok(port)
}

/// (read-u8 [port]) - Read a single byte from a binary input port
pub(super) fn read_u8<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(cx, args, 0)?;
    match port.read_u8() {
        Ok(Some(byte)) => Ok(Value::fixnum(byte as i64)),
        Ok(None) => Ok(Value::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-u8 [port]) - Peek at next byte without consuming it
pub(super) fn peek_u8<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(cx, args, 0)?;
    match port.peek_u8() {
        Ok(Some(byte)) => Ok(Value::fixnum(byte as i64)),
        Ok(None) => Ok(Value::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (u8-ready? [port]) - Check if a byte is ready to be read
pub(super) fn u8_ready_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "u8-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(cx, args, 0)?;
    match port.u8_ready() {
        Ok(ready) => Ok(Value::boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (write-u8 byte [port]) - Write a byte to a binary output port
pub(super) fn write_u8<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-u8 expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract the byte
    let byte = if args[0].is_fixnum() {
        let n = args[0].as_fixnum_unchecked();
        if (0..=255).contains(&n) {
            n as u8
        } else {
            return Err(EvalError::TypeError(
                "write-u8: byte must be an exact integer in [0, 255]".to_string(),
            ));
        }
    } else {
        return Err(EvalError::TypeError(
            "write-u8: first argument must be an exact integer".to_string(),
        ));
    };

    let port = get_binary_output_port(cx, args, 1)?;
    match port.write_u8(byte) {
        Ok(()) => Ok(Value::UNSPECIFIED),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector k [port]) - Read up to k bytes from a binary input port
pub(super) fn read_bytevector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract k
    let k = if args[0].is_fixnum() {
        let n = args[0].as_fixnum_unchecked();
        if n < 0 {
            return Err(EvalError::TypeError(
                "read-bytevector: k must be a non-negative integer".to_string(),
            ));
        }
        n as usize
    } else {
        return Err(EvalError::TypeError(
            "read-bytevector: k must be an integer".to_string(),
        ));
    };

    let port = get_binary_input_port(cx, args, 1)?;
    match port.read_bytevector(k) {
        Ok(Some(bytes)) => Ok(cx.alloc_bytevector(bytes)),
        Ok(None) => Ok(Value::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector! bytevector [port [start [end]]]) - Read into existing bytevector
pub(super) fn read_bytevector_bang<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector! expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Validate bytevector and get its length
    let bv_len = {
        cx.bytevector_len(args[0]).ok_or_else(|| {
            EvalError::TypeError(
                "read-bytevector!: first argument must be a bytevector".to_string(),
            )
        })?
    };

    let port = get_binary_input_port(cx, args, 1)?;

    // Parse start/end
    let start = if args.len() > 2 {
        get_fixnum_index(args[2], "read-bytevector!", bv_len)?
    } else {
        0
    };

    let end = if args.len() > 3 {
        let n = get_fixnum_index(args[3], "read-bytevector!", bv_len)?;
        if n < start {
            return Err(EvalError::TypeError(
                "read-bytevector!: end out of range".to_string(),
            ));
        }
        n
    } else {
        bv_len
    };

    // Read into a temporary buffer, then copy into the bytevector
    let capacity = end - start;
    let mut temp_buf = vec![0u8; capacity];
    match port.read_bytevector_into(&mut temp_buf, 0, capacity) {
        Ok(Some(n)) => {
            cx.bytevector_copy_into(args[0], start, &temp_buf[..n]);
            Ok(Value::fixnum(n as i64))
        }
        Ok(None) => Ok(Value::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// Helper to extract a non-negative integer index from TaggedValue, bounded by max
fn get_fixnum_index(tv: Value<'_>, fn_name: &str, max: usize) -> Result<usize, EvalError> {
    if tv.is_fixnum() {
        let n = tv.as_fixnum_unchecked();
        if n >= 0 && (n as usize) <= max {
            return Ok(n as usize);
        }
        return Err(EvalError::TypeError(format!(
            "{}: index out of range: {}",
            fn_name, n
        )));
    }
    Err(EvalError::TypeError(format!(
        "{}: index must be an integer",
        fn_name
    )))
}

/// (write-bytevector bytevector [port [start [end]]]) - Write bytevector to port
pub(super) fn write_bytevector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "write-bytevector expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Get bytevector bytes
    let bytes = {
        cx.get_bytevector_bytes(args[0]).ok_or_else(|| {
            EvalError::TypeError(
                "write-bytevector: first argument must be a bytevector".to_string(),
            )
        })?
    };

    let port = get_binary_output_port(cx, args, 1)?;

    let bv_len = bytes.len();

    let start = if args.len() > 2 {
        get_fixnum_index(args[2], "write-bytevector", bv_len)?
    } else {
        0
    };

    let end = if args.len() > 3 {
        let n = get_fixnum_index(args[3], "write-bytevector", bv_len)?;
        if n < start {
            return Err(EvalError::TypeError(
                "write-bytevector: end out of range".to_string(),
            ));
        }
        n
    } else {
        bv_len
    };

    match port.write_bytevector(&bytes[start..end]) {
        Ok(()) => Ok(Value::UNSPECIFIED),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}
