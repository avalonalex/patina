# SRFI Reference Implementations

This directory contains reference implementations of Scheme Requests
for Implementation (SRFIs) adapted for use with Patina.

## License

All SRFI reference implementations are provided under the **MIT License**.
See the [LICENSE](./LICENSE) file and individual source files for specific
copyright notices.

## Provenance

These implementations are sourced from the official SRFI repository at
<https://srfi.schemers.org>. They are the canonical reference
implementations written by their respective authors.

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
| 69 | Basic Hash Tables | [srfi-69](https://srfi.schemers.org/srfi-69/) | Panu Kalliokoski |
| 111 | Boxes | [srfi-111](https://srfi.schemers.org/srfi-111/) | John Cowan |
| 113 | Sets and Bags | [srfi-113](https://srfi.schemers.org/srfi-113/) | John Cowan |
| 128 | Comparators (reduced) | [srfi-128](https://srfi.schemers.org/srfi-128/) | John Cowan |
| 132 | Sort Libraries | [srfi-132](https://srfi.schemers.org/srfi-132/) | John Cowan |
| 133 | Vector Library | [srfi-133](https://srfi.schemers.org/srfi-133/) | Taylor Campbell (SRFI 43), John Cowan (SRFI 133) |
| 158 | Generators and Accumulators | [srfi-158](https://srfi.schemers.org/srfi-158/) | Shiro Kawai, John Cowan, Thomas Gilray |

## Modifications from Upstream

- Library names use `(srfi N)` form for R7RS compatibility
- Minor path adjustments for Patina's library loading conventions
- SRFI 1: Added R7RS shim for `check-arg`, `let-optionals`, `:optional`; imports `(srfi 8)` for `receive`
- SRFI 113: Imports R5RS aliases from `(scheme r5rs)` instead of manual shim
- SRFI 128: Uses Patina's native `equal-hash` Rust primitive via `(patina internal predicates)`
- SRFI 133: Removed form-feed characters (now supported by lexer, but kept removed for cleanliness)
- No semantic changes to the reference implementations beyond the above
