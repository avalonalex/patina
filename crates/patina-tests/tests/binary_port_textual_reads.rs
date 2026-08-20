//! Two port widenings the compat corpus asked for, both mirroring the
//! references rather than R7RS's minimum.
//!
//! **`read-line` takes an optional max-chars** (chibi's extension): read at
//! most n characters, still stopping early at a newline (consumed, not
//! included), leftovers staying in the port. R7RS makes the extra argument
//! "an error" — implementation freedom — and both references accept it:
//! chibi honors the limit, Gauche ignores the argument. chibi-mime reads
//! every header line through `(read-line port mime-line-length-limit)`.
//!
//! **Textual reads work on binary ports**: `read-char`, `peek-char` and
//! `read-line` decode UTF-8 from a bytevector port. R7RS calls textual I/O
//! on a binary port an error — freedom again — and both references allow
//! it. chibi-mime parses the header section of a binary message port with
//! `read-line` before switching to `read-u8` for the body; rejecting it
//! failed the suite's whole binary half.

mod common;
use common::eval_program as eval;

/// The truncation semantics, matched against chibi exactly: at most n
/// chars per call, newline still terminates (and is consumed), the
/// remainder stays in the port, EOF only when nothing is left.
#[test]
fn test_read_line_max_chars_matches_chibi() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-string \"hello world\\nsecond\"))
             (list (read-line p 4) (read-line p 4) (read-line p 4)
                   (read-line p 4) (read-line p 4) (eof-object? (read-line p 4)))"
        ),
        "(\"hell\" \"o wo\" \"rld\" \"seco\" \"nd\" #t)"
    );
}

/// A limit larger than the line changes nothing, and the plain one-argument
/// form is unaffected.
#[test]
fn test_read_line_max_chars_larger_than_line() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-string \"short\\nrest\"))
             (list (read-line p 100) (read-line p))"
        ),
        "(\"short\" \"rest\")"
    );
}

/// CRLF handling carries over from the unlimited path: a line terminated
/// within the limit drops its \r, a mid-line cut keeps every char as data.
#[test]
fn test_read_line_max_chars_crlf() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-string \"ab\\r\\ncd\"))
             (list (read-line p 10) (read-line p 10))"
        ),
        "(\"ab\" \"cd\")"
    );
}

#[test]
fn test_textual_reads_on_a_binary_port() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-bytevector (string->utf8 \"hi\\nthere\")))
             (list (read-line p) (peek-char p) (read-char p) (read-line p))"
        ),
        "(\"hi\" #\\t #\\t \"here\")"
    );
}

/// Multi-byte UTF-8 decodes correctly from the byte stream, and the
/// binary operations still see the bytes the textual ones did not consume.
#[test]
fn test_mixed_textual_and_binary_reads() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-bytevector (string->utf8 \"λx\\nrest\")))
             (list (read-char p) (read-char p) (read-char p) (read-u8 p))"
        ),
        "(#\\λ #\\x #\\newline 114)"
    );
}

/// The limited form composes with binary ports too — the exact call shape
/// chibi-mime's header reader uses.
#[test]
fn test_read_line_max_chars_on_binary_port() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-bytevector (string->utf8 \"From: a@b\\n\\nbody\")))
             (list (read-line p 4096) (read-line p 4096) (read-line p 4096))"
        ),
        "(\"From: a@b\" \"\" \"body\")"
    );
}
