# Patina third-party compatibility (vm backend)

**128 of 187 packages pass** — 128 of 186 achievable (excluding 1 out-of-scope pending FFI).

| Status | Packages |
|---|---|
| pass | 128 |
| missing-library | 29 |
| parse-error | 21 |
| load-error | 6 |
| unbound-identifier | 1 |
| wrong-result | 1 |
| runtime-error | 0 |
| timeout | 0 |
| out-of-scope | 1 |

## Missing libraries — the bundling work queue

| Library | Packages |
|---|---|
| (srfi 130) | 4 |
| (chibi) | 3 |
| (foreign c) | 2 |
| (srfi 114 comparators) | 2 |
| (srfi 13) | 2 |
| (srfi 64) | 2 |
| (chibi io) | 1 |
| (chibi irregex) | 1 |
| (macduffie json) | 1 |
| (rebottled pregexp) | 1 |
| (retropikzel named-pipes) | 1 |
| (scheme flonum) | 1 |
| (scheme hash-table) | 1 |
| (scheme small) | 1 |
| (srfi 142) | 1 |
| (srfi 144) | 1 |
| (srfi 160 base) | 1 |
| (srfi 2) | 1 |
| (srfi 23) | 1 |
| (srfi 231) | 1 |

## Parse errors

| Error | Packages |
|---|---|
| `LexError(UnexpectedChar('@'))` | 9 |
| `Each syntax-rules rule must have exactly 2 elements (pattern template)` | 5 |
| `syntax-rules literals must be symbols` | 3 |
| `#<unknown>` | 1 |
| `Duplicate parameter 'space' in lambda` | 1 |
| `Duplicate parameter 'symbol-first' in lambda` | 1 |
| `Name mismatch in interface implementation (expected, gotten) proc0 proc0` | 1 |

## Load errors

| Error | Packages |
|---|---|
| `Exported identifier 'duplicate-file-descriptor' not defined` | 4 |
| `Exported identifier 'begin' not defined` | 2 |

## Unbound identifiers

| Identifier | Packages |
|---|---|
| `srfi-128` | 1 |
| `test-exit` | 1 |
| `test-group` | 1 |
| `use` | 1 |

## Per-package matrix

