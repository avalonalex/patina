//! A leading `@` starts an identifier.
//!
//! R7RS 7.1.1 makes `@` a <special subsequent> and not an <initial>, so a bare
//! `@` is not a conforming identifier. Patina reads it anyway: no conforming
//! program can contain a bare `@` token, so accepting it only widens the
//! accepted language, and the third-party ecosystem depends on it (SXML's `@`
//! attribute marker and `@raw` tag, `(chibi match)`'s `@` record pattern).
//! Chez, Gauche and chibi all read it; Chez and Gauche still *write* it
//! escaped, as we do.

mod common;
use common::*;

#[test]
fn test_bare_at_is_a_symbol() {
    assert_eval_to("'@", "|@|");
    assert_eval_to("(symbol? '@)", "#t");
    assert_eval_to("(eq? '@ (string->symbol \"@\"))", "#t");
}

#[test]
fn test_at_prefixed_identifier() {
    // SXML's `@raw` tag.
    assert_eval_to("(symbol->string '@raw)", "\"@raw\"");
    assert_eval_to("(eq? '@ '@raw)", "#f");
}

#[test]
fn test_at_inside_a_datum() {
    // The shape SXML actually uses: an attribute list tagged with `@`.
    assert_eval_to("(car (cadr '(elem (@ (href \"x\")) \"text\")))", "|@|");
}

#[test]
fn test_at_is_usable_as_a_variable() {
    assert_program_eval_to("(define (@ x) (* x 2)) (@ 21)", "42");
    assert_eval_to("(let ((@ 5)) (+ @ 1))", "6");
}

#[test]
fn test_at_as_a_syntax_rules_literal() {
    // How `(chibi match)` writes its named-record-field pattern.
    assert_program_eval_to(
        "(define-syntax m (syntax-rules (@) ((_ (@ x)) (list 'at x)) ((_ (y x)) (list 'other x))))
         (list (m (@ 1)) (m (z 2)))",
        "((at 1) (other 2))",
    );
}

#[test]
fn test_at_and_unquote_splicing_coexist() {
    // `,@` is lexed before identifiers, so the two never compete for the same
    // `@`. That `,@` alone still splices is a lexer-level fact, pinned by
    // `test_unquote_splicing_still_wins_over_at_identifier`; what needs saying
    // here is that both spellings survive in one form.
    assert_eval_to("`(@ ,@(list 2 3))", "(|@| 2 3)");
}

#[test]
fn test_at_symbol_round_trips_through_write() {
    // The invariant behind reading `@` but writing `|@|`: our own writer's
    // output must read back as the same symbol. Assert the written form
    // literally, so narrowing `symbol_needs_vertical_bars` cannot pass this
    // test while silently breaking the round trip.
    assert_program_eval_to(
        "(define p (open-output-string))
         (write '@ p)
         (define written (get-output-string p))
         (list written (eq? (read (open-input-string written)) '@))",
        "(\"|@|\" #t)",
    );
}
