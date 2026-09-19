;; Two port widenings the compat corpus asked for: `read-line`'s optional
;; max-chars argument (why: the note on `text_input::read_line`) and the
;; textual operations working on binary ports, as UTF-8 — reads (why:
;; `decode_utf8_at` in `port.rs`; chibi-mime exercises them) and, since #404,
;; writes and `read` (chibi-binary-record writes its string fields to a
;; bytevector port, which is what chibi-tar is built on). Both mirror chibi.
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
;; The binary-port rows need no scoping: chibi and Gauche both answer them as
;; written. The other direction — bytes on a *string* port — is where the two
;; part ways (chibi refuses, Gauche allows), Patina refuses with chibi, and
;; nothing here asserts it.
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

;; `read` is a textual read like the others. It takes the datum and nothing
;; after it, so the delimiter is still there for whoever reads next.
(test-equal "read on a binary port" '((1) x #t)
  (let* ((p (open-input-bytevector (string->utf8 "(1) x ")))
         (a (read p))
         (b (read p))
         (c (eof-object? (read p))))
    (list a b c)))

;; What follows the datum need not be text at all — a header, then a binary
;; body — so `read` must not decode further than it reads: 255 is not UTF-8.
(test-equal "read leaves the bytes after the datum to the binary operations"
  '(x 32 255)
  (let* ((p (open-input-bytevector (bytevector 120 32 255)))
         (a (read p))
         (b (read-u8 p))
         (c (read-u8 p)))
    (list a b c)))

;; ─── Textual writes on a binary port ─────────────────────────────────────────
;;
;; The other half, and the half Patina lacked until #404: it read text from a
;; binary port and refused to write it. Text goes out as the UTF-8 it came in
;; as.

(test-equal "write-string and write-char on a binary port"
  (bytevector 102 111 111 33)
  (let ((o (open-output-bytevector)))
    (write-string "foo" o)
    (write-char #\! o)
    (get-output-bytevector o)))

(test-equal "text is written as UTF-8" (bytevector 206 187 120)
  (let ((o (open-output-bytevector)))
    (write-char #\λ o)
    (write-string "x" o)
    (get-output-bytevector o)))

;; The shape chibi-binary-record's field writers have: bytes, a padded string,
;; bytes again, into one port.
(test-equal "textual and binary writes share one position"
  (bytevector 1 97 98 2)
  (let ((o (open-output-bytevector)))
    (write-u8 1 o)
    (write-string "ab" o)
    (write-u8 2 o)
    (get-output-bytevector o)))

(test-equal "write-string with a start and an end on a binary port"
  (bytevector 101 108)
  (let ((o (open-output-bytevector)))
    (write-string "hello" o 1 3)
    (get-output-bytevector o)))

;; Every textual writer ends in the same place, so the datum writers and
;; `newline` follow. Compared as text, since the bytes are only its encoding.
(test-equal "write, display and newline on a binary port"
  "(1 \"a\" #\\b)hi\n"
  (let ((o (open-output-bytevector)))
    (write '(1 "a" #\b) o)
    (display "hi" o)
    (newline o)
    (utf8->string (get-output-bytevector o))))

;; ─── Every port is textual ───────────────────────────────────────────────────
;;
;; `textual-port?` asks whether the textual operations work, and above they
;; work on a binary port in both directions. R7RS §6.13.1 leaves whether the
;; two port types are disjoint to the implementation; chibi and Gauche both
;; answer `#t` here, and an implementation that answered `#f` while reading
;; and writing text through the port would be contradicting itself. The
;; file-port half of this is in `vfs_file_io.rs`, since a suite file has no
;; way to make a file.
(test-equal "a bytevector port is textual, and still binary" '(#t #t #t #t)
  (let ((in (open-input-bytevector (bytevector 1)))
        (out (open-output-bytevector)))
    (list (textual-port? in) (textual-port? out)
          (binary-port? in) (binary-port? out))))

(test-equal "a string port is textual" '(#t #t)
  (list (textual-port? (open-input-string "x"))
        (textual-port? (open-output-string))))

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

;; EOF before a datum starts is normal; EOF inside its representation is a
;; read error (R7RS 6.13.2). Completed comments do not start a datum.
(define (read-result p)
  (guard (e (else (if (and (error-object? e) (read-error? e))
                     'read-error
                     'other-error)))
    (let ((datum (read p)))
      (if (eof-object? datum) 'eof datum))))

(test-equal "an unfinished list raises a read error" 'read-error
  (read-result (open-input-string "(")))

(test-equal "an unfinished vector raises a read error" 'read-error
  (read-result (open-input-string "#(")))

(test-equal "an unfinished bytevector raises a read error" 'read-error
  (read-result (open-input-string "#u8(")))

(test-equal "a lone quote raises a read error" 'read-error
  (read-result (open-input-string "'")))

(test-equal "a lone quasiquote raises a read error" 'read-error
  (read-result (open-input-string "`")))

(test-equal "a lone unquote raises a read error" 'read-error
  (read-result (open-input-string ",")))

(test-equal "a lone unquote-splicing raises a read error" 'read-error
  (read-result (open-input-string ",@")))

(test-equal "an unfinished dotted tail raises a read error" 'read-error
  (read-result (open-input-string "(1 .")))

(test-equal "a datum label without its datum raises a read error" 'read-error
  (read-result (open-input-string "#1=")))

(test-equal "a datum comment without its datum raises a read error" 'read-error
  (read-result (open-input-string "#;")))

(test-equal "an unfinished commented list raises a read error" 'read-error
  (read-result (open-input-string "#; (")))

(test-equal "an unterminated string raises a read error" 'read-error
  (read-result (open-input-string "\"unfinished")))

(test-equal "an unterminated identifier raises a read error" 'read-error
  (read-result (open-input-string "|unfinished")))

(test-equal "an unterminated block comment raises a read error" 'read-error
  (read-result (open-input-string "#| unfinished")))

(define (clean-eof-result input)
  (let* ((p (open-input-string input))
         (a (read-result p))
         (b (read-result p)))
    (list a b (eof-object? (read-char p)))))

(test-equal "clean EOF after empty input" '(eof eof #t)
  (clean-eof-result ""))

(test-equal "clean EOF after whitespace" '(eof eof #t)
  (clean-eof-result " \n"))

(test-equal "clean EOF after a line comment" '(eof eof #t)
  (clean-eof-result "; comment"))

(test-equal "clean EOF after a block comment" '(eof eof #t)
  (clean-eof-result "#| comment |#"))

(test-equal "clean EOF after a datum comment" '(eof eof #t)
  (clean-eof-result "#; (1 2)"))

(test-equal "clean EOF after nested datum comments" '(eof eof #t)
  (clean-eof-result "#; #; 1 2"))

(test-equal "a complete datum precedes an incomplete next datum" '(1 read-error)
  (let* ((p (open-input-string "1 ("))
         (a (read-result p))
         (b (read-result p)))
    (list a b)))

(test-end)
