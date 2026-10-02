(import (scheme base) (scheme write) (scheme time)
        (scheme list) (scheme hash-table) (scheme char) (scheme comparator) (scheme generator)
        (scheme set) (scheme sort) (scheme vector) (scheme text) (scheme ideque) (scheme ilist)
        (scheme rlist) (scheme mapping) (scheme stream) (scheme show) (scheme regex) (scheme charset)
        (scheme lseq) (scheme bytevector) (scheme flonum) (scheme fixnum) (scheme bitwise))
;; Library-load-heavy: the work is the imports. A short churn afterwards
;; gives the collector one safe point outside the (deferred) load.
(let loop ((i 0) (acc '()))
  (if (< i 200000)
      (loop (+ i 1) (if (= 0 (modulo i 1000)) '() (cons i acc)))
      (begin (display (length acc)) (newline))))
