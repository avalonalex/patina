//! Reading R6RS source.
//!
//! Patina is an R7RS implementation and stays one; what these tests pin is the
//! narrow set of R6RS *surface syntax* it accepts so that R6RS libraries and
//! programs can be read at all. Each case is syntax R7RS 7.1.1 reserves, so
//! accepting it widens the accepted language without changing the meaning of
//! any conforming R7RS program — the same trade recorded for the bare `@`
//! token.
//!
//! Every helper here runs both backends.

mod common;

use common::{assert_eval_to, assert_program_eval_error, assert_program_eval_to};

// =============================================================================
// Square brackets
// =============================================================================

#[test]
fn brackets_stand_in_for_parentheses() {
    assert_eval_to("(let ([x 1] [y 2]) (+ x y))", "3");
    assert_eval_to("(cond [(= 1 2) 'no] [else 'yes])", "yes");
    assert_eval_to("(let loop ([i 0]) (if (= i 3) i (loop (+ i 1))))", "3");
}

#[test]
fn brackets_delimit_the_token_before_them() {
    // `[x 1]` must end the `1` at the bracket rather than reading `1]` as a
    // malformed number — the reason the lexer's delimiter set had to widen.
    assert_eval_to("(let ([x 1])x)", "1");
    assert_eval_to("(list 'a[quote b])", "(a b)");
}

#[test]
fn a_bracket_may_close_only_a_bracket() {
    // Accepting brackets without checking their shape would let a typo through
    // silently. Gauche, Chez, Racket and Guile all reject these.
    assert_program_eval_error("(let ([x 1)]) x)");
    assert_program_eval_error("(let ((x 1]) x)");
}

#[test]
fn brackets_are_not_identifier_characters() {
    // The widening splits `a[b]` into three tokens where it used to be one
    // symbol. Nothing conforming is lost — a bracket is neither an `<initial>`
    // nor a `<subsequent>` — and the symbol is still writable with bars.
    assert_eval_to("(symbol->string '|a[b]|)", "\"a[b]\"");
}

#[test]
fn bracket_character_literals_are_unaffected() {
    assert_eval_to(r"(char->integer #\[)", "91");
    assert_eval_to(r"(char->integer #\])", "93");
}

#[test]
fn brackets_inside_strings_are_ordinary_characters() {
    assert_eval_to(r#"(string-length "[a]")"#, "3");
}

#[test]
fn curly_braces_are_still_reserved() {
    // R7RS reserves `{ }` too, but no dialect Patina cares about spends them,
    // so they stay an error rather than being widened along with the brackets.
    assert_program_eval_error("(display {a})");
}

// =============================================================================
// Bytevector syntax
// =============================================================================

#[test]
fn r6rs_bytevector_syntax_is_read() {
    assert_eval_to("(bytevector-u8-ref '#vu8(1 2 3) 1)", "2");
    assert_eval_to("(equal? '#vu8(1 2 3) '#u8(1 2 3))", "#t");
}

#[test]
fn a_bare_hash_v_is_still_an_error() {
    assert_program_eval_error("(display '#vx(1))");
}

// =============================================================================
// Library version references
// =============================================================================

#[test]
fn an_import_may_carry_a_version_reference() {
    // R6RS §7.1 puts the version last and makes it a list. Patina has no
    // version resolution, so the reference is discarded rather than matched:
    // `(scheme base (6))` and `(scheme base)` name the same library.
    assert_program_eval_to("(import (scheme base (6)))\n(+ 1 2)", "3");
    assert_program_eval_to("(import (scheme base ((>= 6))))\n(+ 1 2)", "3");
    assert_program_eval_to("(import (scheme base (or (6) (7))))\n(+ 1 2)", "3");
    assert_program_eval_to("(import (scheme base ()))\n(+ 1 2)", "3");
}

#[test]
fn a_version_reference_does_not_swallow_a_name_part() {
    // `(srfi 1)` ends in a fixnum and `(scheme base)` in a symbol; neither is
    // a list, so neither is mistaken for a version.
    assert_program_eval_to("(import (scheme base) (srfi 1))\n(first '(1 2))", "1");
}
