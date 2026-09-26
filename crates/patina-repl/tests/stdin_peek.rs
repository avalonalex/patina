//! #416: stdin peeks decode one whole character and leave it for the next read.
//! Real stdin is process-wide, so these run Scheme fixtures in child processes
//! on both backends. Redirected files make the 8 KiB boundary reproducible;
//! pipes also exercise the same operations with source-dependent read sizes.

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
