//! Which sites decide "is this callable" — the rows that must stay in Rust.
//!
//! Most of this file is now `tests/scheme/control/callability.scm`, run by
//! `scheme_suite.rs` on both backends (#193 Phase 0). **The rationale and the
//! three rules this section encodes live there**, with the rows they govern;
//! restating them here is how two copies drift apart, and this file's history
//! is largely about claims that were true when written.
//!
//! What stayed is what a `.scm` file cannot express:
//!
//! - a row whose two backends give **different values**. #208 has since given
//!   each backend a `cond-expand` identifier, so this is now expressible in
//!   Scheme; these rows stay here until someone moves them deliberately, which
//!   is a change about divergences and not about file layout;
//! - rows deliberately asserted on **one backend**, for a reason that is not a
//!   disagreement about the answer;
//! - that an error escapes an **unguarded top-level program**, which is
//!   observable only from outside the program;
//! - `assert_program_eval_error_at`, which asserts *which stage* rejected the
//!   program.
//!
//! That is the line #193 predicted the migration would fall on: what is about
//! the *language* went to Scheme, what is about the *implementation* stayed.
//!
//! `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 carries the history.

mod common;
use common::{
    ErrorClass, assert_program_eval_error, assert_program_eval_error_at, assert_program_eval_to,
    eval_program_tree_walker, eval_program_vm,
};

