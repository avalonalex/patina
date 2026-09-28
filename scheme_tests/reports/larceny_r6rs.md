# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-09-28 15:38:11\
**Backend:** VM\
**Lane:** tests/r6rs ((r6rs …) emulation libraries)\
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 14 of 16 |
| Assertions passed | 6493 of 6509 (99.8%) |
| Suites cut short by a top-level error | 0 |
| Suites not reaching a tally | 0 |

A suite that cannot load reaches no tally, and one cut short by a top-level error reaches only part of one, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.

## Assertion failures (16 in 2 suites)

Each entry links to the test case; the name after it is the procedure the assertion exercises.

### base — 9 of 2035 failed

- [base.sld:1374](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L1374) — `#t` `log`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`
- [base.sld:970](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/base.sld#L970) — `string->number` `number->string`

### io/simple — 7 of 56 failed

- [simple.sld:20](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/io/simple.sld#L20) — `binary-port?`
- [simple.sld:32](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/io/simple.sld#L32) — `binary-port?`
- [simple.sld:51](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/io/simple.sld#L51) — `binary-port?`
- [simple.sld:58](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/io/simple.sld#L58) — `binary-port?`
- [simple.sld:89](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/io/simple.sld#L89) — `binary-port?` `current-input-port`
- [simple.sld:93](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/io/simple.sld#L93) — `binary-port?` `current-output-port`
- [simple.sld:97](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/io/simple.sld#L97) — `binary-port?` `current-error-port`

## All suites

| Suite | Status | Passed | Total |
|---|---|---|---|
| arithmetic/fixnums | pass | 3379 | 3379 |
| base | fail | 2026 | 2035 |
| bytevectors | pass | 469 | 469 |
| control | pass | 11 | 11 |
| enums | pass | 26 | 26 |
| eval | pass | 2 | 2 |
| exceptions | pass | 9 | 9 |
| hashtables | pass | 249 | 249 |
| io/simple | fail | 49 | 56 |
| lists | pass | 72 | 72 |
| mutable-pairs | pass | 3 | 3 |
| mutable-strings | pass | 3 | 3 |
| programs | pass | 2 | 2 |
| r5rs | pass | 71 | 71 |
| sorting | pass | 4 | 4 |
| unicode | pass | 118 | 118 |
