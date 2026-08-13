# SRFI Reference Implementations

This directory contains reference implementations of Scheme Requests
for Implementation (SRFIs) adapted for use with Patina.

## License

All SRFI reference implementations are provided under the **MIT License**.
See the [LICENSE](./LICENSE) file and individual source files for specific
copyright notices.

## Provenance

Most implementations are the authors' reference implementations, adapted
for Patina. Two trees meet a stricter, machine-checked standard —
byte-identical to a pinned upstream, enforced by
`crates/patina-tests/tests/bundled_provenance.rs`: `(srfi 27)` (recorded in
[PROVENANCE.md](./PROVENANCE.md)) and `(srfi 132)` (recorded in its own
`132.sld` header). The rest are adaptations; their known deviations are
listed below, and reconciling each one to the byte-identical-or-marked
standard is tracked in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`.

**The Patina project does not claim authorship of these implementations.**
They are included here for convenience and compatibility. Bug reports
related to SRFI semantics should be directed to the upstream SRFI
process; bugs related to Patina-specific adaptation should be reported
to the Patina project.

## Included SRFIs

| SRFI | Name | Source | Authors |
|------|------|--------|---------|
| 1 | List Library | [srfi-1](https://srfi.schemers.org/srfi-1/) | Olin Shivers |
| 8 | receive | [srfi-8](https://srfi.schemers.org/srfi-8/) | John David Stone |
| 27 | Sources of Random Bits | snow-fort `srfi-27` — Retropikzel's R7RS port of the reference implementation; see [PROVENANCE.md](./PROVENANCE.md) | Sebastian Egner |
| 33 | Integer Bitwise-operation Library | chibi-scheme's `(srfi 33)` | Olin Shivers (spec) |
| 60 | Integers as Bits | Patina shim over SRFI 151 (`60.sld`, `60/`) | Aubrey Jaffer (spec) |
| 69 | Basic Hash Tables | [srfi-69](https://srfi.schemers.org/srfi-69/) | Panu Kalliokoski |
| 111 | Boxes | [srfi-111](https://srfi.schemers.org/srfi-111/) | John Cowan |
| 113 | Sets and Bags | [srfi-113](https://srfi.schemers.org/srfi-113/) | John Cowan |
| 128 | Comparators (reduced) | [srfi-128](https://srfi.schemers.org/srfi-128/) | John Cowan |
| 132 | Sort Libraries | [srfi-132](https://srfi.schemers.org/srfi-132/), pinned commit in `132.sld` | Olin Shivers, John Cowan |
| 133 | Vector Library | [srfi-133](https://srfi.schemers.org/srfi-133/) | Taylor Campbell (SRFI 43), John Cowan (SRFI 133) |
| 143 | Fixnums | chibi-scheme's implementation | John Cowan (spec) |
| 151 | Bitwise Operations | chibi-scheme's implementation | John Cowan (spec) |
| 158 | Generators and Accumulators | [srfi-158](https://srfi.schemers.org/srfi-158/) | Shiro Kawai, John Cowan, Thomas Gilray |

## Modifications from Upstream

- Library names use `(srfi N)` form for R7RS compatibility
- Minor path adjustments for Patina's library loading conventions
- SRFI 1: Added R7RS shim for `check-arg`, `let-optionals`, `:optional`; imports `(srfi 8)` for `receive`; `patina-patches.scm` replaces the reference's `%cdrs`/`%cars+cdrs`/`%cars+cdrs+` (their `call/cc` abort optimization interacts poorly with VM library loading)
- SRFI 113: Imports R5RS aliases from `(scheme r5rs)` instead of manual shim
- SRFI 128: Uses Patina's native `equal-hash` Rust primitive via `(patina internal predicates)`
- SRFI 133: Removed form-feed characters (now supported by lexer, but kept removed for cleanliness)
- SRFI 158: carries an inline simplified `any` at the top of `srfi-158-impl.scm`
- SRFI 33/60/132/143/151: carry the bug fixes from the 2026-08-10 post-merge
  audit (`PRD/AUDIT_2026_08_10_PRD.md`, group A) — wrong answers found by the
  upstream reference suites, fixed with regression tests
- This list is best-effort for the adapted ports; only the byte-identical
  trees above are machine-verified against upstream
