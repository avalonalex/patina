# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-09-01 12:30:54  
**Backend:** VM  
**Lane:** tests/r6rs ((r6rs …) emulation libraries)  
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 11 of 16 |
| Assertions passed | 4023 of 4033 (99.8%) |
| Suites not reaching a tally | 1 |

A suite that cannot load reaches no tally, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.

## Failed to load (1)

The suite's library did not compile, so nothing in it ran. Patina's message:

| Suite | Message |
|---|---|
| base | `Error: Parse error in tests/r6rs/base.sld: desugar error: Invalid syntax: let-syntax requires bindings and at least one body expression` |

## Assertion failures (10 in 4 suites)

Each entry links to the test case; the name after it is the procedure the assertion exercises.

### bytevectors — 4 of 28 failed

- [bytevectors.sld:47](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/bytevectors.sld#L47) — `bytevector-u8-ref` `make-bytevector`
- [bytevectors.sld:48](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/bytevectors.sld#L48) — `bytevector-u8-ref` `make-bytevector`
- [bytevectors.sld:65](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/bytevectors.sld#L65) — `b1` `make-bytevector`
- [bytevectors.sld:74](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/bytevectors.sld#L74) — `make-bytevector` `bytevector-s8-set!`

### enums — 3 of 26 failed

- [enums.sld:97](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/enums.sld#L97) — `color`
- [enums.sld:99](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/enums.sld#L99) — `enum-set->list` `color-set`
- [enums.sld:100](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/enums.sld#L100) — `enum-set->list` `color-set`

### eval — 1 of 2 failed

- [eval.sld:16](https://github.com/larcenists/larceny/blob/fef550c7d3923deb7a5a1ccd5a628e54cf231c75/test/R7RS/Lib/tests/r6rs/eval.sld#L16) — `eval` `eval:car`

### exceptions — 2 of 9 failed

- (not located) — `#t` `with-exception-handler`
- (not located) — `v`

## All suites

| Suite | Status | Passed | Total |
|---|---|---|---|
| arithmetic/fixnums | pass | 3379 | 3379 |
| base | load-error | 0 | 0 |
| bytevectors | fail | 24 | 28 |
| control | pass | 11 | 11 |
| enums | fail | 23 | 26 |
| eval | fail | 1 | 2 |
| exceptions | fail | 7 | 9 |
| hashtables | pass | 249 | 249 |
| io/simple | pass | 56 | 56 |
| lists | pass | 72 | 72 |
| mutable-pairs | pass | 3 | 3 |
| mutable-strings | pass | 3 | 3 |
| programs | pass | 2 | 2 |
| r5rs | pass | 71 | 71 |
| sorting | pass | 4 | 4 |
| unicode | pass | 118 | 118 |
