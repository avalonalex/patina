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
//! **The VM keeps one narrow dispatcher**, `call_any`, with the same probe set
//! the apply instructions shed — so a control primitive or continuation
//! reached through `call-with-values`, a prompt handler or an exception
//! handler still fails there:
//! `(call-with-values (lambda () (values + '(1 2))) apply)`. Pinned in
//! `callability.rs`, not here, because both backends do not disagree about it
//! — only the VM is wrong, and the tree-walker is right.

mod common;
use common::*;

const CONTROL_OPS: &str = "PRD/TRACK_Q_QUALITY_PRD.md §1.2";
const GUARD_UNWIND_ORDER: &str = "PRD/TRACK_L_SNOW_LIBRARIES_PRD.md §6";
// HANDLER_REENTRY (audit B3) is gone with the two rows that cited it: both
// converged on 2026-09-01 when `CpsContinuation` gained the handler stack.

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
/// Gauche and the tree-walker both give the two answers above. (chibi answers
/// `(val ())` to both; its continuation does not carry the `set!`s to `log`,
/// which is a different question from the one asked here, so it cannot
/// arbitrate this row.)
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
            (list (run (lambda (x) (set! log (cons x log)))) (reverse log))))
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

/// VM: a `guard` is gone after one of its clauses declines a
/// `raise-continuable`.
///
/// ```text
///   (with-exception-handler (lambda (e) (list 'I e))
///     (lambda () (guard (e ((eq? e 'y) 'caught-y))
///       (list (raise-continuable 'x) (raise-continuable 'y)))))
///   tree-walker, chibi, Gauche => caught-y
///   VM                         => ((I x) (I y))
/// ```
///
/// The `guard`'s handler declines `'x`, which re-raises it through
/// `handler-k` to the outer handler; that returns `(I x)`, and the body
/// continues to raise `'y` — which the `guard` must catch. On the VM the
/// continuable path in `vm_raise_value` pops the handler to run it and
/// re-pushes it only when the handler *returns*; `guard`'s handler leaves
/// through `handler-k` instead, so the re-push is skipped and the `guard` is
/// silently uninstalled for the rest of its body. Found by review of triage
/// families 22/28 (2026-09-01). The skipped re-push predates them; what they
/// changed is the symptom — with the old expansion this program answered
/// `(I x)` on the VM and died with a non-continuable error on the
/// tree-walker, so neither backend was right before. Recorded in
/// {GUARD_UNWIND_ORDER}.
#[test]
fn a_guard_that_declined_a_continuable_raise_is_gone_on_the_vm() {
    const PROGRAM: &str = r#"
        (with-exception-handler (lambda (e) (list 'I e))
          (lambda () (guard (e ((eq? e 'y) 'caught-y))
            (list (raise-continuable 'x) (raise-continuable 'y)))))
    "#;
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "caught-y",
        "the tree-walker keeps the guard installed; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_vm(PROGRAM),
        "((I x) (I y))",
        "\n[vm] NO LONGER DIVERGES — the guard survives a declined raise-continuable.\n\
         Replace both assertions with a single assert_program_eval_to on `caught-y` \
         and close the entry in PRD/TRACK_L_SNOW_LIBRARIES_PRD.md §6."
    );
}

/// VM: a handler that returns from a non-continuable `raise` is not an
/// error — the value it returned is delivered as the value of `(raise …)`.
///
/// R7RS 6.11: "If the handler returns, a secondary exception is raised in the
/// same dynamic environment as the handler." The tree-walker raises it, and
/// the outer `guard` sees the error object. The VM's non-continuable path in
/// `vm_raise_value` cannot tell a handler that returned from one that escaped,
/// because both come back through the same nested run loop, so it delivers
/// the handler's value to the raise's destination register as if the raise had
/// been continuable — `(returned)` here, and `+: expected number` if the
/// raise sat under an arithmetic primitive. Present on `main` before triage
/// families 22/28; recorded in {GUARD_UNWIND_ORDER}.
///
/// The second program is the same hole reached from a *primitive's* error
/// rather than `raise`: the run loop routes a catchable `VmError` through
/// `vm_raise_value` with register 0 as the destination, so the returning
/// handler's value lands in register 0 and `car`'s own destination is left
/// holding `()`.
#[test]
fn a_handler_returning_from_a_non_continuable_raise_is_not_an_error_on_the_vm() {
    const SECONDARY: &str = "(outer \"exception handler returned from non-continuable exception\")";
    for (program, vm_answer) in [
        (
            r#"(guard (o (#t (list 'outer (if (error-object? o) (error-object-message o) o))))
                 (with-exception-handler (lambda (e) 'returned)
                   (lambda () (list (raise 'x)))))"#,
            "(returned)",
        ),
        (
            r#"(guard (o (#t (list 'outer (if (error-object? o) (error-object-message o) o))))
                 (with-exception-handler (lambda (e) 'returned)
                   (lambda () (list (car 5)))))"#,
            "(())",
        ),
    ] {
        assert_eq!(
            eval_program_tree_walker(program),
            SECONDARY,
            "the tree-walker raises the secondary exception; if this changed, it regressed"
        );
        assert_eq!(
            eval_program_vm(program),
            vm_answer,
            "\n[vm] NO LONGER DIVERGES — a returning handler now raises the secondary \
             exception.\nReplace both assertions with a single assert_program_eval_to on \
             the tree-walker's answer and close the entry in \
             PRD/TRACK_L_SNOW_LIBRARIES_PRD.md §6."
        );
    }
}

/// VM: a continuation used *as* the handler for a primitive's error is a
/// fatal `continuation escaped past a synchronous boundary`.
///
/// `(call/cc (lambda (k) (with-exception-handler k thunk)))` is R7RS's own
/// idiom for capturing a raised object, and triage family 24 made it work for
/// `raise` on the VM. A `VmError` from a primitive takes the run loop's route
/// into `vm_raise_value` instead, and `call_any`'s continuation signal is not
/// caught on that path. The tree-walker answers `#t`, as chibi and Gauche do.
/// Recorded in {GUARD_UNWIND_ORDER}.
#[test]
fn a_continuation_as_the_handler_for_a_primitive_error_is_fatal_on_the_vm() {
    assert_divergence(
        "(error-object? (call/cc (lambda (k) (with-exception-handler k (lambda () (car 5))))))",
        On::TreeWalker,
        "#t",
        ErrorClass::AtRuntime,
        GUARD_UNWIND_ORDER,
    );
}
