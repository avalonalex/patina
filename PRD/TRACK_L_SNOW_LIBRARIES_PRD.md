# Track L — Third-Party Library Compatibility PRD

**Created:** 2026-06-20
**Updated:** 2026-09-01 — bookkeeping sweep. §6 Open reconciled against the triage doc by running
the recorded repros: eight fixed entries moved to `PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md` (five from
the 2026-08-24/25 Larceny sweep that were never re-marked here, two already marked in place, and
the quasiquoted-vector hygiene entry, which families 33–35 had fixed under it), and the
relinking-by-name entry re-measured to **VM-only** — the tree-walker now answers all three of its
repros as chibi and Gauche do, courtesy of the hygiene arc. The play-by-play this line used to
carry lives in this file's git history; the shape of it: corpus vendored 2026-08-08 → harness live
2026-08-13 → Larceny lanes and their defect queue 2026-08-24 → all 33 suites loading 2026-08-26 →
the hygiene queue closed 2026-08-31/09-01 (matrix 28 of 28 on both backends, families 33–39 fixed,
family 40 quarantined).
**Status:** In execution — L0, L0.5, L0.75, L4 done; L3 harness live (**127 of 161** vendored
packages pass, of which **127 of 136 are in scope** — the other 25 are excluded by
`compat/EXCLUSIONS.scm` with a recorded reason apiece); L5's reader and libraries landed, its suite
deferred; L5.3's lanes: VM **22 of 33** suites clean (8447/8475), tree-walker **21 of 33**
(8302/8330, stream budgeted per family 26), R6RS **12 of 16** (4017/4025). **L1 is essentially
spent** — the bundling queue is empty; what remains is SRFI 115 (only if the corpus justifies it)
and the low-demand Tangerine trio 146/159/160. **L2 is the open bundling front** (`(chibi match)`,
`(chibi io)`, `(chibi pathname)`, `(chibi show)`/SRFI 166 — the last also worth two corpus
rows; `(chibi string)`, `(chibi optional)` and `(chibi filesystem)`'s portable half are done). The open defect queue is §6 Open plus the non-hygiene triage families;
the exception-extent cluster (families 22+28) closed 2026-09-01, together with
the tree-walker `guard` ordering; two quarantined defects turned out to be its
prerequisites and landed first as #149 and #150.
**Scope decision:** **self-contained.** Patina measures and fixes its own compatibility with the popular third-party R7RS ecosystem using a harness that lives in this repo. No dependency on `snow-chibi`, a chibi installation, or any external package manager at build, test, or CI time. The `patina pkg` end-user fetcher and FFI remain deferred.
**Umbrella:** `PRD/SNOW_AND_PERF_ROADMAP.md` (cross-track sequencing)

---

## 1. Context & problem

The goal is to **run the popular third-party R7RS library ecosystem** — snow-fort.org packages, the chibi library tree, and standalone R7RS libraries — and to know, as a number that regenerates on every run, how much of it works.

The encouraging finding from evaluation: Patina's library *machinery* is already R7RS-complete and ready to consume portable source — `define-library` with all clauses (`export`, `import` with all 5 import-set forms, `begin`, `include`, `include-ci`, `include-library-declarations`, `cond-expand`), the `features` procedure with platform/arch identifiers, and relative-path `include` with circular detection. The blockers are **content and edge cases**, not the import system:

### Verified current-state evidence

| Observation | Evidence |
|---|---|
| Only **9 SRFIs** bundled. | `lib/srfi/` = 1, 8, 69, 111, 113, 128, 132, 133, 158. |
| Only **one `(chibi …)`** library. | `lib/chibi/` = `test.sld` only. |
| Unknown `define-library` clauses are a **fatal error** (aborts the whole load). | `"Unknown library declaration: {}"` — `crates/patina-frontend/src/library_parser.rs:212`. Known clauses handled at `:177` (begin), `:198` (cond-expand), `:206` (include-library-declarations). |
| **No inline `define-library`** code path (only file-based `.sld` discovery). | Zero matches for `define-library`/`define_library` in `crates/patina-frontend/src/desugarer/mod.rs`. |
| The dependency search path (`./.patina/lib/`) the package-manager design assumes is **not wired**. | `LibraryRegistry { search_paths: Vec<PathBuf> }` — `crates/patina-runtime/src/library_registry.rs:87`; defaults set in `with_default_paths`. |
| A Snow-shaped package **actually loads and runs** — nested `.sld` + relative `include`, imported by a separate program. | Verified 2026-08-08 against `target/release/patina`: resolves from `./lib/` and from `$PATINA_HOME/lib/`; fails only when the package sits *beside* the program (that dir is not searched). |
| **No library-path CLI flag**, so no external library directory can be used at all. | Arg parsing at `crates/patina-repl/src/main.rs:19-36` (`--help`, `--tree-walker`, `--dump`, `--trace`, `--vm` only). Every other Scheme has `-A`/`-I`/`-L`. |
| **Shebang lines are not skipped**, so a distributed *program* fails immediately. | `#!/usr/bin/env patina` → `Error: unbound variable: patina` (verified 2026-08-08). |
| A large corpus **with its own test suites** is available without any package manager. | The chibi library tree is **359 `.sld` files, 50 of them `*-test.sld` suites**, with only 34 C-backed files — i.e. overwhelmingly pure Scheme, and the same ecosystem Snow packages depend on. |

The recurring porting frictions (all *resolved* for the existing 9 SRFIs) are catalogued in `PRD/phase2/archive/SRFI_PORTING_ISSUES.md` and will recur for each new reference implementation imported.

## 2. Goals
- A **self-contained compatibility harness in this repo**: one command fetches, runs, and scores the corpus, with no external Scheme toolchain involved.
- A **measured, repeatable coverage number** ("N of M libraries run") whose failure buckets *are* the prioritised work queue for everything else in this track.
- Bundle the dependency libraries the corpus actually needs — the high-frequency SRFIs and `(chibi …)` libraries — as directed by that queue rather than by guesswork.
- Make library loading **degrade gracefully** instead of hard-failing on benign edge cases.
- Make external library directories usable at all (`-A`/`-I`), which is table stakes independent of any distribution mechanism.

## 3. Non-goals (deferred)
- `patina pkg` end-user fetcher — `PRD/future/PACKAGE_MANAGER_DESIGN.md`. L0 wires the forward-compatible `./.patina/lib/` hook. The L3 harness has its *own* internal fetcher; that is test infrastructure, not a shipped package manager, and the two should not be conflated.
- FFI Layer 1/2 for C-shim packages — `PRD/FFI_DESIGN.md`. Pure-Scheme packages (the majority a learner wants) are unaffected, but this bounds the achievable score — see §6.
- **Any dependency on `snow-chibi`, a chibi installation, or a chibi fork.** Investigated 2026-08-08 and rejected as an external moving part: it would put an interactive tool and another implementation's release cadence on our critical path. Snowballs are tarballs and `package.scm` is s-expressions, so the harness reads the *format* directly. Upstreaming a Patina entry into chibi is an optional publicity milestone for after the score is respectable, never a gate.

---

## 4. Work items

### L0 — Close the loading-gap edge cases  *(done — 2026-08-13)*

✅ **Done, all five items.** Deviations from the spec worth recording:
- Unknown clauses warn-and-skip by default; `PATINA_STRICT_LIBRARY_SYNTAX=1` restores the error.
- The inline `define-library` path is **not** in the desugarer — it stays a datum. Both backends
  intercept it at their eval entry (beside the existing `Import` special case) and route it through
  `SchemeLibraryLoader::parse_inline_form`, so the `.sld` and inline paths share one parser and one
  body-resolution. Includes resolve against the current directory; re-evaluating the form replaces
  the library (`LibraryRegistry::register_or_replace`), so REPL redefinition works.
- The shebang is handled in the *lexer* (`#!` followed by `/` or space comments out the line), so it
  works for any entry point, not just script files; `#!fold-case` is unaffected.
- Acceptance tests are split: inline/lenient-clause cases in
  `crates/patina-tests/tests/sld_file_loading.rs` (both backends), while `./.patina/lib/`,
  beside-the-script resolution, and shebang scripts are end-to-end binary-spawn tests in
  `crates/patina-repl/tests/script_running.rs` — those behaviours live in the CLI layer and
  cwd-relative defaults, unreachable from the library API without process-global state.

#### Original spec
1. **Graceful unknown clauses.** `library_parser.rs:212` hard-errors on any unrecognized `define-library` clause; portable `.sld` files occasionally carry vendor-specific ones. Change to warn-and-skip (behind a strictness flag, default lenient) or a small known-clause allowlist, so one unknown clause doesn't abort the whole load.
2. **Inline `(define-library …)` code path.** Add desugarer handling so a `define-library` written directly in a script/REPL source (not a discovered `.sld`) is parsed via the existing `library_parser` and registered. Some single-file libs and `package.scm` forms embed it inline.
3. **`./.patina/lib/` on the search path.** Add the project-local dependency directory to `with_default_paths` in `library_registry.rs` (ahead of the workspace/exe paths) so dependencies can be dropped under a project dir — the forward hook for the eventual fetcher.
4. **Program-relative resolution.** Add the directory containing the script being run to the search path, so a checked-out package runs without an install step (today it does not — see the evidence table).
5. **Skip a leading `#!` line** in script files, so installed Snow programs execute.
- **Acceptance:** new cases in `crates/patina-tests/tests/sld_file_loading.rs` — an inline `define-library`, an `.sld` with an unknown clause (loads with a warning), a library resolved from `./.patina/lib/`, and one resolved beside the running script; plus a shebang-prefixed script that runs.

### L0.5 — Library-path CLI surface  *(done — 2026-08-13)*

✅ **Done, all four items.** Deviations from the spec worth recording:
- `-p` is repeatable (expressions evaluate in order, each non-unspecified result printed in write
  form, then exit 0) and refuses to combine with a script file rather than picking an order.
- `-I`/`-A` apply to script *and* REPL modes on both backends; the search order is
  `-I` flags → `PATINA_LIBRARY_PATH` entries → `./lib` → `./.patina/lib` → `$PATINA_HOME/lib` →
  workspace/exe paths → `-A` flags → the script's own directory.
- Acceptance tests are binary-spawn end-to-end tests in `crates/patina-repl/tests/cli_options.rs`
  (following L0's precedent — this is CLI-layer behaviour), not `crates/patina-tests/`; the
  registry-level prepend/env-path logic has unit tests in `library_registry.rs`.

#### Original spec
External library directories are currently unusable. This is table stakes for any Scheme, independent of distribution mechanism, and every later item depends on it.

- `-A <dir>` (append to search path) and `-I <dir>` (prepend) — the conventional spelling across Scheme implementations, and what the harness uses to point Patina at a fetched package.
- `-p <expr>` — evaluate and print; the harness's capability probe (`patina -p "(features)"`) and a generally useful one-liner facility.
- `--version` — trivial, and needed for reproducible result provenance in the report.
- `PATINA_LIBRARY_PATH` (colon-separated), matching the convention of every other implementation.
- **Acceptance:** `patina -A <dir> prog.scm` resolves a library from `<dir>`; `patina -p "(features)"` prints the feature list; unit coverage in `crates/patina-tests/`.

### L0.75 — Ecosystem survey  *(done — satisfied by the corpus build, 2026-08-09)*

✅ **Done.** `compat/tools/build_corpus.py` fetches the snow-fort index, ranks every package by
dependency in-degree, and emits the committed, regenerable artifacts (`MANIFEST.json` with
per-package `popularity_indegree`/`provides`/`depends`, plus `INVENTORY.md`). L1 was re-ordered
from that data on 2026-08-08. With L3's harness now live, the *measured failure histograms*
supersede in-degree as the ordering signal — see L3.

#### Original spec
Before porting anything, establish what "popular" actually means, as data. Fetch the snow-fort repository index (an s-expression document) and inventory the chibi library tree, then count **import frequency across all packages**. Output a ranked table of imported library names with counts, checked into `compat/`. This single artefact orders L1 and L2 and sets the corpus for L3 — the candidate lists in this PRD are a *hypothesis* to be replaced by it.
- **Acceptance:** a committed, regenerable frequency table; L1/L2 below re-ordered to match it.

### L1 — Bundle the SRFIs that policy and data agree on
**Scope is set by the bundling policy in `PRD/phase2/R7RS_LARGE_STATUS.md`, and ordering by measured
in-degree over `compat/vendor/`.** The earlier hypothesis in this PRD — leading with SRFI 26, 13 and
41 — was wrong and has been dropped: those are pure-Scheme leaves with in-degree ≤ 1 that the corpus
and `-A` already cover, and none is standard-track.

The policy in one line: bundle what R7RS-large names, plus what cannot exist without the runtime,
plus the legacy aliases the ecosystem actually imports. Everything else stays out.

**Everything the policy and the measurements agreed on has shipped, and the queue is flat.**
Compressed 2026-09-01 from the item-by-item log this section used to carry (this file's git
history has it in full); what remains uncompressed is the open remainder and the lessons.

Shipped, in `lib/srfi/` and `lib/scheme/`: SRFI 151 bitwise with `(srfi 60)`/`(srfi 33)` shims
(Rust primitives; the largest measured gap at in-degree 31/19); the Red-edition `(scheme …)`
aliases (`box`, `comparator`, `list`, `set`, `sort`, `vector`, `generator`, `hash-table`);
SRFI 125 hash tables; SRFI 27 random (RNG primitive, in-degree 9); SRFI 143 fixnums; SRFI 130
cursor strings with SRFI 14 char-sets, `(chibi string)` (in-degree 16) and `(chibi optional)`;
the re-export shims `(srfi 23)`, `(srfi 98)`, `(srfi 142)` and `(scheme small)`; and the
Red/Tangerine set the Larceny suites
demanded — 41 `stream`, 101 `rlist`, 116 `ilist`, 117 `list-queue`, 124 `ephemeron` (Rust — an
ephemeron's defining property is what the collector does), 127 `lseq`, 134 `ideque`, 135 `text`,
144 `flonum`. Provenance for every import is in `lib/srfi/PROVENANCE.md` /
`lib/chibi/PROVENANCE.md`.

**Still open in L1:** SRFI 115 regex — large, and only if the corpus justifies it — the
low-demand Tangerine trio 146/159/160, and the near-free re-export shims `(srfi 6)`, `(srfi 9)`,
`(srfi 11)`, `(srfi 39)` (R7RS base already provides the functionality; nothing in the corpus has
asked for them yet, which is why they have kept not happening). Nothing else: no missing library is worth more than two
packages, and the one at three, `(chibi)`, is chibi's implementation core and permanently out of
reach.

Three lessons this queue recorded, kept because each corrected a filed premise:
- *SRFI 125 needed no runtime support* — `equal-hash` was a Rust primitive all along and the
  shipped `(srfi 69)` was already a real table; 125 is a 178-line layer over 69+128. A
  Rust-backed table remains a Track P performance decision, to be taken on a profile.
- *The parse-error bucket was not a defect list* — 9 of its 14 rows are three upstream
  `cond-expand` typos chibi itself never compiles (§6 "Upstream, not ours"), so the honest
  pass-rate ceiling is lower than the bucket suggested.
- *In-degree beats intuition* — the original plan led with SRFI 26/13/41 on feel; measurement
  put 130/14 first and the corpus agreed.

*Note:* SRFI 64 is lower priority than its ubiquity elsewhere suggests — Snow packages overwhelmingly
test with `(chibi test)`, which Patina **already ships**. Primitive-backed work goes under
`crates/patina-runtime/src/stdlib/internal_*.rs`, registered in *both* the primitive registry and the
library builder; aligns with `PRD/PARALLEL_TRACKS.md` Track B3.
- **Porting patterns to reapply** (from `PRD/phase2/archive/SRFI_PORTING_ISSUES.md`): import `(scheme r5rs)` for R5RS naming (`exact->inexact` etc.); shim `:optional`/`let-optionals`/`receive`/`check-arg`; treat form-feed as whitespace (already fixed); defer arity rejection so `guard` can catch `apply` errors (already fixed); watch the VM control-op edge cases in `PRD/phase2/INSTRUCTION_LEVEL_CONTROL_OPS.md`.
- **Acceptance:** one integration test per SRFI exercising its headline forms; `./scripts/run_chibi_tests.sh` stays 1226/1226.

### L2 — Bundle the common `(chibi …)` libraries
Third-party packages frequently `(import (chibi …))`; today only `(chibi test)` exists. Port from the pinned chibi checkout the harness already fetches (locally mirrored at `~/Project/reference/chibi-scheme`), in the order L0.75/L3 dictate. Each ported library also brings its upstream `*-test.sld` suite, which drops straight into the L3 corpus:
- `(chibi match)` — pure Scheme, pervasive in chibi-authored packages. Highest leverage in this group. (✅ `(chibi optional)` turns out to have shipped long ago, with `(chibi test)` in #39 — noticed in the 2026-09-01 bookkeeping sweep; whether the SRFI ports' ad-hoc `:optional`/`let-optionals` shims can now be retired onto it is unchecked.)
- ✅ `(chibi string)` — **done 2026-08-14**, bundled as `(srfi 130)`'s dependency rather than on its
  own schedule, though it earned a place either way at in-degree 16, the highest in the corpus after
  `(slib common)`. Byte-identical to the 0.9.0 snowball; `lib/chibi/PROVENANCE.md` carries the
  record. Bundling it also retired `(srfi 13)` from the missing-library queue — chibi-binary-record and chibi-tar were asking for SRFI 13 only as the
  fallback their `cond-expand` reaches when `(chibi string)` is absent.
- `(chibi io)`, `(chibi pathname)` — mostly pure Scheme over R7RS + SRFIs present after L1.
- `(chibi show)` / SRFI 166 — the formatting library much of the ecosystem writes output with.
- ⚠️ `(chibi filesystem)` — **portable half done 2026-08-14; this entry's premise was wrong.**
  The blocker was never a missing primitive. Upstream's `cond-expand` has branches for `chibi`,
  `chicken` and `sagittarius` and **no `else`**, so on Patina the library loaded defining *nothing*
  and every importer failed on its first export — which is why five packages sat in the load-error
  bucket looking like five separate defects. Adding filesystem primitives alone would have moved
  none of them; there was no branch for them to land in. Worth generalising: a `cond-expand` with no
  `else` makes a library silently inert rather than absent, and the failure surfaces at the
  *importer*, naming an export instead of the cause.

  What landed: directory primitives routed through the VFS `FileSystem` trait as this entry
  intended (`directory-files`, `create-directory`, `delete-directory`, `current-directory`,
  `change-directory`, `file-directory?`, `file-regular?`), and a `(patina …)` branch in the bundled
  `lib/chibi/filesystem.sld` implementing the directory API over them. The POSIX layer — file
  descriptors, `stat` fields, symlinks, pipes, permissions — is stubbed with upstream's own
  `define-unimplemented` idiom, borrowed from its sagittarius branch, which stubs the same fd
  procedures for the same reason. That half is FFI work (`PRD/FFI_DESIGN.md`), and deliberately
  *not* VFS work: raw fds are what that abstraction exists to avoid, so implementing them through it
  would defeat the testability this entry asked for.

  Result: slib-directory, slib-uri and chibi-temp-file pass; chibi-tar advanced to an unrelated
  macro defect. Provenance and the boundary are recorded in `lib/chibi/PROVENANCE.md`; the branch is
  the largest local edit in the bundled tree and is pinned post-edit.
- `(chibi process)` — needs process primitives; the same VFS routing applies to whatever part of it
  is portable, and the same warning applies about checking for an `else` branch first.
- `(chibi uri)` — pure Scheme.
- **Out of scope permanently:** `(chibi ast)` and the other C-backed libraries (`.c`/`.dylib` in the reference tree) — they need FFI and bound the achievable pass rate. See §6.
- **Acceptance:** import + smoke-test each library.

### L3 — `patina-compat`: the self-contained compatibility harness  *(landed 2026-08-13; tier 0 live)*

✅ **The harness exists and the headline number is measured: 126 of 187 vendored packages pass**
(vm backend; snapshot in `compat/reports/results.scm`, matrix in `compat/reports/report.md`).
`cargo run -p patina-compat -- run` re-measures on a machine with nothing but this repo and a
release binary. Deviations from the spec below, all consequences of L4 vendoring the whole corpus:

- **Tier structure collapsed.** The full 187-package snow-fort corpus is vendored (tier 0), so
  `fetch` is unimplemented — nothing needs the network. The pinned chibi library tree (old tier 1)
  is the natural follow-up when the corpus needs breadth.
- **Results are an s-expression**, `compat/reports/results.scm` — read back by the harness with
  patina-frontend's own parser; no serialization dependency added.
- **Execution modes:** packages with a `(test "run-tests.scm")` declaration run their own suite
  (classified from output — the patina CLI runs test-named files resiliently, exit 0 always);
  the rest get a synthesized import probe of every library they provide (strict mode, exit
  meaningful). Cross-package deps resolve via `-A` from each package's transitive closure over
  the corpus's `package.scm` metadata. Packages run from a scratch cwd so their test loggers
  (srfi-64) cannot dirty the byte-identical vendor trees.
- **`parse-error` and `load-error` buckets were added** beyond the spec's list. A library that
  fails to parse leaves every importer unbound, so without the first bucket the real cause is
  misfiled; and the interpreter wraps load-stage failures (export resolution, library-body
  evaluation) in the same "Parse error in ..." message as genuine parse errors, so without the
  second the parser queue is inflated. **Recorded debt:** the durable fix is a distinct
  `LibraryError` variant for load failures with its own Display — the harness currently splits on
  message prefixes.

  **The debt is wider than that one overload, and is worth reading as a single item rather than
  as a growing list of prefixes.** `classify` is an *unenforced prose contract*: it turns on the
  exact wording of five markers produced by four crates — `Library (…) not found`,
  `Parse error in …`, `include:`/`load: parse error in '…'`, `Undefined variable:` /
  `unbound variable:`, and the FFI stub marker — and only the last is a string Patina emits
  deliberately for the harness to read. **No producing site says it is parsed.** Rewording
  `include: parse error in '{}'` to `include: could not parse '{}'` is a plainly harmless-looking
  improvement that would silently move a package from `parse-error` to `runtime-error`, a bucket
  read as "our runtime broke", with every test still green. The durable fix is a machine-readable
  error mode in the CLI — the same shape as the `--strict-errors` flag recorded below, and the
  same reason it is deferred rather than done here.
- **Recorded debt — the CLI's test-file heuristic.** `main.rs` runs any file whose name contains
  "test" resiliently (exit 0 always), which is why test-mode classification reads output rather
  than exit status, and why the harness keeps "test" out of its probe and scratch paths (two
  corpus slugs contain it). An explicit CLI mode flag (`--strict-errors`/`--resilient`) is the
  deeper fix, in the L0.5 spirit of the CLI growing what the harness needs.

**First measured queue (2026-08-13):** genuine parse errors block 23 packages (bare `@` 9,
syntax-rules shape restrictions 8, the rest singletons) and load errors 6 more; missing libraries
block 29, led by `(srfi 130)` ×4, `(chibi)` ×3, `(srfi 114 comparators)`/`(srfi 13)`/`(srfi 64)`
×2 each. The first fix is already in: **`#;` datum comments before a closing paren** broke the
parser (`(a b #;c)`), transitively blocking 19 packages through srfi-14's reference
implementation — fixed with the harness landing, moving the baseline 118 → 126.

**Parse-error triage, 2026-08-14 (126 → 128).** Every parse-error bucket was reduced to a minimal
repro and cross-checked against Gauche and Chez, which is what separates *our* defects from upstream
code that only chibi accepts. Two were ours: **template-introduced identifiers were compared by name
alone** (§6), unblocking **srfi-156**, and **a non-list library declaration hard-errored** (below),
unblocking **srfi-42**. The rest of the queue, with verdicts:

| Bucket | Pkgs | Verdict |
|---|---|---|
| bare `@` identifier | 9 | **Decided and ✅ landed** — we now read it (§6). Patina was right per R7RS 7.1.1, but strictness bought nothing: no conforming program contains a bare `@` token, so accepting it only widens the accepted language. |
| syntax-rules rule with >2 elements | 5 | **Chez rejects these too.** They are typos in chibi's own sources (e.g. `((_) bv off i)` in `ieee-754.scm`) that only chibi's lenient reader tolerates. Matching chibi here means accepting nonsense. |
| `syntax-rules` with no literals list | 3 | Upstream-malformed: `(syntax-rules ((_ x) 'x))` sits in `(chibi monad environment)`'s `cond-expand` **else** branch, which chibi itself never takes. Blocks chibi-show and chibi-snow-commands transitively. |
| `Duplicate parameter 'space' / 'symbol-first'` | 2 | **Ours — ✅ fixed** (§6, identifier identity). chibi-parse now passes. edn advanced to `duplicate pattern variable ch`, which **Chez raises verbatim on the same input**, so what is left there is chibi leniency, not our defect. |
| `Expected proper list in feature requirement` | 1 | **Ours — ✅ fixed.** srfi-42 ships `(cond-expand (stklos …) (else #f))`; the inert `#f` is not a list, so it hard-errored instead of taking L0's warn-and-skip path — and reported a message belonging to a shared list helper, misattributing the problem to the feature requirement. A declaration that is not a proper list, or whose head is not a symbol, now takes the same lenient path as an unknown keyword, still strict under `PATINA_STRICT_LIBRARY_SYNTAX=1`. This closes a gap in L0's own policy: leniency covered unrecognized *keywords* but not shapes that are not declarations at all, which is exactly what a cond-expand branch written for another implementation leaves behind. |

**Bare `@` accepted, 2026-08-14 (129 → 133).** The largest parse-error bucket is gone; the nine
packages it blocked landed as **4 passes** (chibi-html-parser, chibi-sxml, lassik-shell-quote,
slib-xml-parse) and 5 advances to a further failure. That split is the point: a lexer fix cannot
make a package pass on its own, and the advances are what re-ordered the queue —

- **`(srfi 130)` is now the top bundling target at in-degree 6**, up from 4: chibi-app and okmij-ssax
  both reached their imports and asked for it. It leads `(chibi)` ×3 and the ×2 tail by a clear
  margin, so L1 should take it next.
- **chibi-match now loads and runs**, and only its *test suite* still fails — on a genuine defect
  this unblocking exposed (§6, definition-env link lost through a nested `let-syntax`). Worth
  separating: the library third parties actually import is working.
- chibi-mime → `wrong-result` and lassik-dockerfile → `runtime-error` are new, unexamined, and are
  the first entries this track has in either bucket.

**SRFI 130 bundled, 2026-08-14 (133/187 → 134/185).** The denominator moves because L4's policy
drops what Patina bundles: srfi-14 and chibi-string left the corpus, both passing, so the like-for-
like baseline was 131/185 and this is **+3**. Two of the six SRFI 130 packages now pass outright
(chibi-edit-distance, chibi-locale), the other four advanced, and bundling `(chibi string)`
incidentally cleared `(srfi 13)` from the queue — chibi-binary-record now passes and chibi-tar
advanced. `(chibi net-dns)` reclassified to out-of-scope: with its imports resolving it could
finally report that it needs `(chibi net)`, which needs FFI.

**`build_corpus.py --offline` no longer shrinks the corpus** — ✅ **fixed 2026-08-14.** It used to
delete ten packages (srfi-2, 25, 29, 31, 42, 64, 106, 170, 227, 235) without an error: their
`license_evidence` is `srfi-canonical-document`, so their licence was established by fetching
srfi.schemers.org, and offline that fetch returned empty, the licence resolved to unknown, and they
dropped out of the PERMISSIVE bucket. `--offline --check` reported drift on twelve packages when two
had changed. Both bundling PRs in this track worked around it by removing their vendored packages by
hand.

Two changes, both of which make the tool *reuse an answer it already has* rather than derive it
again:

- **A licence is not resolved twice for the same tarball.** `recorded_licences()` reads
  `license`/`license_evidence` back from the committed `MANIFEST.json`, keyed on `tarball_sha256`.
  This is for correctness, not speed: it also skips `read_blob`, but that is worth about 0.2s of a
  0.7s rebuild over 184 packages, since the packages are small. The reuse is **scoped to the
  cache-only path**, and that scoping is load-bearing: a checksum match proves the *package* is
  unchanged, but two of `resolve_licence`'s three sources — the index's `license` field and the
  canonical SRFI document — are not determined by the tarball at all. Reusing them is right when the
  alternative is no answer and wrong when the user has asked to go and look, so `--refresh` re-derives
  every licence and re-fetches both the index and those pages. `--refresh` did not previously re-fetch
  the index at all — it was fetched once, ever — which the flag flip turned from a latent oddity into
  a flag that lied.
- **The canonical SRFI pages are cached**, as the index and the tarballs already were. That
  asymmetry was the root of it: those pages were the one thing the tool fetched every run and kept
  nothing of, which made `--offline` lossy rather than merely slower.

A cache-only `--check` now reports **184 packages and no package-level drift**, matching the
committed corpus exactly.

**A licence is recorded even for a package we decline to vendor.** It is the same answer and it cost
the same to establish, so `REVIEW-QUEUE.json` now carries `tarball_sha256` and `license_evidence`
beside the `license` it already had, and `recorded_licences()` reads both files. This is plumbing
until the next `--refresh` populates those fields; today one package, `srfi 5`, is the only one that
still re-derives — its licence is the SRFI document's, so a cache-only run reads it as unknown and
`INVENTORY.md`'s excluded counts differ by that one row.

**A bundled package is excluded on grounds that have nothing to do with its licence**, so it no
longer appears in the licence-based reports at all. That was the other half of the discrepancy:
`srfi 27` is bundled and therefore in neither generated file, so it had no recorded licence to
reuse and read as unknown from the cache but permissive after a refresh — pure noise, now gone. It
is reported where it belongs instead, in the "excluded N packages Patina bundles" line, which counts
13 rather than 12 because it no longer depends on having priced a licence that does not matter.

The corpus itself is never affected by any of this.

**The default now rebuilds from the pinned cache; `--refresh` asks upstream what is new.** The flags
used to name the mechanism — network or no network — when the real question is whether to refresh,
and the riskier answer was the default. A bare run re-fetched the index and took the highest version
per package, so it could bump upstream versions and fold an unrelated corpus refresh into a change
that meant only to drop a bundled package. That is how a corpus drifts without anyone deciding to.
So: default = rebuild from what is pinned, `--refresh` = go ask, `--offline` kept as an alias for the
default it used to request, and the two are rejected together.

This also puts the recurring job on the safe path by default. When Patina bundles a library, the
corpus must drop the package providing it, and that needs no network at all — a cached tarball was
never re-downloaded even before this, so an unchanged package now costs one index request under
`--refresh` and nothing at all without it.

**`(chibi filesystem)`'s portable half, 2026-08-14 (134/185 → 137/184).** chibi-filesystem left the
corpus on being bundled and was failing, so the baseline is 134/184 and this is **+3**:
slib-directory, slib-uri and chibi-temp-file pass, chibi-tar advanced to an unrelated macro defect.
The load-error bucket went 8 → 3 and `Exported identifier 'duplicate-file-descriptor' not defined`,
its largest entry, is gone. See L2 for why the cause was a missing `cond-expand` branch rather than
a missing primitive.

**The harness learned a third way to be out-of-scope.** It already knew a package whose only missing
imports are FFI-bound libraries. It now also recognises one that *reached* an FFI stub at runtime:
the bundled `(chibi filesystem)` raises a marker string from its `define-unimplemented` procedures,
and `classify` reports those as out-of-scope rather than as our runtime errors. The rule is unit
tested but currently unexercised by the corpus — no package gets that far today — so it is a
correctness guard against future misfiling, not a source of the numbers above.

**Relinking fixed at a `quote` argument, 2026-08-14 (137/184, unchanged).** The headline does not
move and that is the honest report: chibi-match went from `parse-error` — the library dead, its
suite unrunnable — to `wrong-result`, with **74 of its 75 upstream tests passing**. A package scores
`pass` or it does not, so a suite that goes from zero to 74/75 registers as no change at all. The
remaining failure is `match-letrec` (§6; fixed 2026-08-20 — the suite is 75/75 and the package
passes). Worth noting for reading the number: the corpus counts
packages, not tests, so it under-reports exactly this kind of progress.

**Ports became parameter objects, 2026-08-15 (137 → 138 of 184).** lassik-dockerfile was the corpus's
only `runtime-error` and now passes: it redirects output with
`(parameterize ((current-output-port …)) …)`, which until now was rejected outright. A conformance
fix reaching a package nobody had connected to it is the argument for fixing the standard rather
than the symptom — the queue had this filed as an unexamined runtime error, not as a missing
parameter object.

**The queue was pointing at libraries the corpus already had, 2026-08-16 (138 → 141 of 184).**
Four rows of the missing-library histogram — the table this track treats *as* its work queue — were
harness artifacts, and two of them named a library sitting in `compat/vendor/` and passing. Two
clauses of `package.scm` the runner never read:

- **`(test-depends ...)`.** `corpus.rs` collected `depends` only from `library`/`program`
  components, so a dependency only the test program needs was invisible. lassik-string-inflection
  declares `(srfi 64)` there and was filed as missing it; with the dep on `-A` its suite passes
  18/18. Now kept in a field of its own rather than folded into `depends`: it seeds the closure only
  when that test program is what we are about to run, and is never followed out of a *dependency*,
  because nobody importing a package needs its test framework.
- **`(path ...)`.** Patina resolves `(chibi irregex)` at `chibi/irregex.sld` under a search root;
  four vendored packages ship the file elsewhere and say so in `package.scm`. Two of them —
  chibi-irregex and macduffie-json — could not find *their own* library and reported it as missing.
  The runner now stages the name-shaped layout into the scratch directory before running, which is
  what snow's installer does: `default-installer` (chibi's `lib/chibi/snow/commands.scm`) *reads*
  the declared path and *writes* the name-derived one. One deliberate difference — snow keeps
  includes at their package-relative paths, which leaves them unreachable from the `.sld`'s new
  home, so we mirror the `.sld`'s whole directory instead of moving the file alone.

**Staging is not what moved srfi-197, and the fourth row is worth reading carefully.** It was filed
as missing `(srfi 2)`, which the corpus provides; with that resolved it reports the real defect,
`Lexer error: Unexpected character: …` — the reference implementation uses `…₁` as its custom
ellipsis, so **Patina rejects Unicode identifiers**, which R7RS permits and every major
implementation accepts. Its `(path ...)` is off-convention too, but that never mattered: its test
program `(include "./srfi-197.scm")`s the source directly and never imports the library. It will not
pass even with Unicode identifiers — it also includes a `./test.scm` the package does not ship.

**Also fixed: an unlexable *included* file was filed as a runtime error.** `include` and `load` word
a parse failure their own way and quote the path (`include: parse error in '<file>': <detail>`),
which the classifier's "Parse error in ..." matcher missed, so it fell through to `runtime-error` —
a bucket meaning "our runtime broke" for something that never ran. That is how srfi-197 first
landed after the two fixes above, and it is the same misfiling in a third place.

**Two buckets triaged while reading the queue, 2026-08-16 — neither is ours.** chibi-mime's
`wrong-result` (5 of 9, all four failures in `mime-message->sxml`) was recorded as unexamined; it is
`read-line expects 0-1 arguments, got 2`. `chibi/mime.scm` passes chibi's line-length limit while
importing only `(scheme base)`, i.e. it uses a chibi extension without importing `(chibi io)`.

The `No matching pattern for macro case` pair is two *different* invalid shapes that report
identically, which is worth knowing before anyone treats the bucket as one defect: chibi-app has an
`else` that is not last (`(case (length commands) ((0) …) (else …) ((1) …))`, `app.scm:467`, so the
trailing clause is dead code), and chibi-tar has a clause with no body at all (`((#\g #\x))`,
`tar.scm:162`). R7RS's grammar rejects both — `else` is the final clause and a clause needs one or
more expressions — but neither is unanimously rejected in practice, so this is a leniency call
rather than a settled one: on `else`-not-last, Gauche errors ("'else' clause followed by more
clauses") while chibi takes the `else` and Chez takes the trailing clause; on the empty body, Chez
errors ("invalid case clause") while chibi and Gauche both fall through to `else`. Declining both
keeps the stance the `syntax-rules` buckets already set, and each package has exactly one occurrence.

**Core syntactic keywords became exportable, 2026-08-16 (141 → 143 of 184).** r6rs-base and
r6rs-arithmetic-fixnums were the whole of the load-error bucket bar one, both on
`Exported identifier 'begin' not defined`, and both now pass. `(r6rs base)` opens by re-exporting
the whole of R7RS's syntax — which R7RS §5.6.1 permits and every implementation accepts — and
Patina rejected the library outright. Full write-up in `PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md`; the
short version is that these keywords are recognized *by name* rather than bound, so export
resolution had nothing to look up, and `lib/scheme/base.sld` shows the same hole from the other
side by omitting them from its own export list.

Worth recording as a queue-reading lesson rather than only as a fix: this sat in `load-error`, a
bucket of two packages, and looked like a two-package problem. It was a conformance gap in the
library machinery that any package re-exporting standard syntax would have hit — the bucket size
measured how much of *this corpus* trips it, not how big the defect was.

The same reading error nearly shipped inside the fix. It landed with a comment claiming `(only …)`
could not *hide* a core keyword, when in fact `only` **rejected the program** — so narrowing a
blanket re-export, the first thing a consumer does with one, would have failed on the default
backend. The export side is where the corpus pointed, and the corpus only measures what its
packages happen to do; the import side had no package exercising it and so said nothing. Fixed
with it, and `(only …)` now agrees.

**Unicode identifiers accepted, 2026-08-16 (143 of 184, unchanged).** The headline does not move
and the defect entry predicted that before the work started, which is the point of having said it:
srfi-197 was the only package needing it, and it advanced from `parse-error` to `runtime-error`
because its test program `(include "./test.scm")`s a file the package does not ship. That last
detail is upstream, not ours — worth knowing before anyone reopens the row, since `runtime-error`
now holds one package that is not a defect of Patina's.

Two independent causes, and finding only the first would have left SRFI 197 failing with a
different message: `…` was rejected by the identifier predicate, while `₁` never reached that
predicate at all — the token dispatch tests `char::is_numeric`, which is Unicode-aware, so a
subscript was claimed by the *number* reader. Full write-up in the fixed-defects archive.

**SRFI 125 and `(scheme hash-table)` bundled, 2026-08-16 (143 of 184, unchanged).** The corpus
number does not move: chibi-voting was the only package asking for `(scheme hash-table)`, and it
advances from `missing-library` to `wrong-result` at **5 of 7** — both failures are ties broken in a
different order (`((C . D) . 3) ((B . D) . 3) …` against our `((C . D) . 3) ((A . B) . 3) …`), i.e.
the suite depends on hash-table iteration order, which neither SRFI specifies. Not ours to fix.

The port's value is R7RS-large conformance, and it is measured by chibi's own SRFI 125 suite,
now the eighth in `scheme_tests/upstream/`: **74 of 74**. Two of those needed the suite's import
list disambiguated — it imports `(srfi 125)` *and* `(srfi 128)`, which bind `string-hash` to
different procedures (SRFI 69's takes an optional bound, SRFI 128's does not), an ambiguity chibi
never has because its `string-hash` is one native procedure under both names. The exclusion picks
SRFI 125's, which is what the assertions calling it with a bound expect and what SRFI 125
specifies; test bodies are untouched, and the adaptation is described in
`scheme_tests/upstream/README.md` beside SRFI 130's.

That file also had to define five comparator constants belonging to **SRFI 162**, which chibi folds
into its `(srfi 128)`. Bundling SRFI 162 is the obvious next Red-edition step and would retire that
half of the adaptation.

**Four deviations from upstream's file were needed, each because chibi's SRFI 69 is C-backed and
ours is the portable reference implementation** — recorded in `lib/srfi/125.sld`'s header, with
`125/hash.scm` itself byte-identical: a SRFI 128 comparator's one-argument hash function adapted to
SRFI 69's `(obj bound)` convention; immutability, which upstream takes from `(chibi ast)`, tracked
beside the tables; `hash-table-ref`'s and `hash-table-update!`'s `success` argument; and
`hash-table-union!`'s do-not-overwrite rule.

**Review found three defects the 74-test suite had passed over, which is the part worth keeping.**
All three were in the deviations — the code that is *ours* — and none was reachable from the
suite: `hash-table-update!` accepted `success` and silently ignored it (SRFI 69's rest argument
swallowed it, giving 8 where SRFI 125 specifies 71); the SRFI 69 hash fix covered one of the *two*
branches that can return an inexact value, so `2.0` still crashed the table while `2.718` no longer
did, and the test written for it exercised only the branch that was fixed; and the hash-function
adapter's placement is a genuine trade whose two sides fail in opposite directions — adapting only
the comparator path breaks a caller who extracts `(comparator-hash-function c)` by hand, which the
upstream suite does at one fixture, while adapting everything breaks a hash function that *requires*
two arguments, which plain `(srfi 69)` accepts. The second is the better trade and is now the
documented behaviour. The lesson is the recurring one: an upstream suite gates the *ported* code,
not the glue written to port it.

**A live defect in `(srfi 69)` fell out of it, and matters more than the new library does.** SRFI 69
has in-degree 16 in the corpus, and *any float key crashed it*:

```scheme
(hash-table-set! (make-hash-table) 2.718 'e)
;; before => Error: Type error: vector-ref expects an integer index
```

R7RS makes `numerator`/`denominator` return inexact results for inexact arguments, so the reference
implementation's `hash` returned an inexact value, which it then used as a vector index. chibi never
hits it because its SRFI 69 is native. Fixed in the bundled copy and marked `PATINA DEVIATION`
in place.

**`report.md` is written to disk again**, closing the recorded debt that `run` printed the rendered
matrix to stdout and wrote only `results.scm` — so the committed copy went stale unless the caller
redirected into it. `run` now writes both artifacts under the same subset-run guard (`--report` sets
the path); `report` still only prints, which is its job.

**The score stopped counting packages nobody can fix, 2026-08-22 (126 of 162 → 126 of 137 in
scope).** The raw number does not move and is still printed first; what changed is that 25 of the
36 failures now say, in a committed file, why they are not a measurement of Patina — and, more
usefully, stopped appearing in the work queue. Two pieces:

**1. Patina reads `include-shared`.** It was an unknown declaration, so L0's warn-and-skip
swallowed it and the library loaded defining nothing; `(chibi mecab)` then failed at its importer
with `Exported identifier 'mecab?' not defined`. That is the `(chibi filesystem)` hazard for the
third time in this track — a library left parsed and empty, reported anywhere but at the cause —
and this time the cause is not a missing branch but a clause we can never honour. It is now a
known declaration that is refused where it appears, with its own `LibraryError` variant naming the
shared object.

The important property is that **cond-expand already separates the two cases for free**. A branch
that is not taken never reaches the dispatch, so `(chibi crypto sha2)` and `(chibi math linalg)` —
which both ship a `.stub` and both put `include-shared` in a `chibi`-only branch — keep their
portable paths and are *not* FFI-bound. Any rule based on the presence of a `.stub` or `.c` file in
the package would have misfiled both; two of the seven, which is the whole argument for letting the
runtime answer instead of a scan.

This is also the first piece of the "unenforced prose contract" debt recorded above to be paid:
`patina_runtime::NATIVE_EXTENSION_MARKER` is a `pub const` the harness links against, so rewording
that message breaks the build rather than silently moving a package between buckets.

**2. `compat/EXCLUSIONS.scm`, the opt-out list.** A committed s-expression naming each package that
cannot pass for a reason that is not about Patina, under a closed set of reasons: `ffi` (9),
`dependency-not-vendored` (2), `upstream-source-defect` (10), `upstream-test-defect` (4). The
`upstream-source-defect` rows are the audit above, entry for entry, each carrying the file, the
line and the implementation that agrees with us in rejecting it.

Three properties keep it from becoming somewhere to park failures, and they are the reason it is
worth having at all rather than a footnote in this document:

- **An excluded package still runs**, on every pass, and `results.scm` records its status exactly
  as before. Exclusion decides whether a result counts, never whether it is measured. The snapshot
  is the measurement, this file is the policy, and the report is the join — so the three can be
  reviewed separately and a change to one is visible in the diff of one.
- **Every entry declares the status it expects**, and the report calls out an entry that no longer
  matches instead of applying it. A package that starts *passing* is drift too: that is how an
  entry gets retired rather than outliving its reason. A slug the corpus no longer has is reported
  as well — an entry matching nothing subtracts nothing while reading as though it did.
- **The raw number is printed first and is never affected by the file.** `--exclusions none` scores
  every package.

What the exclusions bought, and it is the part worth keeping: **the bundling queue is now five
rows, all actionable** — `(srfi 114 comparators)` ×2, `(srfi 144)` ×2, `(scheme flonum)`, `(srfi
165)`, `(srfi 231)`. It previously carried `(chibi io)` ×1, `(srfi 160 base)` ×1 and `(chibi)` ×3,
and **every one of those five was un-actionable**: the first two are wanted only by C-shim packages
that would still fail with the library in hand, and the `(chibi)` row is corrected below.

**`(chibi)` ×3 was not three packages needing chibi's core.** Two — chibi-xlib,
independentresearch-xattr — are `include-shared` packages that reported `(chibi)` on the way to
needing a `.so`. The third, chibi-assert, has a **portable library that works on Patina today**:
its `cond-expand` else branch is a pure-Scheme `assert`, verified by running it. Only
`chibi/assert-test.sld` imports `(chibi)`, for `protect` and `exception-irritants`. So the row that
this document twice described as the largest permanently-out-of-reach dependency was one C-shim
pair and one chibi-only *test program*. A histogram row is a count of symptoms, not a diagnosis —
the same lesson as the `load-error` bucket of two that turned out to be a conformance gap.

**Scope decision recorded, not taken:** whether the FFI packages are eventually dropped from the
corpus or rescoped onto a future Rust FFI is left open. The opt-out is what both futures need, and
it is deliberately not the same mechanism as either — dropping them is a `build_corpus.py` policy
change, and rescoping them is a new track. Note for whoever takes it: a Rust FFI would **not** make
these packages load unchanged. They are `include-shared` against a chibi-compiled `.stub`, so what
they need is chibi's C ABI, not any FFI.

One row is knowingly imprecise: **chibi-xgboost is FFI-bound but reports `missing-library`**,
because its test library's `(srfi 160 base)` import fails before `(chibi xgboost)` is ever reached.
Its exclusion records the real reason and expects the shadowed status, so if SRFI 160 is ever
bundled the entry drifts and asks to be looked at again.

#### Original spec
A new workspace crate, `crates/patina-compat/`, that owns the whole loop. Everything it needs is in this repo; it shells out to nothing but the Patina binary under test.

**Why a Rust crate rather than a shell script:** the repo's existing `scripts/*.sh` shell out to `curl`/`tar` and depend on ambient tooling. A workspace crate matches the repo idiom, gets `cargo test` and CI for free, and keeps fetch/extract/checksum in-process — which is what makes the harness genuinely self-contained and reproducible on a bare machine.

**Subcommands:** `fetch` (populate the cache from the manifest, verifying checksums) · `run` (execute the corpus, emit machine-readable results) · `report` (render the matrix and histograms).

**Corpus, in three tiers — all pinned, none requiring a package manager:**
| Tier | Source | Network | Runs in CI |
|---|---|---|---|
| **0 — smoke** | A handful of small, permissively-licensed libraries vendored under `compat/vendor/` with attribution | none | always, hermetic |
| **1 — chibi library tree** | Pinned git commit; **50 `*-test.sld` suites** already written, minus the 34 C-backed libraries | on `fetch` | nightly |
| **2 — snow-fort packages** | Manifest of name + version + URL + sha256; snowballs are tarballs, `package.scm` is s-expressions, both read directly | on `fetch` | nightly |

Tier 0 keeps ordinary CI fast and offline; tiers 1–2 give the breadth the goal asks for. Only tier 0 enters git history, so there is no licence or bloat problem — and nothing here is secret, so the harness and its results are public.

**Failure classification (the real payload).** Every run buckets each library as `pass` / `missing-library` / `unbound-identifier` / `runtime-error` / `wrong-result` / `out-of-scope (FFI)`. **The unbound-identifier and missing-library histograms are the L1/L2 work queue**, regenerated free on every run — this is what keeps the track driven by measurement instead of by the guessed lists above.

**Reporting.** A checked-in results snapshot plus a generated matrix, reusing the shape of `scripts/run_chibi_tests.sh` → `scheme_tests/reports/compatibility.md`. Report the achievable denominator (excluding FFI-bound libraries) next to the raw one.

**Location.** In-tree, so a fix and its results delta land in the same PR; a separate repo invites version skew about which Patina produced which matrix.

- **Acceptance:** `cargo run -p patina-compat -- run` reports a baseline "N of M" on a machine with no Scheme installed.

**What gates CI, and what does not.** The corpus pass rate is a *measurement*, not a promise, so it
runs out-of-band and never blocks a merge — a third-party package regressing is information, not a
build break. Bundled libraries are the opposite: they are part of what Patina ships, so their tests
**do** run in routine CI. See L4.

---

### L4 — Make bundled ports canonical, then drop their vendored duplicates  *(done)*

✅ **Done.** `build_corpus.py` now excludes any package whose library Patina bundles, computed from
`lib/` rather than listed, and the corpus dropped from 197 to 188. The drift guard
(`bundled_vs_vendored.rs`) and the `bundled_by_patina` manifest field are deleted.

The guard was worth having while it lasted — it caught `(chibi test)` missing 26 upstream exports —
but its premise expired when `(srfi 60)` was bundled: Patina's is a rename over SRFI 151 while the
vendored one is Jaffer's SLIB implementation, so comparing them measures a deliberate decision rather
than a defect. Keeping both would also have meant answering, for every corpus run, which copy a test
resolved against.

**Postscript (2026-08-19): the exclusion had a side effect nothing caught for a week.** The dropped
packages' snowballs ship their test suites, and while they were vendored those suites ran in the
compat harness; excluding the packages silently removed the suites from everything that runs, so
five bundled chibi libraries had no upstream tests anywhere. Closed by restoring the suites to
`scheme_tests/upstream/` (four run clean — string 52, optional 11, diff 7, term ansi 234;
filesystem is FFI-blocked and recorded as such), running `(srfi 14 test)` alongside them — which
immediately caught a real defect, §6's `ucs-range->char-set` entry — and adding a guard test
(`every_bundled_library_has_a_suite_or_a_recorded_reason`) that walks `lib/srfi` + `lib/chibi`
against the suite table plus a `NO_SUITE` reasons list, so a future bundling cannot reopen the hole.

**Target state:** every bundled library is a faithful import of its upstream reference rather than a
subset or a local adaptation; the **bundled version is canonical**; and the vendored duplicate is
gone from the corpus, so there is no shadowing question to answer and no second copy to keep in sync.

**Why it is not done yet:** the ports have diverged from upstream, and deleting the references before
reconciling them would discard the only evidence of how. Measured today:

| Library | Divergence from upstream |
|---|---|
| `(chibi test)` | ✅ **Done** — replaced with a verbatim upstream import. The subset counted 1163 tests where the framework counts 1226, and reported three failures as passes. |
| `(srfi 1)` | Does not re-export the 26 `c[ad]+r` accessors upstream does; exports `make-list`/`list-copy`, which upstream leaves to `(scheme base)`. 74 lines of real drift, rest whitespace. |
| `(srfi 128)` | Missing `%salt%` (internal, not in the SRFI — likely correct to keep out). |
| `(srfi 8)`, `(srfi 69)` | Identical. Ready to un-duplicate now. |

**`(chibi test)` is done.** It was written as a hack — the minimum needed to get the chibi R7RS
suite running — and was replaced with a verbatim upstream import once the template-ellipsis (#37) and
macro-hygiene (#38) bugs blocking its dependency closure were fixed.

Adopting it changed the headline number, and for the better: the subset counted **1163** tests where
the framework counts **1226**, and it silently reported three failures as passes. Those three were a
real `eqv?` bug on `+nan.0` and `-0.0`, fixed alongside. The subset also special-cased NaN in its own
comparator — the harness was concealing a defect in the thing it existed to measure.

It also made a pre-existing backend gap visible: the tree-walker errors on 15 tests with
`Undefined variable: k_N`, a CPS bug on nested `guard` across a procedure boundary. Repro and
analysis in §6.

**Steps:** reconcile each port against its vendored reference → remove the vendored copy → let
`build_corpus.py` exclude bundled libraries automatically via `bundled_libraries()` → delete
`crates/patina-tests/tests/bundled_vs_vendored.rs` and the `bundled_by_patina` manifest flag, which
exist only to make the interim state safe.

- **Acceptance:** no package in `compat/vendor/` is flagged `bundled_by_patina`; the drift guard and
  the flag are deleted; `./scripts/run_chibi_tests.sh` still reports 1226/1226.

---

### L5 — Read R6RS, so the corpus has a second opinion  *(in execution — bridge landed 2026-08-19; suite deferred)*

**Why this is in Track L rather than beside it.** The track's headline is a
compatibility number, and the number was measuring less than it appeared to.
Of the 184 packages then vendored, **137 ran in probe mode** — the harness
synthesises `(import …)` of everything a package provides and checks it does
not error; nothing is ever called. Only 47 run a test suite, and **29 of those
47 are chibi packages**, because chibi is the only ecosystem whose snowballs
ship their suites. So 113 packages — every slib, every pfds, most srfi — never
executed an assertion, and the behavioural half of the score was chibi's. That
is not a harness failing: only 2 of the 137 probe packages ship a test file at
all. It is a property of the corpus.

The R7RS gate has the same shape one level up. `scheme_tests/chibi/r7rs-tests.scm`
is one file by one author, and the eight upstream SRFI suites, though genuinely
the specifications' own, all arrive through chibi's `lib/`.

R6RS is where an independent, specification-derived suite exists, written by
different people against a different standard. Reading R6RS is the prerequisite
for running it, and the ceiling is worth knowing before starting: `syntax-case`
is Phase 3, and the record, condition and port libraries are not bundled, so
what a suite can reach is bounded — see the table below.

#### L5.0 — The reader  *(done — 2026-08-19)*

Four gaps, each verified against the binary before the work and each a syntax
R7RS 7.1.1 *reserves*, so reading it cannot change what a conforming program
means — the trade the bare `@` decision already settled.

| Gap | Before |
|---|---|
| `[` `]` | `Reserved character (R7RS): [` |
| `#vu8(1 2 3)` | `Unexpected character: v` |
| `(rnrs base (6))` | `Library name parts must be identifiers or non-negative integers, got: pair` |
| `(library …)` | no code path — only `define-library` |

`#!r6rs` already worked, as a consequence of L0's shebang handling.

Bracket **pairing is enforced**: `(let ([x 1)]) x)` is refused. Cross-checked
rather than assumed — Gauche accepts brackets and rejects the mismatch, chibi
does not read brackets as parentheses at all, and Chez, Racket and Guile agree
with Gauche. Accepting them unpaired would have been a dialect nobody has.

`is_delimiter` had to widen to match, and that function's comment had flagged
the widening as wanting "its own decision and its own cross-check": without `]`
in the set, `[x 1]` runs to the end of `1]`, which the number reader rejects.
Safe in the direction the comment warns about — a bracket is neither an
`<initial>` nor a `<subsequent>`, so no conforming identifier contains one —
and cross-checked across `compat/vendor/` and `lib/`, where every bracket
outside a string or comment is `#\[` or `#\]`. Those are unaffected:
`read_character` already takes a delimiter first character as a complete
one-character literal, the path `#\(` has always used.

Version references are **discarded, not matched**. Patina resolves one library
per name and has nowhere to put a second, so checking `((>= 6))` against it
would report agreement it never established.

`.sls` resolves alongside `.sld`. Doing that surfaced that the resolution
exists **twice** — `LibraryRegistry::find_library_file` and
`SchemeLibraryLoader::find_sld_file` — and that only the second is on the path
an actual load takes, so the first appeared fixed while nothing loaded. The
extension list is now one shared constant rather than two lists and a comment
asking them to agree.

#### L5.1 — `--allow-r6rs`, off by default  *(done — 2026-08-19)*

The reader extensions are **off unless asked**. The widening argument above is
sound and is why they can be switched on at all; what it does not cover is that
Patina is a teaching implementation, and a learner who writes `(let ([x 1]) …)`,
watches it work, and concludes brackets are standard has been taught something
false by us.

One switch, not an inferred per-file mode. A mode needs a trigger and every
candidate is unreliable — `#!r6rs` is optional in R6RS and routinely omitted,
`.sls` is convention rather than normative, and an inline `(library …)` at the
REPL has no file — so an inferred mode would be right most of the time and
*silently* wrong the rest. The cost is granularity: the switch is process-wide,
so a program run with it gets R6RS reading for every file it loads. The
refinement available later is to **widen, never narrow**, for a file that
declares itself by `.sls` or `#!r6rs`; that does not reopen the objection,
because such a file is opting in about itself and the switch remains the escape
hatch for one that does not.

#### L5.2 — Bundle the R6RS libraries  *(done — 2026-08-19)*

William D Clinger's R7RS ports, byte-identical from the snow-fort tarballs
under `lib/r6rs/`, with a generated re-export `.sld` per library under
`lib/rnrs/` so R6RS source resolves without being rewritten. 17 libraries.

**They work, and that was measured by calling into them rather than importing
them** — the same distinction this item exists to fix. `div`/`mod`, `assp`/
`remp`, fixnum ops, enum sets, hashtables with non-fixnum keys,
`char-general-category`, `raise-continuable`, and `eval` with
`(environment '(rnrs base))`: **17 of 17**.

Two findings, both in `lib/r6rs/PROVENANCE.md`:

- **`(r6rs no-rnrs)` is required and upstream provides it.** Every guard in the
  tree defers to a host `(rnrs …)` when one exists. Patina's `(rnrs …)` *is*
  these libraries, so that branch closes a cycle; defining upstream's marker
  says "that is not a host implementation" and sends every guard to the
  portable branch, which is why the tree needs no edit.
- **One guard was missing the escape.** `hashtables.sld`'s first `cond-expand`
  tests `(library (rnrs hashtables))` alone where its two siblings in the same
  file do not. With the shim present it fires, `inexact-hash` is never defined,
  and every non-fixnum key fails **at the caller**. A/B'd: shim absent, float,
  rational and symbol keys all work; shim present, none do. This is the
  `(chibi filesystem)` hazard again — a `cond-expand` that leaves a library
  parsed and empty, reported anywhere but at the cause — and it is why
  `r6rs_rnrs_shims.rs` calls into each library instead of importing it.

Per L4's policy the corpus drops what Patina bundles, so the headline moves
**143/184 → 121/162**. Like-for-like unchanged: the per-package matrix is
identical once the 22 bundled rows are removed, and every failure bucket keeps
its count.

#### L5.3 — Run Larceny's R7RS suites from a reference checkout  *(harness landed 2026-08-24; baseline measured)*

**Retargeted.** This item was written as "vendor and run the R6RS test suite",
and the first thing scoping it found was that the suite cannot load here: its
harness `tests/r6rs/test.sls` imports the composite `(rnrs)` and uses R6RS
`define-record-type`, neither of which is bundled. The second thing it found
was better than the fix: **Will Clinger already rewrote that suite for R7RS**,
in Larceny's `test/R7RS/Lib/` — two suites over one R7RS-small harness,
`(tests scheme test)`, which imports only `(scheme base)`, `(scheme cxr)` and
`(scheme write)`:

- `tests/scheme/` — 33 suites: every R7RS-small library plus the Red-edition
  ones (box, comparator, generator, hash-table, ideque, ilist, list,
  list-queue, lseq, rlist, set, sort, stream, text, vector, charset, flonum,
  ephemeron).
- `tests/r6rs/` — the R6RS suite in `define-library` syntax against
  `(r6rs …)` — the exact libraries L5.2 bundled. No `(rnrs)` composite, no
  R6RS record syntax.

This is the second opinion L5 wanted, and a better one than R6RS's: the same
standard as the headline, a different author and provenance (Racket's suite,
mostly by Matthew Flatt, rewritten by Clinger), and coverage of the
Red-edition libraries Patina bundled that until now had only chibi's suites.
Racket's own R7RS package was checked for this role and rejected: its tests
are five small Racket-specific files plus a copy of chibi's `r7rs-tests.scm`.

**Not vendored, by decision.** The suite is LGPL — Larceny's README: derived
from Racket's R6RS tests and "covered by the LGPL license due to its
derivative nature". Racket has since relicensed its copy to Apache-2.0/MIT,
but Clinger's rewrite carries the LGPL notice and Larceny is unmaintained.
Patina is MIT and `compat/vendor/` is permissive-only. So the suite lives in
a reference checkout outside the repo — the same arrangement as
`~/Project/reference/chibi-scheme` — and `scripts/run_larceny_tests.sh` runs
it from there, pinned to `fef550c` (2017-09-08, Larceny's last commit) and
warning on drift. The cost is the CI lane: this is a local, on-demand second
opinion, not a gate. The reports it writes (`scheme_tests/reports/larceny*.md`)
are tracked; the per-suite logs beside them are not.

**Runner.** `./scripts/run_larceny_tests.sh [--tree-walker] [--r6rs] [suite…]`.
One `run/<suite>.sps` program per suite, run from the Lib directory with
`-I .` as upstream's own scripts do (`base.sld` includes
`tests/scheme/base-test1.scm` cwd-relative); the R6RS lane adds
`--allow-r6rs` because those sources use `#vu8(` and brackets. Tallies are
the harness's own (`N tests passed` / `N of M tests failed.`), never
re-derived. A suite whose library fails to load reaches no tally and is
reported as **error** with zero assertions, so the assertion total
under-reports exactly as much as is broken and the *suite* column is the one
to watch. Crashes and timeouts (perl `alarm`, 300 s — macOS has no `timeout`)
are their own statuses.

**Baseline, 2026-08-24:**

| Lane | Suites clean | Assertions | Blocked outright |
|---|---|---|---|
| R7RS, VM | 12 of 33 | 4070 of 4100 (99.3%) | 10 unbundled Red-edition libraries · `base` (include) · `lazy`, `read` (stack overflow) |
| R7RS, tree-walker | 12 of 33 | 3931 of 3961 (99.2%) | the same, plus `char` (stack overflow) |
| R6RS, VM | 10 of 16 | 4008 of 4022 (99.7%) | `base` (`(let-syntax ())`) · `mutable-pairs` (hang) |

**Fourth round, 2026-08-25 — `base` ran, briefly.** Scope-aware shadowing plus a per-token ellipsis
let the suite load (1046 of 1064), which is how five more families were found and pinned; review
then showed the shadowing rule capturing a parameter named `if`, letting an outer variable veto an
inner `let-syntax`, and breaking macro-generating macros, and it was backed out. What stays: cycle
guards on the expansion walkers (a labelled literal as a macro argument no longer hangs the load —
the scope flip is a memoized, cycle-closing copy) and per-datum reader labels. `base` is gated on
the syntax-keyword-bindings project again, now with the two reviews' cases as its acceptance tests.

**Third round, 2026-08-25:** the VM's multiple-values side buffer is gone — R7RS lane **16 of 33
suites, 4260 of 4270 (VM)**; `vector` is clean. Two tree-walker gaps found on the way are pinned
(a continuation invoked with other than one value; `(values)` reaching a consumer as one value).

**Second round, 2026-08-24 (#112):** `equal?` terminates on cycles, `delay-force` is iterative on
both backends, and `string->number` is the reader's number syntax with exact `#e` decimals, and a `;` comment ends at
a bare return — R7RS lane **15 of 33 suites, 4258 of 4270 (VM)**, tree-walker 15 of 33 / 4113 of
4131, R6RS lane 12 of 16 / 4017 of 4025. `lazy`, `read` and R6RS `mutable-pairs` are clean;
`inexact` and `complex` are down to the two expectations that are upstream's own.

**Low-hanging fixes, 2026-08-24 (same day):** nested `include`, the port-open predicates, `rationalize`
at the infinities, Unicode simple case mapping, `(scheme charset)` — R7RS lane
**13 of 33 suites, 4172 of 4193 (VM)**, tree-walker 13 of 33 / 4027 of 4054, R6RS lane 11 of 16 /
4014 of 4022. `file` and R6RS `unicode` are clean; `base` now loads past its includes and stops at a
`syntax-rules` written where `...` is a bound variable (R7RS 4.3.2 — a per-token, scope-aware
decision that belongs in the macro compiler; a whole-macro shortcut was tried and backed out in
review), with a template reference to a keyword-spelled local right behind it.

The ratio flatters; the suite column does not. Of the R7RS suites that are not
clean, 8 never load because the library under test is not bundled —
`ephemeron`, `flonum`/SRFI 144, `ideque`, `ilist`, `list-queue`, `lseq`,
`rlist`, `text` — which is L1's item 6 ("the remaining standard-track set with
little measured demand") acquiring a measured suite each. (`(scheme charset)`
and `(scheme stream)` were two of the ten; both are bundled now.)

**Where the detail lives.** Each lane's report (`scheme_tests/reports/larceny*.md`)
is organised by kind of problem and links every failing assertion to its test
case upstream by permalink — nothing from the suite is quoted. The map from
*defect family* to those links, and to our own original test case for each
family, is `scheme_tests/reports/larceny_triage.md` — a working document,
deleted when the queue is empty. The original cases are
`crates/patina-tests/tests/larceny_families.rs`, pinned so that each fix
trips its test.

**What it found — ours**, in the order they are worth fixing (§6 has the repros):

| Defect | Suites | Backends |
|---|---|---|
| ✅ A nested `include` resolves against the wrong directory, and the base is chosen nondeterministically — *fixed 2026-08-24* | base | both |
| ✅ A shadowed `...` is still the ellipsis, and a template's reference to a keyword-spelled definition-site local is rejected as syntax — *both fixed 2026-08-25*, and with them `let-syntax`'s body and transformer scoping (triage families 14, 15, 23). These **gated `base`**, which now loads and runs 1054 of 1064 on the VM (1053 on the tree-walker, whose one extra failure is a `dynamic-wind` divergence now quarantined as triage family 30). One defect underneath all of them: a local variable was not a binding but a set of *spellings* vetoing syntax resolution, unordered and skipped for any reference carrying scopes. Locals are scoped bindings now and one ordered resolution answers for keywords and variables alike; the ellipsis is decided per token by binding. Two earlier attempts were backed out after review (#111, #114) and their cases are pinned as acceptance tests | base | both |
| ✅ The macro expander's walkers looped forever on a cyclic quoted datum (the scope flip is now a memoized, cycle-closing copy), and the program parser scoped datum labels to the parse rather than the datum — *fixed 2026-08-25* | base | both |
| What `base` found while it briefly ran (triage families 21–25, each pinned): ✅ `let-values` binds sequentially and ✅ `read-line` does not end at a bare return — *both fixed 2026-08-25* (R7RS 7.3's `let-values`; one character loop for `read-line`); ✅ VM: `with-exception-handler` rejects a continuation as handler — *fixed 2026-08-25* (the VM's generic call path can invoke a continuation); ✅ `let-syntax` splices its body's definitions and its transformers see their siblings — *fixed 2026-08-25* with families 14/15; ✅ a `guard` re-raise does not re-enter the dynamic extent — *diagnosed 2026-08-25* to `with-exception-handler` running its handler after the unwind (family 28), **fixed 2026-09-01** in four parts, two of which landed first on their own merits (#150 handler stack, #149 wind common prefix) because the reference `guard` routed ordinary traffic through two defects this entry had called neighbours. `base` goes 1069 → 1071 on the VM and 1068 → 1070 on the tree-walker | base | both |
| Red-edition libraries unbundled, one suite each (nine at the start) — *`(scheme stream)`, `(scheme list-queue)` and `(scheme lseq)` done 2026-08-25* (SRFI 41 from the snow-fort tarball of the reference implementation with `stream-match` from chibi; SRFI 117 and 127 byte-identical from chibi, both plain R7RS over `(srfi 1)` — **superseded 2026-08-26**: chibi's copies carry four defects between them, so both bundles are the SRFIs' own reference implementations; the defects were reported as chibi-scheme #1179/#1180 and fixed upstream 2026-08-27, and `lib/srfi/PROVENANCE.md` records why the bundles stay the reference implementations anyway); ✅ all bundled as of 2026-08-26 — `ideque` (SRFI 134), `flonum` (SRFI 144), `text` (SRFI 135), `ephemeron` (SRFI 124, implemented in Rust: an ephemeron's defining property is what the collector does, so there was nothing to vendor) and `rlist` (SRFI 101, from chibi's R7RS adaptation, the SRFI's own distribution being R6RS). Every one of the 33 suites now loads | 0 suites | both |
| ✅ SRFI 101's shadowing names exposed five hygiene defects (triage families 33–35, each pinned): a `let-syntax` keyword bound unscoped captured any macro's `quote`; the relinker skipped a `quote` head, so a library's template `quote` resolved to the imported one and expanded without end; a literal `'(1 2)` in a template carried its `quote` verbatim; the relinker renamed by spelling, rewriting the user's `(list …)` inside a macro call; and the VM's quasiquote called `list`/`append`/`list->vector` by name — *all fixed 2026-08-26*. Found by making chibi's `(srfi 101 test)` run; it now passes 56 of 56 on both backends. Review of the fix found the next layer, fixed the same day: a `let-syntax` now puts its scope on its body as written (Racket's rule) so a template-generated one binds and scopes its keyword correctly, `quote` inside a quasiquote is data with its unquotes evaluated, and the ellipsis-escape compiler compiles symbols hygienically; plus a use-site binding capturing a template's reference, quarantined as family 36 — measured 2026-08-28 to be wider than first recorded (a parameter or `let` binder captured on the tree-walker only, an *internal define* on **both** backends, the prerequisite for family 38's remaining half; PR #138 attempted that half directly and was closed unmerged). **Fixed 2026-08-31 in the triage doc's two steps**: the by-name fallback no longer resurrects a binding scope resolution rejected, and an internal define binds at the scopes of the body it stands in, like a parameter — the hygiene matrix reads 28 of 28 on both backends and family 38's fallback half closed with it. Step 1's review surfaced family 40 — the VM still resolves a cross-expansion macro-introduced global by bare name where chibi and the tree-walker now refuse; three divergence quarantines pin it, rooted in this section's relinking-by-name defect | — | both (quasiquote: VM; family 36: both) |
| ✅ `equal?` does not terminate on circular structures — *fixed 2026-08-24* (worklist + lazily allocated visited set) | read, r6rs mutable-pairs | both |
| ✅ `delay-force` is not iterative — 100 000 deep overflows the stack — *fixed 2026-08-24* (R7RS 7.3's iterative `force` with `promise-update!`, in the primitive and in the tree-walker's CPS `force`) | lazy | both |
| ✅ VM: a discarded call to `values` poisons the next `call-with-values` — *fixed 2026-08-25* (the values side buffer is gone; multiple values are only ever a `#<values>` object) | vector | VM |
| ✅ Tree-walker: a continuation invoked with other than one value is an arity error; `(values)` reaches a consumer as one unspecified value — *both fixed 2026-08-25*: one `Heap::values_from` now carries the rule all four sites used to spell separately | — | tree-walker |
| ✅ Tree-walker: SRFI 1's `zip` (n-ary `map` via `apply`) raises a wrong-arity error — *fixed 2026-08-25*; the cause was a continuation invoked with two values (`%cars+cdrs`'s `(abort '() '())`), not `apply` | list | tree-walker |
| Tree-walker: a 1.1M-iteration `do` loop building a list overflows the stack | char | tree-walker |
| ✅ Unicode case: `(char-upcase #\ß)` ⇒ `#\S`; `char-ci=?` on final sigma; `string-ci=?` full folding — *fixed 2026-08-24* with the simple mappings (a generated table where std's full mapping expands); `digit-value` over every Nd character still open | char (2), r6rs unicode | both |
| ✅ `string->number`: `"+inf.0"`, `"+nan.0"` ⇒ `#f`; `"1+2i"` ⇒ `#f`; `#e1e1000` ⇒ `+inf.0` instead of an exact integer — *fixed 2026-08-24* (`string->number` is the reader's number syntax; `#e` on a decimal is exact from the text, in the reader too) | inexact, complex | both |
| ✅ `rationalize` with infinities — *fixed 2026-08-24*. `(log -0.0)` and `(sqrt -inf.0)` turned out not to be ours: chibi, Gauche and Chez all answer as Patina does | inexact (1), complex (1) | both |
| `environment` rejects a nested import set, `(prefix (only …) …)` | eval, r6rs eval | both |
| ✅ `input-port-open?` on an output-only port was a type error, not `#f` — *fixed 2026-08-24*; `file` is clean | file | both |
| `write` spells the symbol `@` as `\|@\|` — consistent with reading it bare, but the suite expects `@` | write (3) | both |
| R6RS lane: `(make-bytevector 10 -1)` (a signed fill byte); enum `(color black)` | r6rs bytevectors (4), enums (3) | both |

**Not ours**, recorded so nobody re-diagnoses them: `set-map` — the suite
calls `(set-map proc comparator set)`; SRFI 113's text, chibi and Patina all
have `(set-map comparator proc set)`. `delete-duplicates!` — the suite
requires the result to reuse the input's cells, which SRFI 1 permits but does
not require. `(let-syntax ())` with an empty body, which is what blocks the
R6RS `base` suite — R6RS's splicing `let-syntax` allows it and R7RS's does
not; Gauche and Chez accept it, chibi rejects it as we do, so it is a
leniency decision rather than a defect. Tree-walker `time` — a one-second
busy loop measured at two seconds, i.e. speed.

For calibration: chibi 0.12 fails `base` on the same nested include
(`couldn't open input file: "base-test4.scm"`), and Gauche 0.9.15 rejects
`base.sld` outright at a `syntax-rules` template check, so neither reference
runs the largest suite either; upstream's README records the same for chibi
0.7.3 and Gauche 0.9.5.

## 5. Sequencing within the track
**L0** (edge cases) → **L0.5** (CLI surface) → **L0.75** (survey) → **L3 harness + baseline run** → **L1** (SRFIs: pure-Scheme set first, then primitive-backed) → **L2** (`chibi` libs) → **L3 re-run**, then loop L1/L2 against the refreshed histogram until the curve flattens.

**L5 is not on that loop, and that is the point.** L1/L2 raise the score against
the corpus we have; L5 asks whether that corpus is measuring what the headline
claims. It was added once the loop's own numbers showed the answer was partly no
— 137 of 184 packages checked only that they load, and the behavioural remainder
was 62% chibi — so it runs beside the loop rather than after it.

Note the deliberate departure from numeric order: **L3 is built before L1/L2, not after.** The items are numbered by when they were conceived, not by when they run. Standing the harness up early is cheap once `-A` exists, and it converts the rest of the track from a guessed list of SRFIs into a measured queue — every subsequent port is chosen because it unblocks counted, named libraries. L1's primitive-backed SRFIs (125/143/151/27) can proceed in parallel with the pure-Scheme set.

**Self-containment invariant.** At no point does the track acquire a build-, test-, or CI-time dependency on another Scheme implementation or package manager. The chibi checkout is corpus *data* pinned by commit, not tooling. Anything that would violate this belongs in §3. See `PRD/SNOW_AND_PERF_ROADMAP.md` for the M1–M4 interleave with Track P; note that Track P's GC (P6) is the cross-cutting unblocker that lets L3's real packages run long-running without leaking.

## 6. Known defects surfaced by this track

### Upstream, not ours — the parse-error bucket, audited (2026-08-19)

The audit's follow-up hypothesis read the parse-error histogram as ~4 Patina
macro-defect classes. Running the repros corrected that: **9 of the bucket's
14 rows trace to three upstream defects in `cond-expand` fallback branches
that chibi itself never compiles** — portability shims that, as far as the
corpus can show, no non-chibi implementation ever ran. Each was cross-checked
against chibi 0.12 and Gauche 0.9.15; Gauche rejects all three exactly as
Patina does. Recorded here so no future session re-diagnoses them as ours.

Since 2026-08-22 each of these rows also has a machine-readable home in
`compat/EXCLUSIONS.scm` under `upstream-source-defect`, carrying the same
file, line and cross-check. The prose here is the reasoning; that file is
what the score reads. If they ever disagree, the file is the one a run
acts on — so change both.

- **`(chibi bytevector)` — a dead 4-element rule** (`ieee-754.scm:16`,
  `((_) bv off i)` — a parenthesization typo in `bytes-u8-set-all!`). The
  file sits in the *else* branch; chibi's own branch imports
  `(scheme bytevector)` instead and never reads it. Gauche: "malformed
  macro". Owns 5 rows: chibi-bytevector, chibi-crypto-{md5,rsa,sha2},
  postgresql (all via dependency).
- **`(chibi monad environment)` — a `syntax-rules` with no literals list**
  (`environment.sld:6`, `(syntax-rules ((_ x) 'x))`, the fallback definition
  of `syntax-quote`; chibi's branch imports the real one from `(chibi)`).
  Gauche: "literal list contains non-symbol". Owns 3 rows:
  chibi-monad-environment, chibi-show, chibi-snow-commands (via dependency).
- **`(chibi parse)` — the fallback `grammar-bind` generates an illegal
  macro** (`parse.sld:66`): its inner `new-symbol?`'s pattern embeds the
  use-site expression bound to `name`, so a grammar clause containing
  `,(parse-char (lambda (ch) … ch …))` yields a pattern with `ch` twice.
  Duplicate pattern variables are "an error" (R7RS §4.3.2); chibi tolerates
  them, Gauche rejects ("Pattern variable ch appears more than once …
  new-symbol?") and fails edn end-to-end exactly as Patina does. Owns 1 row:
  edn. ((chibi parse) itself probes clean — the branch only runs at
  `define-grammar` use.)
- **chibi-app — an `else` clause mid-`case`** (`app.scm:467`, followed by a
  `((1) …)` clause). R7RS puts `else` last; Gauche rejects with "'else'
  clause followed by more clauses". Chibi accepts, first-match-wins — which
  makes upstream's trailing clause *dead*, so on chibi the help printer
  treats exactly-one-command like many (a real upstream behavior bug, just a
  quiet one). Declining to imitate is §4 policy; what Patina owes here is a
  better message than "No matching pattern for macro case" with no location.
  Owns 1 row.

The two rows that were genuinely ours: chibi-tar (the empty-body `case`
clause — fixed, see below) and chibi-regexp (open, next entry). The lesson
mirrors L2's cond-expand-no-else finding: **a fallback branch that upstream
never runs is unreviewed code**, and the corpus is the first thing to ever
execute it.

**One more member, from the wrong-result bucket (2026-08-19):** chibi-voting's
`instant-runoff-rank` test expectation depends on hash-table iteration order.
The algorithm walks `(hash-table->alist ...)` and `min-candidate` breaks
count ties by list position, so tied eliminations follow whatever order the
implementation's hash table yields — R7RS-level unspecified. Three
implementations produce three rankings on the suite's fixture (chibi
`(A C D B)` — the baked-in expectation; Gauche `(C A B D)`, failing the suite
exactly as we do; Patina `(A B D C)`). The suite's other failure on that row,
`sort-pairs`, *was* ours — the `list-sort` stability fix below — so
chibi-voting now scores 6/7, identical to Gauche, and the row's residual
failure is upstream's.

### Open

**An exception handler runs after the unwind, not in the raise's dynamic
extent** — ✅ **fixed 2026-09-01**, both backends. Found 2026-08-25 while
attempting Larceny's `guard` re-raise finding (triage family 22, which is this
defect's visible symptom).

```scheme
(with-exception-handler (lambda (e) (log 'handler) 'handled)
  (lambda () (dynamic-wind (lambda () (log 'in))
                           (lambda () (raise-continuable 'x))
                           (lambda () (log 'out)))))
;; was, both backends    => (in out handler out)   — the after-thunk twice
;; now, and chibi/Gauche => (in handler out)
```

R7RS 6.11 calls the handler "in the dynamic environment of the call to
`raise`, except that the current exception handler is the outer one". Both
backends unwound to the handler's own wind depth first, so a `guard` could not
re-enter an extent that was left before its handler ran: nested guards logged
`(out in)` where both references log `(out in out in)`.

**The fix removes work rather than adding it.** A raise crosses no dynamic
extent, so no after-thunk is due: no raise path unwinds now, and popping the
handler stack is the only thing R7RS asks a raise to change. `guard`'s
`guard-k` does the crossing. Four parts, none sufficient alone — two of them
landed first, on their own merits, as **#150** (`CpsContinuation` carries the
handler stack) and **#149** (the VM takes the common prefix of two wind
stacks). What remains here is the raise paths and `guard` itself.

Two things this entry had wrong, both corrected by running it:

- **There were three tree-walker raise paths, not two.** `apply_error`
  (`cps_eval/application.rs`) is the third, and it is what produced the
  `(error "x")` divergence below — alone among the three it never unwound. It
  needed no change: it already had the shape R7RS asks for, and the fix
  brought the other two down to it.
- **It does not reopen the 2026-03-01 decision.**
  `test_dynamic_wind_with_exception` and
  `test_exception_in_dynamic_wind_body_after_runs` give exactly what chibi and
  Gauche give. They state a correct requirement that the correct
  implementation also meets, and both stayed green. The mechanism changed, not
  the decision.

`guard` carries **one deliberate deviation** from R7RS 7.3: the success path
returns its thunk rather than jumping to `guard-k` with it. Equivalent — the
body has returned, so the jump crosses nothing — and required by the
nested-trampoline boundary defect below. Re-measured after #149 and #150: the
verbatim line is now free on the VM but still costs the tree-walker 83
assertions on Larceny's `base`. Restore it when that boundary is fixed.

Larceny `base` goes to 1071 of 1079 on the VM and 1070 of 1079 on the
tree-walker (from 1069 and 1068), the moved rows being this family's. Both
chibi lanes stay 1226/1226. Pinned as
`an_exception_handler_runs_in_the_raises_dynamic_extent`
(`crates/patina-tests/tests/larceny_families.rs`); triage families 22 and 28,
the tree-walker `guard` entry below and `nested_exception_handlers.rs`'s pin
closed with it, as recorded.

**What the second review of #151 found, and where each went.** Four fixed in
the same PR, all older than the family, all made ordinary traffic by the
reference `guard`: (1) the VM popped a `with-exception-handler` handler only
on a closure `Return` — a thunk ending in a tail call to `values`, a
primitive, a parameter or a control primitive left it installed for the rest
of the program, and the reference expansion ends every successful `guard` in
`(apply values args)`, so every one leaked; the first fix for that popped at
a nested run loop's exit depth and lost a *live* handler instead, because
after a tail-call dip a handler the loop was started under and one installed
inside it sit at the same depth — the loop now closes only what was installed
after its own entry (`handlers_at_entry`), and depth-based pops do nothing at
the exit depth. (2) A `call/cc` snapshot clones the register file, and `dst`
still held the previous capture's continuation, so each `guard` iteration
chained to the last — 296 MB at 160k iterations, 21 MB after clearing `dst`
before the snapshot. (3) `call-with-port` and the two `call-with-*-file`
closed their port on an escape; R7RS 6.13.1 closes "if `proc` returns", and
a `guard` clause reading what the callback wrote needs it open — this also
retired audit F6's quarantine on the tree-walker, by removing the
resource rather than the misread behind it. (4) `execute` left the VM holding
the frames, handlers and winds of a form that failed, so the REPL's next form
returned into them. Pinned in `nested_exception_handlers.rs`,
`escape_from_primitive.rs` and `interpreter_api.rs`. Filed rather than fixed:
three VM raise-path gaps (next entry), and two more manifestations of the
tree-walker boundary defect (its entry below).

**The measured cost of the reference expansion**, accepted for consistency
over speed: a `guard` whose body succeeds costs +65% on both backends (3M
iterations, VM 1.15 s → 1.90 s, tree-walker 12.0 s → 19.7 s), a caught raise
+57% on the VM and +33% on the tree-walker (1M, 0.73 s → 1.14 s and
6.3 s → 8.3 s). Every `guard` now captures a full continuation, and on the VM
that clones the register file: non-tail recursion through a `guard` 4000 deep
peaks at 1.83 GB against 1.38 GB before (the tree-walker's CPS capture is
cheap: 111 MB against 312 MB). Per-`guard` memory in a loop is at or below
`main`. A `guard` that captures only what it needs — a lighter continuation
for the success path, or an `else`-only fast path — is the follow-up if a
corpus program shows the cost; none has.

**VM: three gaps in `vm_raise_value`** — ❌ **open**, VM only. Found by the
second review of #151 (2026-09-01); all three predate families 22/28, and the
reference `guard` changed their symptoms without causing them. Pinned in
`crates/patina-tests/tests/backend_divergence.rs` where the tree-walker is
right.

1. **A `guard` is gone after one of its clauses declines a
   `raise-continuable`.** The continuable path pops the handler to run it and
   re-pushes it only when the handler *returns*; `guard`'s handler leaves
   through `handler-k`, so the re-push is skipped and the body continues
   without its `guard`. `(with-exception-handler (lambda (e) (list 'I e))
   (lambda () (guard (e ((eq? e 'y) 'caught-y)) (list (raise-continuable 'x)
   (raise-continuable 'y)))))` — tree-walker, chibi, Gauche `caught-y`; VM
   `((I x) (I y))`. Fix: re-push on the `Escaped` arm too, or have the handler
   run *without* popping and let the raise's handler lookup skip the top one.
2. **A handler that returns from a non-continuable `raise` is not an error.**
   R7RS 6.11 raises a secondary exception; the tree-walker does, the VM
   delivers the handler's value to the raise's destination register as if the
   raise were continuable — `(list (raise 'x))` under `(lambda (e) 'returned)`
   answers `(returned)`, and `(+ 1 (raise 'x))` fails inside `+`. The
   non-continuable path pushes the handler's frame and lets the run loop drive
   it, so nothing is there when it returns; the fix is a marker on that frame
   (or a sentinel return register) that `Return` recognises. Reached from a
   *primitive's* error the same hole is worse: the run loop routes a catchable
   `VmError` through `vm_raise_value` with **register 0** as the destination,
   so `(list (car 5))` under the same handler answers `(())` — the handler's
   value overwrote register 0 and `car`'s own destination kept `()`.
3. **A continuation used as the handler for a primitive's error is fatal** —
   `(call/cc (lambda (k) (with-exception-handler k (lambda () (car 5)))))`
   dies with `continuation escaped past a synchronous boundary`, where the
   same idiom over `(raise 'obj)` works since family 24. The run loop's error
   route does not catch `call_any`'s continuation signal. Tree-walker, chibi
   and Gauche deliver the error object.

None has a Larceny row; all three are pinned.


**Tree-walker: a 1.1M-iteration `do` loop overflows the stack** — ❌
**open**, tree-walker only (the VM runs the same suite in 4 s).
`filter-all-chars` in Larceny's `char.body.scm` is a plain `do` from 0 to
`#x110000` consing matches onto an accumulator — a tail loop with no
recursion in the Scheme. Something under it recurses per iteration on the
tree-walker; the suspects are the collector marking a long continuation
chain or a long list recursively. Reproduce with the suite
(`./scripts/run_larceny_tests.sh --tree-walker char`, ~74 s to the crash)
until a smaller loop is found that does it.

**chibi-regexp: `(regexp 'grapheme)` feeds `#<unspecified>` into the NFA
builder** — ❌ **open**, and the root is architectural. Gauche loads the
library clean; Patina dies at the last form of `regexp.scm`
(`(define re:grapheme (regexp 'grapheme))`) with upstream's own guard,
`(error "expected a state" #<unspecified>)`.

Diagnosis (2026-08-19, by truncation bisection): the grapheme SRE embeds
char-set *objects* from `(chibi char-set boundary)`. That library's
cond-expand prefers `(chibi char-set)` when it resolves — which it does under
the harness, from the vendored corpus — and the vendored `(chibi char-set)`
builds **iset-backed** sets. Patina's `(srfi 14)` char-sets are a different
record type, so in `->rx` (`regexp.scm:761`) the embedded boundary sets
satisfy neither `char-set?` nor any other arm, the `cond` falls off the end,
and `#<unspecified>` flows into `make-state`. Gauche survives because its
`(srfi 14)` is the built-in full-Unicode type and the same fallback keeps
every set homogeneous. Patina cannot take that path today: our `(srfi 14)`
is the Latin-1 reference port, and the boundary data is full-Unicode (hangul
at `#xAC00`, regional indicators at `#x1F1E6`), which the port would refuse
or silently truncate. **Blocked on a full-Unicode char-set story** (SRFI 14
beyond Latin-1, or an iset-compatible representation); not a macro defect.
Two cosmetic defects rode along and are worth fixing sooner: the raised
error displays as `#<unknown>`, and the message doubles its "unhandled
exception:" prefix.

**An imported variable is a stale copy of its binding** — ❌ **open**. Found
2026-08-19 while writing an R6RS library test.

```scheme
(define-library (m counter)
  (export bump count peek)
  (import (scheme base))
  (begin (define count 0)
         (define (bump) (set! count (+ count 1)))
         (define (peek) count)))
(import (m counter))
(bump) (bump)
(list count (peek))
;; Patina (both backends) => (0 2)
;; Gauche, chibi          => (2 2)
```

`import` installs the *value* a variable had when the library was imported
rather than aliasing the library's binding, so a later mutation by the library
is invisible to the importer. The library's own view is live — `peek` reads 2 —
which puts the fault in what `import` installs, not in how `set!` runs.

**The chibi suite is 1226/1226 across it**, because nothing there mutates a
variable another library imported. That is the whole argument for a second
corpus in one example: the defect is not exotic, it is simply outside what one
suite happens to do.

Pinned in `crates/patina-tests/tests/library_loading.rs`. Asserted as-is rather
than via `assert_divergence`, which needs a backend to *fail*: both backends
agree and neither errors, they return a plausible wrong answer. The test says
what to do when it converges.

**VM: invoking a continuation inside its own `dynamic-wind` re-runs the thunks** — ✅ **fixed
2026-09-01**. No primitive involved; the minimal repro is a bare `call/cc`.

```scheme
(dynamic-wind (lambda () (log 'in)) (lambda () (call/cc (lambda (k) (k #f)))) (lambda () (log 'out)))
;; VM => (in out in out)          until 2026-09-01
;; tree-walker, chibi, Gauche => (in out)
```

The VM treated invoking a continuation captured *within* the extent as leaving and re-entering it,
so the after- and before-thunks ran again; R7RS §6.10 only requires them when the dynamic extent is
actually exited and re-entered, which it is not here. `run_wind_transition` forced the common prefix
of the two wind stacks to zero for every full `call/cc` invoke; it takes the real prefix now, keyed
on a new `DynamicWindRecord::id` minted from the counter the tree-walker's record already used.

**Two things the fix turned up, both now pinned in the same test file.** The *value* form of
`dynamic-wind` runs its body on a nested Rust call, so an escape abandons the frame that owns the
cleanup — safe only while every invoke drained the whole stack, and once the prefix is honoured the
after-thunk went from running at the wrong time (`(in out in)`) to never running at all (`(in)`).
It pops and runs its own record now, and the value form matches chibi on all four escape shapes
where main matched on one. And the exit loop recomputes the prefix per iteration: a continuation
invoked from inside an after-thunk replaces `dynamic_winds` wholesale, which a hoisted index would
outlive — unreachable under the forced zero, reachable once the bound can be nonzero.

**Recorded late, and that is the point.** This was visible as the "four `in`/`out` pairs" symptom in
the primitive-escape work, and when those entries were rewritten the standalone repro went with
them — a defect that had been tracked became untracked, and only a review sweep caught it. It was
pinned in `crates/patina-tests/tests/backend_divergence.rs`, which is the mechanism that would have
prevented that: prose in a PRD can be edited away, a failing test cannot. The value-form regression
above makes the same argument twice over — `cargo test` was fully green while the after-thunk leaked,
because the pin covered only head position.

**Still open next door**, and worth naming so nobody assumes identity settled it: `vm_raise_value`,
the two prompt paths (`AbortToPrompt`, `CaptureComposable`) and the value-form arm still locate wind
records by *depth*. A continuation invoke can now replace a stack record-for-record while keeping its
length, so those comparisons rest on an assumption identity was introduced to retire. Composable
invokes also bypass `run_wind_transition` entirely, running every captured `before` thunk.

**VM: a primitive used as a `call-with-values` consumer still mishandles an escaping callback** —
❌ **open**. The last shape of the re-entry class; the rest closed 2026-08-16.

```scheme
(call/cc (lambda (k) (call-with-values (lambda () (values 2 '(1 2 3) (lambda (a b) (k 'x)))) member)))
;; VM => (1 2 3)     tree-walker, chibi, Gauche => x
```

`call-with-values` is a control primitive that pushes no frame, so the consumer runs at the call/cc
lambda's own depth and the continuation restores to exactly that depth. Instrumented: the direct
call reports `before=2 after=1`; this shape reports `before=1 after=1`.

**A previous draft of this entry recorded the wrong remedy.** It said "what is wanted is frame
identity — a per-frame id, or a generation stamped on the frame". Review instrumented every frame
field (`code.id`, `pc`, `register_base`, `num_regs`) across the boundary and found the open case and
the must-not-fire case (`test_a_continuation_used_inside_the_callback_is_not_an_escape`) are
**byte-identical at every field**: the continuation restores a *clone of the very activation* the
primitive is standing on, suspended at the same `pc`. A stamp applied at frame push is cloned along
with the frame and compares equal on both sides, so it cannot separate them. Anyone following that
note would have spent the attempt discovering this.

What does separate them is identity on the **continuation** rather than the frame: a monotone
barrier id pushed at each re-entry boundary, recorded into `VmContinuation` at capture, compared at
invoke — an invocation is an escape iff it was captured under an older barrier. `k2` in the
must-not-fire case is captured *under* the primitive's own barrier and so compares equal.

**Tree-walker: two continuation defects around primitive callbacks** — ❌ **open**. Both found
2026-08-16 while fixing the VM half, and both pinned in
`crates/patina-tests/tests/backend_divergence.rs` so they retire themselves.

```scheme
(member 2 '(1 2 3) (lambda (a b) (call/cc (lambda (k2) (k2 (= a b))))))
;; tree-walker => #f — the callback's value, not the primitive's
;; VM, chibi, Gauche => (2 3)

(call/cc (lambda (k) (set! kk k) (eval '(kk 'from-eval) (interaction-environment)) 'fell-through))
;; tree-walker => escapes *and then* runs the fall-through as well
;; VM, chibi => from-eval
```

`map` with the first shape works on both, so it is specific to a Rust primitive running the
callback. The second is the tree-walker's half of the boundary problem the VM just fixed: it has no
equivalent of `across_reentry`, so an escape out of `eval` or `load` does not abandon the caller —
`(load …)` runs every remaining form in the file.

**Tree-walker: a `guard` handler runs before the unwind** — ✅ **fixed
2026-09-01**. Found 2026-08-15 while moving `with-output-to-file` into Scheme,
by a test whose two backends disagreed.

```scheme
(guard (e (#t (log 'handler)))
  (dynamic-wind (log 'before) (lambda () (error "x")) (log 'after)))
;; VM, chibi, Gauche => (before after handler)
;; tree-walker       => (before handler after)   until 2026-09-01
```

R7RS §4.2.7 evaluates the clauses "in the dynamic environment of the `guard`
expression", so the unwind comes first; the VM was right and both references
agree with it. Not cosmetic: a handler that writes to `current-output-port`
wrote into whatever the not-yet-unwound extent installed, which is exactly how
this surfaced — a handler's output vanished into a port that was about to be
closed.

The cause was `apply_error`, the tree-walker's *third* raise path, which the
entry above had not recorded: alone among the three it did not unwind before
the handler. It turned out to be the one already right, and the fix took the
other two down to it. Converged with triage families 22 and 28; now
`a_guard_clause_runs_after_the_unwind` in
`crates/patina-tests/tests/backend_divergence.rs`, held to one expectation on
both backends.

**Tree-walker: an error inside a wind thunk escapes `guard`** — ❌ **open**. Found 2026-08-15 while
sweeping the class below, and left open deliberately because the fix is one level down from where it
surfaces.

```scheme
(guard (e (#t 'caught))
  (with-exception-handler (lambda (c) 'handled)
    (lambda () (dynamic-wind (lambda () 1) (lambda () (raise 'x)) (lambda () (car 7))))))
;; tree-walker => Error: Type error: car expects a pair   (escapes)
;; VM, Gauche  => caught
```

(Re-measured 2026-09-01: the VM answers `caught`, not `handled` as this block
used to say. The row's substance is unchanged — the tree-walker escapes, the
VM does not — and it is the *mechanism* half of the entry below, which is the
semantics half.)

The visible cause is `run_wind_handlers(…)?` propagating as a Rust error. The actual cause is
`apply_from_direct_tagged` (`cps_eval/wind.rs`): it runs wind thunks and parameter converters on a
*nested trampoline that starts with an empty handler stack*, so anything they raise must come back
through Rust to reach the handlers installed outside. Routing at the `?` is what the
parameter-converter fix below does and it works — the outer frame still has its handlers — but the
general fix is to thread the handler stack into the nested trampoline.

Its sibling row closed on 2026-09-01: `reentered_continuation_keeps_exception_handler` was
`CpsContinuation` not carrying the handler stack, which is a *storage* gap and is fixed by storing
it. **This row is the other half and no field fixes it** — the nested trampoline fabricates an empty
stack because nothing marks which trampoline captured a continuation, so `apply_cps_step` turns
every reified-continuation invoke into an escape and the calling primitive runs its cleanup either
way. The VM's `across_reentry` is the equivalent the tree-walker lacks.

The enabling refactor both reviews converged on independently is a `MachineState` bundling
`cont`/`cont_env`/`prompt_stack`/`dynamic_winds`/`exception_handlers` — the five values threaded
through every `cps_eval` signature, every `StepResult` variant, and the reason for six
`#[allow(clippy::too_many_arguments)]` in the crate. With it, routing becomes one call at the
trampoline instead of ~18 hand-written sites, and the nested trampoline can inherit the stack
instead of fabricating an empty one.

**This row now blocks something concrete, which it did not before.** Until the tree-walker has
`across_reentry`'s equivalent, `guard` cannot use R7RS 7.3's success-path jump — that jump leaves
the body through a continuation on *every* evaluation, so a `guard` inside a `call-with-port`
callback closes the port under the still-running callback. **Measured 2026-09-01, after the two
prerequisite PRs landed:** with the verbatim reference line the VM is unaffected (Larceny `base`
1071 of 1079 either way — #149's wind fix is what made that true), while the tree-walker drops from
1070 of 1079 to 987 of 1009, 13 new failures and 70 assertions that stop running. The VM half is
ready for the reference expansion; this backend is the only thing holding it.
`lib/scheme/base/exceptions.scm` records the deviation and points here.

**Caveat before converging it:** the pinned divergence row records what the VM returns, not an
established correct answer — chibi loops forever on that repro, so no reference could arbitrate.
Establish the right answer first. Pinned in `crates/patina-tests/tests/callability.rs`.

**Two more manifestations, found by the second review of #151 (2026-09-01)**, both pinned in
`backend_divergence.rs`. A `guard` whose clause *declines* inside a primitive's callback loses the
outer `guard`: the reference expansion re-raises through `handler-k`, back inside the callback, and
the nested trampoline there has no handlers, so `(guard (outer (#t …)) (call-with-port p (lambda (p)
(guard (e ((string? e) 'no)) (raise 'sym)))))` dies with `unhandled exception: sym` where the VM,
chibi and Gauche answer `(outer sym)` — on both raise forms; the old expansion re-raised from the
clause side, outside the callback, and happened to work. And a raise inside a callback that *is*
caught outside arrives as the wrong object: the nested trampoline reports it as an "unhandled
exception" error, which the outer trampoline then routes to the `guard` as an error object, so
`(guard (e ((eq? e 'x) 'ok)) (call-with-port p (lambda (p) (raise 'x))))` sees an error object whose
message is `unhandled exception: x` rather than `'x`, and declines. (This one predates the
families: `main` gave the same.) The F6 quarantine — the port closed under the retry loop — retired
with #151, but by `call-with-port` no longer closing on an escape (R7RS 6.13.1), not by the
trampoline telling a local jump from an escape; the misread is still there.

**An exception raised by a `dynamic-wind` after-thunk does not behave like a
`finally`** — ❌ **open**, both backends. Characterised 2026-09-01 while
landing triage families 22 and 28, which changed one of the four cases below
and so forced the question.

Gauche implements Java's `finally` rule exactly, and does so consistently
across every shape. An exception raised by an after-thunk **replaces** the
exception in flight, unwinding **continues** through the outer winds, and the
replacement is delivered to the nearest handler enclosing the `dynamic-wind`.
chibi cannot arbitrate — it overflows the stack on all four probes — so Gauche
is the oracle here, and its self-consistency is the argument for trusting it.

| # | shape | VM | tree-walker | Gauche |
|---|---|---|---|---|
| 1 | `guard` + `raise`, after-thunk raises | uncaught, program stops | escapes | `(one secondary)` |
| 2 | nested guards, inner catches | `(outer secondary)` | escapes | `(inner secondary)` |
| 3 | normal exit, after-thunk raises | ✅ caught | ✅ caught | caught |
| 4 | `call/cc` escape, after-thunk raises | ✅ caught | escapes | caught |

So the VM meets the rule in 2 of 4 and the tree-walker in 1 of 4. **Only case 1
moved with families 22/28**, from `(one primary)` — the after-thunk's exception
silently discarded, which is the one answer the rule most clearly forbids — to
the exception cascading uncaught. Cases 2–4 are unchanged from before that
work, on both backends.

**The unwind itself is not the gap.** The rule also says unwinding *continues*
outward past the thunk that raised, and with a nested wind whose inner
after-thunk raises, Gauche answers `((caught sec2) (in1 in2 out2 out1))` — the
outer thunk still runs. With a single `guard`, ours stops at `sec2` and the
outer thunk never runs (both backends' logs read `in1 in2 out2`, measured
through files the abort cannot swallow) — but that is case 1's consequence: the
handler is gone, nothing catches `sec2`, and an unhandled exception stops the
program where it is raised. Put a handler *outside* and the VM does continue
the unwind: `((outer sec2) (in1 in2 out2 out1))`, the outer thunk run and the
secondary delivered — one handler too far out, which is case 2. (Re-measured
2026-09-01 by review; an earlier version of this paragraph read the
single-guard log as a second gap.) So the fix is about who catches; the
machinery that carries the unwind past a failing thunk is already there and
pinned.

**Why it is not a patch.** Case 1 requires the `guard`'s handler to fire
*twice*: once for the primary, again for the secondary its own escape raised.
`vm_raise_value` pops a handler when it fires and never restores it for a
non-continuable raise, so by the time the after-thunk runs that handler is
gone. Meeting the rule means changing how the handler stack behaves across a
`guard` escape — the same machinery that produced three separate regressions
during families 22/28 (a leaked after-thunk in the value form of
`dynamic-wind`, a stranded caller premise, and this row's case 1). It earns
its own change, with its own review.

The tree-walker additionally needs the nested-trampoline boundary above: it
escapes on three of the four because a wind thunk runs on a trampoline with no
handler stack.

**Acceptance:** the four cases above answer as Gauche does, on both backends,
pinned in `crates/patina-tests/tests/wind_thunk_exceptions.rs` — which asserts
today's answers now, so the fix trips it. Larceny r7rs lanes must not move
except where a row is genuinely gained.

**An identifier swallows `'`, `` ` ``, `,` and `[` instead of ending at them** — ❌ **open**.
Pre-existing; surfaced 2026-08-16 by review of the Unicode-identifier change, which routes many
more tokens through `read_identifier` and so makes the stop set matter more.

```scheme
(length '(a'b))   ;; Patina => 1   chibi, Gauche => 2
(length '(a,b))   ;; Patina => 1   chibi, Gauche => 2
```

Patina ends a token only at whitespace, `(`, `)`, `"` or `;`. R7RS 7.1.1's `<delimiter>` also lists
`|`, and both references additionally stop at `'`, `` ` `` and `,`. The sharpest case is `[`: a
*leading* `[` is a deliberate `ReservedCharacter` error, but `a[b]` swallows it silently, because
the reserved-bracket rule only applies at token start.

**Left open deliberately, and the reason is worth keeping.** Widening a delimiter set can only
*split* tokens that used to be whole, which is the one kind of lexer change that can alter the
meaning of a program that already works — unlike every widening this track has taken so far, which
only accepted previously-rejected text. So it needs its own decision and its own cross-check rather
than riding along. The set now lives in one function (`Lexer::is_delimiter`) instead of seven
inline copies, which is what makes that a one-line decision when someone takes it.

**The backends disagree on renaming core syntax at import** — ❌ **open**. Found 2026-08-16 by
review of the export-side fix, and pre-existing: that fix touched the export path and `(only …)`,
not this.

```scheme
;; inside a library: (import (rename (scheme base) (begin blk)))
;; VM          => loads; `blk` is then unusable
;; tree-walker => Parse error: Identifier 'begin' not found for rename
;; chibi, Gauche => `(blk 1 2)` evaluates to 2
```

At *top* level both backends agree and neither binds `blk`, so only the library-internal path
diverges. Note where the references land: chibi and Gauche make the rename **work**, so neither
of our answers is right, and "make the two agree by rejecting" would pick the wrong one. Making it
work needs the new name to be recognized as syntax at the use site, which is the binding-based
design described in the fixed-defects archive — so this is best fixed by that work rather than
locally. Not pinned as a divergence yet for the reason §6 records elsewhere: establish the correct
answer first, and here the correct answer is a design decision, not an observation.

**That decision is now taken:** `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md` (2026-08-16) works
the design out and stages it into two PRs. It also records five sibling symptoms of the same root
cause, found while costing it and each verified against chibi and Gauche — a top-level `define`
cannot shadow core syntax while a `define-syntax` can, `except` does not except, `prefix` binds the
prefixed name nowhere while leaving the bare one working, `(null-environment 5)` still has
`cond-expand`, and `(list else)` returns a symbol because of the `base.sld` workaround. They are
kept there rather than repeated here: they are one defect, and it now has one document.

**Definition-env relinking rewrites by name** — ❌ **open, VM only since the 2026-08/09 hygiene arc** (re-measured 2026-09-01; it was both backends when recorded). This is the root of triage family 40, whose three `assert_divergence` quarantines pin the class; the fix route recorded there is scoped relinking (Track Q's Q7.5(b)) or the resolve-once design in `PRD/macro/SYNTAX_CASE_DESIGN.md`. No longer blocked on the quasiquoted-vector entry — that one is fixed (see the Fixed table).

*Two symptoms recorded 2026-08-23 while reviewing the VM hygiene work*, both
the bare name collapsing an identity the rest of the pipeline keeps distinct.
When recorded they reproduced on both backends; **re-measured 2026-09-01 the
tree-walker now answers both as chibi and Gauche do** (10 20, and 10) — the
scoped-define and fallback fixes reached them — and the wrong answers below
are the VM's alone, produced by its compiler's bare-name alias:

```scheme
(define-syntax jab
  (syntax-rules ()
    ((_ h v) (begin (define mh v) (define-syntax h (syntax-rules () ((_) mh)))))))

(jab get1 10) (jab get2 20) (list (get1) (get2))
;; Patina (20 20) · chibi, Gauche (10 20)   — two expansions share one binding

(jab get 10) (define mh 99) (get)
;; Patina 99 · chibi, Gauche 10             — a later user global steals it
```

The VM now gives the two `mh`s genuinely distinct globals, and the tree-walker
gives them distinct scoped bindings — the collapse is entirely in the alias
that answers the *bare* name, because that is what relinking asks for. The
second shape is the same mechanism read the other way: `get` consults real
bindings before aliases, which is what stops a macro's temporary overwriting a
user's global, and is therefore also what lets a user's global capture the
macro's. Fixing either means giving the relinker the scope set it already has
in hand — the entry above — not adjusting the alias.
`link_definition_env_refs` / `rewrite_refs` (`patina-frontend/src/desugarer/mod.rs`) collect
`template_symbols: HashSet<Rc<str>>` and then rewrite every occurrence of those *names* in the
expanded output. By expansion time that tree also holds use-site material substituted from pattern
variables, which can be spelled like a template symbol without being one:

```scheme
;; library (t m): helper = 'from-definition, and (mac x) => (list helper x)
(define helper 'from-use-site)
(mac helper)
;; Chez, Gauche => (from-definition from-use-site)
;; Patina       => (from-definition from-definition)   ; the caller's argument was captured
```

✅ *This half is fixed* — the flip-scope discriminator described below landed with triage family 35
(2026-08-26: the relinker renames only identifiers carrying the expansion's fresh scope), and the
repro answers `(from-definition from-use-site)` on both backends, verified 2026-09-01. The earlier
attempt had been backed out over the quasiquoted-vector gap, which families 33-35 also closed. What
remains of this entry is the *alias* half above: the bare name the relinked definition is reachable
by, which is the VM-only family-40 class.

### Fixed

Full write-ups moved to `PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md` (2026-08-15) — this section had
grown to two thirds of the document. Each entry there keeps its repro, its wrong first diagnosis
where there was one, and the guard test that retires it.

| Defect | Fixed |
|---|---|
| Hygiene was not applied inside a quasiquoted vector (captured in both directions) | 2026-08-26, verified 2026-09-01 |
| The relinker captured pattern-variable material spelled like a template symbol | 2026-08-26 (family 35) |
| Tree-walker: SRFI 1's n-ary procedures raised a wrong-arity error (`zip`) | 2026-08-25 |
| VM: a discarded call to `values` poisoned the next `call-with-values` | 2026-08-25 |
| A nested `include` resolved against the wrong directory, nondeterministically | 2026-08-24 |
| `equal?` did not terminate on circular structures | 2026-08-24 |
| `delay-force` was not iterative (100 000 deep overflowed the stack) | 2026-08-24 |
| `mark_substituted_tagged` treated a tail as a form (fixed by shape; no repro ever constructed) | 2026-08-18 |
| VM: a raising parameter converter produced no output at all | 2026-08-18 (by #73/#77) |
| A recursive macro's per-expansion *definitions* collapsed onto one binding | 2026-08-23 |
| A generated macro's template collapsed identifiers from different expansions | 2026-08-20 |
| `read-line` rejected chibi's max-chars argument; textual reads rejected binary ports | 2026-08-19 |
| `list-sort` reversed ties (reference heap sort; both references are stable) | 2026-08-19 |
| `case` rejected a clause with an empty body | 2026-08-19 |
| SRFI 14 `ucs-range->char-set` discarded its base set | 2026-08-19 |
| `(scheme r5rs)` did not export the R5RS syntax keywords | 2026-08-19 |
| The lexer rejected a non-ASCII identifier | 2026-08-16 |
| A library could not re-export a core syntactic keyword | 2026-08-16 |
| VM: an escape out of a re-entrant primitive crashed the process (call position) | 2026-08-15 |
| VM: an escaped-from primitive kept running, and `apply`/value-position escapes were wrong | 2026-08-16 |
| VM: escapes out of `eval`, `load` and a `parameterize` converter were unhandled | 2026-08-16 |
| `with-output-to-file` crashed the VM on an escape | 2026-08-15 |
| Tree-walker: the control primitives' own errors escaped `guard` | 2026-08-15 |
| `make-parameter` objects were not procedures | 2026-08-15 |
| Tree-walker: an unbound variable escaped `guard` in most positions | 2026-08-15 |
| Rust registry primitives ignored the import set at top level | 2026-08-15 |
| The standard port procedures were not parameter objects | 2026-08-15 |
| Relinking stopped at a `quote` *argument* | 2026-08-14 |
| Literal matching had no "both unbound" case | 2026-08-14 |
| Identifier identity was decided by name inside a macro that writes a macro | 2026-08-14 |
| Recursive macros could not introduce a fresh binding per expansion | 2026-08-14 |
| Tree-walker: nested `guard` across a procedure boundary | earlier in track |
| Bare `@` rejected as an identifier | 2026-08-14 |

The lesson they share, and the reason the archive is worth keeping: **run the recorded repro before
designing against it.** Three of these were filed with a cause that was not the cause — the ports
"needs more than a second arity" (it did not), the parameter dispatch "exists because `procedure?`
fails" (it does not), `with-output-to-file` "does not restore" (it restored; the VM crashed) — and
one of them shipped a test that passed against unfixed code.

## 7. Risks & mitigations
- **Per-new-SRFI friction** (non-R7RS constructs, R5RS naming, control-op edges) → apply the resolved patterns in `PRD/phase2/archive/SRFI_PORTING_ISSUES.md`; import each reference implementation incrementally with its own test.
- **The pass rate has a ceiling well below 100%** — a meaningful share of Snow packages import `(chibi ast)` or other C-backed/FFI libraries and can never pass while FFI is deferred. → Have the L3 harness classify these as *out-of-scope* rather than *failing*, and report the achievable denominator alongside the raw one, so the number is not misread as a defect count.
- **Coupling the track to another implementation's tooling** — depending on `snow-chibi` would put an interactive tool and chibi's release cadence on our critical path, and a fork of its implementation tables would need perpetual rebasing. → Avoided by construction: the harness depends on the snowball / `package.scm` / repo-index *formats*, which are stable s-expressions, not on chibi's code. See the self-containment invariant in §5.
- **Corpus rot** — pinned URLs 404, upstream repos move, checksums drift. → Pin by commit/sha and verify on fetch; a fetch failure must be reported as a distinct `unavailable` bucket, never silently folded into `fail`, so the score stays honest.
- **Network flakiness making CI unreliable** → only tier 0 (vendored, offline) gates ordinary CI; tiers 1–2 run nightly where a transient failure is visible but not blocking.
- **Lenient unknown-clause handling hiding real errors** → gate behind a strictness flag and emit a visible warning, not silent skip.
- **New filesystem/process primitives** → implement behind the `FileSystem` trait for testability and future WASM.
- **Stale tracking docs** → reconcile `PRD/phase2/R7RS_LARGE_STATUS.md` (says SRFIs "not started") with `PRD/PARALLEL_TRACKS.md` (9 done) when updating SRFI status.

## 8. Verification (track-wide)
- Routine: `cargo build --release && ./scripts/run_chibi_tests.sh` (must stay 1226/1226) after every item.
  `./scripts/run_chibi_tests_tree_walker.sh` must likewise stay 1226/1226.
- Per-library: a focused integration test exercising headline forms.
- End-to-end: `cargo run -p patina-compat -- run` — the "N of M" number is the track's headline metric and is expected to move monotonically upward.
- Self-containment: the harness must produce that number on a machine with **no other Scheme installed** and, for tier 0, **no network**. This is a standing acceptance criterion, not a one-time check.
- Quality gate: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt`.
