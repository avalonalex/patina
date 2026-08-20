//! `list-sort` is stable — a deliberate, marked deviation from the bundled
//! reference implementation (why: the PATINA LOCAL EDIT in
//! `lib/srfi/132/sort.scm` and `132.sld`'s header), pinned here so a
//! revert to upstream's tie-reversing heap sort cannot land silently.

mod common;
use common::eval_program as eval;

#[test]
fn test_list_sort_is_stable() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 132))
             (list-sort (lambda (x y) (> (cdr x) (cdr y)))
                        '((a . 1) (b . 2) (c . 1) (d . 2) (e . 1)))"
        ),
        "((b . 2) (d . 2) (a . 1) (c . 1) (e . 1))"
    );
}
