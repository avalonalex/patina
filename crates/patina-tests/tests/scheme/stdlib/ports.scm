;; Two port widenings the compat corpus asked for: `read-line`'s optional
;; max-chars argument (why: the note on `text_input::read_line`) and textual
;; reads decoding UTF-8 from binary ports (why: `decode_utf8_at` in
;; `port.rs`). Both mirror chibi; chibi-mime exercises both.
;;
;; Migrated from `crates/patina-tests/tests/binary_port_textual_reads.rs`
;; (#193), which is deleted. Two later sections came from files that keep
;; their file-backed rows in Rust, because a suite file has no way to make a
;; file: the standard ports as parameters, from `standard_ports.rs` (deleted
;; once its one binary-file row moved to `vfs_file_io.rs`), and `read`
;; consuming exactly one datum, the string-port half of `read_consumption.rs`.
;;
;; **Neither is R7RS.** `read-line` takes one optional argument, the port, and
;; a textual procedure on a binary port "is an error". So the rows assert
;; Patina's extensions, and only implementations that share them can
;; corroborate: the max-chars rows are scoped to Patina and chibi, whose
;; extension it is. Gauche's `read-line` takes a different second argument.
;;
;; Every row that reads more than once sequences its reads with `let*`. The
;; Rust originals wrote them as arguments to `list`, whose evaluation order is
;; unspecified, and chibi evaluates arguments right to left.

(import (scheme base) (scheme read) (scheme write) (srfi 64))

(test-begin "ports")

;; ─── read-line's max-chars argument ──────────────────────────────────────────

(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "read-line with a limit splits a long line"
  '("hell" "o wo" "rld" "seco" "nd" #t)
  (let* ((p (open-input-string "hello world\nsecond"))
         (a (read-line p 4))
         (b (read-line p 4))
         (c (read-line p 4))
         (d (read-line p 4))
         (e (read-line p 4))
         (end (eof-object? (read-line p 4))))
    (list a b c d e end)))

;; The limit counts characters, not bytes — chibi's unit. Written with
;; two-byte λs so a byte-counting regression truncates visibly early.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "the limit counts characters, not bytes" '("λλλλ" "λ")
  (let* ((p (open-input-string "λλλλλ"))
         (a (read-line p 4))
         (b (read-line p 4)))
    (list a b)))

;; A limit larger than the line changes nothing, and the plain one-argument
;; form is unaffected.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "a limit longer than the line changes nothing" '("short" "rest")
  (let* ((p (open-input-string "short\nrest"))
         (a (read-line p 100))
         (b (read-line p)))
    (list a b)))

;; CRLF handling: \r is a line ending (R7RS 7.1.1), and return+newline is one
;; ending, so a limit that lands on the \r still ends the line there and the
;; \n goes with it — the next read sees "cd", as in chibi. (This used to pin
;; the older behaviour, where only \n terminated and a cut between the two
;; left the \r as data and an empty line after it.)
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "a return-newline pair is one line ending" '("ab" "cd")
  (let* ((p (open-input-string "ab\r\ncd"))
         (a (read-line p 10))
         (b (read-line p 10)))
    (list a b)))
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "a limit landing on the return still ends the line there"
  '("ab" "cd" #t)
  (let* ((p (open-input-string "ab\r\ncd"))
         (a (read-line p 3))
         (b (read-line p 3))
         (end (eof-object? (read-line p 3))))
    (list a b end)))

;; ─── Textual reads on a binary port ──────────────────────────────────────────

(test-equal "read-line, peek-char and read-char on a binary port"
  '("hi" #\t #\t "here")
  (let* ((p (open-input-bytevector (string->utf8 "hi\nthere")))
         (a (read-line p))
         (b (peek-char p))
         (c (read-char p))
         (d (read-line p)))
    (list a b c d)))

;; Multi-byte UTF-8 decodes correctly from the byte stream, and the binary
;; operations still see the bytes the textual ones did not consume.
(test-equal "textual and binary reads share one position"
  '(#\λ #\x #\newline 114)
  (let* ((p (open-input-bytevector (string->utf8 "λx\nrest")))
         (a (read-char p))
         (b (read-char p))
         (c (read-char p))
         (d (read-u8 p)))
    (list a b c d)))

;; The limited form composes with binary ports too — the exact call shape
;; chibi-mime's header reader uses.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "read-line with a limit on a binary port" '("From: a@b" "" "body")
  (let* ((p (open-input-bytevector (string->utf8 "From: a@b\n\nbody")))
         (a (read-line p 4096))
         (b (read-line p 4096))
         (c (read-line p 4096)))
    (list a b c)))

