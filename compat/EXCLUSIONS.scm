;;; Packages excluded from the Patina compatibility score.
;;;
;;; Read by `cargo run -p patina-compat -- run`; see
;;; `crates/patina-compat/src/exclusions.rs` for the format and the rules.
;;;
;;; What this file is for: the corpus contains packages that cannot pass for
;;; reasons that say nothing about Patina. Counting them makes the headline
;;; measure the corpus, and — worse — leaves the bundling queue pointing at
;;; libraries whose only requester would still fail with them in hand.
;;;
;;; What this file is NOT: a place to park failures. Three things stop that.
;;;
;;;   * Every excluded package still runs, on every pass, and `results.scm`
;;;     records its status exactly as it records everything else. Nothing
;;;     here changes what is measured, only what is scored.
;;;   * Every entry declares the status it was written against. When a
;;;     package stops producing that status the report says the entry has
;;;     drifted rather than quietly applying it — including when the package
;;;     starts passing, which is how an entry gets retired.
;;;   * The raw "N of M" is printed first and is never affected by this file.
;;;
;;; Adding an entry: the note must let a reader check the claim without
;;; re-deriving it — name the file and line for a source defect, and the
;;; implementation that agrees with us in rejecting it. `--exclusions none`
;;; scores every package, for when the raw corpus is what you want.
;;;
;;; Reasons (closed set; a failure that fits none of them is in scope):
;;;   ffi                     needs a foreign-function interface
;;;   dependency-not-vendored our own corpus licence policy, not Patina
;;;   upstream-source-defect  the package's source is invalid Scheme
;;;   upstream-test-defect    the library works; only its suite fails

