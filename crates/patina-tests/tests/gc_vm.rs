//! Garbage collection through real evaluation — **VM** backend.
//!
//! The backend-independent cases live in `common::gc_shared_tests!` and are
//! invoked here against `eval_program_vm`, with `gc_tree_walker.rs` doing the
//! same for the tree-walker. The shared cases take a single-backend evaluator
//! because several assert on `(gc-stats)` counters, which legitimately differ
//! between the backends.
//!
//! This file keeps only the tests that target VM machinery a heap scan cannot
//! reach: the continuation side tables, `CallFrame::closure`, boxed upvalues,
//! and the multi-value `value_buffer`. See `docs/GC_DESIGN.md` §5.2.

#[macro_use]
mod common;
use common::*;

gc_shared_tests!(
    eval_program_vm,
    eval_program_vm_gc_off,
    eval_program_vm_zeal
);

#[test]
fn closure_free_vars_survive_collection() {
    // A collection inside the closure's body, which then reads and writes
    // its free variable through its frame's `closure`.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (make-counter)
          (let ((n 0))
            (lambda () (gc) (set! n (+ n 1)) n)))
        (define c (make-counter))
        (c)
        (c)
        (c)
        "#,
        "3",
    );
}

#[test]
fn mutable_boxed_upvalue_survives_collection() {
    // `set!` on a captured variable boxes it as a MutableCell reached only
    // through the closure's free_vars.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (make-acc)
          (let ((total '()))
            (lambda (x) (set! total (cons x total)) (gc) total)))
        (define a (make-acc))
        (a 1) (a 2)
        (a 3)
        "#,
        "(3 2 1)",
    );
}

#[test]
fn continuation_in_heap_data_survives_collections_and_invokes() {
    // The side tables are weak (design §9.5): a continuation must stay
    // invocable as long as its ref object is reachable — here only through
    // a heap vector, across two collections, re-entering top level twice.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define saved (vector #f))
        (define count 0)
        (define result
          (+ 1 (call/cc (lambda (k) (vector-set! saved 0 k) 1))))
        (set! count (+ count 1))
        (gc)
        (if (< count 3) ((vector-ref saved 0) 41))
        result
        "#,
        "42",
    );
}

#[test]
fn dead_captures_are_pruned_from_side_table() {
    // The §9.5 blowup: dropped `call/cc` captures used to pin their ref
    // objects and snapshots forever through the side table. 20 000 dead
    // captures must leave no more than noise behind after a collection —
    // with strong tables the delta is the full 20 000.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (live-objects)
          (let ((s (gc-stats)))
            (- (cdr (assq 'objects s)) (cdr (assq 'free-objects s)))))
        (define (churn n)
          (if (> n 0) (begin (call/cc (lambda (k) 1)) (churn (- n 1)))))
        (gc)
        (define before (live-objects))
        (churn 20000)
        (gc)
        (< (- (live-objects) before) 200)
        "#,
        "#t",
    );
}

#[test]
fn multiple_values_survive_collection() {
    // `value_buffer` is the multi-value side channel — a root that is easy
    // to miss because it is empty most of the time.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (call-with-values
          (lambda () (gc) (values 1 2 3))
          (lambda (a b c) (+ a b c)))
        "#,
        "6",
    );
}

