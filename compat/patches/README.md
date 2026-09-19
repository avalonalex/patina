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

A patch is **not** a way to make a failure go away. The test is the same for
every one of them: **the rewrite is the one the package's author would make to
run on a conforming R7RS implementation, and it changes no behaviour being
measured.** If a patch would paper over a real incompatibility, the package
belongs in `compat/EXCLUSIONS.scm` with a reason, or should simply fail.

Whose incompatibility it is gets *measured*, not assumed. Where the package
relies on something chibi allows, ask Gauche: if Gauche rejects it as Patina
does, it is the package's non-portability and a patch may spell it portably.
If Gauche accepts it, it may be **a difference in Patina, and that is never
patched** — see the end of this section.

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
  moving it would change what the program does. A patch measures upstream's
  code, not an improvement on it.

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

**A package that would still fail is not patched.** `chibi-assert` reaches 3
of 4 with its test's chibi names rewritten, and the fourth cannot pass off
chibi — Gauche fails it identically. Its status would read `wrong-result`
whether three assertions passed or none, so the patch would buy no regression
signal; the finding goes in its exclusion note instead.

Each patch begins with a `#` comment block — `patch(1)` skips it as leading
garbage — stating what is rewritten, why the old name is wrong, why the new
one is faithful, and what was measured afterwards. That block is the argument;
the diff is just its consequence.

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
