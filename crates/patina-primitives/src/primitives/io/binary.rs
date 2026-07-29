//! Binary I/O operations
//!
//! This module implements R7RS binary I/O procedures:
//! - read-u8, peek-u8, u8-ready?
//! - write-u8
//! - read-bytevector, read-bytevector!
//! - write-bytevector

use super::ports::get_port_tv;
use patina_core::TaggedValue;
use patina_runtime::Port;
use patina_runtime::{EvalError, SharedHeap};
use std::rc::Rc;

/// Helper to get a binary input port from tagged args
fn get_binary_input_port_tagged(
    args: &[TaggedValue],
    idx: usize,
    heap: &std::cell::Ref<'_, patina_core::Heap>,
) -> Result<Rc<Port>, EvalError> {
    if args.len() > idx {
        match get_port_tv(args[idx], heap) {
            Some(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected input port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                Ok(p.clone())
            }
            None => Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        // No port specified - for binary operations, we need an explicit port
        Err(EvalError::TypeError(
            "binary I/O operations require an explicit port".to_string(),
        ))
    }
}

/// Helper to get a binary output port from tagged args
fn get_binary_output_port_tagged(
    args: &[TaggedValue],
    idx: usize,
    heap: &std::cell::Ref<'_, patina_core::Heap>,
) -> Result<Rc<Port>, EvalError> {
    if args.len() > idx {
        match get_port_tv(args[idx], heap) {
            Some(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected output port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                Ok(p.clone())
            }
            None => Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        // No port specified - for binary operations, we need an explicit port
        Err(EvalError::TypeError(
            "binary I/O operations require an explicit port".to_string(),
        ))
    }
}

/// (read-u8 [port]) - Read a single byte from a binary input port
pub(super) fn read_u8(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_binary_input_port_tagged(args, 0, &heap_ref)?
    };
    match port.read_u8() {
        Ok(Some(byte)) => Ok(TaggedValue::fixnum(byte as i64)),
        Ok(None) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-u8 [port]) - Peek at next byte without consuming it
pub(super) fn peek_u8(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_binary_input_port_tagged(args, 0, &heap_ref)?
    };
    match port.peek_u8() {
        Ok(Some(byte)) => Ok(TaggedValue::fixnum(byte as i64)),
        Ok(None) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (u8-ready? [port]) - Check if a byte is ready to be read
pub(super) fn u8_ready_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "u8-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_binary_input_port_tagged(args, 0, &heap_ref)?
    };
    match port.u8_ready() {
        Ok(ready) => Ok(TaggedValue::boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (write-u8 byte [port]) - Write a byte to a binary output port
pub(super) fn write_u8(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-u8 expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract byte from TaggedValue directly
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

    let port = {
        let heap_ref = heap.borrow();
        get_binary_output_port_tagged(args, 1, &heap_ref)?
    };
    match port.write_u8(byte) {
        Ok(()) => Ok(TaggedValue::UNSPECIFIED),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector k [port]) - Read up to k bytes from a binary input port
pub(super) fn read_bytevector(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract k from TaggedValue directly
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

    let port = {
        let heap_ref = heap.borrow();
        get_binary_input_port_tagged(args, 1, &heap_ref)?
    };
    match port.read_bytevector(k) {
        Ok(Some(bytes)) => Ok(heap.borrow_mut().alloc_bytevector(bytes)),
        Ok(None) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector! bytevector [port [start [end]]]) - Read into existing bytevector
pub(super) fn read_bytevector_bang(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector! expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Validate bytevector and get its length
    let bv_len = {
        let heap_ref = heap.borrow();
        heap_ref.bytevector_len(args[0]).ok_or_else(|| {
            EvalError::TypeError(
                "read-bytevector!: first argument must be a bytevector".to_string(),
            )
        })?
    };

    // Get port
    let port = if args.len() > 1 {
        let heap_ref = heap.borrow();
        match get_port_tv(args[1], &heap_ref) {
            Some(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected input port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                p.clone()
            }
            None => return Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        return Err(EvalError::TypeError(
            "read-bytevector!: port argument required".to_string(),
        ));
    };

    // Parse start/end from TaggedValue directly
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
            let mut heap_mut = heap.borrow_mut();
            heap_mut.bytevector_copy_into(args[0], start, &temp_buf[..n]);
            Ok(TaggedValue::fixnum(n as i64))
        }
        Ok(None) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// Helper to extract a non-negative integer index from TaggedValue, bounded by max
fn get_fixnum_index(tv: TaggedValue, fn_name: &str, max: usize) -> Result<usize, EvalError> {
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
pub(super) fn write_bytevector(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "write-bytevector expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Get bytevector bytes
    let bytes = {
        let heap_ref = heap.borrow();
        heap_ref.get_bytevector_bytes(args[0]).ok_or_else(|| {
            EvalError::TypeError(
                "write-bytevector: first argument must be a bytevector".to_string(),
            )
        })?
    };

    // Get port
    let port = if args.len() > 1 {
        let heap_ref = heap.borrow();
        match get_port_tv(args[1], &heap_ref) {
            Some(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected output port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                p.clone()
            }
            None => return Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        return Err(EvalError::TypeError(
            "write-bytevector: port argument required".to_string(),
        ));
    };

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
        Ok(()) => Ok(TaggedValue::UNSPECIFIED),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}
