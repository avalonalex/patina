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
    ErrorClass, assert_program_eval_error, assert_program_eval_error_at, assert_program_eval_to,
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

/// An error raised by user code inside a `dynamic-wind` after thunk reaches
/// the enclosing `guard` — converged 2026-09-01.
///
/// It used to escape on the tree-walker: `run_wind_handlers(…)?` ran the
/// thunk on a nested trampoline with an empty handler stack, so the error had
/// to come back through Rust and nothing routed it to the handlers installed
/// outside. Wind thunks now run as steps of the trampoline the jump was made
/// on, each under the handler stack its `dynamic-wind` call was made in
/// (R7RS 6.10; `cps_eval/wind.rs`), so the `(car 7)` error finds the `guard`
/// like any other raise would. Primitive callbacks still run on the nested
/// trampoline — that boundary stays open in `backend_divergence.rs` and
/// `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.
///
/// `caught` is arbitrated by Gauche (chibi loops forever on this program):
/// the after thunk runs in the environment of the `dynamic-wind` call, which
/// is inside the `with-exception-handler`, but that handler's `'handled` is
/// then a return from a non-continuable `raise`, which R7RS 6.11 makes a
/// secondary exception raised in the same environment — so the `guard`
/// catches either way. The VM's half moved from `handled` to `caught` with
/// the audit's A3 fix (wind records are popped before their after-thunk
/// runs); `handled` came from the old ordering swallowing the thunk's error.
#[test]
fn test_an_error_inside_a_wind_thunk_reaches_the_enclosing_guard() {
    assert_program_eval_to(
        r#"(guard (e (#t 'caught))
             (with-exception-handler (lambda (c) 'handled)
               (lambda ()
                 (dynamic-wind (lambda () 1)
                               (lambda () (raise 'x))
                               (lambda () (car 7))))))"#,
        "caught",
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

/// The hole the fix above did *not* close, closed 2026-09-05 by issue #186.
///
/// `apply` reached through `call_any` — the VM's third and narrowest dispatcher
/// — used to fail here. `call_any` had kept the exact primitive → parameter →
/// closure probe that the apply instructions shed, and it is what runs
/// `call-with-values`' consumer, `call/cc`'s procedure, a wind thunk and —
/// since issue #179 made it a caller — a prompt **body**. So the callee set was
/// uniform across the two apply *instructions* and not across the VM.
///
/// It holds no probe of its own now: it calls `call_value` and reads the frame
/// depth to learn whether the callee finished. The `exit_depth` that this
/// comment used to name as the obstacle was a parameter nothing read.
///
/// Found by review, not by the tests: the first version of *that* work claimed
/// "`apply` accepts every callee a direct call accepts", and a five-token
/// program falsified it — with the same error string the change had just
/// declared fixed, one dispatcher over. Which is why the row is here as a
/// program rather than as a sentence.
#[test]
fn test_apply_through_call_with_values_accepts_a_control_primitive() {
    // The consumer.
    assert_program_eval_to(
        "(call-with-values (lambda () (values + '(1 2))) apply)",
        "3",
    );
    // …in tail position, which pops the frame before dispatching.
    assert_program_eval_to(
        "((lambda () (call-with-values (lambda () (values + '(1 2))) apply)))",
        "3",
    );
    // …and `call-with-values` itself reached as a value, so the consumer is
    // dispatched from `handle_control_primitive` rather than an instruction.
    assert_program_eval_to(
        "(let ((f call-with-values)) (f (lambda () (values + '(1 2))) apply))",
        "3",
    );
    // The producer is the same dispatcher: `values` with no arguments.
    assert_program_eval_to("(call-with-values values list)", "()");
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
