;; #416: a character buffered by peek is visible to every textual read.
;; stdin_peek.rs supplies a boundary-straddling lambda after 8191 ASCII bytes.
(import (scheme base) (scheme read) (scheme write))

(read-string 8191)
(let* ((a (peek-char))
       (b (read-string 3))
       (c (peek-char))
       (d (read-line))
       (e (peek-char))
       (f (read))
       (g (peek-char))
       (h (read-char))
       (end (peek-char)))
  (write (list a b c d e f g h (eof-object? end)))
  (newline))
