//! Garbage collection through real evaluation — **VM** backend.
//!
//! The backend-independent cases live in `common::gc_shared_tests!` and are
//! invoked here against `eval_program_vm`, so the VM is covered in the same
//! test lane as the tree-walker regardless of which backend the
//! feature-switched helpers select.
//!
//! This file keeps only the tests that target VM machinery a heap scan cannot
//! reach: the continuation side tables, `CallFrame::closure` (a bare
//! `HeapIndex`), boxed upvalues, and the multi-value `value_buffer`. See
//! `docs/GC_DESIGN.md` §5.2.

#[macro_use]
mod common;
use common::*;

gc_shared_tests!(eval_program_vm);

#[test]
fn closure_free_vars_survive_collection() {
    // `CallFrame::closure` is a bare HeapIndex, not a TaggedValue — if it
    // were left untraced, the running closure itself would be swept.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (make-counter)
          (let ((n 0))
            (lambda () (gc) (set! n (+ n 1)) n)))
        (define c (make-counter))
        (c)
        (c)
        (c)
        "#,
        "3",
    );
}

#[test]
fn mutable_boxed_upvalue_survives_collection() {
    // `set!` on a captured variable boxes it as a MutableCell reached only
    // through the closure's free_vars.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (make-acc)
          (let ((total '()))
            (lambda (x) (set! total (cons x total)) (gc) total)))
        (define a (make-acc))
        (a 1) (a 2)
        (a 3)
        "#,
        "(3 2 1)",
    );
}

#[test]
fn multiple_values_survive_collection() {
    // `value_buffer` is the multi-value side channel — a root that is easy
    // to miss because it is empty most of the time.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (call-with-values
          (lambda () (gc) (values 1 2 3))
          (lambda (a b c) (+ a b c)))
        "#,
        "6",
    );
}
