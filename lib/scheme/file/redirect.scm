;; `with-input-from-file` / `with-output-to-file`. Scheme rather than
;; primitives so an escape out of the thunk unwinds normally; see
;; `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 for the VM crash that forced it.
;;
;; Two details are load-bearing, and the obvious first draft gets both wrong:
;;
;;   - Restore before closing. `parameterize` restores on the way out and the
;;     enclosing `dynamic-wind` closes after it, because unwinding runs
;;     innermost-first. Closing first would leave a closed port current for the
;;     rest of the unwind, and anything written in that window — a `guard`
;;     handler, say — disappears into it.
;;
;;   - Close at all. chibi need not: it flushes open ports at exit and Patina
;;     does not, so a port left open on a non-local exit loses whatever is
;;     buffered. R7RS §6.13.1 permits leaving it open, not dropping the output.
;;     The cost is that re-entering a continuation captured inside the thunk
;;     finds the port closed.

(define (with-output-to-file filename thunk)
  (let ((port (open-output-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (parameterize ((current-output-port port)) (thunk)))
      (lambda () (close-output-port port)))))

(define (with-input-from-file filename thunk)
  (let ((port (open-input-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (parameterize ((current-input-port port)) (thunk)))
      (lambda () (close-input-port port)))))
