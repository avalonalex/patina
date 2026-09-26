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
//! At the foot are binary-port rows that cannot be suite rows either,
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
    fn new(name: &str, content: impl AsRef<[u8]>) -> Self {
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

/// #411: byte and character operations must share the position left by
/// `read`. File rows live here because the Scheme suites cannot create files.
/// Chibi 0.12 and Gauche 0.9.15 agree on these results (2026-09-26).
#[test]
fn test_file_read_leaves_the_remainder_for_every_byte_operation() {
    let file = TempFile::new("read_then_bytes", "x λyz\nrest");
    let code = format!(
        r#"
        (import (scheme base) (scheme file) (scheme read))
        (define p (open-binary-input-file "{}"))
        (define target (bytevector 0 0 0 0))
        (let* ((datum (read p))
               (ready (u8-ready? p))
               (peek1 (peek-u8 p)) (peek2 (peek-u8 p))
               (empty (read-bytevector 0 p))
               (space (read-u8 p))
               (lead (read-bytevector 1 p))
               (count (read-bytevector! target p 1 3))
               (char-peek (peek-char p)) (char (read-char p))
               (line (read-line p)) (next (read p))
               (end (eof-object? (read-u8 p))))
          (close-port p)
          (list datum ready peek1 peek2 empty space lead count target
                char-peek char line next end))
        "#,
        file.path()
    );
    assert_program_eval_to(
        &code,
        r#"(x #t 32 32 #u8() 32 #u8(206) 2 #u8(0 187 121 0) #\z #\z "" rest #t)"#,
    );

    // The remainder is still ready even if the underlying file reached EOF.
    let file = TempFile::new("read_then_bytes_at_eof", "x ");
    let code = format!(
        r#"
        (import (scheme base) (scheme file) (scheme read))
        (define p (open-binary-input-file "{}"))
        (let* ((datum (read p)) (ready (u8-ready? p))
               (bytes (read-bytevector 1 p)) (end (eof-object? (read p))))
          (close-port p) (list datum ready bytes end))
        "#,
        file.path()
    );
    assert_program_eval_to(&code, "(x #t #u8(32) #t)");
}

/// A finished datum does not require the rest of its line to be UTF-8.
/// Chibi 0.12 and Gauche 0.9.15 agree on these results (2026-09-26).
#[test]
fn test_file_read_leaves_undecodable_bytes_after_a_finished_datum() {
    for (i, (input, remaining, expected)) in [
        (
            &b"x \xff\nrest"[..],
            7,
            "(x 32 #u8(32 255 10 114 101 115 116) #t)",
        ),
        (&b"(1)\xff"[..], 1, "((1) 255 #u8(255) #t)"),
        (&b"\"a\"\xff"[..], 1, "(\"a\" 255 #u8(255) #t)"),
        (&b"|a|\xff"[..], 1, "(a 255 #u8(255) #t)"),
    ]
    .into_iter()
    .enumerate()
    {
        let file = TempFile::new(&format!("read_before_binary_{i}"), input);
        let code = format!(
            r#"
            (import (scheme base) (scheme file) (scheme read))
            (define p (open-binary-input-file "{}"))
            (let* ((datum (read p)) (peek (peek-u8 p))
                   (bytes (read-bytevector {remaining} p)) (end (eof-object? (read p))))
              (close-port p) (list datum peek bytes end))
            "#,
            file.path()
        );
        assert_program_eval_to(&code, expected);
    }
}

/// UTF-8 and tokens can both straddle the native file buffer's 8 KiB edge.
/// A large multiline datum still feeds the incremental reader only once.
/// Chibi 0.12 and Gauche 0.9.15 agree on these results (2026-09-26).
#[test]
fn test_file_read_preserves_bytes_across_buffer_boundaries() {
    for (i, prefix) in [
        // A symbol ends exactly at the boundary; its delimiter is next.
        format!("{}x", " ".repeat(8191)),
        // A two-byte character crosses that boundary inside the datum.
        format!("{}λ", " ".repeat(8191)),
        // A four-byte character crosses it inside a string.
        format!("\"{}🦀\"", "a".repeat(8190)),
        // Comments and newlines all count towards the consumed offset.
        format!("#; (ignored) ({}λ)", "1\n".repeat(10000)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.extend_from_slice(b" \xffz");
        let file = TempFile::new(&format!("read_boundary_{i}"), bytes);
        let quoted = format!("\"{}\"", prefix.replace('\\', "\\\\").replace('"', "\\\""));
        let code = format!(
            r#"
            (import (scheme base) (scheme file) (scheme read))
            (define p (open-binary-input-file "{}"))
            (define reference (open-input-string {quoted}))
            (let* ((same (equal? (read p) (read reference)))
                   (space (read-u8 p)) (peek (peek-u8 p))
                   (bytes (read-bytevector 2 p)))
              (close-port p) (list same space peek bytes))
            "#,
            file.path()
        );
        assert_program_eval_to(&code, "(#t 32 255 #u8(255 122))");
    }
}

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
// Binary ports: where the text stops
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
        let file = TempFile::new(
            "invalid_utf8",
            bytes
                .split_whitespace()
                .map(|b| b.parse::<u8>().unwrap())
                .collect::<Vec<_>>(),
        );
        for open in [
            format!("(open-input-bytevector (bytevector {bytes}))"),
            format!("(open-binary-input-file \"{}\")", file.path()),
        ] {
            let code = format!(
                r#"
            (import (scheme base) (scheme file) (scheme read))
            (define (mentions? text word)
              (let ((n (string-length text)) (m (string-length word)))
                (let loop ((i 0))
                  (and (<= (+ i m) n)
                       (or (string=? (substring text i (+ i m)) word)
                           (loop (+ i 1)))))))
            (define p {open})
            (guard (e (#t (list (read-error? e)
                                (mentions? (error-object-message e) "UTF-8"))))
              (read p))
            "#
            );
            assert_program_eval_to(&code, "(#t #t)");
        }
    }
}

/// An error that is there whatever follows it is still reported as itself.
#[test]
fn test_binary_port_syntax_error_before_undecodable_bytes_is_not_blamed_on_them() {
    let file = TempFile::new("syntax_before_invalid_utf8", [41, 32, 255]);
    for open in [
        "(open-input-bytevector (bytevector 41 32 255))".to_string(),
        format!("(open-binary-input-file \"{}\")", file.path()),
    ] {
        let code = format!(
            r#"
        (import (scheme base) (scheme file) (scheme read))
        (define (mentions? text word)
          (let ((n (string-length text)) (m (string-length word)))
            (let loop ((i 0))
              (and (<= (+ i m) n)
                   (or (string=? (substring text i (+ i m)) word)
                       (loop (+ i 1)))))))
        (define p {open})   ; a stray )
        (guard (e (#t (list (read-error? e)
                            (mentions? (error-object-message e) "UTF-8"))))
          (read p))
        "#
        );
        assert_program_eval_to(&code, "(#t #f)");
    }
}
