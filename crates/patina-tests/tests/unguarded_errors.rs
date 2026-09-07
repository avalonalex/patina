//! Programs that must fail when nothing guards them.
//!
//! This is the Rust home for one assertion kind, collected here so that
//! migrating a file to `tests/scheme/` does not have to keep a whole test
//! binary alive for two rows (#193 Phase 1).
//!
//! # Why a `.scm` file cannot hold these
//!
//! SRFI 64's `test-error` runs its body inside `call/cc` and
//! `with-exception-handler` (`%test-error`), so it asserts that an error
//! *reaches a handler* — which is what the paired
//! `(test-equal 'caught (guard …))` row in the migrated file already checks.
//! Whether the same error also escapes an **unguarded** program is observable
//! only from outside the program, so it needs a caller, and the caller is Rust.
//!
//! Both halves matter and they are not the same claim. Routing changes *where*
//! a catchable error is delivered, never whether it is raised; a regression
//! that routed these to handlers while breaking the top-level path would pass
//! every `test-error` row and fail nothing. That is the shape of defect #71,
//! which is why the pairing exists at all.
//!
//! # What belongs here
//!
//! One row per program, grouped by the file its guarded half lives in, and
//! *only* the unguarded claim. Anything about which stage rejected the program,
//! or which diagnostic it produced, belongs with the feature's own tests —
//! `assert_program_eval_error` deliberately asserts no more than "this failed",
//! because the two backends word their diagnostics differently and error text
//! is not a stable interface here.

mod common;
use common::assert_program_eval_error;

/// Guarded halves: `tests/scheme/control/parameters.scm`.
#[test]
fn parameterize_rejects_a_malformed_form() {
    // No body. R7RS §4.2.6 requires at least one expression.
    assert_program_eval_error("(define p (make-parameter 10)) (parameterize ((p 20)))");
    // A non-parameter in the binding position.
    assert_program_eval_error(r#"(parameterize ((42 20)) (display "hello"))"#);
}
