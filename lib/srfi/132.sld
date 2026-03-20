;; SRFI 132: Sort Libraries
;;
;; Reference: https://srfi.schemers.org/srfi-132/
;; Original author: John Cowan (based on SRFI 95 by Aubrey Jaffer)
;; License: MIT
;;
;; Pure Scheme implementation using merge sort. Does not depend on
;; SRFI 95 or native sort primitives.

(define-library (srfi 132)
  (import (scheme base))

  (export
   list-sorted? vector-sorted?
   list-sort list-stable-sort
   list-sort! list-stable-sort!
   vector-sort vector-stable-sort
   vector-sort! vector-stable-sort!
   list-merge list-merge!
   vector-merge vector-merge!
   list-delete-neighbor-dups
   list-delete-neighbor-dups!
   vector-delete-neighbor-dups
   vector-delete-neighbor-dups!
   vector-find-median
   vector-find-median!
   vector-select!
   vector-separate!)

  ;; Internal sort primitives (merge sort)
  (include "132/sort.scm")

  (begin
    ;; ── Sorted predicates ──────────────────────────────────────────

    (define (list-sorted? less ls)
      (or (null? ls)
          (null? (cdr ls))
          (let lp ((prev (car ls)) (rest (cdr ls)))
            (or (null? rest)
                (and (not (less (car rest) prev))
                     (lp (car rest) (cdr rest)))))))

    (define (vector-sorted? less vec . o)
      (let ((start (if (pair? o) (car o) 0))
            (end (if (and (pair? o) (pair? (cdr o)))
                     (cadr o)
                     (vector-length vec))))
        (or (<= (- end start) 1)
            (let lp ((i (+ start 1)))
              (or (>= i end)
                  (and (not (less (vector-ref vec i)
                                  (vector-ref vec (- i 1))))
                       (lp (+ i 1))))))))

    ;; ── List sort ──────────────────────────────────────────────────

    (define (list-sort less ls) (%list-sort less (list-copy ls)))
    (define list-stable-sort list-sort)

    (define (list-sort! less ls) (%list-sort less ls))
    (define list-stable-sort! list-sort!)

    ;; ── Vector sort ────────────────────────────────────────────────

    (define (vector-sort less vec . o)
      (let* ((start (if (pair? o) (car o) 0))
             (end (if (and (pair? o) (pair? (cdr o)))
                      (cadr o)
                      (vector-length vec)))
             (result (vector-copy vec start end)))
        (%vector-sort! less result 0 (vector-length result))
        result))

    (define vector-stable-sort vector-sort)

    (define (vector-sort! less vec . o)
      (let ((start (if (pair? o) (car o) 0))
            (end (if (and (pair? o) (pair? (cdr o)))
                     (cadr o)
                     (vector-length vec))))
        (%vector-sort! less vec start end)))

    (define vector-stable-sort! vector-sort!)

    ;; ── Merge ──────────────────────────────────────────────────────

    (define (list-merge less ls1 ls2) (%list-merge less ls1 ls2))
    (define (list-merge! less ls1 ls2) (%list-merge! less ls1 ls2))

    (define (vector-merge less vec1 vec2 . o)
      (let* ((s1 (if (pair? o) (car o) 0))
             (e1 (if (and (pair? o) (pair? (cdr o)))
                     (cadr o)
                     (vector-length vec1)))
             (s2 (if (and (pair? o) (pair? (cdr o)) (pair? (cddr o)))
                     (caddr o)
                     0))
             (e2 (if (and (pair? o) (pair? (cdr o)) (pair? (cddr o))
                          (pair? (cdddr o)))
                     (cadddr o)
                     (vector-length vec2)))
             (len1 (- e1 s1))
             (len2 (- e2 s2))
             (res (make-vector (+ len1 len2))))
        (let lp ((i 0) (i1 s1) (i2 s2))
          (cond
           ((and (>= i1 e1) (>= i2 e2)) res)
           ((or (>= i1 e1)
                (and (< i2 e2)
                     (less (vector-ref vec2 i2) (vector-ref vec1 i1))))
            (vector-set! res i (vector-ref vec2 i2))
            (lp (+ i 1) i1 (+ i2 1)))
           (else
            (vector-set! res i (vector-ref vec1 i1))
            (lp (+ i 1) (+ i1 1) i2))))))

    (define (vector-merge! less to vec1 vec2 . o)
      (let* ((at (if (pair? o) (car o) 0))
             (res (apply vector-merge less vec1 vec2
                         (if (pair? o) (cdr o) '()))))
        (vector-copy! to at res)))

    ;; ── Delete neighbor dups ───────────────────────────────────────

    (define (list-delete-neighbor-dups eq ls)
      (let lp ((ls ls) (res '()))
        (cond ((null? ls) (reverse res))
              ((and (pair? res) (eq (car res) (car ls))) (lp (cdr ls) res))
              (else (lp (cdr ls) (cons (car ls) res))))))

    (define (list-delete-neighbor-dups! eq ls)
      (when (pair? ls)
        (let lp ((ls (cdr ls)) (start ls))
          (cond ((null? ls) (set-cdr! start '()))
                ((eq (car start) (car ls)) (lp (cdr ls) start))
                (else (set-cdr! start ls) (lp (cdr ls) ls)))))
      ls)

    (define (vector-delete-neighbor-dups eq vec . o)
      (if (zero? (vector-length vec))
          vec
          (let ((ls (apply vector->list vec o)))
            (list->vector (list-delete-neighbor-dups eq ls)))))

    (define (vector-delete-neighbor-dups! eq vec . o)
      (let ((start (if (pair? o) (car o) 0))
            (end (if (and (pair? o) (pair? (cdr o)))
                     (cadr o)
                     (vector-length vec))))
        (cond
         ((<= end start) start)
         (else
          (let lp ((i (+ start 1)) (fill (+ start 1)))
            (cond
             ((>= i end) fill)
             ((eq (vector-ref vec (- i 1)) (vector-ref vec i))
              (lp (+ i 1) fill))
             (else
              (when (> i fill)
                (vector-set! vec fill (vector-ref vec i)))
              (lp (+ i 1) (+ fill 1)))))))))

    ;; ── Selection and median ───────────────────────────────────────

    (define (choose-pivot less vec left right)
      (let* ((mid (quotient (+ left right) 2))
             (a (vector-ref vec left))
             (b (vector-ref vec mid))
             (c (vector-ref vec right)))
        (if (less a b)
            (if (less b c) mid (if (less a c) right left))
            (if (less a c) left (if (less b c) right mid)))))

    (define (vector-partition! less vec left right pivot)
      (let ((elt (vector-ref vec pivot)))
        (let ((tmp (vector-ref vec pivot)))
          (vector-set! vec pivot (vector-ref vec right))
          (vector-set! vec right tmp))
        (let lp ((i left) (j left))
          (cond
           ((= i right)
            (let ((tmp (vector-ref vec i)))
              (vector-set! vec i (vector-ref vec j))
              (vector-set! vec j tmp))
            j)
           ((less (vector-ref vec i) elt)
            (let ((tmp (vector-ref vec i)))
              (vector-set! vec i (vector-ref vec j))
              (vector-set! vec j tmp))
            (lp (+ i 1) (+ j 1)))
           (else
            (lp (+ i 1) j))))))

    (define (vector-select! less vec k . o)
      (let* ((left (if (pair? o) (car o) 0))
             (k (+ k left)))
        (when (not (<= 0 k (vector-length vec)))
          (error "k out of range" vec k))
        (let select ((left left)
                     (right (- (if (and (pair? o) (pair? (cdr o)))
                                   (cadr o)
                                   (vector-length vec))
                               1)))
          (if (>= left right)
              (vector-ref vec left)
              (let* ((pivot (choose-pivot less vec left right))
                     (pivot-index (vector-partition! less vec left right pivot)))
                (cond
                 ((= k pivot-index) (vector-ref vec k))
                 ((< k pivot-index) (select left (- pivot-index 1)))
                 (else (select (+ pivot-index 1) right))))))))

    (define (vector-separate! less vec k . o)
      (apply vector-select! less vec k o)
      (if #f #f))

    (define (vector-find-median! less vec knil . o)
      (vector-sort! less vec)
      (let* ((len (vector-length vec))
             (mid (quotient len 2))
             (mean (if (pair? o) (car o) (lambda (a b) (/ (+ a b) 2)))))
        (cond
         ((zero? len) knil)
         ((odd? len) (vector-ref vec mid))
         (else (mean (vector-ref vec (- mid 1)) (vector-ref vec mid))))))

    (define (vector-find-median less vec knil . o)
      (let* ((vec (vector-copy vec))
             (len (vector-length vec))
             (mid (quotient len 2))
             (mean (if (pair? o) (car o) (lambda (a b) (/ (+ a b) 2)))))
        (cond
         ((zero? len) knil)
         (else
          (let ((mid-elt (vector-select! less vec mid)))
            (cond
             ((odd? len) mid-elt)
             (else
              (let ((left-max
                     (let lp ((i 0) (best (vector-ref vec 0)))
                       (if (>= i mid)
                           best
                           (if (less best (vector-ref vec i))
                               (lp (+ i 1) (vector-ref vec i))
                               (lp (+ i 1) best))))))
                (mean left-max mid-elt)))))))))))
