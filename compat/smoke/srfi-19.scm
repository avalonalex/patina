(import (scheme base) (scheme time) (prefix (srfi 19) t:) (patina compat smoke))
;; Fixed dates and explicit offsets keep calendar checks independent of the host.
(define (time-fields time)
  (list (t:time-type time) (t:time-nanosecond time) (t:time-second time)))
(define (date-fields date)
  (list (t:date-nanosecond date) (t:date-second date) (t:date-minute date)
        (t:date-hour date) (t:date-day date) (t:date-month date)
        (t:date-year date) (t:date-zone-offset date)))
(define (nanoseconds time)
  (+ (* 1000000000 (t:time-second time)) (t:time-nanosecond time)))
;; An unexpected exception is a failed assertion, allowing later cases to run.
(define (observe thunk) (guard (ex (else 'unexpected-error)) (thunk)))
(check-equal "time construction and predicates" '(#t #f (time-utc 123 42))
  (let ((time (t:make-time t:time-utc 123 42)))
    (list (t:time? time) (t:time? 42) (time-fields time))))
(check-equal "copies are independent and all fields can be changed"
  '((time-utc 123 42) (time-tai 456 99))
  (let* ((original (t:make-time t:time-utc 123 42)) (copy (t:copy-time original)))
    (t:set-time-type! copy t:time-tai)
    (t:set-time-nanosecond! copy 456)
    (t:set-time-second! copy 99)
    (list (time-fields original) (time-fields copy))))
(check-equal "comparison uses seconds before nanoseconds" '(#t #t #t #t #t #f)
  (let ((a (t:make-time t:time-utc 900 1)) (b (t:make-time t:time-utc 100 2)))
    (list (t:time<? a b) (t:time>? b a) (t:time<=? a a) (t:time>=? b b)
          (t:time=? a (t:copy-time a)) (t:time=? a b))))
(check-error "comparisons reject different time types"
  (t:time<? (t:make-time t:time-utc 0 0) (t:make-time t:time-tai 0 0)))
(check-equal "adding a duration carries nanoseconds without mutating inputs"
  '((time-utc 100000000 13) (time-utc 900000000 10) (time-duration 200000000 2))
  (let* ((a (t:make-time t:time-utc 900000000 10))
         (d (t:make-time t:time-duration 200000000 2)) (result (t:add-duration a d)))
    (map time-fields (list result a d))))
(check-equal "subtracting a duration borrows a second" '(time-utc 800000000 7)
  (time-fields (t:subtract-duration (t:make-time t:time-utc 100000000 10)
                                   (t:make-time t:time-duration 300000000 2))))
(check-equal "destructive duration arithmetic returns the updated time"
  '((time-utc 200000000 12) (time-utc 0 10))
  (let* ((a (t:make-time t:time-utc 0 10)) (d (t:make-time t:time-duration 200000000 2))
         (sum (time-fields (t:add-duration! a d))))
    (list sum (time-fields (t:subtract-duration! a d)))))
(check-equal "positive differences preserve the original time objects"
  '(1250000000 time-duration 10250000000 9000000000)
  (let* ((a (t:make-time t:time-utc 250000000 10)) (b (t:make-time t:time-utc 0 9))
         (d (t:time-difference a b)))
    (list (nanoseconds d) (t:time-type d) (nanoseconds a) (nanoseconds b))))
(check-equal "negative differences retain their fractional sign" -750000000
  (nanoseconds (t:time-difference (t:make-time t:time-utc 250000000 10)
                                 (t:make-time t:time-utc 0 11))))
(check-equal "a destructive difference can reuse a point-in-time object"
  '(time-duration 250000000 1)
  (observe (lambda () (time-fields
    (t:time-difference! (t:make-time t:time-utc 250000000 10)
                        (t:make-time t:time-utc 0 9))))))
(check-equal "a destructive difference handles identical operands"
  '(time-duration 0 0)
  (let ((a (t:make-time t:time-utc 500 10)))
    (observe (lambda () (time-fields (t:time-difference! a a))))))
(check-error "duration arithmetic rejects a point as the duration"
  (t:add-duration (t:make-time t:time-utc 0 1) (t:make-time t:time-utc 0 2)))
(define epoch (t:make-date 0 0 0 0 1 1 1970 0))
(check-equal "date accessors preserve all constructor fields"
  '(#t #f (123 56 34 12 29 2 2000 19800))
  (let ((date (t:make-date 123 56 34 12 29 2 2000 19800)))
    (list (t:date? date) (t:date? '()) (date-fields date))))
(check-equal "the Unix epoch converts in both directions"
  '((time-utc 0 0) (0 0 0 0 1 1 1970 0))
  (list (time-fields (t:date->time-utc epoch))
        (date-fields (t:time-utc->date (t:make-time t:time-utc 0 0) 0))))
(check-equal "explicit time zones represent the same instant"
  '(0 (0 0 30 5 1 1 1970 19800))
  (list (t:time-second (t:date->time-utc (t:make-date 0 0 30 5 1 1 1970 19800)))
        (date-fields (t:time-utc->date (t:make-time t:time-utc 0 0) 19800))))
(check-equal "dates before the epoch round trip with nanoseconds"
  '((time-utc 123 -1) (123 59 59 23 31 12 1969 0))
  (let ((time (t:date->time-utc (t:make-date 123 59 59 23 31 12 1969 0))))
    (list (time-fields time) (date-fields (t:time-utc->date time 0)))))
(check-equal "Gregorian century leap rules and weekday numbering" '(60 61 60 4)
  (list (t:date-year-day (t:make-date 0 0 0 0 29 2 2000 0))
        (t:date-year-day (t:make-date 0 0 0 0 1 3 2000 0))
        (t:date-year-day (t:make-date 0 0 0 0 1 3 1900 0))
        (t:date-week-day epoch)))
(check-equal "week numbers ignore the initial partial week" '(0 1 1)
  (list (t:date-week-number epoch 0)
        (t:date-week-number (t:make-date 0 0 0 0 4 1 1970 0) 0)
        (t:date-week-number (t:make-date 0 0 0 0 1 1 2018 0) 1)))
(check-equal "Julian and modified Julian epoch values are exact"
  '(4881175/2 40587 (0 0 0 0 1 1 1970 0) (time-utc 0 0))
  (observe (lambda ()
    (list (t:date->julian-day epoch) (t:date->modified-julian-day epoch)
          (date-fields (t:julian-day->date 4881175/2 0))
          (time-fields (t:modified-julian-day->time-utc 40587))))))
(check-equal "time-to-Julian conversions retain a fractional day" '(2440588 81175/2)
  (let ((time (t:make-time t:time-utc 0 43200)))
    (list (t:time-utc->julian-day time) (t:time-utc->modified-julian-day time))))
(check-equal "UTC and TAI agree on the recorded 2017 offset"
  '((time-tai 123 1483228837) (time-utc 123 1483228800) (time-utc 123 1483228800))
  (let* ((utc (t:make-time t:time-utc 123 1483228800)) (tai (t:time-utc->time-tai utc)))
    (map time-fields (list tai (t:time-tai->time-utc tai) utc))))
(check-equal "the 2016 leap second is preserved through TAI"
  '(0 60 59 23 31 12 2016 0)
  (date-fields (t:time-tai->date
                (t:date->time-tai (t:make-date 0 60 59 23 31 12 2016 0)) 0)))
(check-equal "date and monotonic conversions are inverse"
  '(0 0 0 0 1 1 2017 0)
  (date-fields (t:time-monotonic->date
                (t:date->time-monotonic (t:make-date 0 0 0 0 1 1 2017 0)) 0)))
(check-equal "destructive clock conversions return their converted objects"
  '((time-tai 0 1483228837) (time-monotonic 0 1483228837) (time-utc 0 1483228800))
  (observe (lambda ()
    (let* ((m (t:date->time-monotonic (t:make-date 0 0 0 0 1 1 2017 0)))
           (tai (time-fields (t:time-monotonic->time-tai! m)))
           (monotonic (time-fields (t:time-tai->time-monotonic! m)))
           (utc (time-fields (t:time-monotonic->time-utc! m))))
      (list tai monotonic utc)))))
(check-equal "formatting uses explicit fields and a numeric timezone"
  "2000-02-29 12:34:56 +0530"
  (t:date->string (t:make-date 0 56 34 12 29 2 2000 19800) "~Y-~m-~d ~H:~M:~S ~z"))
(check-equal "parsing a complete template preserves its explicit timezone"
  '(0 56 34 12 29 2 2000 19800)
  (date-fields (t:string->date "2000-02-29 12:34:56 +0530" "~Y-~m-~d ~H:~M:~S ~z")))
(check-error "unknown formatting directives are rejected" (t:date->string epoch "~?"))
(check-error "invalid clock names are rejected" (t:current-time 'not-a-clock))
(check-equal "available clocks preserve their requested types"
  '(time-utc time-tai time-monotonic time-thread time-process)
  (map (lambda (kind) (t:time-type (t:current-time kind)))
       (list t:time-utc t:time-tai t:time-monotonic t:time-thread t:time-process)))
(check-equal "reported clock resolutions are positive integers" #t
  (let ((r (t:time-resolution t:time-utc))) (and (integer? r) (exact? r) (> r 0))))
;; Imported bindings are shared. Restore the clock even if a check raises.
(check-equal "the UTC clock samples seconds and its fractional part together"
  '((time-utc 125000000 1700000000) 1)
  (let ((saved current-second) (calls 0))
    (dynamic-wind
      (lambda () (set! current-second (lambda () (set! calls (+ calls 1)) 1700000000.125)))
      (lambda () (let ((time (t:current-time t:time-utc))) (list (time-fields time) calls)))
      (lambda () (set! current-second saved)))))
(smoke-finish)
