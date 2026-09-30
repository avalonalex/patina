;; The R7RS-large bytevector import (#575), using R6RS procedure semantics.
;; In particular, copy! is source-first and its final argument is a count.
;; Prefix the R7RS-small copy procedure so both conventions are exercised.
;; Mutation of imported bindings is Patina policy, tested in Rust instead.
;; Gauche 0.9.15 fails integer list conversions: its %list->v computes
;; (length list), the procedure, instead of (length lis), the input. Patina,
;; Chibi 0.12 and Chez 10.3 return the expected list (#575, 2026-09-30).
;; DIVERGENCES.tsv records this oracle defect without changing the assertion.

(import (except (scheme base) bytevector-copy!)
        (scheme bytevector)
        (prefix (only (scheme base) bytevector-copy!) small:)
        (srfi 64))

(test-begin "bytevector-library")

(test-equal "signed fill uses two's complement" '(128 128 128)
  (bytevector->u8-list (make-bytevector 3 -128)))

(test-equal "fill mutates the bytevector" '(127 127 127)
  (let ((b (make-bytevector 3 0)))
    (bytevector-fill! b 127)
    (bytevector->u8-list b)))

(test-equal "R6RS copy is source-first with a count"
  '((10 20 30 40) (0 20 30 0 0))
  (let ((source (bytevector 10 20 30 40)) (target (make-bytevector 5 0)))
    (bytevector-copy! source 1 target 1 2)
    (list (bytevector->u8-list source) (bytevector->u8-list target))))

(test-equal "overlap toward higher indices" '(1 1 2 3 4)
  (let ((b (bytevector 1 2 3 4 5)))
    (bytevector-copy! b 0 b 1 4)
    (bytevector->u8-list b)))

(test-equal "overlap toward lower indices" '(2 3 4 5 5)
  (let ((b (bytevector 1 2 3 4 5)))
    (bytevector-copy! b 1 b 0 4)
    (bytevector->u8-list b)))

(test-equal "R7RS-small copy keeps its own convention"
  '((10 20 30 40) (0 20 30 0 0))
  (let ((source (bytevector 10 20 30 40)) (target (make-bytevector 5 0)))
    (small:bytevector-copy! target 1 source 1 3)
    (list (bytevector->u8-list source) (bytevector->u8-list target))))

(test-equal "integer layout and explicit endianness" '((52 18) 4660 13330)
  (let ((b (make-bytevector 2 0)))
    (bytevector-u16-set! b 0 #x1234 (endianness little))
    (list (bytevector->u8-list b)
          (bytevector-u16-ref b 0 (endianness little))
          (bytevector-u16-ref b 0 (endianness big)))))

(test-equal "signed integer wider than 64 bits" -3
  (let ((b (make-bytevector 16 0)))
    (bytevector-sint-set! b 0 -3 (native-endianness) 16)
    (bytevector-sint-ref b 0 (native-endianness) 16)))

(test-equal "IEEE double access" 1.5
  (let ((b (make-bytevector 8 0)))
    (bytevector-ieee-double-set! b 0 1.5 (endianness little))
    (bytevector-ieee-double-ref b 0 (endianness little))))

(test-equal "UTF-16 round trip" "Aλ"
  (utf16->string (string->utf16 "Aλ" (endianness little))
                (endianness little) #t))

(test-equal "UTF-32 supplementary character" "Aλ😀"
  (utf32->string (string->utf32 "Aλ😀" (endianness big))
                (endianness big) #t))

(test-equal "integer list conversions" '(0 257 65535)
  (bytevector->uint-list
    (uint-list->bytevector '(0 257 65535) (native-endianness) 2)
    (native-endianness) 2))

(test-end "bytevector-library")
