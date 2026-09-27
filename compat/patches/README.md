# Package patches

Patches applied to a **copy** of a vendored package before it runs. One file
per package, named `<slug>.patch`, in `-p1` form relative to the package root.

`compat/vendor/` is never written to. Its README calls those trees "unmodified
upstream copies kept for testing", and a harness that edited them would be
measuring its own edits. So the runner copies the package into a scratch
directory, applies the patch there, and runs that — the vendored tree stays
byte-identical to what upstream shipped, and this directory is the reviewable
record of every difference.

## When a patch is justified

There are two authorized uses of package overlays:

- **Portability patches** make the rewrite the package's author would make to
  run on a conforming R7RS implementation, preserving the behavior measured.
- **Correctness patches found while expanding execution coverage for #429**
  repair demonstrated upstream defects, with an issue, reference measurements
  and regression assertions as described below. The owner authorized this
  workflow on 2026-09-27; qualifying patches need no further per-patch approval.

Neither use permits hiding a Patina defect or weakening a test to make a
package pass. Other incompatibilities belong in `compat/EXCLUSIONS.scm` with
a reason, or should simply fail.

Whose incompatibility it is gets *measured*, not assumed. Where the package
relies on something chibi allows, ask Gauche: if Gauche rejects it as Patina
does, it is the package's non-portability and a patch may spell it portably.
If Gauche accepts it, it may be **a difference in Patina, and that is never
patched** — see the end of this section.

### Portability patches

What has been admitted so far, each with the patch that needed it. The list is
here so a new patch is argued against it rather than beside it; a shape that is
not on it needs its own argument, added here.

**In a package's library**

- **A name that has moved, or a chibi extension that has an R7RS spelling.**
  SRFI 114 was withdrawn for SRFI 128 (`in-progress-hash-tables`). Two-argument
  `substring` is chibi's extension — Gauche raises, as Patina does — and R7RS
  spells the same operation `(string-copy s start)` (`chibi-show`,
  `chibi-app`). A rename, with the same semantics.
- **A slip the language rejects, in code chibi never compiles or never
  reaches.** A `syntax-rules` rule with four elements (`chibi-bytevector`); a
  `syntax-rules` with no literals list, in the `cond-expand` branch only other
  implementations take (`chibi-monad-environment`); the one call that reaches
  for SRFI 1's `every` where the file's own `seq-every` is meant
  (`chibi-math-stats`). Corrected to what the surrounding code shows was
  intended. It qualifies because the corrected code is unreachable on chibi or
  is the author's own idiom — not because the correction is small.
- **An import the library uses and never declares** (`comparators`). Adding it
  changes nothing on an implementation that did not enforce the import set.
