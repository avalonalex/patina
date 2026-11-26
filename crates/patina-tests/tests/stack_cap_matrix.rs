//! The VM's stack cap and `&stack-exhausted` (#907, GC stage 4d.6b), row by
//! row, on both backends.
//!
//! Until 4d.6b nothing limited a recursion on the VM but the machine's
//! memory. Now the frame entry that takes the stack to within 1 MiB of its
//! cap (the soft limit) raises `&stack-exhausted`, non-continuably: an error
//! object, which `stack-exhausted?` from `(patina limits)` tells apart, with
//! the last MiB left for its handler. The owner ruled on each part on
//! 2026-11-24 (#782): what the condition is (Q5), what a handler that runs
//! past the cap gets (Q6: the form is abandoned, as by an uncaught error),
//! and when the soft limit is armed again (Q6: as soon as the stack top is
//! at or below it, with no margin).
//!
//! # How a row is scored
//!
//! Each row is a program whose recursion is bounded: 20,000 frames, which
//! is about 2.7 MiB of stack on the VM, past the 2 MiB cap the rows run
//! under ([`CAP`], the least a configuration can ask for). A row records
//! two answers:
//!
//! - **`capped`**: what the VM answers under the cap.
//! - **`free`**: what the program answers with no cap in the way. The
//!   tree-walker keeps none (decision A9 on #782), so its answer is checked
//!   against this one, and so is the VM's under a cap the recursion fits in
//!   ([`ROOMY`]). It is also what chibi 0.12.0, Gauche 0.9.15 and Chez
//!   Scheme 10.4.1 answer for the same recursion written without the
//!   condition's predicate, none of which stops at this depth (#907's
//!   "Today": chibi runs 204,687 frames, Gauche and Chez 10 M and more).
//!
//! No reference implementation raises a catchable condition for a deep
//! recursion, so the capped column has no oracle: it is the ruling, pinned.
//! The nearest neighbours are in the research record on #782: Python's
//! `RecursionError` for the class, Lua for re-arming at the catch with no
//! margin, chibi for abandoning without a handler.
//!
//! # What the rows cover
//!
//! #907's list: the condition caught by `guard`, by an escaping
//! `with-exception-handler` and by a handler that returns; uncaught; the
//! handler's room; the hard cap; a second catch in the same form, which the
//! "1 MiB below" re-arming this replaced would lose, since under a 2 MiB cap
//! every `guard` is within the reserve's distance of the soft limit; a
//! clause that crosses the limit again; a handler that aborts to a prompt
//! outside it, and to one of its own inside the reserve, directly and
//! through a wind; a `guard` that declines, whose re-entry of its handler
//! finds the reserve open again; after thunks on a `guard`'s escape and on the hard-cap path;
//! and a delimited continuation's re-entry.

mod common;

use patina_interpreter::{HeapConfig, Interpreter, TreeWalkInterpreter, TreeWalker};
use patina_runtime::Backend;
use patina_vm::VmBackend;

/// The cap the rows run under: 2 MiB, the floor, so the soft limit is 1 MiB
/// and a frame of `deep`'s is met about 7,700 deep.
const CAP: u64 = 2 << 20;

/// A cap every row's recursion fits under.
const ROOMY: u64 = 64 << 20;

/// How an error is rendered: its first line, after this.
const ERROR: &str = "<error>";

const PRELUDE: &str = "\
(import (scheme base) (patina limits) (patina debug))
;; A bounded non-tail recursion: 20,000 frames is past the 2 MiB cap.
(define (deep n) (if (= n 0) 0 (+ 1 (deep (- n 1)))))
;; `thunk` called with `n` frames below it, each waiting for its value.
(define (at-depth n thunk)
  (if (= n 0) (thunk) (car (list (at-depth (- n 1) thunk)))))
";

struct Row {
    name: &'static str,
    program: &'static str,
    /// The VM's answer under [`CAP`].
    capped: &'static str,
    /// The answer with no cap in the way.
    free: &'static str,
}

