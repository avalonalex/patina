;; SRFI 151 bitwise operations, plus the legacy spellings the ecosystem
;; actually imports: SRFI 60, SRFI 33 and SRFI 142.
;;
;; Migrated from `crates/patina-tests/tests/srfi_151_bitwise.rs` (#193), which
;; is deleted. Upstream's own 151 suite runs in `upstream_srfi_suites.rs`; the
;; three legacy libraries have no suite there, because they are renames over
;; 151, so their rows below are the only tests they have.
;;
;; Bundled because it is the largest measured gap in the vendored corpus:
;; `(srfi 60)` has an in-degree of 31 and `(srfi 33)` 19, against SRFI 151's
;; standard-track status; `(srfi 142)` is 151's own withdrawn predecessor.
;; All four are the same operations under different sets of names.
;;
;; The core operators are Rust primitives. Two's-complement semantics over
;; unbounded integers is the part worth testing: negative operands behave as if
;; sign-extended infinitely to the left, and bignums must behave like fixnums.
;;
;; ── Which oracle can run which rows ─────────────────────────────────────────
;;
;; chibi 0.12 ships (srfi 33) and (srfi 142) but not (srfi 60); Gauche 0.9.15
;; ships (srfi 60) but neither of the others. Each legacy library is imported
;; with a prefix, inside a `cond-expand` that names the implementations known
;; to have it, and each of its rows is skipped where it was not imported. The
;; skipped rows still mention the prefixed names, so chibi warns about unbound
;; `srfi-60:` references while compiling; they are never evaluated.
;;
;; The requirement ought to be `(library (srfi 60))`, which is what R7RS
;; §4.2.1 provides for this. It is not used because Patina's program-level
;; `cond-expand` answers every library requirement false (#265).
;;
;; The SRFI 33 rows found a Patina defect on arrival: `bitwise-merge` and
;; `copy-bit-field` used SRFI 151's argument order, and chibi answered both
;; rows differently. SRFI 33's text sides with chibi, and #264 fixed the shim.

(import (scheme base) (srfi 151)
        (prefix (scheme bitwise) scheme-bitwise:)
        (srfi 64))
(cond-expand
  ((or patina gauche) (import (prefix (srfi 60) srfi-60:)))
  (else))
(cond-expand
  ((or patina chibi)
   (import (prefix (srfi 33) srfi-33:) (prefix (srfi 142) srfi-142:)))
  (else))

(test-begin "bitwise")

;; ─── Core operators ──────────────────────────────────────────────────────────

(test-equal "bitwise-and" 8 (bitwise-and 12 10))
(test-equal "bitwise-ior" 14 (bitwise-ior 12 10))
(test-equal "bitwise-xor" 6 (bitwise-xor 12 10))
(test-equal "bitwise-not" -13 (bitwise-not 12))

;; The operators are n-ary, and the empty case returns the identity SRFI 151
;; specifies — which differs per operator and is easy to get backwards.
(test-equal "n-ary bitwise-and" 15 (bitwise-and 255 63 15))
(test-equal "n-ary bitwise-ior" 15 (bitwise-ior 1 2 4 8))
(test-equal "n-ary bitwise-xor" 7 (bitwise-xor 1 2 4))
(test-equal "the identities of and, ior and xor" '(-1 0 0)
  (list (bitwise-and) (bitwise-ior) (bitwise-xor)))
(test-equal "one argument is returned unchanged" 42 (bitwise-and 42))

;; Negative operands are sign-extended, so a right shift is arithmetic and
;; `bit-count` counts *zero* bits to stay finite.
(test-equal "a right shift of a negative number is arithmetic" -2
  (arithmetic-shift -8 -2))
(test-equal "a left shift of -1" -16 (arithmetic-shift -1 4))
(test-equal "-1 has every bit set" 12 (bitwise-and -1 12))
(test-equal "bit-count of a negative number counts zero bits" '(0 1)
  (list (bit-count -1) (bit-count -2)))
(test-equal "integer-length of -1 and 0" '(0 0)
  (list (integer-length -1) (integer-length 0)))

;; Must behave identically past the fixnum boundary. The primitives take an
;; allocation-free i64 path when every operand is a fixnum and promote to
;; bignums otherwise, so these pin the slow path.
(test-equal "bitwise-and on bignums" (expt 2 100)
  (bitwise-and (expt 2 100) (- (expt 2 101) 1)))
(test-equal "integer-length of a bignum" 101 (integer-length (expt 2 100)))
(test-equal "bit-count of a bignum" 64 (bit-count (- (expt 2 64) 1)))
(test-equal "a left shift into a bignum" 1267650600228229401496703205376
  (arithmetic-shift 1 100))
(test-equal "a right shift back below the boundary" 1
  (arithmetic-shift (expt 2 100) -100))

(test-assert "bit-set? on a set bit" (bit-set? 2 12))
(test-assert "bit-set? on a clear bit" (not (bit-set? 0 12)))
(test-equal "first-set-bit" 3 (first-set-bit 8))
(test-equal "first-set-bit of zero" -1 (first-set-bit 0))
(test-equal "bit-field" 10 (bit-field #b1101101010 0 4))
(test-assert "any-bit-set?" (any-bit-set? 12 10))
(test-assert "every-bit-set?" (every-bit-set? 4 6))
(test-equal "copy-bit" 1 (copy-bit 0 0 #t))

(test-equal "bitwise-nand" -9 (bitwise-nand 12 10))
(test-equal "bitwise-nor" -15 (bitwise-nor 12 10))
(test-equal "bitwise-andc1" 2 (bitwise-andc1 12 10))
(test-equal "bitwise-andc2" 4 (bitwise-andc2 12 10))
;; SRFI 151's `bitwise-if` takes a bit from its second argument where the
;; mask bit is 1. mask 5 (101), 3 (011), 0: bits 0 and 2 from 3, so 1.
(test-equal "bitwise-if" 9 (bitwise-if 3 1 8))
(test-equal "bitwise-if takes mask-1 bits from the second argument" 1
  (bitwise-if 5 3 0))

;; N-ary eqv must fold pairwise. Complementing a seeded xor once at the end
;; coincidentally agrees for odd argument counts and is wrong for every even
;; one, so even arities and the zero-argument identity are the cases that bite.
(test-equal "bitwise-eqv, the SRFI 151 example" -42 (bitwise-eqv 37 12))
(test-equal "bitwise-eqv of two" -7 (bitwise-eqv 12 10))
(test-equal "bitwise-eqv of none and of one" '(-1 42)
  (list (bitwise-eqv) (bitwise-eqv 42)))
(test-equal "bitwise-eqv of three" 7 (bitwise-eqv 1 2 4))
(test-equal "bitwise-eqv of four folds pairwise" -16 (bitwise-eqv 1 2 4 8))

(test-equal "bits->list is LSB-first" '(#f #t #t) (bits->list 6))
(test-equal "list->bits" 6 (list->bits '(#f #t #t)))
(test-equal "bits->list round-trips" 12345 (list->bits (bits->list 12345)))
(test-equal "bits" 6 (bits #f #t #t))

(test-equal "bitwise-fold" 3
  (bitwise-fold (lambda (b acc) (if b (+ acc 1) acc)) 0 #b1011))

;; R7RS-large names this `(scheme bitwise)`. Imported with a prefix, so the
;; row holds whether or not an implementation's two names share bindings.
(test-equal "(scheme bitwise) is the same library" '(8 1024)
  (list (scheme-bitwise:bitwise-and 12 10)
        (scheme-bitwise:arithmetic-shift 1 10)))

;; ─── Absurd shift counts ─────────────────────────────────────────────────────

;; Audit item A6 (`PRD/ARCHIVE/AUDIT_2026_08_10_PRD.md`): an absurd left-shift
;; count must raise a catchable Scheme error. num-bigint allocates the result
;; up front, so unguarded this was a ~137 GB allocation — a process abort
;; rather than an error. Scoped to Patina: refusing is an implementation
;; restriction (R7RS §1.3.2), and an implementation with the memory to
;; compute the answer would not be wrong.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an absurd fixnum shift count is a catchable error" 'caught
  (guard (e (#t 'caught)) (arithmetic-shift 1 (expt 2 40))))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an absurd bignum shift count is a catchable error" 'caught
  (guard (e (#t 'caught)) (arithmetic-shift 1 (expt 2 80))))
;; Generous-but-sane counts still work, and huge right shifts are harmless.
(test-equal "a large but sane left shift" 100001
  (integer-length (arithmetic-shift 1 100000)))
(test-equal "a huge right shift drains to zero" 0
  (arithmetic-shift 123 (- (expt 2 40))))

;; ─── The fixnum/bignum seam ──────────────────────────────────────────────────

;; Pin the seam between the fixnum fast path and the bignum path, which on
;; Patina is at ±2^60 (61-bit fixnums). The fast path is sound because
;; and/ior/xor/not are closed over the sign-extended fixnum range and shifts
;; re-check narrowing — but nothing else here crosses the boundary, so an edit
;; to either half (or to `fits_fixnum`) would otherwise pass every row. Values
;; straddle the seam deliberately: fixnum operands, boundary operands, and
;; results that must promote. Elsewhere these are ordinary integer rows whose
;; answers do not depend on where the seam is.
(define greatest 1152921504606846975)   ; Patina's fx-greatest, 2^60 - 1
(define least -1152921504606846976)     ; Patina's fx-least, -2^60

;; Closure of the fast path at the extremes.
(test-equal "and at the least fixnum" least (bitwise-and least -1))
(test-equal "not of the least fixnum" greatest (bitwise-not least))
(test-equal "not of the greatest fixnum" least (bitwise-not greatest))
(test-equal "ior of the extremes" -1 (bitwise-ior greatest least))
(test-equal "xor of the extremes" -1 (bitwise-xor greatest least))
;; Left shifts: the last one that stays a fixnum, and the first that must
;; promote — the narrowing bail inside the fast path.
(test-equal "the last left shift that stays a fixnum" 576460752303423488
  (arithmetic-shift 1 59))
(test-equal "the first left shift that must promote" 1152921504606846976
  (arithmetic-shift 1 60))
(test-equal "-1 shifted to the least fixnum" least (arithmetic-shift -1 60))
(test-equal "-1 shifted past the least fixnum" -2305843009213693952
  (arithmetic-shift -1 61))
;; Right shifts stay on the fast path even past the width: positive operands
;; drain to 0, negative ones sign-fill to -1.
(test-equal "a positive operand drains to 0" 0 (arithmetic-shift greatest -100))
(test-equal "a negative operand fills to -1" -1 (arithmetic-shift least -100))
;; Mixed fixnum/bignum operands must fall back, not misclassify.
(test-equal "a bignum and a fixnum operand" 0
  (bitwise-and (expt 2 61) greatest))
;; bit-set? at and past the sign boundary.
(test-assert "bit 60 of -1 is set" (bit-set? 60 -1))
(test-assert "bit 60 of the greatest fixnum is clear"
  (not (bit-set? 60 greatest)))
(test-assert "a bit far past the width of a negative is set"
  (bit-set? 200 least))
;; bit-count / integer-length at the extremes. For a negative argument
;; bit-count counts zero bits: (bitwise-not least) = greatest = 60 ones.
(test-equal "bit-count at the extremes" '(60 60)
  (list (bit-count greatest) (bit-count least)))
(test-equal "integer-length at the extremes" '(60 60)
  (list (integer-length greatest) (integer-length least)))

;; ─── SRFI 60, the name the corpus imports most ───────────────────────────────

;; In-degree 31 over the vendored corpus. It spells things differently from
;; 151, so importing 151 is not a substitute.
(cond-expand ((or patina gauche)) (else (test-skip 1)))
(test-equal "SRFI 60 spellings" '(8 14 6 -13 1024 2 #t 3)
  (list (srfi-60:logand 12 10) (srfi-60:logior 12 10) (srfi-60:logxor 12 10)
        (srfi-60:lognot 12) (srfi-60:ash 1 10) (srfi-60:logcount 12)
        (srfi-60:logbit? 2 12) (srfi-60:log2-binary-factors 8)))

;; SRFI 60's list conversions are MSB-first — the opposite of SRFI 151's
;; bits->list family, which is why 151 renamed them.
(cond-expand ((or patina gauche)) (else (test-skip 1)))
(test-equal "SRFI 60's integer->list is MSB-first" '(#t #t #f)
  (srfi-60:integer->list 6))
(cond-expand ((or patina gauche)) (else (test-skip 1)))
(test-equal "SRFI 60's integer->list pads on the left" '(#f #f #t #t #f)
  (srfi-60:integer->list 6 5))
(cond-expand ((or patina gauche)) (else (test-skip 1)))
(test-equal "SRFI 60's list->integer reads MSB-first" 6
  (srfi-60:list->integer '(#t #t #f)))
(cond-expand ((or patina gauche)) (else (test-skip 1)))
(test-equal "SRFI 60's conversions round-trip" 12345
  (srfi-60:list->integer (srfi-60:integer->list 12345)))
(cond-expand ((or patina gauche)) (else (test-skip 1)))
(test-equal "booleans->integer" 10 (srfi-60:booleans->integer #t #f #t #f))

;; SRFI 60 exports both spellings of eight operators — `logand` *and*
;; `bitwise-and`, `ash` *and* `arithmetic-shift`. Easy to lose one half.
(cond-expand ((or patina gauche)) (else (test-skip 1)))
(test-equal "SRFI 60 exports the SRFI 33 spellings too" '(8 16 2 8)
  (list (srfi-60:bitwise-and 12 10) (srfi-60:arithmetic-shift 1 4)
        (srfi-60:bit-count 12) (srfi-60:logand 12 10)))

;; ─── SRFI 142, 151's withdrawn predecessor ───────────────────────────────────

;; A rename over the same primitives, with one semantic twist: its
;; `bitwise-if` takes bits from the *third* argument where the mask bit is 1 —
;; the opposite of 151's — so the shim swaps the trailing arguments rather than
;; aliasing. mask 5 (101), 3 (011), 0: 142 keeps 3's mask-0 bits, so 2, where
;; 151 answers 1 above.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "SRFI 142's bitwise-if takes mask-0 bits from the second argument" 2
  (srfi-142:bitwise-if 5 3 0))

;; Unlike SRFI 60's MSB-first `integer->list`, SRFI 142's is LSB-first — the
;; very family 151 renamed to `bits->list` without changing the order.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "SRFI 142's conversions are LSB-first" '((#f #t #t) 6 6 12345)
  (list (srfi-142:integer->list 6)
        (srfi-142:list->integer '(#f #t #t))
        (srfi-142:vector->integer #(#f #t #t))
        (srfi-142:list->integer (srfi-142:integer->list 12345))))

;; Headline operators reached through the 142 name — including the three
;; jkode-sassy, the corpus package that imports it, actually calls.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "SRFI 142 headline operators" '(8 14 256 3 2)
  (list (srfi-142:bitwise-and 12 10) (srfi-142:bitwise-ior 12 10)
        (srfi-142:arithmetic-shift 1 8) (srfi-142:bit-count 7)
        (srfi-142:first-set-bit 12)))

;; ─── SRFI 33 ─────────────────────────────────────────────────────────────────

;; `bitwise-merge` takes a result bit from its *second* argument where the
;; mask bit is 0: SRFI 33 says "RESULT[k] := if MASK[k] = 0 then I0[k] else
;; I1[k]", which is SRFI 142's `bitwise-if` order. mask 3, 1, 8: bits 0–1 come
;; from 8 and the rest from 1, so 0. These rows asserted 9 and 240, SRFI 151's
;; answers, until #264.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "bitwise-merge takes mask-0 bits from the second argument" '(0 2)
  (list (srfi-33:bitwise-merge 3 1 8) (srfi-33:bitwise-merge 5 3 0)))
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "SRFI 33's any-bits-set? and all-bits-set?" '(#t #t)
  (list (srfi-33:any-bits-set? 12 10) (srfi-33:all-bits-set? 4 6)))

;; All five SRFI 33 field operations. `copy-bit-field` takes
;; `(size position from to)` like its siblings, not SRFI 60's
;; `(to from start end)`, and returns TO with the field replaced by FROM's.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "extract-bit-field" '(15 5)
  (list (srfi-33:extract-bit-field 4 0 255)
        (srfi-33:extract-bit-field 4 8 #xA55A)))
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "replace-bit-field" 245 (srfi-33:replace-bit-field 4 0 5 255))
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "copy-bit-field copies FROM's field into TO" '(15 240)
  (list (srfi-33:copy-bit-field 4 0 255 0) (srfi-33:copy-bit-field 4 4 255 0)))
;; test-bit-field? / clear-bit-field are renames of SRFI 151's bit-field-any? /
;; bit-field-clear, so they take (n start end) — chibi's (srfi 33) makes the
;; same choice.
(cond-expand ((or patina chibi)) (else (test-skip 1)))
(test-equal "test-bit-field? and clear-bit-field" '(#t #f 12)
  (list (srfi-33:test-bit-field? 10 1 2) (srfi-33:test-bit-field? 10 2 3)
        (srfi-33:clear-bit-field 15 0 2)))

(test-end)
