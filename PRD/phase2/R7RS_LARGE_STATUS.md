# R7RS-Large Status Tracking

**Last Updated:** 2026-09-06 — the bundling policy gained a third addition, the standard testing
API (SRFI 64). That one is a **decision, not a state**: the tables below still describe what `lib/`
holds today, and SRFI 64 is not in it until #193's Phase 0 puts it there. Previously 2026-09-01 —
bookkeeping: the header had read 2026-08-08 while the tables
underneath were kept current through the 2026-08-24…26 bundling wave; they now agree. Reconciled
against `lib/` on this date: **Red 16 of 17 shipped** (17 counting SRFI 158 for the superseded
121), **Tangerine 4 of 8**; the measured priority order below is spent and marked as history.

This document is **the bundling policy and edition tracker for Track L** — the answer to "does
Patina ship this library, and why (not)". Track L's L1 defers to it for scope; the corpus
in-degree measurements order it; `crates/patina-tests/tests/r7rs_large_aliases.rs` keeps its
alias tables honest.

---

## Overview

R7RS-large is being developed incrementally through "editions," each focusing on different aspects of the language. Each edition consists of SRFIs that have been voted on and approved by the Scheme Working Group 2 (WG2).

**Official Resources:**
- Main site: https://r7rs.org/
- Working documents: https://github.com/johnwcowan/r7rs-work
- SRFI index: https://srfi.schemers.org/?keywords=r7rs-large

---

## Approved Editions

### Red Edition (2016)

**Status:** ✅ Approved
**Focus:** Data Structures

