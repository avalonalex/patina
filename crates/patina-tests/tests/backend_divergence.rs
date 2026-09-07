//! The registry of known behavioural divergences between the tree-walker and
//! the VM.
//!
//! Every other test file in this crate holds both backends to the *same*
//! expectation. This is the exception list, and `assert_divergence` is the only
//! way onto it: each call pins the working backend's answer, requires the other
//! to still fail, and names the document tracking the bug.
//!
//! **These tests are designed to fail when the bug is fixed** — repairing the
//! broken backend trips the second assertion, and the panic message says to
//! collapse the call into a plain `assert_program_eval_to`. A quarantine that
//! does not retire itself becomes a permanent excuse.
//!
//! Some tests here are *not* divergences: they guard rows that have already
//! converged. They live here because this is where someone working the
//! divergence list will look for them.
//!
//! Sources: `PRD/TRACK_Q_QUALITY_PRD.md` §1.2, re-measured at `2d4ce29`
//! (2026-08-10), and
//! `PRD/ARCHIVE/AUDIT_2026_08_10_PRD.md` B3 (measured 2026-08-10).
//!
//! Shared root cause of the §1.2 cluster: R7RS §6.10 makes `call/cc`,
//! `dynamic-wind`, `values` and `with-exception-handler` ordinary procedures,
//! but both backends resolve them by name at the *call site*, and the registry
//! binding behind the name is missing or a stub. Every one works when called
//! directly, which is why the 1226/1226 chibi suite never catches it — that
//! suite never takes one of them as a value. Track Q Q2 is the fix.
//!
//! **The VM half of the `apply` rows is fixed** (2026-08-16). It was not the
//! registry after all: both apply instructions probed only
//! primitive → parameter → closure, so a VM-intercepted control primitive was
//! rejected before the registry was ever consulted — which is why the
//! `with-exception-handler` row's note blamed a stub that never ran. Both now
//! route through the same dispatcher `Call` uses. What remains on the
//! tree-walker is its genuine registry hole, still Q2 part 1's to fix, and
//! `apply` is simply a third way to reach it.
//!
//! **`call_any`, the VM's dispatcher for calls with no instruction behind
//! them**, kept the probe set the apply instructions shed — so a **control
//! primitive** reached through `call-with-values` or a prompt body failed
//! there, e.g. `(call-with-values (lambda () (values + '(1 2))) apply)`.
//! Closed 2026-09-05 by issue #186: it holds no probe of its own now, being
//! `call_value` plus a frame-depth test that says whether the callee finished.
//!
//! **That is one dispatcher, not the VM.** `with-exception-handler`'s *thunk*
//! still goes through `call_closure`, which takes a compiled closure and
//! nothing else, so `(with-exception-handler h values)` answers `#<values>`
//! on the tree-walker and fails on the VM — issue #190, unpinned here only
//! because it is #179's shape at a site that never had a dispatcher, and the
//! fix carries a handler-cleanup half of its own.
//!
//! Two neighbouring claims were wrong before #186 and are worth keeping
//! straight — a *continuation* did work, `call_any` having grown probes for
//! the full and delimited kinds; and an **exception** handler stopped being a
//! `call_any` caller at all when issue #178 moved the handler call into
//! `raise_step_stub`, whose `Call` is the ordinary instruction.

mod common;
use common::*;

const CONTROL_OPS: &str = "PRD/TRACK_Q_QUALITY_PRD.md §1.2";
const GUARD_UNWIND_ORDER: &str = "PRD/TRACK_L_SNOW_LIBRARIES_PRD.md §6";
// HANDLER_REENTRY (audit B3) is gone with the two rows that cited it: both
// converged on 2026-09-01 when `CpsContinuation` gained the handler stack.
// TREE_WALKER_PROMPTS (issue #169) likewise, on 2026-09-04, when the
// tree-walker gained the prompt API.

// ─── call/cc in value position (Track Q §1.2) ────────────────────────────────

/// Tree-walker: `Undefined variable: patina.internal.control/call/cc`.
/// Fixed by Q2 part 1 — a real registry binding behind the name.
#[test]
fn callcc_bound_with_define() {
    assert_divergence(
        "(define f call/cc) (f (lambda (k) 1))",
        On::Vm,
        "1",
        ErrorClass::AtRuntime,
        CONTROL_OPS,
    );
}

/// Same root cause as [`callcc_bound_with_define`], kept separate because
/// passing a control op *through a higher-order procedure* is the shape real
/// code hits (SRFI 1).
#[test]
fn callcc_passed_to_higher_order_procedure() {
    assert_divergence(
        "(map (lambda (f) (f (lambda (k) 6))) (list call/cc))",
        On::Vm,
        "(6)",
        ErrorClass::AtRuntime,
        CONTROL_OPS,
    );
}

// ─── apply on control ops (Track Q §1.2) ─────────────────────────────────────

/// Was "fails on both", recorded here so Q2 would not mistake backend
/// *agreement* for correctness. Half of it is fixed: the VM now evaluates it to
/// `1`, as R7RS requires and as chibi does, so what was a shared gap is now an
/// ordinary divergence with the tree-walker on the wrong side.
///
/// The tree-walker's remaining hole is the one `callcc_bound_with_define` and
/// `callcc_passed_to_higher_order_procedure` already describe — `call/cc` in
/// value position resolves to a registry binding that is not there — and it is
/// still Q2 part 1's to fix. `apply` is simply a third way to reach it.
#[test]
fn apply_callcc() {
    assert_divergence(
        "(apply call/cc (list (lambda (k) 1)))",
        On::Vm,
        "1",
        ErrorClass::AtRuntime,
        CONTROL_OPS,
    );
}

// ─── Multi-value continuations (TREE_WALKER_CALLCC_MULTI_VALUES.md) ──────────

/// A `call/cc` continuation invoked with multiple values. Converged
/// 2026-08-25: the tree-walker delivers a `#<values>` object for any count
/// but one, as the VM has since #113 and as `(values …)` itself does.
#[test]
fn callcc_multi_value_through_call_with_values() {
    assert_program_eval_to(
        r#"
        (call-with-values
          (lambda ()
            (call-with-current-continuation
              (lambda (k) (k 1 2))))
          (lambda (a b) (list a b)))
        "#,
        "(1 2)",
    );
}

/// The abort pattern used by SRFI 1's `%cars+cdrs`, and the reason the whole
/// n-ary half of `(scheme list)` was unusable on the tree-walker: `zip`,
/// `fold`, `any`, `every` and `list-index` over two or more lists all reach
/// it. Converged with the test above.
#[test]
fn callcc_abort_pattern_through_call_with_values() {
    assert_program_eval_to(
        r#"
        (call-with-values
          (lambda ()
            (call-with-current-continuation
              (lambda (abort)
                (abort '() '()))))
          (lambda (cars cdrs) (list 'cars cars 'cdrs cdrs)))
        "#,
        "(cars () cdrs ())",
    );
    // The SRFI 1 procedures this unblocks are asserted once, in
    // larceny_families.rs's family 5 — not duplicated here.
}

/// An error raised *after* a continuation escape is catchable — converged
/// 2026-09-01 when `CpsContinuation` gained the handler stack. The
/// tree-walker used to abort with `Type error: car expects a pair`, because
/// the escape had emptied the `guard`'s handler stack on the way past.
///
/// Found through a multi-value shape —
/// `(guard (e (#t (list 'caught))) (+ 1 (call/cc (lambda (k) (k 1 2)))))`,
/// reachable since 2026-08-25 when a multi-value continuation invocation
/// stopped raising a wrong-arity error at the call site and started escaping.
/// That program is **not** what is asserted here: delivering two values to a
/// single-value context is unspecified in R7RS, and the references split on
/// it (chibi and our VM let `+` raise on the `#<values>` object; Gauche
/// delivers the first value and answers `2`). Pinning it would hold both
/// backends to a choice R7RS does not require. The escape below is
/// single-valued and the error after it is unambiguous, so every
/// implementation must answer `caught` — chibi and Gauche do.
#[test]
fn an_error_after_a_continuation_escape_is_catchable() {
    assert_program_eval_to(
        "(guard (e (#t 'caught))
           (begin (call-with-current-continuation (lambda (k) (k 1)))
                  (car 7)))",
        "caught",
    );
}

