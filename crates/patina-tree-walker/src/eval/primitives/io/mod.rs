//! I/O primitives for R7RS
//!
//! This module implements R7RS I/O operations organized into submodules:
//! - Port predicates: port?, input-port?, output-port?, etc.
//! - String ports: open-input-string, open-output-string, get-output-string
//! - Current ports: current-input-port, current-output-port, current-error-port
//! - EOF handling: eof-object?, eof-object
//! - Text I/O: display, write, newline, read-char, peek-char, char-ready?
//! - Binary I/O: read-u8, peek-u8, write-u8, read-bytevector, write-bytevector
//! - File I/O: open-input-file, open-output-file, file-exists?, delete-file
//! - Read: read (parse S-expressions from port)

mod binary;
mod datum_writer;
mod file;
mod ports;
mod read;
mod text_input;
mod text_output;

use super::PrimitiveRegistry;
use super::registry::PrimitiveFn;
use crate::eval::EvalResult;
use patina_runtime::Arity;

// =============================================================================
// Registration
// =============================================================================

pub(super) fn register(registry: &mut PrimitiveRegistry) {
    // Port predicates
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "port?",
        Arity::Exact(1),
        "Returns #t if obj is a port.",
        |eval, args, _tail| ports::port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "input-port?",
        Arity::Exact(1),
        "Returns #t if obj is an input port.",
        |eval, args, _tail| ports::input_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "output-port?",
        Arity::Exact(1),
        "Returns #t if obj is an output port.",
        |eval, args, _tail| ports::output_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "textual-port?",
        Arity::Exact(1),
        "Returns #t if obj is a textual port.",
        |eval, args, _tail| ports::textual_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "binary-port?",
        Arity::Exact(1),
        "Returns #t if obj is a binary port.",
        |eval, args, _tail| ports::binary_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "input-port-open?",
        Arity::Exact(1),
        "Returns #t if port is an open input port.",
        |eval, args, _tail| ports::input_port_open_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "output-port-open?",
        Arity::Exact(1),
        "Returns #t if port is an open output port.",
        |eval, args, _tail| ports::output_port_open_p(eval, args).map(EvalResult::Value),
    ));

    // String ports
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-input-string",
        Arity::Exact(1),
        "Create an input port from a string.",
        |eval, args, _tail| ports::open_input_string(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-output-string",
        Arity::Exact(0),
        "Create an output port that accumulates to a string.",
        |eval, args, _tail| ports::open_output_string(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "get-output-string",
        Arity::Exact(1),
        "Get the accumulated string from an output string port.",
        |eval, args, _tail| ports::get_output_string(eval, args).map(EvalResult::Value),
    ));

    // Bytevector ports
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-input-bytevector",
        Arity::Exact(1),
        "Create an input port from a bytevector.",
        |eval, args, _tail| ports::open_input_bytevector(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-output-bytevector",
        Arity::Exact(0),
        "Create an output port that accumulates to a bytevector.",
        |eval, args, _tail| ports::open_output_bytevector(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "get-output-bytevector",
        Arity::Exact(1),
        "Get the accumulated bytevector from an output bytevector port.",
        |eval, args, _tail| ports::get_output_bytevector(eval, args).map(EvalResult::Value),
    ));

    // Current ports
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "current-input-port",
        Arity::Exact(0),
        "Returns the current input port.",
        |eval, args, _tail| ports::current_input_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "current-output-port",
        Arity::Exact(0),
        "Returns the current output port.",
        |eval, args, _tail| ports::current_output_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "current-error-port",
        Arity::Exact(0),
        "Returns the current error port.",
        |eval, args, _tail| ports::current_error_port(eval, args).map(EvalResult::Value),
    ));

    // Port operations
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "close-port",
        Arity::Exact(1),
        "Close a port.",
        |eval, args, _tail| ports::close_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "close-input-port",
        Arity::Exact(1),
        "Close an input port.",
        |eval, args, _tail| ports::close_input_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "close-output-port",
        Arity::Exact(1),
        "Close an output port.",
        |eval, args, _tail| ports::close_output_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "call-with-port",
        Arity::Exact(2),
        "Calls proc with port, then closes the port.",
        ports::call_with_port,
    ));

    // EOF handling
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "eof-object?",
        Arity::Exact(1),
        "Returns #t if obj is the EOF object.",
        |eval, args, _tail| ports::eof_object_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "eof-object",
        Arity::Exact(0),
        "Returns an EOF object.",
        |eval, args, _tail| ports::eof_object(eval, args).map(EvalResult::Value),
    ));

    // Text output
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "display",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a human-readable format.",
        |eval, args, _tail| text_output::display(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a machine-readable format.",
        |eval, args, _tail| text_output::write(eval, args).map(EvalResult::Value),
    ));

    // (scheme write) library procedures
    // display and write are also exported from (scheme write)
    registry.register(PrimitiveFn::new(
        "scheme.write",
        "display",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a human-readable format.",
        |eval, args, _tail| text_output::display(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.write",
        "write",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a machine-readable format.",
        |eval, args, _tail| text_output::write(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.write",
        "write-shared",
        Arity::Range(1, 2),
        "Writes obj with datum labels for all shared structures.",
        |eval, args, _tail| text_output::write_shared(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.write",
        "write-simple",
        Arity::Range(1, 2),
        "Writes obj without datum labels (may loop on circular structures).",
        |eval, args, _tail| text_output::write_simple(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "newline",
        Arity::Range(0, 1),
        "Writes a newline to the textual output port.",
        |eval, args, _tail| text_output::newline(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-char",
        Arity::Range(1, 2),
        "Writes a character to the textual output port.",
        |eval, args, _tail| text_output::write_char(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-string",
        Arity::Range(1, 4),
        "Writes a string to the textual output port.",
        |eval, args, _tail| text_output::write_string(eval, args).map(EvalResult::Value),
    ));

    // Text input
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-char",
        Arity::Range(0, 1),
        "Reads a single character from the input port.",
        |eval, args, _tail| text_input::read_char(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "peek-char",
        Arity::Range(0, 1),
        "Peeks at the next character without consuming it.",
        |eval, args, _tail| text_input::peek_char(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "char-ready?",
        Arity::Range(0, 1),
        "Returns #t if a character is ready to be read.",
        |eval, args, _tail| text_input::char_ready_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-line",
        Arity::Range(0, 1),
        "Reads a line from the input port.",
        |eval, args, _tail| text_input::read_line(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-string",
        Arity::Range(1, 2),
        "Reads k characters from the input port.",
        |eval, args, _tail| text_input::read_string(eval, args).map(EvalResult::Value),
    ));

    // Binary I/O
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-u8",
        Arity::Range(0, 1),
        "Reads a single byte from a binary input port.",
        |eval, args, _tail| binary::read_u8(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "peek-u8",
        Arity::Range(0, 1),
        "Peeks at the next byte without consuming it.",
        |eval, args, _tail| binary::peek_u8(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "u8-ready?",
        Arity::Range(0, 1),
        "Returns #t if a byte is ready to be read.",
        |eval, args, _tail| binary::u8_ready_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-u8",
        Arity::Range(1, 2),
        "Writes a byte to a binary output port.",
        |eval, args, _tail| binary::write_u8(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-bytevector",
        Arity::Range(1, 2),
        "Reads up to k bytes from a binary input port.",
        |eval, args, _tail| binary::read_bytevector(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-bytevector!",
        Arity::Range(1, 4),
        "Reads into an existing bytevector from a binary input port.",
        |eval, args, _tail| binary::read_bytevector_bang(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-bytevector",
        Arity::Range(1, 4),
        "Writes a bytevector to a binary output port.",
        |eval, args, _tail| binary::write_bytevector(eval, args).map(EvalResult::Value),
    ));

    // Read (S-expression parsing) - in (scheme base)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read",
        Arity::Range(0, 1),
        "Reads a Scheme expression from the input port.",
        |eval, args, _tail| read::read(eval, args).map(EvalResult::Value),
    ));

    // (scheme read) library - re-exports read
    registry.register(PrimitiveFn::new(
        "scheme.read",
        "read",
        Arity::Range(0, 1),
        "Reads a Scheme expression from the input port.",
        |eval, args, _tail| read::read(eval, args).map(EvalResult::Value),
    ));

    // File I/O operations (scheme file library)
    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-input-file",
        Arity::Exact(1),
        "Opens a file for reading and returns an input port.",
        |eval, args, _tail| file::open_input_file(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-output-file",
        Arity::Exact(1),
        "Opens a file for writing and returns an output port.",
        |eval, args, _tail| file::open_output_file(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-binary-input-file",
        Arity::Exact(1),
        "Opens a binary file for reading and returns an input port.",
        |eval, args, _tail| file::open_binary_input_file(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-binary-output-file",
        Arity::Exact(1),
        "Opens a binary file for writing and returns an output port.",
        |eval, args, _tail| file::open_binary_output_file(eval, args).map(EvalResult::Value),
    ));

    // Flush is in scheme base
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "flush-output-port",
        Arity::Range(0, 1),
        "Flushes the output port buffer.",
        |eval, args, _tail| ports::flush_output_port(eval, args).map(EvalResult::Value),
    ));

    // File utilities (scheme file library)
    registry.register(PrimitiveFn::new(
        "scheme.file",
        "file-exists?",
        Arity::Exact(1),
        "Returns #t if the file exists.",
        |eval, args, _tail| file::file_exists_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "delete-file",
        Arity::Exact(1),
        "Deletes the specified file.",
        |eval, args, _tail| file::delete_file(eval, args).map(EvalResult::Value),
    ));

    // Higher-order file operations (scheme file library)
    registry.register(PrimitiveFn::new(
        "scheme.file",
        "call-with-input-file",
        Arity::Exact(2),
        "Opens file for reading, calls proc with port, closes port.",
        file::call_with_input_file,
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "call-with-output-file",
        Arity::Exact(2),
        "Opens file for writing, calls proc with port, closes port.",
        file::call_with_output_file,
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "with-input-from-file",
        Arity::Exact(2),
        "Opens file and sets current-input-port for the duration of thunk.",
        file::with_input_from_file,
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "with-output-to-file",
        Arity::Exact(2),
        "Opens file and sets current-output-port for the duration of thunk.",
        file::with_output_to_file,
    ));
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use patina_runtime::value::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn test_string_port_round_trip() {
        let eval = Evaluator::new();

        // Create output port and write to it
        let out_port = ports::open_output_string(&eval, vec![]).unwrap();

        if let Value::Port(p) = &out_port {
            p.write_string("hello").unwrap();
        }

        // Get the string back
        let result = ports::get_output_string(&eval, vec![out_port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "hello");
        } else {
            panic!("Expected string");
        }
    }

    #[test]
    fn test_input_string_port() {
        let eval = Evaluator::new();
        let input = Value::String(Rc::new(RefCell::new("abc".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();

        // Read characters
        let ch1 = text_input::read_char(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(ch1, Value::Character('a')));

        let ch2 = text_input::read_char(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(ch2, Value::Character('b')));

        let ch3 = text_input::read_char(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(ch3, Value::Character('c')));

        let eof = text_input::read_char(&eval, vec![port]).unwrap();
        assert!(matches!(eof, Value::Eof));
    }

    #[test]
    fn test_port_predicates() {
        let eval = Evaluator::new();

        let in_port = ports::open_input_string(
            &eval,
            vec![Value::String(Rc::new(RefCell::new("".to_string())))],
        )
        .unwrap();
        let out_port = ports::open_output_string(&eval, vec![]).unwrap();

        assert!(matches!(
            ports::port_p(&eval, vec![in_port.clone()]),
            Ok(Value::Boolean(true))
        ));
        assert!(matches!(
            ports::input_port_p(&eval, vec![in_port.clone()]),
            Ok(Value::Boolean(true))
        ));
        assert!(matches!(
            ports::output_port_p(&eval, vec![in_port.clone()]),
            Ok(Value::Boolean(false))
        ));
        assert!(matches!(
            ports::textual_port_p(&eval, vec![in_port]),
            Ok(Value::Boolean(true))
        ));

        assert!(matches!(
            ports::port_p(&eval, vec![out_port.clone()]),
            Ok(Value::Boolean(true))
        ));
        assert!(matches!(
            ports::input_port_p(&eval, vec![out_port.clone()]),
            Ok(Value::Boolean(false))
        ));
        assert!(matches!(
            ports::output_port_p(&eval, vec![out_port]),
            Ok(Value::Boolean(true))
        ));

        // Non-port value
        assert!(matches!(
            ports::port_p(&eval, vec![Value::Integer(42)]),
            Ok(Value::Boolean(false))
        ));
    }

    #[test]
    fn test_eof_handling() {
        let eval = Evaluator::new();

        let eof = ports::eof_object(&eval, vec![]).unwrap();
        assert!(matches!(eof, Value::Eof));

        let is_eof = ports::eof_object_p(&eval, vec![eof]).unwrap();
        assert!(matches!(is_eof, Value::Boolean(true)));

        let not_eof = ports::eof_object_p(&eval, vec![Value::Integer(42)]).unwrap();
        assert!(matches!(not_eof, Value::Boolean(false)));
    }

    #[test]
    fn test_read_basic() {
        let eval = Evaluator::new();

        // Read an integer
        let input = Value::String(Rc::new(RefCell::new("42".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = read::read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(result, Value::Integer(42)));

        // Subsequent read should return EOF
        let eof = read::read(&eval, vec![port]).unwrap();
        assert!(matches!(eof, Value::Eof));
    }

    #[test]
    fn test_read_list() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("(+ 1 2)".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = read::read(&eval, vec![port]).unwrap();
        assert!(matches!(result, Value::Pair(_)));
    }

    #[test]
    fn test_read_multiple_expressions() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("1 2 3".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();

        let r1 = read::read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(r1, Value::Integer(1)));

        let r2 = read::read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(r2, Value::Integer(2)));

        let r3 = read::read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(r3, Value::Integer(3)));

        let eof = read::read(&eval, vec![port]).unwrap();
        assert!(matches!(eof, Value::Eof));
    }

    #[test]
    fn test_read_quoted() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("'foo".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = read::read(&eval, vec![port]).unwrap();
        // Should be (quote foo)
        assert!(matches!(result, Value::Pair(_)));
    }

    #[test]
    fn test_read_sexp_string() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("\"hello world\"".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = read::read(&eval, vec![port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "hello world");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_basic() {
        let eval = Evaluator::new();

        // Read 3 chars from "abcd"
        let input = Value::String(Rc::new(RefCell::new("abcd".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = text_input::read_string(&eval, vec![Value::Integer(3), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "abc");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_eof() {
        let eval = Evaluator::new();

        // Read from empty string returns EOF
        let input = Value::String(Rc::new(RefCell::new("".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = text_input::read_string(&eval, vec![Value::Integer(3), port]).unwrap();
        assert!(matches!(result, Value::Eof));
    }

    #[test]
    fn test_read_string_partial() {
        let eval = Evaluator::new();

        // Read more chars than available
        let input = Value::String(Rc::new(RefCell::new("hi".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = text_input::read_string(&eval, vec![Value::Integer(10), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "hi");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_zero() {
        let eval = Evaluator::new();

        // Read 0 chars returns empty string (not EOF)
        let input = Value::String(Rc::new(RefCell::new("hello".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = text_input::read_string(&eval, vec![Value::Integer(0), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "");
        } else {
            panic!("Expected empty string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_with_newline() {
        let eval = Evaluator::new();

        // Read includes newline character
        let input = Value::String(Rc::new(RefCell::new("abc\ndef".to_string())));
        let port = ports::open_input_string(&eval, vec![input]).unwrap();
        let result = text_input::read_string(&eval, vec![Value::Integer(5), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "abc\nd");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }
}