| SRFI | Library Name | Description | `(srfi n)` | `(scheme …)` alias |
|------|-------------|-------------|------------|--------------------|
| SRFI 1 | `(scheme list)` | List library | ✅ shipped | ✅ shipped |
| SRFI 14 | `(scheme charset)` | Character sets | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 14)`, 2026-08-24) |
| SRFI 41 | `(scheme stream)` | Streams (lazy lists) | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 41)`, 2026-08-25) |
| SRFI 101 | `(scheme rlist)` | Random-access lists | ✅ shipped | ✅ shipped (r-prefixed alias, 2026-08-26) |
| SRFI 111 | `(scheme box)` | Boxes (single-value containers) | ✅ shipped | ✅ shipped |
| SRFI 113 | `(scheme set)` | Sets and bags | ✅ shipped | ✅ shipped |
| SRFI 116 | `(scheme ilist)` | Immutable lists | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 116)`, 2026-08-25) |
| SRFI 117 | `(scheme list-queue)` | List queues | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 117)`, 2026-08-25) |
| SRFI 121 | `(scheme generator)` | Generators | — superseded by SRFI 158 | — |
| SRFI 124 | `(scheme ephemeron)` | Ephemerons | ✅ shipped | ✅ shipped (Rust-backed, 2026-08-26) |
| SRFI 125 | `(scheme hash-table)` | Hash tables | ✅ | ✅ |
| SRFI 127 | `(scheme lseq)` | Lazy sequences | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 127)`, 2026-08-25) |
| SRFI 128 | `(scheme comparator)` | Comparators | ✅ shipped | ✅ shipped |
| SRFI 132 | `(scheme sort)` | Sort libraries | ✅ shipped | ✅ shipped |
| SRFI 133 | `(scheme vector)` | Vector library | ✅ shipped | ✅ shipped |
| SRFI 134 | `(scheme ideque)` | Immutable deques | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 134)`, 2026-08-25) |
| SRFI 135 | `(scheme text)` | Immutable texts | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 135)`, 2026-08-26) |

**Red status: 16 of 17 shipped** (SRFI 1, 14, 41, 101, 111, 113, 116, 117, 124, 125, 127, 128, 132, 133, 134, 135), all reachable under both `(srfi n)` and their `(scheme …)` names (17 counting SRFI 158 standing in for the superseded 121).

Two of those did not arrive by bundling Scheme. SRFI 124 is implemented in Rust, because an ephemeron's defining property is a statement about what the collector does and there is no Scheme to vendor. SRFI 101 comes from chibi's R7RS adaptation of the reference implementation rather than from the SRFI, whose own distribution is R6RS `.sls` — and note its alias renames rather than re-exports: R7RS-large's `(scheme rlist)` is `rcons`/`rcar`/`rlist?`, since SRFI 101's own names shadow `(scheme base)`.

**Notes:**
- SRFI 129 (titlecase) was voted down
- SRFI 13 (strings) marked for reballoting
- The `(scheme …)` alias libraries live in `lib/scheme/`, one file per row of the tables above; they are enumerated in `ALIASES` in `crates/patina-tests/tests/r7rs_large_aliases.rs`, which is what checks them.
  Each is a pure re-export of its backing `(srfi n)` — same bindings, not a second copy.
  `crates/patina-tests/tests/r7rs_large_aliases.rs` asserts the export sets stay identical, which is
  the drift this hand-listed approach otherwise invites.

---

### Tangerine Edition (2019)

**Status:** ✅ Approved
**Focus:** Data Structures and Numerics

| SRFI | Library Name | Description | `(srfi n)` | `(scheme …)` alias |
|------|-------------|-------------|------------|--------------------|
| SRFI 115 | `(scheme regex)` | Regular expressions | ❌ | ❌ |
| SRFI 143 | `(scheme fixnum)` | Fixnums | ✅ shipped | ✅ shipped |
| SRFI 144 | `(scheme flonum)` | Flonums | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 144)`, 2026-08-26) |
| SRFI 146 | `(scheme mapping)` | Mappings | ❌ | ❌ |
| SRFI 146 | `(scheme mapping hash)` | Hash mappings | ❌ | ❌ |
| SRFI 151 | `(scheme bitwise)` | Bitwise operations | ✅ shipped | ✅ shipped |
| SRFI 158 | `(scheme generator)` | Generators (supersedes SRFI 121) | ✅ shipped | ✅ shipped |
| SRFI 159 | `(scheme show)` | Formatting/show | ❌ | ❌ |
| SRFI 160 | `(scheme vector @)` | Numeric vectors (u8, s8, f64, etc.) | ❌ | ❌ |
| R6RS | `(scheme bytevector)` | Bytevectors (R6RS compatible) | 🚧 Partial | ❌ |

**Tangerine status: 4 of 8 shipped**, reachable under both names. (SRFI 144 was missing from this table until it was bundled, which is why the denominator moves too.)

**Numeric Tower Requirements:**
- Unbounded exact integers ✅ (BigInt support)
- Unbounded exact rationals ✅ (Rational support)
- Inexact reals ✅ (f64 support)
- Exact and inexact complex ✅ (Complex support)

---

## Pending Editions

### Yellow Edition (Kronos)

**Status:** 🗳️ Voted (details pending)
**Focus:** TBD

*Details to be added when available*

### Orange Edition

**Status:** 📋 Draft
**Focus:** Numbers

The Orange docket contains 23 SRFIs (all but one finalized). However, the macro system ballot was prioritized first.

See: https://small.r7rs.org/wiki/OrangeDocket/

### Macrological Fascicle

**Status:** 📋 First Draft (October 2024)
**Focus:** Macro System

This is part of the new "Foundations" volume approach (named after Greek deities instead of colors).

**Key Features:**
- `syntax-case` (R6RS-based, refined)
- Explicit renaming macros
- Syntax parameters
- Identifier properties
- Procedural syntax object destructuring

