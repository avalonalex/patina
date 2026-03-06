//! Text output operations
//!
//! This module implements R7RS text output procedures:
//! - display, write, write-shared, write-simple
//! - newline, write-char, write-string

use super::datum_writer::{
    format_display_tagged, format_write_shared_tagged, format_write_simple_tagged,
    format_write_tagged,
};
use super::ports::get_output_port_tagged;
use patina_core::TaggedValue;
use patina_runtime::{EvalError, SharedHeap};

/// (display obj [port]) - Write obj in human-readable format
/// Handles circular structures using datum labels (#n= and #n#)
pub(super) fn display(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "display expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_output_port_tagged(&args, 1, &heap_ref)?
    };
    let output = format_display_tagged(args[0], heap);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(TaggedValue::UNSPECIFIED)
}

/// (write obj [port]) - Write obj in machine-readable format
/// Handles circular structures using datum labels (#n= and #n#)
pub(super) fn write(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_output_port_tagged(&args, 1, &heap_ref)?
    };
    let output = format_write_tagged(args[0], heap);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(TaggedValue::UNSPECIFIED)
}

/// (write-shared obj [port]) - Write obj with datum labels for all shared structures
/// Labels both circular and shared (multiply-referenced) structures
pub(super) fn write_shared(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-shared expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_output_port_tagged(&args, 1, &heap_ref)?
    };
    let output = format_write_shared_tagged(args[0], heap);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(TaggedValue::UNSPECIFIED)
}

/// (write-simple obj [port]) - Write obj without datum labels
/// Does not handle circular structures (may loop infinitely)
pub(super) fn write_simple(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-simple expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_output_port_tagged(&args, 1, &heap_ref)?
    };
    let output = format_write_simple_tagged(args[0], heap);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(TaggedValue::UNSPECIFIED)
}

/// (newline [port]) - Write a newline character
pub(super) fn newline(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "newline expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = {
        let heap_ref = heap.borrow();
        get_output_port_tagged(&args, 0, &heap_ref)?
    };
    port.write_string("\n")
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(TaggedValue::UNSPECIFIED)
}

/// (write-char char [port]) - Write a single character
pub(super) fn write_char(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-char expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract char directly from TaggedValue
    let ch = if args[0].is_char() {
        args[0].as_char_unchecked()
    } else {
        return Err(EvalError::TypeError(
            "write-char expects a character".to_string(),
        ));
    };

    let port = {
        let heap_ref = heap.borrow();
        get_output_port_tagged(&args, 1, &heap_ref)?
    };
    port.write_char(ch)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(TaggedValue::UNSPECIFIED)
}

/// (write-string string [port [start [end]]]) - Write a string
pub(super) fn write_string(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "write-string expects 1 to 4 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract string chars
    let chars: Vec<char> = {
        let heap_ref = heap.borrow();
        if args[0].is_string() {
            heap_ref.get_string_chars(args[0]).to_vec()
        } else {
            heap_ref
                .get_string_contents(args[0])
                .map(|s| s.chars().collect())
                .ok_or_else(|| EvalError::TypeError("write-string expects a string".to_string()))?
        }
    };

    // Extract port
    let port = {
        let heap_ref = heap.borrow();
        get_output_port_tagged(&args, 1, &heap_ref)?
    };

    // Handle optional start/end indices
    let start = if args.len() > 2 {
        if args[2].is_fixnum() {
            args[2].as_fixnum_unchecked() as usize
        } else {
            return Err(EvalError::TypeError(
                "write-string: start must be an integer".to_string(),
            ));
        }
    } else {
        0
    };

    let end = if args.len() > 3 {
        if args[3].is_fixnum() {
            args[3].as_fixnum_unchecked() as usize
        } else {
            return Err(EvalError::TypeError(
                "write-string: end must be an integer".to_string(),
            ));
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

    Ok(TaggedValue::UNSPECIFIED)
}
