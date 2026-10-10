//! The CPS tests that could not become suite rows.
//!
//! This file was #193's worked example of a mixed file. Phase 1 moved its
//! plain R7RS control flow to `tests/scheme/control/cps-features.scm`; Phase 2
//! moved the rest — the delimited-continuation half to
//! `tests/scheme/control/prompts.scm`, backtracking and a nested-extent
//! re-entry to `cps-features.scm` beside the rest.
//!
//! What is left runs the VM on a thread with an **explicitly sized stack**.
//! Choosing a stack size is a harness act; a Scheme program cannot ask for
//! one, so this test stays here whatever happens to anything else. Beside it,
//! a library's body, which needs a `define-library` form in a program, where
//! chibi rejects one, so it has no oracle as a suite row; and what a closure
//! keeps alive, which needs `(patina debug)`'s `gc`.

mod common;
use common::{assert_program_eval_to, eval_program_vm};

/// A library's own `v_1` keeps its value through its body, on both backends
/// (#680). The tree-walker bound a continuation's parameter in the
/// environment the continuation captured, in a library's body the library's
/// own, and the CPS transform names the parameter that receives each form's
/// value `v_1`: the next body form overwrote the library's binding, which
/// read `#<unspecified>` after `(define unused …)`.
#[test]
fn a_librarys_binding_named_like_a_cps_temporary_keeps_its_value() {
    assert_program_eval_to(
        "(define-library (cps names) (export v_1) (import (scheme base))
           (begin (define v_1 'library) (define unused (car '(1 2)))))
         (import (prefix (cps names) lib:))
         lib:v_1",
        "library",
    );
}

/// A closure keeps the environment its body reads, and none of the
/// temporaries the CPS transform bound beside it, on both backends (#680).
/// The tree-walker's closure captured the frame it was made in, which can be
/// one of the transform's: here the frame that holds the call's first
/// argument, `(key!)`'s value, so the closure kept the key alive as long as
/// it lived.
#[test]
fn a_closure_keeps_no_temporary_made_beside_it() {
    assert_program_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define e #f)
         (define (key!) (let ((k (list 'key))) (set! e (make-ephemeron k 'datum)) k))
         (define (second a b) b)
         (define (make) (second (key!) (lambda () 'closure)))
         (define kept (make))
         (gc)
         (list (ephemeron-broken? e) (kept))",
        "(#t closure)",
    );
}

/// The value form of `dynamic-wind` costs a VM frame per nesting level, not a
/// Rust one — VM-only, on a deliberately small stack.
///
/// It used to run its body on a nested dispatch loop, so N nested value-form
/// extents meant N nested Rust calls: 5000 of them aborted the process with
/// `fatal runtime error: stack overflow` on `main` (`7e696892`, release,
/// macOS, 8 MB main-thread stack), while 4000 passed. Running the same
/// instructions head position compiles to, in a stub frame, removed the Rust
/// recursion along with the bugs it caused (issue #157): the whole nest is
/// heap-allocated `CallFrame`s now, and 20000 runs fine.
///
/// Three things about the shape of this test:
///
/// - It runs on a thread with an **explicitly sized** stack. A stack overflow
///   aborts the process rather than failing one test, so a default-stack
///   version took every test in this binary down with a bare `fatal runtime
///   error` naming none of them. `STACK` also makes the margin a property of
///   the test rather than of the build profile and platform it lands in — the
///   4000/5000 figures above were measured in release, and `cargo test` runs
///   at `opt-level = 1` across two CI platforms.
/// - `STACK` is a small multiple of what the fixed VM needs, which is a
///   constant: it passes at 256 KB, and `main` cannot reach 5000 at any size
///   this machine will give a thread.
/// - It is **VM-only**, unlike its neighbours. The tree-walker is a CPS
///   evaluator that does use Rust stack per level, so including it would
///   measure that instead and force `STACK` past 1 MB. Both backends are held
///   to nested value-form winds *semantically* by `cps-features.scm`'s
///   "re-entering nested value-form extents runs each thunk once".
/// - `dw` is a **parameter**, not `(define dw dynamic-wind)`: since #442 a
///   call through a global bound to `dynamic-wind` compiles to head
///   position's sequence, so that spelling no longer reaches the value form.
#[test]
fn test_nested_value_form_winds_do_not_nest_rust_frames() {
    const DEPTH: usize = 5000;
    const STACK: usize = 1024 * 1024;
    let handle = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| {
            let program = format!(
                r#"
                (define (nest dw n)
                  (if (= n 0)
                      'done
                      (dw (lambda () #f) (lambda () (nest dw (- n 1))) (lambda () #f))))
                (nest dynamic-wind {DEPTH})
                "#
            );
            assert_eq!(eval_program_vm(&program), "done");
        })
        .expect("spawn");
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}
