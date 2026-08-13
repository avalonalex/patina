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
(also vendored, with the same pin, at `compat/vendor/srfi-27/`, which is where
these files were copied from). `27.scm` sits beside `27.sld` rather than in a
`27/` subdirectory so the `.sld`'s `(include "27.scm")` resolves unchanged.

The package ships no test suite; conformance is covered by
`crates/patina-tests/tests/srfi_27.rs` (both backends must agree on exact
pseudo-randomized streams) and by the SRFI 132 suite, whose quickselect
draws its pivots from `random-integer`.

The rule (audit 2026-08-10, group E): bundled files match upstream; an
unavoidable deviation is marked `;; PATINA LOCAL EDIT:` at the site and
recorded in the tree's one home. `crates/patina-tests/tests/bundled_provenance.rs`
pins every file listed here, so an unrecorded edit fails the suite.
