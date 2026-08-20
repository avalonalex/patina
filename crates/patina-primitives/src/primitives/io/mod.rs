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
pub mod datum_writer;
mod directory;
mod file;
mod ports;
mod read;
mod text_input;
mod text_output;

use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use patina_runtime::Arity;

// =============================================================================
// Registration
// =============================================================================

pub(super) fn register(registry: &mut PrimitiveRegistry) {
    // Port predicates
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "port?",
        Arity::Exact(1),
        "Returns #t if obj is a port.",
        ports::port_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "input-port?",
        Arity::Exact(1),
        "Returns #t if obj is an input port.",
        ports::input_port_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "output-port?",
        Arity::Exact(1),
        "Returns #t if obj is an output port.",
        ports::output_port_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "textual-port?",
        Arity::Exact(1),
        "Returns #t if obj is a textual port.",
        ports::textual_port_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "binary-port?",
        Arity::Exact(1),
        "Returns #t if obj is a binary port.",
        ports::binary_port_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "input-port-open?",
        Arity::Exact(1),
        "Returns #t if port is an open input port.",
        ports::input_port_open_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "output-port-open?",
        Arity::Exact(1),
        "Returns #t if port is an open output port.",
        ports::output_port_open_p,
    ));

    // String ports
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "open-input-string",
        Arity::Exact(1),
        "Create an input port from a string.",
        ports::open_input_string,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "open-output-string",
        Arity::Exact(0),
        "Create an output port that accumulates to a string.",
        ports::open_output_string,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "get-output-string",
        Arity::Exact(1),
        "Get the accumulated string from an output string port.",
        ports::get_output_string,
    ));

    // Bytevector ports
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "open-input-bytevector",
        Arity::Exact(1),
        "Create an input port from a bytevector.",
        ports::open_input_bytevector,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "open-output-bytevector",
        Arity::Exact(0),
        "Create an output port that accumulates to a bytevector.",
        ports::open_output_bytevector,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "get-output-bytevector",
        Arity::Exact(1),
        "Get the accumulated bytevector from an output bytevector port.",
        ports::get_output_bytevector,
    ));

    // Current ports
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "current-input-port",
        Arity::Range(0, 1),
        "Returns the current input port; with an argument, installs one (R7RS parameter).",
        ports::current_input_port,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "current-output-port",
        Arity::Range(0, 1),
        "Returns the current output port; with an argument, installs one (R7RS parameter).",
        ports::current_output_port,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "current-error-port",
        Arity::Range(0, 1),
        "Returns the current error port; with an argument, installs one (R7RS parameter).",
        ports::current_error_port,
    ));

    // Port operations
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "close-port",
        Arity::Exact(1),
        "Close a port.",
        ports::close_port,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "close-input-port",
        Arity::Exact(1),
        "Close an input port.",
        ports::close_input_port,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "close-output-port",
        Arity::Exact(1),
        "Close an output port.",
        ports::close_output_port,
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "call-with-port",
        Arity::Exact(2),
        "Calls proc with port, then closes the port.",
        ports::call_with_port,
    ));

    // EOF handling
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "eof-object?",
        Arity::Exact(1),
        "Returns #t if obj is the EOF object.",
        ports::eof_object_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "eof-object",
        Arity::Exact(0),
        "Returns an EOF object.",
        ports::eof_object,
    ));

    // Text output
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "display",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a human-readable format.",
        text_output::display,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "write",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a machine-readable format.",
        text_output::write,
    ));

    // (scheme write) library procedures
    // display and write are also exported from (scheme write)
    registry.register(PrimitiveFn::new_heap(
        "scheme.write",
        "display",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a human-readable format.",
        text_output::display,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.write",
        "write",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a machine-readable format.",
        text_output::write,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.write",
        "write-shared",
        Arity::Range(1, 2),
        "Writes obj with datum labels for all shared structures.",
        text_output::write_shared,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.write",
        "write-simple",
        Arity::Range(1, 2),
        "Writes obj without datum labels (may loop on circular structures).",
        text_output::write_simple,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "newline",
        Arity::Range(0, 1),
        "Writes a newline to the textual output port.",
        text_output::newline,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "write-char",
        Arity::Range(1, 2),
        "Writes a character to the textual output port.",
        text_output::write_char,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "write-string",
        Arity::Range(1, 4),
        "Writes a string to the textual output port.",
        text_output::write_string,
    ));

    // Text input
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "read-char",
        Arity::Range(0, 1),
        "Reads a single character from the input port.",
        text_input::read_char,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "peek-char",
        Arity::Range(0, 1),
        "Peeks at the next character without consuming it.",
        text_input::peek_char,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "char-ready?",
        Arity::Range(0, 1),
        "Returns #t if a character is ready to be read.",
        text_input::char_ready_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "read-line",
        // 0..=2: the optional second argument is chibi's max-chars
        // extension — see the note on `text_input::read_line`.
        Arity::Range(0, 2),
        "Reads a line from the input port.",
        text_input::read_line,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "read-string",
        Arity::Range(1, 2),
        "Reads k characters from the input port.",
        text_input::read_string,
    ));

    // Binary I/O
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "read-u8",
        Arity::Range(0, 1),
        "Reads a single byte from a binary input port.",
        binary::read_u8,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "peek-u8",
        Arity::Range(0, 1),
        "Peeks at the next byte without consuming it.",
        binary::peek_u8,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "u8-ready?",
        Arity::Range(0, 1),
        "Returns #t if a byte is ready to be read.",
        binary::u8_ready_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "write-u8",
        Arity::Range(1, 2),
        "Writes a byte to a binary output port.",
        binary::write_u8,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "read-bytevector",
        Arity::Range(1, 2),
        "Reads up to k bytes from a binary input port.",
        binary::read_bytevector,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "read-bytevector!",
        Arity::Range(1, 4),
        "Reads into an existing bytevector from a binary input port.",
        binary::read_bytevector_bang,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "write-bytevector",
        Arity::Range(1, 4),
        "Writes a bytevector to a binary output port.",
        binary::write_bytevector,
    ));

    // Read (S-expression parsing) - in (scheme base)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "read",
        Arity::Range(0, 1),
        "Reads a Scheme expression from the input port.",
        read::read,
    ));

    // (scheme read) library - re-exports read
    registry.register(PrimitiveFn::new_heap(
        "scheme.read",
        "read",
        Arity::Range(0, 1),
        "Reads a Scheme expression from the input port.",
        read::read,
    ));

    // File I/O operations (scheme file library)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "open-input-file",
        Arity::Exact(1),
        "Opens a file for reading and returns an input port.",
        file::open_input_file,
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "open-output-file",
        Arity::Exact(1),
        "Opens a file for writing and returns an output port.",
        file::open_output_file,
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "open-binary-input-file",
        Arity::Exact(1),
        "Opens a binary file for reading and returns an input port.",
        file::open_binary_input_file,
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "open-binary-output-file",
        Arity::Exact(1),
        "Opens a binary file for writing and returns an output port.",
        file::open_binary_output_file,
    ));

    // Flush is in scheme base
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "flush-output-port",
        Arity::Range(0, 1),
        "Flushes the output port buffer.",
        ports::flush_output_port,
    ));

    // File utilities (scheme file library)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "file-exists?",
        Arity::Exact(1),
        "Returns #t if the file exists.",
        file::file_exists_p,
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "delete-file",
        Arity::Exact(1),
        "Deletes the specified file.",
        file::delete_file,
    ));

    // Directories. Named and shaped after `(chibi filesystem)`, which is what
    // the ecosystem imports; `lib/chibi/filesystem.sld`'s `patina` branch is
    // the only consumer today.
    for (name, arity, doc, f) in [
        (
            "directory-files",
            Arity::Exact(1),
            "Returns the entry names in a directory, including \".\" and \"..\".",
            directory::directory_files
                as fn(
                    &dyn crate::apply_context::ApplyContext,
                    Vec<patina_core::TaggedValue>,
                )
                    -> Result<patina_core::TaggedValue, patina_runtime::EvalError>,
        ),
        (
            "create-directory",
            Arity::Range(1, 2),
            "Creates a directory. The optional mode is accepted and ignored.",
            directory::create_directory,
        ),
        (
            "delete-directory",
            Arity::Exact(1),
            "Removes an empty directory.",
            directory::delete_directory,
        ),
        (
            "current-directory",
            Arity::Exact(0),
            "Returns the current working directory.",
            directory::current_directory,
        ),
        (
            "change-directory",
            Arity::Exact(1),
            "Sets the current working directory.",
            directory::change_directory,
        ),
        (
            "file-directory?",
            Arity::Exact(1),
            "Returns #t if the path is a directory.",
            directory::file_directory_p,
        ),
        (
            "file-regular?",
            Arity::Exact(1),
            "Returns #t if the path is a regular file.",
            directory::file_regular_p,
        ),
    ] {
        registry.register(PrimitiveFn::new_higher_order(
            "scheme.file",
            name,
            arity,
            doc,
            f,
        ));
    }

    // Higher-order file operations (scheme file library)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "call-with-input-file",
        Arity::Exact(2),
        "Opens file for reading, calls proc with port, closes port.",
        file::call_with_input_file,
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.file",
        "call-with-output-file",
        Arity::Exact(2),
        "Opens file for writing, calls proc with port, closes port.",
        file::call_with_output_file,
    ));

    // `with-input-from-file` and `with-output-to-file` are deliberately not
    // here: they are Scheme, in `lib/scheme/file/redirect.scm`. As primitives
    // they re-entered the VM to run the thunk, and a `call/cc` out of that
    // thunk crashed it.
}

// =============================================================================
// Tests
// =============================================================================
