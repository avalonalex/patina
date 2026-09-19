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

A patch is **not** a way to make a failure go away. It is for a package whose
source names something that has since moved, where the rewrite is the one its
author would make today and changes no behaviour being measured. If a patch
would paper over a real incompatibility, the package belongs in
`compat/EXCLUSIONS.scm` with a reason, or should simply fail.

One other shape qualifies, on the same test: a `cond-expand` with a branch
for each implementation its author had and **no `else`**, so that the package
is unportable by omission rather than by design. The patch adds the `else`,
and what goes in it is the stand-in the author already wrote for another
implementation, moved over — `chibi-tar.patch` copies the package's own
CHICKEN definitions. It stops qualifying the moment the new branch has to
invent behaviour the suite then measures.

What a patch is *not* for is a difference in Patina. `chibi-tar` had one of
those too — text written to a binary port, which chibi and Gauche allow and
Patina refused — and that was changed in Patina (#404). Six rewritten call
sites would have made the package pass and left the inconsistency where it
was, with the next package to find it.

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
