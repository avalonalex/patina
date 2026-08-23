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
  ((slug "chibi-xgboost") (reason ffi) (expect missing-library)
   (note "chibi/xgboost.sld:5 (include-shared \"xgboost/xgboost\"); reports (srfi 160 base) because its test library's import fails before (chibi xgboost) is reached, so the FFI need is real but shadowed"))
  ((slug "chibi-net-dns") (reason ffi) (expect out-of-scope)
   (note "needs (chibi net), which is C-backed upstream"))
  ((slug "chibi-net-smtp") (reason ffi) (expect out-of-scope)
   (note "needs (chibi net), which is C-backed upstream"))
  ((slug "srfi-106") (reason ffi) (expect out-of-scope)
   (note "srfi/106.sld:6 imports (foreign c), chibi's FFI interface library"))
  ((slug "srfi-170") (reason ffi) (expect out-of-scope)
   (note "srfi/170.sld:8 imports (foreign c); a POSIX API over chibi's FFI"))

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

  ((slug "chibi-bytevector") (reason upstream-source-defect) (expect parse-error)
   (note "ieee-754.scm:16 — bytes-u8-set-all! has a 4-element syntax-rules rule ((_) bv off i), a parenthesization typo; Gauche: \"malformed macro\""))
  ((slug "chibi-crypto-md5") (reason upstream-source-defect) (expect parse-error)
   (note "via (chibi bytevector) — see chibi-bytevector"))
  ((slug "chibi-crypto-rsa") (reason upstream-source-defect) (expect parse-error)
   (note "via (chibi bytevector) — see chibi-bytevector"))
  ((slug "chibi-crypto-sha2") (reason upstream-source-defect) (expect parse-error)
   (note "via (chibi bytevector) — see chibi-bytevector; its own include-shared is behind a chibi-only branch and is not the blocker"))
  ((slug "postgresql") (reason upstream-source-defect) (expect parse-error)
   (note "via (chibi bytevector) — see chibi-bytevector"))
  ((slug "chibi-monad-environment") (reason upstream-source-defect) (expect parse-error)
   (note "environment.sld:6 — (syntax-rules ((_ x) 'x)) has no literals list; Gauche: \"literal list contains non-symbol\""))
  ((slug "chibi-show") (reason upstream-source-defect) (expect parse-error)
   (note "via (chibi monad environment) — see chibi-monad-environment"))
  ((slug "chibi-snow-commands") (reason upstream-source-defect) (expect parse-error)
   (note "via (chibi monad environment) — see chibi-monad-environment"))
  ((slug "edn") (reason upstream-source-defect) (expect parse-error)
   (note "(chibi parse) parse.sld:66 — the fallback grammar-bind generates a pattern with `ch` twice; duplicate pattern variables are an error (R7RS 4.3.2) and Gauche fails edn end-to-end as we do"))
  ((slug "chibi-app") (reason upstream-source-defect) (expect parse-error)
   (note "app.scm:467 — an else clause mid-case, followed by ((1) ...); R7RS puts else last and Gauche rejects it. Patina still owes a better message than \"No matching pattern for macro case\" with no location — that part is ours, tracked in PRD §6"))

  ;;; -------------------------------------------------- upstream-test-defect
  ;;; The library third parties import works on Patina; only the package's
  ;;; own test program fails, and for reasons that are not about us.

  ((slug "chibi-assert") (reason upstream-test-defect) (expect missing-library)
   (note "(chibi assert) itself loads and runs on Patina — its cond-expand else branch is portable — but chibi/assert-test.sld:2 imports (chibi), chibi's implementation core, for protect and exception-irritants"))
  ((slug "comparators") (reason upstream-test-defect) (expect unbound-identifier)
   (note "srfi-128/comparators/comparators-test.scm opens with (use test) (use srfi-128) — CHICKEN syntax, not R7RS"))
  ((slug "chibi-voting") (reason upstream-test-defect) (expect wrong-result)
   (note "instant-runoff-rank's expectation depends on hash-table iteration order, which no standard specifies; chibi, Gauche and Patina each produce a different ranking and Gauche fails the suite as we do"))
  ((slug "srfi-197") (reason upstream-test-defect) (expect runtime-error)
   (note "its test program (include \"./test.scm\")s a file the package does not ship"))

  ))
