# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-08-24 18:13:24  
**Backend:** VM  
**Lane:** tests/scheme (R7RS-small + Red edition)  
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 13 of 33 |
| Assertions passed | 4172 of 4193 (99.5%) |
| Suites not reaching a tally | 12 |

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

## Crashed or hung (2)

The process died or was cut off by the timeout before reporting; the defect is in Patina's runtime, not in an assertion.

| Suite | What |
|---|---|
| lazy | stack overflow |
| read | stack overflow |

## Failed to load (1)

The suite's library did not compile, so nothing in it ran. Patina's message:

| Suite | Message |
|---|---|
| base | `Error: Parse error in tests/scheme/base.sld: desugar error: Invalid syntax: invalid use of syntax as a value: `...` is a syntactic keyword` |

## Assertion failures (21 in 8 suites)

Each entry links to the test case; the name after it is the procedure the assertion exercises.

### char — 2 of 139 failed

- [char.body.scm:109](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/char.body.scm#L109) — `chars` `filter-all-chars`
- [char.body.scm:123](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/char.body.scm#L123) — `chars` `filter-all-chars`

### charset — 2 of 93 failed

- [charset.sld:108](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/charset.sld#L108) — `>=` `char-set-size`
- [charset.sld:136](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/charset.sld#L136) — `cs` `char-set`

### complex — 3 of 61 failed

- [complex.sld:55](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.sld#L55) — `string->number` `number->string`
- [complex.sld:56](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.sld#L56) — `string->number` `number->string`
- [complex.body.scm:89](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.body.scm#L89) — `log`

### eval — 1 of 3 failed

- [eval.sld:33](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/eval.sld#L33) — `eval` `eval:car`

### inexact — 8 of 592 failed

- [inexact.sld:361](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L361) — `sqrt`
- [inexact.sld:405](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L405) — `string->number`
- [inexact.sld:406](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L406) — `string->number`
- [inexact.sld:407](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L407) — `string->number`
- [inexact.sld:91](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L91) — `string->number`
- [inexact.sld:95](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L95) — `string->number`
- [inexact.sld:388](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L388) — `string->number`
- [inexact.sld:389](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L389) — `string->number`

### list — 1 of 172 failed

- [list.sld:589](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L589) — `list` `cells`

### vector — 1 of 103 failed

- [vector.sld:118](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/vector.sld#L118) — `'#((0 . 4) (1 . 3) (2 . `

### write — 3 of 40 failed

- [write.sld:368](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/write.sld#L368) — `write-data` `map`
- [write.sld:373](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/write.sld#L373) — `write-data` `map`
- [write.sld:378](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/write.sld#L378) — `write-data` `map`

## All suites

| Suite | Status | Passed | Total |
|---|---|---|---|
| base | load-error | 0 | 0 |
| box | pass | 10 | 10 |
| case-lambda | pass | 5 | 5 |
| char | fail | 137 | 139 |
| charset | fail | 91 | 93 |
| comparator | pass | 158 | 158 |
| complex | fail | 58 | 61 |
| cxr | pass | 28 | 28 |
| ephemeron | not-bundled | 0 | 0 |
| eval | fail | 2 | 3 |
| file | pass | 75 | 75 |
| flonum | not-bundled | 0 | 0 |
| generator | pass | 49 | 49 |
| hash-table | pass | 82 | 82 |
| ideque | not-bundled | 0 | 0 |
| ilist | not-bundled | 0 | 0 |
| inexact | fail | 584 | 592 |
| lazy | crash | 0 | 0 |
| list-queue | not-bundled | 0 | 0 |
| list | fail | 171 | 172 |
| load | pass | 1 | 1 |
| lseq | not-bundled | 0 | 0 |
| process-context | pass | 2 | 2 |
| read | crash | 0 | 0 |
| repl | pass | 0 | 0 |
| rlist | not-bundled | 0 | 0 |
| set | pass | 16 | 16 |
| sort | pass | 2562 | 2562 |
| stream | not-bundled | 0 | 0 |
| text | not-bundled | 0 | 0 |
| time | pass | 2 | 2 |
| vector | fail | 102 | 103 |
| write | fail | 37 | 40 |
