;; SRFI 27 random sources.
;;
;; Migrated from `crates/patina-tests/tests/srfi_27.rs` (#193 Phase 2), which
;; is deleted. The bundled implementation is Sebastian Egner's 54-bit
;; MRG32k3a reference (see `lib/srfi/PROVENANCE.md`), and chibi's suite,
;; vendored at `scheme_tests/upstream/srfi/27/`, exercises the interface. These
;; rows pin what it does not: the exact stream values, that a state
;; round-trip *replays* the tail (upstream has that check commented out as
;; "actually impl defined"), and that values are exact and in range. Do not
;; trim this to the stream values.

(import (scheme base) (srfi 27) (srfi 64))

(test-begin "random")

;; `pseudo-randomize!` must select a reproducible, seed-determined stream —
;; SRFI 27's central guarantee. The exact values are recorded from this
;; implementation: MRG32k3a is fully specified, so a change to them is a
;; generator change, not noise. (Known limit: they pin the port's arithmetic
;; as it is, faithful bugs included — they were not re-derived from
;; L'Ecuyer's matrices.)
;;
;; Scoped to Patina because SRFI 27 mandates no generator: chibi and Gauche
;; answer different numbers, and neither is wrong. The next row is the
;; portable half of the same guarantee.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a pseudo-randomized stream is deterministic" '(744602 257620 441205)
  (let ((s (make-random-source)))
    (random-source-pseudo-randomize! s 4 7)
    (let* ((gen (random-source-make-integers s))
           (a (gen 1000000))
           (b (gen 1000000))
           (c (gen 1000000)))
      (list a b c))))

;; The same (i, j) must reproduce the same stream; a different j must not.
(test-equal "streams reproduce by seed" '(#t #f)
  (let ((take-3 (lambda (i j)
                  (let ((s (make-random-source)))
                    (random-source-pseudo-randomize! s i j)
                    (let* ((gen (random-source-make-integers s))
                           (a (gen 4096))
                           (b (gen 4096))
                           (c (gen 4096)))
                      (list a b c))))))
    (list (equal? (take-3 4 7) (take-3 4 7))
          (equal? (take-3 4 7) (take-3 4 8)))))

;; `state-ref`/`state-set!` round-trips: restoring a state replays the tail.
(test-assert "restoring a state replays the stream"
  (let* ((s (make-random-source))
         (gen (random-source-make-integers s)))
    (gen 1000)
    (gen 1000)
    (let* ((saved (random-source-state-ref s))
           (a1 (gen 1000))
           (a2 (gen 1000)))
      (random-source-state-set! s saved)
      (let* ((b1 (gen 1000))
             (b2 (gen 1000)))
        (equal? (list a1 a2) (list b1 b2))))))

;; Range and type contracts: integers land in [0, n) and are exact; reals
;; land strictly inside (0, 1).
(test-assert "values respect their ranges"
  (do ((i 0 (+ i 1))
       (ok #t (and ok
                   (let ((n (random-integer 17))
                         (r (random-real)))
                     (and (exact? n) (<= 0 n) (< n 17) (< 0 r) (< r 1))))))
      ((= i 200) ok)))

(test-end)
