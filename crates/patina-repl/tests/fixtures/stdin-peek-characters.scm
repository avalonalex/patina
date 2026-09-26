;; #416: peek only the next character, across UTF-8 buffer boundaries.
;; stdin_peek.rs supplies bytes through a pipe or redirected file and checks
;; the copied text, including where an invalid or truncated character raises.
(import (scheme base) (scheme write))

(guard (ex ((error-object? ex) (display "<error>")))
  (let loop ()
    (let* ((first (peek-char))
           (again (peek-char))
           (ready (char-ready?))
           (taken (read-char)))
      (unless (and (equal? first again) (equal? first taken))
        (error "peek changed or consumed the next character"))
      (unless (eof-object? first)
        (unless ready (error "peeked character is not ready"))
        (write-char taken)
        (loop)))))
