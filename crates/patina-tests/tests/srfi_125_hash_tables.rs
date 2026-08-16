//! SRFI 125 (intermediate hash tables) and its R7RS-large name,
//! `(scheme hash-table)`.
//!
//! The bundled library is a thin layer over SRFI 69 and SRFI 128 — see
//! `lib/srfi/125.sld` for the three places it has to diverge from chibi's
//! upstream file, each of which is exercised below. The headline gate is
//! chibi's own 74-test suite, run separately; these pin the behaviours that
//! are ours rather than upstream's, so a regression names itself.

mod common;
use common::*;

/// Both names must reach the same library — the Red edition adopts SRFI 125's
/// bindings unchanged, so `(scheme hash-table)` is a re-export.
#[test]
fn test_both_names_provide_the_same_bindings() {
    for library in ["(srfi 125)", "(scheme hash-table)"] {
        assert_program_eval_to(
            &format!(
                r#"
                (import (scheme base) {library} (srfi 128))
                (define ht (make-hash-table (make-equal-comparator)))
                (hash-table-set! ht 'a 1 'b 2)
                (list (hash-table-size ht)
                      (hash-table-ref/default ht 'a #f)
                      (hash-table-contains? ht 'b)
                      (hash-table-ref/default ht 'z 'none))
                "#
            ),
            "(2 1 #t none)",
        );
    }
}

/// Deviation 1: a SRFI 128 comparator's hash function takes one argument,
/// while our SRFI 69 calls hash functions with a bound. Constructing from a
/// comparator is the path that would break without the adapter.
#[test]
fn test_a_comparator_can_construct_a_table() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 125) (srfi 128))
        (define ht (hash-table (make-equal-comparator) "x" 1 "y" 2))
        (define eqv-ht (make-hash-table (make-eqv-comparator)))
        (hash-table-set! eqv-ht 42 'answer)
        (list (hash-table-size ht)
              (hash-table-ref/default ht "y" #f)
              (hash-table-ref/default eqv-ht 42 #f))
        "#,
        "(2 2 answer)",
    );
}

/// Deviation 2: SRFI 125 has immutable tables; Patina has no object-level
/// immutability, so the flag is tracked beside the table. A one-argument
/// `hash-table-copy` produces an immutable copy — the default is *not*
/// mutable, which is easy to get backwards.
#[test]
fn test_copy_controls_mutability() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 125) (srfi 128))
        (define ht (make-hash-table (make-equal-comparator)))
        (hash-table-set! ht 'k 'v)
        (list (hash-table-mutable? ht)
              (hash-table-mutable? (hash-table-copy ht))
              (hash-table-mutable? (hash-table-copy ht #f))
              (hash-table-mutable? (hash-table-copy ht #t)))
        "#,
        "(#t #f #f #t)",
    );
}

/// Deviation 3a: SRFI 69's `hash-table-ref` takes only a failure thunk;
/// SRFI 125 also applies `success` to the value found.
#[test]
fn test_ref_takes_failure_and_success() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 125) (srfi 128))
        (define ht (make-hash-table (make-equal-comparator)))
        (hash-table-set! ht 'k 7)
        (list (hash-table-ref ht 'k)
              (hash-table-ref ht 'k (lambda () 'absent))
              (hash-table-ref ht 'k (lambda () 'absent) (lambda (v) (* v 2)))
              (hash-table-ref ht 'nope (lambda () 'absent))
              (hash-table-ref ht 'nope (lambda () 'absent) (lambda (v) v)))
        "#,
        "(7 7 14 absent absent)",
    );
}

/// Deviation 3b: SRFI 69's `hash-table-merge!` overwrites; SRFI 125's
/// `hash-table-union!` must leave associations already in the first table
/// alone. Getting this backwards is silent — both tables end up the right
/// size, with the wrong values.
#[test]
fn test_union_keeps_the_first_tables_values() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (scheme write) (srfi 125) (srfi 128))
        (define (table . kvs)
          (apply hash-table (make-equal-comparator) kvs))
        (define a (table 'shared 'from-a 'only-a 1))
        (define b (table 'shared 'from-b 'only-b 2))
        (hash-table-union! a b)
        (list (hash-table-ref/default a 'shared #f)
              (hash-table-ref/default a 'only-a #f)
              (hash-table-ref/default a 'only-b #f))
        "#,
        "(from-a 1 2)",
    );
}

/// The set operations delete keys rather than rewrite values, so they must
/// leave the surviving associations untouched.
#[test]
fn test_intersection_and_difference_keep_their_values() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 125) (srfi 128))
        (define (table . kvs)
          (apply hash-table (make-equal-comparator) kvs))
        (define a (table 'x 'a-x 'y 'a-y))
        (define b (table 'y 'b-y 'z 'b-z))
        (define d (table 'x 'd-x 'y 'd-y))
        (hash-table-intersection! a b)
        (hash-table-difference! d b)
        (list (hash-table->alist a) (hash-table->alist d))
        "#,
        "(((y . a-y)) ((x . d-x)))",
    );
}

/// A float key used to crash `(srfi 69)` outright — `hash` returned an
/// inexact value and `vector-ref` rejected it as an index. Fixed in the
/// bundled SRFI 69, so it is pinned at both layers.
#[test]
fn test_inexact_keys_hash_to_exact_values() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 69))
        (define ht (make-hash-table))
        (hash-table-set! ht 2.718 'e)
        (list (exact-integer? (hash 2.718))
              (exact-integer? (hash (vector 'a 2.718)))
              (hash-table-ref/default ht 2.718 'missing))
        "#,
        "(#t #t e)",
    );
}
