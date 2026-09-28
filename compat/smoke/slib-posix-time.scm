(import (scheme base) (prefix (slib posix-time) p:) (prefix (slib time-zone) z:)
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
(define utc (z:time-zone "UTC0"))
(define east (z:time-zone "IST-5:30"))
(define dst (z:time-zone "EST5EDT,M3.2.0/2,M11.1.0/2"))
(check-equal "GMT formatting uses POSIX fields and a final newline" "Thu Jan  1 00:00:00 1970\n"
  (p:asctime (p:gmtime 0)))
(check-equal "gtime composes GMT decoding and formatting" "Tue Feb 29 00:00:00 2000\n" (p:gtime 951782400))
(check-equal "ctime formats an explicit eastward zone" "Thu Jan  1 05:30:00 1970\n" (p:ctime 0 east))
(check-equal "GMT encoding of a fixed decoded date gives the expected seconds" 951782400
  (p:gmktime '#(0 0 0 29 1 100 2 59 0)))
(check-equal "GMT round trips epoch boundaries and leap dates" '(0 -1 -31622400 951782400 1719792000)
  (map (lambda (t) (p:gmktime (p:gmtime t))) '(0 -1 -31622400 951782400 1719792000)))
(check-equal "fixed timezone round trip preserves the timestamp" 0 (p:mktime (p:localtime 0 east) east))
(check-equal "standard-time round trip preserves the timestamp" 1704067200
  (p:mktime (p:localtime 1704067200 dst) dst))
(check-equal "daylight-time round trip preserves the timestamp" 1719792000
  (p:mktime (p:localtime 1719792000 dst) dst))
(check-equal "mktime treats positive tm_isdst as daylight time" 1719849600
  (p:mktime '#(0 0 12 1 6 124 1 182 1) dst))
(check-equal "fall-back standard and daylight clocks distinguish the repeated hour" '(1730615400 1730611800)
  (list (p:mktime '#(0 30 1 3 10 124 0 307 0) dst)
        (p:mktime '#(0 30 1 3 10 124 0 307 1) dst)))
(check-equal "unknown DST chooses a valid summer offset" 1719849600
  (p:mktime '#(0 0 12 1 6 124 1 182 -1) dst))
(check-equal "timezone conversion does not mutate the supplied vector" '#(0 0 12 1 6 124 1 182 1)
  (let ((tm (vector 0 0 12 1 6 124 1 182 1))) (p:mktime tm dst) tm))
(check-equal "separate GMT results do not overwrite earlier results" '#(0 0 0 1 0 70 4 0 0 0 "GMT")
  (let ((first (p:gmtime 0))) (p:gmtime 86400) first))
;; A file zone with old and new standard/daylight offsets. No host tzdb is used.
(define historical-zone
  '#(tz:file "fixture"
     #(#("OLD" -18000 #f #f #f) #("ODT" -14400 #t #f #f)
       #("NEW" -21600 #f #f #f) #("NDT" -18000 #t #f #f))
     #() #(0 1710054000 1730613600 1735689600 1741507200 1762066800)
     #(0 1 0 2 3 2)))
(check-equal "file zones distinguish both sides of a repeated local hour"
  '(1730611800 1730615400)
  (map (lambda (time) (p:mktime (p:localtime time historical-zone) historical-zone))
       '(1730611800 1730615400)))
(check-equal "file zones select historical offsets with the same DST flag"
  '(1704067200 1719849600 1735776000 1751371200)
  (map (lambda (time) (p:mktime (p:localtime time historical-zone) historical-zone))
       '(1704067200 1719849600 1735776000 1751371200)))
(check-equal "unknown DST is inferred for file zones" 1751371200
  (let ((tm (p:localtime 1751371200 historical-zone)))
    (vector-set! tm 8 -1)
    (p:mktime tm historical-zone)))
(check-equal "an omitted DST field is inferred for shorter decoded vectors" 1719849600
  (p:mktime '#(0 0 12 1 6 124 1 182) dst))
(smoke-finish)
