//! SRFI 124 ephemerons, across both backends.
//!
//! An ephemeron is not a weak pair: its datum is kept alive only while its
//! *key* is reachable by some other path, so a datum that refers back to its
//! own key does not keep the pair alive. That is the property worth pinning,
//! and it is the one a Scheme-level implementation cannot provide — the
//! behaviour is the collector's (`heap::gc`'s ephemeron fixpoint), and these
//! procedures are only its surface.
//!
//! There is no upstream suite to vendor: the SRFI ships implementations but no
//! tests, and chibi's `(srfi 124)` has none either. Larceny's `ephemeron`
//! suite is the only one, it is a lane rather than a cargo test, and it forces
//! collection by allocating 100 million pairs — too slow for the tree-walker
//! and, on the VM, defeated by a stale register (triage family 32). These
//! force collection directly instead.

mod common;
use common::*;

/// The whole point of the type: a datum may refer to its own key without
/// keeping it alive. A weak *pair* cannot do this — the datum is a strong
/// reference, so the cycle would be self-sustaining.
#[test]
fn a_datum_referring_to_its_key_does_not_retain_it() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define (make-one) (let ((k (list 'key))) (make-ephemeron k (vector k))))
         (define e (make-one))
         (gc)
         (list (ephemeron-broken? e) (ephemeron-key e) (ephemeron-datum e))",
        "(#t #f #f)",
    );
}

/// A key still reachable elsewhere keeps the pair whole, and its datum with it.
#[test]
fn a_live_key_keeps_the_pair_and_its_datum() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define k (list 'key))
         (define e (make-ephemeron k (vector 'datum)))
         (gc)
         (list (ephemeron-broken? e) (equal? (ephemeron-key e) '(key)) (ephemeron-datum e))",
        "(#f #t #(datum))",
    );
}

/// The suite's shape: half the keys dropped, and exactly those pairs break.
/// The replacement is computed in a frame that returns, which is what keeps
/// the VM's stale registers out of it — see triage family 32.
#[test]
fn only_the_pairs_whose_keys_died_are_broken() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define keys (map (lambda (n) (list n)) '(0 1 2 3 4 5 6 7 8 9)))
         (define ephemera (map (lambda (k) (make-ephemeron k (vector 1))) keys))
         (define (drop!) (set! keys (reverse (reverse (list-tail keys 5)))))
         (drop!)
         (gc)
         (map ephemeron-broken? ephemera)",
        "(#t #t #t #t #t #f #f #f #f #f)",
    );
}

/// An immediate key has no cell that can die, so such a pair never breaks.
#[test]
fn a_pair_with_an_immediate_key_never_breaks() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define e (make-ephemeron 42 (vector 'datum)))
         (gc)
         (list (ephemeron-broken? e) (ephemeron-key e))",
        "(#f 42)",
    );
}

/// `#f` is a legal key, and such a pair is not broken — which is why "broken"
/// is a state of its own rather than a pair of `#f`s.
#[test]
fn a_false_key_is_not_a_broken_pair() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define e (make-ephemeron #f #f))
         (gc)
         (list (ephemeron? e) (ephemeron-broken? e) (ephemeron-key e))",
        "(#t #f #f)",
    );
}

#[test]
fn the_predicate_and_the_barrier() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron))
         (define k (list 'k))
         (list (ephemeron? (make-ephemeron k 1)) (ephemeron? 5) (ephemeron? '())
               (begin (reference-barrier k) (car k)))",
        "(#t #f #f k)",
    );
}

#[test]
fn the_accessors_reject_a_non_ephemeron() {
    for expr in [
        "(ephemeron-key 5)",
        "(ephemeron-datum 5)",
        "(ephemeron-broken? 5)",
    ] {
        assert_program_eval_error(&format!("(import (scheme base) (scheme ephemeron)) {expr}"));
    }
}