/// The known limit, stated as behaviour: `procedure?` is *wider* than what the
/// sites requiring a procedure will accept.
///
/// A continuation answers `procedure?` with `#t`, yet `make-parameter` rejects
/// it as a converter — because callability is spelled two ways.
/// `Heap::is_procedure` covers closures, primitives, VM continuation refs and
/// (since the parameter fix) parameter objects, while tree-walker continuations
/// live in `Heap::is_continuation`. `Heap::is_callable` is the union, and the
/// sites that mean "any callable" now use it; the four that deliberately ask
/// the narrower question still reject a continuation. Folding `Continuation`
/// into `is_procedure` would widen those four — a behaviour change wanting its
/// own tests.
///
/// **Decided 2026-08-25**, when the VM's generic call path learned to invoke
/// a continuation (so `with-exception-handler` could take one, R7RS's idiom
/// for capturing a raised object): a continuation *is* a procedure for
/// `make-parameter` too — chibi and Gauche apply it to the initial value and
/// escape through it, answering `1`. The VM does the same, through `call_any`.
/// The tree-walker still rejects it, and deliberately so: its converter
/// runs as a direct-mode primitive callback, from which a continuation
/// cannot be invoked at all (PRD §6, "two continuation defects around
/// primitive callbacks"), so the clean rejection is the better of its two
/// answers until that is fixed. Not `assert_divergence` — the tree-walker
/// returns a value, not a failure.
/// The unguarded halves of the catchable-error pairs whose guarded halves are
/// in `tests/scheme/control/*.scm`.
///
/// **This is where a migrated file's unguarded rows land.** Phase 1 briefly
/// gave them a file of their own (`unguarded_errors.rs`); that split one class
/// across two homes that did not cross-reference each other, and since this
/// file exists either way it also meant a migration that deleted one binary
/// added one back — a net zero against the only thing #193 is trying to buy.
/// Adding a row here costs nothing.
///
/// They stayed in Rust because they assert something a `.scm` file cannot:
/// that the error escapes an **unguarded top-level program**. SRFI 64's
/// `test-error` runs its body inside `call/cc` and `with-exception-handler`,
/// which is the same routing the guarded row already checks — so migrating
/// these would have turned each pair into a near-duplicate, and a regression
/// that routed the errors to handlers while breaking the top-level path (the
/// exact shape of defect #71 the section cites) would pass both halves.
///
/// Routing changes *where* a catchable error is delivered, never whether it is
/// raised. Asserting only the guarded half would not notice a fix that
/// swallowed errors instead of routing them.
#[test]
fn a_control_primitive_error_still_escapes_an_unguarded_program() {
    for body in [
        "(with-exception-handler 5 (lambda () 'ok))", // handler is not a procedure
        "(dynamic-wind (lambda () 1))",               // arity
        "(call-with-values (lambda () 1))",           // arity
        "(raise)",                                    // arity
        "(error)",                                    // arity
        "((make-parameter 1) 1 2 3)",                 // a parameter's own arity
        "(error 5)",                                  // message is not a string
        "(error 'sym)",                               // ditto, the chibi-lenient shape
    ] {
        assert_program_eval_error(body);
    }
    // From `case_lambda.rs` when it migrated (#193 Phase 1): a `case-lambda`
    // call matching no clause. Its catchable half is
    // `tests/scheme/control/case-lambda.scm`'s "no clause matches the call";
    // this is the half a `.scm` file cannot state, so it lands in the file that
    // already owns the class rather than keeping a binary alive for one row.
    assert_program_eval_error(
        "(import (scheme case-lambda))
         (define f (case-lambda ((x) x) ((x y) (cons x y))))
         (f 1 2 3)",
    );

    // From `parameters.rs` when it migrated (#193 Phase 1): a non-parameter in
    // `parameterize`'s binding position. Guarded half — "a non-parameter in the
    // binding position is an error" in `tests/scheme/control/parameters.scm`.
    assert_program_eval_error(r#"(parameterize ((42 20)) (write-string "hello"))"#);

    // A non-string name for `get-environment-variable`. Guarded half — "a
    // non-string name raises an error object" in
    // `tests/scheme/stdlib/process-context.scm`.
    //
    // Not migrated from anywhere: that guarded row was *added* in #193 Phase 1,
    // from one the `.rs` file had commented out, and a new guarded row needs
    // its unguarded half as much as a migrated one does.
    assert_program_eval_error("(import (scheme process-context)) (get-environment-variable 123)");

    // Raising an error object whose irritants contain a **cycle**, uncaught.
    //
    // Third of the three paths the missing label arm aborted on, and the only
    // one that had never been covered anywhere: `circular_data.rs` asserted
    // `(write e)`, `tests/scheme/data/circular-data.scm` added `(display e)`
    // when it migrated (#193 Phase 1), and this is the diagnostic for an
    // uncaught raise — which is the top-level path, so no `.scm` file can
    // reach it.
    //
    // Asserting on `#0=` rather than merely on "it errored", because the two
    // are not the same claim: the backend error's `Display` could report an
    // unhandled exception without rendering the irritants at all, and the
    // weaker helper would pass while exercising none of the writer. Measured
    // 2026-09-07 — both backends put the label in the message — so the row
    // states it. A return to the unlabelled walk would abort the process
    // instead of failing here, which is the other half of why this is worth
    // stating separately from the two guarded halves.
    assert_program_eval_error_at(
        "(define xs (list 1))
         (define e (guard (c (#t c)) (error \"boom\" xs)))
         (set-car! xs e)
         (raise e)",
        ErrorClass::AtRuntime,
        ErrorClass::AtRuntime,
        "#0=",
    );
}

/// A non-breaking space is **not** an identifier character, and the reader
/// says so before the program runs.
///
/// From `unicode_identifiers.rs` when it migrated (#193 Phase 1). Everything
/// else in that file is now `tests/scheme/reader/unicode-identifiers.scm`;
/// this row could not go, because a lex error is not a raise — no `guard`
/// reaches it and `test-error` cannot state it. It is here rather than in a
/// file of its own for the reason the section above gives.
///
/// The claim is what the rule's *edge* is. Patina reads any character above
/// ASCII as an identifier constituent, and whitespace is the one exception —
/// without it a stray U+00A0 would silently weld two identifiers into one.
/// Measured 2026-09-07 on all three, chibi 0.12 / Gauche `gosh -r7` / Chez
/// `chez --script`: Patina is stricter than every reference here. chibi welds
/// `a<U+00A0>b` into the symbol `|a b|` and then reports it undefined, while
/// Gauche and Chez both split it and evaluate `3`.
///
/// `assert_program_eval_error_at` rather than `assert_program_eval_error`,
/// because the stage *is* the claim: an implementation that accepted the
/// character and failed later — chibi's answer — would satisfy the weaker
/// helper while having exactly the behaviour this row exists to reject.
#[test]
fn a_non_breaking_space_is_rejected_before_the_program_runs() {
    assert_program_eval_error_at(
        "(define a 1) (define b 2) (+ a\u{00A0}b)",
        ErrorClass::BeforeRun,
        ErrorClass::BeforeRun,
        "U+00A0",
    );
}

/// `parameterize` with no body is rejected **before the program runs**, so it
/// is not the same claim as the rows above and does not pair with anything.
///
/// R7RS §4.2.6 gives `parameterize` a ⟨body⟩, which requires at least one
/// expression; Patina rejects it while desugaring, and the diagnostic names
/// `lambda` rather than `parameterize` because that is what the expansion
/// builds — pinned here only in the part both backends share, since one says
/// `desugar error` and the other `Desugar error`.
///
/// It has no guarded half by construction rather than by choice: a program
/// that never compiles cannot reach a handler, which is exactly why the stage
/// is the assertion and why `test-error` in a `.scm` file could never have
/// stated this one.
#[test]
fn parameterize_with_no_body_is_rejected_before_the_program_runs() {
    assert_program_eval_error_at(
        "(define p (make-parameter 10)) (parameterize ((p 20)))",
        ErrorClass::BeforeRun,
        ErrorClass::BeforeRun,
        "body cannot be empty",
    );
}

#[test]
fn test_procedure_p_is_wider_than_the_sites_that_require_a_procedure() {
    assert_program_eval_to("(call/cc (lambda (k) (procedure? k)))", "#t");
    const CONVERTER: &str = r#"(call/cc (lambda (k)
             (guard (e (#t 'rejected)) (make-parameter 1 k) 'accepted)))"#;
    assert_eq!(
        eval_program_vm(CONVERTER),
        "1",
        "the VM matches chibi and Gauche; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_tree_walker(CONVERTER),
        "rejected",
        "\n[tree-walker] NO LONGER DIVERGES — a continuation converter is applied.\n\
         Replace both assertions with assert_program_eval_to(CONVERTER, \"1\") and \
         widen make-parameter's check to is_callable."
    );
    // The same site accepts an ordinary procedure, so the rejection above is
    // about which spelling of "callable" it uses, not about converters.
    assert_program_eval_to(
        "(guard (e (#t 'rejected)) (make-parameter 1 (lambda (x) x)) 'accepted)",
        "accepted",
    );
}

/// The rest of the VM's frameless call sites take a control primitive too.
///
/// `call_any` is one dispatcher with several callers, and the neighbouring
/// tests exercise the two whose answers both backends agree on. These are the
/// remainder, one program apiece, because "the same function serves them all"
/// is the kind of claim this file exists to distrust: the prompt body only
/// *became* a caller in issue #179, and inherited the hole in silence.
///
/// **VM-only assertions, and not because the tree-walker disagrees about the
/// answer.** Each of these names a control primitive in value position, which
/// the tree-walker resolves through a registry binding that is not there —
/// the hole `backend_divergence.rs::callcc_bound_with_define` and its two
/// neighbours already pin, still Q2 part 1's to fix. Pinning three more rows
/// of that one family here would just be three more things to collapse when
/// it lands.
///
/// Every row was measured against `main` at `30e0bd6` before the fix: each was
/// `Undefined variable: patina.internal.control/…`, or the `Internal error`
/// that name lookup becomes when it is a primitive callback that fails.
///
/// **A zero-argument call site cannot appear here, and that is not the same
/// as its being unaffected** — an earlier draft of this comment said "those
/// sites could never show the defect", which is false, and
/// [`a_wind_thunk_reaches_the_probe_it_cannot_satisfy`] below is the program
/// that falsifies it. A wind thunk and a `call-with-values` producer take no
/// arguments, and every control primitive but `values` requires at least one,
/// so none of them can *succeed* there. Reaching the probe and satisfying it
/// are different questions, and the arity is only an answer to the second.
#[test]
fn every_frameless_call_site_takes_a_control_primitive() {
    // `call/cc`'s own procedure argument, given `call/cc`.
    assert_eq!(eval_program_vm("(procedure? (call/cc call/cc))"), "#t");
    // A parameter converter, the one caller that must have its value
    // synchronously, so it runs a nested dispatch loop for a callee that
    // pushed a frame. The converter runs on the initial value too (R7RS 4.2.6).
    assert_eq!(
        eval_program_vm("(define q (make-parameter (lambda (k) 5) call/cc))\n(q)"),
        "5"
    );
    // A higher-order primitive's callback, which re-enters the VM from Rust:
    // `assoc`'s and `member`'s comparator. `(apply + '(1 2))` is 3, so the
    // first entry matches.
    assert_eq!(
        eval_program_vm("(assoc + (list (list '(1 2))) apply)"),
        "((1 2))"
    );
    assert_eq!(eval_program_vm("(member + (list '(1 2)) apply)"), "((1 2))");
}

/// A wind thunk does reach the probe — it just cannot satisfy it.
///
/// The row that makes the point the neighbour above gets wrong. `apply` as a
/// jump's `after` thunk goes through `push_wind_step` → `call_any` and is
/// called with **no arguments**, so it fails either way; *how* it fails is the
/// whole difference, and it is the same difference every other row in this
/// file shows:
///
/// ```text
///   main 30e0bd6 => Undefined variable: patina.internal.control/apply
///   with #186    => wrong number of arguments: expected at least 2, got 0
/// ```
///
/// The first is the name never resolving. The second is `apply` being reached,
/// recognised, and told it was called wrongly — which is what the tree-walker
/// has always said here, so the two backends now agree on the diagnosis and
/// not merely on the fact of failure.
///
/// The message is the assertion because nothing else can be: `values` is the
/// only control primitive a 0-argument call site can call successfully, and it
/// is in the registry, so it was found by the old probe too. That is the one
/// shape where this file's usual rule — never assert on error text — has no
/// alternative to fall back on, and the diagnosis is the behaviour under test.
#[test]
fn a_wind_thunk_reaches_the_probe_it_cannot_satisfy() {
    assert_program_eval_error_at(
        "(define k #f)\n\
         (define done #f)\n\
         (define v (call/cc (lambda (c) (set! k c) 0)))\n\
         (if (not done)\n\
         \x20   (begin (set! done #t)\n\
         \x20          (dynamic-wind (lambda () 'b) (lambda () (k 1)) apply)))\n\
         v",
        ErrorClass::AtRuntime,
        ErrorClass::AtRuntime,
        "number of arguments: expected at least 2, got 0",
    );
}
