(import (scheme base) (scheme write))
;; a macro-introduced top-level definition that only a macro template refers to
(define-syntax def-getter
  (syntax-rules ()
    ((_ getter setter)
     (begin (define secret 42)
            (define-syntax getter (syntax-rules () ((_) secret)))
            (define-syntax setter (syntax-rules () ((_ v) (set! secret v))))))))
(def-getter get-secret set-secret!)
(define junk (let loop ((i 0) (acc '())) (if (< i 200000) (loop (+ i 1) (cons (make-vector 3 i) acc)) (length acc))))
(set-secret! 7)
(define junk2 (let loop ((i 0) (acc '())) (if (< i 200000) (loop (+ i 1) (cons (make-vector 3 i) acc)) (length acc))))
(display (get-secret)) (newline)
