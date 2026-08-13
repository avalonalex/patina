# Provenance of byte-identical trees under `lib/srfi/`

Libraries whose bundled files are **all** byte-identical to upstream are
recorded here — with no local edit, there is no file to carry an in-file
note. Libraries that do deviate document themselves in their own `.sld`
header instead (`lib/srfi/132.sld` is the model), and `lib/chibi/` has its
own record. One home per tree.

| Package | Version | Files | Upstream | Tarball sha256 |
|---|---|---|---|---|
| `(srfi 27)` | 2025.12.14 | `27.sld`, `27.scm` | snow-fort, Retropikzel's R7RS port of Sebastian Egner's 54-bit MRG32k3a reference implementation (MIT) | `b8d2322e40955ccc986e9b0b10c1c36044ff5c659d33698722f9b36ec77fdea5` |

Tarball URL: `http://snow-fort.org/s/iki.fi/retropikzel/srfi/27/2025.12.14/srfi-27-2025.12.14.tgz`
(the compat corpus vendored the same tarball until Patina bundled it; the
corpus drops packages Patina provides). `27.scm` sits beside `27.sld` rather
than in a `27/` subdirectory so the `.sld`'s `(include "27.scm")` resolves
unchanged.

The package ships no test suite; conformance is covered by
`crates/patina-tests/tests/srfi_27.rs` (both backends must agree on exact
pseudo-randomized streams) and by the SRFI 132 suite, whose quickselect
draws its pivots from `random-integer`.

The rule and its enforcement: `lib/chibi/PROVENANCE.md` § The rule, and
`crates/patina-tests/tests/bundled_provenance.rs`, whose `PINNED` table is
the authoritative scope.

**The boundary:** the adapted ports elsewhere in this tree (SRFI 1, 69, 113,
128, 133, 158, …) are *not* byte-identical to any upstream and are
deliberately unpinned; their sources and known deviations are Track L
territory (`PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`) until each is reconciled to
this standard or recorded here.
