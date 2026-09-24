;; `call-with-input-file` / `call-with-output-file`: `call-with-port` over a
;; freshly opened port. Scheme rather than primitives for the reason
;; `call-with-port` is (`lib/scheme/base/higher_order.scm`): a continuation
;; captured inside `proc` and re-entered after the call returned must find the
;; rest of the call to return into — the port's closing and the value — which
;; a Rust primitive's frame cannot be part of (#471).

(define (call-with-input-file filename proc)
  (call-with-port (open-input-file filename) proc))

(define (call-with-output-file filename proc)
  (call-with-port (open-output-file filename) proc))
