# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-08-24 14:03:46  
**Backend:** tree-walker  
**Lane:** tests/scheme (R7RS-small + Red edition)  
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 12 of 33 |
| Assertions passed | 3931 of 3961 (99.2%) |
| Suites not reaching a tally | 14 |

A suite that cannot load reaches no tally, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.

## Library under test not bundled (10)

Bundling work, not defects — each is a Red-edition library Patina does not ship yet.

| Suite | Missing |
|---|---|
| charset | `(scheme charset)` |
| ephemeron | `(scheme ephemeron)` |
| flonum | `(srfi 144)` |
| ideque | `(scheme ideque)` |
| ilist | `(scheme ilist)` |
| list-queue | `(scheme list-queue)` |
| lseq | `(scheme lseq)` |
| rlist | `(scheme rlist)` |
| stream | `(scheme stream)` |
| text | `(scheme text)` |

## Crashed or hung (3)

The process died or was cut off by the timeout before reporting; the defect is in Patina's runtime, not in an assertion.

| Suite | What |
|---|---|
| char | stack overflow |
| lazy | stack overflow |
| read | stack overflow |

## Failed to load (1)

The suite's library did not compile, so nothing in it ran. Patina's message:

| Suite | Message |
|---|---|
| base | `Error: Invalid syntax: Failed to load library: Parse error in tests/scheme/base.sld: Failed to desugar expression: Invalid syntax: include: cannot read 'base-te` |

## Assertion failures (30 in 7 suites)

Each entry links to the test case; the name after it is the procedure the assertion exercises.

### complex — 3 of 61 failed

- [complex.sld:55](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.sld#L55) — `string->number` `number->string`
- [complex.sld:56](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.sld#L56) — `string->number` `number->string`
- [complex.body.scm:89](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.body.scm#L89) — `log`

### eval — 1 of 3 failed

- [eval.sld:33](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/eval.sld#L33) — `eval` `eval:car`

### file — 2 of 75 failed

- [file.sld:139](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/file.sld#L139) — `open-binary-output-file` `flags`
- [file.sld:154](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/file.sld#L154) — `open-binary-input-file` `flags`

### inexact — 11 of 592 failed

- [inexact.sld:311](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L311) — `rationalize`
- [inexact.sld:312](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L312) — `rationalize`
- [inexact.sld:313](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L313) — `rationalize`
- [inexact.sld:361](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L361) — `sqrt`
- [inexact.sld:405](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L405) — `string->number`
- [inexact.sld:406](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L406) — `string->number`
- [inexact.sld:407](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L407) — `string->number`
- [inexact.sld:91](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L91) — `string->number`
- [inexact.sld:95](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L95) — `string->number`
- [inexact.sld:388](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L388) — `string->number`
- [inexact.sld:389](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L389) — `string->number`

### list — 9 of 172 failed

- [list.sld:360](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L360) — `zip`
- [list.sld:361](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L361) — `zip`
- [list.sld:362](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L362) — `zip` `circular-list`
- [list.sld:397](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L397) — `fold`
- [list.sld:470](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L470) — `filter-map` `number?`
- [list.sld:559](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L559) — `any`
- [list.sld:565](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L565) — `every` `circular-list`
- [list.sld:569](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L569) — `list-index`
- [list.sld:589](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/list.sld#L589) — `list` `cells`

### time — 1 of 2 failed

- [time.sld:49](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/time.sld#L49) — `t0` `truncate`

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
| char | crash | 0 | 0 |
| charset | not-bundled | 0 | 0 |
| comparator | pass | 158 | 158 |
| complex | fail | 58 | 61 |
| cxr | pass | 28 | 28 |
| ephemeron | not-bundled | 0 | 0 |
| eval | fail | 2 | 3 |
| file | fail | 73 | 75 |
| flonum | not-bundled | 0 | 0 |
| generator | pass | 49 | 49 |
| hash-table | pass | 82 | 82 |
| ideque | not-bundled | 0 | 0 |
| ilist | not-bundled | 0 | 0 |
| inexact | fail | 581 | 592 |
| lazy | crash | 0 | 0 |
| list-queue | not-bundled | 0 | 0 |
| list | fail | 163 | 172 |
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
| time | fail | 1 | 2 |
| vector | pass | 103 | 103 |
| write | fail | 37 | 40 |
