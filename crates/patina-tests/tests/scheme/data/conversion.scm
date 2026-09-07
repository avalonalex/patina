;; `number->string` and `string->number` — R7RS §6.2.6.
;;
;; Migrated whole from `crates/patina-tests/tests/conversion.rs` (#193 Phase 1):
;; 51 `#[test]` functions, 123 assertions, no Rust machinery.
;;
;; **How the rows map.** The `.rs` file grouped several assertions per test —
;; three radices, three sign cases — and each of those groups is one row here,
;; comparing a list. That keeps the granularity the file chose, and a failure
;; shows the whole group at once rather than stopping at the first. The 17
;; error assertions become one `test-error` each, because `test-error` takes a
;; single expression — except the six that pin Patina's *strictness* about
;; radices, which are one scoped row together, for the reason recorded there.
;; 44 value rows + 12 error rows = **56**, the extra being a `-0.0` row the
;; `.rs` file did not have.
;;
;; **Nothing is weakened by the move**, which is worth saying because it is not
;; true of every migrated file: `assert_eval_to` compared the datum writer's
;; output, and half this file is *about* how numbers print — but
;; `number->string` returns a string, so comparing strings is comparing the
;; printed form. On the `string->number` side `equal?` reduces to `eqv?` for
;; numbers, which distinguishes exactness, so `100` and `100.0` stay distinct.
;;
;; Oracles, measured 2026-09-07. Three rows are scoped to Patina and report a
;; *skip* elsewhere — the two about complex printing and the one about radix
;; strictness, each because R7RS leaves the answer to the implementation.
;;
;;   patina VM / tree-walker   56 pass
;;   Gauche                    53 pass, 3 skip, no failures
;;   chibi                     51 pass, 3 skip, 2 fail — it alone tolerates a
;;                             third argument to either procedure, noted where
;;                             those two rows sit.

(import (scheme base) (srfi 64))

(test-begin "conversion")

;; ── number->string: radices ─────────────────────────────────────────────────

(test-equal "decimal is the default radix" '("100" "0" "-42")
  (list (number->string 100) (number->string 0) (number->string -42)))

(test-equal "radix 2" '("1100" "11111111" "0")
  (list (number->string 12 2) (number->string 255 2) (number->string 0 2)))

(test-equal "radix 8" '("100" "377" "10")
  (list (number->string 64 8) (number->string 255 8) (number->string 8 8)))

(test-equal "radix 16" '("ff" "100" "fff")
  (list (number->string 255 16) (number->string 256 16) (number->string 4095 16)))

(test-equal "a negative number keeps its sign in every radix" '("-10" "-ff" "-1100")
  (list (number->string -10) (number->string -255 16) (number->string -12 2)))

(test-equal "zero in every radix" '("0" "0" "0" "0")
  (list (number->string 0 2) (number->string 0 8)
        (number->string 0 10) (number->string 0 16)))

;; ── number->string: the numeric tower ───────────────────────────────────────

(test-equal "inexact reals" '("3.14" "100.0" "-2.5")
  (list (number->string 3.14) (number->string 100.0) (number->string -2.5)))

;; Negative zero keeps its sign, and reads back as itself. `debug_format.rs`
;; names `-0.0` as one of the two cases `number->string` handles that the
;; display path does not, and no row covered it. Portable — all four agree.
(test-equal "negative zero keeps its sign, both ways" '("-0.0" "0.0" -0.0)
  (list (number->string -0.0) (number->string 0.0) (string->number "-0.0")))

;; The infinities and NaN have written forms R7RS §6.2.5 fixes exactly, so
;; these are the one place in the file where the spelling is required rather
;; than merely ours.
(test-equal "the infinities and NaN" '("+inf.0" "-inf.0" "+nan.0")
  (list (number->string (/ 1.0 0.0))
        (number->string (/ -1.0 0.0))
        (number->string (/ 0.0 0.0))))

(test-equal "exact rationals" '("1/2" "3/4" "-2/3")
  (list (number->string 1/2) (number->string 3/4) (number->string -2/3)))

(test-equal "exact rationals in another radix" '("f/10" "111/1000")
  (list (number->string 15/16 16) (number->string 7/8 2)))

(test-equal "integers past a machine word" '("99999999999999999999" "56bc75e2d630fffff")
  (list (number->string 99999999999999999999)
        (number->string 99999999999999999999 16)))

;; ── number->string: complex ─────────────────────────────────────────────────
;;
;; **Scoped to Patina, because the answers are ours.** Complex printing is where
;; the three implementations part company, measured 2026-09-07:
;;
;;   (number->string 0+1i)      patina "+i"     chibi "0+i"   gauche "0.0+1.0i"
;;   (number->string 3+4i)      patina "3+4i"   chibi "3+4i"  gauche "3.0+4.0i"
;;   (number->string 3+4i 16)   patina raises   chibi "3+4i"  gauche "3.0+4.0i"
;;
;; Gauche has no exact complex numbers at all, so every rectangular literal
;; arrives inexact there; chibi keeps exactness but does not elide a zero real
;; part. Both are defensible — R7RS §6.2.6 requires only that the result parse
;; back — so these rows pin Patina's spelling rather than a portable one, and
;; the others report a skip instead of a difference.
(cond-expand (patina) (else (test-skip "complex numbers, and the parts we elide")))
(test-equal "complex numbers, and the parts we elide" '("3+4i" "3.0+4.0i" "+i" "-i" "5")
  (list (number->string 3+4i)
        (number->string 3.0+4.0i)
        (number->string 0+1i)      ; a zero real part is elided
        (number->string 0-1i)
        (number->string 5+0i)))    ; an exact zero imaginary part is elided

(cond-expand (patina) (else (test-skip "a complex number has no non-decimal radix")))
(test-error "a complex number has no non-decimal radix" #t (number->string 3+4i 16))

;; ── number->string: what it refuses ─────────────────────────────────────────

;; **Scoped to Patina: R7RS makes these "an error", which does not require
;; signalling.** §6.2.6 says it is an error if radix is not one of 2, 8, 10 or
;; 16, and that an inexact number's radix is 10. Patina signals; chibi and
;; Gauche instead *extend*, and measured 2026-09-07 they agree with each other:
;;
;;   (number->string 100 3)     patina raises   chibi/gauche "10201"
;;   (number->string 100 17)    patina raises   chibi/gauche "5f"
;;   (number->string 3.14 16)   patina raises   chibi "#i191eb851eb851f/8000000000000",
;;                                              gauche "3.14" (radix ignored)
;;
;; All three conform. Scoped rather than left failing because there is no
;; corroboration to lose — both oracles differ — unlike the arity rows below.
(define (raises? thunk) (guard (e (#t 'raises)) (thunk) 'no-error))

(cond-expand (patina) (else (test-skip "a radix outside R7RS's four is rejected")))
(test-equal "a radix outside R7RS's four is rejected"
  '(raises raises raises raises raises)
  (list (raises? (lambda () (number->string 100 3)))
        (raises? (lambda () (number->string 100 17)))
        (raises? (lambda () (number->string 3.14 16)))
        (raises? (lambda () (number->string 100.0 2)))
        (raises? (lambda () (string->number "100" 3)))))

;; Radix 0 is the one all three reject, so it needs no scoping.
(test-error "radix 0 is not allowed" #t (number->string 100 0))

(test-error "a string is not a number" #t (number->string "not a number"))
(test-error "a boolean is not a number" #t (number->string #t))
(test-error "the empty list is not a number" #t (number->string '()))

(test-error "number->string needs an argument" #t (number->string))

;; **chibi does not signal here**, and is the only one that does not: it ignores
;; the extra argument and answers "1", where Patina and Gauche both raise. Kept
;; unscoped for that reason — `cond-expand (patina)` would drop Gauche's
;; agreement, which is the more useful half. chibi reports this row and its
;; `string->number` twin as failures; they are the only two it fails.
(test-error "number->string takes at most two" #t (number->string 1 2 3))

;; ── string->number: radices ─────────────────────────────────────────────────

(test-equal "decimal is the default radix" '(100 0 -42)
  (list (string->number "100") (string->number "0") (string->number "-42")))

(test-equal "an explicit radix argument" '(256 12 64)
  (list (string->number "100" 16) (string->number "1100" 2) (string->number "100" 8)))

(test-equal "hexadecimal, either case" '(255 255 256)
  (list (string->number "ff" 16) (string->number "FF" 16) (string->number "100" 16)))

(test-equal "binary" '(12 255)
  (list (string->number "1100" 2) (string->number "11111111" 2)))

(test-equal "octal" '(64 255)
  (list (string->number "100" 8) (string->number "377" 8)))

;; ── string->number: the numeric tower ───────────────────────────────────────

(test-equal "inexact reals" '(3.14 100.0 100.0 -2.5)
  (list (string->number "3.14") (string->number "1e2")
        (string->number "1E2") (string->number "-2.5")))

(test-equal "exponent notation" '(100.0 150.0 0.2)
  (list (string->number "1e2") (string->number "1.5e2") (string->number "2e-1")))

(test-equal "exact rationals" '(1/2 3/4 -2/3)
  (list (string->number "1/2") (string->number "3/4") (string->number "-2/3")))

(test-equal "an integer past a machine word" 99999999999999999999
  (string->number "99999999999999999999"))

(test-equal "zero, in three spellings" '(0 0.0 0)
  (list (string->number "0") (string->number "0.0") (string->number "#b0")))

(test-equal "negatives across the tower" '(-100 -3.14 -1/2)
  (list (string->number "-100") (string->number "-3.14") (string->number "-1/2")))

;; ── string->number: prefixes ────────────────────────────────────────────────

(test-equal "a radix prefix in the string" '(255 255 12 64 100)
  (list (string->number "#xff") (string->number "#xFF") (string->number "#b1100")
        (string->number "#o100") (string->number "#d100")))

;; R7RS §6.2.6: the radix argument is a *default*, so a prefix wins.
(test-equal "a prefix overrides the radix argument" '(255 12)
  (list (string->number "#xff" 2) (string->number "#b1100" 16)))

(test-equal "an exactness prefix" '(100 100.0 3/2)
  (list (string->number "#e100") (string->number "#i100") (string->number "#e1.5")))

(test-equal "both prefixes, in either order" '(255.0 255.0 16)
  (list (string->number "#x#iff") (string->number "#i#xff") (string->number "#e#x10")))

(test-equal "prefixes are case-insensitive" '(255 255 12)
  (list (string->number "#XFF") (string->number "#xFf") (string->number "#B1100")))

;; ── string->number: what is not a number ────────────────────────────────────
;;
;; Not an error — R7RS §6.2.6 says `string->number` returns #f when the string
;; is not the syntax of a number in the given radix.

(test-equal "a string that is not a number is #f" '(#f #f #f #f #f)
  (list (string->number "not a number") (string->number "")
        (string->number "abc") (string->number "12 34") (string->number "1 2")))

;; Whitespace is not number syntax (R7RS §6.2.7), so a padded string is not a
;; number. Gauche and Chez answer #f; chibi tolerates *leading* whitespace only.
;; This row used to assert the trimmed answer.
(test-equal "padding is not part of number syntax" '(#f #f 100)
  (list (string->number "  100  ") (string->number "\t42\n") (string->number "100")))

;; ── string->number: what it refuses ─────────────────────────────────────────

;; The `string->number` half of the radix group above is folded into that row.

(test-error "a number is not a string" #t (string->number 100))
(test-error "a boolean is not a string" #t (string->number #t))

(test-error "string->number needs an argument" #t (string->number))
(test-error "string->number takes at most two" #t (string->number "1" 2 3))

;; ── Round trips ─────────────────────────────────────────────────────────────
;;
;; R7RS §6.2.6 requires `(string->number (number->string z radix) radix)` to
;; give back a number `eqv?` to `z` — for exact z, and for inexact z whose
;; precision survives. These say so directly, which no pair of one-way rows
;; above can: both directions could be wrong in the same way and still agree.

(test-equal "an integer round-trips" #t
  (let ((n 12345)) (equal? n (string->number (number->string n)))))
(test-equal "a negative integer round-trips" #t
  (let ((n -42)) (equal? n (string->number (number->string n)))))
(test-equal "an inexact real round-trips" #t
  (let ((n 3.14)) (equal? n (string->number (number->string n)))))
(test-equal "an exact rational round-trips" #t
  (let ((n 3/4)) (equal? n (string->number (number->string n)))))
(test-equal "a round trip through radix 2" #t
  (let ((n 12)) (equal? n (string->number (number->string n 2) 2))))
(test-equal "a round trip through radix 16" #t
  (let ((n 255)) (equal? n (string->number (number->string n 16) 16))))
(test-equal "a larger round trip through radix 16" #t
  (let ((n 4095)) (equal? n (string->number (number->string n 16) 16))))

;; ── In ordinary use ─────────────────────────────────────────────────────────

(test-equal "the result of string->number is an ordinary number" 30
  (+ (string->number "10") (string->number "20")))

(test-equal "number->string as a procedure argument" '("1" "2" "3" "4" "5")
  (map number->string '(1 2 3 4 5)))

(define (to-hex n) (number->string n 16))
(test-equal "…including one that closes over a radix" '("a" "b" "c" "d" "e" "f")
  (map to-hex '(10 11 12 13 14 15)))

(test-equal "the predicates agree with the result" '(#t #t)
  (list (number? (string->number "123"))
        (not (number? (string->number "abc")))))

(test-equal "one number, four radices" '("11111111" "377" "255" "ff")
  (list (number->string 255 2) (number->string 255 8)
        (number->string 255 10) (number->string 255 16)))

;; Exactness survives the round trip in both directions — the property `equal?`
;; on numbers is sensitive to, and the reason the rows above can use it.
(test-equal "exactness is carried by the syntax, not lost" '(#t #t)
  (list (exact? (string->number "100"))
        (inexact? (string->number "100.0"))))

(test-end)
