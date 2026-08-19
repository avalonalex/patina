//! What chibi's own SRFI 125 suite cannot see.
//!
//! The suite is the headline gate — 74 assertions, run on both backends by
//! `upstream_srfi_suites.rs` — and it already covers the library's behaviour
//! far better than a hand-written file would. So this one deliberately holds
//! only the cases it *misses*, each of which was a real defect found by review
//! after the suite was green:
//!
//! - `hash-table-copy`'s immutable default, which the suite covers but which
//!   is ours rather than upstream's — the flag has no runtime support behind
//!   it;
//! - `hash-table-update!`'s `success` argument, which the suite calls with at
//!   most four arguments;
//! - `(srfi 69)`'s own `hash` on inexact keys, which the suite never calls.
//!
//! `(scheme hash-table)`'s export list is checked against `(srfi 125)`'s by
//! `r7rs_large_aliases.rs`, which compares the two in both directions — a
//! stronger check than sampling bindings here.

mod common;
use common::*;

/// A hash function handed straight to SRFI 69's constructors — which the
/// upstream suite does at its `ht-symbol` fixture, extracting
/// `(comparator-hash-function default-comparator)` and passing it along.
///
/// It is a *one*-argument procedure, while our SRFI 69 always supplies the
/// bound, so it only works because SRFI 125 adapts every hash function on the
/// way in rather than only the ones it reads out of a comparator. Adapting
/// only the comparator path passes every test in this file and fails the
/// upstream suite, which is why this case is pinned here too.
#[test]
fn test_a_hash_function_passed_by_hand_is_adapted() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 125) (srfi 128))
        (define ht
          (alist->hash-table '((a . 1) (b . 2))
                             equal?
                             (comparator-hash-function (make-default-comparator))))
        (list (hash-table-ref/default ht 'a 'missing)
              (hash-table-ref/default ht 'b 'missing)
              (hash-table-size ht))
        "#,
        "(1 2 2)",
    );
}

/// SRFI 125 defines `hash-table-update!` as
/// `(hash-table-set! ht key (updater (hash-table-ref ht key failure success)))`
/// — so `success` transforms the value *before* the updater sees it.
///
/// SRFI 69's version takes only a failure thunk and has a rest argument, so an
/// inherited one accepted `success` and silently ignored it: the update below
/// produced 8 instead of 71.
#[test]
fn test_update_applies_success_before_the_updater() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 125) (srfi 128))
        (define ht (make-hash-table (make-equal-comparator)))
        (hash-table-set! ht 'k 7)
        (hash-table-update! ht 'k (lambda (v) (+ v 1)) (lambda () 0) (lambda (v) (* v 10)))
        (define missing-key
          (begin
            (hash-table-update! ht 'absent (lambda (v) (* v 2)) (lambda () 5))
            (hash-table-ref/default ht 'absent #f)))
        (list (hash-table-ref/default ht 'k #f) missing-key)
        "#,
        "(71 10)",
    );
}

/// `(srfi 69)`'s `hash` feeds a `vector-ref` index, so an inexact result
/// crashes the table outright — which it did, for every inexact key.
///
/// Two branches produce one: `real?` reaches `numerator`/`denominator`, which
/// R7RS makes inexact for inexact input, and `integer?` matches `2.0` first.
/// The first fix covered only `real?`, so `2.0` still crashed and every test
/// here still passed — hence a case from each branch, plus a composite that
/// recurses back into `hash`.
#[test]
fn test_inexact_keys_hash_to_exact_values() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 69))
        (define ht (make-hash-table))
        (hash-table-set! ht 2.718 'real-branch)
        (hash-table-set! ht 2.0 'integer-branch)
        (list (exact-integer? (hash 2.718))
              (exact-integer? (hash 2.0))
              (exact-integer? (hash 1e10))
              (exact-integer? (hash (vector 'a 2.0)))
              (hash-table-ref/default ht 2.718 'missing)
              (hash-table-ref/default ht 2.0 'missing))
        "#,
        "(#t #t #t #t real-branch integer-branch)",
    );
}

/// `(make-hash-table eq?)` keys by identity, so it must *hash* by identity
/// too. Upstream's `hash-by-identity` is the structural `hash`, and SRFI 125's
/// `make-eq-comparator` routes here, so the mismatch was load-bearing.
///
/// Three failures, one per way structure and identity part company: a key
/// mutated after insertion moves out of its bucket, a procedure has no
/// structure to hash, and a circular one has no end. All three are what chibi
/// answers — its SRFI 69 is C-backed and hashes by identity.
#[test]
fn test_an_eq_table_hashes_by_identity() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 69))
        (define ht (make-hash-table eq?))
        (define k (vector 1 2 3))
        (hash-table-set! ht k 'v)
        (vector-set! k 0 99)
        (hash-table-ref/default ht k 'MISSING)
        "#,
        "v",
    );
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 69))
        (define ht (make-hash-table eq?))
        (hash-table-set! ht car 'v)
        (hash-table-ref/default ht car 'MISSING)
        "#,
        "v",
    );
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 69))
        (define circ (list 1 2))
        (set-cdr! (cdr circ) circ)
        (define ht (make-hash-table eq?))
        (hash-table-set! ht circ 'v)
        (hash-table-ref/default ht circ 'MISSING)
        "#,
        "v",
    );
}

/// Identity hashing must still agree with `eq?` on everything `eq?` accepts,
/// and must still tell distinct objects apart.
#[test]
fn test_identity_hashing_agrees_with_eq() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 69))
        (define-record-type <p> (mk a) p? (a p-a))
        (define r (mk 1))
        (define (round-trip key)
          (let ((h (make-hash-table eq?)))
            (hash-table-set! h key 'found)
            (hash-table-ref/default h key 'MISSING)))
        (define distinct
          (let ((h (make-hash-table eq?)) (a (vector 1)) (b (vector 1)))
            (hash-table-set! h a 'A)
            (hash-table-set! h b 'B)
            (list (hash-table-ref/default h a '?) (hash-table-ref/default h b '?))))
        (list (round-trip r) (round-trip 'sym) (round-trip 42) (round-trip #\a)
              (round-trip #t) (round-trip '()) (round-trip "s") distinct)
        "#,
        "(found found found found found found found (A B))",
    );
}

/// D7 — `hash` reached `numerator` for any real that is not rational, so
/// `+inf.0`, `-inf.0` and `+nan.0` raised instead of hashing. The `exact` wrap
/// that fixed `2.0` and the rationals did not reach them.
#[test]
fn test_non_rational_reals_are_hashable() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (srfi 69))
        (define ht (make-hash-table))
        (hash-table-set! ht +inf.0 'inf)
        (list (exact-integer? (hash +inf.0))
              (exact-integer? (hash -inf.0))
              (exact-integer? (hash +nan.0))
              (hash-table-ref/default ht +inf.0 'MISSING))
        "#,
        "(#t #t #t inf)",
    );
}
