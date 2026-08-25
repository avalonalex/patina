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
//! A second rule, learned the same way: "the other backend does X" is not a
//! reason to believe X — the convergence test below was checked against chibi,
//! Gauche and Chez before the tree-walker was changed to agree with the VM.
//!
//! A third: a defect class cannot be enumerated by grep. The first sweep of
//! `cps_eval/application.rs` matched `return Err(…)` and missed three
//! catchable errors that reach Rust through `?`.
//!
//! `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 carries the history.

mod common;
use common::{
    ErrorClass, On, assert_divergence, assert_program_eval_error, assert_program_eval_to,
    eval_program_tree_walker, eval_program_vm,
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
///
/// **This is stricter than chibi and Gauche**, deliberately. Both accept a
/// non-procedure handler and just run the thunk, returning `ok` — they only
/// care when an exception is actually raised and the handler is called. Chez
/// rejects it as Patina does. R7RS leaves the case unspecified, so this is a
/// choice, not conformance; it is recorded here so nobody "fixes" Patina to
/// match chibi without knowing Chez sits on the other side.
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

/// Converged 2026-08-15: an error raised *by a control primitive itself* is a
/// catchable condition, in every position, on both backends.
///
/// Same class as #71 — a catchable error returned as a Rust `Err` instead of
/// routed through the Scheme handlers — in the file that fix did not reach.
///
/// Checked against chibi, Gauche and Chez first. They differ on whether some
/// of these should raise *at all*, never on catchability once something is
/// raised — so a future change here is about the former, not the latter. The
/// PRD carries the measured table.
///
/// Each body is asserted twice: caught when guarded, and *still an error* when
/// not. Routing changes where a catchable error is delivered, never whether it
/// is raised, and asserting only the first half would not notice a fix that
/// swallowed errors instead of routing them.
///
/// Two of these rows exist because the first sweep missed them. It was defined
/// syntactically — "every bare `return Err(…)`" — while three catchable errors
/// in the same file reach Rust through `?` instead. `(error 5)` is one:
/// fifteen lines below an arity check the sweep did route, in the same
/// function. Grep patterns are not a way to enumerate a defect class.
#[test]
fn test_a_control_primitive_error_is_catchable_in_every_position() {
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
        assert_program_eval_to(&format!("(guard (e (#t 'caught)) {body})"), "caught");
        assert_program_eval_error(body);
    }
}

