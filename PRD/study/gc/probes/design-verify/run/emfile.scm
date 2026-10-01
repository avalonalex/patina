(import (scheme base) (scheme file) (scheme write))
(define count 0)
(write
 (guard (e (#t (list 'failed-at count (file-error? e))))
   (let loop ()
     (if (< count 100000)
         (begin (open-input-file "data.txt") (set! count (+ count 1)) (loop))
         (list 'ok count)))))
(newline)
