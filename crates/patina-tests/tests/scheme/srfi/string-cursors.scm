;; SRFI 130 cursor-based strings.
;;
;; Migrated from `crates/patina-tests/tests/srfi_130_string.rs` (#193), which
;; is deleted. Conformance is not this file's job: upstream's own
;; 219-assertion suite runs in `upstream_srfi_suites.rs`, and hand-written rows
;; beside a ported library check only what the porter thought of. What is left
;; for here is the property the SRFI exists for, the one place the port
;; deviates, and one headline row per form.
;;
;; The Rust file's other two tests were about *where* the library resolves
;; from — that `(srfi 130)` and its `(srfi 14)` dependency load from `lib/`
;; alone, and that `(chibi string)` does not. A suite file cannot say that,
;; because the driver supplies `test-lib/` like every other helper, so they
;; moved to `upstream_srfi_suites.rs` beside the suite whose harness has the
;; same blind spot.

(import (scheme base) (scheme char) (srfi 130) (srfi 64))

(test-begin "string-cursors")

;; The reason the SRFI exists: searching returns a *cursor*, not an index. A
;; port that quietly returned indices would pass most string tests and break
;; every caller that then compares or advances the result.
(test-equal "a search returns a cursor at the match" 2
  (string-cursor->index "hello" (string-contains "hello" "ll")))
(test-equal "a failed search returns #f" #f (string-contains "hello" "zz"))
(test-equal "the cursor difference across a string is its length" 5
  (string-cursor-diff "hello" (string-cursor-start "hello")
                      (string-cursor-end "hello")))
(test-assert "string-cursor-start returns a cursor"
  (string-cursor? (string-cursor-start "abc")))

;; `string-drop` carries this tree's one local edit — upstream calls
;; `(substring str n)`, which R7RS does not allow. The boundaries are where a
;; wrong end argument would show.
(test-equal "dropping nothing keeps the string" "hello" (string-drop "hello" 0))
(test-equal "dropping everything leaves the empty string" ""
  (string-drop "hello" 5))
(test-equal "dropping a prefix keeps the rest" "lo" (string-drop "hello" 3))

;; Headline forms, one row each, as the smoke test Track L's L1 acceptance
;; criterion asks for.
(test-equal "string-join" "a-b-c" (string-join '("a" "b" "c") "-"))
(test-equal "string-split" '("a" "b" "c") (string-split "a,b,c" ","))
(test-equal "string-take" "hel" (string-take "hello" 3))
(test-equal "string-take-right" "lo" (string-take-right "hello" 2))
(test-equal "string-pad" "  7" (string-pad "7" 3))
(test-equal "string-reverse" "cba" (string-reverse "abc"))
(test-assert "string-prefix?" (string-prefix? "hell" "hello"))
(test-equal "string-count" 10 (string-count "hello world" char-alphabetic?))

(test-end)
