;; SRFI 113 sets and bags: linear updates, independent copies and products.
;;
;; `set-xor!` and `bag-xor!` hand their first argument to the shared `sob-xor!`
;; as the result as well as an operand. The reference implementation copied
;; what only the second argument has into that result before scanning the
;; first, so the scan met the copies as the first argument's own entries,
;; zeroed each against its count in the second, and cleaned them away:
;; `(set-xor! {1 2 3} {2 3 4 5})`, the first row below, answered `{1}`.
;; Larceny's `set` suite has assertions for this that never ran here: the
;; suite stops earlier, at an argument-order slip of its own. Larceny triage
;; family 44, and the PATINA LOCAL EDIT in `lib/srfi/113/sets-impl.scm`.
;;
;; SRFI 113 lets a linear-update procedure reuse its first argument or not, so
;; the linear-update rows compare values, written out, and never identity.
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

;; #326: the non-! set theory procedures and bag-sum promise newly allocated
;; results, including with one argument. Check independence by updating the
;; result: this also catches two records sharing a mutable hash table. An
;; entirely functional implementation may share objects (SRFI 113's exception)
;; and still passes, since its linear updates cannot change the source.
(define (set-copy-observations op elements)
  (let* ((source (apply set cmp elements))
         (result (op source))
         (updated (set-adjoin! result 9)))
    (list (members source) (members updated)
          (eq? cmp (set-element-comparator updated)))))

(define (bag-copy-observations op elements)
  (let* ((source (apply bag cmp elements))
         (result (op source))
         (updated (bag-adjoin! result 9 9)))
    (list (counts source) (counts updated)
          (eq? cmp (bag-element-comparator updated)))))

(test-equal "one-argument set-union returns an independent set"
  '((1 2) (1 2 9) #t) (set-copy-observations set-union '(1 2)))
(test-equal "one-argument set-intersection returns an independent set"
  '((1 2) (1 2 9) #t) (set-copy-observations set-intersection '(1 2)))
(test-equal "one-argument set-difference returns an independent set"
  '((1 2) (1 2 9) #t) (set-copy-observations set-difference '(1 2)))
(test-equal "one-argument bag-union returns an independent bag"
  '(((1 . 2) (2 . 1)) ((1 . 2) (2 . 1) (9 . 2)) #t)
  (bag-copy-observations bag-union '(1 1 2)))
(test-equal "one-argument bag-intersection returns an independent bag"
  '(((1 . 2) (2 . 1)) ((1 . 2) (2 . 1) (9 . 2)) #t)
  (bag-copy-observations bag-intersection '(1 1 2)))
(test-equal "one-argument bag-difference returns an independent bag"
  '(((1 . 2) (2 . 1)) ((1 . 2) (2 . 1) (9 . 2)) #t)
  (bag-copy-observations bag-difference '(1 1 2)))
(test-equal "one-argument bag-sum returns an independent bag"
  '(((1 . 2) (2 . 1)) ((1 . 2) (2 . 1) (9 . 2)) #t)
  (bag-copy-observations bag-sum '(1 1 2)))
(test-equal "one-argument set operations copy empty sets independently"
  '((() (9) #t) (() (9) #t) (() (9) #t))
  (map (lambda (op) (set-copy-observations op '()))
       (list set-union set-intersection set-difference)))
(test-equal "one-argument bag operations copy empty bags independently"
  '((() ((9 . 2)) #t) (() ((9 . 2)) #t)
    (() ((9 . 2)) #t) (() ((9 . 2)) #t))
  (map (lambda (op) (bag-copy-observations op '()))
       (list bag-union bag-intersection bag-difference bag-sum)))

;; Multiplying a count by zero leaves no occurrences. All views of the bag
;; must agree: a zero-sized bag cannot retain members or visit unique keys.
(define (zero-product-observations op elements)
  (let ((result (op 0 (apply bag cmp elements))))
    (list (bag? result) (bag-empty? result) (bag-size result)
          (bag-unique-size result) (bag-contains? result 1)
          (bag-element-count result 1) (counts result)
          (bag-fold-unique (lambda (elem count visits) (+ visits 1)) 0 result)
          (eq? cmp (bag-element-comparator result)))))

(test-equal "bag-product by zero removes every occurrence"
  '(#t #t 0 0 #f 0 () 0 #t)
  (zero-product-observations bag-product '(1 1 2)))
(test-equal "bag-product! by zero removes every occurrence"
  '(#t #t 0 0 #f 0 () 0 #t)
  (zero-product-observations bag-product! '(1 1 2)))
(test-equal "bag-product by zero accepts an empty bag"
  '(#t #t 0 0 #f 0 () 0 #t)
  (zero-product-observations bag-product '()))
(test-equal "bag-product! by zero accepts an empty bag"
  '(#t #t 0 0 #f 0 () 0 #t)
  (zero-product-observations bag-product! '()))

(define (positive-product-counts op)
  (map (lambda (n) (counts (op n (bag cmp 1 1 2)))) '(1 3)))
(test-equal "bag-product preserves or scales positive counts"
  '(((1 . 2) (2 . 1)) ((1 . 6) (2 . 3)))
  (positive-product-counts bag-product))
(test-equal "bag-product! preserves or scales positive counts"
  '(((1 . 2) (2 . 1)) ((1 . 6) (2 . 3)))
  (positive-product-counts bag-product!))
(test-equal "bag-product leaves its input unchanged at zero one and three"
  '(((1 . 2) (2 . 1)) ((1 . 2) (2 . 1)) ((1 . 2) (2 . 1)))
  (map (lambda (n)
         (let* ((source (bag cmp 1 1 2))
                (result (bag-product n source)))
           (bag-adjoin! result 9)
           (counts source)))
       '(0 1 3)))

;; The SRFI does not specify diagnostics for invalid multipliers. Patina
;; chooses exact nonnegative integers so multiplicities remain exact counts,
;; and checks before any mutation, even when the input bag is empty. Oracle
;; differences here are spec-silent, not claims of mandatory SRFI errors.
(define invalid-multipliers '(-1 -1/2 1/2 2.0 0.0 +inf.0 +nan.0 invalid))
(define (invalid-product-observations op)
  (map (lambda (n)
         (map (lambda (elements)
                (let* ((source (apply bag cmp elements))
                       (before (counts source))
                       (raised? (guard (e (else #t)) (op n source) #f)))
                  (list raised? (equal? before (counts source)))))
              '(() (1 1 2))))
       invalid-multipliers))
(test-equal "bag-product rejects invalid multipliers before changing its input"
  (make-list 8 '((#t #t) (#t #t)))
  (invalid-product-observations bag-product))
(test-equal "bag-product! rejects invalid multipliers before changing its input"
  (make-list 8 '((#t #t) (#t #t)))
  (invalid-product-observations bag-product!))

(test-end "sets")
