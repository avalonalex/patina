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

## Use the filesystem adaptation from a separate project

For development, a pinned Patina source checkout is the acquisition source for
the maintained adaptation. No Patina release, Snow registration or package
publication is required. The checkout's `test-lib/` is supplied explicitly;
it is not part of the interpreter's bundled library roots.

From a Patina checkout containing [the example](../examples/chibi-filesystem.scm),
build the interpreter, then create a separate temporary project:

```sh
cargo build --release
patina_bin="$(pwd)/target/release/patina"
filesystem_example="$(pwd)/examples/chibi-filesystem.scm"
filesystem_project="$(mktemp -d)"
cp "$filesystem_example" "$filesystem_project/main.scm"
cd "$filesystem_project"

# Pin the maintained sources to the main revision containing #297.
adaptation_revision=5f467589a8a9eb8e29da62c974810e861090fc09
adaptation_source=.patina/sources/patina
mkdir -p "$adaptation_source"
git -C "$adaptation_source" init --quiet
git -C "$adaptation_source" sparse-checkout set test-lib
git -C "$adaptation_source" fetch --depth 1 \
  https://github.com/avalonalex/patina.git "$adaptation_revision"
git -C "$adaptation_source" checkout --detach "$adaptation_revision"

"$patina_bin" --isolated-libraries -A "$adaptation_source/test-lib" main.scm
"$patina_bin" --tree-walker --isolated-libraries -A "$adaptation_source/test-lib" main.scm
# Both print: (filesystem-ok ffi-unavailable)
```

Only acquisition needs network access; running the example again is offline
and does not require Chibi or Snow. The example creates and removes a new
`filesystem-demo-work/` under the project directory, refusing an existing
directory. It verifies file creation/readback, directory listing and traversal,
working-directory restoration after normal return and an exception, and cleanup.
It also checks that `file-status` reports the documented FFI limitation.

The adaptation's Patina branch depends only on `(scheme base)`, `(scheme file)`
and `(patina internal io)`, supplied by the interpreter. It needs no other Chibi
library. CI exercises the example with only `filesystem.sld` and its provenance
notice copied into a separate root; the real Git acquisition above was verified
manually on 2026-09-12. Both backends fail without `-A`, and a deliberately broken
library in the project/environment cannot override the explicitly supplied copy
when isolation is enabled.

**Supported scope:** directory operations are implemented; raw file descriptors,
stat metadata, links and permissions raise `requires FFI, unavailable in Patina:`.
Directory failures raise exceptions where Chibi may return `#f` or an empty list;
see [the adaptation's provenance and differences](chibi/PROVENANCE.md).
This is a Patina adaptation of an implementation-specific API, not a claim of
complete Chibi filesystem compatibility.

Record the full source revision with your project's dependency instructions and
keep the fetched checkout unchanged. To update, acquire the new revision into a
different source directory, run the example and your project tests with that
root, then change the project's `-A` path. Pin the interpreter revision as well
when reproducing results: this library uses Patina's internal filesystem API.
Keep `chibi/PROVENANCE.md` with any copied adaptation; it contains the source
history and licence notice. The full checkout recipe retains it automatically.

## Why this root exists

`PRD/phase2/R7RS_LARGE_STATUS.md` § "Bundling policy" includes R7RS-large
libraries (including drafts) and SRFIs, and keeps implementation-specific
libraries external. A pure-Scheme `(chibi …)` library stays external even if
many tests need it; a SRFI implementation sourced from Chibi can ship under
its SRFI interface. This boundary was made explicit on 2026-09-12.

The earlier cleanup (#194) reached the same outcome by measuring which
libraries had shipped consumers. Measured importers:

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
| `(chibi string)` | **`lib/srfi/130.sld`** | 35 | importer inlined, then moved, #198 |

Everything but the last row is forced by a **test lane**, which is not the same
as being runtime-forced — that is exactly the reasoning the policy exists to
reject. So those libraries move here and the lanes pass `-A`.

`(chibi string)` was the exception for two steps: `lib/srfi/130.sld` imported
it, so it was genuinely runtime-forced, and SRFI 130 is standard-track and
legitimately bundled. #198 removed that importer by inlining the 28 names
`(srfi 130)` actually used into `lib/srfi/130.chibi-string.scm` — and with the
last `lib/` importer gone, the library had this table's own profile and moved
here like the rest.

#198 as filed said to *delete* it. Measured, that costs nine corpus passes
(127 of 161 down to 118): 35 vendored packages import `(chibi string)` and had
been resolving it from the bundled tree. Moving instead of deleting gets the
same result for `lib/` — which is what the policy is about — at no cost to the
corpus. `lib/chibi/` no longer exists, and `lib/` holds standards and Patina's
own code with no `(chibi …)` namespace at all, which was the point of #194.

## Who supplies it

The CLI-based lanes (`patina-compat`, chibi compliance, GC differential and Larceny) enable
`--isolated-libraries` or its startup setting `PATINA_ISOLATED_LIBRARIES=1`. They load bundled
roots and explicitly supplied directories; user `PATINA_LIBRARY_PATH`/`PATINA_HOME`, `./lib`,
`./.patina/lib` and the script's implicit directory cannot shadow or fill their dependencies.
Explicit `-I`/`-A` roots still work. This does not isolate direct Rust API tests, the content of
explicit roots, or Scheme file I/O; it is a library-search boundary, not a sandbox.

| Lane | How |
|---|---|
| `patina-compat` | a fixed root ahead of each package's own, in `crates/patina-compat/src/run.rs` |
| `crates/patina-tests` | `common::test_lib_root()`, added to every interpreter the shared helpers build — except `eval_program_shipped_only` and `eval_program_shipped_only_err`, for the tests whose subject is what does and does not resolve *without* it |
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