**Target:** December 2025 (Scheme's 50th birthday)

See: https://r7rs.org/large/fascicles/macro/1/

---

## Implementation Priority for Patina

### Phase 1: R7RS-small Compliance (Current)

Focus on completing R7RS-small before R7RS-large:
- I/O system (~265 tests blocked)
- Exception handling
- Records (`define-record-type`)
- System interface

### Bundling policy

**Any SRFI named in the R7RS-large standardization process is in scope for bundling.** Standard-track
SRFIs are commitments the project is making anyway, so the usual objection to bundling — that every
bundled library is a permanent compatibility promise you cannot withdraw without a breaking change —
does not apply. That gives a bounded, principled set: the Red and Tangerine tables above, extended as
later editions are ratified.

Three additions are needed, because standard-track membership alone does not cover everything that
must work:

1. **Runtime-forced SRFIs off the standard track.** Libraries that cannot exist as portable Scheme,
   regardless of what any edition names. SRFI 27 (random) needs an RNG primitive; SRFI 170 (POSIX)
   needs syscalls; SRFI 143 must match Patina's actual fixnum width. If a user cannot get a correct,
   reasonably fast copy by pointing `-A` at a directory, bundling is not a convenience — it is the
   only way the library can exist.
2. **Legacy aliases the ecosystem actually imports.** The standard track and real-world usage overlap
   only partially, and the bitwise cluster is the clearest case: R7RS-large names **SRFI 151**, but
   measured against the vendored corpus, **31 packages import `(srfi 60)`** and 19 import `(srfi 33)`.
   Shipping 151 alone leaves all of them failing. Once native bitwise primitives exist, 60 and 33 are
   thin shims over the same primitives — cheap to add, and the reason to add them is demand, not
   standards. Same shape for SRFI 69 (shipped, in-degree 16) versus the standard-track SRFI 125.

3. **The testing API — one library, and the standard one.** Amendment of 2026-09-06, made
   deliberately because it widens the set; #194 raised the question and this settles it. **SRFI 64 is
   to be bundled** — a decision, not yet a state: `lib/srfi/64` does not exist, and #193's Phase 0 is
   the work that creates it.

   **Why bundled rather than supplied from `test-lib/`.** This is the load-bearing part, because the
   `-A` mechanism #194's trio built would serve *our* lanes just as well. It would not serve a user.
   A test library is the one thing nearly every user needs and cannot reasonably write, and
   `test-lib/` is test data rather than a shipped artifact (`test-lib/README.md`) — so supplying it
   there answers "how do Patina's suites run" while leaving "how do I test my own program" answered
   by #195's gap. Bundling is what makes `patina my-test.scm` work with no `-A` and nothing to
   obtain. That is a user-facing capability, which is the distinction this whole policy turns on.

   **Why SRFI 64 rather than `(chibi test)`.** Implementation neutrality: a test library is also what
   every *other* implementation must provide for a portable suite to run, so the one Patina ships
   should be the one they already have. SRFI 64 is that; `(chibi test)` is one implementation's house
   framework. Neutrality settles *which*, and the paragraph above settles *where*; they are separate
   questions and the amendment needs both.

   **The rule this creates is narrow, and the narrowness is the point:** `lib/` ships **one
   third-party testing library, the standard one**, plus Patina's own thin shim over it
   (`(patina test)`, #193). Non-standard frameworks stay out however much our own lanes want them,
   which is why #196, #197 and #198 moved `(chibi test)`, `(chibi diff)`, `(chibi optional)`,
   `(chibi term ansi)`, `(chibi filesystem)` and `(chibi string)` to `test-lib/`. Being forced by a
   test lane is still not being runtime-forced; what changed is that one testing API is now in scope
   by this clause rather than by an edition table.

   **In-degree does not decide this one, and says so.** The Ordering rule below fixes order by
   measured in-degree over `compat/vendor/`, where `(srfi 64)` is 5 and `(chibi test)` is 79 — which
   would invert this decision. It is the wrong instrument here: in-degree counts what *vendored
   packages* import, and the demand being served is from users writing their own tests, who appear in
   no corpus. Recorded rather than glossed, since the numbers are real and point the other way.

   **Consequences, all of which Phase 0 owns:**

   - **`(patina test)` must shim over SRFI 64, not `(chibi test)`.** Not merely a policy violation:
     `test-lib/` is not on the default search path, so a `lib/` library importing a `test-lib/` one
     makes every plain `patina script.scm` that imports `(patina test)` fail to resolve outright.
   - **`(test-exit)` is required for xfail/xpass to bite.** This clause buys the property that a
     quarantined divergence fails once its bug is fixed — but in SRFI 64 an unexpected pass reaches
     the process *only* through `test-exit`, which exits 1 when `xpass-count` or `fail-count` is
     non-zero (`compat/vendor/srfi-64/srfi/64.scm`). `test-end` displays
     `# of unexpected successes` and returns normally, exit code 0. A driver that runs a file to
     `test-end` and reads the exit status gets a false green, which is the same shape as audit E1.
     #193's driver must call `test-exit` or read the runner's counts directly.
   - **The corpus loses a package, but not yet.** `bundled_libraries()` in
     `compat/tools/build_corpus.py` globs both roots and drops every vendored package providing a
     bundled library — and that runs at corpus *build* time, not at run time. Measured after bundling:
     the committed corpus is untouched, still **127 of 161** with `srfi-64` passing. The next
     `build_corpus.py` run drops it, taking the tally to 126 of 160. Not a regression — the package
     is excluded because we provide it — but it costs something real that the headline number does
     not show: **SRFI 64's own 259-line conformance suite (`compat/vendor/srfi-64/test.scm`) runs
     today only as that corpus package**, and loses its home with it. #193's driver is the intended
     new home, since that file is exactly the kind of thing it runs.
   - **The compat classifier becomes coupled to a file we can edit.** `test_suite_failed` in
     `crates/patina-compat/src/run.rs` detects SRFI 64 failures by matching that runner's literal
     summary wording, a shape audit E1 recorded getting wrong once. While SRFI 64 was a byte-identical
     vendored tarball, the wording could not drift; bundled, it can. Keep the bundled copy
     byte-identical and pinned, and treat its output shape as an interface.
   - **Provenance and licence, as for any bundled third-party file.** SRFI 64 is MIT: it needs a row
     in `lib/srfi/PROVENANCE.md` with its tarball sha256, coverage under `lib/srfi/LICENSE`, and a
     pin in `crates/patina-tests/tests/bundled_provenance.rs`. `test-lib/README.md`'s rule applies
     unchanged — a notice obligation does not care which directory the file is in.

**Explicitly out of scope:** pure-Scheme leaf libraries that are neither standard-track nor
runtime-forced. They work fine from a `-A` directory or the vendored corpus, and bundling them makes
Patina a slow package manager for code it does not need to own. Since the amendment above, that
explicitly includes **non-standard testing libraries**: `(chibi test)` is supplied, not shipped.

### Ordering

The policy fixes the *set*; measured dependency in-degree over `compat/vendor/` fixes the *order*.
**This queue is spent (2026-09-01), with one item added since** — every numbered item below shipped
except the two that were always conditional, and the amendment above adds a third: **SRFI 64**
(decided 2026-09-06, unshipped, tracked by #193's Phase 0 — and the one item in-degree does not
order, for the reason that clause gives). The other two: **SRFI 115** (large; only if the corpus
justifies it) and the **Tangerine trio 146/159/160** (standard-track, little measured demand —
159/`(scheme show)` would also clear two corpus rows via `(chibi show)`/SRFI 166, which is the
likeliest reason to take it). The near-free shims `(srfi 6/9/11/39)` at the end also remain, for
want of a package that asks. Kept as written for the record, strikethrough marking what shipped:

1. ~~Bitwise — SRFI 151 + 60/33 shims~~ — **done**. Core operators are Rust primitives in
   `(patina internal bitwise)`; the ~30 derived procedures are Scheme. `(srfi 60)` (in-degree 31)
   and `(srfi 33)` (19) are renames over the same bindings, not separate ports.
2. ~~`(scheme …)` alias libraries for the shipped SRFIs~~ — **done**; see the tables above.
3. ~~**SRFI 125 hash tables**, superseding the shipped SRFI 69 (in-degree 16); keep 69 as an
   alias.~~ ✅ **done 2026-08-16, and not that way.** SRFI 125 is a layer *over* SRFI 69 and
   SRFI 128, not a replacement: SRFI 69 stays as the substrate and keeps its own narrower
   semantics, because it is a separate published SRFI with 16 corpus importers. Four deviations
   were needed and are recorded in `lib/srfi/125.sld`'s header.
4. ~~**SRFI 27 random** — runtime-forced, in-degree 9.~~ — **done**.
5. ~~SRFI 143 fixnums~~ — **done**. Mostly renames over SRFI 151 and `(scheme base)`; the part that
   had to be right is `fx-width` / `fx-greatest` / `fx-least`, derived by probing `fixnum?` rather
   than hardcoded so the library cannot claim a range the tagging does not provide.
6. ~~**SRFI 14 char-sets** (in-degree 4), then the remaining Red data structures (41, 101, 116,
   117, 124, 127, 134, 135)~~ — **all done by 2026-08-26**, the Larceny suites having supplied the
   demand the corpus had not; Tangerine (146, 159, 160) remains.
7. **SRFI 115 regex** last — large, and only if the corpus justifies it.

Near-free re-export shims worth doing alongside, since R7RS base already provides the functionality
and packages import them by SRFI name: `(srfi 9)` records, `(srfi 11)` `let-values`, `(srfi 39)`
parameters, `(srfi 6)` string ports.

### Phase 3: syntax-case

When the Macrological Fascicle is finalized:
- See `PRD/phase2/SYNTAX_CASE_DESIGN.md` for implementation plan

---

## SRFI Reference Links

### Red Edition SRFIs
- [SRFI 1](https://srfi.schemers.org/srfi-1/) - List Library
- [SRFI 14](https://srfi.schemers.org/srfi-14/) - Character-set Library
- [SRFI 41](https://srfi.schemers.org/srfi-41/) - Streams
- [SRFI 111](https://srfi.schemers.org/srfi-111/) - Boxes
- [SRFI 113](https://srfi.schemers.org/srfi-113/) - Sets and Bags
- [SRFI 125](https://srfi.schemers.org/srfi-125/) - Hash Tables
- [SRFI 128](https://srfi.schemers.org/srfi-128/) - Comparators
- [SRFI 132](https://srfi.schemers.org/srfi-132/) - Sort Libraries
- [SRFI 133](https://srfi.schemers.org/srfi-133/) - Vector Library

### Tangerine Edition SRFIs
- [SRFI 115](https://srfi.schemers.org/srfi-115/) - Scheme Regular Expressions
- [SRFI 143](https://srfi.schemers.org/srfi-143/) - Fixnums
- [SRFI 146](https://srfi.schemers.org/srfi-146/) - Mappings
- [SRFI 151](https://srfi.schemers.org/srfi-151/) - Bitwise Operations
- [SRFI 158](https://srfi.schemers.org/srfi-158/) - Generators and Accumulators
- [SRFI 159](https://srfi.schemers.org/srfi-159/) - Combinator Formatting
- [SRFI 160](https://srfi.schemers.org/srfi-160/) - Homogeneous Numeric Vector Libraries

### All R7RS-large SRFIs
- https://srfi.schemers.org/?keywords=r7rs-large
- https://srfi.schemers.org/?keywords=r7rs-large-red
- https://srfi.schemers.org/?keywords=r7rs-large-tangerine

---

## Notes

- R7RS-large is developed incrementally; implementations can support editions progressively
- All SRFIs have reference implementations before being voted on
- The naming convention shifted from colors (spectral order) to Greek deities for newer ballots
- Gauche is a good reference implementation for R7RS-large support
