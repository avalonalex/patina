//! Which sites decide "is this callable", and what they decide.
//!
//! This file exists because prose about those sites kept being wrong. Reviewing
//! the `procedure?`-on-parameters fix turned up three claims in its own commit
//! message that no test could have contradicted: that `dynamic-wind` validates
//! its arguments (it does not), that a certain count of call sites "became
//! correct together" (it was counting grep hits, not decisions), and that
//! `Heap::is_procedure` had become the single source of truth for callability
//! (it has not). Each was a statement about observable behaviour, so each is
//! pinned below.
//!
//! The rule these encode: a claim about *which* check runs is testable by
//! ordering or by what is accepted, without depending on error text — error
//! messages are not a stable interface, and two sites here share one message
//! verbatim.
//!
//! `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 carries the history.

mod common;
use common::{
    ErrorClass, On, assert_divergence, assert_program_eval_error, assert_program_eval_to,
};

/// `dynamic-wind` performs no up-front validation on either backend — neither
/// `apply_dynamic_wind` nor `VmControlPrimitive::DynamicWind` inspects its
/// arguments, they just call the thunks.
///
/// Proven by ordering rather than by an error message: if the arguments were
/// checked first, nothing would have run. Because `before` and `body` both run
/// before the bad after-thunk is reached, the failure is a call, not a check.
///
/// Why it is worth a test: a draft of the parameter fix cited `dynamic-wind` as
/// one of the sites that "became correct" when the callability predicate
/// widened, and wrote a test to prove it. That test passed against unfixed code,
/// because `dynamic-wind` accepts anything until it tries to call it.
#[test]
fn test_dynamic_wind_does_not_validate_its_arguments() {
    assert_program_eval_to(
        r#"(define log '())
           (guard (e (#t (reverse log)))
             (dynamic-wind (lambda () (set! log (cons 'before log)))
                           (lambda () (set! log (cons 'body log)) 'ok)
                           5))"#,
        "(before body)",
    );
}

/// `with-exception-handler`, by contrast, *is* a real decision point: it
/// rejects a non-procedure instead of discovering the problem when it calls.
/// This is the check that a parameter object failed before `procedure?` was
/// fixed, on both backends.
#[test]
fn test_with_exception_handler_validates_its_arguments() {
    assert_program_eval_error("(with-exception-handler 5 (lambda () 'ok))");
    assert_program_eval_error("(with-exception-handler (lambda (e) e) 5)");
    // And it accepts what `procedure?` accepts — the property the parameter
    // fix restored. `tests/parameters.rs` covers the parameter case.
    assert_program_eval_to(
        "(with-exception-handler (lambda (e) e) (lambda () 'ok))",
        "ok",
    );
}

/// The known limit, stated as behaviour: `procedure?` is *wider* than what the
/// sites requiring a procedure will accept.
///
/// A continuation answers `procedure?` with `#t`, yet `make-parameter` rejects
/// it as a converter — because callability is spelled two ways.
/// `Heap::is_procedure` covers closures, primitives, VM continuation refs and
/// (since the parameter fix) parameter objects, while tree-walker continuations
/// live in `Heap::is_continuation`; five callers ask for the disjunction and
/// four ask only the first. Folding `Continuation` into `is_procedure` would
/// close it, at the cost of widening the four — a behaviour change wanting its
/// own tests.
///
/// **This test is meant to fail when that happens.** When it does, the fix is
/// not to adjust the expectation: it is to make `make-parameter` accept the
/// continuation, then update this test, the `is_procedure` doc comment, and the
/// PRD entry that all currently describe the enumeration as incomplete.
#[test]
fn test_procedure_p_is_wider_than_the_sites_that_require_a_procedure() {
    assert_program_eval_to("(call/cc (lambda (k) (procedure? k)))", "#t");
    assert_program_eval_to(
        r#"(call/cc (lambda (k)
             (guard (e (#t 'rejected)) (make-parameter 1 k) 'accepted)))"#,
        "rejected",
    );
    // The same site accepts an ordinary procedure, so the rejection above is
    // about which spelling of "callable" it uses, not about converters.
    assert_program_eval_to(
        "(guard (e (#t 'rejected)) (make-parameter 1 (lambda (x) x)) 'accepted)",
        "accepted",
    );
}

/// Found while pinning the test above: `with-exception-handler`'s own type
/// error is catchable on the VM but escapes `guard` on the tree-walker. It is
/// why the test above asserts rejection with `assert_program_eval_error`
/// rather than catching it.
///
/// Same class as the unbound-variable defect fixed in #71 — a catchable error
/// returned as a Rust `Err` instead of routed through the Scheme handlers —
/// but in `cps_eval/application.rs`, which #71's `try_catchable!` sweep of
/// `step.rs` never reached.
///
/// **This row is one representative of six.** `dynamic-wind`,
/// `call-with-values`, `raise`, `error` and a parameter's own arity error all
/// escape the same way; the PRD entry carries the measured table. They are not
/// quarantined individually because one sweep of the file should close them
/// together — and quarantining six rows that a single change retires would be
/// noise. Do not read this single row as the size of the defect.
#[test]
fn test_with_exception_handler_type_error_catchability_diverges() {
    assert_divergence(
        "(guard (e (#t 'caught)) (with-exception-handler 5 (lambda () 'ok)))",
        On::Vm,
        "caught",
        ErrorClass::AtRuntime,
        "PRD/TRACK_L_SNOW_LIBRARIES_PRD.md §6",
    );
}
