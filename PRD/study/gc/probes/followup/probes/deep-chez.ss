(define (count n) (if (= n 0) 0 (+ 1 (count (- n 1)))))
(define depth (string->number (cadr (command-line))))
(write (guard (e [#t (list 'caught (condition-message e))]) (count depth))) (newline)
