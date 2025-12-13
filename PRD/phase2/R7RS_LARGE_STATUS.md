# R7RS-Large Status Tracking

**Last Updated:** 2025-12-04

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

| SRFI | Library Name | Description | Patina Status |
|------|-------------|-------------|---------------|
| SRFI 1 | `(scheme list)` | List library | ❌ Not started |
| SRFI 14 | `(scheme charset)` | Character sets | ❌ Not started |
| SRFI 41 | `(scheme stream)` | Streams (lazy lists) | ❌ Not started |
| SRFI 101 | `(scheme rlist)` | Random-access lists | ❌ Not started |
| SRFI 111 | `(scheme box)` | Boxes (single-value containers) | ❌ Not started |
| SRFI 113 | `(scheme set)` | Sets and bags | ❌ Not started |
| SRFI 116 | `(scheme ilist)` | Immutable lists | ❌ Not started |
| SRFI 117 | `(scheme list-queue)` | List queues | ❌ Not started |
| SRFI 121 | `(scheme generator)` | Generators | ❌ Not started |
| SRFI 124 | `(scheme ephemeron)` | Ephemerons | ❌ Not started |
| SRFI 125 | `(scheme hash-table)` | Hash tables | ❌ Not started |
| SRFI 127 | `(scheme lseq)` | Lazy sequences | ❌ Not started |
| SRFI 128 | `(scheme comparator)` | Comparators | ❌ Not started |
| SRFI 132 | `(scheme sort)` | Sort libraries | ❌ Not started |
| SRFI 133 | `(scheme vector)` | Vector library | ❌ Not started |
| SRFI 134 | `(scheme ideque)` | Immutable deques | ❌ Not started |
| SRFI 135 | `(scheme text)` | Immutable texts | ❌ Not started |

**Notes:**
- SRFI 129 (titlecase) was voted down
- SRFI 13 (strings) marked for reballoting

---

### Tangerine Edition (2019)

**Status:** ✅ Approved
**Focus:** Data Structures and Numerics

| SRFI | Library Name | Description | Patina Status |
|------|-------------|-------------|---------------|
| SRFI 115 | `(scheme regex)` | Regular expressions | ❌ Not started |
| SRFI 143 | `(scheme fixnum)` | Fixnums | ❌ Not started |
| SRFI 146 | `(scheme mapping)` | Mappings | ❌ Not started |
| SRFI 146 | `(scheme mapping hash)` | Hash mappings | ❌ Not started |
| SRFI 151 | `(scheme bitwise)` | Bitwise operations | ❌ Not started |
| SRFI 158 | `(scheme generator)` | Generators (supersedes SRFI 121) | ❌ Not started |
| SRFI 159 | `(scheme show)` | Formatting/show | ❌ Not started |
| SRFI 160 | `(scheme vector @)` | Numeric vectors (u8, s8, f64, etc.) | ❌ Not started |
| R6RS | `(scheme bytevector)` | Bytevectors (R6RS compatible) | 🚧 Partial |

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

### Phase 2: Foundational R7RS-large

After R7RS-small compliance:

**High Priority (widely used):**
1. SRFI 1 `(scheme list)` - Extended list operations
2. SRFI 125 `(scheme hash-table)` - Hash tables
3. SRFI 128 `(scheme comparator)` - Comparators (needed by many others)
4. SRFI 132 `(scheme sort)` - Sorting
5. SRFI 133 `(scheme vector)` - Extended vector operations

**Medium Priority:**
- SRFI 115 `(scheme regex)` - Regular expressions
- SRFI 151 `(scheme bitwise)` - Bitwise operations
- SRFI 143 `(scheme fixnum)` - Fixnums

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
