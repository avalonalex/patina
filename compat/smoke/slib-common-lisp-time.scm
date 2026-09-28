(import (scheme base) (prefix (slib time-zone) z:)
        (prefix (slib common-lisp-time) c:) (prefix (slib time-core) t:)
        (rename (patina compat smoke) (check-equal smoke-check-equal)))
;; Calendar seconds may be exact or inexact depending on the clock adapter.
;; Compare their numerical values without imposing an exactness contract.
(define (numeric-data x)
  (cond ((number? x) (exact x))
        ((pair? x) (cons (numeric-data (car x)) (numeric-data (cdr x))))
        (else x)))
(define (check-equal label expected actual)
  (smoke-check-equal label (numeric-data expected) (numeric-data actual)))
(define-syntax observed
  (syntax-rules () ((_ expr) (guard (ex (else 'unexpected-error)) expr))))
(define (decoded time zone) (call-with-values (lambda () (c:decode-universal-time time zone)) list))
(check-equal "universal time starts at 1900 rather than 1970" '(0 0 0 1 1 1900 0 #f 0) (decoded 0 0))
(check-equal "the Unix epoch differs by 2208988800 seconds" '(0 0 0 1 1 1970 3 #f 0) (decoded 2208988800 0))
(check-equal "decode returns nine values with one-based months and Monday weekday zero" '(0 0 0 29 2 2000 1 #f 0)
  (decoded 3160771200 0))
(check-equal "explicit zones are hours west, including fractional east offsets" '(0 30 5 1 1 1970 3 #f -11/2)
  (decoded 2208988800 -11/2))
(check-equal "encoding the epoch produces zero universal seconds" 0 (c:encode-universal-time 0 0 0 1 1 1900 0))
(check-equal "encoding a leap date uses the Gregorian calendar" 3160771200 (c:encode-universal-time 0 0 0 29 2 2000 0))
(check-equal "integral explicit zones offset the timestamp" 2209006800 (c:encode-universal-time 0 0 0 1 1 1970 5))
(check-equal "fractional explicit zones preserve half-hour offsets" 2208988800
  (observed (c:encode-universal-time 0 30 5 1 1 1970 -11/2)))
(check-equal "decode and encode round trip explicit timezone offsets" '(2208988800 2208988800 2208988800)
  (map (lambda (zone) (observed
    (let ((parts (decoded 2208988800 zone)))
      (c:encode-universal-time (list-ref parts 0) (list-ref parts 1) (list-ref parts 2)
        (list-ref parts 3) (list-ref parts 4) (list-ref parts 5) zone)))) '(0 5 -11/2)))
;; Replace the shared clock and zone resolver and restore them afterward.
;; No host clock, zoneinfo database or environment mutation is used.
(define (with-clock-and-zone clock zone thunk)
  (let ((saved-clock t:current-time) (saved-zone z:time-zone))
    (dynamic-wind
      (lambda ()
        (set! t:current-time (lambda () clock))
        (set! z:time-zone (lambda (ignored) (saved-zone zone))))
      thunk
      (lambda () (set! t:current-time saved-clock) (set! z:time-zone saved-zone)))))
(check-equal "current universal time uses the sampled clock and universal epoch" 2208988800
  (with-clock-and-zone 0 "UTC0" c:get-universal-time))
(check-equal "get-decoded-time uses the chosen fixed clock and zone" '(0 30 5 1 1 1970 3 #f -11/2)
  (with-clock-and-zone 0 "IST-5:30" (lambda () (call-with-values c:get-decoded-time list))))
(check-equal "default-zone DST lookup uses calendar time rather than universal seconds" '(0 0 12 1 7 2024 0 #t 4)
  (with-clock-and-zone 1719849600 "EST5EDT,M3.2.0/2,M11.1.0/2"
    (lambda () (call-with-values c:get-decoded-time list))))
(check-equal "default-zone encoding applies daylight saving" 3928838400
  (with-clock-and-zone 1719849600 "EST5EDT,M3.2.0/2,M11.1.0/2"
    (lambda () (c:encode-universal-time 0 0 12 1 7 2024))))
(check-equal "default-zone decoding selects DST at the calendar-time boundary" '(0 0 3 10 3 2024 6 #t 4)
  (with-clock-and-zone 1710054000 "EST5EDT,M3.2.0/2,M11.1.0/2"
    (lambda () (call-with-values c:get-decoded-time list))))
(smoke-finish)
