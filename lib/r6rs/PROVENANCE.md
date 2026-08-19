# Provenance of `lib/r6rs/`

William D Clinger's R7RS ports of the R6RS libraries, from snow-fort. The
bundled files are **byte-identical to the vendored tarballs**, with one
documented exception noted below, so there is no per-file note to carry — the
model is `lib/srfi/PROVENANCE.md`.

`lib/rnrs/` sits on top of these: one `.sld` per library that imports the
`(r6rs …)` name and re-exports it, so that R6RS source, which spells its
imports `(rnrs …)`, resolves without being rewritten. Those shims are ours,
generated from the export lists here, and
`crates/patina-tests/tests/r6rs_rnrs_shims.rs` fails if the two drift apart.

Licence: MIT for every package, plus Clinger's own notice at the head of each
file, which is preserved verbatim.

## `(r6rs no-rnrs)` — added by Patina, not upstream

`lib/r6rs/no-rnrs.sld` has no upstream counterpart. Every guard in this tree
reads

```scheme
(and (or (library (rnrs base)) larceny) (not (library (r6rs no-rnrs))))
```

so that on a host which already has `(rnrs …)` these libraries become
re-exports of it rather than a second implementation. Patina has `(rnrs …)`
only because `lib/rnrs/` re-exports *these* libraries, so that branch would
close a cycle. Defining the marker is upstream's own way of saying "what you
can see is not a host implementation", and it sends every guard to the
portable R7RS branch — which is why the rest of the tree needs no edit.

## The one deviation

`hashtables.sld`'s first `cond-expand` guards on `(library (rnrs hashtables))`
**alone**, where the two others in the same file, and every guard in every
other file, also require `(not (library (r6rs no-rnrs)))`. With
`lib/rnrs/hashtables.sld` present that branch fires and `inexact-hash` and its
neighbours are never defined, so every non-fixnum key fails at the *caller*
with `unbound variable: inexact-hash`. Measured: without the shim a float,
rational and symbol key all work; with it, none do. The edit restores the
missing conjunct and nothing else, and is marked `PATINA DEVIATION` in place.

## Files

| Package | Version | Files | Tarball sha256 |
|---|---|---|---|
| `(r6rs arithmetic fixnums)` | 0.0.1 | `arithmetic/fixnums.body.scm`, `arithmetic/fixnums.sld` | `a2839d9592f23c3859ace50bd3132dc37c2691702a382e873c76156b782a6378` |
| `(r6rs base)` | 0.0.1 | `base.body.scm`, `base.sld` | `c3b23446ad2d17ff377006e9756a5e27a89dcd1ab65792e49e01efd5424a6c0d` |
| `(r6rs bytevectors)` | 0.0.1 | `bytevectors.body.scm`, `bytevectors.sld` | `404c59936cd1b67bba8dd58219a535a71fa39d18be034ae7e8b1faad41eb782f` |
| `(r6rs control)` | 0.0.1 | `control.sld` | `b38fb48bac18d46a9a84ecd4ba64ca83d272abf3d789307a49a93a4230965a6a` |
| `(r6rs enums)` | 0.0.1 | `enums.body.scm`, `enums.sld` | `f86c2a234a75fc494f088d831e17cfe23328f5244ea6677bda44dc6451affcbd` |
| `(r6rs eval)` | 0.0.1 | `eval.sld` | `b0e87f14188769d21f5f1c96e4f8a5b4e4dda4d57a8509d313c4da2b6e12f57c` |
| `(r6rs exceptions)` | 0.0.1 | `exceptions.sld` | `9555ee076c379a3f622b9ca2812cf3332a9d9afecd115d7e3b2ddda8aaa9b97f` |
| `(r6rs files)` | 0.0.1 | `files.sld` | `6b674bf8735431e5a1cc809018d0d19b6ffc02ca56781328290de413db5829ef` |
| `(r6rs hashtables)` | 0.0.1 | `hashtables.atop69.scm`, `hashtables.body69.scm`, `hashtables.sld` | `d93f85d56c7eadb1e1603bf95988302bd1e111e3247bb1ddba7d271148633b12` |
| `(r6rs io simple)` | 0.0.1 | `io/simple.sld` | `7f49ae295e3f6fa420e2ccfdbf446dc46cc814bdfb436046191bd8e146346b00` |
| `(r6rs lists)` | 0.0.1 | `lists.body.scm`, `lists.sld` | `46507daa47b2226eeac6e00418301d6e9c46940a744ec9f0e07ac4efb93bb49e` |
| `(r6rs mutable-pairs)` | 0.0.1 | `mutable-pairs.sld` | `850c349b4aadeda7c8c52afcf0b9f237b0b641f70d9786f77490cbf4b13e8bf1` |
| `(r6rs mutable-strings)` | 0.0.1 | `mutable-strings.sld` | `c5f3a290e89314f1572cd7f31ab3238bd4e3c8b0883ebf281b56e174b13031a1` |
| `(r6rs programs)` | 0.0.1 | `programs.sld` | `b10955572de97a44524ce699dbf6c7607c75bb6000ba845051b1e927bd72370b` |
| `(r6rs r5rs)` | 0.0.1 | `r5rs.sld` | `a049e687207b76608448460b70eca530d0b495a8348bff5953fa911abf73772a` |
| `(r6rs sorting)` | 0.0.1 | `sorting.body.scm`, `sorting.sld` | `75235b206c41016a825fbba98d7f6c27ee46a4da089eb13e4585751cfdfb5ef9` |
| `(r6rs unicode)` | 0.0.1 | `unicode.sld` | `0da7ecacbc53ef8ce918b7383bc5309d8bb171cfa3db41e9f229a266faf46650` |
| `(r6rs unicode-reference unicode0)` | 0.0.1 | `unicode-reference/unicode0.body.scm`, `unicode-reference/unicode0.sld` | `1450a1e3defa951e407d4acb9151dc6aca3a5993b620dc7b2c05cd531c8951c4` |
| `(r6rs unicode-reference unicode1)` | 0.0.1 | `unicode-reference/unicode1.body.scm`, `unicode-reference/unicode1.sld` | `ed65f577c1928918c734406431a63b5c31278794793cf40fa8035c339e677fb9` |
| `(r6rs unicode-reference unicode2)` | 0.0.1 | `unicode-reference/unicode2.body.scm`, `unicode-reference/unicode2.sld` | `e46d3fd17c6305c35542ae783a578f5f2324e67d140b6bd84168ddd42190b24c` |
| `(r6rs unicode-reference unicode3)` | 0.0.1 | `unicode-reference/unicode3.body.scm`, `unicode-reference/unicode3.sld` | `62fba6dbb3ddd123b392a538e01aa1db0bff1b8939505a7071619297f0af17ff` |
| `(r6rs unicode-reference unicode4)` | 0.0.1 | `unicode-reference/unicode4.body.scm`, `unicode-reference/unicode4.sld` | `85d46c2a686c606aa504245abf55036de6baac5675a809bca74ac469b1a75222` |

Tarball URLs are `http://snow-fort.org/s/ccs.neu.edu/will/r6rs/<name>/<version>/`
`r6rs-<name>-<version>.tgz`; the exact bytes are pinned by the digests above and
the same tarballs remain vendored in `compat/vendor/`, where the corpus drops
any package whose library Patina bundles.
