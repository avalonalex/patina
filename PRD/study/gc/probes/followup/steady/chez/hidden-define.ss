(define N (string->number (cadr (command-line))))
(define env (interaction-environment))
(eval '(define-syntax def-counter
         (syntax-rules ()
           ((_ name) (begin (define state 0)
                            (define (name) (set! state (+ state 1)) state)))))
      env)
(let loop ((i 0) (acc 0))
  (if (< i N)
      (begin
        (eval '(def-counter counter) env)
        (loop (+ i 1) (+ acc (eval '(counter) env))))
      (begin (display acc) (newline))))
