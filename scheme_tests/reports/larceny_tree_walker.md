# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-09-01 14:16:08  
**Backend:** tree-walker  
**Lane:** tests/scheme (R7RS-small + Red edition)  
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 0 of 1 |
| Assertions passed | 1070 of 1079 (99.2%) |
| Suites not reaching a tally | 0 |

A suite that cannot load reaches no tally, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.

## Assertion failures (9 in 1 suites)

Each entry links to the test case; the name after it is the procedure the assertion exercises.

### base — 9 of 1079 failed

- [base.sld:918](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L918) — `name` `unquote`
- [base.sld:921](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L921) — `name` `foo`
- [base.sld:924](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L924) — `append` `sqrt`
- [base.sld:927](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L927) — `foo` `unquote`
- (not located) — `vector-copy!`
- (not located) — `a`
- [base.sld:2346](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2346) — `bytevector-u8-ref` `make-bytevector`
- [base.sld:2347](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2347) — `bytevector-u8-ref` `make-bytevector`
- [base.sld:2647](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/scheme/base.sld#L2647) — `set!`

## All suites

| Suite | Status | Passed | Total |
|---|---|---|---|
| base | fail | 1070 | 1079 |
