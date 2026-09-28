;; Test-only files live in the runner's per-package scratch directory.
(define-library (patina compat time-fixtures)
  (export call-with-binary-fixture call-with-tzif)
  (import (scheme base) (scheme file))
  (begin
    (define serial 0)
    (define (call-with-binary-fixture bytes proc)
      (set! serial (+ serial 1))
      (let ((path (string-append "./patina-time-fixture-" (number->string serial) ".dat")))
        (if (file-exists? path) (error "fixture already exists" path))
        (dynamic-wind
          (lambda ()
            (let ((out (open-binary-output-file path)))
              (call-with-port out (lambda (out) (write-bytevector bytes out)))))
          (lambda () (proc path))
          (lambda () (if (file-exists? path) (delete-file path))))))
    (define (be32 n)
      (map (lambda (divisor) (modulo (floor-quotient n divisor) 256))
           '(16777216 65536 256 1)))
    (define (call-with-tzif attributes? proc)
      ;; TZif v1: three transitions, two types, one leap record. Signed
      ;; offsets/timestamps exercise bytes above 127 through binary ports.
      (let ((bytes
              (append '(84 90 105 102) (make-list 16 0)
                      (apply append (map be32 (list (if attributes? 2 0)
                        (if attributes? 2 0) 1 3 2 8)))
                      (apply append (map be32 '(-1 100 200))) '(1 0 1)
                      (be32 -18000) '(0 0) (be32 -14400) '(1 4)
                      '(83 84 68 0 68 83 84 0)
                      (be32 1000) (be32 1)
                      (if attributes? '(1 0 0 1) '()))))
        (call-with-binary-fixture (apply bytevector bytes) proc)))))
