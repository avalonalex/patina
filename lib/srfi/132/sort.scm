;;; SRFI 132 internals — pure Scheme merge sort
;;;
;;; Provides the core sort primitives used by the SRFI 132 wrappers.

;; ── List merge sort ──────────────────────────────────────────────

(define (%list-merge less ls1 ls2)
  (cond
   ((null? ls1) ls2)
   ((null? ls2) ls1)
   ((less (car ls2) (car ls1))
    (cons (car ls2) (%list-merge less (cdr ls2) ls1)))
   (else
    (cons (car ls1) (%list-merge less (cdr ls1) ls2)))))

(define (%list-split ls)
  ;; Split list into two halves (by length)
  (let ((len (length ls)))
    (let lp ((i 0) (rest ls) (acc '()))
      (if (>= i (quotient len 2))
          (values (reverse acc) rest)
          (lp (+ i 1) (cdr rest) (cons (car rest) acc))))))

(define (%list-sort less ls)
  (if (or (null? ls) (null? (cdr ls)))
      ls
      (call-with-values
        (lambda () (%list-split ls))
        (lambda (left right)
          (%list-merge less (%list-sort less left) (%list-sort less right))))))

;; Destructive list merge
(define (%list-merge! less ls1 ls2)
  (cond
   ((null? ls1) ls2)
   ((null? ls2) ls1)
   (else
    (let ((a (car ls1)) (b (car ls2)))
      (if (less b a)
          (begin
            (if (null? (cdr ls2))
                (set-cdr! ls2 ls1)
                (let lp ((prev ls2) (rest (cdr ls2)))
                  (cond
                   ((null? rest) (set-cdr! prev ls1))
                   ((less (car rest) (car ls1))
                    (lp rest (cdr rest)))
                   (else
                    (set-cdr! prev ls1)
                    (%list-merge! less (cdr ls1) rest)
                    ;; ls1 was spliced into ls2's chain
                    ))))
            ls2)
          (begin
            (if (null? (cdr ls1))
                (set-cdr! ls1 ls2)
                (let lp ((prev ls1) (rest (cdr ls1)))
                  (cond
                   ((null? rest) (set-cdr! prev ls2))
                   ((not (less (car ls2) (car rest)))
                    (lp rest (cdr rest)))
                   (else
                    (set-cdr! prev ls2)
                    (%list-merge! less rest (cdr ls2))))))
            ls1))))))

;; ── Vector sort (in-place, merge sort) ───────────────────────────

(define (%vector-sort! less vec start end)
  (when (> (- end start) 1)
    (let ((mid (quotient (+ start end) 2)))
      (%vector-sort! less vec start mid)
      (%vector-sort! less vec mid end)
      ;; Merge [start..mid) and [mid..end) in-place via temp vector
      (let* ((len (- end start))
             (tmp (make-vector len)))
        (let lp ((i start) (j mid) (k 0))
          (cond
           ((and (< i mid) (< j end))
            (if (less (vector-ref vec j) (vector-ref vec i))
                (begin (vector-set! tmp k (vector-ref vec j))
                       (lp i (+ j 1) (+ k 1)))
                (begin (vector-set! tmp k (vector-ref vec i))
                       (lp (+ i 1) j (+ k 1)))))
           ((< i mid)
            (vector-set! tmp k (vector-ref vec i))
            (lp (+ i 1) j (+ k 1)))
           ((< j end)
            (vector-set! tmp k (vector-ref vec j))
            (lp i (+ j 1) (+ k 1)))
           (else
            (do ((n 0 (+ n 1)))
                ((= n len))
              (vector-set! vec (+ start n) (vector-ref tmp n))))))))))
