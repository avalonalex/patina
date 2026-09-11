//! Escaping out of a callback that a Rust primitive is running.
//!
//! A higher-order primitive re-enters the VM to run its callback. If a
//! continuation captured *outside* is invoked *inside*, the nested frames are
//! popped rather than run to completion — and until 2026-08-15 the VM then
//! wrote the primitive's result through a register base belonging to a frame
//! that no longer existed, taking the process down with `index out of bounds`
//! in `set_reg_at`. The tree-walker returned the right answer throughout, so
//! it is the expectation here.
//!
//! The primitive is also *abandoned* now rather than left running on a stack
//! it no longer owns: `ApplyContext::apply_proc` returns a non-catchable
//! `ContinuationEscape` so the primitive unwinds through its own `?`, and the
//! dispatch loop takes the value from there.
//!
//! **One shape is still wrong** — a primitive used as a `call-with-values`
//! *consumer*, whose callback escapes; the frame-depth check structurally
//! cannot see it. Diagnosis in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6. Do not
//! read this file as the class being closed.
//!
//! # The tree-walker's side, closed 2026-09-10
//!
//! Its callbacks run on a nested trampoline, which used to start with every
//! stack empty and to read every continuation invoke inside the callback as
//! leaving the primitive — so the rest of the *program* ran from inside the
//! callback, and the callback's value became the form's. Each trampoline now
//! carries its caller's three stacks and an identity that captured
//! continuations record (`cps_eval/types.rs`), and a jump resumes in place,
//! escapes through the primitive, or reports a callback whose primitive has
//! returned, by that identity. The rows this file held as `assert_divergence`
//! quarantines while that was open are plain rows in
//! `tests/scheme/control/cps-features.scm` now, arbitrated by chibi and
//! Gauche, and so is everything else here that needs neither a file on disk
//! nor `eval`'s environment (the "Escaping out of a callback" section there).
//! What stays: the matrix below, whose port rows need real files and whose
//! `eval` row needs `interaction-environment` to see a script's definitions,
//! which Gauche's does not; the `eval` escape, for the same reason; and the
//! port-open test, for the files.

mod common;
use common::{assert_program_eval_to, scratch_path};
use tempfile::TempDir;

