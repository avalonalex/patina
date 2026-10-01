(define-library (t g)
 (export gx gx-aux gy)
 (import (scheme base))
 (begin
(define-syntax gx-aux
  (syntax-rules (else =>)





    ((gx-aux reraise)
     reraise)

    ((gx-aux reraise (else result1 result2 ...))
     (begin result1 result2 ...))

    ((gx-aux reraise (test => result))
     (let ((temp test))
       (if temp (result temp) reraise)))

    ((gx-aux reraise (test => result) clause1 clause2 ...)
     (let ((temp test))
       (if temp
           (result temp)
           (gx-aux reraise clause1 clause2 ...))))

    ((gx-aux reraise (test))
     (or test reraise))

    ((gx-aux reraise (test) clause1 clause2 ...)
     (let ((temp test))
       (if temp
           temp
           (gx-aux reraise clause1 clause2 ...))))

    ((gx-aux reraise (test result1 result2 ...))
     (if test (begin result1 result2 ...) reraise))

    ((gx-aux reraise (test result1 result2 ...) clause1 clause2 ...)
     (if test
         (begin result1 result2 ...)
         (gx-aux reraise clause1 clause2 ...)))))

(define-syntax gx
  (syntax-rules ()
    ((gx (var clause ...) e1 e2 ...)




     ((call-with-current-continuation
       (lambda (guard-k)
         (with-exception-handler
          (lambda (condition)
            ((call-with-current-continuation
              (lambda (handler-k)
                (guard-k
                 (lambda ()
                   (let ((var condition))
                     (gx-aux
                      (handler-k
                       (lambda ()
                         (raise-continuable condition)))
                      clause ...))))))))
          (lambda ()






























            (call-with-values
             (lambda () e1 e2 ...)
             (lambda args
               (lambda ()
                 (apply values args))))))))))))
(define-syntax gy-aux
  (syntax-rules (else =>)





    ((gy-aux reraise)
     reraise)

    ((gy-aux reraise (else result1 result2 ...))
     (begin result1 result2 ...))

    ((gy-aux reraise (test => result))
     (let ((temp test))
       (if temp (result temp) reraise)))

    ((gy-aux reraise (test => result) clause1 clause2 ...)
     (let ((temp test))
       (if temp
           (result temp)
           (gy-aux reraise clause1 clause2 ...))))

    ((gy-aux reraise (test))
     (or test reraise))

    ((gy-aux reraise (test) clause1 clause2 ...)
     (let ((temp test))
       (if temp
           temp
           (gy-aux reraise clause1 clause2 ...))))

    ((gy-aux reraise (test result1 result2 ...))
     (if test (begin result1 result2 ...) reraise))

    ((gy-aux reraise (test result1 result2 ...) clause1 clause2 ...)
     (if test
         (begin result1 result2 ...)
         (gy-aux reraise clause1 clause2 ...)))))

(define-syntax gy
  (syntax-rules ()
    ((gy (var clause ...) e1 e2 ...)




     ((call-with-current-continuation
       (lambda (guard-k)
         (with-exception-handler
          (lambda (condition)
            ((call-with-current-continuation
              (lambda (handler-k)
                (guard-k
                 (lambda ()
                   (let ((var condition))
                     (gy-aux
                      (handler-k
                       (lambda ()
                         (raise-continuable condition)))
                      clause ...))))))))
          (lambda ()






























            (call-with-values
             (lambda () e1 e2 ...)
             (lambda args
               (lambda ()
                 (apply values args))))))))))))))
