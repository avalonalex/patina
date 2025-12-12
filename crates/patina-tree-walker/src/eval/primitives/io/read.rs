//! Read (S-expression parsing)
//!
//! This module implements the R7RS read procedure for parsing
//! S-expressions from input ports.

use super::ports::get_input_port;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_frontend::Parser;
use patina_runtime::value::Value;
use patina_runtime::{Port, PortData};
use std::io::{self, BufRead};
use std::rc::Rc;

/// (read [port]) - Read a Scheme expression from an input port
/// Returns the parsed value, or eof-object if at end of input
pub(super) fn read(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;

    // Determine what kind of port we have and get content if applicable
    let (remaining, is_stdin, is_file) = {
        let data = port.data.borrow();
        match &*data {
            PortData::String(s) => {
                // For string ports, copy the remaining content
                (Some(s.content[s.position..].to_string()), false, false)
            }
            PortData::Stdio(patina_runtime::StdioKind::Stdin) => (None, true, false),
            PortData::Stdio(_) => {
                return Err(EvalError::TypeError("not an input port".to_string()));
            }
            PortData::File(_) => (None, false, true),
            PortData::Bytevector(_) => {
                return Err(EvalError::TypeError("read: not a textual port".to_string()));
            }
            PortData::Closed => {
                return Err(EvalError::IOError("port is closed".to_string()));
            }
        }
    };

    if is_stdin {
        return read_from_stdin();
    }

    if is_file {
        return read_from_file_port(&port);
    }

    let remaining = remaining.unwrap();
    if remaining.trim().is_empty() {
        return Ok(Value::Eof);
    }

    // Parse the expression
    let mut parser =
        Parser::new(&remaining).map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;

    let result = parser.parse();

    match result {
        Ok(value) => {
            // Calculate bytes consumed by finding how much the parser consumed
            let consumed = calculate_consumed_bytes(&remaining, &value);
            port.advance_position(consumed)
                .map_err(|e| EvalError::IOError(e.to_string()))?;
            Ok(value)
        }
        Err(patina_frontend::ParseError::UnexpectedEof) => Ok(Value::Eof),
        Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    }
}

/// Read a complete S-expression from stdin
/// This accumulates lines until we have a complete expression
fn read_from_stdin() -> Result<Value, EvalError> {
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut buffer = String::new();

    loop {
        let mut line = String::new();
        match handle.read_line(&mut line) {
            Ok(0) => {
                // EOF
                if buffer.trim().is_empty() {
                    return Ok(Value::Eof);
                }
                // Try to parse what we have
                break;
            }
            Ok(_) => {
                buffer.push_str(&line);
                // Try to parse - if successful, we're done
                // If we get UnexpectedEof, we need more input
                if let Ok(mut parser) = Parser::new(&buffer) {
                    match parser.parse() {
                        Ok(value) => return Ok(value),
                        Err(patina_frontend::ParseError::UnexpectedEof) => {
                            // Need more input
                            continue;
                        }
                        Err(e) => {
                            return Err(EvalError::InvalidSyntax(format!("read: {}", e)));
                        }
                    }
                }
            }
            Err(e) => return Err(EvalError::IOError(e.to_string())),
        }
    }

    // Try to parse the accumulated buffer
    let mut parser =
        Parser::new(&buffer).map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;

    match parser.parse() {
        Ok(value) => Ok(value),
        Err(patina_frontend::ParseError::UnexpectedEof) => Ok(Value::Eof),
        Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    }
}

/// Read a complete S-expression from a file port
/// Reads lines until we have a complete expression
fn read_from_file_port(port: &Rc<Port>) -> Result<Value, EvalError> {
    let mut buffer = String::new();

    loop {
        // Read a line from the file
        match port.read_line() {
            Ok(None) => {
                // EOF
                if buffer.trim().is_empty() {
                    return Ok(Value::Eof);
                }
                // Try to parse what we have
                break;
            }
            Ok(Some(line)) => {
                buffer.push_str(&line);
                // Try to parse - if successful, we're done
                // If we get UnexpectedEof, we need more input
                if let Ok(mut parser) = Parser::new(&buffer) {
                    match parser.parse() {
                        Ok(value) => return Ok(value),
                        Err(patina_frontend::ParseError::UnexpectedEof) => {
                            // Need more input
                            continue;
                        }
                        Err(e) => {
                            return Err(EvalError::InvalidSyntax(format!("read: {}", e)));
                        }
                    }
                }
            }
            Err(e) => return Err(EvalError::IOError(e.to_string())),
        }
    }

    // Try to parse the accumulated buffer
    let mut parser =
        Parser::new(&buffer).map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;

    match parser.parse() {
        Ok(value) => Ok(value),
        Err(patina_frontend::ParseError::UnexpectedEof) => Ok(Value::Eof),
        Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    }
}

/// Calculate how many bytes were consumed to parse a value
/// This is done by re-parsing and finding where the parser stops
fn calculate_consumed_bytes(input: &str, _parsed: &Value) -> usize {
    // Skip leading whitespace
    let trimmed = input.trim_start();
    let whitespace_len = input.len() - trimmed.len();

    if trimmed.is_empty() {
        return input.len();
    }

    // Parse once and measure by trying increasingly smaller substrings
    // until parsing fails or gives a different result
    //
    // A more efficient approach: find the logical end of the expression
    // by tracking parentheses/brackets and string boundaries
    let mut depth = 0;
    let mut in_string = false;
    let mut in_line_comment = false;
    let mut i = 0;
    let chars: Vec<char> = trimmed.chars().collect();

    while i < chars.len() {
        let ch = chars[i];

        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if in_string {
            if ch == '\\' && i + 1 < chars.len() {
                i += 2; // Skip escape sequence
                continue;
            }
            if ch == '"' {
                in_string = false;
                if depth == 0 {
                    i += 1;
                    break;
                }
            }
            i += 1;
            continue;
        }

        match ch {
            ';' => {
                in_line_comment = true;
                i += 1;
            }
            '"' => {
                in_string = true;
                i += 1;
            }
            '(' | '[' | '{' => {
                depth += 1;
                i += 1;
            }
            ')' | ']' | '}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            '#' if i + 1 < chars.len() && chars[i + 1] == '(' => {
                // Vector #(
                depth += 1;
                i += 2;
            }
            '#' if i + 3 < chars.len()
                && chars[i + 1] == 'u'
                && chars[i + 2] == '8'
                && chars[i + 3] == '(' =>
            {
                // Bytevector #u8(
                depth += 1;
                i += 4;
            }
            '\'' | '`' => {
                // Quote/quasiquote - need to include the following expression
                i += 1;
            }
            ',' => {
                // Unquote - may be ,@ or just ,
                i += 1;
                if i < chars.len() && chars[i] == '@' {
                    i += 1;
                }
            }
            _ if ch.is_whitespace() => {
                if depth == 0 && i > 0 {
                    // End of atom at top level
                    break;
                }
                i += 1;
            }
            _ => {
                // Part of an atom
                if depth == 0 {
                    // Read until delimiter
                    while i < chars.len() {
                        let c = chars[i];
                        if c.is_whitespace()
                            || c == '('
                            || c == ')'
                            || c == '"'
                            || c == ';'
                            || c == '\''
                            || c == '`'
                            || c == ','
                        {
                            break;
                        }
                        i += 1;
                    }
                    break;
                }
                i += 1;
            }
        }
    }

    // Calculate byte offset from character offset
    let consumed_chars: String = chars[..i].iter().collect();
    whitespace_len + consumed_chars.len()
}
