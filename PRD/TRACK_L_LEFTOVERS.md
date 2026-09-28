# Track L — leftovers

**Created:** 2026-09-19, when the track's working record was archived.
**Archive:** [`ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md`](ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md)
(2,500 lines, 2026-06-20 → 2026-09-18) and
[`ARCHIVE/TRACK_L_FIXED_DEFECTS.md`](ARCHIVE/TRACK_L_FIXED_DEFECTS.md).

Track L set out to run the third-party R7RS ecosystem — snow-fort packages and
the libraries they lean on — and to know, as a number that regenerates on
demand, how much of it works. Its loop has converged. This page says where it
stopped and points at what is left. **It holds no narrative: every work item is
a GitHub issue**, and the reasoning behind a past decision is in the archive.

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
  8536 assertions, on both backends. Of the 28 failures, 3 wait on #422;
  the VM has one GC failure (#423), where the tree-walker instead has a
  timing failure. The other 24 are not ours or are by decision.
  `./scripts/run_larceny_tests.sh`.
- **Larceny, R6RS (measured 2026-09-28):** 14 of 16 suites clean, 4467 of
  4474 assertions, on both backends. Seven `io/simple` port-predicate checks
  differ by decision (#412); `base` still cannot load (#424).
- **chibi's R7RS suite:** 1226 of 1226 on both backends, the routine gate.

## What is left

Each line is an issue. When one closes, delete its line; when this list is
empty, archive this page.

**Backends**
- #423 — VM: a stale register keeps a replaced value alive (GC precision; `PRD/future/GC_STAGE5_PRD.md`).

One recorded debt has no issue because it has no known symptom: on the VM,
`vm_raise_value`, the two prompt paths and the value-form arm still locate wind
records by *depth*, an assumption that continuation identity was introduced to
retire (archive §6, "Still open next door"). File it when it produces one.

## Decisions waiting on the owner

These are not defects. Each is a choice R7RS leaves open, where the references
split or where matching them has a cost, and none should be taken in passing.

- #422 — whether `write` spells the symbol `@` as `|@|`. chibi and Gauche split.
- #424 — whether `(r6rs base)` gets R6RS's splicing `let-syntax`. It is all that blocks ~2000 assertions of the R6RS `base` suite.

## Standing rules the track leaves behind

- **Self-contained.** No build-, test- or CI-time dependency on another Scheme or a package manager. The corpus is data pinned by checksum.
- **What ships:** `PRD/phase2/R7RS_LARGE_STATUS.md` is the bundling policy — R7RS-large libraries and SRFIs, never another implementation's namespace.
- **What is excluded, and what is patched:** `compat/EXCLUSIONS.scm` (a closed set of reasons; an excluded package still runs) and `compat/patches/README.md` (portability patches, the authorized upstream correctness-patch workflow during #429, and why a Patina difference is never patched).
- **Whose defect it is** is settled by measurement against chibi and Gauche, never by which of them accepts a program. `crates/patina-tests/tests/scheme/DIVERGENCES.tsv` is where a difference is classified.
- **Port policy (#412):** file and standard ports support both characters and bytes, as chibi and Gauche do; string ports remain textual-only, as in chibi.
- **Reader boundaries (#421):** quote prefixes and vertical bars end unescaped tokens, following Gauche; Chibi 0.12's narrower boundaries are recorded in the oracle divergence register. Names containing those delimiters use vertical bars.
- **Square-root policy (#418):** negative real inputs produce an exact zero real component, following Chibi and preserving the reader/writer round trip; infinity and inexact imaginary components remain inexact.
- **The Larceny defect queue** is `scheme_tests/reports/larceny_triage.md`, which deletes itself when its families close; three remain, all listed above.
