;; A file cut short inside its last form, for the (scheme load) tests:
;; the first definition is complete, the second never closes.
(define loaded-before-cut 42)
(define loaded-after-cut
  (+ loaded-before-cut
