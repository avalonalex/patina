;; #417: a large read limit must not become the allocation size.
;; script_running.rs supplies tenk.bin (10000 bytes, each index modulo 250)
;; and empty.bin, and checks this program in a subprocess on both backends.
;; A regression may abort the process, so this is not an in-process suite.
(import (scheme base) (scheme write) (scheme file))

(define limit (expt 2 46))
(define (check name)
  (let* ((p (open-binary-input-file name))
         (zero (read-bytevector 0 p))
         (bytes (read-bytevector limit p))
         (end (read-bytevector limit p))
         (zero-at-end (read-bytevector 0 p)))
    (write (list zero
                 (if (eof-object? bytes) 'empty
                     (list (bytevector-length bytes)
                           (bytevector-u8-ref bytes 0)
                           (bytevector-u8-ref bytes 9999)))
                 (eof-object? end) zero-at-end))
    (newline)
    (close-port p)))

(check "tenk.bin")
(check "empty.bin")