(patina-compat-exclusions
 (version 1)
 (exclusions

  ;;; ---------------------------------------------------------------- ffi
  ;;; Track L §3 defers FFI, and these are the C-shim spelling of it: chibi
  ;;; `.stub` files compiled against a host library. A Rust FFI would not
  ;;; make them load unchanged, so they are out of scope under either future
  ;;; — dropped from the corpus, or rescoped onto that work.
  ;;;
  ;;; Patina refuses `include-shared` deliberately and names the shared
  ;;; object, so these are detected rather than assumed. Note that a package
  ;;; carrying a `.stub` is *not* on its own evidence of this: chibi-crypto-
  ;;; sha2 and chibi-math-linalg both ship one behind a `chibi`-only
  ;;; cond-expand branch and have a working portable path, which is why they
  ;;; are absent from this section.

  ((slug "chibi-mecab") (reason ffi) (expect out-of-scope)
   (note "chibi/mecab.sld:38 (include-shared \"mecab\") — bindings to libmecab"))
  ((slug "chibi-ssl") (reason ffi) (expect out-of-scope)
   (note "chibi/ssl.sld:14 (include-shared \"ssl\") — bindings to OpenSSL"))
  ((slug "chibi-xlib") (reason ffi) (expect out-of-scope)
   (note "chibi/xlib.sld:45 (include-shared \"xlib\") — bindings to Xlib"))
  ((slug "independentresearch-xattr") (reason ffi) (expect out-of-scope)
   (note "independentresearch/xattr.sld:9 (include-shared \"xattr\") — POSIX extended attributes"))
  ;; Retired its own prediction: the note below used to end "the FFI need is
  ;; real but shadowed", because the package reported (srfi 160 base) missing
  ;; before it could reach its own include-shared. #383 bundled SRFI 160, the
  ;; shadow lifted, and it now proves the FFI need directly — so the expected
  ;; status moves from missing-library to out-of-scope, which is what the
  ;; drift check asked for.
  ((slug "chibi-xgboost") (reason ffi) (expect out-of-scope)
   (note "chibi/xgboost.sld:5 (include-shared \"xgboost/xgboost\") — bindings to libxgboost, reported directly since #383 bundled the (srfi 160 base) that used to shadow it"))
  ((slug "chibi-net-dns") (reason ffi) (expect out-of-scope)
   (note "needs (chibi net), which is C-backed upstream"))
  ((slug "chibi-net-smtp") (reason ffi) (expect out-of-scope)
   (note "needs (chibi net), which is C-backed upstream"))
  ((slug "srfi-106") (reason ffi) (expect out-of-scope)
   (note "srfi/106.sld:6 imports (foreign c), chibi's FFI interface library"))
  ((slug "srfi-170") (reason ffi) (expect out-of-scope)
   (note "srfi/170.sld:8 imports (foreign c); a POSIX API over chibi's FFI"))
  ;; Two that sat under upstream-source-defect until #428's patches removed
  ;; the defect in front of them, and what was behind it turned out to be FFI.
  ((slug "postgresql") (reason ffi) (expect out-of-scope)
   (note "imports (foreign c); reported directly since compat/patches/chibi-bytevector.patch removed the (chibi bytevector) typo that used to stop it first"))
  ;; Behind the literals-list typo was Patina's own #431 - the bundled
  ;; (srfi 115) failing to load with a (chibi char-set) on the path, which this
  ;; package's closure puts there - and that was *ours*, so it was fixed in the
  ;; change that re-filed this entry rather than excluded around. Behind that
  ;; is what is left: (chibi net http), C-backed like the rest of (chibi net).
  ;; Both backends report it alike, which the defect in front of it did not.
  ((slug "chibi-snow-commands") (reason ffi) (expect missing-library)
   (note "needs (chibi net http), C-backed upstream like the rest of (chibi net); (srfi 18) threads is wanted behind it"))

  ;;; ------------------------------------------- dependency-not-vendored
  ;;; build_corpus.py vendors only packages whose licence it can establish.
  ;;; These two need a package it declined, so the blocker is our own
  ;;; corpus policy arriving in the score — see compat/vendor/REVIEW-QUEUE.json,
  ;;; where both dependencies sit in the UNKNOWN bucket.

  ((slug "rebottled-cl-pdf") (reason dependency-not-vendored) (expect missing-library)
   (note "needs (rebottled pregexp); REVIEW-QUEUE.json has it under UNKNOWN licence, so it is not vendored"))
  ((slug "retropikzel-pstk") (reason dependency-not-vendored) (expect missing-library)
   (note "needs (retropikzel named-pipes); REVIEW-QUEUE.json has it under UNKNOWN licence, so it is not vendored"))

  ;;; ------------------------------------------------ upstream-source-defect
  ;;; Audited row by row on 2026-08-19 against chibi 0.12 and Gauche 0.9.15
  ;;; and written up in Track L PRD §6 "Upstream, not ours". Each is a
  ;;; cond-expand fallback branch chibi itself never compiles, so the corpus
  ;;; is the first thing ever to execute it; Gauche rejects all of them
  ;;; exactly as Patina does. Accepting them would mean widening the
  ;;; language to match one reader's leniency.
  ;;;
  ;;; Ten packages left this section on 2026-09-19 (#428), of the twelve it
  ;;; held. Every entry here was written before compat/patches/ existed, so
  ;;; none had been asked whether a patch would do. Eight now pass, each patch
  ;;; header carrying its argument: chibi-bytevector (and with it
  ;;; chibi-crypto-md5, -rsa and -sha2, which one token had been holding
  ;;; back), chibi-monad-environment, chibi-show, chibi-app and chibi-regexp.
  ;;; Two moved to `ffi`, which is what was behind the defect in front of
  ;;; them: postgresql and chibi-snow-commands. (A ninth pass, comparators,
  ;;; came out of upstream-test-defect below.) What stays is what no faithful
  ;;; patch reaches.

  ((slug "edn") (reason upstream-source-defect) (expect parse-error)
   (note "(chibi parse) parse.sld:66 — the fallback grammar-bind generates a pattern with `ch` twice; duplicate pattern variables are an error (R7RS 4.3.2) and Gauche fails edn end-to-end as we do"))
  ;; chibi-regexp sat here as won't-fix: two upstream libraries both call
  ;; their type a "char-set" and mean different record types, and regexp.sld
  ;; ends up with one of each whenever a (chibi char-set) is reachable. The
  ;; conclusion then was that the only repair was reimplementing (chibi
  ;; char-set) over SRFI 14. It is one line:
  ;; `compat/patches/chibi-char-set-boundary.patch` makes the boundary library
  ;; choose by the `chibi` feature, as regexp.sld beside it does, and
  ;; `chibi-regexp.patch` guards the one test group whose data file the
  ;; snowball does not ship. 86 of 86 on both backends. Patina's own bundled
  ;; (srfi 115) has the same line and the same exposure: #431.

  ;; srfi-179 builds a `u1-storage-class` — a one-bit vector — out of
  ;; `u1vector-ref`, `u1vector-set!`, `make-u1vector`, `u1vector-length` and
  ;; `u1?` (`srfi/179/transforms.scm:34`), and exports it from `srfi/179.sld`.
  ;; Nothing in the package defines any of them. They are not SRFI 160's
  ;; either: neither our bundled `(srfi 160 base)` nor the SRFI's own
  ;; reference implementation has a `u1` type, and the twelve the SRFI
  ;; specifies start at u8. They are a **chibi extension, implemented in C** —
  ;; `lib/srfi/160/uvprims.c` in the chibi tree defines `u1vector_ref` and
  ;; `u1vector_set`. So the package depends on its host providing a type
  ;; outside the SRFI it names as its dependency, which no conforming
  ;; implementation of that SRFI supplies.
  ;;
  ;; The two in-progress-hash-* packages used to sit here, for importing
  ;; `(srfi 114 comparators)` — a name nothing provides. They now pass, moved
  ;; to SRFI 128 by `compat/patches/in-progress-hash-tables.patch`, whose
  ;; header carries the argument (SRFI 114 is withdrawn; snow cannot resolve
  ;; the three-part name either) and why each rename is faithful.

  ;; Reached only since #383 bundled `(srfi 160 base)`; before that it stopped
  ;; at the missing library and this was invisible.
  ((slug "srfi-179") (reason upstream-source-defect) (expect parse-error)
   (note "srfi/179/transforms.scm:34 builds u1-storage-class from u1vector-ref and friends, which nothing defines: they are a chibi C extension (lib/srfi/160/uvprims.c), not part of SRFI 160, whose own reference implementation starts at u8"))

  ;; chibi-app sat here for an `else` clause mid-`case` (app.scm:467). It now
  ;; passes, 6 of 6: `compat/patches/chibi-app.patch` removes the clause after
  ;; `else`, which chibi never reaches, rather than moving `else` below it,
  ;; which would change what the program does. Patina's message for that
  ;; shape, which this entry used to carry, named neither the clause nor
  ;; where it was; it does both since #432.

  ;; chibi-math-stats sat here for one commit, for calling SRFI 1's `every` on
  ;; a vector at stats.scm:812. It now passes: the file defines its own
  ;; `seq-every` for exactly that check and uses it in three other places, so
  ;; `compat/patches/chibi-math-stats.patch` spells line 812 the way its author
  ;; spells the rest. That header carries the argument, including why chibi's
  ;; `#t` is a wrong answer rather than leniency.

  ;;; -------------------------------------------------- upstream-test-defect
  ;;; The library third parties import works on Patina; only the package's
  ;;; own test program fails, and for reasons that are not about us.

  ((slug "chibi-assert") (reason upstream-test-defect) (expect missing-library)
   (note "(chibi assert) itself loads and runs on Patina — its cond-expand else branch is portable — but chibi/assert-test.sld:2 imports (chibi), chibi's implementation core, for protect and exception-irritants. Not patched, measured 2026-09-19: with those rewritten to guard and error-object-irritants it is 3 of 4, and the fourth cannot pass off chibi — the portable branch reports the datum inside 'three as a free variable and raises unbound variable, on Gauche exactly as on Patina"))
  ;; comparators sat here for a test program written as a CHICKEN script. It
  ;; now passes, 144 of 144: `compat/patches/comparators.patch` replaces the
  ;; three header lines with an R7RS import - the README's third admitted
  ;; shape, and the one that stretches furthest - and adds three imports the
  ;; *library* used and never declared.
  ((slug "chibi-voting") (reason upstream-test-defect) (expect wrong-result)
   (note "instant-runoff-rank's expectation depends on hash-table iteration order, which no standard specifies; chibi, Gauche and Patina each produce a different ranking and Gauche fails the suite as we do"))
  ((slug "srfi-197") (reason upstream-test-defect) (expect runtime-error)
   (note "its test program (include \"./test.scm\")s a file the package does not ship"))

  ))
