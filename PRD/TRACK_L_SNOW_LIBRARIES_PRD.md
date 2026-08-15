# Track L — Third-Party Library Compatibility PRD

**Created:** 2026-06-20
**Updated:** 2026-08-08 — corpus vendored (197 packages, `compat/vendor/`); L1 rescoped to the R7RS-large bundling policy and ordered by measured in-degree. Earlier: reframed from "be a Snow target" to **self-contained compatibility coverage**; verified the loading machinery end-to-end; established that no chibi fork, upstream PR, or external package manager is required; promoted the harness (L3) to the centrepiece.
**Status:** In execution — L0, L0.5, L0.75, L4 done; L3 harness live with a measured baseline
(**138 of 184** vendored packages pass, 2026-08-15); L1/L2 continue against the measured queue
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

**SRFI 130 + SRFI 14 landed 2026-08-14, and the queue they came off is now flat.** SRFI 130 (Red
edition, cursor-based strings) reached in-degree 6 after the `@` fix and led the table; SRFI 14 came
with it, since `(chibi string)` — which SRFI 130 is written against — imports it for exactly two
names. Both are byte-identical snowball imports; the chain and its one deviation are recorded in
`lib/srfi/PROVENANCE.md`, `lib/chibi/PROVENANCE.md` and `lib/srfi/130.sld`'s header.

The result reorders what is left. **No missing library is now worth more than two packages**, and the
one at three is `(chibi)`, chibi's implementation core, which is permanently out of reach. Bundling
was the cheap lever for most of this track; it is close to spent. The remaining corpus failures are
concentrated in the parse-error and load-error buckets — defects, not absent libraries — so the
queue below should be read as a completeness exercise against the policy rather than as the ordered
route to a higher score.

Priority order, highest value first:
1. **Bitwise: SRFI 151, plus `(srfi 60)` / `(srfi 33)` shims.** Standard-track *and* the largest
   ecosystem gap — 31 packages import `(srfi 60)`, 19 import `(srfi 33)`, and R7RS-large names 151.
   Shipping 151 alone leaves all of them failing. Needs Rust primitives; portable Scheme would be
   unusably slow.
2. **`(scheme …)` alias libraries** for the six Red-edition SRFIs already shipped (1, 111, 113, 128,
   132, 133) and for 158. A `.sld` re-export each — the cheapest R7RS-large progress available.
3. **SRFI 125** hash tables, superseding the shipped SRFI 69 (in-degree 16); keep 69 as an alias.
   Needs a `HeapObjectData::HashMap` variant.
4. **SRFI 27** random — impossible without an RNG primitive; in-degree 9.
5. **SRFI 143** fixnums — must match the VM's actual fixnum width.
6. ~~**SRFI 14** char-sets (in-degree 4)~~ — ✅ **done 2026-08-14**, pulled forward by SRFI 130.
   Then the remaining standard-track set with little measured demand: Red 41, 101, 116, 117, 124,
   127, 134, 135 and Tangerine 146, 159, 160.
8. ✅ **SRFI 130** cursor-based strings — **done 2026-08-14**, out of numeric order because the
   measured queue put it first. Red edition, and the top missing library at in-degree 6.
7. **SRFI 115** regex last — large, and only if the corpus justifies it.

Near-free re-export shims to do alongside, since R7RS base already provides the functionality but
packages import them by SRFI name: `(srfi 9)`, `(srfi 11)`, `(srfi 39)`, `(srfi 6)`.

*Note:* SRFI 64 is lower priority than its ubiquity elsewhere suggests — Snow packages overwhelmingly
test with `(chibi test)`, which Patina **already ships**. Primitive-backed work goes under
`crates/patina-runtime/src/stdlib/internal_*.rs`, registered in *both* the primitive registry and the
library builder; aligns with `PRD/PARALLEL_TRACKS.md` Track B3.
- **Porting patterns to reapply** (from `PRD/phase2/archive/SRFI_PORTING_ISSUES.md`): import `(scheme r5rs)` for R5RS naming (`exact->inexact` etc.); shim `:optional`/`let-optionals`/`receive`/`check-arg`; treat form-feed as whitespace (already fixed); defer arity rejection so `guard` can catch `apply` errors (already fixed); watch the VM control-op edge cases in `PRD/phase2/INSTRUCTION_LEVEL_CONTROL_OPS.md`.
- **Acceptance:** one integration test per SRFI exercising its headline forms; `./scripts/run_chibi_tests.sh` stays 1226/1226.

### L2 — Bundle the common `(chibi …)` libraries
Third-party packages frequently `(import (chibi …))`; today only `(chibi test)` exists. Port from the pinned chibi checkout the harness already fetches (locally mirrored at `~/Project/reference/chibi-scheme`), in the order L0.75/L3 dictate. Each ported library also brings its upstream `*-test.sld` suite, which drops straight into the L3 corpus:
- `(chibi match)`, `(chibi optional)` — pure Scheme, pervasive in chibi-authored packages, and `(chibi optional)` also retires the ad-hoc `:optional` / `let-optionals` shims the existing SRFI ports carry. Highest leverage in this group.
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
remaining failure is `match-letrec` (§6). Worth noting for reading the number: the corpus counts
packages, not tests, so it under-reports exactly this kind of progress.

