;; #360: directives belong to the input port and persist across read calls.
;; Raw character/line reads do not interpret directives. In particular, a
;; directive seen only by lookahead must not take effect before it is read.
;; Measured 2026-09-29: Chibi 0.12 and Gauche 0.9.15 both pass all 12 rows.
(import (scheme base) (scheme read) (srfi 64))

(test-begin "fold-case-state")

(define (read-all port)
  (let loop ((out '()))
    (let ((x (read port)))
      (if (eof-object? x) (reverse out) (loop (cons x out))))))
(define (both-ports text procedure)
  (list (procedure (open-input-string text))
        (procedure (open-input-bytevector (string->utf8 text)))))

(test-equal "folding and later disabling persist across reads"
  '((abc def ghi JKL MNO) (abc def ghi JKL MNO))
  (both-ports "#!fold-case ABC DEF GHI #!no-fold-case JKL MNO" read-all))
(test-equal "directives on separate lines persist"
  '((abc def GHI JKL) (abc def GHI JKL))
  (both-ports "#!fold-case\nABC\nDEF\n#!no-fold-case\nGHI\nJKL" read-all))
(test-equal "a directive inside a datum affects subsequent reads"
  '(((a) b c) ((a) b c))
  (both-ports "(#!fold-case A) B C" read-all))
(test-equal "a directive inside a datum comment persists"
  '((abc def) (abc def))
  (both-ports "#;(#!fold-case IGNORED) ABC DEF" read-all))
(test-equal "character names use the mode of preceding reads"
  '((a #\newline #\space) (a #\newline #\space))
  (both-ports "#!fold-case A #\\NEWLINE #\\Space" read-all))

(test-equal "lookahead cannot disable folding before a raw line read"
  '((a b c) (a b c))
  (both-ports "#!fold-case A #!no-fold-case\nB C"
    (lambda (p)
      (let* ((a (read p)) (line (read-line p)) (b (read p)) (c (read p)))
        (list a b c)))))
(test-equal "lookahead cannot enable folding before a raw line read"
  '((A B C) (A B C))
  (both-ports "A #!fold-case\nB C"
    (lambda (p)
      (let* ((a (read p)) (line (read-line p)) (b (read p)) (c (read p)))
        (list a b c)))))
(test-equal "peeks and character reads preserve the current mode"
  '((a #\space #\space b) (a #\space #\space b))
  (both-ports "#!fold-case A B"
    (lambda (p)
      (let* ((a (read p)) (peek (peek-char p)) (space (read-char p)) (b (read p)))
        (list a peek space b)))))
(test-equal "directive text in strings and comments does not change the mode"
  '((a "#!no-fold-case" b c) (a "#!no-fold-case" b c))
  (both-ports "#!fold-case A \"#!no-fold-case\" ; #!no-fold-case\nB #| #!no-fold-case |# C" read-all))
(test-equal "separate ports keep separate modes"
  '(abc ABC def DEF)
  (let* ((p (open-input-string "#!fold-case ABC DEF"))
         (q (open-input-string "ABC DEF"))
         (a (read p)) (b (read q)) (c (read p)) (d (read q)))
    (list a b c d)))
(test-equal "aliases and the current input port share the mode"
  '(abc def ghi)
  (let* ((p (open-input-string "#!fold-case ABC DEF GHI"))
         (alias p) (a (read p)) (b (read alias))
         (c (parameterize ((current-input-port p)) (read))))
    (list a b c)))
(test-equal "ending after a directive reports EOF"
  '(#t #t)
  (both-ports "#!fold-case #;IGNORED #!no-fold-case"
    (lambda (p) (and (eof-object? (read p)) (eof-object? (read p))))))

(test-end)
