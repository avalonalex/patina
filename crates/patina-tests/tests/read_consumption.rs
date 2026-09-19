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
//!
//! At the foot are the bytevector-port rows that cannot be suite rows either,
//! for a different reason: they are about bytes that do not decode, where no
//! oracle can corroborate.

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
fn test_file_port_incomplete_datum_is_a_read_error() {
    for (i, input) in [
        "(",
        "(\n1\n",
        "#(",
        "#u8(",
        "'",
        "`",
        ",",
        ",@",
        "(1 .",
        "#1=",
        "#;",
        "#; (",
        "\"unfinished",
        "|unfinished",
        "#| unfinished",
    ]
    .iter()
    .enumerate()
    {
        let f = TempFile::new(&format!("incomplete_{i}"), input);
        let code = format!(
            r#"
            (import (scheme base) (scheme file) (scheme read))
            (define p (open-input-file "{path}"))
            (define result
              (guard (e (else (and (error-object? e) (read-error? e))))
                (read p)
                #f))
            (close-input-port p)
            result
            "#,
            path = f.path()
        );
        assert_program_eval_to(&code, "#t");
    }
}

#[test]
fn test_file_port_clean_eof_after_comments() {
    for (i, input) in [
        "",
        " \n",
        "; comment",
        "#| comment |#",
        "#; (1 2)",
        "#; #; 1 2",
    ]
    .iter()
    .enumerate()
    {
        let f = TempFile::new(&format!("clean_eof_{i}"), input);
        let code = format!(
            r#"
            (import (scheme base) (scheme file) (scheme read))
            (define p (open-input-file "{path}"))
            (define result (and (eof-object? (read p))
                                (eof-object? (read p))
                                (eof-object? (read-char p))))
            (close-input-port p)
            result
            "#,
            path = f.path()
        );
        assert_program_eval_to(&code, "#t");
    }
}

#[test]
fn test_file_port_complete_datum_before_incomplete_datum() {
    let f = TempFile::new("complete_then_incomplete", "1 (");
    let code = format!(
        r#"
        (import (scheme base) (scheme file) (scheme read))
        (define p (open-input-file "{path}"))
        (define a (read p))
        (define b (guard (e (else (read-error? e))) (read p) #f))
        (close-input-port p)
        (list a b)
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(1 #t)");
}

#[test]
fn test_file_port_multiple_datums_one_line() {
    let f = TempFile::new("datums_one_line", "5 40 102334155\n");
    let code = format!(
        r#"
        (import (scheme base) (scheme file) (scheme read))
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
        (import (scheme base) (scheme file) (scheme read))
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
        (import (scheme base) (scheme file) (scheme read))
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
        (import (scheme base) (scheme file) (scheme read))
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
        (import (scheme base) (scheme file) (scheme read))
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

// =============================================================================
// Datums that span lines, and ones the file cuts short (#329)
// =============================================================================

/// A string literal, a block comment, and a `|…|` identifier may all run past
/// the end of a line. The line-buffered reader must keep taking lines for
/// them, exactly as it does for an unclosed list: treating the lexer's
/// unterminated-construct errors as failures made every such datum unreadable
/// from a file port, while the same text read fine from a string port.
#[test]
fn test_read_takes_more_lines_for_a_datum_that_spans_them() {
    for (name, content, expected) in [
        (
            "multiline_string",
            "(a \"one\ntwo\")\n(b)\n",
            // The newline survives the round trip as an escape, so this
            // expectation is the two characters `\` and `n`, not a newline.
            r#"((a "one\ntwo") (b) #t)"#,
        ),
        (
            "multiline_block_comment",
            "(a #| one\ntwo |# b)\n(c)\n",
            "((a b) (c) #t)",
        ),
        (
            "multiline_bar_identifier",
            "(a |one\ntwo|)\n(c)\n",
            "((a |one\ntwo|) (c) #t)",
        ),
    ] {
        let file = TempFile::new(name, content);
        let code = format!(
            r#"
            (import (scheme base) (scheme file) (scheme read))
            (define p (open-input-file "{}"))
            (list (read p) (read p) (eof-object? (read p)))
            "#,
            file.path()
        );
        assert_program_eval_to(&code, expected);
    }
}

/// The same reader must still report a file that ends inside a datum, rather
/// than waiting for a line that never comes or returning it as an EOF object.
#[test]
fn test_read_reports_a_file_that_ends_inside_a_datum() {
    let file = TempFile::new("cut_short", "(a b\n");
    let code = format!(
        r#"
        (import (scheme base) (scheme file) (scheme read))
        (define p (open-input-file "{}"))
        (guard (e ((read-error? e) 'read-error) (#t 'other)) (read p))
        "#,
        file.path()
    );
    assert_program_eval_to(&code, "read-error");
}

/// What `read` raises must not depend on where the file lives. The VM asked
/// "does the message mention a file?" before "is this a read error?", so a
/// path with `file` anywhere in it turned a read error into a file error on
/// that backend only.
#[test]
fn test_a_read_error_is_one_whatever_the_path_is_called() {
    for name in ["plain_cut", "profile_cut"] {
        let file = TempFile::new(name, "(a b\n");
        let code = format!(
            r#"
            (import (scheme base) (scheme file) (scheme read))
            (define p (open-input-file "{}"))
            (guard (e ((read-error? e) 'read-error) ((file-error? e) 'file-error) (#t 'other))
              (read p))
            "#,
            file.path()
        );
        assert_program_eval_to(&code, "read-error");
    }
}

/// A byte order mark is not program text, and the lexer drops it — but the
/// offsets it reports still have to land in the caller's own buffer, which
/// still has it. One character short left the port re-reading the last
/// character of the datum it had just returned.
#[test]
fn test_a_byte_order_mark_does_not_shift_what_read_consumes() {
    let file = TempFile::new("byte_order_mark", "\u{feff}(a) (b) 42\n");
    let code = format!(
        r#"
        (import (scheme base) (scheme file) (scheme read))
        (define p (open-input-file "{}"))
        (list (read p) (read p) (read p) (eof-object? (read p)))
        "#,
        file.path()
    );
    assert_program_eval_to(&code, "((a) (b) 42 #t)");
}

// =============================================================================
// Bytevector ports: where the text stops
// =============================================================================
//
// `read` on a binary port parses the decodable text at its front (#404). The
// rows where a delimiter ends the datum are suite rows, in
// `tests/scheme/stdlib/ports.scm`. These are the rows where the datum, or the
// search for one, reaches bytes that are not UTF-8. chibi reads such a byte
// into the token raw and Gauche reads it as Latin-1, so neither can vouch for
// an answer here, and a suite row would only be two register entries.

/// A closer ends its datum whatever follows it, so a header may sit right
/// against a binary body, and the body is still there for `read-u8`.
#[test]
fn test_binary_port_datum_ended_by_a_closer_leaves_the_undecodable_bytes() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (scheme read))
        (define (datum-then-byte . bytes)
          (let* ((p (open-input-bytevector (apply bytevector bytes)))
                 (datum (read p))
                 (byte (read-u8 p)))
            (list datum byte)))
        (list (datum-then-byte 40 49 41 255)     ; (1)
              (datum-then-byte 34 97 34 255)     ; "a"
              (datum-then-byte 124 97 124 255)   ; |a|
              (datum-then-byte 120 32 255))      ; x, and then the space
        "#,
        r#"(((1) 255) ("a" 255) (a 255) (x 32))"#,
    );
}

/// Anything else that reaches the undecodable bytes is reported as that, and
/// as a read error on both backends. Not as the datum so far — `x` for
/// `x\xFF` invents a delimiter, where chibi and Gauche read the byte into the
/// token — not as an end of input, which the port has not reached, and not as
/// an end of file.
#[test]
fn test_binary_port_datum_that_runs_into_undecodable_bytes_is_a_read_error() {
    for bytes in [
        "255",                      // nothing else
        "32 255",                   // whitespace first
        "59 255 10 120",            // inside a line comment
        "120 255",                  // x
        "39 120 255",               // 'x
        "39 255",                   // '
        "40 49 32 255",             // (1
        "34 97 255 34",             // inside a string
        "35 124 255 124 35 32 120", // inside a block comment
        "120 206",                  // x, then half of a two-byte character
    ] {
        let code = format!(
            r#"
            (import (scheme base) (scheme read))
            (define (mentions? text word)
              (let ((n (string-length text)) (m (string-length word)))
                (let loop ((i 0))
                  (and (<= (+ i m) n)
                       (or (string=? (substring text i (+ i m)) word)
                           (loop (+ i 1)))))))
            (define p (open-input-bytevector (bytevector {bytes})))
            (guard (e (#t (list (read-error? e)
                                (mentions? (error-object-message e) "UTF-8"))))
              (read p))
            "#
        );
        assert_program_eval_to(&code, "(#t #t)");
    }
}

/// An error that is there whatever follows it is still reported as itself.
#[test]
fn test_binary_port_syntax_error_before_undecodable_bytes_is_not_blamed_on_them() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (scheme read))
        (define (mentions? text word)
          (let ((n (string-length text)) (m (string-length word)))
            (let loop ((i 0))
              (and (<= (+ i m) n)
                   (or (string=? (substring text i (+ i m)) word)
                       (loop (+ i 1)))))))
        (define p (open-input-bytevector (bytevector 41 32 255)))   ; a stray )
        (guard (e (#t (list (read-error? e)
                            (mentions? (error-object-message e) "UTF-8"))))
          (read p))
        "#,
        "(#t #f)",
    );
}
