;; SRFI 130 cursor-based strings.
;;
;; Migrated from `crates/patina-tests/tests/srfi_130_string.rs` (#193), which
;; is deleted. Conformance is not this file's job: upstream's own
;; 219-assertion suite runs in `upstream_srfi_suites.rs`, and hand-written rows
;; beside a ported library check only what the porter thought of. What is left
;; for here is the property the SRFI exists for, the places the port
;; deviates, regressions absent from upstream (#204), and one headline row
;; per form.
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

;; `string-drop` carries a local edit — upstream calls
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

;; #204: start is inclusive and end exclusive. These cases were absent from
;; upstream's suite. The Unicode rows use explicit cursors, which Chibi keeps
;; distinct from indexes even though Patina uses the same representation.
(test-equal "string-any excludes the prefix" #f
  (string-any char-numeric? "1abc" 1 4))
(test-equal "string-every excludes the prefix" #t
  (string-every char-alphabetic? "1abc" 1 4))
(test-equal "string-any with only start" #f
  (string-any char-numeric? "1abc" 1))
(test-equal "string-every with only start" #t
  (string-every char-alphabetic? "1abc" 1))
(test-equal "string-any excludes the suffix" #f
  (string-any char-numeric? "abc1" 0 3))
(test-equal "string-every excludes the suffix" #t
  (string-every char-alphabetic? "abc1" 0 3))
(test-equal "string-any respects Unicode cursors" #f
  (let ((s "1λ界1"))
    (string-any char-numeric? s
      (string-index->cursor s 1) (string-index->cursor s 3))))
(test-equal "string-every respects Unicode cursors" #t
  (let ((s "1λ界1"))
    (string-every (lambda (ch) (not (char=? ch #\1))) s
      (string-index->cursor s 1) (string-index->cursor s 3))))
(test-equal "string-any on empty ranges does not call the predicate" '(#f #f #f)
  (map (lambda (bounds)
         (apply string-any (lambda (ch) (error "empty range visited")) bounds))
       '(("" 0 0) ("abc" 1 1) ("abc" 3 3))))
(test-equal "string-every on empty ranges does not call the predicate" '(#t #t #t)
  (map (lambda (bounds)
         (apply string-every (lambda (ch) (error "empty range visited")) bounds))
       '(("" 0 0) ("abc" 1 1) ("abc" 3 3))))

;; SRFI 130's predicates are witness-generating, including non-boolean true
;; values. `string-every` cannot be implemented by negating `string-any`.
;; https://srfi.schemers.org/srfi-130/srfi-130.html#string-every
(test-equal "string-any returns the first successful witness" #\b
  (string-any (lambda (ch) (and (not (char=? ch #\a)) ch)) "abc"))
(test-equal "string-every returns the last witness" #\c
  (string-every (lambda (ch) ch) "abc"))
(test-equal "string-every returns the bounded last witness" #\b
  (string-every (lambda (ch) ch) "1abc" 1 3))
(test-equal "string-any on a single character returns its witness" #\b
  (string-any (lambda (ch) ch) "abc" 1 2))
(test-equal "string-every on a single character returns its witness" #\b
  (string-every (lambda (ch) ch) "abc" 1 2))
(test-equal "string-every stops at the first false result" #f
  (string-every (lambda (ch)
                  (if (char=? ch #\a) #f (error "visited after failure")))
                "ab"))

;; Audit the sibling range paths through their public SRFI names. Right-to-left
;; searches return the successor of a match, or start when nothing matches.
;; Explicit cursors avoid Chibi's separate integer-bound string-skip defect.
(test-equal "sibling searches and count respect the bounded range"
  '(2 3 1 4 2)
  (let* ((s "1a1b1")
         (start (string-index->cursor s 1))
         (end (string-index->cursor s 4)))
    (list (string-cursor->index s (string-index s char-numeric? start end))
          (string-cursor->index s (string-index-right s char-numeric? start end))
          (string-cursor->index s (string-skip s char-numeric? start end))
          (string-cursor->index s (string-skip-right s char-numeric? start end))
          (string-count s char-alphabetic? 1 4))))
(test-equal "sibling searches return range boundaries when no match exists"
  '(3 1 3 1 0)
  (let* ((s "1ab1")
         (start (string-index->cursor s 1))
         (end (string-index->cursor s 3)))
    (list (string-cursor->index s (string-index s char-numeric? start end))
          (string-cursor->index s (string-index-right s char-numeric? start end))
          (string-cursor->index s (string-skip s char-alphabetic? start end))
          (string-cursor->index s (string-skip-right s char-alphabetic? start end))
          (string-count s char-numeric? 1 3))))

(test-end)
