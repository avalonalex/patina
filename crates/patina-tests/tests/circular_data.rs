//! Tests for circular/shared data handling (datum labels)

mod common;
use common::*;

// =============================================================================
// write on circular data (via read)
// =============================================================================

#[test]
fn test_write_circular_pair() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

#[test]
fn test_write_circular_list() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a b . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a b . #0#)""##);
}

#[test]
fn test_display_circular_pair() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (display x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

#[test]
fn test_write_shared_circular() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (write-shared x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

// =============================================================================
// Quoted circular literals (previously caused stack overflow)
// =============================================================================

#[test]
fn test_quoted_circular_literal_write() {
    let code = r##"
        (import (scheme write))
        (define x '#0=(a . #0#))
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

#[test]
fn test_quoted_circular_literal_car() {
    let code = r##"
        (define x '#0=(hello . #0#))
        (car x)
    "##;
    assert_program_eval_to(code, "hello");
}

#[test]
fn test_quoted_circular_eq() {
    // The cdr of a circular pair should be eq? to the pair itself
    let code = r##"
        (define x '#0=(a . #0#))
        (eq? x (cdr x))
    "##;
    assert_program_eval_to(code, "#t");
}

// =============================================================================
// Non-circular data still works
// =============================================================================

#[test]
fn test_write_non_circular_list() {
    let code = r#"
        (import (scheme write))
        (let ((out (open-output-string)))
          (write '(1 2 3) out)
          (get-output-string out))
    "#;
    assert_program_eval_to(code, r#""(1 2 3)""#);
}

#[test]
fn test_write_non_circular_vector() {
    let code = r##"
        (import (scheme write))
        (let ((out (open-output-string)))
          (write '#(a b c) out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#(a b c)""##);
}

// =============================================================================
// Mutable circular structures (set-cdr!)
// =============================================================================

#[test]
fn test_write_set_cdr_circular() {
    let code = r##"
        (import (scheme write))
        (define x (list 'a 'b))
        (set-cdr! (cdr x) x)
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a b . #0#)""##);
}

// =============================================================================
// Where the label goes (issue #188)
//
// A label has to be written where the pair it names is written. Two places in
// the writer used to have nowhere to put one, and both answers are checked
// here against chibi and Gauche, which agree on every case below — character
// for character where Patina and they make the same shorthand choice.
// =============================================================================

/// A cycle whose label lands on an interior cdr rather than the head.
///
/// `write_tagged_list_contents` could emit a back-reference to an
/// already-defined label but never a definition, so a labelled tail carried on
/// inline as if unlabelled — nothing ever marked it emitted, and the
/// back-reference it was waiting for could not arrive. Plain `write` on this
/// ordinary circular list grew without bound. chibi and Gauche: identical.
#[test]
fn test_a_cycle_entering_at_an_interior_cdr_terminates() {
    let code = r##"
        (import (scheme write))
        (define y (list 1 2 3))
        (set-cdr! (cddr y) (cdr y))
        (let ((out (open-output-string)))
          (write y out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""(1 . #0=(2 3 . #0#))""##);
}

/// A shared (acyclic) tail, from the same gap: two lists ending in one pair.
/// The sharing was silently dropped — `((1 3) (2 3))`, which reads back as two
/// separate tails. chibi and Gauche: identical.
#[test]
fn test_write_shared_labels_a_shared_tail() {
    let code = r##"
        (import (scheme write))
        (define t (list 3))
        (let ((out (open-output-string)))
          (write-shared (list (cons 1 t) (cons 2 t)) out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""((1 . #0=(3)) (2 . #0#))""##);
}

/// The reported crash: a cycle that re-enters through a quoted form.
///
/// The shorthand rendered `'x` with a scratch `emitted` map of its own, so the
/// recursion could not see that the enclosing datum had already defined the
/// label, defined it again, and came back round. Gauche writes exactly this;
/// chibi writes `#0=(quote #0#)`, differing only in not abbreviating.
#[test]
fn test_a_cycle_through_a_quote_form_terminates() {
    let code = r##"
        (import (scheme write))
        (define x (list 'quote 1))
        (set-car! (cdr x) x)
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0='#0#""##);
}

/// The non-crashing half of the same bug: a label defined *twice*, once
/// outside a quoted form and once inside it. `read` cannot make sense of
/// `(#0=(1) '#0=(1))` — one label, two definitions — and the sharing it was
/// recording is gone either way. Both orders are checked, because the
/// definition and the reference swap places.
#[test]
fn test_a_label_crossing_a_quote_boundary_is_defined_once() {
    let setup = r##"
        (import (scheme write))
        (define a (list 1))
        (define q (list 'quote a))
        (let ((out (open-output-string)))
    "##;
    assert_program_eval_to(
        &format!("{setup} (write-shared (list a q) out) (get-output-string out))"),
        r##""(#0=(1) '#0#)""##,
    );
    assert_program_eval_to(
        &format!("{setup} (write-shared (list q a) out) (get-output-string out))"),
        r##""('#0=(1) #0#)""##,
    );
}

/// When the label belongs to the pair the shorthand would elide, the shorthand
/// declines: `'x` writes no pair for the cdr, so there is nowhere to put it.
/// Patina used to print `('(1) #0=((1)))`, whose `#0=` names a *different*
/// structure from the one inside the shorthand. chibi and Gauche both fall
/// back to the dotted form, and so does this now — identically.
#[test]
fn test_the_quote_shorthand_declines_when_the_elided_pair_is_labelled() {
    let code = r##"
        (import (scheme write))
        (define c (list (list 1)))
        (define q (cons 'quote c))
        (let ((out (open-output-string)))
          (write-shared (list q c) out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""((quote . #0=((1))) #0#)""##);
}

/// The labels are structure, not decoration: writing, reading back and writing
/// again returns the identical text for each shape above. A label that named
/// the wrong pair, or a definition that went missing, would survive every
/// assertion above and fail this one.
#[test]
fn test_the_new_label_forms_round_trip_through_read() {
    let code = r##"
        (import (scheme write) (scheme read))
        (define (round-trips? v)
          (let* ((p (open-output-string))
                 (ignored (write-shared v p))
                 (text (get-output-string p))
                 (back (read (open-input-string text)))
                 (p2 (open-output-string))
                 (ignored2 (write-shared back p2)))
            (string=? text (get-output-string p2))))
        (define t (list 3))
        (define y (list 1 2 3))
        (set-cdr! (cddr y) (cdr y))
        (define c (list (list 1)))
        (define cyc (list 'quote 1))
        (set-car! (cdr cyc) cyc)
        (define a (list 1))
        (define q (list 'quote a))
        (list (round-trips? (list (cons 1 t) (cons 2 t)))
              (round-trips? y)
              (round-trips? (list (cons 'quote c) c))
              (round-trips? cyc)
              (round-trips? (list a q))
              (round-trips? (list q a)))
    "##;
    // The last three are the shapes this fix newly emits: a label defined
    // immediately before an abbreviation, and one referenced inside one.
    assert_program_eval_to(code, "(#t #t #t #t #t #t)");
}

/// The guard that makes the shorthand decline covers all four abbreviations,
/// so the test does too. Naming the function for `quote` makes narrowing the
/// guard to that one arm look reasonable, and nothing else here would notice:
/// `write-shared` would go back to emitting a `#n=` inside a form whose pair
/// it elides.
#[test]
fn test_every_abbreviation_declines_when_its_elided_pair_is_labelled() {
    for (head, prefix) in [
        ("quote", "'"),
        ("quasiquote", "`"),
        ("unquote", ","),
        ("unquote-splicing", ",@"),
    ] {
        // Shared, so the elided pair is labelled: the shorthand must decline.
        let code = format!(
            r##"(import (scheme write))
                (define c (list (list 1)))
                (define q (cons '{head} c))
                (let ((out (open-output-string)))
                  (write-shared (list q c) out)
                  (get-output-string out))"##
        );
        assert_program_eval_to(&code, &format!(r##""(({head} . #0=((1))) #0#)""##));

        // Unshared, so it applies — the same guard, answering the other way.
        let code = format!(
            r##"(import (scheme write))
                (let ((out (open-output-string)))
                  (write-shared (list '{head} (list 1)) out)
                  (get-output-string out))"##
        );
        assert_program_eval_to(&code, &format!(r##""{prefix}(1)""##));
    }
}

/// Plain `write` labels only what is circular, so a merely-shared tail stays
/// unlabelled and is written out twice.
///
/// The counterpart to `test_write_shared_labels_a_shared_tail`, and the reason
/// it is here: the tail rule was widened from "labelled *and* already emitted"
/// to "labelled", so its correctness now rests entirely on pass 1 not labelling
/// shared pairs unless `write-shared` asked. Without this, that could regress
/// into noisy non-conforming `write` output with every other test still green.
#[test]
fn test_plain_write_leaves_a_merely_shared_tail_unlabelled() {
    let code = r##"
        (import (scheme write))
        (define t (list 3))
        (let ((out (open-output-string)))
          (write (list (cons 1 t) (cons 2 t)) out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""((1 3) (2 3))""##);
}

/// A list's spine costs no stack, in either pass.
///
/// Both passes used to walk the cdr chain by recursion, so depth was the
/// list's *length* and `write` on 100_000 elements overflowed the stack —
/// which aborts the process rather than raising anything a `guard` could
/// catch. Length and nesting are different things, and only nesting is
/// allowed to cost a frame now.
///
/// The assertion is on the length of the output rather than its text: what is
/// under test is that the writer returns at all.
#[test]
fn test_a_long_list_does_not_exhaust_the_stack() {
    let code = r##"
        (import (scheme write))
        (define xs (let loop ((i 0) (acc '())) (if (= i 100000) acc (loop (+ i 1) (cons 0 acc)))))
        (let ((out (open-output-string)))
          (write xs out)
          (string-length (get-output-string out)))
    "##;
    // 100000 digits + 99999 separators + 2 parens.
    assert_program_eval_to(code, "200001");
}
