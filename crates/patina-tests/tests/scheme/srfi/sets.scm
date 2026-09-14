;; SRFI 113 sets and bags: the linear-update set operations.
;;
;; `set-xor!` and `bag-xor!` hand their first argument to the shared `sob-xor!`
;; as the result as well as an operand. The reference implementation copied
;; what only the second argument has into that result before scanning the
;; first, so the scan met the copies as the first argument's own entries,
;; zeroed each against its count in the second, and cleaned them away:
;; `(set-xor! {1 2} {2 3})` answered `{1}`. Larceny's `set` suite asserts it,
;; but those assertions never ran here: the suite stops earlier, at an
;; argument-order slip of its own. Larceny triage family 44, and the PATINA
;; LOCAL EDIT in `lib/srfi/113/sets-impl.scm`.
;;
;; SRFI 113 lets a linear-update procedure reuse its first argument or not, so
;; every row compares values, written out, and never identity.
;;
;; Of the other operations that scan twice, `bag-sum!` has the same hazard and
;; is right only because it scans its first argument first: the counts it
;; writes back are never zero, so the second scan still reads membership
;; correctly. Union reads `max`, which comes out the same in either order, and
;; intersection and difference scan once.

(import (scheme base) (srfi 113) (srfi 128) (srfi 64))

(test-begin "sets")

(define cmp (make-default-comparator))

;; Order-free views, so no row depends on hash-table iteration order.
(define (insert-sorted x xs key)
  (cond ((null? xs) (list x))
        ((< (key x) (key (car xs))) (cons x xs))
        (else (cons (car xs) (insert-sorted x (cdr xs) key)))))
(define (sort-by key xs)
  (let loop ((xs xs) (acc '()))
    (if (null? xs)
        acc
        (loop (cdr xs) (insert-sorted (car xs) acc key)))))
(define (members s) (sort-by (lambda (x) x) (set->list s)))
(define (counts b) (sort-by car (bag->alist b)))

(test-equal "set-xor! keeps the elements only its second argument has"
  '(1 4 5)
  (members (set-xor! (set cmp 1 2 3) (set cmp 2 3 4 5))))

;; The count matters as well as the membership: 3 appears twice in the second
;; bag and not at all in the first.
(test-equal "bag-xor! keeps what only its second argument has, with its count"
  '((1 . 1) (2 . 1) (3 . 2))
  (counts (bag-xor! (bag cmp 1 1 2) (bag cmp 1 2 2 3 3))))

;; Both operands and the result are one set here, the other way the shared
;; result can alias.
(test-equal "set-xor! of a set with itself is empty"
  '()
  (let ((s (set cmp 1 2)))
    (members (set-xor! s s))))

(test-equal "bag-sum! counts what only its second argument has once"
  '((1 . 3) (2 . 3) (3 . 1))
  (counts (bag-sum! (bag cmp 1 1 2) (bag cmp 1 2 2 3))))

(test-end "sets")
