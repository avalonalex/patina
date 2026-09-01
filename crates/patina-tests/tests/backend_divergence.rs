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

/// An error raised *after* a continuation escape is catchable on both
/// backends — converged 2026-09-01 when `CpsContinuation` gained the handler
/// stack.
///
/// Delivering two values to a single-value context is unspecified in R7RS,
/// and the references take different options: chibi and the VM let `+` raise
/// on the `#<values>` object, which a `guard` around it catches; Gauche
/// delivers the first value and answers `2`. What was not an option was the
/// tree-walker's answer — the type error aborted the program with the
/// `guard`'s handler stack gone.
///
/// It became reachable on 2026-08-25, when a multi-value continuation
/// invocation stopped raising a wrong-arity error at the call site (where it
/// *was* catchable) and started escaping, and the escape path reset the
/// handler stack to empty. It carries the captured stack now.
#[test]
fn an_error_after_a_multi_value_escape_is_catchable() {
    assert_program_eval_to(
        "(guard (e (#t (list 'caught))) (+ 1 (call/cc (lambda (k) (k 1 2)))))",
        "(caught)",
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

/// Tree-walker: a `guard` handler runs *inside* the dynamic extent of the
/// erroring expression, before `dynamic-wind`'s after-thunk.
///
/// ```text
///   VM, chibi, Gauche => (before after handler)
///   tree-walker       => (before handler after)
/// ```
///
/// R7RS §4.2.7 puts the unwind first and both references agree with the VM.
/// Not cosmetic: a handler writing to `current-output-port` writes into
/// whatever the un-unwound extent installed, which is how it was found.
///
/// Not `assert_divergence` — that needs the broken backend to *fail*, and this
/// one returns a plausible wrong answer.
#[test]
fn guard_handler_runs_before_the_unwind_on_the_tree_walker() {
    const PROGRAM: &str = r#"
        (define log '())
        (guard (e (#t (set! log (cons 'handler log))))
          (dynamic-wind (lambda () (set! log (cons 'before log)))
                        (lambda () (error "x"))
                        (lambda () (set! log (cons 'after log)))))
        (reverse log)
    "#;
    assert_eq!(
        eval_program_vm(PROGRAM),
        "(before after handler)",
        "the VM matches chibi and Gauche; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "(before handler after)",
        "\n[tree-walker] NO LONGER DIVERGES — it now unwinds before the handler.\n\
         Replace both assertions with assert_program_eval_to(PROGRAM, \
         \"(before after handler)\") and update {GUARD_UNWIND_ORDER}."
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

/// VM: invoking a continuation captured inside its own `dynamic-wind` extent
/// re-runs the wind thunks, as though the extent had been exited and
/// re-entered.
///
/// ```text
///   (dynamic-wind in (lambda () (call/cc (lambda (k) (k #f)))) out)
///   tree-walker, chibi, Gauche => (in out)
///   VM                         => (in out in out)
/// ```
///
/// R7RS §6.10 runs the thunks when the extent is actually left and re-entered;
/// invoking `k` here never leaves it. Pinned because this defect was tracked
/// in the PRD, lost in an edit, and recovered only by review — a test cannot
/// be edited away by accident.
#[test]
fn continuation_within_its_own_wind_reruns_the_thunks_on_the_vm() {
    const PROGRAM: &str = r#"
        (import (scheme base))
        (define log '())
        (dynamic-wind (lambda () (set! log (cons 'in log)))
                      (lambda () (call/cc (lambda (k) (k #f))))
                      (lambda () (set! log (cons 'out log))))
        (reverse log)
    "#;
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "(in out)",
        "the tree-walker matches chibi and Gauche; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_vm(PROGRAM),
        "(in out in out)",
        "\n[vm] NO LONGER DIVERGES — it now runs the wind thunks once.\n\
         Replace both assertions with assert_program_eval_to(PROGRAM, \"(in out)\") \
         and update {GUARD_UNWIND_ORDER}."
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

// ─── the nested trampoline, in its resource-corruption form ──────────────────

/// Tree-walker: `I/O error: port is closed`.
///
/// A `call/cc` retry loop inside a `call-with-port` callback. The callback has
/// not returned — the continuation resumes *inside* it — but the tree-walker's
/// nested trampoline reads the invoke as an escape and closes the port, so the
/// next write in the same callback fails on a port the program still holds.
///
/// The root cause is the already-tracked nested-trampoline defect (Track L §6):
/// wind and callback thunks run on a trampoline that does not share the outer
/// one's state, so what came back through Rust cannot be told from what
/// escaped. What is new here is the *manifestation* — a resource closed while
/// still in use, rather than an error routed to the wrong handler — which is
/// why it earns its own row (audit F6). VM and chibi both answer `"012"`.
#[test]
fn call_with_port_closes_the_port_on_an_in_extent_continuation_invoke() {
    assert_divergence(
        r#"(call-with-port (open-output-string)
             (lambda (p)
               (let ((n 0))
                 (let ((k (call/cc (lambda (c) c))))
                   (write-string (number->string n) p)
                   (set! n (+ n 1))
                   (if (< n 3) (k k)))
                 (get-output-string p))))"#,
        On::Vm,
        "\"012\"",
        ErrorClass::AtRuntime,
        GUARD_UNWIND_ORDER,
    );
}

/// A continuation escaping from an *after* thunk skips the enclosing after
/// thunk on the tree-walker.
///
/// R7RS 6.10: `dynamic-wind`'s third thunk runs whenever control leaves the
/// dynamic extent, and calling `k` from inside one is still leaving — the
/// outer wind has not finished unwinding, so its own after thunk is still
/// owed. The VM pays it; the tree-walker stops at the inner one and never
/// runs `outer-after`.
///
/// Found when Larceny's `base` suite began loading (families 14/15/23): it is
/// the one assertion in that suite the two backends answer differently, and
/// it counts the winds rather than naming them, so upstream reads 7 against
/// the tree-walker's 3. Quarantined with explicit per-backend assertions
/// rather than `assert_divergence` because the broken side returns a value.
///
/// chibi cannot arbitrate this one: re-entering `k` from an after thunk sends
/// it into an unbounded loop. Gauche and the suite's own expectation agree
/// with the VM.
#[test]
fn a_continuation_from_an_after_thunk_skips_the_outer_after_on_the_tree_walker() {
    const PROGRAM: &str = r#"
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
    "#;
    assert_eq!(
        eval_program_vm(PROGRAM),
        "(from-after (outer-before inner-before body inner-after outer-after))",
        "the VM runs every after thunk it owes; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "(from-after (outer-before inner-before body inner-after))",
        "\n[tree-walker] NO LONGER DIVERGES — it now runs the outer after thunk.\n\
         Replace both assertions with a single assert_program_eval_to on the VM's \
         answer and close the entry in scheme_tests/reports/larceny_triage.md."
    );
}
