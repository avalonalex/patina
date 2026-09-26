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

Measured 2026-09-19. **Re-measure rather than quote** — a corpus number carries
its date (#381), and the archive's status line went stale twice.

- **Corpus:** 143 of 161 packages pass, which is **143 of 143 in scope**; 18 are
  excluded by `compat/EXCLUSIONS.scm` with a reason apiece (11 FFI, 2 licence,
  5 upstream defects no faithful patch reaches). Nine came back in under
  `compat/patches/` on 2026-09-19. That is the VM's reading; the tree-walker
  passes the same 143 and reads 143 of 144, because one excluded package files
  its failure in a different bucket there (#382).
  `cargo run --release -p patina-compat -- run`.
- **What that does not say:** 106 of the 143 are *probe-mode* — imported, never
  called. 37 run a suite. The headline means "loads", not "works" (#429).
- **Larceny, R7RS:** 24 of 33 suites clean, 8512 of 8534 assertions, on both
  backends. Of the 22 failures, 17 are not ours or are by decision, 3 wait on
  #422 and 2 are ours (#418, #423). `./scripts/run_larceny_tests.sh`.
- **Larceny, R6RS:** 15 of 16 suites, 4474 of 4474; the sixteenth is #424.
- **chibi's R7RS suite:** 1226 of 1226 on both backends, the routine gate.

## What is left

Each line is an issue. When one closes, delete its line; when this list is
empty, archive this page.

**Backends**
- #423 — VM: a stale register keeps a replaced value alive (GC precision; `PRD/future/GC_STAGE5_PRD.md`).

**The corpus and its harness**
- #429 — expand execution coverage beyond import-only probes.
- #382 — the classifier keys on error prose no producer knows is parsed.
- #384 — two upstream SRFI 160 defects, quarantined.

One recorded debt has no issue because it has no known symptom: on the VM,
`vm_raise_value`, the two prompt paths and the value-form arm still locate wind
records by *depth*, an assumption that continuation identity was introduced to
retire (archive §6, "Still open next door"). File it when it produces one.

## Decisions waiting on the owner

These are not defects. Each is a choice R7RS leaves open, where the references
split or where matching them has a cost, and none should be taken in passing.

- #412 — byte operations on textual file ports. Both references allow them; costs two Larceny assertions.
- #421 — identifier delimiters: `(length '(a'b))` is 1 here and 2 in both references. The one reader widening that can change a working program.
- #422 — whether `write` spells the symbol `@` as `|@|`. chibi and Gauche split.
- #418 — `(sqrt -inf.0)`: an exact or an inexact zero real part. Tied to how the writer elides one.
- #424 — whether `(r6rs base)` gets R6RS's splicing `let-syntax`. It is all that blocks ~2000 assertions of the R6RS `base` suite.

## Standing rules the track leaves behind

- **Self-contained.** No build-, test- or CI-time dependency on another Scheme or a package manager. The corpus is data pinned by checksum.
- **What ships:** `PRD/phase2/R7RS_LARGE_STATUS.md` is the bundling policy — R7RS-large libraries and SRFIs, never another implementation's namespace.
- **What is excluded, and what is patched:** `compat/EXCLUSIONS.scm` (a closed set of reasons; an excluded package still runs) and `compat/patches/README.md` (when a patch is justified, and why a Patina difference is never one).
- **Whose defect it is** is settled by measurement against chibi and Gauche, never by which of them accepts a program. `crates/patina-tests/tests/scheme/DIVERGENCES.tsv` is where a difference is classified.
- **The Larceny defect queue** is `scheme_tests/reports/larceny_triage.md`, which deletes itself when its families close; four remain, all listed above.
