# `test-lib/` — third-party libraries the test lanes supply

**Nothing here is bundled with Patina.** These are third-party Scheme
libraries that Patina's *test lanes* need, put on the library search path with
`-A` by whatever is running. `patina script.scm` does not see them, and that
is the point:

```console
$ cat probe.scm
(import (scheme base) (chibi filesystem))
(display (procedure? directory-files))

$ ./target/release/patina probe.scm
Error: Library (chibi filesystem) not found

$ ./target/release/patina -A test-lib probe.scm
#t
```

`crates/patina-repl/tests/cli_options.rs` pins both halves of that, so the
boundary is a checked claim rather than a stated one.

## Why this root exists

`PRD/phase2/R7RS_LARGE_STATUS.md` § "Explicitly out of scope" rules out
bundling *pure-Scheme leaf libraries that are neither standard-track nor
runtime-forced*, on the grounds that they work fine from a `-A` directory.
Most of `lib/chibi/` was a standing exception to that rule. Measured importers
(#194):

| Library | `lib/` importers | `compat/` importers |
|---|---|---|
| `(chibi test)` | none | 79 |
| `(chibi filesystem)` | none | 16 |
| `(chibi diff)` | `lib/chibi/test.sld` only | 0 |
| `(chibi term ansi)` | `diff.sld`, `test.sld` only | 0 |
| `(chibi optional)` | `lib/chibi/diff.sld` only | 13 |
| `(chibi string)` | **`lib/srfi/130.sld`** | 35 |

Everything but the last row is forced by a **test lane**, which is not the same
as being runtime-forced — that is exactly the reasoning the policy exists to
reject. So those libraries move here and the lanes pass `-A`.

`(chibi string)` is the exception and stays in `lib/`: `lib/srfi/130.sld`
imports it, so it is genuinely runtime-forced, and SRFI 130 is standard-track
and legitimately bundled. #198 removes that last importer by inlining what
`(srfi 130)` uses, at which point `lib/chibi/` goes away entirely — but until
then the row is real and the rule above does not reach it.

## Who supplies it

| Lane | How |
|---|---|
| `patina-compat` | a fixed root ahead of each package's own, in `crates/patina-compat/src/run.rs` |
| `crates/patina-tests` | `common::test_lib_root()`, added to every interpreter the shared helpers build |

Each is the same statement in its own dialect: *this lane runs third-party
code, so it supplies third-party libraries.*

## Why not `compat/vendor/`

`compat/vendor/` is a **byte-identical** extraction of upstream tarballs, and
its README commits to keeping it that way — "if a local change ever becomes
necessary, add a patch file applied at sync time rather than editing in
place". `chibi/filesystem.sld` carries a recorded Patina `cond-expand` branch
without which the library defines nothing under Patina, so it cannot live
there. This root is for the copies we maintain; `compat/vendor/` is for the
copies we do not.

The corpus builder knows the difference: `bundled_libraries()` in
`compat/tools/build_corpus.py` reads **both** `lib/` and here, so a library
supplied from this root still keeps its upstream package out of the corpus.
Vendoring a second copy of `(chibi filesystem)` would achieve nothing —
upstream's `cond-expand` has no `else` branch, which is why we carry one.

## Provenance

Per-tree, in the tree: `chibi/PROVENANCE.md`. Files claimed byte-identical to
an upstream release are pinned by
`crates/patina-tests/tests/bundled_provenance.rs`, exactly as the trees under
`lib/` are — moving a file off the shipped path does not make upstream drift
less worth catching, and a BSD 3-Clause notice obligation does not care which
directory the file is in.
