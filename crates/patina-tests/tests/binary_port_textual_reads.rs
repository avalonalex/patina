//! Two port widenings the compat corpus asked for: `read-line`'s optional
//! max-chars argument (why: the note on `text_input::read_line`) and
//! textual reads decoding UTF-8 from binary ports (why: `decode_utf8_at`
//! in `port.rs`). Both mirror the references; chibi-mime exercises both.

mod common;
use common::eval_program as eval;

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

/// The limit counts characters, not bytes — chibi's unit. Written with
/// two-byte λs so a byte-counting regression truncates visibly early.
#[test]
fn test_read_line_max_chars_counts_chars_not_bytes() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-string \"λλλλλ\"))
             (list (read-line p 4) (read-line p 4))"
        ),
        "(\"λλλλ\" \"λ\")"
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

/// CRLF handling: \r is a line ending (R7RS 7.1.1), and return+newline is
/// one ending, so a limit that lands on the \r still ends the line there
/// and the \n goes with it — the next read sees "cd", as in chibi. (This
/// used to pin the older behaviour, where only \n terminated and a cut
/// between the two left the \r as data and an empty line after it.)
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
    assert_eq!(
        eval(
            "(import (scheme base))
             (define p (open-input-string \"ab\\r\\ncd\"))
             (list (read-line p 3) (read-line p 3) (eof-object? (read-line p 3)))"
        ),
        "(\"ab\" \"cd\" #t)"
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
