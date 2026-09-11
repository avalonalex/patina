;; SRFI 162 — the comparator constants and min/max procedures, which live in
;; `(srfi 128)`.
;;
;; Migrated from `crates/patina-tests/tests/srfi_162_comparators.rs` (#193
;; Phase 2), which is deleted. The upstream SRFI 128 suite
;; (`upstream_srfi_suites.rs`) is the real gate — 170 assertions, and it
;; reaches these names because Patina exports them from `(srfi 128)`. What it
;; does not check is where they come from, which is the whole design
;; decision: SRFI 162 says its bindings belong in the SRFI 128 library rather
;; than one of their own. So these rows pin that reachability, and
;; `pair-comparator`, which the specification lists and chibi's export list
;; omits.

(import (scheme base) (srfi 128) (prefix (scheme comparator) sc:) (srfi 64))

(test-begin "comparators")

;; Every SRFI 162 name resolves through `(srfi 128)`, and each constant is
;; actually a comparator rather than an unbound name that merely imported.
(test-equal "every SRFI 162 constant is a comparator exported by (srfi 128)"
  '(#t #t #t #t #t #t #t #t #t #t #t #t #t)
  (map comparator?
       (list default-comparator boolean-comparator real-comparator
             char-comparator char-ci-comparator
             string-comparator string-ci-comparator
             pair-comparator list-comparator vector-comparator
             eq-comparator eqv-comparator equal-comparator)))

;; `pair-comparator` is specified by SRFI 162 ("compares pairs as if by the
;; application of make-pair-comparator … with default-comparator") and
;; defined by its sample implementation, but chibi's `(srfi 128)` export list
;; leaves it out. Ours does not, so the omission cannot be inherited by
;; copying chibi's list in some later edit.
(test-equal "pair-comparator orders pairs" '(#t #t #t)
  (list (=? pair-comparator '(1 . 2) '(1 . 2))
        (<? pair-comparator '(1 . 2) '(1 . 3))
        (<? pair-comparator '(1 . 9) '(2 . 0))))

(test-equal "comparator-min and -max take arguments and lists" '(5 1 "c" "a")
  (list (comparator-max real-comparator 3 1 4 1 5)
        (comparator-min real-comparator 3 1 4 1 5)
        (comparator-max-in-list string-comparator '("b" "a" "c"))
        (comparator-min-in-list string-comparator '("b" "a" "c"))))

;; The case-insensitive constants are not aliases of their case-sensitive
;; neighbours — the pairing is easy to get wrong when transcribing a list of
;; sixteen names.
(test-equal "the case-insensitive comparators ignore case" '(#t #f #t #f)
  (list (=? string-ci-comparator "Abc" "aBC")
        (=? string-comparator "Abc" "aBC")
        (=? char-ci-comparator #\A #\a)
        (=? char-comparator #\A #\a)))

;; R7RS-large reaches the same bindings under `(scheme comparator)`.
;; `r7rs_large_aliases.rs` checks the export sets are equal; this checks the
;; names work through the alias, which an equal-but-both-broken pair of lists
;; would not catch. Imported under a prefix so the file does not depend on
;; the two libraries exporting identical bindings.
(test-equal "the names work through the R7RS-large alias" '(7 #t #t)
  (list (sc:comparator-max sc:real-comparator 2 7 1)
        (sc:comparator? sc:default-comparator)
        (sc:=? sc:equal-comparator '(1 #(2)) '(1 #(2)))))

(test-end)
