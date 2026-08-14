# Track L — Third-Party Library Compatibility PRD

**Created:** 2026-06-20
**Updated:** 2026-08-08 — corpus vendored (197 packages, `compat/vendor/`); L1 rescoped to the R7RS-large bundling policy and ordered by measured in-degree. Earlier: reframed from "be a Snow target" to **self-contained compatibility coverage**; verified the loading machinery end-to-end; established that no chibi fork, upstream PR, or external package manager is required; promoted the harness (L3) to the centrepiece.
**Status:** In execution — L0, L0.5, L0.75, L4 done; L3 harness live with a measured baseline
(**127 of 187** vendored packages pass, 2026-08-14); L1/L2 continue against the measured queue
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
6. **SRFI 14** char-sets (in-degree 4), then the remaining standard-track set with little measured
   demand: Red 41, 101, 116, 117, 124, 127, 134, 135 and Tangerine 146, 159, 160.
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
- `(chibi string)`, `(chibi io)`, `(chibi pathname)` — mostly pure Scheme over R7RS + SRFIs present after L1.
- `(chibi show)` / SRFI 166 — the formatting library much of the ecosystem writes output with.
- `(chibi filesystem)`, `(chibi process)` — need new primitives (filesystem/process); route file ops through the VFS `FileSystem` trait (`PRD/phase2/VFS_DESIGN.md`) so they stay testable.
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

**Parse-error triage, 2026-08-14 (126 → 127).** Every parse-error bucket was reduced to a minimal
repro and cross-checked against Gauche and Chez, which is what separates *our* defects from upstream
code that only chibi accepts. One was ours and is fixed here — **template-introduced identifiers
were compared by name alone** (§6), unblocking **srfi-156**. The rest of the queue, with verdicts:

| Bucket | Pkgs | Verdict |
|---|---|---|
| bare `@` identifier | 9 | Patina is right per R7RS 7.1.1 (`@` is a ⟨special subsequent⟩, not an ⟨initial⟩); chibi and Gauche accept it. Needs the explicit decision recorded in §6, not a silent relaxation. |
| syntax-rules rule with >2 elements | 5 | **Chez rejects these too.** They are typos in chibi's own sources (e.g. `((_) bv off i)` in `ieee-754.scm`) that only chibi's lenient reader tolerates. Matching chibi here means accepting nonsense. |
| `syntax-rules` with no literals list | 3 | Upstream-malformed: `(syntax-rules ((_ x) 'x))` sits in `(chibi monad environment)`'s `cond-expand` **else** branch, which chibi itself never takes. Blocks chibi-show and chibi-snow-commands transitively. |
| `Duplicate parameter 'space' / 'symbol-first'` | 2 | chibi-parse and edn get *past* the §6 fix and now fail deeper, in `grammar-bind`'s `new-symbol?` guard. That guard decides whether a nonterminal is already bound by putting the accumulated names in a `syntax-rules` **literals** list, so it turns on `free-identifier=?` for a template-substituted identifier. A real gap, but its own investigation — not a small fix. |
| `Expected proper list in feature requirement` | 1 | **Ours, fix pending.** srfi-42 ships `(cond-expand (stklos …) (else #f))`; the inert `#f` is not a list, so it hard-errors instead of taking L0's warn-and-skip path — and reports a message belonging to a shared list helper, which misattributes the problem to the feature requirement. |

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

**Bare `@` rejected as an identifier** — 9 corpus packages, including `(chibi match)`. Patina is
strictly correct per R7RS 7.1.1 (`@` is a ⟨special subsequent⟩, not an ⟨initial⟩); chibi and Gauche
accept it. A deliberate strictness decision rather than a bug, but it needs making explicitly.

**Rust registry primitives ignore the import set at top level** — ❌ **open**, own PR. (Adjacent
cleanup landed 2026-08-12, PR #52: the four continuation-broken Rust higher-order primitives were
*deleted*, removing those instances of the pattern — but deletion was available only because Scheme
replacements already existed for continuation-safety reasons. The general registry/import-set
scoping fix below is still required.) A program that
imports only `(scheme base)` can still call primitives no imported library exports:

```scheme
(import (scheme base))
(cadddr '(1 2 3 4))      ;; => 4      -- (scheme cxr)
(bit-count 12)           ;; => 2      -- (srfi 151)
(arithmetic-shift 1 10)  ;; => 1024   -- (srfi 151)
(bitwise-and 12 10)      ;; => 8      -- (srfi 151)
```

Scheme-level exports *are* scoped correctly — `list-sort` and `bitwise-nand` are unbound under the
same import — so this is specific to the primitive registry: registered primitives land in the
top-level environment regardless of what was imported. Inside a `define-library` imports are
enforced properly, which is the whole reason this went unnoticed.

That asymmetry is also a testing hazard, and it already cost us one bug. `(srfi 132)`'s
`vector-merge` called `cadddr` without importing `(scheme cxr)`; it failed only inside the library,
while any script-level check of the same expression passed. Library code cannot be validated by
top-level scripts until this is fixed.

Adjacent and lower-stakes: `lib/scheme/base.sld` exports the eight three-deep `cxr` procedures
(`caaar`…`cdddr`) as a documented extension, where R7RS-small puts them only in `(scheme cxr)`.
Worth settling in the same PR while the cost of tightening is still low.

**The standard port procedures are not parameter objects** — ❌ **open**, own PR. R7RS §6.13.1:
`current-input-port`, `current-output-port` and `current-error-port` "are parameter objects, which
can be overridden with `parameterize`". Patina implements all three as plain 0-argument procedures,
so `parameterize` rejects them:

```scheme
(parameterize ((current-input-port (open-input-string "a b c"))) (read))
;; => Invalid syntax: current-input-port expects exactly 0 arguments, got 1
```

`make-parameter` and `parameterize` work correctly for user-defined parameters, so the machinery
exists; only the three built-ins are outside it. They are backed by Rust-side global state
(`get_current_output_port` / `set_current_output_port` in `primitives/io/ports.rs`) that other
primitives read directly, so the fix has to make that global read through the parameter's dynamic
binding rather than merely accepting a second arity — which is why this is its own PR and not an
inline fix.

This is the one non-zero entry in the reference-suite expectations table: SRFI 158's suite defines
`with-input-from-string` as `(parameterize ((current-input-port (open-input-string str))) (thunk))`,
which is pure R7RS. It was previously recorded as the suite depending on a chibi extension. It is
not — the extension is three lines of standard Scheme, and it is our `parameterize` that refuses it.
Lower the SRFI 158 expectation to 0 when this lands.

Redirecting the current ports is also a capability the L3 harness will want directly, for capturing
a package's output without touching the real stdout.

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
