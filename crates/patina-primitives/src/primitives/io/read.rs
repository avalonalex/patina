//! Read (S-expression parsing)
//!
//! This module implements the R7RS read procedure for parsing
//! S-expressions from input ports.

use super::ports::get_input_port_tagged;
use patina_core::TaggedValue;
use patina_frontend::{Parser, Reader};
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;
use patina_runtime::{Port, PortData};
use std::io::{self, BufRead};
use std::rc::Rc;

/// (read [port]) - Read a Scheme expression from an input port
/// Returns the parsed value, or eof-object if at end of input
pub(super) fn read(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract port from tagged args or use current-input-port
    let port = {
        let heap_ref = heap.borrow();
        get_input_port_tagged(args, 0, &heap_ref)?
    };

    // Determine what kind of port we have and get content if applicable
    let mut undecodable_follows = false;
    let (remaining, is_stdin, is_file) = {
        let data = port.data.borrow();
        match &*data {
            PortData::String(s) => {
                // For string ports, copy the remaining content
                (Some(s.content[s.position..].to_string()), false, false)
            }
            // A bytevector port is read like a string port, over the text at
            // its front: `read` is a textual read like `read-char` and
            // `read-line`, which already decode UTF-8 from a binary port, and
            // chibi and Gauche both read a datum from one (#404). Only the
            // decodable prefix is parsed, because what follows the datum need
            // not be text — a header, then a binary body — and the position
            // advances by the datum's bytes alone, so `read-u8` continues
            // from the byte after it.
            PortData::Bytevector(b) => {
                let bytes = &b.content[b.position..];
                let text = patina_core::port::utf8_prefix(bytes);
                undecodable_follows = text.len() < bytes.len();
                (Some(text.to_string()), false, false)
            }
            PortData::Stdio(patina_runtime::StdioKind::Stdin) => (None, true, false),
            PortData::Stdio(_) => {
                return Err(EvalError::TypeError("not an input port".to_string()));
            }
            PortData::File(_) => (None, false, true),
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
    // Parse directly into the evaluator's heap. The parser reads its first
    // token here, so a string cut short by the undecodable bytes is met here.
    let mut parser = match Parser::new_with_heap(&remaining, heap.clone()) {
        Ok(parser) => parser,
        Err(e) if undecodable_follows && ran_out_of_text(&e) => return Err(undecodable_bytes()),
        Err(e) => return Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    };

    match parser.parse_next() {
        Ok(Some(tv)) => {
            // Advance the port past exactly what the parser consumed
            let consumed_bytes: usize = remaining
                .chars()
                .take(parser.consumed_end())
                .map(|c| c.len_utf8())
                .sum();
            // The datum's last token reached the end of the text, and the
            // text ended because the bytes stopped decoding, not because the
            // port did. A closer ends its datum whatever follows it, so
            // `(header)` and `"header"` may sit against a binary body. Any
            // other token is ended by a delimiter, and the byte after this
            // one is not one: chibi and Gauche both read it into the token.
            // Returning `x` for `x\xFF` would be inventing a delimiter.
            if undecodable_follows
                && consumed_bytes == remaining.len()
                && !remaining.ends_with([')', ']', '"', '|'])
            {
                return Err(undecodable_bytes());
            }
            port.advance_position(consumed_bytes)
                .map_err(|e| EvalError::IOError(e.to_string()))?;
            Ok(tv)
        }
        Ok(None) => {
            // Whitespace and completed comments have also been consumed.
            port.advance_position(remaining.len())
                .map_err(|e| EvalError::IOError(e.to_string()))?;
            // No datum in the text, but the port is not at its end: what is
            // left is bytes that do not decode. The other textual reads raise
            // there too, rather than report an end of file.
            if undecodable_follows {
                return Err(undecodable_bytes());
            }
            Ok(TaggedValue::EOF)
        }
        // The same again, inside a datum: the text ran out, the port did not.
        // "Unexpected end of input" would send someone looking for a missing
        // parenthesis in a port whose next byte is 0xFF.
        Err(e) if undecodable_follows && ran_out_of_text(&e) => Err(undecodable_bytes()),
        Err(e) => Err(read_error(&e)),
    }
}

/// What `read` reports when the text at the front of a binary port stops at
/// bytes that do not decode, and the datum — or the search for one — reached
/// them.
///
/// `InvalidSyntax`, as every other error `read` raises is, because that is
/// what makes it a `read-error?` on both backends. As an `IOError` it was one
/// on the VM alone: the VM classifies by the `read:` in the message, the
/// tree-walker by the variant first.
fn undecodable_bytes() -> EvalError {
    EvalError::InvalidSyntax("read: invalid UTF-8 in binary port".to_string())
}

/// Whether `e` says the text ended before the datum did: inside a list, after
/// a prefix, or inside a token that more text could finish. What a parser
/// handed only the decodable prefix of a binary port reports when the datum
/// runs on into the rest — and the only errors that prefix can be blamed for;
/// `#\bogus` is wrong whatever follows it.
fn ran_out_of_text(e: &patina_frontend::ParseError) -> bool {
    use patina_frontend::ParseError;
    match e {
        ParseError::IncompleteDatum { .. } | ParseError::UnexpectedEof => true,
        ParseError::LexError(lex) => lex.is_incomplete(),
        _ => false,
    }
}

/// Read one datum from a line-oriented source (stdin or a file port).
///
/// Lines are read until they hold a complete datum, each fed to a [`Reader`]
/// as it arrives: the datum is read once, rather than the text being read
/// again after every line, which cost time proportional to the square of a
/// datum's length (#341).
///
/// Any text after the datum is stored in the port's pushback buffer so the
/// next textual read — `read`, `read-char`, `read-line`, ... — continues from
/// it instead of it being lost with the local buffer.
fn read_buffered(
    port: &Rc<Port>,
    heap: &SharedHeap,
    mut next_line: impl FnMut() -> Result<Option<String>, EvalError>,
) -> Result<TaggedValue, EvalError> {
    let mut text = port.take_pushback();
    let mut reader = Reader::new(patina_frontend::dialect::allow_r6rs());
    reader.feed(&text);

    loop {
        if let Some(datum) = reader.next_datum(heap, |parser| parser) {
            let value = datum.map_err(|e| read_error(&e))?;
            port.set_pushback(remainder_after(&text, reader.take_consumed()));
            return Ok(value);
        }

        match next_line()? {
            Some(line) => {
                reader.feed(&line);
                text.push_str(&line);
            }
            None => {
                // Nothing more is coming, so what is left is read as it
                // stands: a trailing datum is returned, and one that is
                // unfinished is reported rather than waited for.
                reader.no_more_text();
                return match reader.next_datum(heap, |parser| parser) {
                    Some(Ok(value)) => {
                        port.set_pushback(remainder_after(&text, reader.take_consumed()));
                        Ok(value)
                    }
                    Some(Err(e)) => Err(read_error(&e)),
                    None => Ok(TaggedValue::EOF),
                };
            }
        }
    }
}

/// Render a parse error for `read`.
///
/// `IncompleteDatum` drops its line and column on the way out: the parser
/// counts them from the start of the text it was handed, which here is
/// whatever the port has not read yet, so they would name a position in a
/// slice the caller cannot see rather than one in the file.
fn read_error(e: &patina_frontend::ParseError) -> EvalError {
    if matches!(e, patina_frontend::ParseError::IncompleteDatum { .. }) {
        return EvalError::InvalidSyntax(
            "read: unexpected end of input inside a datum".to_string(),
        );
    }
    EvalError::InvalidSyntax(format!("read: {}", e))
}

/// Text after the first `consumed_chars` characters of `buffer`
fn remainder_after(buffer: &str, consumed_chars: usize) -> String {
    buffer.chars().skip(consumed_chars).collect()
}
