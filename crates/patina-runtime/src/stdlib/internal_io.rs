//! (patina internal io) - Input/Output operations (R7RS §6.13)
//!
//! All I/O primitives including ports, reading, and writing.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal io) library
pub fn build_internal_io(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "io".to_string(),
    ];

    let primitives = [
        // === Port predicates ===
        ("port?", Arity::Exact(1)),
        ("input-port?", Arity::Exact(1)),
        ("output-port?", Arity::Exact(1)),
        ("textual-port?", Arity::Exact(1)),
        ("binary-port?", Arity::Exact(1)),
        ("input-port-open?", Arity::Exact(1)),
        ("output-port-open?", Arity::Exact(1)),
        // === Current ports ===
        // R7RS 6.13.1: parameter objects, so they take an optional value to
        // install as well as no argument to read.
        ("current-input-port", Arity::Range(0, 1)),
        ("current-output-port", Arity::Range(0, 1)),
        ("current-error-port", Arity::Range(0, 1)),
        // === Port operations ===
        ("close-port", Arity::Exact(1)),
        ("close-input-port", Arity::Exact(1)),
        ("close-output-port", Arity::Exact(1)),
        ("call-with-port", Arity::Exact(2)),
        // === String ports ===
        ("open-input-string", Arity::Exact(1)),
        ("open-output-string", Arity::Exact(0)),
        ("get-output-string", Arity::Exact(1)),
        // === Bytevector ports ===
        ("open-input-bytevector", Arity::Exact(1)),
        ("open-output-bytevector", Arity::Exact(0)),
        ("get-output-bytevector", Arity::Exact(1)),
        // === File ports ===
        ("open-input-file", Arity::Exact(1)),
        ("open-output-file", Arity::Exact(1)),
        ("open-binary-input-file", Arity::Exact(1)),
        ("open-binary-output-file", Arity::Exact(1)),
        ("call-with-input-file", Arity::Exact(2)),
        ("call-with-output-file", Arity::Exact(2)),
        ("file-exists?", Arity::Exact(1)),
        ("delete-file", Arity::Exact(1)),
        // === Directories ===
        // Not R7RS — these back `(chibi filesystem)`'s portable half, and are
        // reachable only by importing that library, not `(scheme file)`.
        ("directory-files", Arity::Exact(1)),
        ("create-directory", Arity::Range(1, 2)),
        ("delete-directory", Arity::Exact(1)),
        ("current-directory", Arity::Exact(0)),
        ("change-directory", Arity::Exact(1)),
        ("file-directory?", Arity::Exact(1)),
        ("file-regular?", Arity::Exact(1)),
        // === EOF handling ===
        ("eof-object?", Arity::Exact(1)),
        ("eof-object", Arity::Exact(0)),
        // === Text input ===
        ("read", Arity::Range(0, 1)),
        ("read-char", Arity::Range(0, 1)),
        ("peek-char", Arity::Range(0, 1)),
        ("read-line", Arity::Range(0, 1)),
        ("read-string", Arity::Range(1, 2)),
        ("char-ready?", Arity::Range(0, 1)),
        // === Text output ===
        ("write", Arity::Range(1, 2)),
        ("write-shared", Arity::Range(1, 2)),
        ("write-simple", Arity::Range(1, 2)),
        ("display", Arity::Range(1, 2)),
        ("newline", Arity::Range(0, 1)),
        ("write-char", Arity::Range(1, 2)),
        ("write-string", Arity::Range(1, 4)),
        ("flush-output-port", Arity::Range(0, 1)),
        // === Binary input ===
        ("read-u8", Arity::Range(0, 1)),
        ("peek-u8", Arity::Range(0, 1)),
        ("u8-ready?", Arity::Range(0, 1)),
        ("read-bytevector", Arity::Range(1, 2)),
        ("read-bytevector!", Arity::Range(1, 4)),
        // === Binary output ===
        ("write-u8", Arity::Range(1, 2)),
        ("write-bytevector", Arity::Range(1, 4)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }

    exports
}
