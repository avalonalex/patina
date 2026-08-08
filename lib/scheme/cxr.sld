;; (scheme cxr) - R7RS Extended Car/Cdr Library
;;
;; R7RS: exports the twenty-four compositions of three to four car/cdr
;; operations (the two-deep compositions live in (scheme base)). All are
;; registry primitives re-exported from (patina internal lists).

(define-library (scheme cxr)
  (import (patina internal lists))

  (export
    ;; Three-deep compositions (also exported by this implementation's
    ;; (scheme base) as an extension)
    caaar caadr cadar caddr
    cdaar cdadr cddar cdddr
    ;; Four-deep compositions
    caaaar caaadr caadar caaddr
    cadaar cadadr caddar cadddr
    cdaaar cdaadr cdadar cdaddr
    cddaar cddadr cdddar cddddr))
