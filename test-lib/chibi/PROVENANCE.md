# Provenance of `test-lib/chibi/`

Third-party `(chibi …)` libraries that Patina supplies to its test lanes and
does **not** bundle — see `test-lib/README.md` for why this root exists and
who puts it on the search path.

Every file here is **byte-identical** to its pinned upstream snowball package
(verified 2026-08-12, before the move, by diffing against the sha256-checked
tarball below), with one exception: `filesystem.sld` carries a Patina
`cond-expand` branch, described at the end of this file. All are by Alex Shinn
under the BSD 3-Clause licence, reproduced in full under
[Licence](#licence) below.

`filesystem.sld` carries **no in-file copyright notice** — upstream's own file
has none, and we have not removed one — so for that file **this record is the
only notice**, which is why the licence text lives here rather than behind a
link.

| Package | Version | Files here | Tarball sha256 |
|---|---|---|---|
| `(chibi filesystem)` | 0.9.0 | `filesystem.sld` (+ local branch) | `dad608a7fbc00fe8e9929ff6124edad13bedfdc58cc14042be29b33f64c13483` |

Tarball URLs follow the pattern
`http://snow-fort.org/s/gmail.com/alexshinn/chibi/<name>/<version>/chibi-<name>-<version>.tgz`.
These are snow-fort snowball releases, older than chibi-scheme's git head.

## Licence

All files in this tree are covered by the following, from chibi-scheme's
`COPYING`. It is reproduced here, not linked, because BSD 3-Clause condition 1
requires a source redistribution to retain the conditions and disclaimer — and
because a clone taken offline, or a link that rots, must still carry them.

```
Copyright (c) 2009-2021 Alex Shinn
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.
3. The name of the author may not be used to endorse or promote products
   derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

Patina's own MIT licence covers Patina; it does not relicense anything in this
tree, and vendoring changes no terms. Condition 3 is why nothing in this
repository presents Alex Shinn or chibi-scheme as endorsing Patina — the
README's references describe what we test against, which is attribution, not
endorsement.

## `(chibi filesystem)` — the one local branch (2026-08-14)

Upstream's `cond-expand` has branches for `chibi`, `chicken` and `sagittarius`
and **no `else`**, so on any other implementation the library loads defining
nothing and every importer fails on its first export. That is why five corpus
packages sat in the load-error bucket: not a missing primitive anywhere, a
missing branch. `filesystem.sld` therefore carries a `(patina …)` branch, marked
`;; PATINA LOCAL EDIT:` at its head. The rest of the file is upstream.

The branch is deliberately half an implementation, along a line the library's
own shape draws. The portable directory API — `directory-files`,
`create-directory`, `current-directory`, `directory-fold-tree`,
`with-directory` and friends — is implemented on primitives that route through
Patina's VFS `FileSystem` trait, so it behaves the same against an in-memory
filesystem. The POSIX layer — file descriptors, `stat` fields, symlinks, pipes,
permissions — is stubbed with upstream's own `define-unimplemented` idiom,
lifted from its sagittarius branch, which stubs the same fd procedures for the
same reason. Those need the FFI layer (`PRD/FFI_DESIGN.md`), not more Scheme,
and raise a marker string that `crates/patina-compat` classifies as
out-of-scope rather than as our defect.

The directory primitives the branch is built on **raise where chibi's return
`#f`** — `create-directory`, `delete-directory` and `change-directory` — and
`directory-files` raises where chibi returns `'()` for an unreadable
directory. Deliberate, and recorded here because it is what chibi-ecosystem
code notices: anything that *branches* on `#f`, including upstream's own
`(or (file-directory? dir) … (create-directory dir))` idiom in
`create-directory*`, gets an exception instead. Everything is catchable and
panic-free. The reasoning is at the primitives themselves, in
`crates/patina-primitives/src/primitives/io/directory.rs`.

`delete-file-hierarchy` is the branch's one deviation from upstream *behaviour*
(audit 2026-08-17, B1/B2). Upstream refuses `""` and `"/"` before touching
anything and honours an `ignore-errors?` argument; the branch had neither, so a
computed-empty path began a depth-first walk of the root. Both are restored, but
the tolerance is spelled with `guard` rather than upstream's return-value test,
because Patina's `delete-file`/`delete-directory` raise where chibi's return
`#f` (the same deviation `F1` records for the directory primitives generally).

`filesystem.scm` and `filesystem.stub` are **not** vendored. Both are reachable
only from the `chibi` branch, which also needs `include-shared` and the C shim,
so shipping them would put unreachable C-dependent Scheme in the library tree.

## The rule

The one in `lib/chibi/PROVENANCE.md` § The rule (audit 2026-08-10, group E),
unchanged — not restated here, so there is one wording to edit rather than
two. This file is the provenance home it names for this tree.

Supplying a library with `-A` rather than shipping it exempts it from nothing:
drift is as worth catching here, and BSD 3-Clause condition 1 applies whatever
directory the file sits in. `bundled_provenance.rs` pins this tree for that
reason.
