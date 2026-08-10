# Licences of vendored packages

Every package in this directory retains its own licence and its authors' copyright. Nothing here is
relicensed under Patina's MIT licence, and vendoring changes no terms. This file records what those
terms are, how each was determined, and what obligations they place on this repository.

`MANIFEST.json` is the machine-readable source of truth: `license`, `license_class`, and
`license_evidence` per package.

## Inventory

| Licence | Packages | Class |
|---|---|---|
| BSD | 63 | permissive |
| MIT | 54 | permissive |
| SLIB (Aubrey Jaffer) | 44 | **non-standard permissive** |
| public domain | 17 | permissive |
| ISC | 5 | permissive |
| CC0-1.0 | 2 | permissive |
| MIT Scheme, 1988–1995 form | 1 | **non-standard permissive** |
| Apache-2.0 | 1 | permissive |
| Expat | 1 | permissive |

**Also excluded:** nine packages whose library Patina bundles itself — see `README.md`.

**Excluded and not vendored:** GPL-3.0 (18), GPL (8), MPL-2.0 (2), 53 packages with no discoverable
licence, and `(srfi 5)` under a document-only licence (see below). Listed in `REVIEW-QUEUE.json`.

## How each licence was determined

Resolution order, most to least reliable. The method used is recorded per package so weak calls stay
auditable.

| Method | Packages | Confidence |
|---|---|---|
| `spdx` — `SPDX-License-Identifier:` tag in the source | 11 | authoritative |
| `licence-text` — full licence body matched in a shipped file | 53 | high |
| `index-field` — the `license` field in snow-fort's `repo.scm` | 98 | good (author-declared, unverified) |
| `text-match` — a licence grant paragraph matched in a shipped file | 19 | good |
| `srfi-canonical-document` — the MIT grant published in the defining SRFI at srfi.schemers.org | 9 | good |
| `slib-family-inference` — **inferred**, see caveat below | 7 | weakest |

**Caveat on inference.** Snowballs generally contain no `LICENSE` file — only sources, `package.scm`,
and a manual — so licence text usually had to be read out of source headers. Seven `slib *` packages
carry no notice in any shipped file; they were classified as SLIB/Jaffer because they come from the
same `SLIB-3b5-r7rs` distribution by the same author as the 38 that do. That is a reasonable
inference, not a verified fact. If it matters, confirm against SLIB upstream.

## The two SRFI licence regimes

SRFIs do not all use the same licence, and the difference matters when vendoring the reference
implementation as code:

| Regime | Tell-tale wording | Usable as software? |
|---|---|---|
| **MIT** — most SRFIs | "Permission is hereby granted, free of charge… to deal in the Software without restriction" | yes |
| **SRFI document licence** — early SRFIs | "This document and translations of it may be copied and furnished to others… However, this document itself may not be modified in any way" | no — it is a *documentation* licence |

The second is derived from the IETF/W3C document licence. It permits copying and redistributing the
document, and derivative works that "assist in its implementation", but it never cleanly grants
software rights to the reference code and forbids modifying the document itself.

**`(srfi 5)` (Andy Gaynor, 1999) is under the document licence and is therefore not vendored.** Other
implementations do ship these old reference implementations, but the grant is ambiguous enough that
it is not worth muddying this repository's licence story for a library with an in-degree of 0. To
recheck any SRFI by hand: read the Copyright section at the bottom of
`https://srfi.schemers.org/srfi-N/srfi-N.html`, then the `LICENSE` file in
`github.com/scheme-requests-for-implementation/srfi-N`, and finally the reference implementation's
own header — which can differ from the document. `(srfi 170)`, for instance, is MIT overall but
carries BSD-3-Clause on the parts derived from scsh.

## Non-standard permissive licences

46 packages are under two old, permissive, non-OSI-listed licences. Both grant everything needed for
this use — copy, modify, redistribute, any purpose — and **neither is copyleft**. They are called out
because they are not off-the-shelf terms and carry conditions worth knowing.

### SLIB licence (Aubrey Jaffer) — 45 packages

Notably including `(srfi 60)`, the third most-depended-on library in the entire snow-fort ecosystem.
Verbatim, as it appears in the sources:

> Copyright (C) 1991, 1993, 2001, 2003, 2005 Aubrey Jaffer
>
> Permission to copy this software, to modify it, to redistribute it, to distribute modified
> versions, and to use it for any purpose is granted, subject to the following restrictions and
> understandings.
>
> 1. Any copy made of this software must include this copyright notice in full.
> 2. I have made no warranty or representation that the operation of this software will be
>    error-free, and I am under no obligation to provide any services, by way of maintenance,
>    update, or otherwise.
> 3. In conjunction with products arising from the use of this material, there shall be no use of my
>    name in any advertising, promotional, or sales literature without prior written consent in each
>    case.

### MIT Scheme licence, 1988–1995 form — 1 package

`(srfi 14)`, the SRFI 14 char-set reference implementation, derived from the MIT Scheme runtime. This
is **not** the MIT/Expat licence and it is **not** the GPL that modern MIT/GNU Scheme uses; it is the
older MIT Scheme distribution licence:

