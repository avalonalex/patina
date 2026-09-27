(import (scheme base) (scheme read) (prefix (srfi 166) f:)
        (only (srfi 130) string-cursor-back string-cursor-next string-cursor->index)
        (patina compat smoke))
(define (observe thunk) (guard (ex (else 'unexpected-error)) (thunk)))
(define (safe-show . args) (observe (lambda () (apply f:show args))))
;; Gauche rejects out-of-range cursor movement; integer-cursor implementations
;; can tolerate it. Enforce the same boundary in these two CI regressions.
(define (with-checked-cursors thunk)
  (let ((back string-cursor-back) (next string-cursor-next))
    (dynamic-wind
      (lambda ()
        (set! string-cursor-back
          (lambda (str cursor count)
            (if (> count (string-cursor->index str cursor))
                (error "cursor before start") (back str cursor count))))
        (set! string-cursor-next
          (lambda (str cursor)
            (if (>= (string-cursor->index str cursor) (string-length str))
                (error "cursor after end") (next str cursor)))))
      (lambda () (observe thunk))
      (lambda () (set! string-cursor-back back) (set! string-cursor-next next)))))
(define position (f:fn ((r f:row) (c f:col)) (f:written (list r c))))
(check-equal "show captures strings, characters and written objects" "hello 42(a b)"
  (safe-show #f "hello" #\space 42 '(a b)))
(check-equal "explicit output ports receive every formatter" "abc"
  (let ((out (open-output-string)))
    (safe-show out (f:each "a" f:nothing (f:each-in-list '("b" "c"))))
    (get-output-string out)))
(check-equal "true selects the current output port" "current"
  (let ((out (open-output-string)))
    (parameterize ((current-output-port out)) (safe-show #t "current"))
    (get-output-string out)))
(check-error "invalid destinations are rejected" (f:show 'invalid "x"))
(check-equal "displayed and written strings differ" "x|\"x\""
  (safe-show #f (f:displayed "x") "|" (f:written "x")))
(check-equal "nested state bindings restore their enclosing radix" "#xff/255/#xff/255"
  (safe-show #f (f:with ((f:radix 16)) 255 "/" (f:with ((f:radix 10)) 255) "/" 255)
              "/" 255))
(check-equal "persistent state changes are local to one show" '("#xff/#x10" "255")
  (list (safe-show #f (f:with! (f:radix 16)) 255 "/" 16) (safe-show #f 255)))
(check-equal "numeric precision and comma grouping" "12.50/1,234,567"
  (observe (lambda ()
    (safe-show #f (f:with ((f:precision 2)) (f:numeric 25/2)) "/" (f:numeric/comma 1234567)))))
(check-equal "joining invokes the element formatter once per element" '("a,b,c" 3)
  (let ((calls 0))
    (let ((result (safe-show #f (f:joined
                    (lambda (x) (set! calls (+ calls 1)) (f:displayed x)) '(a b c) ","))))
      (list result calls))))
(check-equal "empty joins emit no separators" ""
  (safe-show #f (f:joined f:displayed '() ",") (f:joined/prefix f:displayed '() ",")
              (f:joined/suffix f:displayed '() ",")))
(check-equal "prefix, suffix and final-element joins" ",a,b|a,b,|a,and b"
  (safe-show #f (f:joined/prefix f:displayed '(a b) ",") "|"
              (f:joined/suffix f:displayed '(a b) ",") "|"
              (f:joined/last f:displayed (lambda (x) (f:each "and " x)) '(a b) ",")))
(check-equal "dotted and bounded range joins" "a,b,tail=z|2:3:4"
  (safe-show #f (f:joined/dot f:displayed (lambda (x) (f:each "tail=" x)) '(a b . z) ",")
              "|" (f:joined/range f:displayed 2 5 ":")))
(check-equal "left, right and centered padding" "...ab|ab...|.ab.."
  (safe-show #f (f:with ((f:pad-char #\.))
    (f:padded 5 "ab") "|" (f:padded/right 5 "ab") "|" (f:padded/both 5 "ab"))))
(check-equal "left, right and centered trimming" "def|abc|bcd"
  (safe-show #f (f:trimmed 3 "abcdef") "|" (f:trimmed/right 3 "abcdef") "|"
              (f:trimmed/both 3 "abcdef")))
(check-equal "ellipsis consumes part of the trimming width" "ab..."
  (safe-show #f (f:with ((f:ellipsis "...")) (f:trimmed/right 5 "abcdef"))))
(check-equal "fitting combines padding and trimming" "  ab|abc"
  (safe-show #f (f:fitted 4 "ab") "|" (f:fitted/right 3 "abcdef")))
(check-equal "captured output is passed once to its consumer" '("<ab>" 1)
  (let ((calls 0))
    (let ((result (safe-show #f (f:call-with-output (f:each "a" "b")
                     (lambda (str) (set! calls (+ calls 1)) (f:each "<" str ">"))))))
      (list result calls))))
(check-equal "ordinary output advances the column" "abc(0 3)" (safe-show #f "abc" position))
(check-equal "a leading newline advances the row and resets the column" "\nx(1 1)"
  (safe-show #f "\nx" position))
(check-equal "a trailing newline leaves column zero" "ab\n(1 0)"
  (safe-show #f "ab\n" position))
(check-equal "multiple newlines count rows and the final suffix" "a\nb\nc(2 1)"
  (safe-show #f "a\nb\nc" position))
(check-equal "freshline emits only a needed newline" "x\ny\n"
  (safe-show #f f:fl "x" f:fl f:fl "y" f:nl f:fl))
(check-equal "absolute spacing and tab stops use the current column" "a  bc   d"
  (safe-show #f "a" (f:space-to 3) "b" (f:tab-to 4) "c" (f:tab-to 4) "d"))
(check-equal "pretty output can be read back" '(define (f x) (if x '(a b) '#(c d)))
  (read (open-input-string (safe-show #f (f:with ((f:width 12))
    (f:pretty '(define (f x) (if x '(a b) '#(c d)))))))))
;; Label numbering is deliberately unspecified; check the reconstructed sharing.
(check-equal "shared writing preserves aliases" #t
  (let* ((tail (list 'x)) (text (safe-show #f (f:written-shared (list tail tail))))
         (copy (read (open-input-string text))))
    (and (equal? copy '((x) (x))) (eq? (car copy) (car (cdr copy))))))
(check-equal "shared pretty printing preserves a cycle" #t
  (observe (lambda ()
    (let ((cycle (list 'x)))
      (set-cdr! cycle cycle)
      (let ((copy (read (open-input-string (safe-show #f (f:pretty-shared cycle))))))
        (and (eq? (car copy) 'x) (eq? copy (cdr copy))))))))
(check-equal "terminal widths handle wide and combining characters" '(3 2 1)
  (list (f:string-terminal-width "abc") (f:string-terminal-width "界")
        (f:string-terminal-width "e\x301;")))
(check-equal "terminal-aware padding uses display width" "..界"
  (safe-show #f (f:terminal-aware (f:with ((f:pad-char #\.)) (f:padded 4 "界")))))
(check-equal "case transformers compose with formatters" "ABC/def"
  (safe-show #f (f:upcased "a" "Bc") "/" (f:downcased "D" "Ef")))
(check-equal "nested colors restore their enclosing color" "\x1b;[31ma\x1b;[34mb\x1b;[31mc\x1b;[0m"
  (safe-show #f (f:as-red "a" (f:as-blue "b") "c")))
(check-equal "RGB colors produce their specified ANSI sequences"
  "\x1b;[38;5;196mx\x1b;[0m|\x1b;[48;2;1;2;3my\x1b;[0m"
  (safe-show #f (f:as-color 5 0 0 "x") "|" (f:on-true-color 1 2 3 "y")))
(check-error "invalid palette channels are rejected" (f:show #f (f:as-color 6 0 0 "x")))
(check-error "invalid true-color channels are rejected" (f:show #f (f:as-true-color 0 0 256 "x")))
(check-equal "word wrapping normalizes whitespace and respects width" "one two\nthree"
  (safe-show #f (f:with ((f:width 7)) (f:wrapped "one  two\tthree"))))
(check-equal "character wrapping keeps the final line unterminated" "abcd\nef"
  (safe-show #f (f:with ((f:width 4)) (f:wrapped/char "abcdef"))))
(check-equal "word wrapping admits an exact-width final line" "one two"
  (safe-show #f (f:with ((f:width 7)) (f:wrapped/list '("one" "two")))))
(check-equal "word wrapping handles empty input and indivisible long words" '("" "abcdef")
  (list (safe-show #f (f:wrapped " \t\n"))
        (safe-show #f (f:with ((f:width 3)) (f:wrapped "abcdef")))))
(check-equal "character wrapping joins successive formatter chunks" "abcd\nef"
  (safe-show #f (f:with ((f:width 4)) (f:wrapped/char "ab" "cd" "ef"))))
(check-equal "character wrapping preserves explicit newlines and blank lines" "a\n\nb\n"
  (safe-show #f (f:with ((f:width 4)) (f:wrapped/char "a\n\nb\n"))))
(check-equal "character wrapping respects the existing column" "xab\ncd"
  (safe-show #f "x" (f:with ((f:width 3)) (f:wrapped/char "abcd"))))
(check-equal "character wrapping handles exact-width and empty input" '("abcd" "")
  (list (safe-show #f (f:with ((f:width 4)) (f:wrapped/char "abcd")))
        (safe-show #f (f:wrapped/char ""))))
(check-equal "columnar output aligns finite columns" "ab  12\nc   3\n"
  (observe (lambda () (safe-show #f (f:with ((f:width 8))
    (f:columnar (f:displayed "ab\nc\n") (f:displayed "12\n3\n")))))))
(check-equal "tabular output measures the widest line in each column" "|a |123|\n|bc|4  |\n"
  (observe (lambda () (safe-show #f
    (f:tabular "|" (f:displayed "a\nbc\n") "|" (f:displayed "123\n4\n") "|")))))
(check-equal "numeric grouping never steps before the first character" "1,234,567"
  (with-checked-cursors (lambda () (f:show #f (f:numeric/comma 1234567)))))
(check-equal "word tokenization never steps beyond the final character" "one two"
  (with-checked-cursors (lambda () (f:show #f (f:wrapped "one two")))))
(smoke-finish)
