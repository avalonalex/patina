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
//!
//! They rely on `(gc)` collecting at its call, before the caller's next
//! instruction (#639). The tests at the end pin that: a key that dies just
//! before `(gc)` breaks its pair right after it, wherever the call is —
//! including `gc` itself handed to a control primitive as its thunk, and
//! code that `eval`, `load`, a parameter's converter or a re-entered
//! continuation runs — with every safe point's collection turned off as well;
//! where collection is deferred `(gc)` posts the collection and counts it
//! instead; and the VM clears the caller's destination before it collects.

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

// ─── `(gc)` collects at its call (#639) ──────────────────────────────────────
//
// Each test drops the only other path to a key just before `(gc)` and asks
// whether the pair broke right after it, in the same frame. A `(gc)` that
// only posted a request would leave that to the next safe point, which today
// comes before the next instruction anyway; GC_PRD's stage 3 removes that
// poll. So each position runs twice, the second time with every safe point's
// collection turned off, where only a collection at the call can break it.

/// The globals every position test starts from: one key, and its pair.
const ONE_KEY: &str = "(import (scheme base) (scheme ephemeron) (patina debug))
     (define key (list 'key))
     (define e (make-ephemeron key 'datum))";

/// `program` answers `expected` on both backends, with safe points and
/// without.
fn assert_at_the_call(program: &str, expected: &str) {
    assert_program_eval_to(program, expected);
    assert_program_eval_to_without_safe_points(program, expected);
}

fn assert_after_one_key(body: &str, expected: &str) {
    assert_at_the_call(&format!("{ONE_KEY}\n{body}"), expected);
}

#[test]
fn gc_in_head_position_breaks_a_dead_key() {
    assert_after_one_key(
        "(define (run) (set! key #f) (gc) (ephemeron-broken? e))
         (run)",
        "#t",
    );
}

/// In tail position the call is the frame's last act: compiled as a call of
/// the primitive whose value the frame returns, and, through a procedure
/// value, a tail call the frame is replaced by.
#[test]
fn gc_in_tail_position_breaks_a_dead_key() {
    assert_after_one_key(
        "(define (collect-last) (set! key #f) (gc))
         (define (run) (collect-last) (ephemeron-broken? e))
         (run)",
        "#t",
    );
    assert_after_one_key(
        "(define (collect-last-with collect) (set! key #f) (collect))
         (define (run) (collect-last-with gc) (ephemeron-broken? e))
         (run)",
        "#t",
    );
}

/// As an argument, `(gc)`'s value goes to a register the caller reads next.
#[test]
fn gc_as_an_argument_breaks_a_dead_key() {
    assert_after_one_key(
        "(define (observe ignored) (ephemeron-broken? e))
         (define (run) (set! key #f) (observe (gc)))
         (run)",
        "#t",
    );
    assert_after_one_key(
        "(define (run) (set! key #f) (apply (lambda (ignored) (ephemeron-broken? e)) (list (gc))))
         (run)",
        "#t",
    );
}

/// In the before thunk, in tail position, and in the after thunk, in head
/// position, each breaking the pair whose key died just before it and no
/// other: the second key lives until the body drops it.
#[test]
fn gc_in_both_dynamic_wind_thunks_breaks_a_dead_key() {
    let program = |wind: &str| {
        format!(
            "(import (scheme base) (scheme ephemeron) (patina debug))
             (define k1 (list 'k1))
             (define k2 (list 'k2))
             (define e1 (make-ephemeron k1 'd1))
             (define e2 (make-ephemeron k2 'd2))
             (define seen '())
             (define (note!) (set! seen (cons (list (ephemeron-broken? e1)
                                                    (ephemeron-broken? e2))
                                              seen)))
             (define (before) (set! k1 #f) (gc))
             (define (body) (note!) (set! k2 #f))
             (define (after) (gc) (note!))
             {wind}
             (reverse seen)"
        )
    };
    // Thunks written in place, and passed as values.
    for wind in [
        "(dynamic-wind (lambda () (set! k1 #f) (gc))
                       (lambda () (note!) (set! k2 #f))
                       (lambda () (gc) (note!)))",
        "(dynamic-wind before body after)",
    ] {
        assert_at_the_call(&program(wind), "((#t #f) (#t #t))");
    }
}

/// In a `guard` clause, which runs in the `guard`'s continuation once the
/// raise has unwound, and in a handler a continuable raise returns from.
#[test]
fn gc_in_an_exception_handler_breaks_a_dead_key() {
    assert_after_one_key(
        "(define (run)
           (guard (c (#t (set! key #f) (gc) (ephemeron-broken? e)))
             (raise 'oops)))
         (run)",
        "#t",
    );
    assert_after_one_key(
        "(define (run)
           (with-exception-handler
             (lambda (c) (set! key #f) (gc) (ephemeron-broken? e))
             (lambda () (raise-continuable 'oops))))
         (run)",
        "#t",
    );
}

/// The collection happens at the call, not at a later safe point: with every
/// safe point's collection turned off, the collection count has risen when
/// the next primitive in the same frame reads it, and the pair has broken.
/// Before #639 `(gc)` posted a request that only a safe point ran, which here
/// would have answered `(0 #f)`.
#[test]
fn collects_at_its_call_with_safe_points_skipped() {
    assert_program_eval_to_without_safe_points(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define (collections) (cdr (assq 'collections (gc-stats))))
         (define key (list 'key))
         (define e (make-ephemeron key 'datum))
         (define (run)
           (let ((before (collections)))
             (set! key #f)
             (gc)
             (list (- (collections) before) (ephemeron-broken? e))))
         (run)",
        "(1 #t)",
    );
}

/// Where collection is deferred, `(gc)` posts the collection and counts it
/// in `deferred-collections`, as every `(gc)` posted before #639: inside a
/// comparator that `(patina internal lists)`'s `member` calls from Rust, on a
/// nested loop, the pair stays whole; the outermost loop's next safe point
/// runs the collection, and then it is broken.
#[test]
fn gc_where_collection_is_deferred_posts_and_counts() {
    // Not with safe points skipped: a safe point is what runs what it posts.
    assert_program_eval_to(
        &format!(
            "{ONE_KEY}
         (import (only (patina internal lists) member))
         (define (stat name) (cdr (assq name (gc-stats))))
         (define inside #f)
         (member 1 (list 1)
                 (lambda (x y)
                   (let ((c (stat 'collections)) (d (stat 'deferred-collections)))
                     (set! key #f)
                     (gc)
                     (set! inside (list (- (stat 'collections) c)
                                        (- (stat 'deferred-collections) d)
                                        (ephemeron-broken? e))))
                   (= x y)))
         (list inside (ephemeron-broken? e))"
        ),
        "((0 1 #f) #t)",
    );
}

/// A library body the backend loads at its top level, a `define-library`
/// form's, collects at its `(gc)` (#677). It was posted and counted there, as
/// it still is on a nested loop (above), while a body being loaded deferred
/// collection for as long as its unevaluated forms were held
/// (`ParsedLibrary`); the load's registry entry roots them now. At least one
/// collection, since under the stress lane allocation collects in the body
/// too.
#[test]
fn gc_in_a_library_body_collects_at_its_call() {
    assert_program_eval_to(
        "(define-library (gc in a body)
           (import (scheme base) (patina debug))
           (export observed)
           (begin
             (define (stat name) (cdr (assq name (gc-stats))))
             (define c (stat 'collections))
             (define d (stat 'deferred-collections))
             (gc)
             (define observed (list (< c (stat 'collections))
                                    (- (stat 'deferred-collections) d)))))
         (import (scheme base) (gc in a body))
         observed",
        "(#t 0)",
    );
}

/// The VM clears the register `(gc)`'s value goes to before it collects
/// (`deliver_before_collecting`). A liveness map keeps a call's destination
/// as a root from the call on, so what the register held before the call
/// would otherwise survive the collection: here the key, which `(take!)`
/// returned into the register the allocator gives `(gc)`'s value as well
/// (`patina --dump` shows both calls writing `r4`), and which nothing reads
/// in between. Collecting first and clearing after answers `#f`, and no
/// check build notices, since keeping a value longer frees nothing early.
///
/// The VM only, because the tree-walker keeps the key for another reason:
/// the CPS transform binds a sequence's discarded value to the parameter of
/// the continuation for the rest of the body, which holds it until the
/// procedure returns. The program shape answers `#f` there with or without
/// #639.
#[test]
fn gc_on_the_vm_clears_its_destination_before_it_collects() {
    let program = "(import (scheme base) (scheme ephemeron) (patina debug))
         (define box (list #f))
         (define e (let ((k (list 'key))) (set-car! box k) (make-ephemeron k 'datum)))
         (define (id x) x)
         (define (take!) (let ((k (car box))) (set-car! box #f) k))
         (define (run) (id (id (take!))) (id (gc)) (ephemeron-broken? e))
         (run)";
    assert_eq!(eval_program_vm(program), "#t");
    assert_eq!(eval_program_vm_without_safe_points(program), "#t");
}

// ─── `gc` as a control primitive's thunk ─────────────────────────────────────
//
// `gc` is the first resumable primitive that takes no arguments, so it is the
// first a control primitive can be handed as a thunk. Its collecting stub
// then goes over a stub frame of the control primitive's own — on the VM the
// value forms' stubs and the frames of a jump's, an abort's and a composable
// re-entry's travel; on the tree-walker under the continuation the control
// primitive built — where no resumable primitive's frame went before #639.

/// Both `dynamic-wind` thunks are `gc`. The first key dies before the call
/// and the before thunk breaks it; the second dies in the body and the after
/// thunk breaks it, run by the extent's normal exit, by the travel of a
/// jump out of the body, or by an abort's. In head position, as a value, and
/// through `apply`.
#[test]
fn gc_as_both_dynamic_wind_thunks_breaks_a_dead_key() {
    let program = |call: &str| {
        format!(
            "(import (scheme base) (scheme ephemeron) (patina debug))
             (define (as-value procedure) procedure)
             (define k1 (list 'k1))
             (define k2 (list 'k2))
             (define e1 (make-ephemeron k1 'd1))
             (define e2 (make-ephemeron k2 'd2))
             (define seen '())
             (define (note!) (set! seen (cons (list (ephemeron-broken? e1)
                                                    (ephemeron-broken? e2))
                                              seen)))
             (define t (make-continuation-prompt-tag 'p))
             (set! k1 #f)
             {call}
             (note!)
             (reverse seen)"
        )
    };
    let returns = "(lambda () (note!) (set! k2 #f))";
    let jumps = "(lambda () (note!) (set! k2 #f) (k 'out))";
    let aborts = "(lambda () (note!) (set! k2 #f) (abort-current-continuation t 'out))";
    let mut calls = vec![format!("(apply dynamic-wind (list gc {returns} gc))")];
    for wind in ["dynamic-wind", "(as-value dynamic-wind)"] {
        calls.push(format!("({wind} gc {returns} gc)"));
        calls.push(format!("(call/cc (lambda (k) ({wind} gc {jumps} gc)))"));
        calls.push(format!(
            "(call-with-continuation-prompt (lambda () ({wind} gc {aborts} gc))
                                            t
                                            (lambda (v k) v))"
        ));
    }
    for call in calls {
        assert_at_the_call(&program(&call), "((#t #f) (#t #t))");
    }
}

/// `gc` as `with-exception-handler`'s thunk, `call-with-continuation-
/// prompt`'s body and `call-with-values`' producer, each in head position
/// and reached as a value.
#[test]
fn gc_as_a_control_primitives_thunk_breaks_a_dead_key() {
    for (operator, call) in [
        ("with-exception-handler", "(lambda (c) c) gc"),
        (
            "call-with-continuation-prompt",
            "gc (make-continuation-prompt-tag 'p) (lambda (v k) v)",
        ),
    ] {
        for operator in [operator.to_string(), format!("(as-value {operator})")] {
            assert_after_one_key(
                &format!(
                    "(define (as-value procedure) procedure)
                     (define (run) (set! key #f) ({operator} {call}) (ephemeron-broken? e))
                     (run)"
                ),
                "#t",
            );
        }
    }
    for operator in ["call-with-values", "(as-value call-with-values)"] {
        assert_after_one_key(
            &format!(
                "(define (as-value procedure) procedure)
                 (define (run)
                   (set! key #f)
                   ({operator} gc (lambda ignored (ephemeron-broken? e))))
                 (run)"
            ),
            "#t",
        );
    }
}

/// `gc` as the before thunk that resuming an abort's composable continuation
/// runs again on its way back into the extent: the key dies between the
/// abort and the resume, and the body, going on from the abort, sees it
/// broken.
#[test]
fn gc_as_the_before_thunk_of_a_resumed_extent_breaks_a_dead_key() {
    for wind in ["dynamic-wind", "(as-value dynamic-wind)"] {
        assert_after_one_key(
            &format!(
                "(define (as-value procedure) procedure)
                 (define t (make-continuation-prompt-tag 'p))
                 (define resume #f)
                 (define seen '())
                 (call-with-continuation-prompt
                   (lambda ()
                     ({wind} gc
                             (lambda ()
                               (abort-current-continuation t 'captured)
                               (set! seen (cons (ephemeron-broken? e) seen)))
                             (lambda () #f)))
                   t
                   (lambda (v k) (set! resume k)))
                 (set! key #f)
                 (resume 'again)
                 seen"
            ),
            "(#t)",
        );
    }
}

/// A continuation captured just before `(gc)` and re-entered twice: each
/// pass drops one more key, and its `(gc)` breaks that pair and no other.
#[test]
fn gc_in_a_reentered_continuation_breaks_each_newly_dead_key() {
    assert_at_the_call(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define keys (list (list 'a) (list 'b) (list 'c)))
         (define es (map (lambda (k) (make-ephemeron k 'd)) keys))
         (define again #f)
         (define seen '())
         (define (run)
           (let ((i (call/cc (lambda (k) (set! again k) 0))))
             (set! keys (cdr keys))
             (gc)
             (set! seen (cons (map ephemeron-broken? es) seen))
             (when (< i 2) (again (+ i 1)))))
         (run)
         (reverse seen)",
        "((#t #f #f) (#t #t #f) (#t #t #t))",
    );
}

/// `(gc)` in code that `eval` and `load` run, which each machine runs in its
/// own loop rather than a nested one (#477), collects at its call there too.
#[test]
fn gc_in_evaluated_and_loaded_code_breaks_a_dead_key() {
    assert_after_one_key(
        "(import (scheme eval))
         (define (run)
           (set! key #f)
           (eval '(gc) (environment '(patina debug)))
           (ephemeron-broken? e))
         (run)",
        "#t",
    );
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = scratch_path(&dir, "collect.scm");
    std::fs::write(
        &path,
        "(set! key #f) (gc) (set! inside (ephemeron-broken? e))",
    )
    .expect("write the file to load");
    assert_after_one_key(
        &format!(
            "(import (scheme load))
             (define inside #f)
             (load \"{path}\")
             inside"
        ),
        "#t",
    );
}

/// `(gc)` in a parameter's converter, which `make-parameter` and
/// `parameterize` call from a resumable primitive's stub (#478): one
/// suspended call under another.
#[test]
fn gc_in_a_parameter_converter_breaks_a_dead_key() {
    let convert = "(define (convert x)
                     (if (eq? x 'collect)
                         (begin (set! key #f) (gc) (ephemeron-broken? e))
                         x))";
    for program in [
        format!(
            "{convert}
             (define p (make-parameter 0 convert))
             (parameterize ((p 'collect)) (p))"
        ),
        format!(
            "{convert}
             (define p (make-parameter 'collect convert))
             (p)"
        ),
    ] {
        assert_after_one_key(&program, "#t");
    }
}
