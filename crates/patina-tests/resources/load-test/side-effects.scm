;; Test file for (scheme load) - side effects
;; This file modifies a variable that should already exist
(set! side-effect-counter (+ side-effect-counter 1))
