# Vendored third-party R7RS libraries

**This directory is test data. It is not part of Patina.**

184 third-party Scheme packages from [snow-fort.org](http://snow-fort.org), vendored so Patina can be
measured against real-world R7RS code (Track L — `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`).

## Purpose, and what this is not

These packages exist here for **one reason: to run them and find out what Patina gets wrong.** That is
the whole relationship.

- **Not a dependency.** Nothing here is compiled into, linked against, imported by, or shipped with
  the interpreter. Deleting this directory does not affect the build.
- **Not an endorsement.** Inclusion says a package is *popular and permissively licensed*, nothing
  more. It is not a claim that the code is correct, secure, maintained, or worth using. Selection was
  mechanical — dependency in-degree and a licence check — with no review of the code itself.
- **Not a fork or a redistribution channel.** These are unmodified upstream copies kept for testing.
  Anyone wanting to *use* these libraries should get them from upstream, not from here, where they
  will be stale the moment upstream moves.
- **Not vetted.** The code has not been audited. Some of it is decades old, some targets other Scheme
  implementations, some will not run under Patina at all — which is precisely the point.

Copyright in each package remains with its authors, under its own licence. See `LICENSES.md` for the
full inventory and the obligations that come with it.

## Selection

Every package in the snow-fort index (285 entries; 278 after deduplicating versions) was downloaded
and inspected. Ranking is by **dependency in-degree** — how many other packages import a library this
one provides — because snow-fort publishes no download counts. Everything permissively licensed was
vendored; there is no popularity cutoff, since at 8 MB the tail costs nothing and adds coverage.

| Bucket | Packages | Vendored |
|---|---|---|
| Permissive (BSD 63, MIT 54, public-domain 17, ISC 5, CC0 2, Apache-2.0 1, Expat 1) | 143 | yes |
| Non-standard permissive (SLIB/Jaffer 44, old MIT-Scheme 1) | 45 | yes — see `LICENSES.md` |
| Copyleft (GPL-3.0 18, GPL 8, MPL-2.0 2) | 28 | **no** |
| No licence statement found | 53 | **no** |
| Document licence only (`srfi 5`) | 1 | **no** — see `LICENSES.md` |

**Libraries Patina bundles itself are excluded**, computed from `lib/` rather than listed here.
Patina's copy is canonical, so a vendored duplicate has no role: nothing tests it, and anything
importing it resolves to the bundled version. Thirteen are excluded on that basis, including
`(chibi test)`, `(chibi string)`, `(srfi 1)` and `(srfi 130)`'s dependencies.

Copyleft is excluded to keep this MIT-licensed repository's licence story simple, not because of any
judgement about the code. Packages with no discoverable licence are excluded because absence of a
licence means absence of permission. Both groups are listed in `REVIEW-QUEUE.json`; several are
widely depended on and would be worth recovering if upstream clarifies terms.

## Layout and provenance

Each directory is a **byte-identical extraction of the upstream tarball**, upstream HTML manuals
included. Library paths are preserved (`srfi-1/srfi/1.sld`, `chibi-test/chibi/test.sld`), so a package
directory can be put on the library search path as-is.

Provenance is in `MANIFEST.json` — upstream URL, version, tarball SHA-256, licence and how it was
determined, in-degree, provided libraries, dependencies. It is deliberately **not** recorded as extra
files inside the package trees, so re-syncing upstream stays a clean diff and "did we change
anything?" stays answerable.

**Nothing here has been modified.** If a local change ever becomes necessary, add a patch file applied
at sync time rather than editing in place — otherwise the next upstream sync silently reverts it or
conflicts.

Note: the snow-fort index's `sha-256` field is the author's *signature* digest, not a file checksum.
`tarball_sha256` in the manifest is computed from the downloaded bytes.

## Caveats

- **8 packages ship C stubs** (`.stub`/`.c`) and cannot run without an FFI. They are flagged
  `needs_ffi` in the manifest and should be scored *out-of-scope*, not *failing*.
- **Popularity is thin in the tail.** In-degree reaches 1 by about rank 32, so most of the corpus is
  breadth rather than evidence of importance. The top of the distribution is where the signal is.
- **In-degree measures declared dependencies** in `package.scm`. It captures what package authors
  import, which is the best available proxy, but not what end users actually run.
- Composition skews toward a few prolific sources: slib 51, chibi 47, srfi 35, r6rs 22, pfds 16. A
  high pass rate concentrated in one family is not broad compatibility.

## Files here

| File | What it is |
|---|---|
| `INVENTORY.md` | **Per-package table: version, licence, in-degree, libraries provided.** Generated. |
| `MANIFEST.json` | Machine-readable provenance, including `tarball_sha256` for every package. Generated. |
| `REVIEW-QUEUE.json` | Packages excluded by licence, with versions and in-degrees. Generated. |
| `LICENSES.md` | Licence inventory, the two non-standard licences in full, and the obligations. Hand-written. |
| `README.md` | This file. Hand-written. |

Package **versions** are recorded in `INVENTORY.md` and `MANIFEST.json`. They are upstream's version
strings, captured at vendoring time; the exact bytes are pinned by `tarball_sha256`, which is what to
compare against if you need to know whether a tree still matches what upstream published.

## Regenerating

```sh
python3 compat/tools/build_corpus.py            # rebuild from the pinned cache (compat/.cache/)
python3 compat/tools/build_corpus.py --refresh  # ask upstream what is new
python3 compat/tools/build_corpus.py --check    # rebuild into a temp dir and diff against this one
```

With `--refresh` the tool fetches `http://snow-fort.org/s/repo.scm`, deduplicates to the highest
version per package, ranks by in-degree, resolves each licence, and vendors the acceptable ones.
Without it, it rebuilds from what is already cached and recorded, which is what you want when the
corpus must change for a reason that has nothing to do with upstream — most often because Patina now
bundles a library, so the package providing it has to leave the corpus. A `--refresh` would also
bump versions, and that belongs in its own change.

Either way it regenerates the three generated files above and **leaves `README.md` and
`LICENSES.md` alone** — an earlier version deleted them, so that exclusion is now explicit in the
code.

`--check` exits non-zero on any drift, which makes it usable as a CI guard once the corpus is
expected to be stable.

A licence is never resolved twice for the same tarball: `MANIFEST.json` records it against
`tarball_sha256`, and identical bytes cannot yield a different answer. That is what lets a cache-only
rebuild reproduce the corpus exactly. It did not always — SRFI packages whose snowball carries no
licence text are resolved against their canonical SRFI document, and those pages were once the one
thing the tool fetched every run without caching, so a cache-only rebuild quietly dropped ten
packages. The pages are cached now.

`REVIEW-QUEUE.json` records `tarball_sha256` and `license_evidence` beside the `license` it already
carried, so a licence established for a package we decline to vendor is reusable too — the answer
cost the same to establish either way. Packages Patina bundles are excluded from these reports
entirely: they leave the corpus for a reason that has nothing to do with their licence, and pricing
one only made the reports depend on whether the run could reach the network.