**Ports became parameter objects, 2026-08-15 (137 → 138 of 184).** lassik-dockerfile was the corpus's
only `runtime-error` and now passes: it redirects output with
`(parameterize ((current-output-port …)) …)`, which until now was rejected outright. A conformance
fix reaching a package nobody had connected to it is the argument for fixing the standard rather
than the symptom — the queue had this filed as an unexamined runtime error, not as a missing
parameter object.

**Recorded debt — `report.md` is written by shell redirect.** `run` and `report` print the rendered
matrix to stdout and only `results.scm` is written to disk, so the committed
`compat/reports/report.md` goes stale unless the caller redirects into it. Either write both
artifacts, or stop committing the rendered copy.

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

## 5. Sequencing within the track
**L0** (edge cases) → **L0.5** (CLI surface) → **L0.75** (survey) → **L3 harness + baseline run** → **L1** (SRFIs: pure-Scheme set first, then primitive-backed) → **L2** (`chibi` libs) → **L3 re-run**, then loop L1/L2 against the refreshed histogram until the curve flattens.

Note the deliberate departure from numeric order: **L3 is built before L1/L2, not after.** The items are numbered by when they were conceived, not by when they run. Standing the harness up early is cheap once `-A` exists, and it converts the rest of the track from a guessed list of SRFIs into a measured queue — every subsequent port is chosen because it unblocks counted, named libraries. L1's primitive-backed SRFIs (125/143/151/27) can proceed in parallel with the pure-Scheme set.

**Self-containment invariant.** At no point does the track acquire a build-, test-, or CI-time dependency on another Scheme implementation or package manager. The chibi checkout is corpus *data* pinned by commit, not tooling. Anything that would violate this belongs in §3. See `PRD/SNOW_AND_PERF_ROADMAP.md` for the M1–M4 interleave with Track P; note that Track P's GC (P6) is the cross-cutting unblocker that lets L3's real packages run long-running without leaking.

## 6. Known defects surfaced by this track

### Open

**`with-output-to-file` does not restore through `dynamic-wind`** — ❌ **open**. Both backends.
`with_output_to_file` / `with_input_from_file` (`patina-primitives/src/primitives/io/file.rs`) save
the current port, call the thunk, and restore afterwards in straight-line Rust. That covers a normal
return and an error, but not a `call/cc` escape out of the thunk: the unwind skips the restore and
the process is left writing into a closed file port. `parameterize` gets this right via
`dynamic-wind`, and now that the ports are parameter objects these two could simply be expressed in
terms of it — one mechanism instead of two, with the escape case handled by construction. Not folded
into the parameter-object change because it is a behaviour change to a different file.

**The same tail-is-not-a-form defect is still live in `mark_substituted_tagged`** — ❌ **open**.
Found 2026-08-14 by auditing the *class* behind the `quote`-argument fix below, which is the practice
this section already follows. `mark_substituted_tagged`
(`patina-macros/src/macro_expander/expander/hygiene.rs`) walks a pair tree by recursing on the cdr
and re-reading its head, exactly as `rewrite_refs` did:

```rust
if self.is_macro_definition_tagged(car) || self.is_quote_form_tagged(car) {
    return tv;                       // correct for a form, wrong for a tail
}
let new_car = self.mark_substituted_tagged(car);
let new_cdr = self.mark_substituted_tagged(cdr);   // cdr re-read as a form
```

So for a substituted value shaped like `(f quote y)`, the recursive call on the tail `(quote y)`
sees a quote head and returns unchanged — and `y`, plus everything after it, never receives the
macro scope. That function exists precisely so substituted identifiers can be told apart from a
nested macro's own pattern variables, which is the mechanism behind two already-fixed entries in
this section, so losing the mark is a hygiene defect rather than a cosmetic one.

**Honest limit on this entry:** the code shape is confirmed identical, but no observable repro has
been constructed yet. Reaching it needs a single pattern variable bound to a list with `quote`,
`syntax-rules`, `define-syntax`, `let-syntax` or `letrec-syntax` in non-initial position, whose
later elements then matter to an inner macro's identity comparisons — plausible from
`(chibi parse)`-style macro-writing-macro code but not yet exhibited. Do not close it as theoretical
without trying; do not report it as user-visible without a repro.

The durable fix is the one the sibling walkers already use: `compile_template` and
`compile_template_escaped` flatten the spine once via `collect_list_items` and index element 0,
which is what `rewrite_form` converged on independently. `mark_substituted_tagged` is the one that
still hand-rolls car/cdr recursion. Checked and clean: `stamp_expansion_source`,
`contains_identifier_tagged`, `flip_scope_on_tagged_impl`, `strip_identifiers_impl` and
`evaluate_feature_requirement_tagged` — the first four recurse uniformly with no head dispatch at
all, and the last flattens before dispatching.

