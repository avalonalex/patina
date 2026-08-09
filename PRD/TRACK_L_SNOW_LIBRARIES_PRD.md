# Track L — Third-Party Library Compatibility PRD

**Created:** 2026-06-20
**Updated:** 2026-08-08 — corpus vendored (197 packages, `compat/vendor/`); L1 rescoped to the R7RS-large bundling policy and ordered by measured in-degree. Earlier: reframed from "be a Snow target" to **self-contained compatibility coverage**; verified the loading machinery end-to-end; established that no chibi fork, upstream PR, or external package manager is required; promoted the harness (L3) to the centrepiece.
**Status:** Planning → ready to execute
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

### L0 — Close the loading-gap edge cases  *(small, high-leverage; do first)*
1. **Graceful unknown clauses.** `library_parser.rs:212` hard-errors on any unrecognized `define-library` clause; portable `.sld` files occasionally carry vendor-specific ones. Change to warn-and-skip (behind a strictness flag, default lenient) or a small known-clause allowlist, so one unknown clause doesn't abort the whole load.
2. **Inline `(define-library …)` code path.** Add desugarer handling so a `define-library` written directly in a script/REPL source (not a discovered `.sld`) is parsed via the existing `library_parser` and registered. Some single-file libs and `package.scm` forms embed it inline.
3. **`./.patina/lib/` on the search path.** Add the project-local dependency directory to `with_default_paths` in `library_registry.rs` (ahead of the workspace/exe paths) so dependencies can be dropped under a project dir — the forward hook for the eventual fetcher.
4. **Program-relative resolution.** Add the directory containing the script being run to the search path, so a checked-out package runs without an install step (today it does not — see the evidence table).
5. **Skip a leading `#!` line** in script files, so installed Snow programs execute.
- **Acceptance:** new cases in `crates/patina-tests/tests/sld_file_loading.rs` — an inline `define-library`, an `.sld` with an unknown clause (loads with a warning), a library resolved from `./.patina/lib/`, and one resolved beside the running script; plus a shebang-prefixed script that runs.

### L0.5 — Library-path CLI surface  *(small; prerequisite for the harness)*
External library directories are currently unusable. This is table stakes for any Scheme, independent of distribution mechanism, and every later item depends on it.

- `-A <dir>` (append to search path) and `-I <dir>` (prepend) — the conventional spelling across Scheme implementations, and what the harness uses to point Patina at a fetched package.
- `-p <expr>` — evaluate and print; the harness's capability probe (`patina -p "(features)"`) and a generally useful one-liner facility.
- `--version` — trivial, and needed for reproducible result provenance in the report.
- `PATINA_LIBRARY_PATH` (colon-separated), matching the convention of every other implementation.
- **Acceptance:** `patina -A <dir> prog.scm` resolves a library from `<dir>`; `patina -p "(features)"` prints the feature list; unit coverage in `crates/patina-tests/`.

### L0.75 — Ecosystem survey  *(cheap; sizes and orders everything after it)*
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
- **Acceptance:** one integration test per SRFI exercising its headline forms; `./scripts/run_chibi_tests.sh` stays 1163/1163.

### L2 — Bundle the common `(chibi …)` libraries
Third-party packages frequently `(import (chibi …))`; today only `(chibi test)` exists. Port from the pinned chibi checkout the harness already fetches (locally mirrored at `~/Project/reference/chibi-scheme`), in the order L0.75/L3 dictate. Each ported library also brings its upstream `*-test.sld` suite, which drops straight into the L3 corpus:
- `(chibi match)`, `(chibi optional)` — pure Scheme, pervasive in chibi-authored packages, and `(chibi optional)` also retires the ad-hoc `:optional` / `let-optionals` shims the existing SRFI ports carry. Highest leverage in this group.
- `(chibi string)`, `(chibi io)`, `(chibi pathname)` — mostly pure Scheme over R7RS + SRFIs present after L1.
- `(chibi show)` / SRFI 166 — the formatting library much of the ecosystem writes output with.
- `(chibi filesystem)`, `(chibi process)` — need new primitives (filesystem/process); route file ops through the VFS `FileSystem` trait (`PRD/phase2/VFS_DESIGN.md`) so they stay testable.
- `(chibi uri)` — pure Scheme.
- **Out of scope permanently:** `(chibi ast)` and the other C-backed libraries (`.c`/`.dylib` in the reference tree) — they need FFI and bound the achievable pass rate. See §6.
- **Acceptance:** import + smoke-test each library.

