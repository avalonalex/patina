# R7RS-Large Status Tracking

**Last Updated:** 2026-08-08 — reconciled shipped-SRFI status against `lib/srfi/`, added the bundling policy and a measured priority order

This document tracks the status of R7RS-large editions and Patina's support for them.

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
| SRFI 101 | `(scheme rlist)` | Random-access lists | ❌ | ❌ |
| SRFI 111 | `(scheme box)` | Boxes (single-value containers) | ✅ shipped | ✅ shipped |
| SRFI 113 | `(scheme set)` | Sets and bags | ✅ shipped | ✅ shipped |
| SRFI 116 | `(scheme ilist)` | Immutable lists | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 116)`, 2026-08-25) |
| SRFI 117 | `(scheme list-queue)` | List queues | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 117)`, 2026-08-25) |
| SRFI 121 | `(scheme generator)` | Generators | — superseded by SRFI 158 | — |
| SRFI 124 | `(scheme ephemeron)` | Ephemerons | ❌ | ❌ |
| SRFI 125 | `(scheme hash-table)` | Hash tables | ✅ | ✅ |
| SRFI 127 | `(scheme lseq)` | Lazy sequences | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 127)`, 2026-08-25) |
| SRFI 128 | `(scheme comparator)` | Comparators | ✅ shipped | ✅ shipped |
| SRFI 132 | `(scheme sort)` | Sort libraries | ✅ shipped | ✅ shipped |
| SRFI 133 | `(scheme vector)` | Vector library | ✅ shipped | ✅ shipped |
| SRFI 134 | `(scheme ideque)` | Immutable deques | ✅ shipped | ✅ shipped (alias over the bundled `(srfi 134)`, 2026-08-25) |
| SRFI 135 | `(scheme text)` | Immutable texts | ❌ | ❌ |

**Red status: 13 of 17 shipped** (SRFI 1, 14, 41, 111, 113, 116, 117, 125, 127, 128, 132, 133, 134), all reachable under both `(srfi n)` and their `(scheme …)` names (14 counting SRFI 158 standing in for the superseded 121).

**Notes:**
- SRFI 129 (titlecase) was voted down
- SRFI 13 (strings) marked for reballoting
- The `(scheme …)` alias libraries landed in `lib/scheme/{list,box,set,comparator,sort,vector,generator}.sld`.
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
| SRFI 146 | `(scheme mapping)` | Mappings | ❌ | ❌ |
| SRFI 146 | `(scheme mapping hash)` | Hash mappings | ❌ | ❌ |
| SRFI 151 | `(scheme bitwise)` | Bitwise operations | ✅ shipped | ✅ shipped |
| SRFI 158 | `(scheme generator)` | Generators (supersedes SRFI 121) | ✅ shipped | ✅ shipped |
| SRFI 159 | `(scheme show)` | Formatting/show | ❌ | ❌ |
| SRFI 160 | `(scheme vector @)` | Numeric vectors (u8, s8, f64, etc.) | ❌ | ❌ |
| R6RS | `(scheme bytevector)` | Bytevectors (R6RS compatible) | 🚧 Partial | ❌ |

**Tangerine status: 3 of 7 shipped**, reachable under both names.

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

Two additions are needed, because standard-track membership alone does not cover everything that must
work:

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

**Explicitly out of scope:** pure-Scheme leaf libraries that are neither standard-track nor
runtime-forced. They work fine from a `-A` directory or the vendored corpus, and bundling them makes
Patina a slow package manager for code it does not need to own.

### Ordering

The policy fixes the *set*; measured dependency in-degree over `compat/vendor/` fixes the *order*.
Highest-value first:

1. ~~Bitwise — SRFI 151 + 60/33 shims~~ — **done**. Core operators are Rust primitives in
   `(patina internal bitwise)`; the ~30 derived procedures are Scheme. `(srfi 60)` (in-degree 31)
   and `(srfi 33)` (19) are renames over the same bindings, not separate ports.
2. ~~`(scheme …)` alias libraries for the shipped SRFIs~~ — **done**; see the tables above.
3. ~~**SRFI 125 hash tables**, superseding the shipped SRFI 69 (in-degree 16); keep 69 as an
   alias.~~ ✅ **done 2026-08-16, and not that way.** SRFI 125 is a layer *over* SRFI 69 and
   SRFI 128, not a replacement: SRFI 69 stays as the substrate and keeps its own narrower
   semantics, because it is a separate published SRFI with 16 corpus importers. Four deviations
   were needed and are recorded in `lib/srfi/125.sld`'s header.
4. **SRFI 27 random** — runtime-forced, in-degree 9.
5. ~~SRFI 143 fixnums~~ — **done**. Mostly renames over SRFI 151 and `(scheme base)`; the part that
   had to be right is `fx-width` / `fx-greatest` / `fx-least`, derived by probing `fixnum?` rather
   than hardcoded so the library cannot claim a range the tagging does not provide.
6. **SRFI 14 char-sets** (in-degree 4), then the remaining Red data structures (41, 101, 116, 117,
   124, 127, 134, 135) and Tangerine (146, 159, 160), which are standard-track but show little
   ecosystem demand.
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
