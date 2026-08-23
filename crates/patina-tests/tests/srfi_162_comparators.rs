//! SRFI 162 — the comparator constants and min/max procedures.
//!
//! The upstream SRFI 128 suite (`upstream_srfi_suites.rs`) is the real gate:
//! 170 assertions, and it reaches these names because Patina exports them
//! from `(srfi 128)`. What that suite does *not* check is where they come
//! from, which is the whole design decision here — SRFI 162 says its bindings
//! belong in the SRFI 128 library rather than in one of their own, so this
//! file pins the reachability from every name Patina offers them under, and
//! `pair-comparator`, which the specification lists and chibi's export list
//! happens to omit.

mod common;

/// Every SRFI 162 name resolves through `(srfi 128)`, and each constant is
/// actually a comparator rather than an unbound name that merely imported.
#[test]
fn srfi_162_names_are_exported_from_srfi_128() {
    common::assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 128))
        (map comparator?
             (list default-comparator boolean-comparator real-comparator
                   char-comparator char-ci-comparator
                   string-comparator string-ci-comparator
                   pair-comparator list-comparator vector-comparator
                   eq-comparator eqv-comparator equal-comparator))
        "#,
        "(#t #t #t #t #t #t #t #t #t #t #t #t #t)",
    );
}

/// `pair-comparator` is specified by SRFI 162 ("compares pairs as if by the
/// application of `make-pair-comparator` … with `default-comparator`") and is
/// defined by its sample implementation, but chibi's `(srfi 128)` export list
/// leaves it out. Ours does not, so the omission cannot be inherited by
/// copying chibi's list in some later edit.
#[test]
fn pair_comparator_is_exported_and_orders_pairs() {
    common::assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 128))
        (list (=? pair-comparator '(1 . 2) '(1 . 2))
              (<? pair-comparator '(1 . 2) '(1 . 3))
              (<? pair-comparator '(1 . 9) '(2 . 0)))
        "#,
        "(#t #t #t)",
    );
}

#[test]
fn comparator_min_and_max_take_arguments_and_lists() {
    common::assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 128))
        (list (comparator-max real-comparator 3 1 4 1 5)
              (comparator-min real-comparator 3 1 4 1 5)
              (comparator-max-in-list string-comparator '("b" "a" "c"))
              (comparator-min-in-list string-comparator '("b" "a" "c")))
        "#,
        "(5 1 \"c\" \"a\")",
    );
}

/// The case-insensitive constants are not aliases of their case-sensitive
/// neighbours — the pairing is easy to get wrong when transcribing a list of
/// sixteen names.
#[test]
fn the_case_insensitive_comparators_ignore_case() {
    common::assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 128))
        (list (=? string-ci-comparator "Abc" "aBC")
              (=? string-comparator "Abc" "aBC")
              (=? char-ci-comparator #\A #\a)
              (=? char-comparator #\A #\a))
        "#,
        "(#t #f #t #f)",
    );
}

/// R7RS-large reaches the same bindings under `(scheme comparator)`.
/// `r7rs_large_aliases.rs` checks the export sets are equal; this checks the
/// names work through the alias, which an equal-but-both-broken pair of lists
/// would not catch.
#[test]
fn the_names_work_through_the_r7rs_large_alias() {
    common::assert_program_eval_to(
        r#"
        (import (scheme base) (scheme comparator))
        (list (comparator-max real-comparator 2 7 1)
              (comparator? default-comparator)
              (=? equal-comparator '(1 #(2)) '(1 #(2))))
        "#,
        "(7 #t #t)",
    );
}