### L3 — `patina-compat`: the self-contained compatibility harness  *(the centrepiece)*
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

- **Acceptance:** `cargo run -p patina-compat -- run` reports a baseline "N of M" on a machine with no Scheme installed, and tier 0 runs as part of normal CI so regressions surface without network.

---

## 5. Sequencing within the track
**L0** (edge cases) → **L0.5** (CLI surface) → **L0.75** (survey) → **L3 harness + baseline run** → **L1** (SRFIs: pure-Scheme set first, then primitive-backed) → **L2** (`chibi` libs) → **L3 re-run**, then loop L1/L2 against the refreshed histogram until the curve flattens.

Note the deliberate departure from numeric order: **L3 is built before L1/L2, not after.** The items are numbered by when they were conceived, not by when they run. Standing the harness up early is cheap once `-A` exists, and it converts the rest of the track from a guessed list of SRFIs into a measured queue — every subsequent port is chosen because it unblocks counted, named libraries. L1's primitive-backed SRFIs (125/143/151/27) can proceed in parallel with the pure-Scheme set.

**Self-containment invariant.** At no point does the track acquire a build-, test-, or CI-time dependency on another Scheme implementation or package manager. The chibi checkout is corpus *data* pinned by commit, not tooling. Anything that would violate this belongs in §3. See `PRD/SNOW_AND_PERF_ROADMAP.md` for the M1–M4 interleave with Track P; note that Track P's GC (P6) is the cross-cutting unblocker that lets L3's real packages run long-running without leaking.

## 6. Risks & mitigations
- **Per-new-SRFI friction** (non-R7RS constructs, R5RS naming, control-op edges) → apply the resolved patterns in `PRD/phase2/archive/SRFI_PORTING_ISSUES.md`; import each reference implementation incrementally with its own test.
- **The pass rate has a ceiling well below 100%** — a meaningful share of Snow packages import `(chibi ast)` or other C-backed/FFI libraries and can never pass while FFI is deferred. → Have the L3 harness classify these as *out-of-scope* rather than *failing*, and report the achievable denominator alongside the raw one, so the number is not misread as a defect count.
- **Coupling the track to another implementation's tooling** — depending on `snow-chibi` would put an interactive tool and chibi's release cadence on our critical path, and a fork of its implementation tables would need perpetual rebasing. → Avoided by construction: the harness depends on the snowball / `package.scm` / repo-index *formats*, which are stable s-expressions, not on chibi's code. See the self-containment invariant in §5.
- **Corpus rot** — pinned URLs 404, upstream repos move, checksums drift. → Pin by commit/sha and verify on fetch; a fetch failure must be reported as a distinct `unavailable` bucket, never silently folded into `fail`, so the score stays honest.
- **Network flakiness making CI unreliable** → only tier 0 (vendored, offline) gates ordinary CI; tiers 1–2 run nightly where a transient failure is visible but not blocking.
- **Lenient unknown-clause handling hiding real errors** → gate behind a strictness flag and emit a visible warning, not silent skip.
- **New filesystem/process primitives** → implement behind the `FileSystem` trait for testability and future WASM.
- **Stale tracking docs** → reconcile `PRD/phase2/R7RS_LARGE_STATUS.md` (says SRFIs "not started") with `PRD/PARALLEL_TRACKS.md` (9 done) when updating SRFI status.

## 7. Verification (track-wide)
- Routine: `cargo build --release && ./scripts/run_chibi_tests.sh` (must stay 1163/1163) after every item.
- Per-library: a focused integration test exercising headline forms.
- End-to-end: `cargo run -p patina-compat -- run` — the "N of M" number is the track's headline metric and is expected to move monotonically upward.
- Self-containment: the harness must produce that number on a machine with **no other Scheme installed** and, for tier 0, **no network**. This is a standing acceptance criterion, not a one-time check.
- Quality gate: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt`.