> Copyright (c) 1988-1995 Massachusetts Institute of Technology
>
> Permission to copy and modify this software, to redistribute either the original software or a
> modified version, and to use this software for any purpose is granted, subject to the following
> restrictions and understandings.
>
> 1. Any copy made of this software must include this copyright notice in full.
> 2. Users of this software agree to make their best efforts (a) to return to the MIT Scheme project
>    any improvements or extensions that they make, so that these may be included in future
>    releases; and (b) to inform MIT of noteworthy uses of this software.
> 3. All materials developed as a consequence of the use of this software shall duly acknowledge
>    such use, in accordance with the usual standards of acknowledging credit in academic research.
> 4. MIT has made no warrantee or representation that the operation of this software will be
>    error-free, and MIT is under no obligation to provide any services, by way of maintenance,
>    update, or otherwise.
> 5. In conjunction with products arising from the use of this material, there shall be no use of
>    the name of the Massachusetts Institute of Technology nor of any adaptation thereof in any
>    advertising, promotional, or sales literature without prior written consent from MIT in each
>    case.

### What these require of this repository

Three obligations, all currently satisfied:

1. **Reproduce the copyright notice in full.** Satisfied because every file is vendored unmodified,
   notices intact. This is the concrete reason the no-modification rule in `README.md` matters: strip
   a header and the licence is breached.
2. **Do not use the authors' names to promote anything.** Neither Aubrey Jaffer nor MIT may appear in
   Patina's advertising or promotional material. Listing them here, as an accurate record of what the
   test corpus contains, is not promotional use — but "tested against Aubrey Jaffer's SLIB" as a
   selling point would be.
3. **No warranty** from any author, for any of this.

The MIT Scheme licence's clause 2 asks users to make "best efforts" to return improvements to the MIT
Scheme project and to inform MIT of noteworthy uses. It is phrased as an agreement rather than a
condition of the grant, and is generally treated as a request. It is moot here regardless: the code is
vendored unmodified and only executed as a test input, so there are no improvements to return.

## Complete non-standard package list

| Package | In-degree | Licence | Evidence |
|---|---|---|---|
| `(slib common)` | 20 | SLIB-Jaffer | slib-family-inference |
| `(srfi 63)` | 9 | SLIB-Jaffer | licence-text |
| `(srfi 14)` | 4 | MIT-Scheme-old | licence-text |
| `(slib color)` | 3 | SLIB-Jaffer | licence-text |
| `(slib filename)` | 3 | SLIB-Jaffer | licence-text |
| `(slib printf)` | 3 | SLIB-Jaffer | licence-text |
| `(slib scanf)` | 3 | SLIB-Jaffer | licence-text |
| `(slib time-core)` | 3 | SLIB-Jaffer | licence-text |
| `(slib array-for-each)` | 2 | SLIB-Jaffer | licence-text |
| `(slib byte)` | 2 | SLIB-Jaffer | licence-text |
| `(slib color-space)` | 2 | SLIB-Jaffer | licence-text |
| `(slib generic-write)` | 2 | SLIB-Jaffer | slib-family-inference |
| `(slib pretty-print)` | 2 | SLIB-Jaffer | slib-family-inference |
| `(slib string-port)` | 2 | SLIB-Jaffer | licence-text |
| `(slib subarray)` | 2 | SLIB-Jaffer | licence-text |
| `(slib time-zone)` | 2 | SLIB-Jaffer | licence-text |
| `(slib alist)` | 1 | SLIB-Jaffer | licence-text |
| `(slib coerce)` | 1 | SLIB-Jaffer | licence-text |
| `(slib common-list-functions)` | 1 | SLIB-Jaffer | licence-text |
| `(slib directory)` | 1 | SLIB-Jaffer | licence-text |
| `(slib modular)` | 1 | SLIB-Jaffer | licence-text |
| `(slib rev2-procedures)` | 1 | SLIB-Jaffer | licence-text |
| `(slib tzfile)` | 1 | SLIB-Jaffer | licence-text |
| `(slib xml-parse)` | 1 | SLIB-Jaffer | licence-text |
| `(slib array-interpolate)` | 0 | SLIB-Jaffer | licence-text |
| `(slib byte-number)` | 0 | SLIB-Jaffer | licence-text |
| `(slib chapter-order)` | 0 | SLIB-Jaffer | licence-text |
| `(slib charplot)` | 0 | SLIB-Jaffer | licence-text |
| `(slib common-lisp-time)` | 0 | SLIB-Jaffer | licence-text |
| `(slib daylight)` | 0 | SLIB-Jaffer | licence-text |
| `(slib determinant)` | 0 | SLIB-Jaffer | licence-text |
| `(slib dynamic)` | 0 | SLIB-Jaffer | slib-family-inference |
| `(slib factor)` | 0 | SLIB-Jaffer | licence-text |
| `(slib fourier-transform)` | 0 | SLIB-Jaffer | licence-text |
| `(slib line-io)` | 0 | SLIB-Jaffer | licence-text |
| `(slib math-integer)` | 0 | SLIB-Jaffer | licence-text |
| `(slib math-real)` | 0 | SLIB-Jaffer | licence-text |
| `(slib nbs-iscc)` | 0 | SLIB-Jaffer | slib-family-inference |
| `(slib posix-time)` | 0 | SLIB-Jaffer | licence-text |
| `(slib pprint-file)` | 0 | SLIB-Jaffer | licence-text |
| `(slib random-inexact)` | 0 | SLIB-Jaffer | licence-text |
| `(slib rationalize)` | 0 | SLIB-Jaffer | slib-family-inference |
| `(slib resene)` | 0 | SLIB-Jaffer | licence-text |
| `(slib saturate)` | 0 | SLIB-Jaffer | slib-family-inference |
| `(slib uri)` | 0 | SLIB-Jaffer | licence-text |

## Not legal advice

This is an engineer's reading of licence texts, recorded so the reasoning is visible and checkable.
The classification of the two non-standard licences as permissive, and the treatment of the MIT
Scheme "best efforts" clause as non-binding, are judgements. If Patina's distribution terms ever
depend on them, get an actual review.
