//! A character above ASCII starts an identifier.
//!
//! R7RS 7.1.1 builds <identifier> from ASCII letters, but 7.1 lets an
//! implementation extend the grammar and does not say how far. R6RS 4.2.4
//! spells out a limit by Unicode general category; chibi, Gauche and Chez all
//! go past it and read *any* character above ASCII — all three accept `“` as
//! an identifier. Patina matches them, which needs no category tables and can
//! only widen the accepted language, since every token this admits is one that
//! used to be a lex error.
//!
//! Found via SRFI 197, whose reference implementation uses `…₁` as its custom
//! `syntax-rules` ellipsis so its templates can emit a literal `...`.

mod common;
use common::*;

#[test]
fn test_characters_beyond_ascii_are_identifiers() {
    // `λ` and `café` already worked — `is_alphabetic` is Unicode-aware — and
    // stay here so a future narrowing cannot quietly take letters with it.
    assert_program_eval_to("(define λ 1) λ", "1");
    assert_program_eval_to("(define café 2) café", "2");
    // These are what the change added: category Po, Sm and So.
    assert_program_eval_to("(define … 3) …", "3");
    assert_program_eval_to("(define → 4) →", "4");
    assert_program_eval_to("(define ± 5) ±", "5");
}

/// The case that separates the rule we chose from the one we did not.
///
/// R6RS 4.2.4 defines identifier constituents by Unicode general category,
/// and every other character in this file is inside that set — so a narrowing
/// to the R6RS rule would leave the rest of these tests passing. `“` is
/// category Pi, which R6RS excludes and chibi, Gauche and Chez all accept.
/// It is the reason the rule is "anything above ASCII" rather than a category
/// list, so it is the one that has to be asserted.
#[test]
fn test_a_character_outside_the_r6rs_categories_is_still_an_identifier() {
    assert_program_eval_to("(define “ 1) “", "1");
}

/// The shape SRFI 197 needs: a subscript digit, which is Unicode category No.
/// `char::is_numeric` is Unicode-aware, so before this the number dispatch
/// claimed it and reported `Invalid number: ₁` — it never reached the
/// identifier path at all.
#[test]
fn test_a_non_ascii_numeric_is_an_identifier_not_a_number() {
    assert_program_eval_to("(define ₁ 1) ₁", "1");
    assert_program_eval_to("(define …₁ 42) …₁", "42");
    assert_eval_to("(symbol? '₁)", "#t");
    assert_eval_to("(symbol->string '…₁)", "\"…₁\"");
}

/// SRFI 197's actual use: a custom ellipsis, so the template can emit `...`
/// as a literal rather than as a repetition marker.
#[test]
fn test_a_unicode_identifier_works_as_a_custom_ellipsis() {
    assert_program_eval_to(
        r#"
        (define-syntax my-list
          (syntax-rules …₁ ()
            ((_ x …₁) (list x …₁))))
        (my-list 1 2 3)
        "#,
        "(1 2 3)",
    );
}

/// The three ASCII forms that go through the predicates this change narrowed:
/// the dispatch itself, `peek_is_numeric` and `peek_is_decimal_start`.
///
/// Deliberately not a tour of number syntax — rationals, complexes, radix
/// prefixes and exponents reach `read_number` through paths the change never
/// touched, and are covered in `compliance/`.
#[test]
fn test_ascii_number_literals_still_lex_as_numbers() {
    assert_eval_to("42", "42");
    assert_eval_to(".3", "0.3");
    assert_eval_to("-.25", "-0.25");
}

/// The writer answers a different question — "would every R7RS reader take
/// this bare?" — so it stays strict and bars what it cannot vouch for.
/// Reading our own output back must still round-trip.
#[test]
fn test_the_writer_stays_strict_and_round_trips() {
    assert_eval_to("'…₁", "|…₁|");
    assert_eval_to("'→", "|→|");
    // `display` is unaffected — only `write` has to be re-readable.
    assert_program_eval_to("(import (scheme write)) (display '→) 'done", "done");
    assert_eval_to("(eq? '…₁ (string->symbol \"…₁\"))", "#t");
}

/// Whitespace is the one thing above ASCII that is *not* an identifier
/// character, so a stray non-breaking space cannot silently weld two
/// identifiers into one. Patina rejects it outright, which is stricter than
/// every reference: chibi welds `a<U+00A0>b` into the symbol `|a b|`, while
/// Gauche and Chez both split it and evaluate `3`.
#[test]
fn test_a_non_breaking_space_is_not_an_identifier_character() {
    assert_program_eval_error("(define a 1) (define b 2) (+ a\u{00A0}b)");
}