- **A `cond-expand` that is unportable by omission**: a branch for each
  implementation its author had and no `else`. The `else` holds the stand-in
  the author already wrote for another implementation, moved over
  (`chibi-tar` copies the package's own CHICKEN definitions), or is empty, so
  that what cannot run is skipped rather than stopping the file
  (`chibi-show`'s test, for chibi's own `complex` feature). It stops
  qualifying the moment the new branch has to invent behaviour the suite then
  measures.
- **A library that chooses differently from its only client.**
  `chibi-char-set-boundary` picked its char-set library by *availability*
  while `(chibi regexp)`, which exists to consume its sets, picks by the
  `chibi` *feature*; on chibi the two are one library, and elsewhere they
  agreed only if nothing else was installed. The patch makes the dependency ask
  its client's question. This is the narrowest of the shapes: it rewrites a
  `cond-expand` test that is valid as written, and is admitted only because the
  two libraries are one author's, the disagreement is a load failure rather
  than a preference, and nothing changes on chibi.
- **Dead code that other implementations reject is removed, not repaired.**
  `chibi-app` has a `case` clause after its `else`, which chibi never reaches;
  the patch deletes the clause rather than moving `else` below it, because
  moving it would change what the program does. This portability rewrite
  preserves the behavior being measured.

**In a package's test program** — admitted because the alternative is that
assertions which could run never do (#428). Each has a limit, and the limit is
the point:

- **A test header written for another implementation's module system.**
  `comparators` opens with CHICKEN's `(use test) (use srfi-128)` and a `load`;
  the assertions under it are `test-group` / `test` code that `(chibi test)`
  runs unchanged. The patch may replace how the program *finds* its framework
  and its subject. It may not touch an assertion or an expected value, and if
  the body needed rewriting too it would be a port, which belongs upstream.
- **A test group that opens an input the package does not ship.**
  `chibi-regexp`'s last group reads `tests/re-tests.txt`, which is in chibi's
  source tree and not in the snowball. The patch may guard the group on that
  file's presence, so it runs wherever its data exists and is empty elsewhere.
  It may not guard a group because it *fails*: a guard is for an input that is
  missing, never for an answer that is wrong.

**What a patch is never for is a difference in Patina.** `chibi-tar` had one —
text written to a binary port, which chibi and Gauche both allow and Patina
refused — and that was changed in Patina (#404); six rewritten call sites would
have made the package pass and left the inconsistency for the next package to
find. The same happened again with #431: the line `chibi-char-set-boundary`
patches also sat in Patina's *own* bundled `(srfi 115)`, where it was fixed in
the library rather than excluded around. A patched package that then shows a
Patina defect is the mechanism working.

**A portability patch must add regression signal.** `chibi-assert` reaches 3
of 4 with its test's chibi names rewritten, and the fourth cannot pass off
chibi — Gauche fails it identically. Its status would read `wrong-result`
whether three assertions passed or none, so the patch would buy no regression
signal; the finding goes in its exclusion note instead.

### Correctness patches during #429

The owner authorizes upstream correctness repairs discovered while working
on [#429](https://github.com/avalonalex/patina/issues/429), including defects
in dependencies exercised by a new driver. Proceed without a separate owner
decision for each repair when the following evidence and checks are in place:

1. Search the issues and Track L archive, then file or update an issue before
   fixing the defect. Record a minimal reproduction, the expected behavior
   and its basis, and why the defect belongs to the upstream package.
2. Compare the pinned implementation before and after the repair on both
   Patina backends, Chibi and Gauche. Record any library renaming, branch
   selection or declaration adaptation needed to exercise the same code.
   Document unavailable comparisons or reference differences explicitly;
   they are not passing comparisons or grounds for changing the expected
   answer. The evidence must distinguish an upstream defect from a Patina
   incompatibility.
3. Add assertions for the repaired behavior to a maintained CI smoke driver.
   Keep existing assertions and their expected results intact. Use the
   smallest repair that implements the documented behavior.
4. Keep the pristine vendor extraction unchanged. Put the correction in a
   named overlay here, link its issue in the patch header and this record,
   run the patch-application guard and both backend smoke gates, and refresh
   the full corpus measurement when coverage or results change.

This authorization is scoped to #429 execution-coverage work. It does not
authorize masking Patina differences or changing bundling policy. Outside
that scope, correctness changes still need an owner decision; portability
patches continue to follow the rules above.

Corrections recorded so far (the first four were approved individually before
the broader #429 authorization):

- [#504](https://github.com/avalonalex/patina/issues/504), `srfi-95.patch`:
  make `sorted?` compare the elements of a two-element array.
- [#505](https://github.com/avalonalex/patina/issues/505), `srfi-180.patch`:
  validate JSON number syntax before calling Scheme's `string->number`.
- [#508](https://github.com/avalonalex/patina/issues/508),
  `pfds-hash-array-mapped-trie.patch`: preserve colliding keys and repair
  nested replacement, deletion, mapping and folding. Hash slices advance once,
  collision nodes retain full hashes, and bitmap positions are translated to
  compact child-vector indices. The smoke driver also checks 480 mixed
  operations against an alist model, preserving earlier map versions.
- [#511](https://github.com/avalonalex/patina/issues/511), `srfi-38.patch`:
  let the host reader decode string escapes emitted by the host writer in
  the portable `38.scm` implementation. Copy each decoded string to preserve
  its mutability, including through shared labels. The smoke driver checks
  control characters, hexadecimal escapes and mutation through aliases.
- [#513](https://github.com/avalonalex/patina/issues/513), `srfi-42.patch`:
  remove the extra continuation argument passed to the inner `:vector`
  generator when multiple vectors are enumerated with an index variable.
  The drivers cover indexed concatenation, empty vectors and use through
  SRFI 78's `check-ec`.
- [#515](https://github.com/avalonalex/patina/issues/515), `srfi-19.patch`:
  preserve negative duration fractions, calculate destructive differences
  before mutation, correct week numbering and Julian timezone arithmetic,
  convert monotonic dates through TAI, sample UTC seconds and fractions
  together, and preserve the requested thread-clock type. The driver uses
  fixed dates, explicit offsets and a temporarily replaced clock binding.
  Process/thread clocks still use the upstream wall-clock surrogate; this
  patch does not provide CPU-time measurement.
- [#516](https://github.com/avalonalex/patina/issues/516), `srfi-166.patch`:
  admit exact-width word lines, preserve explicit newlines and stream
  character chunks without adding a final newline. Keep digit grouping and
  word tokenization within cursor bounds. The smoke driver temporarily
  enforces checked cursor movement so CI detects the invalid movement that
  Gauche rejects but integer-cursor implementations tolerate.
- [#517](https://github.com/avalonalex/patina/issues/517), `slib-alist.patch`:
  remove the `=` shortcut through `assv`, which uses `eqv?`. The existing
  predicate scan correctly handles exact/inexact equivalents for lookup,
  inquiring and replacement.

These patches deliberately correct upstream behavior. All defects were
reproduced with the pinned libraries on Patina's two backends, Chibi and
Gauche; all corrections are checked on those same implementations, and their
regressions are asserted by the CI smoke drivers. SRFI 38's comparisons select
the portable `38.scm` branch on all four; the separate `38.chibi.scm` branch
is unchanged and is not the subject of those comparisons. SRFI 42's reference
staging removes the inert `#f` library declaration rejected by Gauche; its
implementation body is identical to the patched corpus. SRFI 166's reference
staging selects portable optional-argument macros and supplies an isolated
SRFI 165 dependency. That staged dependency collects environment defaults in
a vector instead of multiple values, avoiding Gauche's multiple-value limit;
the SRFI 166 implementation bodies remain identical to the patched corpus.
They do not weaken an assertion or hide a Patina difference. The corpus now measures these packages
with the documented corrections, not the pristine upstream behavior.

Each patch begins with a `#` comment block — `patch(1)` skips it as leading
garbage — stating what is rewritten, why it is justified, its issue and
regression coverage, and what was measured before and after. That block is the
argument; the diff is just its consequence.

## How it runs

`stage_patched_copy` in `crates/patina-compat/src/run.rs`:

1. Clears and re-copies `compat/vendor/<slug>` into scratch.
2. Applies `compat/patches/<slug>.patch` with
   `--fuzz=0 --forward --batch`.
3. Returns that directory as a search root, ahead of the pristine one. A
   package's *test script* is taken from the patched copy too, since that is
   usually where the patched import lives.

The three `patch(1)` flags each prevent a specific silent wrong answer:

- `--fuzz=0` — context must match exactly, so a stale patch cannot slide a
  hunk into the wrong place and report success.
- `--forward --batch` — never reverse, never prompt. Staging can reach the
  same package twice (once as a dependency of another patched package, once
  for its own run); without these, `patch(1)` detects the already-applied
  patch, answers its own prompt with `-R` because stdin is closed, and
  silently *undoes* the patch while exiting 0. That produced a different score
  under `--jobs 8` than under `--jobs 1` before it was fixed.

A patch that fails to apply warns and the package runs **unpatched**, so
whatever the patch was covering comes back and is scored. The alternative —
keeping the old result — would let a stale patch hide a regression.

## The guard

`every_patch_applies_to_its_package` (same file) applies every patch here to a
fresh copy of its package and fails the build if one does not, or if one
applies but changes nothing. A patch goes stale the moment its package is
re-vendored, and this is where that is cheap to notice — rather than during a
corpus run, where it looks like the package's own regression.
