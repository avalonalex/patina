# Patina third-party compatibility (vm backend)

**126 of 162 packages pass** — 126 of 160 achievable (excluding 2 out-of-scope pending FFI).

| Status | Packages |
|---|---|
| pass | 126 |
| missing-library | 16 |
| parse-error | 13 |
| load-error | 1 |
| unbound-identifier | 2 |
| wrong-result | 1 |
| runtime-error | 1 |
| timeout | 0 |
| out-of-scope | 2 |

## Missing libraries — the bundling work queue

| Library | Packages |
|---|---|
| (chibi) | 3 |
| (foreign c) | 2 |
| (srfi 114 comparators) | 2 |
| (srfi 144) | 2 |
| (chibi io) | 1 |
| (rebottled pregexp) | 1 |
| (retropikzel named-pipes) | 1 |
| (scheme flonum) | 1 |
| (srfi 160 base) | 1 |
| (srfi 165) | 1 |
| (srfi 231) | 1 |

## Parse errors

| Error | Packages |
|---|---|
| `Each syntax-rules rule must have exactly 2 elements (pattern template), in macro bytes-u8-set-all!` | 5 |
| `syntax-rules literals must be symbols` | 3 |
| `Body must contain at least one expression (not just define-syntax)` | 1 |
| `Failed to compile macro new-symbol?: Invalid syntax: Duplicate pattern variable: ch` | 1 |
| `Macro expansion failed: Invalid syntax: No matching pattern for macro case` | 1 |
| `Macro expansion failed: Invalid syntax: No matching pattern for macro ssax:make-parser/positional-args` | 1 |
| `unhandled exception: unhandled exception: #<unknown>` | 1 |

## Load errors

| Error | Packages |
|---|---|
| `Exported identifier 'mecab?' not defined` | 1 |

## Unbound identifiers

| Identifier | Packages |
|---|---|
| `load` | 1 |
| `read-padded-string` | 1 |
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
| chibi-binary-record | probe | pass |
| chibi-bytevector | test | parse-error |
| chibi-char-set | probe | pass |
| chibi-char-set-boundary | probe | pass |
| chibi-config | probe | pass |
| chibi-crypto-md5 | test | parse-error |
| chibi-crypto-rsa | test | parse-error |
| chibi-crypto-sha2 | test | parse-error |
| chibi-edit-distance | test | pass |
| chibi-html-parser | probe | pass |
| chibi-irregex | probe | pass |
| chibi-iset | test | pass |
| chibi-locale | test | pass |
| chibi-match | test | pass |
| chibi-math-linalg | test | missing-library |
| chibi-math-prime | test | pass |
| chibi-math-stats | test | missing-library |
| chibi-mecab | test | load-error |
| chibi-mime | test | pass |
| chibi-monad-environment | probe | parse-error |
| chibi-net-dns | test | out-of-scope |
| chibi-net-smtp | test | out-of-scope |
| chibi-parse | test | pass |
| chibi-pathname | test | pass |
| chibi-quoted-printable | test | pass |
| chibi-regexp | test | parse-error |
| chibi-scribble | test | pass |
| chibi-show | test | parse-error |
| chibi-snow-commands | probe | parse-error |
| chibi-ssl | test | missing-library |
| chibi-sxml | probe | pass |
| chibi-tar | test | unbound-identifier |
| chibi-temp-file | probe | pass |
| chibi-term-edit-line | probe | pass |
| chibi-uri | test | pass |
| chibi-voting | test | wrong-result |
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
| jkode-sassy | test | pass |
| lassik-dockerfile | test | pass |
| lassik-shell-quote | test | pass |
| lassik-string-inflection | test | pass |
| lassik-trivial-tar-writer | probe | pass |
| lassik-unpack-assoc | probe | pass |
| lightweight-testing | probe | pass |
| macduffie-json | probe | pass |
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
| slib-directory | probe | pass |
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
| slib-uri | probe | pass |
| slib-xml-parse | probe | pass |
| srfi-106 | probe | missing-library |
| srfi-11 | probe | pass |
| srfi-145 | probe | pass |
| srfi-156 | test | pass |
| srfi-16 | probe | pass |
| srfi-166 | probe | missing-library |
| srfi-170 | probe | missing-library |
| srfi-175 | test | pass |
| srfi-179 | test | missing-library |
| srfi-180 | probe | pass |
| srfi-19 | probe | pass |
| srfi-197 | test | runtime-error |
| srfi-2 | probe | pass |
| srfi-227 | probe | pass |
| srfi-235 | test | pass |
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
| srfi-78 | probe | pass |
| srfi-95 | probe | pass |
