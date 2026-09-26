# Patina third-party compatibility (vm backend)

**Measured:** 2026-09-26T16:18:10Z

**143 of 161 packages pass.**

**143 of 143 in scope** — 18 packages are excluded from the score by `compat/EXCLUSIONS.scm`, each for a reason that is not a measurement of Patina. The raw number above never moves because of that file.

| Status | Packages | In scope |
|---|---|---|
| pass | 143 | 143 |
| missing-library | 4 | 0 |
| parse-error | 2 | 0 |
| load-error | 0 | 0 |
| unbound-identifier | 0 | 0 |
| wrong-result | 1 | 0 |
| runtime-error | 1 | 0 |
| timeout | 0 | 0 |
| out-of-scope | 10 | 0 |

## Excluded from the score

These packages still run on every pass — exclusion decides whether a result counts, never whether it is measured, and `results.scm` records them exactly as it records everything else.

### Needs a foreign-function interface (11)

| Package | Status | Why |
|---|---|---|
| chibi-mecab | out-of-scope | chibi/mecab.sld:38 (include-shared "mecab") — bindings to libmecab |
| chibi-net-dns | out-of-scope | needs (chibi net), which is C-backed upstream |
| chibi-net-smtp | out-of-scope | needs (chibi net), which is C-backed upstream |
| chibi-snow-commands | missing-library | needs (chibi net http), C-backed upstream like the rest of (chibi net); (srfi 18) threads is wanted behind it |
| chibi-ssl | out-of-scope | chibi/ssl.sld:14 (include-shared "ssl") — bindings to OpenSSL |
| chibi-xgboost | out-of-scope | chibi/xgboost.sld:5 (include-shared "xgboost/xgboost") — bindings to libxgboost, reported directly since #383 bundled the (srfi 160 base) that used to shadow it |
| chibi-xlib | out-of-scope | chibi/xlib.sld:45 (include-shared "xlib") — bindings to Xlib |
| independentresearch-xattr | out-of-scope | independentresearch/xattr.sld:9 (include-shared "xattr") — POSIX extended attributes |
| postgresql | out-of-scope | imports (foreign c); reported directly since compat/patches/chibi-bytevector.patch removed the (chibi bytevector) typo that used to stop it first |
| srfi-106 | out-of-scope | srfi/106.sld:6 imports (foreign c), chibi's FFI interface library |
| srfi-170 | out-of-scope | srfi/170.sld:8 imports (foreign c); a POSIX API over chibi's FFI |

### Dependency not vendored (corpus licence policy) (2)

| Package | Status | Why |
|---|---|---|
| rebottled-cl-pdf | missing-library | needs (rebottled pregexp); REVIEW-QUEUE.json has it under UNKNOWN licence, so it is not vendored |
| retropikzel-pstk | missing-library | needs (retropikzel named-pipes); REVIEW-QUEUE.json has it under UNKNOWN licence, so it is not vendored |

### Upstream source defect (2)

| Package | Status | Why |
|---|---|---|
| edn | parse-error | (chibi parse) parse.sld:66 — the fallback grammar-bind generates a pattern with `ch` twice; duplicate pattern variables are an error (R7RS 4.3.2) and Gauche fails edn end-to-end as we do |
| srfi-179 | parse-error | srfi/179/transforms.scm:34 builds u1-storage-class from u1vector-ref and friends, which nothing defines: they are a chibi C extension (lib/srfi/160/uvprims.c), not part of SRFI 160, whose own reference implementation starts at u8 |

### Upstream test defect (3)

| Package | Status | Why |
|---|---|---|
| chibi-assert | missing-library | (chibi assert) itself loads and runs on Patina — its cond-expand else branch is portable — but chibi/assert-test.sld:2 imports (chibi), chibi's implementation core, for protect and exception-irritants. Not patched, measured 2026-09-19: with those rewritten to guard and error-object-irritants it is 3 of 4, and the fourth cannot pass off chibi — the portable branch reports the datum inside 'three as a free variable and raises unbound variable, on Gauche exactly as on Patina |
| chibi-voting | wrong-result | instant-runoff-rank's expectation depends on hash-table iteration order, which no standard specifies; chibi, Gauche and Patina each produce a different ranking and Gauche fails the suite as we do |
| srfi-197 | runtime-error | its test program (include "./test.scm")s a file the package does not ship |

