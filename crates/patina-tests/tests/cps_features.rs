//! The one CPS test that could not become a suite row.
//!
//! This file was #193's worked example of a mixed file. Phase 1 moved its
//! plain R7RS control flow to `tests/scheme/control/cps-features.scm`; Phase 2
//! moved the rest — the delimited-continuation half to
//! `tests/scheme/control/prompts.scm`, backtracking and a nested-extent
//! re-entry to `cps-features.scm` beside the rest.
//!
//! What is left runs the VM on a thread with an **explicitly sized stack**.
//! Choosing a stack size is a harness act; a Scheme program cannot ask for
//! one, so this test stays here whatever happens to anything else.

mod common;
use common::eval_program_vm;

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
#[test]
fn test_nested_value_form_winds_do_not_nest_rust_frames() {
    const DEPTH: usize = 5000;
    const STACK: usize = 1024 * 1024;
    let handle = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| {
            let program = format!(
                r#"
                (define dw dynamic-wind)
                (define (nest n)
                  (if (= n 0)
                      'done
                      (dw (lambda () #f) (lambda () (nest (- n 1))) (lambda () #f))))
                (nest {DEPTH})
                "#
            );
            assert_eq!(eval_program_vm(&program), "done");
        })
        .expect("spawn");
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}
