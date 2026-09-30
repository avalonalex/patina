//! Read (S-expression parsing)
//!
//! This module implements the R7RS read procedure for parsing
//! S-expressions from input ports.

use super::ports::get_input_port_tagged;
use patina_core::TaggedValue;
use patina_frontend::Parser;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

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

    let name = port.input_name();
    Parser::read_from_port(port.clone(), heap.clone())
        .map(|datum| datum.unwrap_or(TaggedValue::EOF))
        .map_err(|e| read_error(&e, &name, &port))
}

/// Report the data source's coordinates, not coordinates in an unread slice
/// or just the Scheme call site. Syntax and encoding errors remain read-error?.
fn read_error(e: &patina_frontend::ParseError, name: &str, port: &patina_core::Port) -> EvalError {
    use patina_frontend::{ParseError, lexer::LexError};
    let at = e
        .span()
        .map(|span| (span.start.line, span.start.column))
        .unwrap_or_else(|| {
            let at = port.input_position().cursor;
            (at.line, at.column)
        });
    let message = match e.kind() {
        ParseError::LexError(lex) => match lex.kind() {
            LexError::Input(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                "invalid UTF-8 in binary port".to_string()
            }
            LexError::Input(error) => return EvalError::IOError(error.to_string()),
            _ => e.to_string(),
        },
        _ => e.to_string(),
    };
    EvalError::InvalidSyntax(format!("read: {name}:{}:{}: {message}", at.0, at.1))
}