/// Every re-entrant primitive reachable without a file, **left by both kinds
/// of transfer**: a full continuation escaping out, and an
/// `abort-current-continuation` reaching a prompt outside. `'x` is what each
/// carries, so it is also what the whole expression must produce.
///
/// The escape half is the original sweep. The abort half is issue #177, and
/// the reason the sweep is parameterised rather than duplicated: the two
/// leave a re-entry boundary by *different* routes, and the boundary check
/// that catches one need not catch the other. It did not — an abort lands at
/// exactly the depth a callback returning normally leaves, so the frame-depth
/// test every boundary used read it as a return. `force` was fixed first and
/// alone; `eval` kept the old failure verbatim, and a parameter *set* stored
/// the abort's value into the parameter and never ran the handler.
///
/// That is twice this file's own promise — "every re-entrant primitive" — has
/// been narrower than it sounds. `eval` and the parameter set are listed now
/// because they were missing, not only because they broke.
#[test]
fn test_every_re_entrant_primitive_can_be_left_by_escape_and_by_abort() {
    let dir = TempDir::new().expect("temp dir");
    let input = scratch_path(&dir, "in.txt");
    std::fs::write(&input, "seed\n").expect("seed file");
    let output = scratch_path(&dir, "out.txt");

    // `LEAVE` is what the callback evaluates. Both transfers are spelled
    // through globals so that the `eval` row — which runs its form in a fresh
    // environment — can reach them like any other.
    const PRELUDE: &str = "(import (scheme base) (scheme lazy) (scheme file) \
                           (scheme eval) (scheme repl)) \
                           (define esc #f) \
                           (define t (make-continuation-prompt-tag 'p))";

    let bodies = [
        "(member 2 '(1 2 3) (lambda (a b) LEAVE))".to_string(),
        "(assoc 2 '((1 . a) (2 . b)) (lambda (a b) LEAVE))".to_string(),
        "(force (delay LEAVE))".to_string(),
        "(make-parameter 1 (lambda (v) LEAVE))".to_string(),
        // A parameter *set*, which reaches its converter through a different
        // boundary than `parameterize` does — the one that stored the abort's
        // value into the parameter.
        "(let ((q (make-parameter 0 (lambda (v) (if (= v 5) LEAVE v))))) (q 5))".to_string(),
        "(eval 'LEAVE (interaction-environment))".to_string(),
        "(call-with-port (open-output-string) (lambda (p) LEAVE))".to_string(),
        format!(r#"(call-with-input-file "{input}" (lambda (p) LEAVE))"#),
        format!(r#"(call-with-output-file "{output}" (lambda (p) LEAVE))"#),
    ];

    for (transfer, wrap, leave) in [
        (
            "escape",
            "(call/cc (lambda (k) (set! esc k) BODY))",
            "(esc 'x)",
        ),
        (
            "abort",
            "(call-with-continuation-prompt (lambda () BODY) t (lambda (v k2) v))",
            "(abort-current-continuation t 'x)",
            // Both backends since 2026-09-10. Until then the tree-walker ran
            // most of these callbacks on a nested trampoline that started
            // every stack empty, so the abort found no prompt — the
            // "primitive's callback" hole its winds and handlers had since
            // before prompts. Escaping always worked there, because a full
            // continuation carries its own target rather than searching for
            // one; an abort's landing now carries its prompt's trampoline,
            // so it unwinds through the primitive the same way.
        ),
    ] {
        for body in &bodies {
            let program = format!(
                "{PRELUDE} {}",
                wrap.replace("BODY", &body.replace("LEAVE", leave))
            );
            assert_eq!(common::eval_program(&program), "x", "[{transfer}] {body}");
        }
    }
}

/// `eval` and `load` re-enter the VM the same way a higher-order primitive
/// does, and the first attempt missed them: it detected the escape in
/// `apply_proc` alone, so `load` kept executing the remaining forms of the
/// file after the continuation had been invoked, where chibi stops at the
/// escaping form.
///
/// Both backends since 2026-09-10. The tree-walker used to escape *and then
/// continue*: the nested `eval` run had its own copy of the escape-catching
/// arm and resumed the outer continuation *inside* itself, then returned
/// into the primitive, which ran the lambda on — so `'fell-through` reached
/// the same continuation a second time, and in an SRFI 64 file every row
/// after this one ran twice. A trampoline now resumes only the continuations
/// whose chain ends in it; this one's ends in the outer run, so the escape
/// unwinds through `eval`. Rust rather than a suite row because Gauche's
/// `interaction-environment` does not see a script's top-level definitions.
#[test]
fn test_escaping_out_of_eval() {
    assert_program_eval_to(
        r#"(import (scheme base) (scheme eval) (scheme repl))
           (define kk #f)
           (define trace '())
           (define r (call/cc (lambda (k)
                       (set! kk k)
                       (eval '(kk 'from-eval) (interaction-environment))
                       (set! trace (cons 'ran-on trace))
                       'fell-through)))
           (list r (reverse trace))"#,
        "(from-eval ())",
    );
}

/// A continuation captured by one form of a loaded file and invoked from a
/// later form of the same file. `load` evaluates each form on its own
/// nested run; the earlier run has returned by the time `k` is invoked, and
/// what every implementation does is resume the rest of that form and then
/// carry on with the form *after the invoking one* — `r` is 11, the `if`
/// does not run again, `n` stays 1. The first cut of the trampoline fix
/// refused a continuation whose run had returned, and the review measured
/// this program failing where `main`, the VM, chibi and Gauche all agree.
/// Rust because it needs a file on disk.
#[test]
fn test_load_reenters_a_continuation_captured_by_an_earlier_form() {
    let dir = TempDir::new().expect("temp dir");
    let loaded = scratch_path(&dir, "loaded.scm");
    std::fs::write(
        &loaded,
        "(define k #f) (define n 0)\n\
         (define r (+ 1 (call/cc (lambda (c) (set! k c) 1))))\n\
         (set! n (+ n 1))\n\
         (if (< n 3) (k 10))\n\
         (define loaded-result (list r n))\n",
    )
    .expect("loaded file");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme load))
               (load "{loaded}")
               loaded-result"#
        ),
        "(11 1)",
    );
}

/// What the port primitives do with their port when the callback escapes.
/// R7RS 6.13.1: `call-with-port` closes the port "if `proc` returns" — and
/// only then, because a `guard` clause runs after that escape and is entitled
/// to read what the callback wrote, or to decide the port is still its own.
/// Until 2026-09-01 all three closed on every exit. Closing on the normal
/// return is the other half, so it is checked in the same program.
#[test]
fn test_an_escape_out_of_a_port_callback_leaves_the_port_open() {
    let dir = TempDir::new().expect("temp dir");
    let input = scratch_path(&dir, "in.txt");
    std::fs::write(&input, "seed\n").expect("seed file");
    let output = scratch_path(&dir, "out.txt");

    assert_program_eval_to(
        r#"(import (scheme base))
           (define out #f)
           (list
             (guard (e (#t (get-output-string out)))
               (call-with-port (open-output-string)
                 (lambda (p) (set! out p) (write-string "inside" p) (raise 'x))))
             (call-with-port (open-output-string)
               (lambda (p) (set! out p) 'returned))
             (output-port-open? out))"#,
        "(\"inside\" returned #f)",
    );
    // The condition is in the answer too: until 2026-09-10 the tree-walker
    // delivered a raise inside a primitive's callback to the outer `guard`
    // as an "unhandled exception" error object rather than as `'x`, and this
    // test left it out so as not to test that defect instead of its own
    // subject.
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file))
               (define seen #f)
               (list
                 (guard (e (#t (list e (read-char seen))))
                   (call-with-input-file "{input}"
                     (lambda (p) (set! seen p) (raise 'x))))
                 (input-port-open? (call-with-input-file "{input}" (lambda (p) p)))
                 (guard (e (#t (list e (output-port-open? seen))))
                   (call-with-output-file "{output}"
                     (lambda (p) (set! seen p) (raise 'y))))
                 (output-port-open? (call-with-output-file "{output}" (lambda (p) p))))"#
        ),
        "((x #\\s) #f (y #t) #f)",
    );
}
