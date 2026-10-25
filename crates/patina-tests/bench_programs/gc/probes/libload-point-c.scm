;; libload's libraries imported by a top-level begin: a load met inside a
;; form, which defers its collection to the form's end (point C; #787, #658).
;; It is the load decision 9 on #782 recommends GC stage 4c's memory gate
;; measure.
(import (scheme base) (patina debug))
(begin
  (import (scheme write) (scheme time)
          (scheme list) (scheme hash-table) (scheme char) (scheme comparator) (scheme generator)
          (scheme set) (scheme sort) (scheme vector) (scheme text) (scheme ideque) (scheme ilist)
          (scheme rlist) (scheme mapping) (scheme stream) (scheme show) (scheme regex) (scheme charset)
          (scheme lseq) (scheme bytevector) (scheme flonum) (scheme fixnum) (scheme bitwise)))
;; It measures a deferred load. If the load stops deferring, it would
;; measure something else, so it fails. The premise is the default trigger's,
;; under which nothing collects before the load: under PATINA_GC_STRESS at
;; 16 K allocations the bootstrap collects first, so a census of it runs at
;; `--config default` (#799).
(let* ((s (gc-stats))
       (collections (cdr (assq 'collections s)))
       (window (cdr (assq 'deferral-max-bytes s))))
  (unless (and (= collections 1) (>= window (* 250 1024 1024)))
    (error "the point-C load no longer defers" collections window))
  (display "ok") (newline))
