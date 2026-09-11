;; `(scheme time)` — `current-second`, `current-jiffy`, `jiffies-per-second`,
;; R7RS §6.14.
;;
;; Migrated from `crates/patina-tests/tests/scheme_time.rs` (#193 Phase 2),
;; which is deleted. It built a `TreeWalkInterpreter` by hand for every one of
;; its 14 tests, so none had run on the VM. Every row here is a property the
;; report states, so the rows hold under load: nothing asserts how *long*
;; anything took beyond "less than a second", which is the lesson of
;; Larceny's `time.sld`, whose wall-clock assertion flakes on a busy machine.

(import (scheme base) (scheme time) (srfi 64))

(test-begin "time")

;; "Returns an inexact number representing the current time on the
;; International Atomic Time (TAI) scale." 1577836800 is 2020-01-01T00:00:00Z,
;; so the third element says the clock is a real one rather than an epoch
;; counter started at zero.
(test-equal "current-second is an inexact, recent, non-decreasing time"
  '(#t #t #t #t)
  (let* ((t1 (current-second))
         (t2 (current-second)))
    (list (inexact? t1) (> t1 0) (> t1 1577836800) (>= t2 t1))))

;; "Returns the number of jiffies as an exact integer that have elapsed since
;; an arbitrary, implementation-defined epoch."
(test-equal "current-jiffy is an exact, non-negative, non-decreasing integer"
  '(#t #t #t #t)
  (let* ((j1 (current-jiffy))
         (j2 (current-jiffy)))
    (list (exact? j1) (integer? j1) (>= j1 0) (>= j2 j1))))

;; "Returns an exact integer representing the number of jiffies per SI
;; second. This value is an implementation-specified constant."
(test-equal "jiffies-per-second is a positive exact integer constant"
  '(#t #t #t #t)
  (let ((j (jiffies-per-second)))
    (list (exact? j) (integer? j) (> j 0) (= j (jiffies-per-second)))))

;; Patina's constant: microsecond resolution. The report leaves the value to
;; the implementation, so this row is Patina's alone.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "Patina counts microseconds" 1000000 (jiffies-per-second))

;; The report's own example: timing a computation with jiffies. A 1000-element
;; list's length takes well under a second on any machine, however loaded.
(test-assert "the report's time-length example measures under a second"
  (let ((time-length (lambda ()
                       (let ((list (make-list 1000))
                             (start (current-jiffy)))
                         (length list)
                         (/ (- (current-jiffy) start)
                            (jiffies-per-second))))))
    (let ((duration (time-length)))
      (and (>= duration 0) (< duration 1)))))

(test-end)
