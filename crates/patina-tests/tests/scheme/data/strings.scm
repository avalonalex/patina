;; Strings — R7RS §6.7, with the non-ASCII cases that a byte-oriented
;; representation would get wrong.
;;
;; Migrated from `crates/patina-tests/tests/compliance/strings.rs` (#193),
;; which is deleted. One row here per Rust assertion. Two changes of shape:
;;
;; - Programs that defined a string and then mutated it are `let` bodies.
;; - **Every mutated string is made by `make-string` or `string-copy`, never a
;;   literal.** Literals are immutable in R7RS (§3.4), and mutating one "is an
;;   error". Four Rust rows called `string-set!` on a literal. Patina allows
;;   it, but the rows were about `string-set!`, not about literals, and a
;;   `string-set!` error row on a literal could pass for the wrong reason on
;;   an implementation that refuses the literal before it looks at the index.
;;
;; The case-insensitive comparisons are `(scheme char)`'s, not `(scheme
;; base)`'s, so the file imports it.

(import (scheme base) (scheme char) (srfi 64))

(test-begin "strings")

;; ─── Predicates and length ───────────────────────────────────────────────────

(test-equal "string? of a string" #t (string? "hello"))
(test-equal "string? of the empty string" #t (string? ""))
(test-equal "string? of a symbol" #f (string? 'hello))
(test-equal "string? of a number" #f (string? 42))
(test-equal "string? of a character" #f (string? #\a))

(test-equal "length of the empty string" 0 (string-length ""))
(test-equal "length of hello" 5 (string-length "hello"))
(test-equal "length of hello world" 11 (string-length "hello world"))

;; Length counts characters, not bytes.
(test-equal "length of two CJK characters" 2 (string-length "你好"))
(test-equal "length of mixed ASCII and CJK" 8 (string-length "Hello 世界"))
(test-equal "length with an emoji" 7 (string-length "Hello 🌍"))
(test-equal "length with an accented letter" 4 (string-length "Café"))

;; ─── string-ref ──────────────────────────────────────────────────────────────

(test-equal "string-ref at 0" #\h (string-ref "hello" 0))
(test-equal "string-ref at the end" #\o (string-ref "hello" 4))
(test-equal "string-ref in the middle" #\r (string-ref "world" 2))

(test-equal "string-ref of the first CJK character" #\你 (string-ref "你好" 0))
(test-equal "string-ref of the second CJK character" #\好 (string-ref "你好" 1))
(test-equal "string-ref past ASCII into CJK" #\世 (string-ref "Hello 世界" 6))
(test-equal "string-ref of the last CJK character" #\界 (string-ref "Hello 世界" 7))
(test-equal "string-ref of an accented letter" #\é (string-ref "Café" 3))

(test-error "string-ref at a negative index" #t (string-ref "hello" -1))
(test-error "string-ref at the length" #t (string-ref "hello" 5))
(test-error "string-ref far past the end" #t (string-ref "hello" 100))

;; ─── Construction ────────────────────────────────────────────────────────────

(test-equal "make-string of length 0" "" (make-string 0))
(test-equal "make-string with a fill" "aaaaa" (make-string 5 #\a))
(test-equal "make-string with another fill" "***" (make-string 3 #\*))
(test-equal "make-string has the length asked for" 10
  (string-length (make-string 10 #\x)))
(test-equal "make-string with a CJK fill" "世世世" (make-string 3 #\世))
(test-equal "a CJK fill still counts characters" 3
  (string-length (make-string 3 #\世)))
(test-equal "make-string with a kana fill" "いいいいい" (make-string 5 #\い))

(test-equal "string of nothing" "" (string))
(test-equal "string of one character" "h" (string #\h))
(test-equal "string of five characters" "hello" (string #\h #\e #\l #\l #\o))
(test-equal "string of CJK characters" "你好" (string #\你 #\好))
(test-equal "string with an accented letter" "Café" (string #\C #\a #\f #\é))
(test-equal "string of an emoji" "💡" (string #\💡))

;; R7RS §6.6's required character names.
(test-equal "#\\alarm is a character" #t (char? #\alarm))
(test-equal "#\\backspace is a character" #t (char? #\backspace))
(test-equal "#\\delete is a character" #t (char? #\delete))
(test-equal "#\\escape is a character" #t (char? #\escape))
(test-equal "#\\null is a character" #t (char? #\null))
(test-equal "#\\return is a character" #t (char? #\return))
(test-equal "#\\space is a character" #t (char? #\space))
(test-equal "#\\tab is a character" #t (char? #\tab))
(test-equal "#\\newline is a character" #t (char? #\newline))

;; The hex scalar value notation.
(test-equal "#\\x03BB is a character" #t (char? #\x03BB))
(test-equal "#\\x03BB is lambda" "λ" (string #\x03BB))
(test-equal "#\\x03B1 is alpha" "α" (string #\x03B1))
(test-equal "#\\x4E16 is a CJK character" "世" (string #\x4E16))

;; ─── Mutation ────────────────────────────────────────────────────────────────

(test-equal "string-set! in a made string" "aaXaa"
  (let ((s (make-string 5 #\a))) (string-set! s 2 #\X) s))
(test-equal "string-set! at index 0" "Hello"
  (let ((s (string-copy "hello"))) (string-set! s 0 #\H) s))
;; Replacing an ASCII character with a multi-byte one must not shift the rest.
(test-equal "string-set! a CJK character into ASCII" "Hello 世orld"
  (let ((s (string-copy "Hello World"))) (string-set! s 6 #\世) s))
(test-equal "string-set! a CJK character into a made string" "a你a"
  (let ((s (make-string 3 #\a))) (string-set! s 1 #\你) s))

(test-error "string-set! at a negative index" #t
  (string-set! (string-copy "hello") -1 #\x))
(test-error "string-set! at the length" #t
  (string-set! (string-copy "hello") 5 #\x))
(test-error "string-set! of a non-character" #t
  (string-set! (string-copy "hello") 0 42))

;; ─── Comparison ──────────────────────────────────────────────────────────────

(test-equal "string=? of equal strings" #t (string=? "hello" "hello"))
(test-equal "string=? of different strings" #f (string=? "hello" "world"))
(test-equal "string=? of empty strings" #t (string=? "" ""))
(test-equal "string=? is case-sensitive" #f (string=? "hello" "Hello"))
(test-equal "string=? of three equal strings" #t (string=? "a" "a" "a"))
(test-equal "string=? of three with one different" #f (string=? "a" "a" "b"))
(test-equal "string=? of equal CJK strings" #t (string=? "你好" "你好"))
(test-equal "string=? of different CJK strings" #f (string=? "你好" "世界"))
(test-equal "string=? with an accented letter" #t (string=? "Café" "Café"))

(test-equal "string<? a b" #t (string<? "a" "b"))
(test-equal "string<? b a" #f (string<? "b" "a"))
(test-equal "string<? apple banana" #t (string<? "apple" "banana"))
(test-equal "string<? of equal strings" #f (string<? "hello" "hello"))
(test-equal "string<? of three in order" #t (string<? "a" "b" "c"))
(test-equal "string<? of three out of order" #f (string<? "a" "c" "b"))

(test-equal "string>? b a" #t (string>? "b" "a"))
(test-equal "string>? a b" #f (string>? "a" "b"))
(test-equal "string>? banana apple" #t (string>? "banana" "apple"))

(test-equal "string<=? a b" #t (string<=? "a" "b"))
(test-equal "string<=? a a" #t (string<=? "a" "a"))
(test-equal "string<=? b a" #f (string<=? "b" "a"))

(test-equal "string>=? b a" #t (string>=? "b" "a"))
(test-equal "string>=? a a" #t (string>=? "a" "a"))
(test-equal "string>=? a b" #f (string>=? "a" "b"))

;; Lexicographic ordering: a prefix sorts first, and uppercase ASCII sorts
;; before lowercase.
(test-equal "the empty string sorts first" #t (string<? "" "a"))
(test-equal "a prefix sorts first" #t (string<? "a" "aa"))
(test-equal "a shared prefix compares the rest" #t (string<? "apple" "application"))
(test-equal "uppercase sorts before lowercase" #t (string<? "A" "a"))

;; ─── Case-insensitive comparison, from (scheme char) ─────────────────────────

(test-equal "string-ci=? Hello hello" #t (string-ci=? "Hello" "hello"))
(test-equal "string-ci=? HELLO hello" #t (string-ci=? "HELLO" "hello"))
(test-equal "string-ci=? of different strings" #f (string-ci=? "hello" "world"))
(test-equal "string-ci=? of three" #t (string-ci=? "ABC" "abc" "AbC"))

(test-equal "string-ci<? of strings equal but for case" #f (string-ci<? "abc" "ABC"))
(test-equal "string-ci<? abc BCD" #t (string-ci<? "abc" "BCD"))
(test-equal "string-ci<? Apple BANANA" #t (string-ci<? "Apple" "BANANA"))
(test-equal "string-ci<? A a" #f (string-ci<? "A" "a"))

(test-equal "string-ci>? BCD abc" #t (string-ci>? "BCD" "abc"))
(test-equal "string-ci>? of strings equal but for case" #f (string-ci>? "abc" "ABC"))

(test-equal "string-ci<=? of strings equal but for case" #t (string-ci<=? "abc" "ABC"))
(test-equal "string-ci<=? abc BCD" #t (string-ci<=? "abc" "BCD"))

(test-equal "string-ci>=? of strings equal but for case" #t (string-ci>=? "ABC" "abc"))
(test-equal "string-ci>=? BCD abc" #t (string-ci>=? "BCD" "abc"))

;; ─── string-append and substring ─────────────────────────────────────────────

(test-equal "string-append of nothing" "" (string-append))
(test-equal "string-append of one" "hello" (string-append "hello"))
(test-equal "string-append of three" "hello world" (string-append "hello" " " "world"))
(test-equal "string-append of four" "abcd" (string-append "a" "b" "c" "d"))
(test-equal "string-append of CJK characters" "你好" (string-append "你" "好"))
(test-equal "string-append of mixed strings" "Hello 世界"
  (string-append "Hello " "世" "界"))
(test-equal "string-append of the empty string" "" (string-append ""))
(test-equal "string-append of three empty strings" "" (string-append "" "" ""))

(test-equal "substring of the whole string" "hello" (substring "hello" 0 5))
(test-equal "substring of the middle" "ell" (substring "hello" 1 4))
(test-equal "substring of the first word" "hello" (substring "hello world" 0 5))
(test-equal "substring of the second word" "world" (substring "hello world" 6 11))
(test-equal "substring of nothing at 0" "" (substring "hello" 0 0))
(test-equal "substring of nothing in the middle" "" (substring "hello" 2 2))
(test-equal "substring of leading CJK" "你好" (substring "你好世界" 0 2))
(test-equal "substring of trailing CJK" "世界" (substring "你好世界" 2 4))
(test-equal "substring of the ASCII half" "Hello" (substring "Hello 世界" 0 5))
(test-equal "substring of the CJK half" "世界" (substring "Hello 世界" 6 8))

(test-error "substring from a negative start" #t (substring "hello" -1 3))
(test-error "substring past the end" #t (substring "hello" 0 6))
(test-error "substring with its end before its start" #t (substring "hello" 3 2))

;; ─── Conversions ─────────────────────────────────────────────────────────────

(test-equal "string->list of the empty string" '() (string->list ""))
(test-equal "string->list of abc" '(#\a #\b #\c) (string->list "abc"))
(test-equal "string->list of hello" '(#\h #\e #\l #\l #\o) (string->list "hello"))
(test-equal "string->list of CJK" '(#\你 #\好) (string->list "你好"))
(test-equal "string->list with an accented letter" '(#\C #\a #\f #\é)
  (string->list "Café"))
(test-equal "string->list from a start" '(#\e #\l #\l #\o) (string->list "hello" 1))
(test-equal "string->list of a range" '(#\e #\l #\l) (string->list "hello" 1 4))
(test-equal "string->list of an empty range" '() (string->list "hello" 0 0))

(test-equal "list->string of the empty list" "" (list->string '()))
(test-equal "list->string of abc" "abc" (list->string '(#\a #\b #\c)))
(test-equal "list->string of hello" "hello" (list->string '(#\h #\e #\l #\l #\o)))
(test-equal "list->string of CJK" "你好" (list->string '(#\你 #\好)))
(test-equal "list->string with an accented letter" "Café"
  (list->string '(#\C #\a #\f #\é)))
(test-equal "list->string of other CJK" "世界" (list->string '(#\世 #\界)))

(test-equal "string->list round-trips ASCII" "hello"
  (list->string (string->list "hello")))
(test-equal "string->list round-trips CJK" "你好"
  (list->string (string->list "你好")))
(test-equal "list->string round-trips" '(#\a #\b)
  (string->list (list->string '(#\a #\b))))

;; The Rust file's edge-case test repeated four expressions other tests in it
;; already asserted — `(string-length "")`, `(string->list "")`,
;; `(list->string '())` and `(string-copy "")` — with the same answers. Each is
;; a row in this file once.

(test-equal "string-copy of a string" "hello" (string-copy "hello"))
(test-equal "string-copy of the empty string" "" (string-copy ""))
(test-equal "string-copy makes a new string" "hello"
  (let* ((s1 (string-copy "hello"))
         (s2 (string-copy s1)))
    (string-set! s2 0 #\H)
    s1))
(test-equal "string-copy from a start" "ello" (string-copy "hello" 1))
(test-equal "string-copy of a range" "ell" (string-copy "hello" 1 4))
(test-equal "string-copy of the whole range" "hello" (string-copy "hello" 0 5))
(test-equal "string-copy of a CJK range" "好世" (string-copy "你好世界" 1 3))

;; ─── Combined ────────────────────────────────────────────────────────────────

(test-equal "the length of an appended string" 11
  (string-length (string-append "Hello" " " "World")))
(test-equal "a substring is a new, mutable string" "Hello"
  (let ((s (substring "hello world" 0 5))) (string-set! s 0 #\H) s))
(test-equal "copies are independent of each other" "Xaa Yaa"
  (let* ((s1 (make-string 3 #\a))
         (s2 (string-copy s1)))
    (string-set! s1 0 #\X)
    (string-set! s2 0 #\Y)
    (string-append s1 " " s2)))

(test-equal "length of mixed content" 14 (string-length "Hello 世界 World"))
(test-equal "string-ref of an accented letter in mixed content" #\é
  (string-ref "Café 咖啡" 3))
(test-equal "string-ref of CJK after an accented letter" #\咖
  (string-ref "Café 咖啡" 5))

(test-end)
