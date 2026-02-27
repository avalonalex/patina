//! Text input operations
//!
//! This module implements R7RS text input procedures:
//! - read-char, peek-char, char-ready?
//! - read-line, read-string

use super::ports::get_input_port_tagged;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_core::TaggedValue;

/// (read-char [port]) - Read a single character
pub(super) fn read_char(
    eval: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = eval.global_env.heap();
    let port = {
        let heap_ref = heap.borrow();
        get_input_port_tagged(&args, 0, &heap_ref)?
    };
    match port.read_char() {
        Ok(Some(ch)) => Ok(TaggedValue::character(ch)),
        Ok(None) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-char [port]) - Peek at next character without consuming it
pub(super) fn peek_char(
    eval: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = eval.global_env.heap();
    let port = {
        let heap_ref = heap.borrow();
        get_input_port_tagged(&args, 0, &heap_ref)?
    };
    match port.peek_char() {
        Ok(Some(ch)) => Ok(TaggedValue::character(ch)),
        Ok(None) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (char-ready? [port]) - Check if a character is ready to be read
pub(super) fn char_ready_p(
    eval: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "char-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = eval.global_env.heap();
    let port = {
        let heap_ref = heap.borrow();
        get_input_port_tagged(&args, 0, &heap_ref)?
    };
    match port.char_ready() {
        Ok(ready) => Ok(TaggedValue::boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-line [port]) - Read a line as a string
pub(super) fn read_line(
    eval: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-line expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = eval.global_env.heap();
    let port = {
        let heap_ref = heap.borrow();
        get_input_port_tagged(&args, 0, &heap_ref)?
    };
    match port.read_line() {
        Ok(Some(mut line)) => {
            // Remove trailing newline if present (R7RS behavior)
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(heap.borrow_mut().alloc_string(line))
        }
        Ok(None) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-string k [port]) - Read k characters from port
/// Returns a string of up to k characters, or eof-object if at EOF before reading any
pub(super) fn read_string(
    eval: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-string expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = eval.global_env.heap();

    // Extract k from TaggedValue directly
    let k = if args[0].is_fixnum() {
        let n = args[0].as_fixnum_unchecked();
        if n < 0 {
            return Err(EvalError::TypeError(
                "read-string: k must be a non-negative integer".to_string(),
            ));
        }
        n as usize
    } else {
        return Err(EvalError::TypeError(
            "read-string: k must be an integer".to_string(),
        ));
    };

    let port = {
        let heap_ref = heap.borrow();
        get_input_port_tagged(&args, 1, &heap_ref)?
    };

    // Special case: reading 0 characters returns empty string
    if k == 0 {
        return Ok(heap.borrow_mut().alloc_string(String::new()));
    }

    let mut result = String::new();
    for _ in 0..k {
        match port.read_char() {
            Ok(Some(ch)) => result.push(ch),
            Ok(None) => break, // EOF
            Err(e) => return Err(EvalError::IOError(e.to_string())),
        }
    }

    if result.is_empty() {
        // If we couldn't read any characters (hit EOF immediately), return EOF
        Ok(TaggedValue::EOF)
    } else {
        Ok(heap.borrow_mut().alloc_string(result))
    }
}