| Package | Mode | Status |
|---|---|---|
| arvyy-interface | test | parse-error |
| arvyy-mustache | test | pass |
| chibi-app | test | parse-error |
| chibi-assert | test | missing-library |
| chibi-base64 | test | pass |
| chibi-binary-record | probe | missing-library |
| chibi-bytevector | test | parse-error |
| chibi-char-set | probe | pass |
| chibi-char-set-boundary | probe | pass |
| chibi-config | probe | pass |
| chibi-crypto-md5 | test | parse-error |
| chibi-crypto-rsa | test | parse-error |
| chibi-crypto-sha2 | test | parse-error |
| chibi-edit-distance | test | missing-library |
| chibi-filesystem | test | load-error |
| chibi-html-parser | probe | parse-error |
| chibi-irregex | probe | missing-library |
| chibi-iset | test | pass |
| chibi-locale | test | wrong-result |
| chibi-match | test | parse-error |
| chibi-math-linalg | test | missing-library |
| chibi-math-prime | test | pass |
| chibi-math-stats | test | missing-library |
| chibi-mecab | test | missing-library |
| chibi-mime | test | parse-error |
| chibi-monad-environment | probe | parse-error |
| chibi-net-dns | test | missing-library |
| chibi-net-smtp | test | out-of-scope |
| chibi-parse | test | parse-error |
| chibi-pathname | test | pass |
| chibi-quoted-printable | test | pass |
| chibi-regexp | test | parse-error |
| chibi-scribble | test | pass |
| chibi-show | test | parse-error |
| chibi-snow-commands | probe | parse-error |
| chibi-ssl | test | missing-library |
| chibi-string | test | pass |
| chibi-sxml | probe | parse-error |
| chibi-tar | test | missing-library |
| chibi-temp-file | probe | load-error |
| chibi-term-edit-line | probe | pass |
| chibi-uri | test | pass |
| chibi-voting | test | missing-library |
| chibi-xgboost | test | missing-library |
| chibi-xlib | probe | missing-library |
| chrisoei-cint | test | missing-library |
| chrisoei-test | probe | pass |
| comparators | test | unbound-identifier |
| edn | test | parse-error |
| generators | probe | pass |
| in-progress-hash-bimaps | test | missing-library |
| in-progress-hash-tables | test | missing-library |
| independentresearch-xattr | probe | missing-library |
| jkode-sassy | test | missing-library |
| lassik-dockerfile | test | parse-error |
| lassik-shell-quote | test | parse-error |
| lassik-string-inflection | test | missing-library |
| lassik-trivial-tar-writer | probe | pass |
| lassik-unpack-assoc | probe | pass |
| lightweight-testing | probe | pass |
| macduffie-json | probe | missing-library |
| okmij-ssax | test | parse-error |
| pfds-alist | probe | pass |
| pfds-bitwise | probe | pass |
| pfds-bounded-balance-tree | probe | pass |
| pfds-deque | probe | pass |
| pfds-difference-list | probe | pass |
| pfds-fector | probe | pass |
| pfds-fingertree | probe | pass |
| pfds-hash-array-mapped-trie | probe | pass |
| pfds-heap | probe | pass |
| pfds-lazy-list | probe | pass |
| pfds-list-helpers | probe | pass |
| pfds-priority-search-queue | probe | pass |
| pfds-queue | probe | pass |
| pfds-sequence | probe | pass |
| pfds-set | probe | pass |
| pfds-vector | probe | pass |
| postgresql | probe | parse-error |
| r6rs-arithmetic-fixnums | probe | load-error |
| r6rs-base | probe | load-error |
| r6rs-bytevectors | probe | pass |
| r6rs-control | probe | pass |
| r6rs-enums | probe | pass |
| r6rs-eval | probe | pass |
| r6rs-exceptions | probe | pass |
| r6rs-files | probe | pass |
| r6rs-hashtables | probe | pass |
| r6rs-io-simple | probe | pass |
| r6rs-lists | probe | pass |
| r6rs-mutable-pairs | probe | pass |
| r6rs-mutable-strings | probe | pass |
| r6rs-programs | probe | pass |
| r6rs-r5rs | probe | pass |
| r6rs-sorting | probe | pass |
| r6rs-unicode | probe | pass |
| r6rs-unicode-reference-unicode0 | probe | pass |
| r6rs-unicode-reference-unicode1 | probe | pass |
| r6rs-unicode-reference-unicode2 | probe | pass |
| r6rs-unicode-reference-unicode3 | probe | pass |
| r6rs-unicode-reference-unicode4 | probe | pass |
| rebottled-cl-pdf | probe | missing-library |
| rebottled-pstk | probe | pass |
| retropikzel-pstk | probe | missing-library |
| slib-alist | probe | pass |
| slib-array-for-each | probe | pass |
| slib-array-interpolate | probe | pass |
| slib-byte | probe | pass |
| slib-byte-number | probe | pass |
| slib-chapter-order | probe | pass |
| slib-charplot | probe | pass |
| slib-coerce | probe | pass |
| slib-color | probe | pass |
| slib-color-space | probe | pass |
| slib-common | probe | pass |
| slib-common-lisp-time | probe | pass |
| slib-common-list-functions | probe | pass |
| slib-daylight | probe | pass |
| slib-determinant | probe | pass |
| slib-directory | probe | load-error |
| slib-dynamic | probe | pass |
| slib-factor | probe | pass |
| slib-filename | probe | pass |
| slib-format | probe | pass |
| slib-fourier-transform | probe | pass |
| slib-generic-write | probe | pass |
| slib-line-io | probe | pass |
| slib-math-integer | probe | pass |
| slib-math-real | probe | pass |
| slib-minimize | probe | pass |
| slib-modular | probe | pass |
| slib-nbs-iscc | probe | pass |
| slib-posix-time | probe | pass |
| slib-pprint-file | probe | pass |
| slib-pretty-print | probe | pass |
| slib-printf | probe | pass |
| slib-queue | probe | pass |
| slib-random-inexact | probe | pass |
| slib-rationalize | probe | pass |
| slib-resene | probe | pass |
| slib-rev2-procedures | probe | pass |
| slib-saturate | probe | pass |
| slib-scanf | probe | pass |
| slib-soundex | probe | pass |
| slib-string-case | probe | pass |
| slib-string-port | probe | pass |
| slib-string-search | probe | pass |
| slib-subarray | probe | pass |
| slib-time-core | probe | pass |
| slib-time-zone | probe | pass |
| slib-topological-sort | probe | pass |
| slib-tree | probe | pass |
| slib-tzfile | probe | pass |
| slib-uri | probe | load-error |
| slib-xml-parse | probe | parse-error |
| srfi-106 | probe | missing-library |
| srfi-11 | probe | pass |
| srfi-14 | probe | pass |
| srfi-145 | probe | pass |
| srfi-156 | test | pass |
| srfi-16 | probe | pass |
| srfi-166 | probe | missing-library |
| srfi-170 | probe | missing-library |
| srfi-175 | test | pass |
| srfi-179 | test | missing-library |
| srfi-180 | probe | pass |
| srfi-19 | probe | pass |
| srfi-197 | test | missing-library |
| srfi-2 | probe | pass |
| srfi-227 | probe | pass |
| srfi-235 | test | missing-library |
| srfi-25 | probe | pass |
| srfi-26 | probe | pass |
| srfi-28 | probe | pass |
| srfi-29 | probe | pass |
| srfi-31 | probe | pass |
| srfi-37 | probe | pass |
| srfi-38 | probe | pass |
| srfi-39 | probe | pass |
| srfi-41 | probe | pass |
| srfi-42 | probe | pass |
| srfi-43 | probe | pass |
| srfi-51 | probe | pass |
| srfi-63 | probe | pass |
| srfi-64 | test | pass |
| srfi-78 | probe | missing-library |
| srfi-95 | probe | pass |

