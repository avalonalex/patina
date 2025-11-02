;; Basic arithmetic tests - R7RS compliant
(import (scheme base)
        (scheme write))

;; Test addition
(display (+ 1 2 3)) (newline)              ; 6
(display (+ )) (newline)                   ; 0
(display (+ 5)) (newline)                  ; 5
(display (+ -1 -2 -3)) (newline)           ; -6

;; Test subtraction
(display (- 10 3)) (newline)               ; 7
(display (- 5)) (newline)                  ; -5
(display (- 20 5 3)) (newline)             ; 12

;; Test multiplication
(display (* 2 3 4)) (newline)              ; 24
(display (* )) (newline)                   ; 1
(display (* 7)) (newline)                  ; 7
(display (* -2 3)) (newline)               ; -6

;; Test division
(display (/ 20 4)) (newline)               ; 5
(display (/ 100 10 2)) (newline)           ; 5
