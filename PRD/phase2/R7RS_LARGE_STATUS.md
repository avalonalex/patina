# R7RS-Large Coverage and Bundling Policy

**Updated:** 2026-09-30

This is the current bundling policy and edition tracker. Track L is complete;
remaining library work is tracked in issues. Red's 17 library families are
available using SRFI 158 for generators. Tangerine has **9 of 10** adopted
items; integer division remains open in [#576](https://github.com/avalonalex/patina/issues/576).

## Bundling policy

**Bundle R7RS-large libraries (including drafts) and SRFIs. Keep libraries specific to another
Scheme implementation external.** Owner decision, 2026-09-12; this replaces the earlier restriction
to edition members plus selected exceptions.

- **R7RS-large draft membership or being a SRFI is sufficient for eligibility.** A SRFI need not
  belong to an R7RS-large edition, require runtime support, or meet a corpus popularity threshold.
- **Implementation-specific APIs stay out of the shipped bundle:** Chibi, Gauche, Gambit and Chez
  libraries remain external dependencies even when pure Scheme or needed by our test lanes.
  Those consumers supply them through `-A`, `-I` or `PATINA_LIBRARY_PATH`.
- **Judge the API, not the origin of its implementation.** A SRFI implementation sourced from
  Chibi is eligible under its SRFI interface. Port or internalize any implementation-specific
  helpers instead of shipping the foreign implementation's public library namespace. SRFI 130's
  inlined string helpers are the existing example.
- **Eligibility sets scope; demand and implementation cost set order.** This is not a claim that
  every SRFI already ships, or a requirement to implement all of them immediately. Runtime and
  FFI requirements can still defer an eligible library. Record the version of any draft implemented.

Patina's own public extensions and internal support libraries remain part of Patina. Other
third-party libraries are obtained separately; see the
[acquisition workflow](../future/PACKAGE_MANAGER_DESIGN.md) delivered by
[#195](https://github.com/avalonalex/patina/issues/195). SRFI 64 falls under the
general SRFI rule; `(chibi test)` remains external.

## Red Edition

**All 17 adopted library families are available.** Generators use the successor
SRFI 158; Patina does not ship a separate `(srfi 121)` library.

| Adopted API | Shipped implementation | R7RS-large name |
|-------------|------------------------|-----------------|
| SRFI 1 | `(srfi 1)` | `(scheme list)` |
| SRFI 14 | `(srfi 14)` | `(scheme charset)` |
| SRFI 41 | `(srfi 41)` | `(scheme stream)` |
| SRFI 101 | `(srfi 101)` | `(scheme rlist)` |
| SRFI 111 | `(srfi 111)` | `(scheme box)` |
| SRFI 113 | `(srfi 113)` | `(scheme set)` |
| SRFI 116 | `(srfi 116)` | `(scheme ilist)` |
| SRFI 117 | `(srfi 117)` | `(scheme list-queue)` |
| SRFI 121, superseded | `(srfi 158)` | `(scheme generator)` |
| SRFI 124 | `(srfi 124)` | `(scheme ephemeron)` |
| SRFI 125 | `(srfi 125)` | `(scheme hash-table)` |
| SRFI 127 | `(srfi 127)` | `(scheme lseq)` |
| SRFI 128 | `(srfi 128)` | `(scheme comparator)` |
| SRFI 132 | `(srfi 132)` | `(scheme sort)` |
| SRFI 133 | `(srfi 133)` | `(scheme vector)` |
| SRFI 134 | `(srfi 134)` | `(scheme ideque)` |
| SRFI 135 | `(srfi 135)` | `(scheme text)` |

Most aliases preserve the backing library's export names. `(scheme rlist)`
renames the SRFI 101 interface to avoid shadowing ordinary list operations
(`rcons`, `rcar`, `make-rlist`, and the list conversions).

## Tangerine Edition

**9 of 10 adopted items are available.** The
[final ballot results](https://groups.google.com/g/scheme-reports-wg2/c/ZDG-J5Mi2og)
adopted nine SRFIs plus R6RS bytevectors. Count each adopted item once: SRFI 146
provides two libraries, and SRFI 160 provides a family of libraries.

| Adopted API | R7RS-large name | Patina status |
|-------------|-----------------|---------------|
| SRFI 115 | `(scheme regex)` | Shipped over `(srfi 115)` |
| SRFI 141 | `(scheme division)` | **Missing**, including `(srfi 141)` — [#576](https://github.com/avalonalex/patina/issues/576) |
| SRFI 143 | `(scheme fixnum)` | Shipped over `(srfi 143)` |
| SRFI 144 | `(scheme flonum)` | Shipped over `(srfi 144)` |
| SRFI 146 | `(scheme mapping)`, `(scheme mapping hash)` | Shipped over `(srfi 146)` and `(srfi 146 hash)` |
| SRFI 151 | `(scheme bitwise)` | Shipped over `(srfi 151)` |
| SRFI 158 | `(scheme generator)` | Shipped over `(srfi 158)` |
| SRFI 159 | `(scheme show)` | Shipped over `(srfi 159)` |
| SRFI 160 | `(scheme vector base)`, `(scheme vector TYPE)` | Shipped over `(srfi 160 base)` and the twelve typed libraries |
| R6RS bytevectors | `(scheme bytevector)` | Shipped over `(r6rs bytevectors)` — [#575](https://github.com/avalonalex/patina/issues/575) |

`TYPE` denotes `u8`, `s8`, `u16`, `s16`, `u32`, `s32`, `u64`, `s64`, `f32`,
`f64`, `c64`, or `c128`. SRFI 4 supplies the numeric-vector substrate.

`(scheme bytevector)` shares bindings with `(r6rs bytevectors)` and
`(rnrs bytevectors)`. Its `bytevector-copy!` follows R6RS:
`(source source-start target target-start count)`. The `(scheme base)` procedure
remains destination-first, with optional source bounds. Use `except` or
`prefix` when importing both conventions; for example:

```scheme
(import (except (scheme base) bytevector-copy!)
        (scheme bytevector))
```

## Verification and provenance

Availability above means the libraries and import names ship; it does not claim
that every implementation has no known deviations. The executable checks and
provenance records give the narrower evidence:

- [`r7rs_large_aliases.rs`](../../crates/patina-tests/tests/r7rs_large_aliases.rs)
  checks alias export sets and shared bindings, with behavioral exercises on
  both backends. Bytevectors are checked against both R6RS names.
- [`bytevector-library.scm`](../../crates/patina-tests/tests/scheme/data/bytevector-library.scm)
  exercises the public bytevector name, both copy conventions, overlapping
  copies, integer/IEEE access, and text encodings. The CLI tests also require
  the alias to resolve with isolated library lookup.
- [`upstream_srfi_suites.rs`](../../crates/patina-tests/tests/upstream_srfi_suites.rs)
  and the [suite inventory](../../scheme_tests/upstream/README.md) track upstream
  coverage and explicit reasons for omissions. Larceny's separately obtained
  R6RS bytevector suite is additional coverage, not a vendored CI dependency.
- [SRFI provenance](../../lib/srfi/PROVENANCE.md) and
  [R6RS provenance](../../lib/r6rs/PROVENANCE.md) record sources, licenses, and
  adaptations. The bytevector alias reuses the pinned R6RS implementation.

When bundling third-party code, preserve its license notices, record provenance,
and pin it in `bundled_provenance.rs`. Register its upstream suite or document
why it cannot run; excluding a bundled package from the compatibility corpus
must not discard its test coverage. Keep the bundled SRFI 64 copy pinned and
its summary wording stable, since the compatibility classifier consumes it.
SRFI 64 drivers must read the runner counts or call `test-exit`: `test-end`
alone can return successfully after a failure or unexpected pass.

R7RS-small is implemented and remains gated by the two Chibi backend scripts;
the old I/O, exception, records, and system-interface queue is retired.

## Beyond Red and Tangerine

Eligibility extends to later R7RS-large drafts and other SRFIs under the policy
above. This table is not an inventory of every SRFI Patina ships, nor a claim
that R7RS-large as a whole is complete. New implementation work belongs in
GitHub issues, with the adopted draft version recorded where applicable.

For the [Macrological Fascicle, draft 1](https://r7rs.org/large/fascicles/macro/1/),
Patina ships the splicing local syntax forms as `(srfi 188)` (#424). This does
not implement its procedural macro facilities; see the
[current syntax-case design](../macro/SYNTAX_CASE_DESIGN.md).

## Historical record

The completed compatibility work is summarized in the
[Track L archive](../ARCHIVE/TRACK_L_LEFTOVERS.md). The superseded policies,
porting narrative, and old priority lists remain available in the
[pre-cleanup revision of this tracker](https://github.com/avalonalex/patina/blob/ca1a1762c1c40be7c28896f9983809b6bbfcca95/PRD/phase2/R7RS_LARGE_STATUS.md).
That revision contains stale coverage claims, including the omitted SRFI 141;
use the tables above for current status.
