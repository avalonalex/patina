;; Numeric comparison tests - R7RS compliant
(import (scheme base)
        (scheme write))

;; Test =
(display (= 5 5)) (newline)                ; #t
(display (= 5 6)) (newline)                ; #f
(display (= 3 3 3)) (newline)              ; #t
(display (= 3 3 4)) (newline)              ; #f

;; Test <
(display (< 1 2)) (newline)                ; #t
(display (< 2 1)) (newline)                ; #f
(display (< 1 2 3)) (newline)              ; #t
(display (< 1 3 2)) (newline)              ; #f
(display (< 1 1)) (newline)                ; #f