;; ─── The standard ports are parameter objects (R7RS §6.13.1) ───────────────

;; They used to be plain zero-argument procedures, so `parameterize` rejected
;; them outright — the one failure in SRFI 158's upstream suite, which defines
;; its own `with-input-from-string` in exactly those terms. Why writing the
;; backing thread-local *is* rebinding is explained above the three procedures
;; in `patina-primitives/src/primitives/io/ports.rs`.
;;
;; Restoration is asserted by where output *lands*, never by port identity:
;; `(current-output-port)` allocates a fresh wrapper per call, so `eq?` on two
;; reads is #f regardless.

(test-equal "parameterize rebinds the current input port" 'a
  (parameterize ((current-input-port (open-input-string "a b c"))) (read)))

;; A primitive given no port argument reads the current port, so it must
;; observe the rebinding.
(test-equal "output with no port argument follows the parameter" "a42\nb"
  (let ((out (open-output-string)))
    (parameterize ((current-output-port out))
      (display "a") (write 42) (newline) (write-string "b"))
    (get-output-string out)))

(test-equal "the previous output port is restored" '("in" "out")
  (let ((outer (open-output-string))
        (inner (open-output-string)))
    (parameterize ((current-output-port outer))
      (parameterize ((current-output-port inner)) (display "in"))
      (display "out"))
    (list (get-output-string inner) (get-output-string outer))))

;; `parameterize` restores through `dynamic-wind`, so an escape must not leave
;; the world writing into a string port.
(test-equal "an escape restores the output port" '("in" "after")
  (let ((outer (open-output-string))
        (inner (open-output-string)))
    (parameterize ((current-output-port outer))
      (call-with-current-continuation
        (lambda (k)
          (parameterize ((current-output-port inner))
            (display "in")
            (k 'escaped))))
      (display "after"))
    (list (get-output-string inner) (get-output-string outer))))

;; The third port, which the rows above do not otherwise reach.
(test-equal "the current error port is a parameter too" "2"
  (let ((e (open-output-string)))
    (parameterize ((current-error-port e)) (display 2 (current-error-port)))
    (get-output-string e)))

(test-equal "reading each standard port takes no argument" '(#t #t #t)
  (list (output-port? (current-output-port))
        (input-port? (current-input-port))
        (output-port? (current-error-port))))

;; Patina's standard ports also accept one argument, the setter `parameterize`
;; uses, and that must not turn them into procedures that accept anything: a
;; non-port, and a port facing the wrong way, are rejected. Scoped to Patina,
;; and skipped rather than registered elsewhere: R7RS gives a parameter object
;; no arguments, and chibi 0.12 and Gauche 0.9.15 both treat the call as a
;; setter that accepts a non-port, which rebinds the port SRFI 64 itself
;; reports to (measured 2026-09-11).
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a non-port or a wrong-direction port is rejected"
  '(rejected rejected rejected)
  (let* ((a (guard (e (#t 'rejected)) (current-output-port 5) 'accepted))
         (b (guard (e (#t 'rejected))
              (current-input-port (open-output-string))
              'accepted))
         (c (guard (e (#t 'rejected))
              (current-output-port (open-input-string "x"))
              'accepted)))
    (list a b c)))

;; ─── read consumes exactly one datum ─────────────────────────────────────────

;; `read_consumption.rs` pins a bug where `read` on file ports accumulated a
;; whole line, parsed one datum and discarded the rest, so a second `read` on
;; "5 40" returned end of file. Its file-port rows stay there; these are its
;; string-port rows, the control that shows the rule is the port's and not
;; the file's.

(test-equal "several datums on one line are read one at a time"
  '(5 40 102334155 #t)
  (let* ((p (open-input-string "5 40 102334155"))
         (a (read p))
         (b (read p))
         (c (read p))
         (d (read p)))
    (list a b c (eof-object? d))))

;; read consumes the datum but not the delimiter after it.
(test-equal "read leaves the delimiter for read-char" '(5 #\space 40)
  (let* ((p (open-input-string "5 40"))
         (a (read p))
         (c (read-char p))
         (b (read p)))
    (list a c b)))

(test-equal "read takes one list datum at a time" '((1 2) (3 4))
  (let* ((p (open-input-string "(1 2) (3 4)"))
         (a (read p))
         (b (read p)))
    (list a b)))

(test-end)
