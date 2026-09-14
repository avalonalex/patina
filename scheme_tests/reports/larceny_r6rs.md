# Patina vs Larceny's R7RS test suite — by kind of problem

**Generated:** 2026-09-13 17:04:00\
**Backend:** VM\
**Lane:** tests/r6rs ((r6rs …) emulation libraries)\
**Suite:** larcenists/larceny @ `fef550c7d392` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`

This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.

| | |
|---|---|
| Suites fully passing | 15 of 16 |
| Assertions passed | 4474 of 4474 (100.0%) |
| Suites not reaching a tally | 1 |

A suite that cannot load reaches no tally, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.

## Failed to load (1)

The suite's library did not compile, so nothing in it ran. Patina's message:

| Suite | Message |
|---|---|
| base | `Error: Parse error in tests/r6rs/base.sld: desugar error: Invalid syntax: let-syntax requires bindings and at least one body expression` |

## All suites

| Suite | Status | Passed | Total |
|---|---|---|---|
| arithmetic/fixnums | pass | 3379 | 3379 |
| base | load-error | 0 | 0 |
| bytevectors | pass | 469 | 469 |
| control | pass | 11 | 11 |
| enums | pass | 26 | 26 |
| eval | pass | 2 | 2 |
| exceptions | pass | 9 | 9 |
| hashtables | pass | 249 | 249 |
| io/simple | pass | 56 | 56 |
| lists | pass | 72 | 72 |
| mutable-pairs | pass | 3 | 3 |
| mutable-strings | pass | 3 | 3 |
| programs | pass | 2 | 2 |
| r5rs | pass | 71 | 71 |
| sorting | pass | 4 | 4 |
| unicode | pass | 118 | 118 |
