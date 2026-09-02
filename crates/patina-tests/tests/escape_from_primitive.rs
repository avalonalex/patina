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

mod common;
use common::{assert_program_eval_to, eval_program_vm, scratch_path};
use tempfile::TempDir;

/// Every re-entrant primitive reachable without a file, escaped out of. `'x`
/// is what the continuation is given, so it is also what the whole expression
/// must produce.
#[test]
fn test_a_continuation_can_escape_out_of_every_re_entrant_primitive() {
    let dir = TempDir::new().expect("temp dir");
    let input = scratch_path(&dir, "in.txt");
    std::fs::write(&input, "seed\n").expect("seed file");
    let output = scratch_path(&dir, "out.txt");

    for body in [
        "(member 2 '(1 2 3) (lambda (a b) (k 'x)))".to_string(),
        "(assoc 2 '((1 . a) (2 . b)) (lambda (a b) (k 'x)))".to_string(),
        "(force (delay (k 'x)))".to_string(),
        "(make-parameter 1 (lambda (v) (k 'x)))".to_string(),
        "(call-with-port (open-output-string) (lambda (p) (k 'x)))".to_string(),
        format!(r#"(call-with-input-file "{input}" (lambda (p) (k 'x)))"#),
        format!(r#"(call-with-output-file "{output}" (lambda (p) (k 'x)))"#),
    ] {
        assert_program_eval_to(
            &format!(
                "(import (scheme base) (scheme lazy) (scheme file)) \
                 (call/cc (lambda (k) {body}))"
            ),
            "x",
        );
    }
}

/// The bad register offset tracked frame depth rather than being a fixed
/// mistake, so escaping twice and from a nested depth is the case that would
/// catch an off-by-one "fix" working at one depth only.
#[test]
fn test_escaping_repeatedly_and_from_a_nested_depth() {
    assert_program_eval_to(
        r#"(import (scheme base))
           (define (run) (call/cc (lambda (k) (member 2 '(1 2 3) (lambda (a b) (k 'deep))))))
           (define (nested) (call/cc (lambda (k) (member 2 '(1 2) (lambda (a b) (k (run)))))))
           (list (run) (run) (nested))"#,
        "(deep deep deep)",
    );
}

/// A closure comparator that does *not* escape must still work — the guard
/// fires on frame depth, and one that fired spuriously would break every
/// re-entrant call. The primitive-comparator forms are covered in
/// `compliance/lists.rs` and `vm_callprimitive.rs`; these are the closure
/// forms, which are the ones that push a frame.
#[test]
fn test_a_closure_callback_that_does_not_escape_still_works() {
    assert_program_eval_to(
        r#"(import (scheme base))
           (list (member 2 '(1 2 3) (lambda (a b) (= a b)))
                 (assoc 2 '((1 . a) (2 . b)) (lambda (a b) (= a b))))"#,
        "((2 3) (2 . b))",
    );
}

/// Reaching the same primitives other than by call position. `apply` and
/// value-position dispatch go through `call_primitive_proc`, which has no
/// depth check of its own — they work because the escape is now signalled
/// from the re-entry boundary instead, and every route unwinds the same way.
#[test]
fn test_escaping_through_apply_and_value_position() {
    for form in [
        "(apply member (list 2 '(1 2 3) (lambda (a b) (k 'x))))",
        "(let ((ops (list member))) ((car ops) 2 '(1 2 3) (lambda (a b) (k 'x))))",
    ] {
        assert_program_eval_to(
            &format!("(import (scheme base) (scheme lazy)) (call/cc (lambda (k) {form}))"),
            "x",
        );
    }
}

/// The primitive stops when the continuation is invoked, instead of running
/// on to completion. `member` would otherwise keep calling the comparator for
/// the remaining elements — each call re-invoking the continuation.
#[test]
fn test_the_escaped_from_primitive_is_abandoned() {
    assert_program_eval_to(
        r#"(import (scheme base))
           (define seen '())
           (define r (call/cc (lambda (k)
                       (member 9 '(1 2 3)
                         (lambda (a b) (set! seen (cons b seen)) (k #f))))))
           (list r (reverse seen))"#,
        "(#f (1))",
    );
}

/// A continuation captured *and* invoked inside the callback, returning
/// normally, is not an escape — the primitive must run to completion. This is
/// the case a naive "any continuation invocation unwinds the primitive" rule
/// would break, and the reason the check is a frame-depth comparison rather
/// than a "was a continuation invoked" flag.
///
/// VM-only, deliberately: the tree-walker writes nothing for this program.
/// `assert_divergence` does not fit — it needs the broken backend to *fail*,
/// and this one succeeds with a wrong value — so the divergence is pinned in
/// `backend_divergence.rs` instead, where it will retire itself. chibi and
/// Gauche both agree with the VM's `(2 3)`.
#[test]
fn test_a_continuation_used_inside_the_callback_is_not_an_escape() {
    assert_eq!(
        eval_program_vm(
            r#"(import (scheme base))
               (member 2 '(1 2 3) (lambda (a b) (call/cc (lambda (k2) (k2 (= a b))))))"#
        ),
        "(2 3)",
    );
}

/// `eval` and `load` re-enter the VM the same way a higher-order primitive
/// does, and the first attempt missed them: it detected the escape in
/// `apply_proc` alone, so `load` kept executing the remaining forms of the
/// file after the continuation had been invoked, where chibi stops at the
/// escaping form.
///
/// VM-only: the tree-walker escapes *and then continues*, running the
/// fall-through as well. Pinned as a divergence in `backend_divergence.rs`.
#[test]
fn test_escaping_out_of_eval() {
    assert_eq!(
        eval_program_vm(
            r#"(import (scheme base) (scheme eval) (scheme repl))
               (define kk #f)
               (call/cc (lambda (k)
                 (set! kk k)
                 (eval '(kk 'from-eval) (interaction-environment))
                 'fell-through))"#
        ),
        "from-eval",
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
    // The condition itself is deliberately not in the answer: on the
    // tree-walker a raise inside a primitive's callback reaches the outer
    // `guard` as an "unhandled exception" error object rather than as `'x` —
    // the nested-trampoline defect of Track L §6, not this test's subject.
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file))
               (define seen #f)
               (list
                 (guard (e (#t (read-char seen)))
                   (call-with-input-file "{input}"
                     (lambda (p) (set! seen p) (raise 'x))))
                 (input-port-open? (call-with-input-file "{input}" (lambda (p) p)))
                 (guard (e (#t (output-port-open? seen)))
                   (call-with-output-file "{output}"
                     (lambda (p) (set! seen p) (raise 'y))))
                 (output-port-open? (call-with-output-file "{output}" (lambda (p) p))))"#
        ),
        "(#\\s #f #t #f)",
    );
}

/// The parameter *set* path, which runs a converter through a different
/// boundary than `make-parameter` construction does. Before this it lost the
/// enclosing top-level `define` outright on the VM.
#[test]
fn test_escaping_out_of_a_parameter_converter_during_parameterize() {
    assert_program_eval_to(
        r#"(import (scheme base))
           (define kk #f)
           (define p (make-parameter 0 (lambda (v) (if kk (kk 'from-converter) v))))
           (define r (call/cc (lambda (k) (set! kk k) (parameterize ((p 1)) 'done))))
           r"#,
        "from-converter",
    );
}
