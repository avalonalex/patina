//! Read (S-expression parsing)
//!
//! This module implements the R7RS read procedure for parsing
//! S-expressions from input ports.

use super::ports::get_input_port_tagged;
use patina_core::TaggedValue;
use patina_frontend::Parser;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;
use patina_runtime::{Port, PortData};
use std::io::{self, BufRead};
use std::rc::Rc;

/// (read [port]) - Read a Scheme expression from an input port
/// Returns the parsed value, or eof-object if at end of input
pub(super) fn read(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract port from tagged args or use current-input-port
    let port = {
        let heap_ref = heap.borrow();
        get_input_port_tagged(&args, 0, &heap_ref)?
    };

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
        return read_buffered(&port, heap, || {
            let stdin = io::stdin();
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => Ok(None),
                Ok(_) => Ok(Some(line)),
                Err(e) => Err(EvalError::IOError(e.to_string())),
            }
        });
    }

    if is_file {
        // The pushback buffer was already drained by read_buffered, so
        // Port::read_line reads from the underlying file here
        let file_port = port.clone();
        return read_buffered(&port, heap, move || {
            file_port
                .read_line()
                .map_err(|e| EvalError::IOError(e.to_string()))
        });
    }

    let remaining = remaining.unwrap();
    if remaining.trim().is_empty() {
        return Ok(TaggedValue::EOF);
    }

    // Parse directly into the evaluator's heap
    let mut parser = Parser::new_with_heap(&remaining, heap.clone())
        .map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;

    match parser.parse() {
        Ok(tv) => {
            // Advance the port past exactly what the parser consumed
            let consumed_bytes: usize = remaining
                .chars()
                .take(parser.consumed_end())
                .map(|c| c.len_utf8())
                .sum();
            port.advance_position(consumed_bytes)
                .map_err(|e| EvalError::IOError(e.to_string()))?;
            Ok(tv)
        }
        Err(patina_frontend::ParseError::UnexpectedEof) => Ok(TaggedValue::EOF),
        Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    }
}

/// Read one datum from a line-oriented source (stdin or a file port).
///
/// Lines are accumulated until they form a complete datum. Any text after
/// the datum is stored in the port's pushback buffer so the next textual
/// read — `read`, `read-char`, `read-line`, ... — continues from it instead
/// of it being lost with the local buffer.
fn read_buffered(
    port: &Rc<Port>,
    heap: &SharedHeap,
    mut next_line: impl FnMut() -> Result<Option<String>, EvalError>,
) -> Result<TaggedValue, EvalError> {
    let mut buffer = port.take_pushback();

    loop {
        if !buffer.trim().is_empty() {
            // Constructor failure means the first token is incomplete
            // (e.g. an unterminated string) — fall through for more input
            if let Ok(mut parser) = Parser::new_with_heap(&buffer, heap.clone()) {
                match parser.parse() {
                    Ok(tv) => {
                        port.set_pushback(remainder_after(&buffer, parser.consumed_end()));
                        return Ok(tv);
                    }
                    Err(patina_frontend::ParseError::UnexpectedEof) => {
                        // Datum incomplete — need more input
                    }
                    Err(e) => {
                        return Err(EvalError::InvalidSyntax(format!("read: {}", e)));
                    }
                }
            }
        }

        match next_line()? {
            Some(line) => buffer.push_str(&line),
            None => {
                // EOF on the underlying source: parse what is left so a
                // trailing datum (or error) is surfaced
                if buffer.trim().is_empty() {
                    return Ok(TaggedValue::EOF);
                }
                let mut parser = Parser::new_with_heap(&buffer, heap.clone())
                    .map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;
                return match parser.parse() {
                    Ok(tv) => {
                        port.set_pushback(remainder_after(&buffer, parser.consumed_end()));
                        Ok(tv)
                    }
                    Err(patina_frontend::ParseError::UnexpectedEof) => Ok(TaggedValue::EOF),
                    Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
                };
            }
        }
    }
}

/// Text after the first `consumed_chars` characters of `buffer`
fn remainder_after(buffer: &str, consumed_chars: usize) -> String {
    buffer.chars().skip(consumed_chars).collect()
}
