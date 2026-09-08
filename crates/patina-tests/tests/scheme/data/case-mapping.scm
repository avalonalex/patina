;; Unicode case mapping — R7RS §6.6, the *simple* mappings.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (Larceny
;; family 6, #193 Phase 1). Fixed 2026-08-24.
;;
;; R7RS asks for the simple mappings, not the full ones, and the interesting
;; characters are those where the two differ:
;;
;;   - `ß` upcases to `SS` under the *full* mapping and to itself under the
;;     simple one, so `char-upcase` must leave it alone — a character
;;     procedure cannot return two characters;
;;   - `İ` (U+0130) downcases to `i̇` (two code points) fully, and to `i`
;;     simply;
;;   - `ᾀ` (U+1F80) upcases to `ᾈ` (U+1F88) in the tabled simple mapping,
;;     where a naive one-to-one table has no entry at all;
;;   - `ẞ` (U+1E9E) folds to `ß`, which is what makes the `-ci` comparisons
;;     agree.
;;
;; The `-ci` character comparisons compare *simple* foldings; `string-ci=?`
;; compares full ones, which is why `"Straße"` and `"STRASSE"` are equal as
;; strings while `#\ß` and `#\t` do not order as their full foldings would.

(import (scheme base) (scheme char) (srfi 64))

(test-begin "case-mapping")

(test-equal "the simple mappings, where they differ from the full ones"
  '(#\ß #\ß #\ß #\i #\ᾈ)
  (list (char-upcase #\ß)        ; full would be SS, which will not fit
        (char-foldcase #\ß)
        (char-foldcase #\x1E9E)  ; ẞ folds to ß
        (char-downcase #\x130)   ; İ, simply
        (char-upcase #\x1F80)))  ; ᾀ → ᾈ, tabled

(test-equal "the case-insensitive comparisons compare foldings"
  '(#t #t #f #t)
  (list (char-ci=? #\ς #\σ)          ; final and medial sigma
        (char-ci=? #\ß #\x1E9E)      ; ß and ẞ fold together
        (char-ci<? #\ß #\t)          ; ß folds to itself, which is not < t
        (string-ci=? "Straße" "STRASSE")))  ; strings fold fully

(test-end)
