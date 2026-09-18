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
  ;; Two upstream libraries both call their type a "char-set" and mean
  ;; different things, and `regexp.sld` imports one of each. Its `char-set?`
  ;; comes from `(srfi 14)` (regexp.sld:35) while `(chibi char-set boundary)`
  ;; (regexp.sld:63) resolves its own cond-expand to `(chibi char-set)`, whose
  ;; sets are iset-backed — a different record type. So the grapheme SRE's
  ;; embedded boundary sets satisfy no arm of `->rx`'s cond (regexp.scm:761),
  ;; it falls off the end returning #<unspecified>, and upstream's own guard
  ;; raises "expected a state" at the file's last form,
  ;; `(define re:grapheme (regexp 'grapheme))`. MEASURED 2026-09-18:
  ;; `(char-set? char-set:hangul-l)` is #f under the harness's roots.
  ;;
  ;; chibi is homogeneous the other way — its regexp.sld cond-expand takes
  ;; `(chibi)`, so its char-set? is the chibi one. Gauche is homogeneous by
  ;; accident: its cond-expand `(library ...)` test answers #f for anything on
  ;; the -I path (measured with a two-line library of our own: #f from
  ;; cond-expand, yet `import` of the same library works), so the boundary
  ;; library takes its `else` branch and every set is a real SRFI 14 one.
  ;; Patina resolves like chibi and imports char-set? like Gauche, which is
  ;; the only combination that mixes. Every step of that is R7RS-correct on
  ;; our side; Gauche's is the conformance gap, and not one to copy.
  ;;
  ;; Won't fix. The only repair on our side is a `(chibi char-set)` in
  ;; `test-lib/` reimplemented over our `(srfi 14)` — which is maintaining a
  ;; third-party library's API, not the `(chibi filesystem)` precedent (that
  ;; is upstream's own file plus one marked cond-expand branch). #372 already
  ;; removed the *other* half of this, the Latin-1 clipping that made those
  ;; boundary sets load empty; the type mismatch was underneath it and is not
  ;; ours. Its two dependents are excluded here already, for FFI and for an
  ;; unrelated upstream defect, so this entry costs one package.
  ((slug "chibi-regexp") (reason upstream-source-defect) (expect parse-error)
   (note "regexp.scm:1180 — (regexp 'grapheme) feeds #<unspecified> into make-state, because regexp.sld imports char-set? from (srfi 14) while its (chibi char-set boundary) dependency resolves to iset-backed (chibi char-set). chibi and Gauche each end up with one char-set type and load it; see the comment above for how each gets there"))

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
  ;; Both in-progress-hash-* packages depend on `(srfi 114 comparators)`, a
  ;; library name **nothing provides** — not this corpus, not Patina, not
  ;; chibi, not Gauche. The name itself is legal: R7RS 5.6.1 admits any
  ;; sequence of identifiers and unsigned integers, 36 vendored libraries use
  ;; three parts, and defining that exact name and importing it works under
  ;; all three implementations (measured 2026-09-18). It is simply a name the
  ;; author expected to exist.
  ;;
  ;; SRFI 114's own text says its procedures are in `(srfi 114)`; its sample
  ;; implementation names the library `(comparators)`, which this corpus
  ;; vendors as the `comparators` package. Neither is the three-element name.
  ;;
  ;; **Snow cannot resolve it either**, which is the part that settles this,
  ;; since snow-fort is chibi's own package manager. `library-name->path`
  ;; (chibi's `lib/chibi/snow/package.scm:327`) joins a name's parts with `/`
  ;; and nothing else — so `(srfi 114 comparators)` means exactly
  ;; `srfi/114/comparators.sld`, no alias table and no dependency rewriting.
  ;; No package in the index ships that path; `comparators` installs as
  ;; `comparators.sld`. chibi's `alias-for` cannot bridge it, because it is a
  ;; declaration *inside* a library file and would need that path to exist
  ;; first. So the dependency is unsatisfiable in snow's own ecosystem.
  ;;
  ;; Measured on the unmodified packages: chibi 0.12 fails on the identical
  ;; import with the identical diagnosis, and ships no SRFI 114 at all; Gauche
  ;; 0.9.15 *does* ship `(srfi 114)` and still cannot load them, failing a step
  ;; earlier on `(r6rs hashtables)` — which Patina provides, so Patina already
  ;; gets further into these packages than Gauche does.
  ;;
  ;; What would fix them, and why it is not done: bundling SRFI 114 under the
  ;; name the SRFI specifies and rewriting that one import makes both load —
  ;; tables directly, bimaps transitively, since the bad name appears only in
  ;; *its* `package.scm` metadata. That was staged and verified, then dropped:
  ;; `compat/vendor/README.md` calls these "unmodified upstream copies kept for
  ;; testing" whose purpose is "to run them and find out what Patina gets
  ;; wrong", and patching a package so it passes changes what is measured.
  ;; Issue #393 holds that decision; these entries retire if it goes the other
  ;; way.
  ((slug "in-progress-hash-tables") (reason upstream-source-defect) (expect missing-library)
   (note "in-progress/hash/tables.sld:67 imports (srfi 114 comparators), a name nothing provides: SRFI 114 says (srfi 114), its sample implementation says (comparators), and snow resolves the name only as the path srfi/114/comparators.sld, which no package ships. chibi fails identically; Gauche has (srfi 114) and still cannot load it"))
  ((slug "in-progress-hash-bimaps") (reason upstream-source-defect) (expect missing-library)
   (note "via (in-progress hash tables) — see in-progress-hash-tables. Its own .sld is clean; the bad name appears only in its package.scm metadata"))

  ;; Reached only since #383 bundled `(srfi 160 base)`; before that it stopped
  ;; at the missing library and this was invisible.
  ((slug "srfi-179") (reason upstream-source-defect) (expect parse-error)
   (note "srfi/179/transforms.scm:34 builds u1-storage-class from u1vector-ref and friends, which nothing defines: they are a chibi C extension (lib/srfi/160/uvprims.c), not part of SRFI 160, whose own reference implementation starts at u8"))
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
