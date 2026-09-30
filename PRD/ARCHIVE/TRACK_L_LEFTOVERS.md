# Track L — completion record

**Created:** 2026-09-19, when the track's working record was archived.
**Completed and archived:** 2026-09-29, after [#551](https://github.com/avalonalex/patina/pull/551)
closed the final backlog item, [#423](https://github.com/avalonalex/patina/issues/423).
**Earlier records:** [`TRACK_L_SNOW_LIBRARIES_PRD.md`](TRACK_L_SNOW_LIBRARIES_PRD.md)
(2,500 lines, 2026-06-20 → 2026-09-18) and
[`TRACK_L_FIXED_DEFECTS.md`](TRACK_L_FIXED_DEFECTS.md).

Track L set out to run the third-party R7RS ecosystem — snow-fort packages and
the libraries they lean on — and to know, as a number that regenerates on
demand, how much of it works. The recorded backlog is complete. This archived
ledger preserves the dated measurements, scope boundaries and standing rules;
it is no longer an active queue. **New work belongs in GitHub issues**, and the
reasoning behind past decisions remains in those issues and the earlier records.

## Where the track stopped

| Item | State |
|---|---|
| L0, L0.5, L0.75 — loading edge cases, `-A`/`-I`, ecosystem survey | done |
| L1 — bundle R7RS-large libraries and SRFIs | done: both approved editions ship in full and no in-scope package waits on a library |
| L2 — implementation-specific libraries stay external | done: `(chibi …)` comes from `test-lib/` or the corpus, never `lib/`; package resolution and public distribution are deferred by design |
| L3 — the corpus harness, `patina-compat` | live |
| L4 — bundled ports canonical, vendored duplicates dropped | done |
| L5 — a second opinion: R6RS reader and libraries, Larceny's suites | done; the lanes run on demand |

## The numbers, and how to read them

Measured 2026-09-19 unless dated below. **Re-measure rather than quote** — a
corpus number carries its date (#381), and the archive's status line went stale
twice.

- **Corpus (measured 2026-09-28):** 143 of 161 packages pass, which is
  **143 of 143 in scope**; 18 are excluded by `compat/EXCLUSIONS.scm` with a reason apiece (11 FFI, 2 licence,
  5 upstream defects no faithful patch reaches). Nine came back in under
  `compat/patches/` on 2026-09-19. Both backends now agree on every package's
  bucket and the 143-of-143 scoped score; structured diagnostics remove the
  prose-classification mismatch (#382).
  `cargo run --release -p patina-compat -- run`.
- **Execution coverage (#429, measured 2026-09-28):** seventeen smoke batches
  exercise 106 packages with 1522 assertions, including all 16 PFDS packages.
  Of the 143 passes, 37 run upstream suites, 106 run maintained smoke checks,
  and none remain import-only probes.
  The smoke drivers are gated in CI on both backends. A pass means only what
  that package's mode measured. #429's import-only gap is closed; descriptor,
  terminal and GUI integration limits remain explicit in the drivers and
  `docs/TEST_ORGANIZATION.md`.
- **Larceny, R7RS (measured 2026-09-28):** 24 of 33 suites clean, 8508 of
  8536 assertions, on both backends. Of the 28 failures, the VM has one GC
  failure (#423), where the tree-walker instead has a timing failure. The
  other 27 are not ours or are by decision, including three symbol-spelling
  expectations retained by decision in #422.
  A focused VM rerun after #423 passes `ephemeron` 6 of 6; the full-lane
  totals above predate that fix. `./scripts/run_larceny_tests.sh`.
- **Larceny, R6RS (measured 2026-09-28):** 14 of 16 suites clean, 6493 of
  6509 assertions, on both backends. `base` now runs all 2035 assertions
  after #424; its nine remaining numeric expectations differ from the R7RS
  procedures the facade supplies. Seven `io/simple` port-predicate checks
  differ by decision (#412).
- **chibi's R7RS suite:** 1226 of 1226 on both backends, the routine gate.

## Closure and follow-up work

All issues in Track L's recorded backlog are closed. [#429](https://github.com/avalonalex/patina/issues/429)
closed the import-only execution-coverage gap; #551 closed #423, the last
remaining backend item. Completion is bounded by the corpus exclusions and
the classified reference differences above, not a claim of universal compatibility.

Separate work remains outside this completed backlog:

- Reporting: retain evidence for compatibility failures
  ([#379](https://github.com/avalonalex/patina/issues/379)) and remove timing-only
  churn from committed chibi reports ([#380](https://github.com/avalonalex/patina/issues/380)).
- Broader collection work remains in the [GC stage 5 PRD](../future/GC_STAGE5_PRD.md).
- Package resolution and public distribution remain deferred, as does FFI;
  see the [package-manager design](../future/PACKAGE_MANAGER_DESIGN.md) and
  [FFI design](../FFI_DESIGN.md).

One recorded debt has no issue because it has no known symptom: on the VM,
`vm_raise_value`, the two prompt paths and the value-form arm still locate wind
records by *depth*, an assumption that continuation identity was introduced to
retire (archive §6, "Still open next door"). File it when it produces one.

## Standing rules the track leaves behind

- **Self-contained.** No build-, test- or CI-time dependency on another Scheme or a package manager. The corpus is data pinned by checksum.
- **What ships:** the [bundling policy](../../docs/README.md#library-bundling-policy) covers R7RS-large libraries and SRFIs, never another implementation's namespace.
- **What is excluded, and what is patched:** `compat/EXCLUSIONS.scm` (a closed set of reasons; an excluded package still runs) and `compat/patches/README.md` (portability patches, the authorized upstream correctness-patch workflow during #429, and why a Patina difference is never patched).
- **Whose defect it is** is settled by measurement against chibi and Gauche, never by which of them accepts a program. `crates/patina-tests/tests/scheme/DIVERGENCES.tsv` is where a difference is classified.
- **Port policy (#412):** file and standard ports support both characters and bytes, as chibi and Gauche do; string ports remain textual-only, as in chibi.
- **Reader boundaries (#421):** quote prefixes and vertical bars end unescaped tokens, following Gauche; Chibi 0.12's narrower boundaries are recorded in the oracle divergence register. Names containing those delimiters use vertical bars.
- **Symbol output (#422):** retain standard R7RS escaping (`@` writes as `|@|`), even for names accepted bare as reader extensions; `symbol->string` returns the unescaped name, and name conversion and writer/reader round trips preserve it.
- **Square-root policy (#418):** negative real inputs produce an exact zero real component, following Chibi and preserving the reader/writer round trip; infinity and inexact imaginary components remain inexact.
- **Local syntax (#424):** `(srfi 188)` supplies splicing forms, reused under the R6RS names by the R6RS facade; `(scheme base)` retains ordinary R7RS forms. The facade accepts SRFI 188's mixed-definition/expression extension.
- **Larceny triage** remains in `scheme_tests/reports/larceny_triage.md`, including upstream and by-decision expectations. New Patina defects need their own issues; Track L's closed backlog does not erase those classifications.
