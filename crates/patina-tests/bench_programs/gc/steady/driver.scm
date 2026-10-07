;; The steady-state lane's driver (PRD/GC_PRD.md §17.4, #652), included by
;; every row. A row defines `(cycle i)`, one iteration of what it repeats,
;; and calls `(steady-run cycle)`, which runs it 4N times, N the first
;; command-line argument, and prints one reading at each of N, 2N, 3N and
;; 4N for scripts/run_steady_state.py:
;;
;;   STEADY at=<cycle> cpu-us=<..> pause-us=<..> [live-bytes=<..> ...]
;;
;; At N and 4N it first reads the CPU and pause time, then calls (gc) twice
;; and reads what the collection found: live-bytes, the footprint's two
;; parts and the per-owner counts. (gc) collects at its call, so these
;; readings are deterministic. The segments [N, 2N) and [3N, 4N) are timed
;; by cpu-us less pause-us, and the forced collections fall outside them.

(define (steady-arg k)
  (list-ref (command-line) k))

(define (steady-number k)
  (string->number (steady-arg k)))

(define (steady-print at keys stats)
  (display "STEADY at=")
  (display at)
  (for-each (lambda (key)
              (let ((value (cdr (assq key stats))))
                (display " ")
                (display key)
                (display "=")
                (display (if value value "none"))))
            keys)
  (newline))

(define (steady-reading at forced)
  (let ((before (gc-stats)))
    (if forced
        (begin
          (gc)
          (gc)
          (let ((after (gc-stats)))
            (steady-print at '(cpu-us pause-total-us collections) before)
            (display "STEADY-AFTER at=")
            (display at)
            (for-each (lambda (key)
                        (display " ")
                        (display key)
                        (display "=")
                        (display (cdr (assq key after))))
                      '(live-bytes committed-bytes external-bytes symbols resident-bytes
                        cpu-us pause-total-us collections))
            (newline)))
        (steady-print at '(cpu-us pause-total-us collections) before))))

(define (steady-run cycle)
  (let ((n (steady-number 1)))
    (let loop ((i 0))
      (cond ((= i n) (steady-reading i #t))
            ((or (= i (* 2 n)) (= i (* 3 n))) (steady-reading i #f)))
      (if (< i (* 4 n))
          (begin (cycle i) (loop (+ i 1)))
          (steady-reading i #t)))))