const ROWS: &[Row] = &[
    Row {
        name: "caught by guard, told apart by its predicate",
        program: "(guard (e ((stack-exhausted? e) 'exhausted)) (deep 20000))",
        capped: "exhausted",
        free: "20000",
    },
    Row {
        name: "an error object: its message, and the cap as its irritant",
        program: "(guard (e ((error-object? e)
            (list (error-object-message e) (error-object-irritants e))))
  (deep 20000))",
        capped: "(\"stack exhausted\" (2097152))",
        free: "20000",
    },
    Row {
        name: "caught by a handler that escapes",
        program: "(call/cc (lambda (k)
  (with-exception-handler
    (lambda (e) (k (if (stack-exhausted? e) 'escaped 'other)))
    (lambda () (deep 20000)))))",
        capped: "escaped",
        free: "20000",
    },
    Row {
        name: "a handler that returns: the raise is not continuable",
        program: "(guard (outer (#t (if (stack-exhausted? outer) 'the-same 'secondary)))
  (with-exception-handler
    (lambda (e) 'returned)
    (lambda () (deep 20000))))",
        capped: "secondary",
        free: "20000",
    },
    Row {
        name: "uncaught",
        program: "(deep 20000)",
        capped: "<error>: stack exhausted: the stack reached its limit, 1 MiB under its cap of 2097152 bytes (--stack-max or PATINA_STACK_MAX raises the cap)",
        free: "20000",
    },
    Row {
        name: "the handler's room: 3,000 frames of the last MiB",
        program: "(call/cc (lambda (k)
  (with-exception-handler
    (lambda (e) (k (list 'handler-ran (deep 3000))))
    (lambda () (deep 20000)))))",
        capped: "(handler-ran 3000)",
        free: "20000",
    },
    Row {
        name: "the hard cap: a handler that runs through the last MiB",
        program: "(with-exception-handler
  (lambda (e) (deep 20000))
  (lambda () (deep 20000)))",
        capped: "<error>: stack exhausted: a handler for the first exhaustion ran out of room in the last MiB under the cap of 2097152 bytes, and the form was abandoned",
        free: "20000",
    },
    Row {
        name: "a second catch in the same form",
        program: "(list (guard (e ((stack-exhausted? e) 'first)) (deep 20000))
      (guard (e ((stack-exhausted? e) 'second)) (deep 20000)))",
        capped: "(first second)",
        free: "(20000 20000)",
    },
    Row {
        name: "a clause just under the limit crosses it again: the next handler out",
        program: "(guard (outer ((stack-exhausted? outer) 'outer))
  (at-depth 5000
    (lambda ()
      (guard (inner ((stack-exhausted? inner) (deep 20000)))
        (deep 20000)))))",
        capped: "outer",
        free: "20000",
    },
    Row {
        name: "caught twice by a handler that aborts to a prompt",
        program: "(define tag (make-continuation-prompt-tag 'p))
(define (once)
  (call-with-continuation-prompt
    (lambda ()
      (with-exception-handler
        (lambda (e) (abort-current-continuation tag 'aborted))
        (lambda () (deep 20000))))
    tag
    (lambda (v k) v)))
(list (once) (once))",
        capped: "(aborted aborted)",
        free: "(20000 20000)",
    },
    Row {
        name: "a handler's own prompt, aborted to inside the reserve",
        program: "(define tag (make-continuation-prompt-tag 'p))
(guard (e ((stack-exhausted? e) 'a-second-condition))
  (call/cc
    (lambda (out)
      (with-exception-handler
        (lambda (e)
          (out (call-with-continuation-prompt
                 (lambda () (abort-current-continuation tag 'aborted))
                 tag
                 (lambda (v k) (deep 2000) v))))
        (lambda () (deep 20000))))))",
        capped: "aborted",
        free: "20000",
    },
    Row {
        name: "the same through a wind: the landing an abort travels to keeps the reserve",
        program: "(define tag (make-continuation-prompt-tag 'p))
(guard (e ((stack-exhausted? e) 'a-second-condition))
  (call/cc
    (lambda (out)
      (with-exception-handler
        (lambda (e)
          (out (call-with-continuation-prompt
                 (lambda ()
                   (dynamic-wind
                     (lambda () #f)
                     (lambda () (abort-current-continuation tag 'aborted))
                     (lambda () #f)))
                 tag
                 (lambda (v k) (deep 2000) v))))
        (lambda () (deep 20000))))))",
        capped: "aborted",
        free: "20000",
    },
    Row {
        name: "a guard that declines raises the same condition again, not a second one",
        program: "(let ((seen #f))
  (guard (outer ((stack-exhausted? outer)
                 (if (eq? outer seen) 'the-same-object 'another-object)))
    (guard (inner ((begin (set! seen inner) #f) 'never))
      (deep 20000))))",
        capped: "the-same-object",
        free: "20000",
    },
    Row {
        name: "an after thunk runs on the guard's escape, with the reserve",
        program: "(let ((log '()))
  (guard (e ((stack-exhausted? e) (reverse (cons 'caught log))))
    (dynamic-wind
      (lambda () (set! log (cons 'before log)))
      (lambda () (deep 20000))
      (lambda () (deep 2000) (set! log (cons 'after log))))))",
        capped: "(before after caught)",
        free: "20000",
    },
    Row {
        name: "a delimited continuation re-entered past the soft limit",
        program: "(define tag (make-continuation-prompt-tag 'p))
(define (deep-abort n)
  (if (= n 0) (abort-current-continuation tag 0) (+ 1 (deep-abort (- n 1)))))
(define k (call-with-continuation-prompt
            (lambda () (deep-abort 5000)) tag (lambda (v k) k)))
(guard (e ((stack-exhausted? e) 'exhausted))
  (at-depth 5000 (lambda () (k 0))))",
        capped: "exhausted",
        free: "5000",
    },
];

/// Run `code` after the prelude on `interp`, rendering a rejection as
/// [`ERROR`] and its first line.
fn answer<B: Backend>(interp: &Interpreter<B>, code: &str) -> String
where
    B::Error: std::fmt::Display,
{
    match interp.eval_program_owned(&format!("{PRELUDE}{code}")) {
        Ok(value) => interp.display_tagged(&value),
        Err(e) => {
            let message = e.to_string();
            let first = message.lines().next().unwrap_or("").trim();
            format!("{ERROR}: {first}")
        }
    }
}

fn vm(cap: u64) -> Interpreter<VmBackend> {
    Interpreter::new(VmBackend::with_config(HeapConfig::new().max_stack(cap)))
}

fn vm_zeal(cap: u64) -> Interpreter<VmBackend> {
    Interpreter::new(VmBackend::with_gc_mode_and_config(
        patina_core::GcMode::Zeal,
        HeapConfig::new().max_stack(cap),
    ))
}

/// An uncaught condition's first line carries a source location on some
/// paths and not on others; the rows pin the message and not where it was
/// reported from.
fn matches(got: &str, want: &str) -> bool {
    match want.strip_prefix(ERROR) {
        Some(rest) => got
            .strip_prefix(ERROR)
            .is_some_and(|got| got.contains(rest.trim_start_matches(": "))),
        None => got == want,
    }
}

fn score(make: fn(u64) -> Interpreter<VmBackend>, tree_walker: fn() -> TreeWalkInterpreter) {
    let mut moved = Vec::new();
    for row in ROWS {
        let capped = answer(&make(CAP), row.program);
        if !matches(&capped, row.capped) {
            moved.push(format!(
                "{}: the VM under the cap answers {capped:?}, recorded {:?}",
                row.name, row.capped
            ));
        }
        let roomy = answer(&make(ROOMY), row.program);
        if roomy != row.free {
            moved.push(format!(
                "{}: the VM with room answers {roomy:?}, recorded {:?}",
                row.name, row.free
            ));
        }
        let walked = answer(&tree_walker(), row.program);
        if walked != row.free {
            moved.push(format!(
                "{}: the tree-walker answers {walked:?}, recorded {:?}",
                row.name, row.free
            ));
        }
    }
    assert!(
        moved.is_empty(),
        "{} moved:\n{}",
        moved.len(),
        moved.join("\n")
    );
}

/// Every row, on the VM under the cap and with room, and on the
/// tree-walker.
#[test]
fn rows_score_as_recorded() {
    score(vm, common::tree_walker_interpreter);
}

/// Three rows with a collection at every outermost safe point: a
/// collection is one of the places the soft limit is armed again, and the
/// handler's frames sit above the limit while one runs. Only three, and
/// only under the cap: a collection at each of 7,700 nested calls scans
/// thirty million frames, and with room the recursion is 20,000 deep.
#[test]
fn three_rows_score_as_recorded_under_zeal() {
    let under_zeal = [
        "caught by guard, told apart by its predicate",
        "the handler's room: 3,000 frames of the last MiB",
        "a second catch in the same form",
    ];
    for name in under_zeal {
        let row = ROWS
            .iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("no row named {name:?}"));
        let capped = answer(&vm_zeal(CAP), row.program);
        assert!(matches(&capped, row.capped), "{name}: {capped:?}");
    }
}

/// The check counts what 4d.7's interleaved stack will hold, 40 bytes a
/// frame and 8 a register: `deep`'s frame is met about 7,700 deep under the
/// 2 MiB cap, whose soft limit is 1 MiB, and twice as deep, less the fixed
/// megabyte, under 3 MiB.
#[test]
fn the_limit_is_met_where_the_bytes_say() {
    let depth_at = |cap: u64| -> i64 {
        let interp = vm(cap);
        let reached = answer(
            &interp,
            "(define reached 0)
(define (count n) (set! reached n) (+ 1 (count (+ n 1))))
(guard (e ((stack-exhausted? e) reached)) (count 0))",
        );
        reached.parse().unwrap_or_else(|_| panic!("{reached}"))
    };
    let at_two = depth_at(2 << 20);
    let at_three = depth_at(3 << 20);
    // A megabyte of `count`'s frames, 136 bytes each today (a 40-byte
    // header and twelve registers): 7,706 deep. The range allows a register
    // more or less, and not a frame counted without its header (10,900) or
    // with two (6,000).
    assert!((7_000..=8_400).contains(&at_two), "{at_two}");
    // The soft limit doubles, from 1 MiB to 2: so does the depth, within a
    // frame or two of the first form's own.
    assert!(
        (at_three - 2 * at_two).abs() <= 4,
        "{at_two} then {at_three}"
    );
}

/// After the hard cap the interpreter goes on: the form is abandoned as by
/// an uncaught error, the stack is empty, the soft limit is armed again, and
/// the next form's exhaustion is caught like the first.
#[test]
fn the_interpreter_goes_on_after_the_hard_cap() {
    let interp = vm(CAP);
    let lost = answer(
        &interp,
        "(with-exception-handler (lambda (e) (deep 20000)) (lambda () (deep 20000)))",
    );
    assert!(lost.contains("ran out of room"), "{lost}");
    for _ in 0..3 {
        let again = interp
            .eval_program_owned("(guard (e ((stack-exhausted? e) 'again)) (deep 20000))")
            .map(|value| interp.display_tagged(&value))
            .unwrap_or_else(|e| e.to_string());
        assert_eq!(again, "again");
    }
    let fits = interp
        .eval_program_owned("(deep 5000)")
        .map(|value| interp.display_tagged(&value))
        .unwrap_or_else(|e| e.to_string());
    assert_eq!(fits, "5000");
}

/// On the hard-cap path no after thunk runs, as none runs when an uncaught
/// error abandons a form today: whether an abandoned form should run them
/// is #776's question, and this path will follow that ruling, since it is
/// the uncaught error's path. Pinned so that the answer moves with it.
#[test]
fn the_hard_cap_runs_no_after_thunk() {
    let interp = vm(CAP);
    let lost = answer(
        &interp,
        "(define log '())
(dynamic-wind
  (lambda () (set! log (cons 'before log)))
  (lambda ()
    (with-exception-handler (lambda (e) (deep 20000)) (lambda () (deep 20000))))
  (lambda () (set! log (cons 'after log))))",
    );
    assert!(lost.contains("ran out of room"), "{lost}");
    let log = interp
        .eval_program_owned("log")
        .map(|value| interp.display_tagged(&value))
        .unwrap_or_else(|e| e.to_string());
    assert_eq!(log, "(before)");
}

/// A re-entry that would not fit under the cap is refused before it is
/// appended, and raises the catchable condition: under a 4 MiB cap, a
/// region of 16,000 frames, about 2.2 MiB, re-entered with 16,000 frames
/// already below it. Nothing has crossed the 3 MiB soft limit, so this is
/// the first exhaustion, not the hard cap.
#[test]
fn a_reentry_that_would_not_fit_raises_the_condition() {
    let interp = vm(4 << 20);
    let caught = answer(
        &interp,
        "(define tag (make-continuation-prompt-tag 'p))
(define (deep-abort n)
  (if (= n 0) (abort-current-continuation tag 0) (+ 1 (deep-abort (- n 1)))))
(define k (call-with-continuation-prompt
            (lambda () (deep-abort 16000)) tag (lambda (v k) k)))
(list (guard (e ((stack-exhausted? e) 'exhausted))
        (at-depth 16000 (lambda () (k 0))))
      ;; Re-entered with room, the same region returns its count.
      (k 0))",
    );
    assert_eq!(caught, "(exhausted 16000)");
}

/// A re-entry refused under the soft limit raises the condition with the
/// limit still armed: its handler runs below the limit, and when it goes on
/// to cross it, that is a new catchable condition for the next handler out,
/// not the hard cap.
#[test]
fn a_reentry_refused_under_the_soft_limit_leaves_it_armed() {
    let interp = vm(4 << 20);
    let got = answer(
        &interp,
        "(define tag (make-continuation-prompt-tag 'p))
(define (deep-abort n)
  (if (= n 0) (abort-current-continuation tag 0) (+ 1 (deep-abort (- n 1)))))
(define k (call-with-continuation-prompt
            (lambda () (deep-abort 16000)) tag (lambda (v k) k)))
(guard (outer ((stack-exhausted? outer) 'a-second-condition))
  (at-depth 16000
    (lambda ()
      (with-exception-handler
        (lambda (e) (deep 40000))
        (lambda () (k 0))))))",
    );
    assert_eq!(got, "a-second-condition");
}

/// A primitive that calls back on a nested loop: the internal `member`
/// with a comparator (`AGENTS.md`, "A procedure that calls back into the
/// program"). The handler stack is the machine's, so the condition raised
/// inside the callback reaches a handler installed outside the primitive's
/// call as `&stack-exhausted`, with its irritant.
#[test]
fn the_condition_reaches_a_handler_outside_a_nested_loop() {
    let got = answer(
        &vm(CAP),
        "(import (prefix (only (patina internal lists) member) internal:))
(guard (e ((stack-exhausted? e) (list 'kept-its-kind (error-object-irritants e))))
  (internal:member 1 '(1) (lambda (a b) (deep 20000))))",
    );
    assert_eq!(got, "(kept-its-kind (2097152))");
}

/// The hard cap met inside such a callback abandons the form all the same:
/// a `guard` outside the primitive's call does not catch it.
#[test]
fn the_hard_cap_inside_a_nested_loop_is_not_caught() {
    let interp = vm(CAP);
    let lost = answer(
        &interp,
        "(import (prefix (only (patina internal lists) member) internal:))
(guard (e (#t 'caught-the-hard-cap))
  (with-exception-handler
    (lambda (e) (internal:member 1 '(1) (lambda (a b) (deep 20000))))
    (lambda () (deep 20000))))",
    );
    assert!(lost.contains("ran out of room"), "{lost}");
    // And the next form starts afresh.
    let again = interp
        .eval_program_owned("(guard (e ((stack-exhausted? e) 'again)) (deep 20000))")
        .map(|value| interp.display_tagged(&value))
        .unwrap_or_else(|e| e.to_string());
    assert_eq!(again, "again");
}

/// `stack-exhausted?` recognises that condition and nothing else: an
/// ordinary error object, a raised datum, a primitive's error and values
/// that are no condition answer `#f`, on both backends. The tree-walker
/// keeps no cap and never raises the condition, and imports the library
/// all the same.
#[test]
fn the_predicate_answers_false_for_anything_else() {
    const PROGRAM: &str = "(list (stack-exhausted? (guard (e (#t e)) (error \"ordinary\" 1)))
      (stack-exhausted? (guard (e (#t e)) (raise 'a-datum)))
      (stack-exhausted? (guard (e (#t e)) (vector-ref (vector) 1)))
      (stack-exhausted? \"stack exhausted\")
      (stack-exhausted? 2097152))";
    let want = "(#f #f #f #f #f)";
    assert_eq!(answer(&vm(CAP), PROGRAM), want);
    assert_eq!(answer(&common::tree_walker_interpreter(), PROGRAM), want);
}

/// `(gc-stats)`'s `stack-limit` is the cap in bytes on the VM, the
/// configuration's or the machine's default, and 0 on the tree-walker,
/// which keeps none; and a configuration under the floor is raised to it.
#[test]
fn stack_limit_reads_the_cap() {
    const KEY: &str = "(cdr (assq 'stack-limit (gc-stats)))";
    assert_eq!(answer(&vm(CAP), KEY), "2097152");
    assert_eq!(answer(&vm(48 << 20), KEY), (48u64 << 20).to_string());
    assert_eq!(answer(&vm(1), KEY), "2097152");
    let default = answer(&common::vm_interpreter(), KEY);
    let expected = match HeapConfig::try_from_env() {
        Ok(config) => config.stack_cap(),
        Err(error) => panic!("{error}"),
    };
    assert_eq!(default, expected.to_string());
    assert_eq!(answer(&common::tree_walker_interpreter(), KEY), "0");
    // The tree-walker takes a configuration and keeps no cap.
    let walker = Interpreter::new(TreeWalker::with_config(HeapConfig::new().max_stack(CAP)));
    assert_eq!(answer(&walker, KEY), "0");
    assert_eq!(answer(&walker, "(deep 20000)"), "20000");
}

/// The default cap holds what runs today (#907's acceptance): a recursion
/// 10 M deep, 1.36 GB of stack as the cap counts it (136 bytes a frame),
/// where the machine's default is at least 1.5 GiB, as it is on both CI
/// runners (a quarter of 7 GB is 1.75 GB). On a machine whose default is
/// under 1 GiB the same recursion meets the cap and is caught; between the
/// two, which of them happens turns on the frame's exact size, and either
/// is right.
#[test]
fn the_default_cap_holds_a_ten_million_frame_recursion() {
    let cap = HeapConfig::new().stack_cap();
    let interp = Interpreter::new(VmBackend::with_config(&HeapConfig::new()));
    let got = answer(
        &interp,
        "(guard (e ((stack-exhausted? e) 'exhausted)) (deep 10000000))",
    );
    if cap >= 3 << 29 {
        assert_eq!(got, "10000000", "under a default cap of {cap} bytes");
    } else {
        eprintln!("the default cap here is {cap} bytes: the recursion may meet it");
        if cap < 1 << 30 {
            assert_eq!(got, "exhausted", "under a default cap of {cap} bytes");
        } else {
            assert!(
                got == "10000000" || got == "exhausted",
                "{got}, under a default cap of {cap} bytes"
            );
        }
    }
}

/// `(gc-stats)`'s `committed-stack-bytes` (#909, GC stage 4d.8), read with
/// primitives alone, so that the read enters no closure.
const COMMITTED: &str = "(cdr (assq 'committed-stack-bytes (gc-stats)))";

/// A recursion's pages go back at the second collection after it returns
/// (#909): 300,000 frames are 40.8 MB of stack, held after the return and
/// after one `(gc)`, and all but the first MiB given back by the second.
/// The stack is granted room in steps of 256 KiB, so a machine that has
/// run little holds a whole number of them, inside the first MiB. The
/// program collects before it recurses, so that no paced collection falls
/// between the return and the first `(gc)`, where it would be the first of
/// the two observations.
#[test]
fn a_recursions_pages_go_back_at_the_second_collection() {
    let program = format!(
        "(let* ((start (begin (gc) {COMMITTED}))
       (depth (deep 300000))
       (grown {COMMITTED})
       (one (begin (gc) {COMMITTED}))
       (two (begin (gc) {COMMITTED}))
       (three (begin (gc) {COMMITTED})))
  (list (and (< 0 start 1048577) (modulo start 262144)) depth
        (>= grown 40800000) (< grown 41200000) (= one grown) two three))"
    );
    assert_eq!(
        answer(&vm(ROOMY), &program),
        "(0 300000 #t #t #t 1048576 1048576)"
    );
}

/// The hysteresis (#909): a program that recurses to the same depth
/// between every two collections keeps its pages. Ten cycles of a
/// recursion and a `(gc)` read the same committed bytes after each
/// collection, where giving back at every collection would read the first
/// MiB.
#[test]
fn a_recursion_in_every_cycle_keeps_its_pages() {
    let program = format!(
        "(begin (gc)
  (let loop ((i 0) (seen '()))
    (if (= i 10)
        (list (length seen) (apply = seen) (>= (car seen) 40800000))
        (begin (deep 300000) (gc) (loop (+ i 1) (cons {COMMITTED} seen))))))"
    );
    assert_eq!(answer(&vm(ROOMY), &program), "(10 #t #t)");
}

/// A stack that a `guard` catching `&stack-exhausted` leaves behind goes
/// back the same way: the cap's 2 MiB are held when the condition is
/// caught, and the first MiB after two collections. And the tree-walker,
/// which keeps no such stack, reads 0.
#[test]
fn committed_stack_bytes_after_the_cap_and_on_the_tree_walker() {
    let program = format!(
        "(let* ((caught (guard (e ((stack-exhausted? e) 'exhausted)) (deep 20000)))
       (held {COMMITTED})
       (after (begin (gc) (gc) {COMMITTED})))
  (list caught (> held 1048576) (<= held 2097152) after))"
    );
    assert_eq!(answer(&vm(CAP), &program), "(exhausted #t #t 1048576)");
    let walker = common::tree_walker_interpreter();
    assert_eq!(answer(&walker, COMMITTED), "0");
    assert_eq!(
        answer(
            &walker,
            &format!("(begin (deep 20000) (gc) (gc) {COMMITTED})")
        ),
        "0"
    );
}

/// Row 4d's gate, per PR (#910; the nightly's `stack-returned` row holds it
/// at 10 M frames on the release build): a recursion 1 M deep under the
/// default cap, 136 MB of stack, whose pages go back at the second
/// collection after it returns. The key then reads the first MiB, which
/// is never given back, and `stack-limit` the cap the machine was built
/// with. It collects before it recurses, as the test of the second
/// collection above does and for its reason. Where the default cap is under
/// 256 MiB, which is a machine with under 1 GiB of memory for it, the
/// recursion may not fit, and the test says so and stops.
#[test]
fn a_million_frames_under_the_default_cap_go_back_after_two_collections() {
    let cap = HeapConfig::new().stack_cap();
    if cap < 256 << 20 {
        eprintln!("the default cap here is {cap} bytes: 1 M frames may not fit");
        return;
    }
    let interp = Interpreter::new(VmBackend::with_config(&HeapConfig::new()));
    let program = format!(
        "(let* ((depth (begin (gc) (deep 1000000)))
       (grown {COMMITTED})
       (one (begin (gc) {COMMITTED}))
       (two (begin (gc) {COMMITTED})))
  (list depth (>= grown 136000000) (= one grown) two
        (cdr (assq 'stack-limit (gc-stats)))))"
    );
    assert_eq!(
        answer(&interp, &program),
        format!("(1000000 #t #t 1048576 {cap})")
    );
}
