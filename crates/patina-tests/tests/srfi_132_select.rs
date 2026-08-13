//! SRFI 132 selection procedures at sizes past `just-sort-it-threshold`.
//!
//! `vector-select!` and `vector-find-median` only enter their real quickselect
//! path once the range reaches 50 elements (`just-sort-it-threshold` in
//! `lib/srfi/132/select.scm`); below that they punt to a full sort. The bundled
//! reference suite never exceeds ~12 elements, so nothing exercised the
//! pivot-choosing RNG — which shipped calling unbound identifiers and crashed
//! every large range (audit item A1, `PRD/ARCHIVE/AUDIT_2026_08_10_PRD.md`).

mod common;
use common::eval_program as eval;

fn srfi132(expr: &str) -> String {
    eval(&format!("(import (scheme base) (srfi 132)) {expr}"))
}

/// A 100-element reversed vector: the k-th smallest is simply k.
#[test]
fn test_vector_select_past_the_sort_threshold() {
    assert_eq!(
        srfi132(
            "(let ((v (make-vector 100)))\
               (do ((i 0 (+ i 1))) ((= i 100)) (vector-set! v i (- 99 i)))\
               (vector-select! < v 50))"
        ),
        "50"
    );
}

#[test]
fn test_vector_find_median_past_the_sort_threshold() {
    assert_eq!(
        srfi132(
            "(let ((v (make-vector 61)))\
               (do ((i 0 (+ i 1))) ((= i 61)) (vector-set! v i (- 60 i)))\
               (vector-find-median < v 0))"
        ),
        "30"
    );
}

/// Duplicates drive the equal-to-pivot partition paths; repeated selection on
/// the same vector is fine because vector-select! only permutes.
#[test]
fn test_vector_select_with_duplicates() {
    assert_eq!(
        srfi132(
            "(let ((v (make-vector 90)))\
               (do ((i 0 (+ i 1))) ((= i 90)) (vector-set! v i (modulo i 3)))\
               (list (vector-select! < v 10)\
                     (vector-select! < v 45)\
                     (vector-select! < v 89)))"
        ),
        "(0 1 2)"
    );
}
