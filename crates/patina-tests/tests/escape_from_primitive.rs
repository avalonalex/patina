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
//! **These cover the primitive in call position only.** Reaching the same
//! primitives through `apply`, through a value (`((car ops) …)`) or through
//! `call-with-values` goes via `call_primitive_proc`, which has no equivalent
//! check and is still wrong — and none of these paths *abandons* the
//! primitive, which keeps running on a stack it no longer owns. Both are
//! recorded as open in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6, with repros.
//! Do not read this file as the class being closed.

mod common;
use common::{assert_program_eval_to, scratch_path};
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
