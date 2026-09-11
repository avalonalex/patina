;; Two port widenings the compat corpus asked for: `read-line`'s optional
;; max-chars argument (why: the note on `text_input::read_line`) and textual
;; reads decoding UTF-8 from binary ports (why: `decode_utf8_at` in
;; `port.rs`). Both mirror chibi; chibi-mime exercises both.
;;
;; Migrated from `crates/patina-tests/tests/binary_port_textual_reads.rs`
;; (#193), which is deleted.
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

(import (scheme base) (srfi 64))

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

(test-end)
