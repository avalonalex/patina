//! Tests that `read` consumes exactly one datum and preserves the rest of
//! the input for subsequent operations on the same port.
//!
//! Regression tests for a bug where `read` on file ports (and stdin)
//! accumulated whole lines into a local buffer, parsed one datum, and
//! discarded the remainder — so a second `read` on "5 40" returned EOF
//! instead of 40.
//!
//! Only the file-port rows are here, because a suite file cannot make a file.
//! The string-port rows, the control showing the rule is the port's and not
//! the file's, moved to `tests/scheme/stdlib/ports.scm` (#193).

mod common;

use common::assert_program_eval_to;

/// Helper: create a temp file path that won't collide across tests.
fn temp_path(name: &str) -> String {
    let dir = std::env::temp_dir();
    dir.join(format!("patina_read_consumption_{}", name))
        .to_str()
        .unwrap()
        .to_string()
}

/// Helper: ensure a temp file is cleaned up after test.
struct TempFile(String);

impl TempFile {
    fn new(name: &str, content: &str) -> Self {
        let path = temp_path(name);
        std::fs::write(&path, content).unwrap();
        TempFile(path)
    }
    fn path(&self) -> &str {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// =============================================================================
// File ports
// =============================================================================

#[test]
fn test_file_port_multiple_datums_one_line() {
    let f = TempFile::new("datums_one_line", "5 40 102334155\n");
    let code = format!(
        r#"
        (import (scheme file))
        (define p (open-input-file "{path}"))
        (let* ((a (read p))
               (b (read p))
               (c (read p))
               (d (read p)))
          (close-input-port p)
          (list a b c (eof-object? d)))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(5 40 102334155 #t)");
}

#[test]
fn test_file_port_datums_across_lines() {
    let f = TempFile::new("datums_across_lines", "1\n(2\n 3)\n4 5\n");
    let code = format!(
        r#"
        (import (scheme file))
        (define p (open-input-file "{path}"))
        (let* ((a (read p))
               (b (read p))
               (c (read p))
               (d (read p))
               (e (read p)))
          (close-input-port p)
          (list a b c d (eof-object? e)))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(1 (2 3) 4 5 #t)");
}

#[test]
fn test_file_port_read_then_read_line() {
    // The rest of the line after the datum must be visible to read-line
    let f = TempFile::new("read_then_read_line", "7 rest of line\nnext\n");
    let code = format!(
        r#"
        (import (scheme file))
        (define p (open-input-file "{path}"))
        (let* ((a (read p))
               (l1 (read-line p))
               (l2 (read-line p)))
          (close-input-port p)
          (list a l1 l2))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(7 \" rest of line\" \"next\")");
}

#[test]
fn test_file_port_read_then_read_char() {
    let f = TempFile::new("read_then_read_char", "7 x\n");
    let code = format!(
        r#"
        (import (scheme file))
        (define p (open-input-file "{path}"))
        (let* ((a (read p))
               (c1 (read-char p))
               (c2 (read-char p)))
          (close-input-port p)
          (list a c1 c2))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(7 #\\space #\\x)");
}

#[test]
fn test_file_port_datum_without_trailing_newline() {
    let f = TempFile::new("no_trailing_newline", "1 2");
    let code = format!(
        r#"
        (import (scheme file))
        (define p (open-input-file "{path}"))
        (let* ((a (read p))
               (b (read p))
               (c (read p)))
          (close-input-port p)
          (list a b (eof-object? c)))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(1 2 #t)");
}
