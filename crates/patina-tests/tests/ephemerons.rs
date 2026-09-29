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
//! collection by allocating 100 million pairs. These tests force collection
//! directly, including the register-lifetime regression from #423.

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
/// Collect in the same frame that computes the replacement: #423 left the
/// old list alive in the computation's temporary registers until that frame
/// returned. No helper may hide those registers from this collection.
#[test]
fn only_the_pairs_whose_keys_died_are_broken() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define keys (map (lambda (n) (list n)) '(0 1 2 3 4 5 6 7 8 9)))
         (define ephemera (map (lambda (k) (make-ephemeron k (vector 1))) keys))
         (define (drop-and-collect!)
           (set! keys (reverse (reverse (list-tail keys 5))))
           (gc)
           (map ephemeron-broken? ephemera))
         (drop-and-collect!)",
        "(#t #t #t #t #t #f #f #f #f #f)",
    );
}

#[test]
fn earlier_operands_stay_live_across_collection_in_a_later_operand() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define key (list 'key))
         (define e (make-ephemeron key 'datum))
         (list key (begin (set! key #f) (gc) (ephemeron-broken? e)))",
        "((key) #f)",
    );
}

#[test]
fn reference_barrier_keeps_its_argument_live_until_the_call() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define (check key)
           (define e (make-ephemeron key 'datum))
           (define broken #f)
           (gc)
           (set! broken (ephemeron-broken? e))
           (reference-barrier key)
           broken)
         (check (list 'key))",
        "#f",
    );
}

#[test]
fn a_tail_call_retires_the_previous_register_window() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define e #f)
         (define (finish) (gc) (ephemeron-broken? e))
         (define (start key)
           (set! e (make-ephemeron key 'datum))
           (finish))
         (define (run) (start (list 'key)))
         (run)",
        "#t",
    );
}

#[test]
fn a_self_tail_call_retires_its_argument_copies() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define e #f)
         (define (loop key first?)
           (if first?
               (loop (list 'key) #f)
               (begin
                 (set! e (make-ephemeron key 'datum))
                 (set! key #f)
                 (gc)
                 (ephemeron-broken? e))))
         (loop #f #t)",
        "#t",
    );
}

#[test]
fn a_continuation_does_not_resurrect_finished_temporaries() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define keys (list (list 'dead) (list 'live)))
         (define e (make-ephemeron (car keys) 'datum))
         (define saved #f)
         (define (drop-and-capture!)
           (set! keys (reverse (reverse (cdr keys))))
           (call/cc (lambda (k) (set! saved k) #f))
           (gc)
           (ephemeron-broken? e))
         (drop-and-capture!)",
        "#t",
    );
}

#[test]
fn a_continuation_keeps_an_earlier_operand_for_reentry() {
    assert_program_eval_to(
        "(import (scheme base) (patina debug))
         (define saved #f)
         (define pass 0)
         (define result
           (list (list 'alive)
                 (call/cc (lambda (k) (set! saved k) 'first))))
         (set! pass (+ pass 1))
         (if (= pass 1)
             (begin (set! result #f) (gc) (saved 'second))
             result)
         result",
        "((alive) second)",
    );
}

#[test]
fn a_delimited_snapshot_retires_dead_temps_and_keeps_pending_operands() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define keys (list (list 'dead) (list 'live)))
         (define e (make-ephemeron (car keys) 'datum))
         (define saved #f)
         (define tag (make-continuation-prompt-tag))
         (call-with-continuation-prompt
           (lambda ()
             (set! keys (reverse (reverse (cdr keys))))
             (list (list 'alive)
                   (abort-current-continuation tag 'first)))
           tag (lambda (value k) (set! saved k) value))
         (gc)
         (list (ephemeron-broken? e)
               (saved 'second)
               (begin (gc) (saved 'third)))",
        "(#t ((alive) second) ((alive) third))",
    );
}

/// A datum that is a *continuation* survives, which is the case where the two
/// weak mechanisms meet.
///
/// The VM keeps a captured continuation's payload in a side table under a weak
/// id, traced only once its `VmContinuationRef` has been marked. An ephemeron
/// retained by the ephemeron fixpoint can be what marks that ref — so if the
/// two fixpoints run in sequence rather than nested, the id is discovered
/// after the weak-id loop has stopped, its payload is never traced, and
/// `sweep_weak` keeps a store entry pointing at swept slots. The program below
/// then dies with "expected a procedure, got object" on the VM while the
/// tree-walker, which has no such side table, prints the list.
#[test]
fn an_ephemeron_holding_a_continuation_keeps_its_payload() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define kk (list 'key))
         (define e #f)
         (define (capture)
           (let ((secret (list 'a 'b 'c)))
             (let ((v (call/cc (lambda (c) (set! e (make-ephemeron kk c)) 0))))
               (if (= v 0) 'captured secret))))
         (capture)
         (gc)
         ((ephemeron-datum e) 1)",
        "(a b c)",
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