/// The escape path's broadest effect, and the one nothing else covers: after
/// an inner `guard` fires, a *later* raise must still find the outer handler.
///
/// The tree-walker reset `exception_handlers` to empty on every re-entry, and
/// a `guard` that fires re-enters — `guard` expands to `call/cc` +
/// `with-exception-handler`, and catching invokes `guard-k`. So one caught
/// exception emptied the handler stack for everything after it:
///
/// ```text
///   tree-walker, before => Error: unhandled exception: y
///   VM, chibi, Gauche   => (outer y)
/// ```
///
/// Nothing in `nested_exception_handlers.rs` caught this: those tests nest
/// guards but never raise again *after* an inner one has fired, so they pass
/// either way. This is an ordinary shape — a loop that catches per item and
/// then fails on something else — not an exotic one.
#[test]
fn a_raise_after_an_earlier_guard_fired_still_finds_the_outer_handler() {
    assert_program_eval_to(
        "(guard (o (#t (list 'outer o)))
           (begin (guard (i (#t 'inner)) (raise 'x))
                  (raise 'y)))",
        "(outer y)",
    );
}

// ─── Continuation re-entry keeps the handler stack (audit B3 — closed) ───────

/// Re-entering a continuation captured under `with-exception-handler` keeps
/// the handler on both backends — converged 2026-09-01, closing audit B3.
///
/// The VM always restored the stack from its `VmContinuation` snapshot. The
/// tree-walker's escape path in `cps_eval/mod.rs` reset `exception_handlers`
/// to empty, because `CpsContinuation` did not carry them; it does now, and
/// re-entry restores them like `dynamic_winds`. Kept as the regression guard,
/// held to one expectation on both backends.
#[test]
fn reentered_continuation_keeps_exception_handler() {
    assert_program_eval_to(
        r#"
        (define saved #f)
        (define entered #f)
        (define (run)
          (with-exception-handler
            (lambda (e) 42)
            (lambda ()
              (call/cc (lambda (k) (set! saved k) #f))
              (raise-continuable 'boom))))
        (let ((first (run)))
          (if entered
              (list 'second-pass first)
              (begin (set! entered #t) (saved #f))))
        "#,
        "(second-pass 42)",
    );
}

// ─── Not divergences ─────────────────────────────────────────────────────────

/// §1.2 recorded this as a VM failure (`Wrong number of arguments: expected 1,
/// got 2`) at `7a6a797`; both backends return `7` as of `2d4ce29`. Kept as the
/// regression guard for a row that was fixed without anyone noticing.
#[test]
fn apply_values_agrees() {
    assert_program_eval_to("(apply values (list 7))", "7");
}

/// A `guard` clause runs after the unwind, on both backends — converged
/// 2026-09-01 with Track L triage families 22 and 28.
///
/// ```text
///   VM, chibi, Gauche => (before after handler)
///   tree-walker       => (before handler after)   until 2026-09-01
/// ```
///
/// R7RS §4.2.7 evaluates the clauses in the `guard`'s own dynamic
/// environment, so the after-thunk runs before them. Not cosmetic: a handler
/// writing to `current-output-port` wrote into whatever the un-unwound extent
/// installed, which is how it was found.
///
/// The tree-walker diverged because `(error "x")` reached the handler from
/// `apply_error`, which — alone among the three raise paths — did not unwind
/// first. The fix took the *other* two down to `apply_error`'s behaviour
/// rather than the reverse: no raise path unwinds now, and the unwind comes
/// from `guard-k`, which is where R7RS puts it. Kept as the regression guard
/// for both backends. See {GUARD_UNWIND_ORDER}.
#[test]
fn a_guard_clause_runs_after_the_unwind() {
    assert_program_eval_to(
        r#"
        (define log '())
        (guard (e (#t (set! log (cons 'handler log))))
          (dynamic-wind (lambda () (set! log (cons 'before log)))
                        (lambda () (error "x"))
                        (lambda () (set! log (cons 'after log)))))
        (reverse log)
    "#,
        "(before after handler)",
    );
}

/// Tree-walker: a continuation that escapes out of `eval` does not abandon
/// the rest of the expression — it escapes *and then continues*.
///
/// ```text
///   (call/cc (lambda (k) (set! kk k) (eval '(kk 'from-eval) …) 'fell-through))
///   VM, chibi  => from-eval
///   tree-walker => from-eval, and then 'fell-through runs too
/// ```
///
/// The VM half was fixed by routing every `ApplyContext` re-entry through one
/// boundary check; the tree-walker has no equivalent. Not `assert_divergence`
/// — that needs the broken backend to *fail*, and this one succeeds twice.
#[test]
fn escape_out_of_eval_does_not_abandon_on_the_tree_walker() {
    const PROGRAM: &str = r#"
        (import (scheme base) (scheme eval) (scheme repl))
        (define kk #f)
        (define trace '())
        (call/cc (lambda (k)
          (set! kk k)
          (eval '(kk 'from-eval) (interaction-environment))
          (set! trace (cons 'ran-on trace))
          'fell-through))
        (reverse trace)
    "#;
    assert_eq!(
        eval_program_vm(PROGRAM),
        "()",
        "the VM abandons at the escape; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "(ran-on)",
        "\n[tree-walker] NO LONGER DIVERGES — it now abandons at the escape.\n\
         Replace both assertions with assert_program_eval_to(PROGRAM, \"()\") \
         and update {GUARD_UNWIND_ORDER}."
    );
}

/// Tree-walker: a primitive callback that captures and invokes its *own*
/// continuation makes the whole program produce nothing.
///
/// ```text
///   (member 2 '(1 2 3) (lambda (a b) (call/cc (lambda (k2) (k2 (= a b))))))
///   VM, chibi, Gauche => (2 3)
///   tree-walker       => #f — the callback's value, not the primitive's
/// ```
///
/// Not an escape — the continuation is used and returned from normally — so
/// it is the case a cruder "any continuation invocation unwinds" rule would
/// break, which is why it is pinned rather than left to prose.
#[test]
fn callback_using_its_own_continuation_yields_nothing_on_the_tree_walker() {
    const PROGRAM: &str = r#"
        (import (scheme base))
        (member 2 '(1 2 3) (lambda (a b) (call/cc (lambda (k2) (k2 (= a b))))))
    "#;
    assert_eq!(eval_program_vm(PROGRAM), "(2 3)");
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "#f",
        "\n[tree-walker] NO LONGER DIVERGES — it now returns the primitive's \
         value.\nReplace both assertions with \
         assert_program_eval_to(PROGRAM, \"(2 3)\") and update \
         {GUARD_UNWIND_ORDER}."
    );
}

/// Invoking a continuation captured inside its own `dynamic-wind` extent runs
/// the wind thunks once, on both backends — converged 2026-09-01.
///
/// ```text
///   (dynamic-wind in (lambda () (call/cc (lambda (k) (k #f)))) out)
///   tree-walker, chibi, Gauche => (in out)
///   VM                         => (in out in out)   until 2026-09-01
/// ```
///
/// R7RS §6.10 runs the thunks when the extent is actually left and re-entered;
/// invoking `k` here never leaves it. The VM's wind transition (then
/// `run_wind_transition`, now `step_wind_jump`) forced the common prefix to
/// zero on every full `call/cc` invoke, so it exited and re-entered even the
/// extents both stacks shared. It takes the common prefix now, keyed on the
/// wind record's identity, as the tree-walker always did.
///
/// Found while taking `guard` to R7RS 7.3's expansion for Track L triage
/// families 22 and 28 (`PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6): that expansion
/// leaves its body through a continuation far more often, and under the old
/// rule a `guard` inside a `with-output-to-file` re-ran that form's after
/// thunk — which closes the port — so the next write failed on a port the
/// program still held. The defect is older and independent of that work, and
/// is fixed here on its own terms. Kept as the regression guard: it was
/// tracked in the PRD once, lost in an edit, and recovered only by review,
/// which is why it lives in a test.
#[test]
fn a_continuation_within_its_own_wind_runs_the_thunks_once() {
    assert_program_eval_to(
        r#"
        (import (scheme base))
        (define log '())
        (dynamic-wind (lambda () (set! log (cons 'in log)))
                      (lambda () (call/cc (lambda (k) (k #f))))
                      (lambda () (set! log (cons 'out log))))
        (reverse log)
    "#,
        "(in out)",
    );
}

/// The same jump through the **value** form of `dynamic-wind`, which is a
/// different code path and the one that regressed while this PR was written.
///
/// Head-position `dynamic-wind` compiles to `PushWind`/`PopWind`, so a
/// continuation resuming inside the body still reaches the instruction that
/// pops the record. The value form used to run its body on a nested Rust call
/// in `handle_control_primitive`, and an escape abandoned the frame that owned
/// the cleanup — it used to be safe to abandon it because a full continuation
/// invoke drained every wind record on the way past. Once the transition kept
/// the records both stacks share, that stopped being true, and the after-thunk
/// went from running at the wrong time to never running at all:
///
/// ```text
///   (define dw dynamic-wind) (dw in (lambda () (call/cc (lambda (k) (k #f)))) out)
///   main VM              => (in out in)     the thunks of a jump that crossed nothing
///   mid-fix VM           => (in)            after-thunk leaked entirely
///   chibi, Gauche, now   => (in out)
/// ```
///
/// The leaked record also outlived its owner and fired at the next unrelated
/// transfer, so the fourth shape below pins that a later `dynamic-wind` is
/// unaffected. `cargo test` was fully green with the leak present, because
/// nothing exercised the value form with an escaping body.
///
/// The nested Rust call is gone since 2026-09-02 (issue #157, the row below):
/// the value form runs the same `PushWind`/`PopWind` sequence in a stub frame,
/// so these four shapes now go through the instructions head position uses.
/// They stay because they are the shapes that caught the leak.
#[test]
fn the_value_form_of_dynamic_wind_runs_its_after_thunk_once() {
    assert_program_eval_to(
        r#"
        (import (scheme base))
        (define dw dynamic-wind)
        (define (probe run)
          (let ((log '()))
            (run (lambda (x) (set! log (cons x log))))
            (reverse log)))
        (list
          ;; escape stays inside the extent
          (probe (lambda (note)
                   (dw (lambda () (note 'in))
                       (lambda () (call/cc (lambda (k) (k #f))))
                       (lambda () (note 'out)))))
          ;; the record must not survive to fire at a later, unrelated wind
          (probe (lambda (note)
                   (dw (lambda () (note 'in))
                       (lambda () (call/cc (lambda (k) (k #f))))
                       (lambda () (note 'after-dw)))
                   (dynamic-wind (lambda () (note 'in2))
                                 (lambda () 'body)
                                 (lambda () (note 'out2)))))
          ;; reached through apply, not a variable reference
          (probe (lambda (note)
                   (apply dynamic-wind
                          (list (lambda () (note 'in))
                                (lambda () (call/cc (lambda (k) (k #f))))
                                (lambda () (note 'out))))))
          ;; escape that genuinely leaves the extent
          (probe (lambda (note)
                   (call/cc (lambda (esc)
                     (dw (lambda () (note 'in))
                         (lambda () (esc 'gone))
                         (lambda () (note 'out))))))))
    "#,
        "((in out) (in after-dw in2 out2) (in out) (in out))",
    );
}

/// A continuation escaping from an *after* thunk still runs the enclosing
/// after thunk — converged 2026-09-01 (triage family 30).
///
/// R7RS 6.10: `dynamic-wind`'s third thunk runs whenever control leaves the
/// dynamic extent, and calling `k` from inside one is still leaving — the
/// outer wind has not finished unwinding, so its own after thunk is still
/// owed. The VM always paid it. The tree-walker ran the whole unwind on a
/// nested trampoline that a second jump escaped out of, so it stopped at the
/// inner thunk and never ran `outer-after`. It now runs each wind thunk as a
/// step of the trampoline the jump was made on, with the record already
/// popped, so the second jump starts from where the first had got to and the
/// outer thunk is still on its path.
///
/// Found when Larceny's `base` suite began loading (families 14/15/23): it
/// was the one assertion in that suite the two backends answered differently.
/// chibi cannot arbitrate this one — re-entering `k` from an after thunk
/// sends it into an unbounded loop — but Gauche and the suite's own
/// expectation agree with this answer.
#[test]
fn a_continuation_from_an_after_thunk_still_runs_the_outer_after() {
    assert_program_eval_to(
        r#"
        (define trace '())
        (define (note x) (set! trace (cons x trace)))
        (define result
          (call-with-current-continuation
            (lambda (k)
              (dynamic-wind
                (lambda () (note 'outer-before))
                (lambda ()
                  (dynamic-wind
                    (lambda () (note 'inner-before))
                    (lambda () (note 'body) (k 'from-body))
                    (lambda () (note 'inner-after) (k 'from-after))))
                (lambda () (note 'outer-after))))))
        (list result (reverse trace))
    "#,
        "(from-after (outer-before inner-before body inner-after outer-after))",
    );
}

/// Bad syntax handed to the `eval` primitive is the *caller's* error, raised
/// while the program runs — catchable, on both backends. The tree-walker used
/// to wrap it in a non-catchable `InternalError` (so this program died) while
/// the VM caught it; converged when the D3 error-class work relabeled the
/// eval-primitive path as `InvalidSyntax`. `EvalError::DesugarError` stays
/// reserved for the `Backend::eval` entry, where nothing is running yet.
#[test]
fn evaled_bad_syntax_is_catchable_on_both() {
    assert_program_eval_to(
        "(import (scheme eval) (scheme repl)) \
         (guard (e (#t 'caught)) (eval '(if) (interaction-environment)))",
        "caught",
    );
}

/// Converged 2026-08-15: an unbound variable is a catchable condition in
/// every position, on both backends.
///
/// The tree-walker's CPS step function routed lookup failures through the
/// Scheme exception handlers in some arms and `?`-propagated them in others,
/// so whether `guard` caught the error depended on where the variable sat.
/// chibi, Gauche and Chez catch every position here; the VM already did.
/// Enforced structurally by the `try_catchable!` macro in `step.rs`; history
/// in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.
#[test]
fn unbound_variable_is_catchable_in_every_position() {
    for body in [
        "undefined-name",              // bare reference
        "(undefined-name)",            // operator position
        "(list (undefined-name))",     // operand position
        "(+ 1 (undefined-name))",      // operand of a primitive
        "(if undefined-name 1 2)",     // `if` test
        "(set! undefined-name 1)",     // `set!` target
        "(define x undefined-name) x", // `define` value
        "(call/cc undefined-name)",    // `call/cc` operand
        "`(,undefined-name)",          // unquote
    ] {
        assert_program_eval_to(
            &format!("(import (scheme base)) (guard (e (#t 'caught)) {body})"),
            "caught",
        );
    }
}

/// A continuation captured *inside* an after thunk while a jump is running it
/// resumes that thunk, and the jump then lands — converged 2026-09-02 with
/// the VM half of the `finally` rule.
///
/// ```text
///   both, Gauche => (escaped (before after))
///   VM           => (() (before after after))   until 2026-09-02
/// ```
///
/// `return`'s continuation is the rest of the thunk and then the jump that
/// was running it. Each backend had to make that second half a resumable
/// thing before this could work: the tree-walker's `Jump` step (`wind.rs`),
/// the VM's `ResumeWindJump` stub frame (`runtime/vm_state.rs`). The VM's old
/// answer is what a nested Rust call gives you — the continuation captured
/// the *enclosing* frame instead, parked inside the inlined `dynamic-wind`
/// sequence at its `PopWind`, so re-entering it ran `Call after` a second
/// time and the escape value never arrived. Recorded in {GUARD_UNWIND_ORDER}.
///
/// Until PR #152's fix in the tree-walker's escape arm (`cps_eval/mod.rs`),
/// this program *crashed* the tree-walker with `Error: Continuation escape`:
/// the parked escape's resumption was invoked in place, and its own parked
/// escape was carried out of the trampoline by a `?`. No primitive callback
/// is needed to reach it: a wind thunk whose tail is any `call/cc` that is
/// later invoked does.
#[test]
fn a_continuation_captured_inside_a_running_after_thunk_resumes_it() {
    assert_program_eval_to(
        r#"
        (define log '())
        (define (note x) (set! log (cons x log)))
        (define r
          (call/cc (lambda (k)
            (dynamic-wind
              (lambda () (note 'before))
              (lambda () (k 'escaped))
              (lambda ()
                (call/cc (lambda (return)
                  (note 'after)
                  (return 'stopped)
                  (note 'unreached))))))))
        (list r (reverse log))
    "#,
        "(escaped (before after))",
    );
}

/// A before thunk run by a re-entry sees the handlers of its own
/// `dynamic-wind` call, not those installed at the *jump* — converged
/// 2026-09-02 with the VM half of the `finally` rule.
///
/// ```text
///   both, Gauche => (outer b)
///   VM           => (inner b)   until 2026-09-02
/// ```
///
/// The inner `guard` was not installed when `dynamic-wind` was called, so
/// R7RS 6.10 puts the before thunk's `raise` outside it. This is the
/// complement of `wind_thunk_exceptions.rs`, whose rows all have a handler
/// *missing* at the jump; here one is *extra*, and the VM's old "handlers
/// from the machine, not the record" answered wrong in that direction too.
/// Both backends now take the thunk's handler stack from the wind record.
/// Recorded in {GUARD_UNWIND_ORDER}.
#[test]
fn a_before_thunk_on_reentry_sees_its_own_dynamic_winds_handlers() {
    assert_program_eval_to(
        r#"
        (let ((k #f) (n 0))
          (let ((r (guard (o (#t (list 'outer o)))
                     (dynamic-wind
                       (lambda () (set! n (+ n 1)) (if (= n 2) (raise 'b)))
                       (lambda () (call/cc (lambda (c) (set! k c) 'first)))
                       (lambda () #f)))))
            (if (eq? r 'first)
                (guard (i (#t (list 'inner i))) (k 'second))
                r)))
    "#,
        "(outer b)",
    );
}

/// A continuation re-entering the body of the **value** form of
/// `dynamic-wind` finds the call still intact — converged 2026-09-02
/// (issue #157).
///
/// Two symptoms, one cause. The call's remaining obligations — deliver the
/// body's value, pop the record, run *its own* after-thunk — used to live in
/// the Rust frame `handle_control_primitive` ran the body on, and a re-entry
/// restores the VM's frames, not that one:
///
/// ```text
///   (define r (dw (lambda () #f) «capture k, return 'first» (lambda () #f)))
///   (if (eq? r 'first) (k 'second) #f)          => second, VM said ()
///
///   (dw in1 «capture saved» out1) then (dw in2 (lambda () (saved 'second)) out2)
///                                              => (in1 out1 in2 out2 in1 out1),
///                                                 VM said (… in1 out2)
/// ```
///
/// The `()` was the `NULL` `call/cc`'s capture cleared `dst` to, left in a
/// live register — downstream it surfaced as an unrelated `type error:
/// expected a procedure, got null` rather than as a visibly wrong value. The
/// wrong after-thunk came from the `Escaped` arm deciding what it still owed
/// with a *length* test, `dynamic_winds.len() > wind_depth`, applied after
/// the jump had already replaced that stack with the target's: it truncated
/// the target's records and re-ran its own.
///
/// The fix is the move PR #156 made for a jump's wind thunks — the value form
/// now runs the same `PushWind`/`Call`/`PopWind` sequence head position
/// compiles to, in a stub frame of its own, so "the rest of the
/// `dynamic-wind`" is a pc that the continuation restores. Head position was
/// never affected, for exactly that reason. Recorded in {GUARD_UNWIND_ORDER}.
#[test]
fn the_value_form_of_dynamic_wind_survives_a_reentry_into_its_body() {
    // The body's value is the call's value, delivered on the re-entry too.
    assert_program_eval_to(
        r#"
        (define dw dynamic-wind)
        (define k #f)
        (define r (dw (lambda () #f)
                      (lambda () (call/cc (lambda (c) (set! k c) 'first)))
                      (lambda () #f)))
        (if (eq? r 'first) (k 'second) #f)
        r
    "#,
        "second",
    );
    // Re-entering extent 1 from inside extent 2 leaves extent 2 (`out2`) and
    // enters extent 1 (`in1`); when the resumed body returns, extent 1 closes
    // with its *own* after-thunk, `out1`.
    assert_program_eval_to(
        r#"
        (import (scheme base))
        (define dw dynamic-wind)
        (define log '())
        (define (note x) (set! log (cons x log)))
        (define saved #f)
        (define done #f)
        (dw (lambda () (note 'in1))
            (lambda () (call/cc (lambda (c) (set! saved c) 'first)))
            (lambda () (note 'out1)))
        (if (not done)
            (begin (set! done #t)
                   (dw (lambda () (note 'in2))
                       (lambda () (saved 'second))
                       (lambda () (note 'out2)))))
        (reverse log)
    "#,
        "(in1 out1 in2 out2 in1 out1)",
    );
}

/// A continuation captured in the value form's **before** or **after** thunk
/// and re-entered after the call has returned — issue #159, converged
/// 2026-09-02 with the body case above and by the same change.
///
/// The value form ran each thunk on a nested dispatch loop. While that loop is
/// still on the Rust stack a continuation captured in the thunk resumes fine —
/// a retry loop inside a before-thunk always worked, on `main` too. It is the
/// *late* re-entry, after the `dynamic-wind` call has returned and the loop is
/// gone, that had nothing to come back to:
///
/// ```text
///   A: capture in `before`, re-enter later => (val (in body out body out))
///   B: capture in `after`,  re-enter later => (val (in body out))
///   main VM said (#<unknown> (in body out)) to both
/// ```
///
/// `#<unknown>` is an uninitialised register reaching user-visible output: the
/// re-entry delivered into a frame that no longer existed, and the rest of the
/// `dynamic-wind` — the body, the after-thunk, the value — never ran at all.
/// The tree-walker, Gauche and chibi all give the two answers above.
///
/// `probe` sequences the run and the log read with `let*` rather than writing
/// `(list (run …) (reverse log))`. R7RS leaves argument order unspecified and
/// chibi evaluates right-to-left, so the shorter spelling reads an empty log
/// there and answers `(val ())` to both rows — a bug in the *test*, not a
/// disagreement about `dynamic-wind`. It was written that way first, and
/// cross-checking the row against chibi is what caught it.
///
/// Filed separately from #157 because that issue documents only the body case,
/// and the failure here is visibly different: not a wrong value but an
/// uninitialised one, and not a mis-ordered thunk but a call that stops dead.
#[test]
fn the_value_form_of_dynamic_wind_reenters_its_before_and_after_thunks() {
    assert_program_eval_to(
        r#"
        (import (scheme base))
        (define dw dynamic-wind)
        (define (probe run)
          (let ((log '()))
            (let* ((r (run (lambda (x) (set! log (cons x log)))))
                   (l (reverse log)))
              (list r l))))
        (list
          ;; A — the resumed before-thunk returns, and the rest of the call
          ;; runs a second time from there.
          (probe (lambda (note)
                   (let ((k #f) (done #f))
                     (let ((r (dw (lambda () (note 'in) (call/cc (lambda (c) (set! k c))))
                                  (lambda () (note 'body) 'val)
                                  (lambda () (note 'out)))))
                       (if (not done) (begin (set! done #t) (k #f)))
                       r))))
          ;; B — the resumed after-thunk returns, and the call is then over,
          ;; so nothing repeats.
          (probe (lambda (note)
                   (let ((k #f) (done #f))
                     (let ((r (dw (lambda () (note 'in))
                                  (lambda () (note 'body) 'val)
                                  (lambda () (note 'out) (call/cc (lambda (c) (set! k c)))))))
                       (if (not done) (begin (set! done #t) (k #f)))
                       r)))))
    "#,
        "((val (in body out body out)) (val (in body out)))",
    );
}

/// A `call/cc` retry loop *inside* one of the value form's wind thunks, which
/// resumes while the thunk is still running.
///
/// Not a converged row — `main` answered this correctly too, because the
/// nested dispatch loop the thunk ran on was still on the Rust stack to resume
/// into. It is the *late* re-entry, after that loop is gone, that was broken
/// (#159, the row above). It is here as a guard on the rewritten path: the thunks are ordinary
/// frames of `value_wind_stub` now, and this is the shape that would notice if
/// the stub's register window or its `Call` sequence got the thunk's own
/// re-entry wrong.
#[test]
fn the_value_form_of_dynamic_wind_captures_inside_its_own_thunks() {
    assert_program_eval_to(
        r#"
        (import (scheme base))
        (define dw dynamic-wind)
        (define (probe run)
          (let ((log '()))
            (run (lambda (x) (set! log (cons x log))))
            (reverse log)))
        (list
          ;; a retry loop inside the before-thunk
          (probe (lambda (note)
                   (let ((n 0))
                     (dw (lambda ()
                           (let ((k (call/cc (lambda (c) c))))
                             (note n)
                             (set! n (+ n 1))
                             (if (< n 3) (k k))))
                         (lambda () (note 'body))
                         (lambda () (note 'out))))))
          ;; and one inside the after-thunk
          (probe (lambda (note)
                   (let ((n 0))
                     (dw (lambda () (note 'in))
                         (lambda () (note 'body))
                         (lambda ()
                           (let ((k (call/cc (lambda (c) c))))
                             (note n)
                             (set! n (+ n 1))
                             (if (< n 3) (k k)))))))))
    "#,
        "((0 1 2 body out) (in body 0 1 2))",
    );
}

// ─── the tree-walker's nested trampoline ─────────────────────────────────────

/// A `call/cc` retry loop inside a `call-with-port` callback answers `"012"`
/// on both backends — converged 2026-09-01, but read the mechanism before
/// counting it as a fix.
///
/// The tree-walker used to fail with `I/O error: port is closed`: the callback
/// has not returned — the continuation resumes *inside* it — but the nested
/// trampoline reads the invoke as an escape, and `call-with-port` closed the
/// port on every exit, so the next write in the same callback failed on a port
/// the program still held (audit F6, the resource-corruption manifestation of
/// the nested-trampoline defect in Track L §6).
///
/// What changed is `call-with-port`, not the trampoline: R7RS 6.13.1 closes
/// the port only "if `proc` returns", so an escape leaves it open now (found
/// by review of triage families 22/28). The misread is still there — the two
/// tests below and `callback_using_its_own_continuation_yields_nothing_on_the_tree_walker`
/// show it — it just no longer destroys a resource on this shape. VM and
/// chibi answered `"012"` throughout.
#[test]
fn call_with_port_survives_an_in_extent_continuation_invoke() {
    assert_program_eval_to(
        r#"(call-with-port (open-output-string)
             (lambda (p)
               (let ((n 0))
                 (let ((k (call/cc (lambda (c) c))))
                   (write-string (number->string n) p)
                   (set! n (+ n 1))
                   (if (< n 3) (k k)))
                 (get-output-string p))))"#,
        "\"012\"",
    );
}

/// Tree-walker: `unhandled exception: sym` — a declining `guard` clause
/// inside a primitive's callback loses the outer `guard`.
///
/// R7RS 7.3's `guard` re-raises a declined condition by jumping back *into*
/// the raise point through `handler-k` and calling `raise-continuable` there,
/// so the next handler out is the one that was installed around the raise.
/// That jump lands inside the `call-with-port` callback, which runs on the
/// tree-walker's nested trampoline — and that trampoline starts with an empty
/// handler stack, so the re-raise finds nothing. The same nested-trampoline
/// defect as the test above and
/// `callback_using_its_own_continuation_yields_nothing_on_the_tree_walker`,
/// in its third manifestation; it became
/// reachable on 2026-09-01 when `guard` took the reference expansion (triage
/// families 22/28). The old expansion re-raised from the clause side, outside
/// the callback, and happened to find the outer handler. Both raise forms
/// reach it. VM, chibi and Gauche: `(outer sym)`.
#[test]
fn a_declining_guard_inside_a_port_callback_loses_the_outer_guard_on_the_tree_walker() {
    for raise in ["raise", "raise-continuable"] {
        assert_divergence(
            &format!(
                "(guard (outer (#t (list 'outer outer)))
                   (call-with-port (open-input-string \"a\")
                     (lambda (p) (guard (e ((string? e) 'no)) ({raise} 'sym)))))"
            ),
            On::Vm,
            "(outer sym)",
            ErrorClass::AtRuntime,
            GUARD_UNWIND_ORDER,
        );
    }
}

/// Tree-walker: a raise inside a primitive's callback reaches the outer
/// `guard` as the wrong object.
///
/// The callback's `(raise 'x)` finds no handler on the nested trampoline, so
/// that trampoline reports it as an `unhandled exception: x` *error*, which
/// the outer trampoline then routes to the `guard` as an error object. A
/// clause testing for `'x` declines, and the program dies re-raising an
/// object nobody raised. VM, chibi, Gauche: `(sym x)`. Older than triage
/// families 22/28 — `main` gave the same — and the same trampoline defect.
#[test]
fn a_raise_inside_a_port_callback_reaches_the_guard_as_an_error_object_on_the_tree_walker() {
    assert_divergence(
        r#"(guard (e ((symbol? e) (list 'sym e))
                    ((error-object? e) (raise (error-object-message e))))
             (call-with-port (open-input-string "a") (lambda (p) (raise 'x))))"#,
        On::Vm,
        "(sym x)",
        ErrorClass::AtRuntime,
        GUARD_UNWIND_ORDER,
    );
}

// ─── the VM's raise paths, where the tree-walker is right ─────────────────────

/// A `guard` survives one of its clauses declining a `raise-continuable`.
///
/// The `guard`'s handler declines `'x`, which re-raises it through
/// `handler-k` to the outer handler; that returns `(I x)`, and the body
/// continues to raise `'y` — which the `guard` must catch. chibi and Gauche
/// agree.
///
/// The VM used to answer `((I x) (I y))`: its continuable path popped the
/// handler to run it and re-pushed it only when the handler *returned*, in
/// Rust after a nested dispatch loop, and `guard`'s handler leaves through
/// `handler-k` instead — so the re-push was skipped and the `guard` was
/// silently uninstalled for the rest of its body. Converged 2026-09-05 with
/// issue #178, which made the re-push an instruction in a frame: `handler-k`
/// captures that frame like any other, so re-entering it runs the re-push.
#[test]
fn a_guard_survives_declining_a_continuable_raise() {
    assert_program_eval_to(
        "(with-exception-handler (lambda (e) (list 'I e))\n\
         \x20 (lambda () (guard (e ((eq? e 'y) 'caught-y))\n\
         \x20   (list (raise-continuable 'x) (raise-continuable 'y)))))",
        "caught-y",
    );
}

/// A handler that returns from a non-continuable `raise` raises the
/// secondary exception R7RS 6.11 asks for.
///
/// "If the handler returns, a secondary exception is raised in the same
/// dynamic environment as the handler." The VM used to deliver the handler's
/// value to the raise's destination register as if the raise had been
/// continuable — `(returned)` for the first thunk, and for the second, where
/// the raise is a *primitive's* error routed with register 0 as its
/// destination, the returning handler's value landed in r0 and `car`'s own
/// destination was left holding `()`. It could not tell a handler that
/// returned from one that escaped, because both came back through the same
/// nested run loop.
///
/// Converged 2026-09-05 with issue #178: the return lands on `ResumeRaise`,
/// an instruction, which sees it whatever the handler was and whichever route
/// the raise took.
#[test]
fn a_handler_returning_from_a_non_continuable_raise_raises_the_secondary() {
    const SECONDARY: &str = "(outer \"exception handler returned from non-continuable exception\")";
    for thunk in ["(list (raise 'x))", "(list (car 5))"] {
        assert_program_eval_to(
            &format!(
                "(guard (o (#t (list 'outer (if (error-object? o) (error-object-message o) o))))\n\
                 \x20 (with-exception-handler (lambda (e) 'returned) (lambda () {thunk})))"
            ),
            SECONDARY,
        );
    }
}

/// A continuation used *as* the handler, for a primitive's error.
///
/// `(call/cc (lambda (k) (with-exception-handler k thunk)))` is R7RS's own
/// idiom for capturing a raised object; chibi and Gauche answer `#t` too.
/// Triage family 24 made it work for `raise` on the VM, but a `VmError` from a
/// primitive takes the run loop's route into `vm_raise_value`, which called
/// the handler through `call_any` — the narrow dispatcher, which does not
/// accept a continuation. Converged 2026-09-05 with issue #178: the handler
/// is called by the `Call` instruction of `raise_step_stub` now, which is the
/// same dispatcher every other call goes through.
#[test]
fn a_continuation_can_be_the_handler_for_a_primitive_error() {
    assert_program_eval_to(
        "(error-object? (call/cc (lambda (k) (with-exception-handler k (lambda () (car 5))))))",
        "#t",
    );
}

// ─── the prompt API, on both backends (issue #169, closed 2026-09-04) ────────

/// `(scheme base)` exports the prompt API, and both backends implement it.
///
/// Until 2026-09-04 only the VM did, and four quarantines stood here: two
/// `assert_divergence` calls pinning the tree-walker's deliberate
/// not-implemented error (#170), one asserting that error's wording, one
/// that `guard` could catch it. They failed together the moment the API
/// landed, which is what `assert_divergence` is designed to do, and became
/// this. The behaviour itself is scored by `control_flow_matrix.rs` and the
/// prompt tests in `cps_features.rs`; what stays here is the pair of programs
/// that used to be the divergence, held to one answer.
#[test]
fn the_prompt_api_answers_the_same_on_both_backends() {
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (call-with-continuation-prompt (lambda () 42) t (lambda (v k) v))",
        "42",
    );
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (call-with-continuation-prompt\n\
         \x20 (lambda () (abort-current-continuation t 'ab))\n\
         \x20 t (lambda (v k) (list 'handler v)))",
        "(handler ab)",
    );
}

/// An abort with no prompt to go to is an ordinary raised error, so a
/// program can catch it — the property the #170 error had and its
/// replacement had to keep. Both backends: the VM raises
/// `NoMatchingPrompt` through the same route as any runtime error.
#[test]
fn an_abort_with_no_matching_prompt_is_catchable() {
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (guard (e (#t (list 'caught (error-object? e))))\n\
         \x20 (abort-current-continuation t 'nowhere))",
        "(caught #t)",
    );
}

/// Two answers the review of #175 found the backends giving differently,
/// both fixed on the side that was wrong, and held together here.
///
/// Extra arguments to `call-with-continuation-prompt` go to the body, which
/// is Racket's signature (`proc [prompt-tag handler] arg ...`); the
/// tree-walker rejected them with an arity error and the VM dropped them, so
/// a one-argument body was called with none. And `continuation?` answers
/// `#t` for a continuation whichever backend captured it: the VM's are
/// `VmContinuationRef`s, which the predicate did not know.
#[test]
fn the_prompt_apis_edges_agree_on_both_backends() {
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (call-with-continuation-prompt (lambda (a b) (list 'body a b)) t (lambda (v k) v) 1 2)",
        "(body 1 2)",
    );
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (call-with-continuation-prompt\n\
         \x20 (lambda () (abort-current-continuation t 'x))\n\
         \x20 t (lambda (v k) (list (procedure? k) (continuation? k))))",
        "(#t #t)",
    );
    assert_program_eval_to(
        "(call/cc (lambda (c) (list (procedure? c) (continuation? c))))",
        "(#t #t)",
    );
}

// ─── VM prompt defects found by the review of #175 (issues #176–#179) ────────
//
// Four were filed; #176 and #178 are fixed and their entries below are plain
// both-backend assertions now. **#177 and #179 are the two still pinned** —
// count those, not the tests in this block. Guile 3.0.11 backs the
// tree-walker's answer wherever it is cited; chibi and Gauche cannot express
// a tagged prompt at all, so they are never among the oracles here
// (`control_flow_matrix.rs` says the same, per row).

/// A prompt does not outlive its body when a continuation captured inside
/// the body is re-entered — issue #176, fixed 2026-09-05.
///
/// The body's value has been delivered twice, once normally and once through
/// the re-entered continuation, so no prompt is live when the abort runs and
/// `guard` catches it. The tree-walker and Guile 3.0.11 always answered this;
/// the VM died with `expected a procedure, got null`, aborting to a prompt
/// whose body had returned in an earlier top-level form.
///
/// `(call/cc …)` is in **tail position** of the prompt body, which is what
/// made it reachable: the body's frame is popped before the capture, so the
/// prompt's `stack_depth` already equals `frames.len()` while the body is
/// still running. An abort at that moment must still find the prompt —
/// `a_tail_position_prompt_is_still_live` beside this, which **Guile 3.0.11
/// and Racket 9.3** both answer `(h x)` — and the same depth reading holds
/// once the body is finished. No comparison of depths tells the two apart.
///
/// So the prompt rides into the continuation's snapshot, and no later sweep
/// reaches it: the frames that would have crossed its depth are gone. The
/// arrival of a full continuation is the one moment the reading *is* exact —
/// the value is being delivered right then, so a prompt with no frame above
/// it has already delivered its own — and that is where the snapshot's
/// resolved prompts are dropped (`restore_continuation`).
///
/// Both spellings are asserted: the two-top-level-form version from the
/// issue, and the same sequence inside a single `let` body. The first fix
/// tried closed only the first — it truncated the prompt stack when a
/// dispatch loop returned, and inside one loop no loop returns between the
/// re-entry and the abort.
#[test]
fn a_prompt_does_not_outlive_a_re_entered_body() {
    const PROGRAM: &str = "(define t (make-continuation-prompt-tag 'p))\n\
         (define saved #f) (define n 0)\n\
         (define r (call-with-continuation-prompt\n\
         \x20           (lambda () (call/cc (lambda (c) (set! saved c) 'first)))\n\
         \x20           t (lambda (v k) (list 'h v))))\n\
         (set! n (+ n 1))\n\
         (if (= n 1) (saved 'second))\n\
         (list 'r r (guard (e (#t 'no-prompt)) (abort-current-continuation t 'stale)))";

    // Two top-level forms, as the issue filed it.
    assert_program_eval_to(PROGRAM, "(r second no-prompt)");

    // …and the same sequence inside one `let` body, where no dispatch loop
    // returns between the re-entry and the abort. This spelling is why the
    // fix is at the continuation's arrival rather than at a loop's exit: a
    // loop-exit backstop passes the program above and leaves this one dying
    // with `expected a procedure, got null`.
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (define saved #f) (define n 0)\n\
         (let ()\n\
         \x20 (define r (call-with-continuation-prompt\n\
         \x20             (lambda () (call/cc (lambda (c) (set! saved c) 'first)))\n\
         \x20             t (lambda (v k) (list 'h v))))\n\
         \x20 (set! n (+ n 1))\n\
         \x20 (if (= n 1) (saved 'second))\n\
         \x20 (list 'r r (guard (e (#t 'no-prompt)) \
             (abort-current-continuation t 'stale))))",
        "(r second no-prompt)",
    );
}

/// The window the #176 fix must not close: a prompt whose body has
/// **tail-called** away is still the prompt an abort belongs to, even though
/// its `stack_depth` already equals `frames.len()`.
///
/// Its own test rather than a second assertion in the one above, so that a
/// regression in either direction is reported on its own: if the two shared a
/// test, reintroducing #176 would panic before this ever ran, and the report
/// would not say whether the over-aggressive direction had broken too.
///
/// **Guile 3.0.11 and Racket 9.3** both answer `(h x)`. chibi and Gauche have
/// no tagged prompt API, so they cannot be asked.
#[test]
fn a_tail_position_prompt_is_still_live() {
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (call-with-continuation-prompt\n\
         \x20 (lambda () (call/cc (lambda (c) (abort-current-continuation t 'x))))\n\
         \x20 t (lambda (v k) (list 'h v)))",
        "(h x)",
    );
}

/// An abort out of a Rust primitive's callback reaches its prompt — issue
/// #177, fixed 2026-09-05.
///
/// `force` runs its thunk through `ApplyContext::apply_proc`, a re-entry
/// boundary with a nested dispatch loop under it. An abort there is not a
/// return: it cuts every stack back to its prompt and pushes one stub frame
/// that has yet to run, so the primitive must be abandoned rather than handed
/// a value.
///
/// The **tail** spelling is the one that broke. The tail call pops the prompt
/// body's frame before `force` runs, so the abort's landing sits at exactly
/// the depth a callback returning normally would leave — and every check on
/// the way out was a frame-depth comparison. `force` cached the abort's value
/// as its thunk's result and ran on, over the registers the landing was about
/// to use: `expected a procedure, got object`. The non-tail spelling leaves a
/// frame behind and always worked, which is what made the depth reading look
/// sufficient.
#[test]
fn an_abort_out_of_a_primitives_callback_reaches_its_prompt() {
    const PRELUDE: &str = "(import (scheme lazy))\n(define t (make-continuation-prompt-tag 'p))\n";
    for (body, expected) in [
        (
            "(force (delay (abort-current-continuation t 'a1)))",
            "(h a1)",
        ),
        (
            "(list 'y (force (delay (abort-current-continuation t 'a3))))",
            "(h a3)",
        ),
    ] {
        assert_program_eval_to(
            &format!(
                "{PRELUDE}(call-with-continuation-prompt (lambda () {body})\n\
                 \x20 t (lambda (v k) (list 'h v)))"
            ),
            expected,
        );
    }
}

/// Tree-walker: an abort out of a callback reached through a *nested
/// trampoline* does not find its prompt.
///
/// The VM answers this since #177. The tree-walker's
/// `apply_from_direct_tagged` — the trampoline a Rust primitive's callback
/// runs on — starts every stack empty, so the abort searches a prompt stack
/// that has none of the caller's prompts and raises "no matching prompt tag",
/// which the `guard` then catches. `dynamic-wind` records and exception
/// handlers have the same hole on that path and predate prompts entirely; it
/// is the "primitive's callback" entry in {GUARD_UNWIND_ORDER}, and
/// `cps_eval/prompts.rs` names it as inherited rather than added.
///
/// `force` is not the probe here: the tree-walker routes it through the CPS
/// evaluator rather than the trampoline, so it answers `(h a1)` there. A
/// comparator passed to `assoc` does go through the trampoline.
#[test]
fn an_abort_out_of_a_nested_trampoline_callback_is_lost_on_the_tree_walker() {
    assert_eq!(
        eval_program_vm(
            "(define t (make-continuation-prompt-tag 'p))\n\
             (call-with-continuation-prompt\n\
             \x20 (lambda () (assoc 1 '((1 . a)) (lambda (a b) (abort-current-continuation t 'x))))\n\
             \x20 t (lambda (v k) (list 'h v)))"
        ),
        "(h x)",
        "the VM reaches the prompt; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_tree_walker(
            "(define t (make-continuation-prompt-tag 'p))\n\
             (guard (e (#t (list 'caught (error-object? e))))\n\
             \x20 (call-with-continuation-prompt\n\
             \x20   (lambda () (assoc 1 '((1 . a)) (lambda (a b) (abort-current-continuation t 'x))))\n\
             \x20   t (lambda (v k) (list 'h v))))"
        ),
        "(caught #t)",
        "\n[tree-walker] NO LONGER DIVERGES — the abort now finds its prompt \
         through the nested trampoline.\nReplace both assertions with a single \
         assert_program_eval_to on `(h x)` and close the \"primitive's callback\" \
         entry in PRD/TRACK_L_SNOW_LIBRARIES_PRD.md §6."
    );
}

/// A composable continuation captured from inside an exception handler
/// carries the handler with it — issue #178, fixed 2026-09-05.
///
/// Resuming `k` returns from the handler. R7RS 6.11 then reinstalls the
/// handler after a continuable raise, so the second `raise-continuable` must
/// reach it; and raises a secondary exception after a non-continuable one.
/// The VM did neither: both were Rust after a nested dispatch loop, which the
/// resumed frames returned straight past. Guile 3.0.11 answers as both
/// backends now do.
#[test]
fn a_composable_continuation_captured_inside_a_raise_handler_carries_it() {
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (guard (e (#t (list 'caught e)))\n\
         \x20 (call-with-continuation-prompt\n\
         \x20   (lambda ()\n\
         \x20     (with-exception-handler\n\
         \x20       (lambda (e) (if (eq? e 'rc)\n\
         \x20                       (abort-current-continuation t (list 'aborted e))\n\
         \x20                       (list 'handled-again e)))\n\
         \x20       (lambda () (list 'body (raise-continuable 'rc) (raise-continuable 'rc2)))))\n\
         \x20   t (lambda (v k) (list 'h (k 'resumed)))))",
        "(h (body resumed (handled-again rc2)))",
    );
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag 'p))\n\
         (guard (e (#t (list 'caught (error-object? e))))\n\
         \x20 (call-with-continuation-prompt\n\
         \x20   (lambda ()\n\
         \x20     (with-exception-handler\n\
         \x20       (lambda (e) (abort-current-continuation t (list 'aborted e)))\n\
         \x20       (lambda () (list 'body (raise 'nc)))))\n\
         \x20   t (lambda (v k) (list 'h (k 'resumed)))))",
        "(caught #t)",
    );
}

/// A prompt is closed by *identity*, not by taking the top of the stack, and
/// a dispatch loop closes the prompts it did not open.
///
/// Both are crashes the review of #179 found, and both need a body that
/// finishes **without a frame** — the case that made
/// `call-with-continuation-prompt` close its own prompt at all.
///
/// The first: such a body can still re-enter the VM and leave a prompt above
/// this call's. `pop()` then took *that* one, leaving this call's live for the
/// next abort to land on — `expected a procedure, got null`, reading a handler
/// out of a frame that had been cut away. `truncate(prompt_idx)` closes the
/// frame this call pushed and anything the body stranded on top of it.
///
/// The second is older and independent of the body: a prompt opened inside a
/// nested dispatch loop outlives it, because the sweep at a loop's own exit
/// depth deliberately pops nothing. The handler half of that backstop has
/// always been there; the prompt half was not, and a parameter converter that
/// opens a prompt is enough to reach it with no prompt body in sight.
#[test]
fn a_no_frame_body_closes_this_calls_prompt_and_no_other() {
    const TAGS: &str = "(define t (make-continuation-prompt-tag 'p))\n\
         (define t2 (make-continuation-prompt-tag 'q))\n";

    // The body is the primitive `assoc`, whose comparator opens a prompt of
    // its own — so when `assoc` returns, the stack top is not this call's.
    assert_program_eval_to(
        &format!(
            "{TAGS}(define (cmp a b)\n\
             \x20 (call-with-continuation-prompt (lambda () (equal? a b)) t2 (lambda (v k) 'h2)))\n\
             (define r (call-with-continuation-prompt assoc t (lambda (v k) (list 'H v))\n\
             \x20                                    2 '((1 a) (2 b)) cmp))\n\
             (list r (guard (e (#t 'no-prompt)) (abort-current-continuation t 'stale)))"
        ),
        "((2 b) no-prompt)",
    );

    // No prompt body at all: a parameter converter opens one inside the
    // nested loop its call runs on, and the loop must close it on the way out.
    assert_program_eval_to(
        &format!(
            "{TAGS}(define q (make-parameter 1\n\
             \x20 (lambda (v) (call-with-continuation-prompt (lambda () (* v 10)) t2 \
                 (lambda (a k) 'h2)))))\n\
             (q 42)\n\
             (list (q) (guard (e (#t 'no-prompt)) (abort-current-continuation t2 'stale)))"
        ),
        "(420 no-prompt)",
    );
}

/// The prompt body may be a primitive or a parameter object — issue #179,
/// fixed 2026-09-05.
///
/// **Racket 9.3 and Guile 3.0.11** both take those, and so did the
/// tree-walker; the VM called the body through `call_closure`, which accepts
/// a compiled closure and nothing else. It goes through `call_any` now, the
/// same dispatcher that already served the prompt's *handler*.
///
/// Not *any* procedure, which is what this test claimed until the review of
/// #179: `call_any` is the narrow dispatcher, so a VM-intercepted control
/// primitive as the body still fails — see
/// `a_control_primitive_as_the_prompt_body_fails_on_the_vm` below, which
/// pins that rather than leaving the gap inside a name that denies it.
///
/// What the change costs is the one thing worth remembering here: a primitive
/// or a parameter object finishes without a frame, so no `Return` comes to
/// carry the prompt off by depth, and `call-with-continuation-prompt` closes
/// its own prompt in that case.
#[test]
fn the_prompt_body_may_be_a_primitive_or_a_parameter() {
    const T: &str = "(define t (make-continuation-prompt-tag 'p))\n";
    // A parameter object, and a primitive given the arguments that follow the
    // handler.
    assert_program_eval_to("(call-with-continuation-prompt (make-parameter 7))", "7");
    assert_program_eval_to(
        &format!("{T}(call-with-continuation-prompt + t (lambda (v k) v) 1 2 3)"),
        "6",
    );
    // …and the prompt does not outlive such a body: nothing is left for a
    // later abort to find, which is the half a depth sweep cannot do here.
    assert_program_eval_to(
        &format!(
            "{T}(list (call-with-continuation-prompt (make-parameter 9) t (lambda (v k) 'h))\n\
             \x20     (guard (e (#t 'no-prompt)) (abort-current-continuation t 'stale)))"
        ),
        "(9 no-prompt)",
    );
    // A closure body still reaches its handler through an abort, which is the
    // path that does get a frame.
    assert_program_eval_to(
        &format!(
            "{T}(call-with-continuation-prompt\n\
             \x20 (lambda () (abort-current-continuation t 'ab)) t (lambda (v k) (list 'h v)))"
        ),
        "(h ab)",
    );
}

/// A VM-intercepted **control** primitive as the prompt body — issue #186,
/// closed 2026-09-05.
///
/// ```text
///   (call-with-continuation-prompt apply t (lambda (v k) 'h) + '(1 2 3))
///   tree-walker => 6
///   VM          => Undefined variable: patina.internal.control/apply
/// ```
///
/// Not introduced by #179 and not its to fix: `call_any` probed primitive →
/// parameter → continuation → closure, and a control primitive is claimed by
/// name *before* any of that, so it matched nothing and fell through to a
/// registry lookup that missed. Every caller of that dispatcher had the same
/// hole — `call-with-values`' consumer already did, and #179 made the prompt
/// *body* a second. `call_any` holds no probe of its own now; it is
/// `call_value` plus a frame-depth test, so a callee is callable here exactly
/// when it is callable from a `Call` instruction. The sibling row is
/// `tests/scheme/control/callability.scm`'s "apply as call-with-values' consumer"
/// rows (migrated from `callability.rs` by #193 Phase 0).
///
/// Kept as its own test rather than folded into the neighbour above, whose
/// name used to say "any procedure" and so denied this row existed.
#[test]
fn a_control_primitive_can_be_the_prompt_body() {
    const T: &str = "(define t (make-continuation-prompt-tag 'p))\n";
    // `apply`, whose callee needs a frame the prompt must not close early.
    assert_program_eval_to(
        &format!("{T}(call-with-continuation-prompt apply t (lambda (v k) 'h) + '(1 2 3))"),
        "6",
    );
    // `dynamic-wind`, which pushes a stub frame of the VM's own.
    assert_program_eval_to(
        &format!(
            "{T}(call-with-continuation-prompt dynamic-wind t (lambda (v k) 'h)\n\
             \x20 (lambda () 1) (lambda () 2) (lambda () 3))"
        ),
        "2",
    );
    // `values`, which needs no frame at all — the case that makes
    // `call-with-continuation-prompt` close its own prompt.
    assert_program_eval_to(
        &format!("{T}(call-with-continuation-prompt values t (lambda (v k) 'h) 5)"),
        "5",
    );
    // …and that prompt is gone: nothing is left for a later abort to land on.
    assert_program_eval_to(
        &format!(
            "{T}(list (call-with-continuation-prompt values t (lambda (v k) 'h) 5)\n\
             \x20     (guard (e (#t 'no-prompt)) (abort-current-continuation t 'stale)))"
        ),
        "(5 no-prompt)",
    );
    // A body that aborts to the prompt this very call pushed.
    assert_program_eval_to(
        &format!(
            "{T}(call-with-continuation-prompt abort-current-continuation t\n\
             \x20 (lambda (v k) (list 'h v)) t 'ab)"
        ),
        "(h ab)",
    );
}
