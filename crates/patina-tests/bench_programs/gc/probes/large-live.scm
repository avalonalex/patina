;; A large live heap (#649, PRD/GC_PRD.md §9.10): a balanced tree of records
;; and vectors built once, of argument 1 objects, then short-lived
;; allocation that forces majors over it. On demand: 20,000,000 objects is
;; about 1 GiB in the headered layout, several in today's slots.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define-record-type node (make-node left right payload) node? (left node-left) (right node-right) (payload node-payload))
(define (build n)
  (if (<= n 0) #f
      (let ((half (quotient (- n 2) 2)))
        (make-node (build half) (build (- n 2 half)) (vector n n)))))
(define tree (build (arg 1 20000000)))
(define (churn n acc) (if (= n 0) (length acc) (churn (- n 1) (if (= 0 (modulo n 1000)) '() (cons n acc)))))
(display (churn (arg 2 20000000) '()))
(newline)
(display (node? tree))
(newline)
