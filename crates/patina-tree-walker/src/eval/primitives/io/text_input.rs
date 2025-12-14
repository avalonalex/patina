//! Text input operations
//!
//! This module implements R7RS text input procedures:
//! - read-char, peek-char, char-ready?
//! - read-line, read-string

use super::ports::get_input_port;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// (read-char [port]) - Read a single character
pub(super) fn read_char(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.read_char() {
        Ok(Some(ch)) => Ok(Value::Character(ch)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-char [port]) - Peek at next character without consuming it
pub(super) fn peek_char(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.peek_char() {
        Ok(Some(ch)) => Ok(Value::Character(ch)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (char-ready? [port]) - Check if a character is ready to be read
pub(super) fn char_ready_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "char-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.char_ready() {
        Ok(ready) => Ok(Value::Boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-line [port]) - Read a line as a string
pub(super) fn read_line(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-line expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.read_line() {
        Ok(Some(mut line)) => {
            // Remove trailing newline if present (R7RS behavior)
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(Value::String(Rc::new(RefCell::new(line.chars().collect()))))
        }
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-string k [port]) - Read k characters from port
/// Returns a string of up to k characters, or eof-object if at EOF before reading any
pub(super) fn read_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-string expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let k = match &args[0] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "read-string: k must be a non-negative integer".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "read-string: k must be an integer".to_string(),
            ));
        }
    };

    let port = get_input_port(&args, 1)?;

    // Special case: reading 0 characters returns empty string
    if k == 0 {
        return Ok(Value::String(Rc::new(RefCell::new(Vec::new()))));
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
        Ok(Value::Eof)
    } else {
        Ok(Value::String(Rc::new(RefCell::new(
            result.chars().collect(),
        ))))
    }
}
