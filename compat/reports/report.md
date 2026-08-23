# Patina third-party compatibility (vm backend)

**126 of 162 packages pass.**

**126 of 137 in scope** — 25 packages are excluded from the score by `compat/EXCLUSIONS.scm`, each for a reason that is not a measurement of Patina. The raw number above never moves because of that file.

| Status | Packages | In scope |
|---|---|---|
| pass | 126 | 126 |
| missing-library | 11 | 7 |
| parse-error | 13 | 3 |
| load-error | 0 | 0 |
| unbound-identifier | 2 | 1 |
| wrong-result | 1 | 0 |
| runtime-error | 1 | 0 |
| timeout | 0 | 0 |
| out-of-scope | 8 | 0 |

## Missing libraries — the bundling work queue

| Library | In-scope packages |
|---|---|
| (srfi 114 comparators) | 2 |
| (srfi 144) | 2 |
| (scheme flonum) | 1 |
| (srfi 165) | 1 |
| (srfi 231) | 1 |

## Parse errors

| Error | In-scope packages |
|---|---|
| `Body must contain at least one expression (not just define-syntax)` | 1 |
| `Macro expansion failed: Invalid syntax: No matching pattern for macro ssax:make-parser/positional-args` | 1 |
| `unhandled exception: unhandled exception: #<unknown>` | 1 |

## Unbound identifiers

| Identifier | In-scope packages |
|---|---|
| `read-padded-string` | 1 |

## Excluded from the score

These packages still run on every pass — exclusion decides whether a result counts, never whether it is measured, and `results.scm` records them exactly as it records everything else.

### Needs a foreign-function interface (9)

| Package | Status | Why |
|---|---|---|
| chibi-mecab | out-of-scope | chibi/mecab.sld:38 (include-shared "mecab") — bindings to libmecab |
| chibi-net-dns | out-of-scope | needs (chibi net), which is C-backed upstream |
| chibi-net-smtp | out-of-scope | needs (chibi net), which is C-backed upstream |
| chibi-ssl | out-of-scope | chibi/ssl.sld:14 (include-shared "ssl") — bindings to OpenSSL |
| chibi-xgboost | missing-library | chibi/xgboost.sld:5 (include-shared "xgboost/xgboost"); reports (srfi 160 base) because its test library's import fails before (chibi xgboost) is reached, so the FFI need is real but shadowed |
| chibi-xlib | out-of-scope | chibi/xlib.sld:45 (include-shared "xlib") — bindings to Xlib |
| independentresearch-xattr | out-of-scope | independentresearch/xattr.sld:9 (include-shared "xattr") — POSIX extended attributes |
| srfi-106 | out-of-scope | srfi/106.sld:6 imports (foreign c), chibi's FFI interface library |
| srfi-170 | out-of-scope | srfi/170.sld:8 imports (foreign c); a POSIX API over chibi's FFI |

### Dependency not vendored (corpus licence policy) (2)

| Package | Status | Why |
|---|---|---|
| rebottled-cl-pdf | missing-library | needs (rebottled pregexp); REVIEW-QUEUE.json has it under UNKNOWN licence, so it is not vendored |
| retropikzel-pstk | missing-library | needs (retropikzel named-pipes); REVIEW-QUEUE.json has it under UNKNOWN licence, so it is not vendored |

### Upstream source defect (10)