/// The class is narrowed, not closed. An error raised by user code inside a
/// `dynamic-wind` thunk still escapes `guard` on the tree-walker, because
/// `run_wind_handlers(…)?` propagates it as a Rust error.
///
/// The depth is one level below that `?`: `apply_from_direct_tagged`
/// (`cps_eval/wind.rs`) runs wind thunks on a *nested trampoline* that starts
/// with an empty handler stack, so the error has to come back through Rust to
/// reach the handlers installed outside. Routing at the `?` is what the
/// parameter-converter case in this PR does and it works, but the general fix
/// is to thread the handler stack into the nested trampoline. Tracked in the
/// PRD; see `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.
///
/// The pinned value is what the VM returns, *not* an established correct
/// answer: chibi loops forever on this program, so no reference was available
/// to arbitrate. What this row asserts is only that the two backends disagree
/// — the tree-walker dies where the VM does not. Establish the right answer
/// before converging them.
///
/// The VM's half moved from `handled` to `caught` with the audit's A3 fix
/// (wind records are popped before their after-thunk runs). Both are
/// unarbitrated, but `caught` is the explicable one: the after-thunk's own
/// `(car 7)` error reaches the enclosing `guard`, which is where an error
/// raised during unwinding should land. `handled` came from the old ordering
/// swallowing that error and letting the original `raise` reach the handler
/// — and chibi calls a handler returning from a non-continuable `raise` an
/// error in its own right (`(with-exception-handler (lambda (c) 'handled)
/// (lambda () (raise 'x)))` answers `caught` there, where the VM still
/// answers `handled`).
#[test]
fn test_an_error_inside_a_wind_thunk_still_escapes_on_the_tree_walker() {
    assert_divergence(
        r#"(guard (e (#t 'caught))
             (with-exception-handler (lambda (c) 'handled)
               (lambda ()
                 (dynamic-wind (lambda () 1)
                               (lambda () (raise 'x))
                               (lambda () (car 7))))))"#,
        On::Vm,
        "caught",
        ErrorClass::AtRuntime,
        "PRD/TRACK_L_SNOW_LIBRARIES_PRD.md §6",
    );
}

// ─── `apply`'s callee set is `Call`'s callee set ─────────────────────────────

/// `apply` used as a value, which is what started this section.
///
/// The desugarer intercepts `apply` in head position and lowers it to a
/// dedicated instruction, so `(apply f xs)` never consults the binding. Reached
/// any other way — through a variable, an argument, a higher-order procedure —
/// it resolved to the `apply` that `(patina internal control)` exports, and the
/// VM then dispatched it through the primitive registry, where nothing
/// implements it: spreading a list into a real call is work only the VM can do.
/// So the VM reported `Undefined variable: patina.internal.control/apply` while
/// the tree-walker, which intercepts `apply` by name at call time, answered 6.
#[test]
fn test_apply_is_callable_as_a_value() {
    assert_program_eval_to("(let ((f apply)) (f + '(1 2 3)))", "6");
    // Through a higher-order procedure, the shape real code hits.
    assert_program_eval_to("(map (lambda (f) (f + '(1 2))) (list apply))", "(3)");
    // In tail position, which takes a different dispatcher.
    assert_program_eval_to("(define (call-it g) (g + '(7 8))) (call-it apply)", "15");
    // Fixed arguments before the spread list.
    assert_program_eval_to("(let ((f apply)) (f + 1 2 '(3 4)))", "10");
}

/// The deeper half of the same fix, and the reason it is here rather than in a
/// test named after `apply`: both apply instructions probed only
/// primitive → parameter → closure, so *`apply`'s* idea of what is callable was
/// narrower than `Call`'s. Every callee below is accepted by a direct call and
/// was rejected through `apply`.
///
/// Scoped to the two apply *instructions* on purpose — see
/// `test_apply_through_call_with_values_is_still_broken_on_the_vm` for the
/// dispatcher this does not cover.
///
/// Verified against chibi and Gauche, which accept all of them.
#[test]
fn test_apply_instructions_accept_every_callee_a_direct_call_accepts() {
    // A VM-intercepted control primitive.
    assert_program_eval_to(
        "(apply with-exception-handler
                (list (lambda (e) 43) (lambda () (raise-continuable 'x))))",
        "43",
    );
    assert_program_eval_to(
        "(define r '())
         (apply dynamic-wind
                (list (lambda () (set! r 1)) (lambda () 2) (lambda () (set! r 3))))",
        "2",
    );
    // `apply` itself is one of them, so this is also the self-application case.
    assert_program_eval_to("(apply apply (list + '(1 2)))", "3");
    // A parameter object — the one callee kind the old code already handled,
    // and covered on its own in `parameters.rs`. Kept to complete the set.
    assert_program_eval_to("(define p (make-parameter 5)) (apply p '())", "5");
}

/// A continuation reached through `apply`, on both backends.
///
/// Kept separate because it is easy to conflate with the one case still
/// failing on the tree-walker, and I did conflate them: this was first written
/// as a pinned divergence, and `assert_divergence` rejected it. What the
/// tree-walker still fails is `(apply call/cc …)` — `call/cc` *as apply's
/// callee*, resolved by name in value position — not a continuation object,
/// which it invokes here fine. That one is pinned in
/// `backend_divergence.rs::apply_callcc`.
#[test]
fn test_apply_invokes_a_continuation() {
    assert_program_eval_to("(call/cc (lambda (k) (let ((f apply)) (f k '(42)))))", "42");
}

/// The hole the fix above does *not* close, pinned so the claim stays honest.
///
/// `apply` reached through `call_any` — the VM's third and narrowest dispatcher
/// — still fails. `call_any` kept the exact primitive → parameter → closure
/// probe that the apply instructions shed, and it is what runs
/// `call-with-values`' consumer, prompt handlers and exception handlers. So the
/// callee set is uniform across the two apply *instructions* and not yet across
/// the VM.
///
/// Found by review, not by the tests: the first version of this work claimed
/// "`apply` accepts every callee a direct call accepts", and a five-token
/// program falsified it — with the same error string the change had just
/// declared fixed, one dispatcher over.
///
/// Not in `backend_divergence.rs` because the backends do not *disagree* about
/// what is right here: the tree-walker and chibi both answer 3, and only the VM
/// is wrong. The fix is to give `call_any` `call_value`'s probe set, which
/// needs an `exit_depth` its eleven call sites do not all have.
#[test]
fn test_apply_through_call_with_values_is_still_broken_on_the_vm() {
    assert_divergence(
        "(call-with-values (lambda () (values + '(1 2))) apply)",
        On::TreeWalker,
        "3",
        ErrorClass::AtRuntime,
        "PRD/TRACK_Q_QUALITY_PRD.md §1.2",
    );
}