// This pins VM register retirement, not a guarantee about exactly when the
// tree-walker releases discarded CPS results.
#[test]
fn discarded_results_and_both_branch_tests_release_the_key() {
    for condition in ["#t", "#f"] {
        assert_gc_eval_to(
            &format!(
                "(import (scheme base) (scheme ephemeron) (patina debug))
                 (define key (list 'key))
                 (define e (make-ephemeron key 'datum))
                 (define (drop!)
                   key
                   (if (begin key {condition}) key key)
                   (reference-barrier key)
                   (set! key #f)
                   (gc)
                   (ephemeron-broken? e))
                 (drop!)"
            ),
            "#t",
        );
    }
}

#[test]
fn replacing_a_captured_local_releases_its_previous_value() {
    assert_gc_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define (drop!)
           (define keys (list (list 'dead) (list 'live)))
           (define current (lambda () keys))
           (define e (make-ephemeron (car keys) 'datum))
           (set! keys (reverse (reverse (cdr keys))))
           (gc)
           (list (ephemeron-broken? e) (current)))
         (drop!)",
        "(#t ((live)))",
    );
}

#[test]
fn wide_pending_operands_survive_collection() {
    let operands = (0..80).map(|n| format!("(list {n}) ")).collect::<String>();
    assert_gc_eval_to(
        &format!(
            "(import (patina debug))
             (map car (list {operands} (begin (gc) (list 80))))"
        ),
        &format!(
            "({})",
            (0..81).map(|n| n.to_string()).collect::<Vec<_>>().join(" ")
        ),
    );
}

#[test]
fn rebound_control_operator_keeps_operands_after_a_larger_expression() {
    assert_gc_eval_to(
        "(import (patina debug))
         (define (run)
           (list (list 1 2 3 4 5) (list 6 7 8 9 10))
           (call-with-values (lambda () (list 'alive)) (lambda (x) x)))
         (set! call-with-values (lambda (producer consumer)
                                  (gc) (consumer (producer))))
         (run)",
        "(alive)",
    );
}

#[test]
fn code_units_do_not_grow_across_forms_a_collection_lets_go() {
    // `code-units` (#836) counts the units the VM has loaded and not let go.
    // Fifty `eval`s of fresh forms that make no closure each let their unit
    // go as the form returns, so the count reads as it did before them.
    // Fifty that make a closure, which dies with the form, are each kept by
    // the closure's count until a collection frees it, and two `(gc)`s let
    // them all go: the count after them reads as the count before. A unit
    // nothing can run that a collection did not let go would leave the
    // count above where it started. Collecting only where `(gc)` asks, the
    // second batch is counted until then, fifty more; at every outermost
    // safe point each goes at the one after it, and the call that reads the
    // count is one; in the environment's mode, anywhere between.
    //
    // The last reading follows fifty more of them and a single `(gc)`,
    // which frees their closures and lets their units go before it returns,
    // so the count reads as it did before them in every mode: a write of
    // the count before the collection's release, which a second `(gc)`
    // would correct, leaves it fifty above where collection waits for
    // `(gc)`. One collection is enough here because no form's code quotes a
    // closure: a closure only a released unit's constants reach is marked
    // by the collection that releases that unit, and goes at the next.
    //
    // Each reading is taken in the form that did the work before it, with
    // no form loaded between: a load writes the count afresh, and would
    // hide a release that did not. Every reading form counts its own unit,
    // the same one each time. Both batches run once first, so that whatever
    // their first run keeps for good is kept before the first reading. The
    // count is positive, if only for the units the program's own procedures
    // keep; its value is held to the VM's table of units by
    // `the_heaps_count_of_code_units_is_the_tables_size` in `vm_state.rs`,
    // since these readings, compared with each other, would pass a count
    // off by a constant.
    let code = r#"
        (import (scheme base) (scheme eval) (patina debug))
        (define (units) (cdr (assq 'code-units (gc-stats))))
        (define (settled) (gc) (gc) (units))
        (define env (environment '(scheme base)))
        (define (plain i) (list '+ i 1))
        (define (closing i) `(car (map (lambda (x) (+ x ,i)) (list 1 2 3))))
        (define (run-forms make-form)
          (let loop ((i 0))
            (if (< i 50) (begin (eval (make-form i) env) (loop (+ i 1))))))
        (define u0 #f) (define u1 #f) (define u2 #f) (define u3 #f) (define u4 #f)
        (run-forms plain)
        (run-forms closing)
        (set! u0 (settled))
        (set! u1 (begin (run-forms plain) (units)))
        (set! u2 (begin (run-forms closing) (units)))
        (set! u3 (settled))
        (set! u4 (begin (run-forms closing) (gc) (units)))
        (list (> u0 0) (- u1 u0) (- u2 u0) (- u3 u0) (- u4 u0))
        "#;
    assert_eq!(eval_program_vm_gc_off(code), "(#t 0 50 0 0)");
    assert_eq!(eval_program_vm_zeal(code), "(#t 0 0 0 0)");
    let plain = eval_program_vm(code);
    let readings: Vec<&str> = plain
        .trim_matches(|c| c == '(' || c == ')')
        .split_whitespace()
        .collect();
    let [positive, after_plain, closing, after_gc, after_one_gc] = readings[..] else {
        panic!("expected five readings: {plain}")
    };
    assert_eq!(
        (positive, after_plain, after_gc, after_one_gc),
        ("#t", "0", "0", "0"),
        "readings {plain}"
    );
    assert!(
        closing.parse::<i64>().is_ok_and(|n| (0..=50).contains(&n)),
        "the forms that made closures read {closing} units above the start: {plain}"
    );
}

#[test]
fn code_units_count_a_unit_a_live_closure_keeps() {
    // A procedure `eval` compiled keeps its unit for as long as it lives:
    // `keep` holds it, and the count is one above the reading before it, as
    // it is after two `(gc)`s, which leave the unit loaded because the
    // closure is live. Once `keep` lets the closure go, one `(gc)` frees it
    // and lets the unit go before it returns, and the count reads as it did
    // before. Each reading is taken in the form that did the work before
    // it, as in `code_units_do_not_grow_across_forms_a_collection_lets_go`.
    // `eval` is warmed up first, its closure dropped and collected before
    // the first reading. Under zeal too, with a collection at every
    // outermost safe point. Each `(gc)` is written where it runs, once a
    // call, so that the zeal half's check that zeal collected beyond the
    // program's `(gc)`s counts every one of them.
    assert_gc_and_zeal_eval_to(
        r#"
        (import (scheme base) (scheme eval) (patina debug))
        (define (units) (cdr (assq 'code-units (gc-stats))))
        (define env (environment '(scheme base)))
        (define keep (eval '(lambda (x) (* x 3)) env))
        (define answer #f)
        (define u0 #f) (define u1 #f) (define u2 #f) (define u3 #f)
        (set! keep #f)
        (set! u0 (begin (gc) (gc) (units)))
        (set! u1 (begin (set! keep (eval '(lambda (x) (* x 3)) env)) (units)))
        (set! u2 (begin (gc) (gc) (units)))
        (set! answer (keep 14))
        (set! u3 (begin (set! keep #f) (gc) (units)))
        (list answer (> u0 0) (- u1 u0) (- u2 u0) (- u3 u0))
        "#,
        "(42 #t 1 1 0)",
    );
}

#[test]
fn a_paced_collection_leaves_the_namespaces_its_code_release_drops_out_of_l() {
    // A unit's link table holds the namespace it was linked in (#836), so a
    // namespace `environment` made, with a procedure compiled in it, lives
    // until the collection that frees the procedure releases its unit, and
    // that is after the sweep has read the external bytes. The sweep's L
    // counted every such namespace made since the last collection, about
    // 32 KB each, and the threshold, max(8 MiB, 2·L), followed them: each
    // collection of this loop came about 1.76 times further off than the
    // one before, and its garbage grew without bound (345 MiB resident at
    // 32,000 cycles, where the settled L holds 21). The collection's finish
    // settles L once the units are released
    // (`GcController::finish_collection`).
    //
    // One form runs a thousand cycles, each a fresh namespace, a procedure
    // compiled in it, called and dropped: about 35 MB, four collections 8
    // MiB apart. After each cycle the loop reads `(gc-stats)`, and where the
    // count of collections has moved, the L and the threshold the collection
    // left. Each L is within 1 MiB of the L two `(gc)`s left before the
    // loop (about 40 KB above it: the cycle's own namespace and the
    // readings), where the sweep's L was 7 MiB above it at the first; each
    // threshold is the 8 MiB floor, where it was 14 MiB after the first;
    // and there are at least three such collections, where the growing
    // interval allowed two.
    //
    // The interpreter collects at the adaptive threshold whatever the
    // lane's variable says, so the stress lane asserts the same as a plain
    // run: stress and zeal collect every few allocations, each collection
    // finding at most the last cycle's namespace dead, and replace the
    // threshold this reads.
    let code = r#"
        (import (scheme base) (scheme eval) (patina debug))
        (define (stat key) (cdr (assq key (gc-stats))))
        (define (cycle)
          ((eval '(lambda (x) (* x 2)) (environment '(scheme base))) 21))
        (define (run n)
          (let loop ((i 0) (seen (stat 'collections)) (readings '()))
            (if (= i n)
                (reverse readings)
                (begin
                  (cycle)
                  (let* ((stats (gc-stats))
                         (now (cdr (assq 'collections stats))))
                    (loop (+ i 1)
                          now
                          (if (= now seen)
                              readings
                              (cons (list (cdr (assq 'live-bytes stats))
                                          (cdr (assq 'bytes-trigger stats)))
                                    readings))))))))
        (cycle)
        (gc)
        (gc)
        (define base (stat 'live-bytes))
        (define floor (stat 'bytes-trigger))
        (list base floor (run 1000))
        "#;
    let answer = eval_program_vm_adaptive(code);
    let numbers: Vec<u64> = answer
        .split(|c: char| !c.is_ascii_digit())
        .filter(|field| !field.is_empty())
        .map(|field| field.parse().expect("a number"))
        .collect();
    let [base, floor, readings @ ..] = &numbers[..] else {
        panic!("expected the base L, the floor and the readings: {answer}")
    };
    assert_eq!(*floor, 8 << 20, "the adaptive floor: {answer}");
    for reading in readings.chunks(2) {
        let [live, threshold] = reading else {
            panic!("a reading is an L and a threshold: {answer}")
        };
        assert!(
            *live < base + (1 << 20),
            "a paced collection's L, {live}, counts the namespaces its code release dropped \
             (the L before the loop was {base}): {answer}"
        );
        assert_eq!(
            threshold, floor,
            "the threshold followed the garbage: {answer}"
        );
    }
    assert!(
        readings.len() >= 2 * 3,
        "fewer than three paced collections in 35 MB: {answer}"
    );
}

#[test]
fn a_collection_at_a_call_leaves_the_namespaces_its_code_release_drops_out_of_l() {
    // The same settle at `(gc)`'s collection, which runs at its call
    // (`collect_at_call`) and not at a poll. Fifty procedures, each compiled
    // in a namespace of its own, are held in a vector: the L a `(gc)` leaves
    // counts their namespaces, 1.6 MB of external bytes. Once the vector
    // lets them go, one `(gc)` frees the procedures and releases their
    // units, whose tables were the namespaces' last holders, before it
    // returns: `external-bytes` is down by the fifty namespaces, and the L
    // that `(gc)` left is what a second `(gc)` leaves, to the byte. The
    // sweep's L, read before the release, was the fifty namespaces above
    // it.
    //
    // Collecting only where `(gc)` asks, that `(gc)` is the collection
    // that finds them dead, as it is at the adaptive threshold, which 1.6
    // MB does not reach. In the environment's mode too: under the stress
    // lane's variable an earlier collection may find some of them dead, and
    // the readings hold wherever the collections fall.
    let code = r#"
        (import (scheme base) (scheme eval) (patina debug))
        (define (stat key) (cdr (assq key (gc-stats))))
        (define held (make-vector 50 #f))
        (define (fill!)
          (let loop ((i 0))
            (when (< i 50)
              (vector-set! held i
                           (eval '(lambda (x) (* x 2)) (environment '(scheme base))))
              (loop (+ i 1)))))
        (define l0 #f) (define e0 #f) (define l1 #f) (define e1 #f) (define l2 #f)
        (fill!)
        (vector-fill! held #f)
        (gc)
        (gc)
        (set! l0 (begin (fill!) (gc) (stat 'live-bytes)))
        (set! e0 (stat 'external-bytes))
        (set! l1 (begin (vector-fill! held #f) (gc) (stat 'live-bytes)))
        (set! e1 (stat 'external-bytes))
        (set! l2 (begin (gc) (stat 'live-bytes)))
        (list (> (- e0 e1) 1000000) (>= (- l0 l2) (- e0 e1)) (- l1 l2))
        "#;
    assert_eq!(eval_program_vm_gc_off(code), "(#t #t 0)");
    assert_eq!(eval_program_vm_adaptive(code), "(#t #t 0)");
    assert_eq!(eval_program_vm(code), "(#t #t 0)");
}

#[test]
fn a_bound_placeholder_is_never_dropped() {
    // The procedure `keep` holds reads `later` and `later2`, which nothing
    // binds when `eval` compiles it, so its unit links a placeholder for
    // each when it loads (#836): two binding records. While `keep` holds
    // the procedure, the program defines `later` and imports `later2`,
    // which adopt the two placeholders into the global namespace's slots,
    // keeping their words, so that the count does not move: a definition or
    // an import that made a record of its own would leave the placeholder
    // counted beside it, and one that left the entry in the namespace's
    // store a check build refuses after the sweep. Then `keep` lets the
    // procedure go: nothing links the two records any more, and a bound
    // record is the namespace's, so the collections keep both and the count
    // still does not move. The procedure is held across both bindings,
    // collections included, so under zeal the adoption is exercised with a
    // collection at every outermost safe point. Every variable but `later`
    // is defined before the first reading; `later`'s definition is the
    // adoption counted.
    assert_gc_and_zeal_eval_to(
        r#"
        (import (scheme base) (scheme eval) (scheme repl) (patina debug))
        (define-library (p4b9 lib)
          (export later2)
          (import (scheme base))
          (begin (define later2 'imported)))
        (define env (interaction-environment))
        (define (records) (cdr (assq 'binding-records (gc-stats))))
        (define keep #f) (define base #f) (define r0 #f) (define r1 #f) (define r2 #f)
        (gc) (gc)
        (set! base (records))
        (set! keep (eval '(lambda () (list later later2)) env))
        (set! r0 (records))
        (define later 1)
        (import (p4b9 lib))
        (gc) (gc)
        (set! r1 (records))
        (set! keep #f)
        (gc) (gc)
        (set! r2 (records))
        (list (- r0 base) (- r1 r0) (- r2 r1) later later2)
        "#,
        "(2 0 0 1 imported)",
    );
}

/// The controls of `release_dead_code`'s check (#836): a check build
/// asserts, after every collection, that each unit the collection kept had
/// its link table traced, since whatever keeps a unit is something that can
/// run its code and reports the unit's code id. patina-vm's test-only switch
/// (`test_support::SkipCodeIds`) has the collection pass over the ids of the
/// code one kind of keeper keeps, as a keeper of that kind that reported
/// nothing would leave them, so that the units it keeps go untraced and the
/// rest are traced: the first collection with the switch on panics with the
/// check's message. There is a control for each of the two ways a unit is
/// kept (`unit_in_use`), so that a check that covered only one of them, or
/// only some of the kept units, fails one. The program imports and defines
/// before the switch goes on, and runs `(gc)`, a form of its own, under it.
/// Declared after the interpreter, the switch is off again before the
/// interpreter drops in the unwind.
///
/// Run in every check build: every debug `cargo test`, and a release one
/// with this crate's `gc-check` feature, which CI's release GC lane runs
/// with `--include-ignored`. A build without the check reports them
/// ignored. No test in this file but the check builds' controls may have
/// `a_check_build_refuses` in its name, which CI's line filters on.
fn gc_with_tables_untraced(skip: fn() -> patina_vm::test_support::SkipCodeIds) {
    let interp = vm_interpreter();
    interp
        .eval_program_owned("(import (scheme base) (patina debug)) (define (g) 1)")
        .expect("import and define");
    let _skip = skip();
    let _ = interp.eval_program_owned("(gc)");
}

/// The units live closures keep go untraced: `g`'s, and the bootstrap's
/// libraries', whose closures they made. `(gc)`'s own unit, which its frame
/// and the running code keep, is traced, so a check that covered only the
/// units something holds passes here.
#[test]
#[cfg_attr(
    not(any(debug_assertions, feature = "gc-check")),
    ignore = "needs a check build"
)]
#[should_panic(expected = "did not trace its link table")]
fn a_check_build_refuses_a_unit_a_closure_keeps_untraced() {
    gc_with_tables_untraced(patina_vm::test_support::SkipCodeIds::kept_by_closures);
}

/// `(gc)`'s own unit, which its frame and the running code keep, is the one
/// kept unit left untraced; the units closures keep are traced, so a check
/// that covered only the units a live closure keeps passes here, as one that
/// looked at only the first kept unit would, unless that is `(gc)`'s.
#[test]
#[cfg_attr(
    not(any(debug_assertions, feature = "gc-check")),
    ignore = "needs a check build"
)]
#[should_panic(expected = "did not trace its link table")]
fn a_check_build_refuses_a_unit_a_frame_keeps_untraced() {
    gc_with_tables_untraced(patina_vm::test_support::SkipCodeIds::kept_by_frames);
}

/// The control of the placeholders a unit's link table keeps (#836): a
/// collection keeps every placeholder a kept unit's table links, since it
/// traces that table, and the pass at the sweep's start drops only the
/// placeholders nothing marked. `g`'s unit links `unbound-836-control`, a
/// name nothing binds, to a placeholder, which only `g`'s table marks.
/// patina-vm's test-only switch (`test_support::SkipLinkEntries`) has a
/// collection mark each table traced and pass over its entries, so that
/// `(gc)` drops the placeholder `g`'s live unit links and the sweep frees
/// its pairs, while `release_dead_code`'s check, which asks only whether
/// each kept unit's table was traced, stays quiet. `(g)` then reads through
/// the freed record, which a check build refuses: as a use after free, or,
/// once a later allocation has reused the slot, as a stale reference
/// (`heap::check`). Without the switch `(g)` raises an unbound-variable
/// error. The program defines `g` before the switch goes on, so that `g`'s
/// unit is linked first, and runs `(begin (gc) (g))` under it. Run in every
/// check build, as the controls above are.
#[test]
#[cfg_attr(
    not(any(debug_assertions, feature = "gc-check")),
    ignore = "needs a check build"
)]
fn a_check_build_refuses_a_placeholder_the_pass_dropped_while_linked() {
    let interp = vm_interpreter();
    interp
        .eval_program_owned(
            "(import (scheme base) (patina debug)) (define (g) unbound-836-control)",
        )
        .expect("import and define");
    let unbound = interp
        .eval_str_owned("(g)")
        .expect_err("`unbound-836-control` is unbound");
    assert!(
        format!("{unbound}").contains("unbound-836-control"),
        "{unbound}"
    );
    let _skip = patina_vm::test_support::SkipLinkEntries::new();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = interp.eval_str_owned("(begin (gc) (g))");
    }));
    let payload = caught.expect_err("a read through a placeholder the sweep freed");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|m| m.to_string()))
        .unwrap_or_default();
    assert!(
        message.starts_with("use-after-free: pair slot")
            || message.starts_with("stale pair reference"),
        "{message}"
    );
}
