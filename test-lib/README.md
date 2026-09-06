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

Importers are **as measured before any of it moved**, which is what the
policy question turns on — the paths below are the pre-move ones and several
no longer exist:

| Library | importers then in `lib/` | `compat/` importers | Outcome |
|---|---|---|---|
| `(chibi test)` | none | 79 | moved, #197 |
| `(chibi filesystem)` | none | 16 | moved, #196 |
| `(chibi diff)` | `lib/chibi/test.sld` only | 0 | moved, #197 |
| `(chibi term ansi)` | `lib/chibi/{diff,test}.sld` only | 0 | moved, #197 |
| `(chibi optional)` | `lib/chibi/diff.sld` only | 13 | moved, #197 |
| `(chibi string)` | **`lib/srfi/130.sld`** | 35 | stays in `lib/` |

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
| `crates/patina-tests` | `common::test_lib_root()`, added to every interpreter the shared helpers build — except `eval_program_shipped_only`, for the tests whose subject is that something resolves *without* it |
| `scripts/run_chibi_tests.sh` (+ the tree-walker wrapper) | `-A test-lib`, plus a `[ -d ]` check on the root. The suite reports *through* `(chibi test)`, so a bad path yields no tally and the run dies at "Could not parse a total from the suite output" |
| `scripts/run_gc_differential.sh` | `-A test-lib`, plus `assert_suite_ran` on every lane, pinning the count at 1226 — the lane compares runs for *equality*, so identical failures would otherwise pass |

Note what the last two rows are guarding, and where each stops. A missing root
does not make patina exit non-zero: it reports the unresolved library, keeps
evaluating, and exits 0 with deterministic output. A lane that only diffs two
such runs finds them byte-identical and reports success. So each lane checks
that the suite reached its expected total — not that the directory exists (a
present-but-empty `test-lib/` passes that), and not merely that two runs
agreed.

The `[ -d ]` check is a courtesy that names the likely cause early; the tally
is the actual guard.

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
