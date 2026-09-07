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
;; **59 rows**: 43 migrated value rows, 12 error rows, and 4 added — `-0.0`, the
;; two `write`/`number->string` agreement rows, and a split of the complex row
;; so the half an oracle can corroborate is not scoped away with the half it
;; cannot.
;;
;; **What the move costs, precisely.** `assert_eval_to` compared the datum
;; writer's output. For the `number->string` rows nothing is lost — the
;; procedure *returns* a string, so comparing strings is comparing the printed
;; form. For the `string->number` rows something is: `(test-equal '(1/2 …) …)`
;; compares with `equal?`, so a writer regression that leaves values intact —
;; the #187/#189 class — is no longer caught here. That is the standing cost
;; `docs/TEST_ORGANIZATION.md` records for the whole migration, and this file is
;; not exempt from it; `equal?` does still reduce to `eqv?` for numbers, so
;; `100` and `100.0` stay distinct.
;;
;; The error rows lose something too: `assert_eval_error` required both backends
;; to fail at the same `ErrorClass`, and `test-error` cannot say that. The
;; driver compares the two backends' count vectors, so a divergence in *which*
;; rows failed would still surface — a divergence in the stage would not.
;;
;; Oracles, measured 2026-09-07. Three rows are scoped to Patina and report a
;; *skip* elsewhere — complex-part elision, complex radix, and radix strictness
;; — each because R7RS leaves the answer to the implementation.
;;
;;   patina VM / tree-walker   58 pass, 1 expected failure (the defect below)
;;   Gauche                    55 pass, 3 skip, 1 fail — it has no exact complex
;;                             numbers, so "3+4i" prints as "3.0+4.0i"
;;   chibi                     54 pass, 3 skip, 2 fail — it alone tolerates a
;;                             third argument to either procedure
;;
;; The two oracle failures are each one implementation against the other three,
;; and are noted where those rows sit. Neither is scoped away: doing so would
;; also drop the agreement of the oracle that *does* corroborate the row.

(import (scheme base) (scheme write) (srfi 64))

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

;; Negative zero keeps its sign, and reads back as itself. Portable — all four
;; agree, `write` and `number->string` included. (`debug_format.rs:81` and
;; `conversion.rs:199` both say `-0.0` is a case `number->string` handles that
;; the display path does not; measured, that is not true — `(write -0.0)` is
;; `-0.0` on both backends. The comments are stale.)
(test-equal "negative zero keeps its sign, both ways" '("-0.0" "0.0" -0.0)
  (list (number->string -0.0) (number->string 0.0) (string->number "-0.0")))

;; The other half of that stale comment names scientific notation, and there the
;; two paths really do disagree — which is a defect, not a property to pin:
;;
;;   (write 1e21)            1000000000000000000000.0
;;   (number->string 1e21)   "1.0e+21"
;;
;; One number, two external representations, on both backends. chibi is
;; self-consistent (`1e+21` either way) and so is Gauche (`1.0e21`). R7RS §6.2.6
;; requires only that the result read back, which both spellings do, so this is
;; a quality defect rather than a conformance one — but a program that writes a
;; number and one that converts it should not disagree.
;;
;; `(scheme write)` is in this file's import set for this row alone. It resolves
;; without one on Patina — the top level carries `(scheme base)`, and `write` is
;; exported from there (issue #211) — so the omission is invisible here and
;; fails on both oracles. Third time in this migration; see #211.
;;
;; Asserted as the property that *should* hold, with the failure expected on
;; Patina only, so the row retires itself when the defect is fixed: an xpass is
;; something `scheme_suite.rs` reports. Both oracles pass it today.
(define (written x) (let ((p (open-output-string))) (write x p) (get-output-string p)))
(cond-expand (patina (test-expect-fail 1)) (else))
(test-equal "write and number->string agree on a number needing an exponent" #t
  (string=? (written 1e21) (number->string 1e21)))
(test-equal "…and on one that does not" #t
  (string=? (written 3.14) (number->string 3.14)))

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
;; Unscoped: chibi agrees with both of these, and scoping would throw that away
;; — the mistake #212 was corrected for. Gauche alone differs, because it has no
;; exact complex numbers, so it answers "3.0+4.0i" to both.
(test-equal "a complex number keeps the exactness of its parts" '("3+4i" "3.0+4.0i")
  (list (number->string 3+4i) (number->string 3.0+4.0i)))

;; Scoped: here all three part company, so there is no corroboration to lose.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "the parts we elide" '("+i" "-i" "5")
  (list (number->string 0+1i)      ; a zero real part is elided
        (number->string 0-1i)
        (number->string 5+0i)))    ; an exact zero imaginary part is elided

(cond-expand (patina) (else (test-skip 1)))
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
;; `error-object?` rather than a catch-all: with six claims sharing one row, a
;; thunk that failed for an unrelated reason — a typo, a later arity change —
;; would still answer `raises` and hide the degraded element.
(define (raises? thunk)
  (guard (e ((error-object? e) 'raises)) (thunk) 'no-error))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "radix rules R7RS leaves to the implementation are enforced"
  '(raises raises raises raises raises raises)
  (list (raises? (lambda () (number->string 100 3)))     ; outside 2/8/10/16
        (raises? (lambda () (number->string 100 17)))
        (raises? (lambda () (string->number "100" 3)))
        (raises? (lambda () (string->number "100" 17)))
        (raises? (lambda () (number->string 3.14 16)))   ; inexact wants radix 10
        (raises? (lambda () (number->string 100.0 2)))))

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

(test-equal "string->number: decimal is the default radix" '(100 0 -42)
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

(test-equal "string->number: inexact reals" '(3.14 100.0 100.0 -2.5)
  (list (string->number "3.14") (string->number "1e2")
        (string->number "1E2") (string->number "-2.5")))

(test-equal "exponent notation" '(100.0 150.0 0.2)
  (list (string->number "1e2") (string->number "1.5e2") (string->number "2e-1")))

(test-equal "string->number: exact rationals" '(1/2 3/4 -2/3)
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
;; number. This row used to assert the trimmed answer.
;;
;; chibi tolerates *leading* whitespace, but nothing here shows that: both
;; probes also have trailing padding, which chibi rejects, so it answers #f like
;; everyone else. Isolating it would take `(string->number " 100")` and a scope,
;; and the row is about our answer rather than about chibi's.
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