| Package | Status | Why |
|---|---|---|
| chibi-app | parse-error | app.scm:467 — an else clause mid-case, followed by ((1) ...); R7RS puts else last and Gauche rejects it. Patina still owes a better message than "No matching pattern for macro case" with no location — that part is ours, tracked in PRD §6 |
| chibi-bytevector | parse-error | ieee-754.scm:16 — bytes-u8-set-all! has a 4-element syntax-rules rule ((_) bv off i), a parenthesization typo; Gauche: "malformed macro" |
| chibi-crypto-md5 | parse-error | via (chibi bytevector) — see chibi-bytevector |
| chibi-crypto-rsa | parse-error | via (chibi bytevector) — see chibi-bytevector |
| chibi-crypto-sha2 | parse-error | via (chibi bytevector) — see chibi-bytevector; its own include-shared is behind a chibi-only branch and is not the blocker |
| chibi-monad-environment | parse-error | environment.sld:6 — (syntax-rules ((_ x) 'x)) has no literals list; Gauche: "literal list contains non-symbol" |
| chibi-show | parse-error | via (chibi monad environment) — see chibi-monad-environment |
| chibi-snow-commands | parse-error | via (chibi monad environment) — see chibi-monad-environment |
| edn | parse-error | (chibi parse) parse.sld:66 — the fallback grammar-bind generates a pattern with `ch` twice; duplicate pattern variables are an error (R7RS 4.3.2) and Gauche fails edn end-to-end as we do |
| postgresql | parse-error | via (chibi bytevector) — see chibi-bytevector |

### Upstream test defect (4)

| Package | Status | Why |
|---|---|---|
| chibi-assert | missing-library | (chibi assert) itself loads and runs on Patina — its cond-expand else branch is portable — but chibi/assert-test.sld:2 imports (chibi), chibi's implementation core, for protect and exception-irritants |
| chibi-voting | wrong-result | instant-runoff-rank's expectation depends on hash-table iteration order, which no standard specifies; chibi, Gauche and Patina each produce a different ranking and Gauche fails the suite as we do |
| comparators | unbound-identifier | srfi-128/comparators/comparators-test.scm opens with (use test) (use srfi-128) — CHICKEN syntax, not R7RS |
| srfi-197 | runtime-error | its test program (include "./test.scm")s a file the package does not ship |

## Per-package matrix

| Package | Mode | Status | Scope |
|---|---|---|---|
| arvyy-interface | test | parse-error | in scope |
| arvyy-mustache | test | pass | in scope |
| chibi-app | test | parse-error | upstream-source-defect |
| chibi-assert | test | missing-library | upstream-test-defect |
| chibi-base64 | test | pass | in scope |
| chibi-binary-record | probe | pass | in scope |
| chibi-bytevector | test | parse-error | upstream-source-defect |
| chibi-char-set | probe | pass | in scope |
| chibi-char-set-boundary | probe | pass | in scope |
| chibi-config | probe | pass | in scope |
| chibi-crypto-md5 | test | parse-error | upstream-source-defect |
| chibi-crypto-rsa | test | parse-error | upstream-source-defect |
| chibi-crypto-sha2 | test | parse-error | upstream-source-defect |
| chibi-edit-distance | test | pass | in scope |
| chibi-html-parser | probe | pass | in scope |
| chibi-irregex | probe | pass | in scope |
| chibi-iset | test | pass | in scope |
| chibi-locale | test | pass | in scope |
| chibi-match | test | pass | in scope |
| chibi-math-linalg | test | missing-library | in scope |
| chibi-math-prime | test | pass | in scope |
| chibi-math-stats | test | missing-library | in scope |
| chibi-mecab | test | out-of-scope | ffi |
| chibi-mime | test | pass | in scope |
| chibi-monad-environment | probe | parse-error | upstream-source-defect |
| chibi-net-dns | test | out-of-scope | ffi |
| chibi-net-smtp | test | out-of-scope | ffi |
| chibi-parse | test | pass | in scope |
| chibi-pathname | test | pass | in scope |
| chibi-quoted-printable | test | pass | in scope |
| chibi-regexp | test | parse-error | in scope |
| chibi-scribble | test | pass | in scope |
| chibi-show | test | parse-error | upstream-source-defect |
| chibi-snow-commands | probe | parse-error | upstream-source-defect |
| chibi-ssl | test | out-of-scope | ffi |
| chibi-sxml | probe | pass | in scope |
| chibi-tar | test | unbound-identifier | in scope |
| chibi-temp-file | probe | pass | in scope |
| chibi-term-edit-line | probe | pass | in scope |
| chibi-uri | test | pass | in scope |
| chibi-voting | test | wrong-result | upstream-test-defect |
| chibi-xgboost | test | missing-library | ffi |
| chibi-xlib | probe | out-of-scope | ffi |
| chrisoei-cint | test | missing-library | in scope |
| chrisoei-test | probe | pass | in scope |
| comparators | test | unbound-identifier | upstream-test-defect |
| edn | test | parse-error | upstream-source-defect |
| generators | probe | pass | in scope |
| in-progress-hash-bimaps | test | missing-library | in scope |
| in-progress-hash-tables | test | missing-library | in scope |
| independentresearch-xattr | probe | out-of-scope | ffi |
| jkode-sassy | test | pass | in scope |
| lassik-dockerfile | test | pass | in scope |
| lassik-shell-quote | test | pass | in scope |
| lassik-string-inflection | test | pass | in scope |
| lassik-trivial-tar-writer | probe | pass | in scope |
| lassik-unpack-assoc | probe | pass | in scope |
| lightweight-testing | probe | pass | in scope |
| macduffie-json | probe | pass | in scope |
| okmij-ssax | test | parse-error | in scope |
| pfds-alist | probe | pass | in scope |
| pfds-bitwise | probe | pass | in scope |
| pfds-bounded-balance-tree | probe | pass | in scope |
| pfds-deque | probe | pass | in scope |
| pfds-difference-list | probe | pass | in scope |
| pfds-fector | probe | pass | in scope |
| pfds-fingertree | probe | pass | in scope |
| pfds-hash-array-mapped-trie | probe | pass | in scope |
| pfds-heap | probe | pass | in scope |
| pfds-lazy-list | probe | pass | in scope |
| pfds-list-helpers | probe | pass | in scope |
| pfds-priority-search-queue | probe | pass | in scope |
| pfds-queue | probe | pass | in scope |
| pfds-sequence | probe | pass | in scope |
| pfds-set | probe | pass | in scope |
| pfds-vector | probe | pass | in scope |
| postgresql | probe | parse-error | upstream-source-defect |
| rebottled-cl-pdf | probe | missing-library | dependency-not-vendored |
| rebottled-pstk | probe | pass | in scope |
| retropikzel-pstk | probe | missing-library | dependency-not-vendored |
| slib-alist | probe | pass | in scope |
| slib-array-for-each | probe | pass | in scope |
| slib-array-interpolate | probe | pass | in scope |
| slib-byte | probe | pass | in scope |
| slib-byte-number | probe | pass | in scope |
| slib-chapter-order | probe | pass | in scope |
| slib-charplot | probe | pass | in scope |
| slib-coerce | probe | pass | in scope |
| slib-color | probe | pass | in scope |
| slib-color-space | probe | pass | in scope |
| slib-common | probe | pass | in scope |
| slib-common-lisp-time | probe | pass | in scope |
| slib-common-list-functions | probe | pass | in scope |
| slib-daylight | probe | pass | in scope |
| slib-determinant | probe | pass | in scope |
| slib-directory | probe | pass | in scope |
| slib-dynamic | probe | pass | in scope |
| slib-factor | probe | pass | in scope |
| slib-filename | probe | pass | in scope |
| slib-format | probe | pass | in scope |
| slib-fourier-transform | probe | pass | in scope |
| slib-generic-write | probe | pass | in scope |
| slib-line-io | probe | pass | in scope |
| slib-math-integer | probe | pass | in scope |
| slib-math-real | probe | pass | in scope |
| slib-minimize | probe | pass | in scope |
| slib-modular | probe | pass | in scope |
| slib-nbs-iscc | probe | pass | in scope |
| slib-posix-time | probe | pass | in scope |
| slib-pprint-file | probe | pass | in scope |
| slib-pretty-print | probe | pass | in scope |
| slib-printf | probe | pass | in scope |
| slib-queue | probe | pass | in scope |
| slib-random-inexact | probe | pass | in scope |
| slib-rationalize | probe | pass | in scope |
| slib-resene | probe | pass | in scope |
| slib-rev2-procedures | probe | pass | in scope |
| slib-saturate | probe | pass | in scope |
| slib-scanf | probe | pass | in scope |
| slib-soundex | probe | pass | in scope |
| slib-string-case | probe | pass | in scope |
| slib-string-port | probe | pass | in scope |
| slib-string-search | probe | pass | in scope |
| slib-subarray | probe | pass | in scope |
| slib-time-core | probe | pass | in scope |
| slib-time-zone | probe | pass | in scope |
| slib-topological-sort | probe | pass | in scope |
| slib-tree | probe | pass | in scope |
| slib-tzfile | probe | pass | in scope |
| slib-uri | probe | pass | in scope |
| slib-xml-parse | probe | pass | in scope |
| srfi-106 | probe | out-of-scope | ffi |
| srfi-11 | probe | pass | in scope |
| srfi-145 | probe | pass | in scope |
| srfi-156 | test | pass | in scope |
| srfi-16 | probe | pass | in scope |
| srfi-166 | probe | missing-library | in scope |
| srfi-170 | probe | out-of-scope | ffi |
| srfi-175 | test | pass | in scope |
| srfi-179 | test | missing-library | in scope |
| srfi-180 | probe | pass | in scope |
| srfi-19 | probe | pass | in scope |
| srfi-197 | test | runtime-error | upstream-test-defect |
| srfi-2 | probe | pass | in scope |
| srfi-227 | probe | pass | in scope |
| srfi-235 | test | pass | in scope |
| srfi-25 | probe | pass | in scope |
| srfi-26 | probe | pass | in scope |
| srfi-28 | probe | pass | in scope |
| srfi-29 | probe | pass | in scope |
| srfi-31 | probe | pass | in scope |
| srfi-37 | probe | pass | in scope |
| srfi-38 | probe | pass | in scope |
| srfi-39 | probe | pass | in scope |
| srfi-41 | probe | pass | in scope |
| srfi-42 | probe | pass | in scope |
| srfi-43 | probe | pass | in scope |
| srfi-51 | probe | pass | in scope |
| srfi-63 | probe | pass | in scope |
| srfi-64 | test | pass | in scope |
| srfi-78 | probe | pass | in scope |
| srfi-95 | probe | pass | in scope |
