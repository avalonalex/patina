
;; Constant live set (a 100,000-slot vector of 10-element lists), constant
;; allocation rate: each iteration builds a 10-element list and stores it
;; into a slot, so the previous occupant becomes garbage.
(define n (string->number (cadr (command-line))))
(define live (make-vector 100000 '()))
(define (mk k i) (let loop ((j 0) (acc '())) (if (= j k) acc (loop (+ j 1) (cons i acc)))))
(let loop ((i 0))
  (when (< i n)
    (vector-set! live (modulo i 100000) (mk 10 i))
    (loop (+ i 1))))
(write (length (vector-ref live 0))) (newline)
