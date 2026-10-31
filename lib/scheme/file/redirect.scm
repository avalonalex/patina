;; `with-input-from-file` / `with-output-to-file`. Scheme rather than
;; primitives so an escape out of the thunk unwinds normally; see
;; `PRD/ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 for the VM crash that forced it.
;;
;; Two details are load-bearing, and the obvious first draft gets both wrong:
;;
;;   - Restore before closing. `parameterize` restores on the way out and the
;;     enclosing `dynamic-wind` closes after it, because unwinding runs
;;     innermost-first. Closing first would leave a closed port current for the
;;     rest of the unwind, and anything written in that window — a `guard`
;;     handler, say — disappears into it.
;;
;;   - Close at all. R7RS §6.13.1 permits leaving the port open on an escape,
;;     but a port left open is written out only when a collection finds it
;;     dead, its interpreter is dropped or the program ends (#785, #604,
;;     #343), so a program that reads the file back after escaping would find
;;     what was buffered missing.
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
