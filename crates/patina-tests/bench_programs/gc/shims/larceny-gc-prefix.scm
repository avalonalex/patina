;; Prepended to Larceny's test/Benchmarking/GC programs (gcold.sch,
;; queue3.sch), which expect an R5RS harness: the libraries they use and a
;; run-benchmark that runs a thunk and reports its time, called either as
;; (run-benchmark name thunk) or (run-benchmark name count thunk ok?).
;; Patina-authored; the programs themselves stay in the Larceny checkout.
(import (scheme base) (scheme cxr) (scheme write) (scheme time) (scheme fixnum))
(define (run-benchmark name . rest)
  (let* ((once (= (length rest) 1))
         (count (if once 1 (car rest)))
         (thunk (if once (car rest) (cadr rest)))
         (ok? (if once (lambda (x) #t) (caddr rest)))
         (j0 (current-jiffy)))
    (let loop ((i 0) (result #f))
      (if (< i count)
          (loop (+ i 1) (thunk))
          (begin
            (if (ok? result)
                (begin
                  (display "Elapsed time: ")
                  (write (inexact (/ (- (current-jiffy) j0) (jiffies-per-second))))
                  (display " seconds for ")
                  (display name))
                (begin
                  (display "ERROR: returned incorrect result: ")
                  (write result)))
            (newline)
            result)))))
