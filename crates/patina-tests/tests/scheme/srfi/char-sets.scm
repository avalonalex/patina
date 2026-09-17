;; SRFI 14 / (scheme charset) char-sets over the whole Unicode range.
;;
;; Larceny triage family 45 had "Ours: none yet"; this is it. The defect
;; (issue #372) was that the bundled implementation was Olin Shivers' Latin-1
;; reference, which stores a char-set as a 256-character string indexed by
;; code point. Two behaviours came out of that, and the rows below are
;; organised around them because they fail differently:
;;
;;   - a character above U+00FF *raised*, indexing past the string's end
;;   - a range above U+00FF was *silently clipped*, so `char-set:full` had 256
;;     members and a Hangul range was empty
;;
;; The second is the one a suite catches late, so it gets the most rows.
;;
;; Conformance is not this file's job: upstream's own 72-assertion suite runs
;; in `upstream_srfi_suites.rs`, and Larceny's 93-assertion `charset` suite
;; runs in the Larceny lane (93/93 on both backends since #372). What is here
;; is the Unicode property those two do not reach, the representation's own
;; invariants, and one headline row per form.
;;
;; Portability, since this file is also an oracle under chibi and Gauche: no
;; row asserts the *size* of a Unicode class. The implementations ship
;; different UCD versions and disagree on every one of them — measured
;; 2026-09-17, chibi has 2544 lower-case characters and Gauche 2155 — so a
;; cardinality row would be a divergence in three directions. The rows assert
;; membership of long-assigned characters, and relations between sets, both of
;; which are stable across UCD versions.

(import (scheme base) (scheme char) (scheme charset) (srfi 64))

(test-begin "char-sets")

;; ---------------------------------------------------------------------------
;; The defect: a character above U+00FF
;; ---------------------------------------------------------------------------

;; The repro from issue #372. Every one of these raised a `string-ref` index
;; error before the rewrite.
(test-assert "a Greek letter is a letter"
  (char-set-contains? char-set:letter (integer->char #x3BB)))       ; λ
(test-assert "a CJK ideograph is a letter"
  (char-set-contains? char-set:letter (integer->char #x4E2D)))      ; 中
(test-assert "a Hangul syllable is a letter"
  (char-set-contains? char-set:letter (integer->char #xAC00)))      ; 가
;; Deseret, assigned in Unicode 3.1. A newer letter would make this row a
;; UCD-version test rather than a representation test: U+10940 answers #f
;; under chibi 0.12 and Gauche 0.9.15, consistently with their own
;; `char-alphabetic?`, because their tables predate it.
(test-assert "a character beyond the BMP is a letter"
  (char-set-contains? char-set:letter (integer->char #x10400)))

;; `char-set` and `string->char-set` raised on the way *in*, which is a
;; different path from `char-set-contains?`: one writes the backing store and
;; the other reads it.
(test-assert "a char-set can be built from a character above U+00FF"
  (char-set-contains? (char-set (integer->char #x3BB))
                      (integer->char #x3BB)))
(test-equal "a one-character non-ASCII string makes a one-member set" 1
  (char-set-size (string->char-set "λ")))
(test-assert "list->char-set takes characters above U+00FF"
  (char-set-contains? (list->char-set (list (integer->char #x4E2D)))
                      (integer->char #x4E2D)))

;; SRFI 130's `string-index` and `string-skip` raised too, for the same
;; reason, whenever their char-set predicate met such a character. They are
;; not imported here — that library's own file covers them — but the
;; procedures they called are these two, which is why fixing SRFI 14 fixed
;; them without touching SRFI 130.

;; ---------------------------------------------------------------------------
;; The defect: a silently clipped range
;; ---------------------------------------------------------------------------

;; `ucs-range->char-set` clipped to the first 256 code points and said
;; nothing, so this was the empty set. The bound is exclusive.
(test-equal "a Hangul range has the width it was asked for" 16
  (char-set-size (ucs-range->char-set #xAC00 #xAC10)))
(test-assert "a clipped range's members are actually present"
  (char-set-contains? (ucs-range->char-set #xAC00 #xAC10)
                      (integer->char #xAC00)))
(test-assert "the exclusive upper bound is excluded"
  (not (char-set-contains? (ucs-range->char-set #xAC00 #xAC10)
                           (integer->char #xAC10))))
(test-equal "an empty range is empty" 0
  (char-set-size (ucs-range->char-set #x100 #x100)))

;; `char-set:full` reported 256. The universe is every code point except the
;; surrogates, which are not characters in any conforming implementation:
;; #x110000 - (#xE000 - #xD800) = 1112064. Larceny's suite asserts only
;; `>=` this number, which is why the Latin-1 implementation's 256 was the
;; mildest symptom the suite could have shown.
(test-assert "char-set:full holds every character"
  (>= (char-set-size char-set:full)
      (- #x110000 (- #xE000 #xD800))))
(test-assert "char-set:full contains a character above U+00FF"
  (char-set-contains? char-set:full (integer->char #x10FFFF)))
(test-equal "char-set:empty is empty" 0 (char-set-size char-set:empty))

;; ---------------------------------------------------------------------------
;; The classes agree with (scheme char)'s predicates
;; ---------------------------------------------------------------------------
;; The constants and the predicates are two views of the same Unicode data. If
;; they were built from different tables they would disagree on whatever one
;; version added, and each of these characters is a case where a Latin-1 or
;; ASCII-only class would answer wrongly.

(test-assert "char-set:letter agrees with char-alphabetic? on Greek"
  (eq? (char-set-contains? char-set:letter #\λ) (char-alphabetic? #\λ)))
(test-assert "char-set:lower-case agrees with char-lower-case? on Cyrillic"
  (eq? (char-set-contains? char-set:lower-case #\д)
       (char-lower-case? #\д)))
(test-assert "char-set:upper-case agrees with char-upper-case? on Cyrillic"
  (eq? (char-set-contains? char-set:upper-case #\Д)
       (char-upper-case? #\Д)))
(test-assert "char-set:whitespace agrees with char-whitespace? on NBSP-adjacent space"
  (eq? (char-set-contains? char-set:whitespace (integer->char #x2028))
       (char-whitespace? (integer->char #x2028))))
(test-assert "char-set:digit agrees with char-numeric? on Devanagari"
  (eq? (char-set-contains? char-set:digit (integer->char #x0966))
       (char-numeric? (integer->char #x0966))))

;; A Unicode decimal digit outside ASCII is in char-set:digit, which the
;; Latin-1 implementation could not represent at all.
(test-assert "a Devanagari digit is a digit"
  (char-set-contains? char-set:digit (integer->char #x0966)))
(test-assert "a superscript two is not a decimal digit"
  ;; Unicode No, not Nd. SRFI 14's digit class is Nd, as char-numeric? is.
  (not (char-set-contains? char-set:digit (integer->char #x00B2))))

;; `char-set:title-case` was empty in the Latin-1 implementation, which simply
;; said `(define char-set:title-case char-set:empty)`. Lt is small but real.
(test-assert "a titlecase letter is in char-set:title-case"
  (char-set-contains? char-set:title-case (integer->char #x01C5)))  ; ǅ
(test-assert "an uppercase letter is not titlecase"
  (not (char-set-contains? char-set:title-case #\A)))

;; ---------------------------------------------------------------------------
;; The classes that are fixed by definition rather than by a Unicode table
;; ---------------------------------------------------------------------------
;; These are the only classes whose exact membership is portable, so they are
;; the only ones asserted exactly.

(test-equal "char-set:ascii is the first 128 code points" 128
  (char-set-size char-set:ascii))
(test-equal "char-set:hex-digit is the 22 ASCII hex digits" 22
  (char-set-size char-set:hex-digit))
(test-assert "char-set:hex-digit is exactly the ASCII hex digits"
  (char-set= char-set:hex-digit (->char-set "0123456789abcdefABCDEF")))
(test-assert "a fullwidth digit is not a hex digit"
  (not (char-set-contains? char-set:hex-digit (integer->char #xFF10))))

;; ---------------------------------------------------------------------------
;; Relations between the classes
;; ---------------------------------------------------------------------------
;; Stable across UCD versions, unlike the cardinalities, and each one is a
;; statement the Latin-1 implementation could only make about Latin-1.

(test-assert "letter+digit is the union of letter and digit"
  (char-set= char-set:letter+digit
             (char-set-union char-set:letter char-set:digit)))
(test-assert "lower-case is a subset of letter"
  (char-set<= char-set:lower-case char-set:letter))
(test-assert "upper-case is a subset of letter"
  (char-set<= char-set:upper-case char-set:letter))
(test-assert "digit is a subset of letter+digit"
  (char-set<= char-set:digit char-set:letter+digit))
(test-assert "printing contains whitespace"
  (char-set<= char-set:whitespace char-set:printing))
(test-assert "graphic is a subset of printing"
  (char-set<= char-set:graphic char-set:printing))
(test-assert "every class is a subset of the universe"
  (and (char-set<= char-set:letter char-set:full)
       (char-set<= char-set:digit char-set:full)
       (char-set<= char-set:graphic char-set:full)))
(test-assert "iso-control and graphic are disjoint"
  (char-set= char-set:empty
             (char-set-intersection char-set:iso-control
                                    char-set:graphic)))

;; ---------------------------------------------------------------------------
;; Set algebra across the whole range
;; ---------------------------------------------------------------------------
;; The reference implementation's algebra was a loop over 256 string indices.
;; These rows put each operation astride the Latin-1 boundary, where an
;; implementation that still thought in 256s would answer wrongly rather than
;; raise.

(define high-a (integer->char #x3BB))     ; λ
(define high-b (integer->char #x4E2D))    ; 中
(define mixed (char-set #\a high-a high-b))

(test-equal "a mixed set counts every member" 3 (char-set-size mixed))
(test-assert "union spans the boundary"
  (char-set= (char-set-union (char-set #\a) (char-set high-a))
             (char-set #\a high-a)))
(test-assert "intersection spans the boundary"
  (char-set= (char-set-intersection mixed (char-set high-a #\z))
             (char-set high-a)))
(test-assert "difference spans the boundary"
  (char-set= (char-set-difference mixed (char-set #\a))
             (char-set high-a high-b)))
(test-assert "xor spans the boundary"
  (char-set= (char-set-xor (char-set #\a high-a) (char-set high-a high-b))
             (char-set #\a high-b)))
(test-assert "adjoin takes a character above U+00FF"
  (char-set-contains? (char-set-adjoin (char-set #\a) high-b) high-b))
(test-assert "delete removes a character above U+00FF"
  (not (char-set-contains? (char-set-delete mixed high-b) high-b)))

;; The complement is taken within the universe, not within Latin-1, so it is
;; enormous — and the size arithmetic is what catches an off-by-one in the
;; surrogate hole.
(test-equal "the complement of a singleton is the universe less one"
  (- (char-set-size char-set:full) 1)
  (char-set-size (char-set-complement (char-set #\a))))
(test-assert "a character above U+00FF is in the complement of an ASCII set"
  (char-set-contains? (char-set-complement (->char-set "abc")) high-a))
(test-assert "complementing twice is the identity"
  (char-set= mixed (char-set-complement (char-set-complement mixed))))
(test-assert "a set and its complement are disjoint"
  (char-set= char-set:empty
             (char-set-intersection mixed (char-set-complement mixed))))
(test-assert "a set and its complement cover the universe"
  (char-set= char-set:full
             (char-set-union mixed (char-set-complement mixed))))

(test-assert "diff+intersection splits a set across the boundary"
  (call-with-values
      (lambda () (char-set-diff+intersection mixed (char-set high-a)))
    (lambda (d i)
      (and (char-set= d (char-set #\a high-b))
           (char-set= i (char-set high-a))))))

;; ---------------------------------------------------------------------------
;; Iteration reaches characters above U+00FF
;; ---------------------------------------------------------------------------

(test-equal "fold visits every member" 3
  (char-set-fold (lambda (c n) (+ n 1)) 0 mixed))
(test-assert "->list round-trips through list->char-set"
  (char-set= mixed (list->char-set (char-set->list mixed))))
(test-assert "->string round-trips through string->char-set"
  (char-set= mixed (string->char-set (char-set->string mixed))))
(test-equal "for-each visits every member" 3
  (let ((n 0))
    (char-set-for-each (lambda (c) (set! n (+ n 1))) mixed)
    n))
(test-assert "map can map across the boundary"
  (char-set= (char-set-map char-upcase (char-set high-a))
             (char-set (char-upcase high-a))))
(test-equal "count applies its predicate to every member" 2
  (char-set-count (lambda (c) (> (char->integer c) #xFF)) mixed))
(test-assert "every sees only members"
  (char-set-every (lambda (c) (char-set-contains? mixed c)) mixed))
(test-assert "any finds a member above U+00FF"
  (char-set-any (lambda (c) (> (char->integer c) #xFF)) mixed))
(test-assert "any is false on a set with no such member"
  (not (char-set-any (lambda (c) (> (char->integer c) #xFF))
                     (->char-set "abc"))))

;; A cursor walk must reach every member, including those above U+00FF. The
;; order SRFI 14 leaves unspecified; both references walk ascending, and
;; Larceny's suite pins that accumulation, so this file asserts the *set* the
;; walk produces rather than the order, and leaves the order to that suite.
(test-assert "a cursor walk reaches every member"
  (char-set= mixed
             (list->char-set
              (let loop ((cur (char-set-cursor mixed)) (acc '()))
                (if (end-of-char-set? cur)
                    acc
                    (loop (char-set-cursor-next mixed cur)
                          (cons (char-set-ref mixed cur) acc)))))))
(test-assert "a cursor walk over the empty set ends immediately"
  (end-of-char-set? (char-set-cursor char-set:empty)))

;; ---------------------------------------------------------------------------
;; Filter and unfold over a Unicode domain
;; ---------------------------------------------------------------------------

(test-assert "filter selects from a domain above U+00FF"
  (char-set= (char-set-filter (lambda (c) (> (char->integer c) #xFF)) mixed)
             (char-set high-a high-b)))
(test-assert "unfold builds a set of characters above U+00FF"
  (char-set= (char-set-unfold null? car cdr (list high-a high-b))
             (char-set high-a high-b)))
(test-assert "unfold's base set is included"
  (char-set= (char-set-unfold null? car cdr (list high-a) (char-set #\a))
             (char-set #\a high-a)))

;; ---------------------------------------------------------------------------
;; Equality, subset and hash
;; ---------------------------------------------------------------------------

(test-assert "sets built in different orders are equal"
  (char-set= (char-set #\a high-a high-b) (char-set high-b #\a high-a)))
(test-assert "a duplicate character does not change a set"
  (char-set= (char-set high-a) (char-set high-a high-a)))
(test-assert "equal sets hash alike"
  (= (char-set-hash (char-set #\a high-a high-b))
     (char-set-hash (char-set high-b high-a #\a))))
(test-assert "a hash respects its bound"
  (let ((h (char-set-hash char-set:letter 100)))
    (and (<= 0 h) (< h 100))))
(test-assert "hashing the universe respects its bound"
  ;; The Latin-1 implementation hashed by walking 256 indices; this one walks
  ;; ranges, so the universe hashes in two steps rather than 1112064.
  (let ((h (char-set-hash char-set:full 1000)))
    (and (<= 0 h) (< h 1000))))
(test-assert "a set is a subset of itself"
  (char-set<= mixed mixed))
(test-assert "a proper subset is not equal"
  (not (char-set= (char-set high-a) mixed)))

;; ---------------------------------------------------------------------------
;; Linear-update procedures
;; ---------------------------------------------------------------------------
;; SRFI 14 allows these to reuse their first argument. What must hold is the
;; returned value, and that a fresh copy is unaffected.

(test-assert "adjoin! adds a character above U+00FF"
  (char-set-contains? (char-set-adjoin! (char-set #\a) high-b) high-b))
(test-assert "delete! removes a character above U+00FF"
  (not (char-set-contains? (char-set-delete! (char-set #\a high-b) high-b)
                           high-b)))
(test-assert "union! spans the boundary"
  (char-set= (char-set-union! (char-set #\a) (char-set high-a))
             (char-set #\a high-a)))
(test-assert "complement! is taken within the universe"
  (char-set-contains? (char-set-complement! (->char-set "abc")) high-a))
(test-assert "ucs-range->char-set! extends its base set"
  (char-set= (ucs-range->char-set! #xAC00 #xAC02 #f (char-set #\a))
             (char-set #\a (integer->char #xAC00) (integer->char #xAC01))))
(test-assert "copying leaves the original alone"
  (let* ((original (char-set #\a high-a))
         (copy (char-set-copy original)))
    (char-set-adjoin! copy high-b)
    (and (char-set= original (char-set #\a high-a))
         (char-set-contains? copy high-b))))

;; ---------------------------------------------------------------------------
;; ->char-set and the type predicate
;; ---------------------------------------------------------------------------

(test-assert "->char-set accepts a char-set" (char-set? (->char-set mixed)))
(test-assert "->char-set accepts a string above U+00FF"
  (char-set-contains? (->char-set "λ") high-a))
(test-assert "->char-set accepts a character above U+00FF"
  (char-set-contains? (->char-set high-a) high-a))
(test-assert "a string is not a char-set" (not (char-set? "abc")))
(test-assert "a number is not a char-set" (not (char-set? 37)))

(test-end)