**`match-letrec` does not match** — ❌ **open**. Both backends. The one remaining failure in
`(chibi match)`'s suite (74 of 75) after the relinking fix below, and the reason chibi-match scores
`wrong-result` rather than `pass`. Every `match-letrec` fails, including the simplest one, while the
neighbouring `match-let` works:

```scheme
(match-letrec (((x y) (list 1 2))) (list x y))
;; chibi => (1 2)
;; Patina => Error: type error: error: first argument must be a string
```

**Read that error carefully — it is not the failure.** The match fails, and chibi's failure branch
is `(error 'match "no matching pattern")` (`match.scm:327`), whose first argument is a *symbol*.
R7RS 6.11 gives `error` a string message, so Patina rejects the call and reports a type error about
`error` itself, burying the real problem. Chez and Gauche accept a symbol there, so this is chibi
leniency of the kind §4 already declines to imitate — but it is the second time in this track that
a lenient `error` has hidden the defect underneath it, which is worth weighing if the question is
ever reopened. Diagnosing this one means neutralising that call first.

**Hygiene is not applied inside a quasiquoted vector** — ❌ **open**. Both backends. Six lines, and
it captures in *both* directions at once — the template's own binding and the caller's argument each
resolve to the other:

```scheme
(define tmp 'use-site)
(define-syntax g1 (syntax-rules () ((_)   (let ((tmp 'introduced)) `#(,tmp)))))
(define-syntax g2 (syntax-rules () ((_ e) (let ((tmp 'introduced)) `#(,e)))))
(list (g1) (g2 tmp))
;; Chez, Gauche => (#(introduced) #(use-site))
;; Patina       => (#(use-site)   #(introduced))
```

The list equivalents (`` `(,tmp) ``) are correct, so this is specific to vectors. The obvious
suspect is `flip_scope_on_tagged_impl` in `patina-macros/src/macro_expander/mod.rs`, which ends
"All other values (vectors, etc.) pass through unchanged" while its own doc comment two screens
earlier claims it "traverses the heap structure (pairs, **vectors**, identifiers)". The doc and the
code disagree, and `contains_identifier_tagged` has the same gap — so its early exit can skip the
flip for a tree whose only identifiers sit inside a vector. `rewrite_refs` in the desugarer already
fixed exactly this oversight for itself ("A pair-only walk silently left the reference unlinked").

**Do not treat that as the diagnosis.** Teaching both functions to descend into vectors did *not*
change the behaviour, and instrumentation showed the flip never runs on that expansion at all —
neither the vector arm nor the pass-through tail was reached. So something upstream is also not
producing the identifiers one would expect. Start by confirming what the expanded output actually
contains before changing the flip.

**Definition-env relinking rewrites by name** — ❌ **open**, and blocked on the entry above.
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

The template's `helper` should resolve to the definition environment; the one the *caller* passed
must not. Template-local bindings are already handled correctly (a template that says
`(let ((helper …)) …)` works), so this is specifically pattern-variable material colliding with a
template symbol name.

The fix needs a way to tell template-emitted identifiers from substituted ones. The natural
discriminator is the flip-scope invariant — introduced identifiers carry the expansion's macro
scope, substituted ones had it flipped back off. That was implemented and **backed out**: it fixes
this capture but breaks `test_quasiquoted_vector_elements_are_rewritten`, because template material
inside a quasiquoted vector also comes out with empty scopes. That is the same vector gap as the
entry above, which is why this is blocked on it rather than independently fixable.

If the vector work does not restore the invariant, the deeper fix is to decide at template-expansion
time instead, where `Template::Symbol` and `Template::Var` distinguish the two without any proxy.

### Fixed

**Tree-walker: the control primitives' own errors escaped `guard`** — ✅ **mostly fixed**
(2026-08-15); one case remains open, see the end of this entry.
The last of the class fixed in #71: a catchable error returned as a Rust `Err` instead of routed
through the Scheme exception handlers. That fix swept `step.rs`; these lived in
`cps_eval/application.rs`, which it never reached — so on the tree-walker `guard` could not catch a
control primitive's *own* arity or type error, while the VM caught all of them.

**The references decided which backend was right.** "Match the other backend" is not a reason to
believe the other backend, so all six positions were run against chibi, Gauche and Chez first:

| probe | chibi | Gauche | Chez | Patina VM | Patina tw (before) |
|---|---|---|---|---|---|
| `(with-exception-handler 5 (lambda () 'ok))` | `ok` — no check | `ok` — no check | caught | caught | escaped |
| `(dynamic-wind (lambda () 1))` | caught | caught | caught | caught | escaped |
| `(call-with-values (lambda () 1))` | caught | caught | caught | caught | escaped |
| `(raise)` | caught | caught | caught | caught | escaped |
| `(error)` | caught | caught | caught | caught | escaped |
| `((make-parameter 1) 1 2 3)` | `#<undef>` | caught | caught | caught | escaped |

Where the references disagree it is about whether to raise **at all** — chibi and Gauche accept a
non-procedure handler and just run the thunk, and chibi returns `#<undef>` for a parameter called
with three arguments, which R7RS §4.2.6 makes explicitly implementation-dependent. None of them
disagrees about catchability *once something is raised*. The old tree-walker behaviour matched no
implementation in any of the six, which is the cleanest mandate this track has had.

**Done as a sweep, not as six patches** — #71's lesson was that fixing reported instances leaves the
class alive. But the first attempt at that sweep defined the class *syntactically* — "every bare
`return Err(EvalError::…)` in the file" — and review caught that this is not the same set as "every
catchable error that can reach Scheme". `application.rs` has five `?`-propagation sites and the grep
audited none of them. Three were live escapes, two of them inside functions the sweep had already
edited:

- `(error 5)` — the message type check, fifteen lines below the arity check that *was* routed, in
  the same function. Fixed here; chibi, Gauche and the VM all catch it.
- a parameter converter that raises — `apply_from_direct_tagged(conv, …)?` in `apply_parameter`,
  thirteen lines above the arity arm that was routed. Fixed here; chibi and Gauche catch it.
- an error raised inside a `dynamic-wind` thunk — `run_wind_handlers(…)?`. **Not fixed**, see below.

Two sites are deliberately left propagating, both checked rather than assumed: `apply_other_primitive`'s
"called with non-primitive procedure" is statically unreachable (its only caller has already matched
`Procedure::Primitive`), and `eval_primop`'s arity checks have no continuation in scope but are
already routed by their caller — the `try_catchable!` arm #71 added to `step.rs`. Review also found
the first of those was `TypeError`, which `is_catchable()` treats as a *user* condition; it is now
`InternalError`, so the code and the rationale agree.

One site needed more than a mechanical rewrite. `apply_with_exception_handler` decided its type
error inside a live `heap.borrow()`, and the router allocates the exception object
(`heap().borrow_mut().alloc_exception`), so routing in place would have panicked on the RefCell.
The check now yields the message and the borrow ends before the routing call — the
extract-to-a-`let`-first discipline in `CLAUDE.md`. The same shape recurred in `apply_error`.

**Narrowed, not closed — and the remaining depth is one level down.** An error raised by user code
inside a wind thunk still escapes `guard` on the tree-walker. The `?` is only where it surfaces:
`apply_from_direct_tagged` (`cps_eval/wind.rs`) runs wind thunks and parameter converters on a
*nested trampoline that starts with an empty handler stack*, so anything they raise has to come back
through Rust to reach the handlers installed outside. Routing at the `?` works for the converter
case because the outer frame still has its handlers, but the general fix is to thread the handler
stack into the nested trampoline — the same shape as the already-quarantined
`reentered_continuation_keeps_exception_handler`, where `CpsContinuation` does not carry them
either. Pinned as a self-retiring divergence.

**Also recorded, VM side, pre-existing:** `(guard (e (#t 'caught)) (p 9))` where `p`'s converter
raises produces *no output at all* on the VM (exit 0, nothing written), rather than an error or a
catch. Unrelated to this change — the VM's parameter path is untouched here — but found by the same
probe and worth a look before anyone trusts converter errors on either backend.

Guarded by `crates/patina-tests/tests/callability.rs`, where every body is asserted twice — caught
when guarded, still an error when not — so a future change that *swallowed* errors instead of
routing them would fail. The divergence row pinned when this was found retired itself on the fix,
printing "NO LONGER DIVERGES"; a new row now pins the wind-thunk case that remains.

**A caveat on that new row.** Its pinned value is what the VM returns, not an established correct
answer: chibi loops forever on the repro, so no reference could arbitrate. It asserts only that the
two backends disagree. The same caveat applies to the continuation-arity site routed here —
`(call/cc (lambda (k) (k 1 2)))` now yields a catchable error on the tree-walker where Gauche and
the VM return `1`, which is the known multi-value-continuation gap, not something this change
settled.

**`make-parameter` objects were not procedures** — ✅ **fixed** (2026-08-15). Both backends. Found
while checking whether the three standard ports had become *real* parameter objects or only ones
that satisfy our particular `parameterize` macro. They had, and the check turned up the mirror gap:

```scheme
(procedure? current-output-port)   ;; => #t   (correct)
(procedure? (make-parameter 1))    ;; => #f   (wrong)
```

R7RS §4.2.6 defines `make-parameter` as returning "a newly allocated parameter object, **which is a
procedure** that accepts zero arguments and returns the value associated with the parameter object";
Gauche agrees. (§7.3's sample implementation builds one out of a `lambda`, which is the same answer
from the other direction. An earlier draft of this entry cited §7.3 as the *definition* and
paraphrased it as "a newly allocated procedure" inside quotation marks — both wrong, and the sort of
thing that survives because nobody re-opens the spec.)

Only the *predicate* was wrong — `(p)`, `(apply p '())` and passing `p` through `map` all worked
before the fix, which is the shape of the defect: `Heap::is_procedure` enumerated callable variants
(closure tag, native `Procedure`, VM closure, VM continuation refs) and omitted `Parameter`.

The fix is one variant added to that enumeration, not a special case in `procedure?`, because the
other callers mean the same thing by the question. Observably that changed three decision points:
`with-exception-handler`'s argument checks on *each* backend (both rejected a parameter before the
fix — verified by running it), and `make-parameter`'s own converter check, which had worked around
the gap with `is_procedure(c) || is_parameter(c)`; that `||` is now redundant and gone. Adjacent,
and fixed alongside because the two are one thought: a parameter fell through the datum writer's
chain to `#<unknown>`, harmless until `procedure?` began answering `#t` about it.

**Two counting errors, caught by review and corrected rather than quietly dropped.** A draft of this
entry claimed eight sites "became correct together", listing `dynamic-wind` and `force` among them.
Checked one at a time: `dynamic-wind` type-checks nothing on either backend — neither
`apply_dynamic_wind` nor `VmControlPrimitive::DynamicWind` inspects its arguments, they just call
the thunks — so it accepted a parameter all along; the `patina-primitives` `with_exception_handler`
is shadowed on the normal call path by each backend's own control primitive, and reachable only
through `apply`, where its two checks do run but the body then returns `InternalError("not yet
implemented")` regardless, so they can never turn a rejection into a success; and `force`'s thunk
check is reachable only for a `Delayed` promise, which `make-promise` does not build. Counting call
sites in a grep is not the same as counting decisions, and the test that was going to guard
`dynamic-wind` would have passed against unfixed code.

**The previous entry's own claim was also wrong.** It held that the bespoke per-backend dispatch
exists *because* parameters fail `is_procedure`, so fixing the predicate would remove the need for
it. Both backends route on `get_parameter` (`try_call_parameter` in the VM, the `param_opt` branch
in the tree-walker's `application.rs`), never on `is_procedure`, and that dispatch exists because
reading and setting a parameter is its own calling convention. So step (2) — re-expressing the three
ports as `make-parameter`-backed objects — is unblocked but buys only the per-port collapse.

**The claims above are now pinned by tests**, in `crates/patina-tests/tests/callability.rs` — added
because three statements in this entry's own first draft were wrong in ways no test could have
contradicted. `dynamic-wind` performing no validation is proven by *ordering* (its before- and
body-thunks run before a bad after-thunk is reached), not by matching an error message; and the
"still half-applied" limit below is pinned as behaviour — a continuation answers `procedure?` with
`#t` yet is rejected as a `make-parameter` converter. That last test is written to fail when the gap
is closed, verified by closing it locally: whoever folds `Continuation` in gets a failure telling
them to update the test, the doc comment and this entry together.

**Still half-applied, and left open deliberately.** `is_procedure` is not yet the single source of
truth for callability: tree-walker continuations (`HeapObjectData::Continuation`) are callable but
live in `is_continuation`, so five callers still spell the question `is_procedure(x) ||
is_continuation(x)` while four omit the disjunct. Folding that variant in would let all five drop
it, but it widens the four that omit it today — a behaviour change wanting its own tests, not a
rider on this one. Recorded in the `is_procedure` doc comment so the next reader does not mistake
the enumeration for complete.

Guarded by `crates/patina-tests/tests/parameters.rs`, which was ported from a tree-walker-only local
helper to the shared both-backend helpers in the same change (Track Q Q1) — a single-backend file
could not have stated the property it was missing. It pins the widening as a *predicate*: things
that are not callable still answer `#f`, and the standard ports still answer `#t`.

**Tree-walker: an unbound variable escaped `guard` in most positions** — ✅ **fixed**
(2026-08-15). A bare `undefined-var` was a catchable condition, but `(undefined-fn)` was not — on the
tree-walker only. chibi, Gauche and Chez catch both, and the VM already did.

The defect was a class, not an instance: `step.rs` hand-copied the route-through-handlers dance in
some arms (`Var`) and `?`-propagated the same lookup failure in others, so catchability depended on
where the variable sat. Auditing every arm after fixing the reported `LetVal`/`App` case found five
more escaping positions — `if` tests, `set!` targets, `define` values, `call/cc` operands, and
unquotes — each confirmed divergent (VM caught them, tree-walker did not). The fix is structural: a
single `try_catchable!` macro in `step.rs` that every fallible user-level evaluation goes through,
routing failures into `maybe_route_error_through_cps` (which already held the catchability policy —
`InternalError` and continuation escapes still propagate). A new arm can no longer quietly
reintroduce the escape, and continuation-environment lookups stay `?` because a missing continuation
is a compiler invariant, not a user error.

Found by a test written for the import-set fix below, which used `guard` to assert a name was
unbound and got two different answers from the two backends. Pinned in
`crates/patina-tests/tests/backend_divergence.rs` as a converged row covering all nine positions.

**Rust registry primitives ignored the import set at top level** — ✅ **fixed** (2026-08-15).
VM only; the tree-walker was right all along, which turned out to be the whole story. A program
importing only `(scheme base)` could still call `cadddr` from `(scheme cxr)` or `bitwise-and` from
`(srfi 151)`, because `VmBackend::with_fs` called `install_primitives()` — a loop binding *every*
registered primitive into globals by short name. The registry knows each primitive's owning library
(it is right there in `qualified_name`, and the loop even reads it) but nothing consulted it, so
`import` never got a say.

Scheme-level exports were scoped correctly and libraries enforced their imports properly, so the
hole was specific to registered primitives at the top level — and that asymmetry is why it survived.
The same expression succeeded at the top level and failed inside a `define-library`, and the top
level is where one naturally checks. It had already cost a real bug: `(srfi 132)`'s `vector-merge`
called `cadddr` without importing `(scheme cxr)` and failed only inside the library.

**The fix was deleting a call.** `load_bootstrap()` already ran immediately afterwards, loading
`(scheme base)` and `(patina debug)` and defining exactly their exports — the correct model, already
written, already what the tree-walker did. `install_primitives()` was redundant with it and
destructive to it. Primitives reached through a library carry `registry_index: None`, which
`resolve_index_cached` fills in on first call, so the VM's primitive fast path is untouched; the
function stays for the VM unit tests, which build a bare `VmState` with no library machinery.

The predicted fallout did not arrive: **77 test binaries green, 1226/1226 on both backends, and the
corpus unchanged at 138**. In hindsight that was foreseeable — the suite runs every case on both
backends and asserts they agree, so anything relying on the leak was already failing. One corpus
package swapped `srfi-128` for `load` in its unbound list, which is the leak closing in plain view.

New guard: `crates/patina-tests/tests/primitives_reachable_by_import.rs` asserts every registered
primitive is exported by *some* shipped library. Before this change an under-exporting library
builder cost nothing, because the blanket install handed the primitive out regardless; now such a
primitive is simply unreachable, and nothing else would notice. It deliberately checks reachability
rather than the primitive's own `library` label — several are labelled with a library adjacent to
the one R7RS exports them from (`char->integer` says `scheme.char`; R7RS puts it in `(scheme base)`),
so asserting the label would be asserting a naming convention instead of the property that matters.

**Still open, and deliberately not folded in:** `lib/scheme/base.sld` exports the eight three-deep
`cxr` procedures (`caaar`…`cdddr`), where R7RS-small puts them only in `(scheme cxr)`. That is a
documented extension rather than a leak, and tightening it is a separate decision.


**The standard port procedures were not parameter objects** — ✅ **fixed** (2026-08-15). Both
backends. R7RS §6.13.1 requires `current-input-port`, `current-output-port` and `current-error-port`
to be parameter objects overridable with `parameterize`; all three were plain 0-argument procedures,
so `parameterize` rejected them outright.

The fix is smaller than this entry previously predicted, and the reason is worth keeping. It warned
that accepting a second arity would not be enough, because the ports are backed by Rust-side globals
that other primitives read directly. But `parameterize` (`lib/scheme/base/parameters.scm`) drives a
parameter *through the object itself* — `(p)` to read, `(p v)` to install, `(p old)` from
`dynamic-wind`'s after-thunk to restore. So writing the thread-local from the setter arity is not a
shortcut around the dynamic binding: the thread-local **is** the binding, and it is exactly what
`display` with no port argument consults. The version that would have been broken is the opposite
one — a Scheme-level rebinding that left the thread-local alone would redirect nothing.

`(chibi test)`'s SRFI 158 suite defines `with-input-from-string` in precisely these terms, so its
expectation drops 1 → 0 and **every upstream suite now runs at zero failures**; the expectations
table in `scheme_tests/upstream/README.md` has no non-zero row left. Redirecting the current output
port is also the capability L3 wanted for capturing a package's output without touching real stdout.

**Adjacent, and left open: `(current-output-port)` allocates a fresh wrapper per call**, so
`(eq? (current-output-port) (current-output-port))` is `#f`. Pre-existing — verified against the
build before this change — and untouched by it, since `parameterize` compares nothing and the
restore path passes the same underlying port back. It is still odd for a parameter to return a
different object each read, and `(eq? p (current-output-port))` is a reasonable thing for a package
to write. The tests in `crates/patina-tests/tests/standard_ports.rs` assert restoration by where
output *lands* rather than by identity, which is the better test regardless.

**Relinking stopped at a `quote` *argument*** — ✅ **fixed** (2026-08-14). Both backends. Found the
moment the `@` fix let `(chibi match)` load, and the only §6 entry that was blocking a corpus
package. Recorded first as "a library-internal macro loses its definition environment through a
nested `let-syntax`", which is where it was found rather than what it was — the narrowing is the
useful part, so both the symptom and the cause are kept here.

The cause is one line of tree-walking. `rewrite_refs` recursed on each **cdr** as though a cdr were
a form, and its first act on a form is to read the head for a quoting operator. A tail is not a
form: in `(pass quote (helper 1 2))` the tail is `(quote (helper 1 2))`, which read as a quote form,
so the walk returned it untouched and everything after the argument `quote` was left unrelinked.
`helper` stayed a bare name and was resolved, unsuccessfully, at the use site. Three lines are
enough:

```scheme
;; library (t m) exports only `top`; `helper` and `pass` are internal
(define-syntax helper (syntax-rules () ((_ a b) (list 'expanded a b))))
(define-syntax pass   (syntax-rules () ((_ a b) b)))
(define-syntax top    (syntax-rules () ((_) (pass quote (helper 1 2)))))
;; (import (t m)) (top)
;; Chez, Gauche, and now Patina => (expanded 1 2)
;; before                       => Error: unbound variable: helper
```

Nothing about it needed `let-syntax`, nested macros, or libraries beyond one private helper; the
three-macro chain it was found in only made `quote` land in argument position. Any template that
passes `quote` along was affected. The fix walks a form's spine once — head read in head position,
arguments read as arguments — instead of re-reading each tail as a new form, which is also how the
evaluator reads `(f quote x)`. The old walk simply disagreed with it.

**Do not read the original diagnosis as a near miss.** It blamed the relinking *basis* — a
`HashSet` of template names that "cannot express which occurrence came from that template" — and
proposed designing this together with the entry below. That would have been a rewrite of the
machinery to fix a defect in how a list is traversed. The lesson is the ordinary one: the three-macro
repro was reduced further before anything was designed, and reducing it changed the answer.

`(chibi match)` reaches this for **any pattern with a quoted tail**, since `(x . 'bad)` reads as
`(x quote bad)` and is threaded through `match-two` as three arguments. The symptom there was worth
reading too, because it named neither the cause nor the right macro: with `match-two` unlinked it
was no longer a macro, so the desugarer treated the form as an application and desugared its
arguments — including the inert getter/setter pair `((cons 1 2) (set! (cons 1 2)))` that only the
`(set! setter)` rule ever destructures — and reported `set! expects 2 arguments, got 1`.

Result: chibi-match went from a dead suite to **74 of 75**, and the corpus from parse-error to
wrong-result on that package. The one remaining failure is `match-letrec`, recorded below. The
entry below — relinking rewriting *by name* — is untouched by this and stays open.

The two entries after this one were found on 2026-08-14 by auditing for the defect class behind the
fixed items in this section — an identifier comparison made on the wrong basis. Each is cross-checked
against **two** reference implementations (Chez Scheme and Gauche), not one. Neither is reachable
from the corpus queue today; both were found by construction, so nothing in §4 depends on them. The
two after those predate this track's macro work and are unchanged.

**Literal matching had no "both unbound" case** — ✅ **fixed** (2026-08-14). Both backends.
R7RS 4.3.2 gives literal *matching* `free-identifier=?` semantics: an input matches a literal when
both denote the same binding, **or both are unbound and have the same name**. That second clause was
missing. `tagged_matches_literal` instead required the literal's scopes to be a subset of the
input's, so an *introduced* literal (carrying the enclosing expansion's scopes) could never match a
*substituted* input (carrying none), even with both unbound:

```scheme
(define-syntax m
  (syntax-rules ()
    ((_ e) (let-syntax ((n (syntax-rules (k) ((n k) 'lit) ((n x) 'notlit))))
             (n e)))))
(list (m k) (m other))
;; before => (notlit notlit)
;; after / Chez / Gauche => (lit notlit)
```

Bindings are not visible in that function; the caller's `is_literal_shadowed_tagged` veto runs first
and is the binding-aware half. So the fix is to compare names, matching what the function's three
other arms already did — the identifier-vs-identifier arm was the lone exception.

Note this is a *different question* from the one below, and conflating them is the trap: **membership**
("is this pattern identifier a literal at all?") compares identity, while **matching** ("does this
input match that literal?") is `free-identifier=?`. Predates the identity work below — verified on
`ac6f6d2`.

Moved `arvyy-interface` off `Name mismatch in interface implementation … proc0 proc0` — two
identifiers spelled alike that compared unequal — onto an unrelated failure.

**Identifier identity was decided by name inside a macro that writes a macro** — ✅ **fixed**
(2026-08-14). Both backends. Three places compared identifiers by name alone, so an identifier
*substituted* from the outer use site and one *introduced* by the outer template were confused
whenever they were spelled the same. Every case below was cross-checked against Chez Scheme.

| Site | Was | Now |
|---|---|---|
| `matcher/literal.rs` | a pattern literal with empty scopes matched **any** identifier | matches the same identifier, via the general name-and-scopes comparison that was already below it |
| `compiler/pattern.rs` | any substituted identifier became a *literal* pattern | classified by the literals list alone, per R7RS 4.3.2 — so a substituted identifier that is not a literal is an ordinary pattern variable |
| `compiler/{helpers,template,escape}.rs` | pattern variables keyed by name | keyed by `(name, scopes)`, so a substituted identifier never collides with an introduced pattern variable spelled alike |

The literals list now keeps the scopes each identifier was written with (`LiteralSpec` in
`patina-core/src/compiled_macro.rs`); both literals parsers previously discarded them.

This unblocks the `new-symbol?` guard in `(chibi parse)`'s `grammar-bind`. `syntax-rules` cannot
compare two identifiers, so the guard builds an inner macro whose literals list holds the names bound
so far and calls it with an identifier that matches nothing: a literal cannot match, so the fallback
rule answers "already bound", while a non-literal is a pattern variable, matches, and answers "new".
With a literal that matched anything, the answer was always "new" and the same grammar nonterminal
got a variable more than once — surfacing as `Duplicate parameter 'space' in lambda`.

Watch the interaction with the fix below: the two `space` parameters were *correct* duplicates, so
rejecting them was right. The defect was upstream of that, in the guard that should have stopped the
second one being created.

**Recursive macros could not introduce a fresh binding per expansion** — ✅ **fixed** (2026-08-14).
Both backends. `check_no_duplicates_scoped` in `patina-frontend/src/desugarer/utils.rs` keyed its
`HashSet` on the parameter *name*, so two params introduced by different expansions of the same
template were rejected as duplicates even though their scope sets differed:

```scheme
(define-syntax gen
  (syntax-rules ()
    ((_ () (args ...)) (lambda (args ...) (list args ...)))
    ((_ (x . rest) (args ...)) (gen rest (args ... a)))))
((gen (1 2) ()) 10 20)
;; before => Error: Duplicate parameter 'a' in lambda
;; after / Gauche / Chez => (10 20)
```

Nothing downstream needed changing: `ScopeId::fresh()` per expansion already gave each introduced
`a` its own scope, and both the VM's `alpha_rename` pass and the tree-walker bind params by name
*and* scopes. Only the eager desugarer check disagreed. The two rest-parameter checks beside it
compared by name alone for the same reason and now share one `binds_identifier` rule. Genuine
duplicates — `(lambda (q q) q)`, `(lambda (q . q) q)` — still error. Regression tests in
`crates/patina-tests/tests/compliance/macros_advanced.rs` run on both backends and include SRFI
156's `extract-placeholders` shape, which is what surfaced this.

**Tree-walker: nested `guard` across a procedure boundary** — ✅ **fixed**. Was 15 errors in the
R7RS suite, all `Undefined variable: k_N`. Pre-existing (reproduced on `main` before this track) and
invisible until upstream `(chibi test)` was adopted, because its applier calls every test thunk from
inside its own `guard`.

Cause: `capture_cont_bindings` returned `None` for the six `ContValue` variants that decorate another
continuation with an effect — popping an exception handler, caching a forced promise, running a wind
thunk, delivering multiple values. Returning `None` dropped the whole entry from the captured
continuation environment, so the captured body's reference to that binder was stranded. Those
variants now capture through to the continuation they wrap, via one shared
`ContValue::wrapped_cont()` rule rather than six arms. The wrapper's effect is still not serialized,
which matches the escape path's existing behaviour of rebuilding handler and wind state.

The VM was never affected: `call/cc` snapshots whole machine state instead of serializing
continuations name by name, so there is no per-variant case to leave unimplemented.

```scheme
(define (run thunk)
  (guard (e (#t (list 'outer e)))
    (list 'ok (thunk))))

(run (lambda () (guard (x (else 'inner)) (error "boom"))))
;; tree-walker => Error: Undefined variable: k_7
;; VM          => (ok inner)
```

`(chibi test)` hits it because `test-default-applier` calls the test thunk from inside its own
`guard`, so every suite test whose body uses `guard` trips it — which is why the errors cluster in
6.10 Control Features and 6.11 Exceptions. A single `guard`, a `guard` under `dynamic-wind`, a
`guard` under `call/cc`, and two sequential `guard`s all work; only the nesting-across-a-call case
fails.

**Bare `@` rejected as an identifier** — ✅ **decided and fixed** (2026-08-14). Both backends. We now
read a leading `@`, which was the largest parse-error bucket at 9 packages and moved the corpus
129 → 133.

Patina was *strictly correct*: R7RS 7.1.1 makes `@` a ⟨special subsequent⟩ and not an ⟨initial⟩, so
a bare `@` is not a conforming identifier. The decision to relax it rests on two facts, not on
counting packages:

1. **Accepting it cannot change the meaning of any conforming program**, because no conforming
   program contains a bare `@` token. This is a pure widening — unlike the `syntax-rules` shape
   buckets above, where matching chibi would mean accepting genuinely malformed input.
2. **The ecosystem treats `@` as a datum, not as sloppiness.** SXML specifies `@` as the attribute
   marker (and `@raw` as a tag); `(chibi match)` uses `@` as a `syntax-rules` literal for its
   named-record-field pattern. Chez, Gauche and chibi all read it.

The reader/writer asymmetry that follows is deliberate and matches the references: we read `@` but
still **write** it as `|@|`, exactly as Gauche does (Chez writes `\x40;`). The invariant worth
holding is that our writer's output reads back as itself under our own reader, not that we mimic
another implementation's spelling.

Scope was one `matches!` arm in `is_identifier_start` (`patina-frontend/src/lexer/mod.rs`), which
already admitted `.` and non-ASCII letters — both themselves deviations from a literal reading of
7.1.1, so this is consistent with what the lexer already did rather than a new posture. `,@` is
lexed before identifiers and so is unaffected, as is `⟨real⟩@⟨real⟩` polar notation (`@` is an
identifier *start*; a leading digit still reads as a number). Both are covered by unit tests
alongside `crates/patina-tests/tests/at_identifiers.rs`.

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
