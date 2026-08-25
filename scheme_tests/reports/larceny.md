# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-08-25 08:20:10  
**Backend:** VM  
**Lane:** tests/scheme (R7RS-small + Red edition)  
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 16 of 33 |
| Assertions passed | 5306 of 5334 (99.5%) |
| Suites not reaching a tally | 9 |

A suite that cannot load reaches no tally, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.

## Library under test not bundled (9)

Bundling work, not defects — each is a Red-edition library Patina does not ship yet.

| Suite | Missing |
|---|---|
| ephemeron | `(scheme ephemeron)` |
| flonum | `(srfi 144)` |
| ideque | `(scheme ideque)` |
| ilist | `(scheme ilist)` |
| list-queue | `(scheme list-queue)` |
| lseq | `(scheme lseq)` |
| rlist | `(scheme rlist)` |
| stream | `(scheme stream)` |
| text | `(scheme text)` |

## Assertion failures (28 in 8 suites)

Each entry links to the test case; the name after it is the procedure the assertion exercises.

### base — 18 of 1064 failed

- [base.sld:730](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L730) — `values` `list`
- (not located) — `v`
- (not located) — `v`
- [base.sld:918](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L918) — `name` `unquote`
- [base.sld:921](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L921) — `name` `foo`
- [base.sld:924](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L924) — `append` `sqrt`
- [base.sld:927](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L927) — `foo` `unquote`
- [base.sld:1003](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L1003) — `define` `let-syntax`
- [base.sld:1014](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L1014) — `define` `letrec-syntax`
- [base.sld:1118](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L1118) — `let-syntax` `syntax-rules`
- [base.sld:1126](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L1126) — `letrec-syntax` `syntax-rules`
- (not located) — `vector-copy!`
- (not located) — `a`
- [base.sld:2346](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2346) — `bytevector-u8-ref` `make-bytevector`
- [base.sld:2347](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2347) — `bytevector-u8-ref` `make-bytevector`
- [base.sld:2741](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2741) — `read-error?` `with-exception-handler`
- [base.sld:2748](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2748) — `file-error?` `with-exception-handler`
- [base.sld:2805](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2805) — `map` `call-with-port`

### char — 1 of 139 failed

- [char.body.scm:109](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/char.body.scm#L109) — `chars` `filter-all-chars`

### charset — 2 of 93 failed

- [charset.sld:108](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/charset.sld#L108) — `>=` `char-set-size`
- [charset.sld:136](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/charset.sld#L136) — `cs` `char-set`

### complex — 1 of 61 failed

- [complex.body.scm:89](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.body.scm#L89) — `log`

### eval — 1 of 3 failed

- [eval.sld:33](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/eval.sld#L33) — `eval` `eval:car`

### inexact — 1 of 592 failed

- [inexact.sld:361](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L361) — `sqrt`

### list — 1 of 172 failed

- [list.sld:589](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L589) — `list` `cells`

### write — 3 of 40 failed

- [write.sld:368](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/write.sld#L368) — `write-data` `map`
- [write.sld:373](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/write.sld#L373) — `write-data` `map`
- [write.sld:378](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/write.sld#L378) — `write-data` `map`

## All suites

| Suite | Status | Passed | Total |
|---|---|---|---|
| base | fail | 1046 | 1064 |
| box | pass | 10 | 10 |
| case-lambda | pass | 5 | 5 |
| char | fail | 138 | 139 |
| charset | fail | 91 | 93 |
| comparator | pass | 158 | 158 |
| complex | fail | 60 | 61 |
| cxr | pass | 28 | 28 |
| ephemeron | not-bundled | 0 | 0 |
| eval | fail | 2 | 3 |
| file | pass | 75 | 75 |
| flonum | not-bundled | 0 | 0 |
| generator | pass | 49 | 49 |
| hash-table | pass | 82 | 82 |
| ideque | not-bundled | 0 | 0 |
| ilist | not-bundled | 0 | 0 |
| inexact | fail | 591 | 592 |
| lazy | pass | 33 | 33 |
| list-queue | not-bundled | 0 | 0 |
| list | fail | 171 | 172 |
| load | pass | 1 | 1 |
| lseq | not-bundled | 0 | 0 |
| process-context | pass | 2 | 2 |
| read | pass | 44 | 44 |
| repl | pass | 0 | 0 |
| rlist | not-bundled | 0 | 0 |
| set | pass | 16 | 16 |
| sort | pass | 2562 | 2562 |
| stream | not-bundled | 0 | 0 |
| text | not-bundled | 0 | 0 |
| time | pass | 2 | 2 |
| vector | pass | 103 | 103 |
| write | fail | 37 | 40 |
