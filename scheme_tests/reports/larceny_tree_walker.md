# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-09-02 12:18:29  
**Backend:** tree-walker  
**Lane:** tests/scheme (R7RS-small + Red edition)  
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 22 of 33 |
| Assertions passed | 8309 of 8336 (99.7%) |
| Suites not reaching a tally | 1 |

A suite that cannot load reaches no tally, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.

## Crashed or hung (1)

No tally was reached: the process died, or the runner's timeout cut it off. A crash is a defect in Patina's runtime; a timeout may be one, or may be a suite that needs longer than the budget on this backend — the triage doc says which for each.

| Suite | What |
|---|---|
| char | stack overflow |

## Assertion failures (27 in 10 suites)

Each entry links to the test case; the name after it is the procedure the assertion exercises.

### base — 8 of 1079 failed

- [base.sld:918](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L918) — `name` `unquote`
- [base.sld:921](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L921) — `name` `foo`
- [base.sld:924](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L924) — `append` `sqrt`
- [base.sld:927](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L927) — `foo` `unquote`
- (not located) — `vector-copy!`
- (not located) — `a`
- [base.sld:2346](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2346) — `bytevector-u8-ref` `make-bytevector`
- [base.sld:2347](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2347) — `bytevector-u8-ref` `make-bytevector`

### charset — 2 of 93 failed

- [charset.sld:108](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/charset.sld#L108) — `>=` `char-set-size`
- [charset.sld:136](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/charset.sld#L136) — `cs` `char-set`

### complex — 1 of 61 failed

- [complex.body.scm:89](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/complex.body.scm#L89) — `log`

### eval — 1 of 3 failed

- [eval.sld:33](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/eval.sld#L33) — `eval` `eval:car`

### flonum — 1 of 1280 failed

- [flonum.sld:632](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/flonum.sld#L632) — `map`

### ilist — 8 of 345 failed

- [ilist.sld:757](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L757) — `comparator-test-type` `iq`
- [ilist.sld:882](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L882) — `comparator-test-type` `iq`
- [ilist.sld:918](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L918) — `comparator-test-type` `iq`
- [ilist.sld:954](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L954) — `comparator-test-type` `iq`
- [ilist.sld:956](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L956) — `comparator-test-type` `iq`
- [ilist.sld:971](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L971) — `comparator-compare` `iq`
- [ilist.sld:1039](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L1039) — `comparator-compare` `iq`
- [ilist.sld:1043](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/ilist.sld#L1043) — `comparator-compare` `iq`

### inexact — 1 of 592 failed

- [inexact.sld:361](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/inexact.sld#L361) — `sqrt`

### list — 1 of 172 failed

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
| base | fail | 1071 | 1079 |
| box | pass | 10 | 10 |
| case-lambda | pass | 5 | 5 |
| char | crash | 0 | 0 |
| charset | fail | 91 | 93 |
| comparator | pass | 158 | 158 |
| complex | fail | 60 | 61 |
| cxr | pass | 28 | 28 |
| ephemeron | pass | 6 | 6 |
| eval | fail | 2 | 3 |
| file | pass | 75 | 75 |
| flonum | fail | 1279 | 1280 |
| generator | pass | 49 | 49 |
| hash-table | pass | 82 | 82 |
| ideque | pass | 114 | 114 |
| ilist | fail | 337 | 345 |
| inexact | fail | 591 | 592 |
| lazy | pass | 33 | 33 |
| list-queue | pass | 40 | 40 |
| list | fail | 171 | 172 |
| load | pass | 1 | 1 |
| lseq | pass | 109 | 109 |
| process-context | pass | 2 | 2 |
| read | pass | 44 | 44 |
| repl | pass | 0 | 0 |
| rlist | pass | 82 | 82 |
| set | pass | 16 | 16 |
| sort | pass | 2562 | 2562 |
| stream | pass | 81 | 81 |
| text | pass | 1069 | 1069 |
| time | fail | 1 | 2 |
| vector | pass | 103 | 103 |
| write | fail | 37 | 40 |
