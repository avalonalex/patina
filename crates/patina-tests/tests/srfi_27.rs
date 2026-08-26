//! SRFI 27 conformance, differential across both backends.
//!
//! The bundled implementation is Sebastian Egner's 54-bit MRG32k3a reference
//! (see `lib/srfi/PROVENANCE.md`). chibi's suite, vendored at
//! `scheme_tests/upstream/srfi/27/`, exercises the interface; what these pin
//! and it does not are the exact stream values, recorded from this
//! implementation — MRG32k3a is fully specified, so any change to them is a
//! generator change, not noise. (Known limit: the values pin the port's
//! arithmetic as-is, faithful bugs included — they were not re-derived from
//! L'Ecuyer's matrices.) Every helper runs both backends and requires them
//! to agree.

mod common;
use common::assert_program_eval_to;

/// `pseudo-randomize!` must select a reproducible, seed-determined stream —
/// this is SRFI 27's central guarantee, and the exact values pin MRG32k3a.
#[test]
fn pseudo_randomized_stream_is_deterministic() {
    assert_program_eval_to(
        "(import (srfi 27)) \
         (define s (make-random-source)) \
         (random-source-pseudo-randomize! s 4 7) \
         (define gen (random-source-make-integers s)) \
         (list (gen 1000000) (gen 1000000) (gen 1000000))",
        "(744602 257620 441205)",
    );
}

/// The same (i, j) must reproduce the same stream; a different j must not.
#[test]
fn streams_reproduce_by_seed() {
    assert_program_eval_to(
        "(import (srfi 27)) \
         (define (take-3 i j) \
           (let* ((s (make-random-source)) \
                  (_ (random-source-pseudo-randomize! s i j)) \
                  (gen (random-source-make-integers s))) \
             (list (gen 4096) (gen 4096) (gen 4096)))) \
         (list (equal? (take-3 4 7) (take-3 4 7)) \
               (equal? (take-3 4 7) (take-3 4 8)))",
        "(#t #f)",
    );
}

/// `state-ref`/`state-set!` round-trips: restoring a state replays the tail.
#[test]
fn state_round_trip_replays_the_stream() {
    assert_program_eval_to(
        "(import (srfi 27)) \
         (define s (make-random-source)) \
         (define gen (random-source-make-integers s)) \
         (gen 1000) (gen 1000) \
         (define saved (random-source-state-ref s)) \
         (define a (list (gen 1000) (gen 1000))) \
         (random-source-state-set! s saved) \
         (equal? a (list (gen 1000) (gen 1000)))",
        "#t",
    );
}

/// Range and type contracts: integers land in [0, n) and are exact;
/// reals land strictly inside (0, 1).
#[test]
fn values_respect_their_ranges() {
    assert_program_eval_to(
        "(import (scheme base) (srfi 27)) \
         (do ((i 0 (+ i 1)) \
              (ok #t (and ok \
                          (let ((n (random-integer 17)) \
                                (r (random-real))) \
                            (and (exact? n) (<= 0 n) (< n 17) (< 0 r) (< r 1)))))) \
             ((= i 200) ok))",
        "#t",
    );
}
