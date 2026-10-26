;; parameterize over current-output-port in a loop, each body writing one
;; character with no port argument (#618, #608).
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define n (arg 1 300000))
(define out (open-output-string))
(let loop ((i 0))
  (when (< i n)
    (parameterize ((current-output-port out)) (write-char #\x))
    (when (= 0 (modulo i 1000)) (get-output-string out) (set! out (open-output-string)))
    (loop (+ i 1))))
(display "ok")
(newline)
