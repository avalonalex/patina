;; #412: datum, byte, and character reads share stdin's buffered position.
;; Input: "x λyz\nrest". The Rust driver supplies both a file and a pipe.
(import (scheme base) (scheme read) (scheme write))
(define target (bytevector 0 0 0 0))
(let* ((datum (read))
       (ready (u8-ready? (current-input-port)))
       (peek1 (peek-u8)) (peek2 (peek-u8))
       (empty (read-bytevector 0))
       (space (read-u8))
       (lead (read-bytevector 1))
       (count (read-bytevector! target (current-input-port) 1 3))
       (char-peek (peek-char)) (char (read-char))
       (line (read-line)) (next (read))
       (end (eof-object? (read-u8))))
  (write (list datum ready peek1 peek2 empty space lead count target
               char-peek char line next end))
  (newline))
