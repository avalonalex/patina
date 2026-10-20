//! Text input operations
//!
//! This module implements R7RS text input procedures:
//! - read-char, peek-char, char-ready?
//! - read-line, read-string
//!
//! Each takes the context (#706).

use super::ports::get_input_port;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

/// (read-char [port]) - Read a single character
pub(super) fn read_char<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(cx, args, 0)?;
    match port.read_char() {
        Ok(Some(ch)) => Ok(Value::character(ch)),
        Ok(None) => Ok(Value::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-char [port]) - Peek at next character without consuming it
pub(super) fn peek_char<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(cx, args, 0)?;
    match port.peek_char() {
        Ok(Some(ch)) => Ok(Value::character(ch)),
        Ok(None) => Ok(Value::EOF),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (char-ready? [port]) - Check if a character is ready to be read
pub(super) fn char_ready_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "char-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(cx, args, 0)?;
    match port.char_ready() {
        Ok(ready) => Ok(Value::boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-line [port [max-chars]]) - Read a line as a string
///
/// The second argument is chibi's extension, not R7RS: read at most
/// `max-chars` *characters* (chibi's unit — not bytes, so a multi-byte
/// character counts once), still stopping early at a newline (consumed, not
/// included), with anything past the limit left in the port. R7RS makes the
/// extra argument "an error", i.e. implementation freedom, and both
/// references accept it — chibi honors the limit, Gauche ignores the
/// argument. Real code relies on it: chibi-mime reads every header line
/// through `(read-line port mime-line-length-limit)` as a DoS guard, and
/// rejecting the arity failed its whole suite.
pub(super) fn read_line<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-line expects 0 to 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(cx, args, 0)?;

    // Both forms go through one character loop, so the line endings are the
    // same with and without a max: R7RS 7.1.1's newline, return, and
    // return+newline (`Port::read_line`, kept for the reader, is
    // newline-only and used to leave "abc\rdef" as one line).
    let max = if args.len() == 2 {
        match args[1].as_fixnum() {
            Some(n) if n >= 0 => n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "read-line: max-chars must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        usize::MAX
    };
    let mut line = String::new();
    let mut chars_read = 0;
    while chars_read < max {
        match port.read_char() {
            Ok(Some('\n')) => break,
            // A bare return ends the line too, and return+newline is one
            // ending: the newline that may follow is consumed rather than
            // read as an empty next line.
            Ok(Some('\r')) => {
                if matches!(port.peek_char(), Ok(Some('\n'))) {
                    let _ = port.read_char();
                }
                break;
            }
            Ok(Some(c)) => {
                line.push(c);
                chars_read += 1;
            }
            // The source ended before anything was read: that is EOF.
            Ok(None) if line.is_empty() => return Ok(Value::EOF),
            Ok(None) => break,
            Err(e) => return Err(EvalError::IOError(e.to_string())),
        }
    }
    // max = 0 skips the loop entirely; peek to tell EOF from data-remaining
    // (the one case the loop could not decide).
    if max == 0 && matches!(port.peek_char(), Ok(None)) {
        return Ok(Value::EOF);
    }
    Ok(cx.alloc_string(line))
}

/// (read-string k [port]) - Read k characters from port
/// Returns a string of up to k characters, or eof-object if at EOF before reading any
pub(super) fn read_string<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-string expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract k
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

    let port = get_input_port(cx, args, 1)?;

    // Special case: reading 0 characters returns empty string
    if k == 0 {
        return Ok(cx.alloc_string(String::new()));
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
        Ok(Value::EOF)
    } else {
        Ok(cx.alloc_string(result))
    }
}