## Per-package matrix

| Package | Mode | Status | Scope |
|---|---|---|---|
| arvyy-interface | test | pass | in scope |
| arvyy-mustache | test | pass | in scope |
| chibi-app | test | pass | in scope |
| chibi-assert | test | missing-library | upstream-test-defect |
| chibi-base64 | test | pass | in scope |
| chibi-binary-record | probe | pass | in scope |
| chibi-bytevector | test | pass | in scope |
| chibi-char-set | probe | pass | in scope |
| chibi-char-set-boundary | probe | pass | in scope |
| chibi-config | probe | pass | in scope |
| chibi-crypto-md5 | test | pass | in scope |
| chibi-crypto-rsa | test | pass | in scope |
| chibi-crypto-sha2 | test | pass | in scope |
| chibi-edit-distance | test | pass | in scope |
| chibi-html-parser | probe | pass | in scope |
| chibi-irregex | probe | pass | in scope |
| chibi-iset | test | pass | in scope |
| chibi-locale | test | pass | in scope |
| chibi-match | test | pass | in scope |
| chibi-math-linalg | test | pass | in scope |
| chibi-math-prime | test | pass | in scope |
| chibi-math-stats | test | pass | in scope |
| chibi-mecab | test | out-of-scope | ffi |
| chibi-mime | test | pass | in scope |
| chibi-monad-environment | probe | pass | in scope |
| chibi-net-dns | test | out-of-scope | ffi |
| chibi-net-smtp | test | out-of-scope | ffi |
| chibi-parse | test | pass | in scope |
| chibi-pathname | test | pass | in scope |
| chibi-quoted-printable | test | pass | in scope |
| chibi-regexp | test | pass | in scope |
| chibi-scribble | test | pass | in scope |
| chibi-show | test | pass | in scope |
| chibi-snow-commands | probe | missing-library | ffi |
| chibi-ssl | test | out-of-scope | ffi |
| chibi-sxml | probe | pass | in scope |
| chibi-tar | test | pass | in scope |
| chibi-temp-file | probe | pass | in scope |
| chibi-term-edit-line | probe | pass | in scope |
| chibi-uri | test | pass | in scope |
| chibi-voting | test | wrong-result | upstream-test-defect |
| chibi-xgboost | test | out-of-scope | ffi |
| chibi-xlib | probe | out-of-scope | ffi |
| chrisoei-cint | test | pass | in scope |
| chrisoei-test | probe | pass | in scope |
| comparators | test | pass | in scope |
| edn | test | parse-error | upstream-source-defect |
| generators | probe | pass | in scope |
| in-progress-hash-bimaps | test | pass | in scope |
| in-progress-hash-tables | test | pass | in scope |
| independentresearch-xattr | probe | out-of-scope | ffi |
| jkode-sassy | test | pass | in scope |
| lassik-dockerfile | test | pass | in scope |
| lassik-shell-quote | test | pass | in scope |
| lassik-string-inflection | test | pass | in scope |
| lassik-trivial-tar-writer | probe | pass | in scope |
| lassik-unpack-assoc | probe | pass | in scope |
| lightweight-testing | probe | pass | in scope |
| macduffie-json | probe | pass | in scope |
| okmij-ssax | test | pass | in scope |
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
| postgresql | probe | out-of-scope | ffi |
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
| srfi-166 | probe | pass | in scope |
| srfi-170 | probe | out-of-scope | ffi |
| srfi-175 | test | pass | in scope |
| srfi-179 | test | parse-error | upstream-source-defect |
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
| srfi-42 | probe | pass | in scope |
| srfi-43 | probe | pass | in scope |
| srfi-51 | probe | pass | in scope |
| srfi-63 | probe | pass | in scope |
| srfi-64 | test | pass | in scope |
| srfi-78 | probe | pass | in scope |
| srfi-95 | probe | pass | in scope |
