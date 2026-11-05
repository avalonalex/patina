;; The steady-state lane's driver (PRD/GC_PRD.md §17.4, #652), included by
;; every row. A row defines `(cycle i)`, one iteration of what it repeats,
;; and calls `(steady-run cycle)`, which runs it 4N times, N the first
;; command-line argument, and prints readings at N, 2N, 3N and 4N for
;; scripts/run_steady_state.py, one key=value per key:
;;
;;   STEADY at=<cycle> cpu-us=<..> pause-total-us=<..> collections=<..>
;;   STEADY-AFTER at=<cycle> live-bytes=<..> ... collections=<..>
;;
;; The first, `steady-before-keys`, at each of the four. At N and 4N it is
;; read before the driver calls (gc) twice, and the second,
;; `steady-after-keys`, after: what the collection found, live-bytes, the
;; footprint's two parts and the per-owner counts, then the times. (gc)
;; collects at its call, so these readings are deterministic. The segments
;; [N, 2N) and [3N, 4N) are timed by cpu-us less pause-total-us, and the
;; forced collections fall outside them.
;;
;; The per-owner counts are `steady-per-owner-keys`, each an SS5 clause of
;; its own in the runner (`SS5:<key>`), whose PER_OWNER_KEYS must be this
;; list: a test keeps the two equal. A key comes with the PR that adds it to
;; (gc-stats). Every key is read through `steady-value`, which prints `none`
;; for a key (gc-stats) lacks or one that answers #f, and the runner fails a
;; row that reads `none` in a key one of its clauses reads.

(define (steady-arg k)
  (list-ref (command-line) k))

(define (steady-number k)
  (string->number (steady-arg k)))

(define steady-per-owner-keys '(symbols ports))

(define steady-before-keys '(cpu-us pause-total-us collections))

(define steady-after-keys
  (append '(live-bytes committed-bytes external-bytes)
          steady-per-owner-keys
          '(resident-bytes cpu-us pause-total-us collections)))

;; `key`'s value in `stats`, or `none` when (gc-stats) has no such key or it
;; answers #f, as `resident-bytes` and `cpu-us` do where they cannot be read.
(define (steady-value key stats)
  (let ((entry (assq key stats)))
    (if (and entry (cdr entry)) (cdr entry) 'none)))

(define (steady-print kind at keys stats)
  (display kind)
  (display " at=")
  (display at)
  (for-each (lambda (key)
              (display " ")
              (display key)
              (display "=")
              (display (steady-value key stats)))
            keys)
  (newline))

(define (steady-reading at forced)
  (let ((before (gc-stats)))
    (if forced
        (begin
          (gc)
          (gc)
          (let ((after (gc-stats)))
            (steady-print "STEADY" at steady-before-keys before)
            (steady-print "STEADY-AFTER" at steady-after-keys after)))
        (steady-print "STEADY" at steady-before-keys before))))

(define (steady-run cycle)
  (let ((n (steady-number 1)))
    (let loop ((i 0))
      (cond ((= i n) (steady-reading i #t))
            ((or (= i (* 2 n)) (= i (* 3 n))) (steady-reading i #f)))
      (if (< i (* 4 n))
          (begin (cycle i) (loop (+ i 1)))
          (steady-reading i #t)))))
