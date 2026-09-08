;; Line endings — R7RS §7.1.1: a line ending is a newline, a return, or a
;; return followed by a newline, and all three end whatever the line was.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (#193
;; Phase 1), where these three rows were scattered across Larceny family 16,
;; that file's "Review of #112" section, and its "What `base` found once it
;; ran" section. They are one concern — a *bare* return ends a line — and the
;; three places it can be got wrong: a `;` comment, a `#!` line, and
;; `read-line`. Grouping them is the point of the redistribution; nothing here
;; is new.
;;
;; The bare return is the case implementations forget, because a file written
;; on any modern system does not contain one. Larceny's suites do.

(import (scheme base) (scheme read) (srfi 64))

(test-begin "line-endings")

;; Ours ran a `;` comment to the *newline* only, so a datum after a
;; return-terminated comment was swallowed. Fixed 2026-08-24.
;;
;; Two rows rather than the `.rs` file's one, so that each ending's failure
;; names itself instead of arriving as a four-element list to diff.
;;
;; **`let*`, not argument positions**, and this file is where that rule earned
;; its place in `docs/TEST_ORGANIZATION.md`. Written as
;; `(list (read p) (read p) …)` these reads happen right-to-left on chibi, and
;; the reversed order made chibi look like it failed the return+newline row
;; too — an `oracle-defect` was very nearly registered against it for a bug in
;; the test. Sequenced properly, chibi answers that row exactly as we do.
(define bare-return-port (open-input-string "first ; comment\rsecond"))
(test-equal "a line comment ends at a bare return" '(first second)
  (let* ((a (read bare-return-port)) (b (read bare-return-port)))
    (list a b)))

;; A return+newline is *one* ending, not two, so there is no empty datum
;; between the comment and `third`. All three implementations agree here.
(define crlf-port (open-input-string "first ; comment\r\nthird"))
(test-equal "a return+newline pair is one ending" '(first third #t)
  (let* ((a (read crlf-port)) (b (read crlf-port)) (c (eof-object? (read crlf-port))))
    (list a b c)))

;; A shebang line is a comment by another spelling, and ends the same way.
(test-equal "a shebang line ends at a bare return" 42
  (read (open-input-string "#!/usr/bin/env patina\r42")))

;; `read-line` defers to the same rule (R7RS §6.13.2 → §7.1.1), and chibi and
;; Gauche split all three endings. Fixed 2026-08-25. The fourth case is the one
;; that catches an off-by-one: a return+newline pair is *one* ending, so
;; `"abc\r\ndef"` has a second line of `"def"`, not `""`.
(define (lines s)
  (let ((p (open-input-string s)))
    (let loop ((acc '()))
      (let ((l (read-line p)))
        (if (eof-object? l) (reverse acc) (loop (cons l acc)))))))

(test-equal "read-line ends a line at all three endings"
  '(("abc" "def") ("abc" "def") ("abc" "def") ("abc") ("" ""))
  (map lines '("abc\ndef" "abc\rdef" "abc\r\ndef" "abc\r" "\r\n\n")))

(test-end)
