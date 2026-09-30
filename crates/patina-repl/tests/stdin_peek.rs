//! #416: stdin peeks decode one whole character and leave it for the next read.
//! Real stdin is process-wide, so these run Scheme fixtures in child processes
//! on both backends. Redirected files make the 8 KiB boundary reproducible;
//! pipes also exercise the same operations with source-dependent read sizes.
//! #412 extends that shared position to byte operations and datum lookahead.

mod common;

use common::{BOTH_BACKENDS, run_with_deadline_bytes, spawn_patina_with_stdin};
use std::fs;

fn check_input(program: &str, input: &[u8], expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("peek.scm"), program).unwrap();
    let input_path = dir.path().join("input.bin");
    fs::write(&input_path, input).unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend_from_slice(&["--isolated-libraries", "peek.scm"]);
        for redirected in [true, false] {
            let (stdout, stderr, status) = if redirected {
                let file = fs::File::open(&input_path).unwrap();
                spawn_patina_with_stdin(dir.path(), &args, file.into()).finish_with_status()
            } else {
                run_with_deadline_bytes(dir.path(), &args, input)
            };
            assert!(
                status.success(),
                "{backend:?}, redirected={redirected}: {status}\n{stderr}"
            );
            assert!(
                stdout == expected,
                "{backend:?}, redirected={redirected}: expected {} bytes, got {}; output begins {:?}\n{stderr}",
                expected.len(),
                stdout.len(),
                stdout.chars().take(100).collect::<String>()
            );
        }
    }
}

#[test]
fn stdin_reads_keep_directives_and_exclude_unread_lookahead() {
    let program = r#"(import (scheme base) (scheme read) (scheme write))
      (let* ((a (read)) (b (read)) (c (read)) (d (read))) (write (list a b c d)))"#;
    for input in [
        "#!fold-case ABC DEF #!no-fold-case GHI JKL",
        "#!fold-case\nABC\nDEF\n#!no-fold-case\nGHI\nJKL",
    ] {
        check_input(program, input.as_bytes(), "(abc def GHI JKL)");
    }
    let program = r#"(import (scheme base) (scheme read) (scheme write))
      (let* ((a (read)) (line (read-line)) (b (read)) (c (read))) (write (list a b c)))"#;
    for (input, expected) in [
        ("#!fold-case A #!no-fold-case\nB C", "(a b c)"),
        ("A #!fold-case\nB C", "(A B C)"),
    ] {
        check_input(program, input.as_bytes(), expected);
    }
}

#[test]
fn stdin_peeks_preserve_characters_across_utf8_boundaries() {
    for ch in ['λ', '€', '𐀀'] {
        for split in 1..ch.len_utf8() {
            let input = format!("{}{ch}z", "a".repeat(8192 - split));
            check_input(
                include_str!("fixtures/stdin-peek-characters.scm"),
                input.as_bytes(),
                &input,
            );
        }
    }
}

#[test]
fn stdin_peek_defers_invalid_utf8_until_that_character_is_reached() {
    for input in [
        b"x \xff".as_slice(),
        b"x \xce",
        b"x \xe2\x82",
        b"x \xf0\x90\x80",
    ] {
        check_input(
            include_str!("fixtures/stdin-peek-characters.scm"),
            input,
            "x <error>",
        );
    }
}

#[test]
fn stdin_peek_handles_empty_input() {
    check_input(include_str!("fixtures/stdin-peek-characters.scm"), b"", "");
}

#[test]
fn stdin_peek_leaves_text_for_string_line_and_datum_reads() {
    let input = format!("{}λab€cd\n(1 2)z", "x".repeat(8191));
    check_input(
        include_str!("fixtures/stdin-peek-mixed-reads.scm"),
        input.as_bytes(),
        "(#\\λ \"λab\" #\\€ \"€cd\" #\\( (1 2) #\\z #\\z #t)\n",
    );
}

#[test]
fn stdin_datum_character_and_byte_reads_share_one_position() {
    check_input(
        include_str!("fixtures/stdin-mixed-byte-reads.scm"),
        "x λyz\nrest".as_bytes(),
        "(x #t 32 32 #u8() 32 #u8(206) 2 #u8(0 187 121 0) #\\z #\\z \"\" rest #t)\n",
    );
}

#[test]
fn stdin_datum_read_leaves_undecodable_bytes_for_byte_reads() {
    let program = r#"
        (import (scheme base) (scheme read) (scheme write))
        (let* ((datum (read)) (peek (peek-u8)) (bytes (read-bytevector 99))
               (end (eof-object? (read))))
          (write (list datum peek bytes end)))
        "#;
    for (input, expected) in [
        (
            b"x \xff\nrest".as_slice(),
            "(x 32 #u8(32 255 10 114 101 115 116) #t)",
        ),
        (b"(1)\xff", "((1) 255 #u8(255) #t)"),
        (b"\"a\"\xff", "(\"a\" 255 #u8(255) #t)"),
        (b"|a|\xff", "(a 255 #u8(255) #t)"),
    ] {
        check_input(program, input, expected);
    }
}

#[test]
fn stdin_peek_across_a_buffer_boundary_leaves_bytes_for_binary_reads() {
    check_input(
        r#"
        (import (scheme base) (scheme write))
        (do ((i 0 (+ i 1))) ((= i 8191)) (read-char))
        (define target (bytevector 0 0))
        (let* ((ch (peek-char)) (peek (peek-u8)) (lead (read-bytevector 1))
               (count (read-bytevector! target)) (next (read-char))
               (end (eof-object? (read-u8))))
          (write (list (char->integer ch) peek lead count target next end)))
        "#,
        format!("{}λzw", "a".repeat(8191)).as_bytes(),
        "(955 206 #u8(206) 2 #u8(187 122) #\\w #t)",
    );
}

#[test]
fn standard_output_and_error_accept_mixed_byte_and_text_writes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("output.scm"),
        r#"
        (import (scheme base) (scheme write))
        (write-u8 65)
        (write-char #\λ)
        (write-bytevector (bytevector 66))
        (write-u8 67 (current-error-port))
        (write-bytevector (bytevector 68) (current-error-port))
        (close-port (current-error-port))
        (write (binary-port? (current-error-port)))
    "#,
    )
    .unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("output.scm");
        let (stdout, stderr, status) = run_with_deadline_bytes(dir.path(), &args, b"");
        assert!(status.success(), "{backend:?}: {stderr}");
        assert_eq!(stdout, "AλB#t", "{backend:?}");
        assert_eq!(stderr, "CD", "{backend:?}");
    }
}
