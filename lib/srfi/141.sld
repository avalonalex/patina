;; SRFI 141: Integer division (#576).
;;
;; The SRFI's own reference implementation, from
;; https://github.com/scheme-requests-for-implementation/srfi-141
;; commit c1ed65d529d44bf96cf93eeaefaf665495e426ad,
;; srfi-141-impl.scm. Its Taylor R. Campbell BSD-2-Clause notice and
;; William D Clinger permission notice are retained in 141/division.scm.
;;
;; This R7RS wrapper replaces upstream's (srfi-141) name with (srfi 141).
;; Seven marked datum comments in the body reuse (scheme base)'s
;; exact-integer? and floor/truncate families. A marked argument validator
;; and calls in the twelve added public procedures reject non-integers and
;; zero divisors, as Patina's existing division procedures do; upstream's
;; inexact paths do not check these error situations. Other code is verbatim.
;; The implementation and wrapper are pinned post-edit in
;; crates/patina-tests/tests/bundled_provenance.rs.

(define-library (srfi 141)
  (import (scheme base))
  (export ceiling/ ceiling-quotient ceiling-remainder
          floor/ floor-quotient floor-remainder
          truncate/ truncate-quotient truncate-remainder
          round/ round-quotient round-remainder
          euclidean/ euclidean-quotient euclidean-remainder
          balanced/ balanced-quotient balanced-remainder)
  (include "141/division.scm"))
