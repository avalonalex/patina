//! `list-sort` is stable — a deliberate deviation from the bundled
//! reference implementation, pinned here so it cannot silently revert.
//!
//! SRFI 132 does not require `list-sort` to be stable (that is
//! `list-stable-sort`'s contract), and Shivers' reference exercises the
//! freedom with a heap sort that *reverses* ties. But both references ship
//! a stable `list-sort` — chibi delegates to its native sort rather than to
//! this file of its own reference, and so does Gauche — and real code leans
//! on it: chibi-voting's `sort-pairs` orders pairs by count with residual
//! ties expected in input order, and the tie-reversing heap sort failed its
//! suite. The edit (an alias for `list-merge-sort`) is marked at the site
//! in `lib/srfi/132/sort.scm` and recorded in `lib/srfi/132.sld`'s header.

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

/// The aliases stay consistent: list-sort and list-stable-sort agree on
/// ties now, which is what makes the deviation safe to lean on.
#[test]
fn test_list_sort_agrees_with_list_stable_sort() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 132))
             (let ((xs '((a . 1) (b . 2) (c . 1) (d . 2) (e . 1)))
                   (lt (lambda (x y) (> (cdr x) (cdr y)))))
               (equal? (list-sort lt xs) (list-stable-sort lt xs)))"
        ),
        "#t"
    );
}
