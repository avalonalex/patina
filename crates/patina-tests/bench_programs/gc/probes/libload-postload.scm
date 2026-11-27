;; libload's imports and nothing after them (#931): the gc mode's epilogue
;; then collects right after the load, so this workload's forced pause is
;; the post-load major the stage-4 exit baseline reports (decision 25 on
;; #782), beside libload's, which follows its churn.
(import (scheme base) (scheme write) (scheme time)
        (scheme list) (scheme hash-table) (scheme char) (scheme comparator) (scheme generator)
        (scheme set) (scheme sort) (scheme vector) (scheme text) (scheme ideque) (scheme ilist)
        (scheme rlist) (scheme mapping) (scheme stream) (scheme show) (scheme regex) (scheme charset)
        (scheme lseq) (scheme bytevector) (scheme flonum) (scheme fixnum) (scheme bitwise))
(display "loaded")
(newline)
