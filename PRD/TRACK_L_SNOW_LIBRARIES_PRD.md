# Track L — Snow Library Compatibility PRD

**Created:** 2026-06-20
**Status:** Planning → ready to execute
**Scope decision:** library-compat-first — bundle dependencies and fix loading-gap edge cases so manually-placed Snow source runs. The `patina pkg` auto-fetcher and FFI are deferred.
**Umbrella:** `PRD/SNOW_AND_PERF_ROADMAP.md` (cross-track sequencing)

---

## 1. Context & problem

The goal is to **run real, portable Snow packages** (snow-fort.org / chibi ecosystem). The encouraging finding from evaluation: Patina's library *machinery* is already R7RS-complete and ready to consume portable source — `define-library` with all clauses (`export`, `import` with all 5 import-set forms, `begin`, `include`, `include-ci`, `include-library-declarations`, `cond-expand`), the `features` procedure with platform/arch identifiers, and relative-path `include` with circular detection. The blockers are **content and edge cases**, not the import system:

### Verified current-state evidence

| Observation | Evidence |
|---|---|
| Only **9 SRFIs** bundled. | `lib/srfi/` = 1, 8, 69, 111, 113, 128, 132, 133, 158. |
| Only **one `(chibi …)`** library. | `lib/chibi/` = `test.sld` only. |
| Unknown `define-library` clauses are a **fatal error** (aborts the whole load). | `"Unknown library declaration: {}"` — `crates/patina-frontend/src/library_parser.rs:212`. Known clauses handled at `:177` (begin), `:198` (cond-expand), `:206` (include-library-declarations). |
| **No inline `define-library`** code path (only file-based `.sld` discovery). | Zero matches for `define-library`/`define_library` in `crates/patina-frontend/src/desugarer/mod.rs`. |
| The dependency search path (`./.patina/lib/`) the package-manager design assumes is **not wired**. | `LibraryRegistry { search_paths: Vec<PathBuf> }` — `crates/patina-runtime/src/library_registry.rs:87`; defaults set in `with_default_paths`. |

The recurring porting frictions (all *resolved* for the existing 9 SRFIs) are catalogued in `PRD/phase2/SRFI_PORTING_ISSUES.md` and will recur for each new reference implementation imported.

## 2. Goals
- A real **pure-Scheme Snow package** loads and runs, exercised by an integration test (the acceptance bar).
- Bundle the dependency libraries such packages commonly import (the high-frequency SRFIs and `(chibi …)` libraries).
- Make library loading **degrade gracefully** instead of hard-failing on benign edge cases.

## 3. Non-goals (deferred)
- `patina pkg` auto-fetcher (download/extract snowballs) — `PRD/future/PACKAGE_MANAGER_DESIGN.md`. L0 wires the forward-compatible `./.patina/lib/` hook.
- FFI Layer 1/2 for C-shim packages — `PRD/FFI_DESIGN.md`. Pure-Scheme packages (the majority a learner wants) are unaffected.

---

## 4. Work items

### L0 — Close the loading-gap edge cases  *(small, high-leverage; do first)*
1. **Graceful unknown clauses.** `library_parser.rs:212` hard-errors on any unrecognized `define-library` clause; portable `.sld` files occasionally carry vendor-specific ones. Change to warn-and-skip (behind a strictness flag, default lenient) or a small known-clause allowlist, so one unknown clause doesn't abort the whole load.
2. **Inline `(define-library …)` code path.** Add desugarer handling so a `define-library` written directly in a script/REPL source (not a discovered `.sld`) is parsed via the existing `library_parser` and registered. Some single-file libs and `package.scm` forms embed it inline.
3. **`./.patina/lib/` on the search path.** Add the project-local dependency directory to `with_default_paths` in `library_registry.rs` (ahead of the workspace/exe paths) so dependencies can be dropped under a project dir — the forward hook for the eventual fetcher.
- **Acceptance:** new cases in `crates/patina-tests/tests/sld_file_loading.rs` — an inline `define-library`, an `.sld` with an unknown clause (loads with a warning), and a library resolved from `./.patina/lib/`.

### L1 — Bundle the common missing SRFIs
Prioritize by how often Snow packages import them.
- **Pure-Scheme (cheap; `.sld` + `.scm` only):** SRFI 26 (`cut`/`cute`), SRFI 13 (strings), SRFI 14 (char-sets), SRFI 41 (streams), SRFI 64 (test — many packages' test harness).
- **Need Rust primitives** (add under `crates/patina-runtime/src/stdlib/internal_*.rs`, registered in *both* the primitive registry and the library builder; aligns with `PRD/PARALLEL_TRACKS.md` Track B3):
  - SRFI 125 — standard hash-table; needs a `HeapObjectData::HashMap` variant.
  - SRFI 143 — fixnum operations.
  - SRFI 151 — bitwise operations.
- **Porting patterns to reapply** (from `PRD/phase2/SRFI_PORTING_ISSUES.md`): import `(scheme r5rs)` for R5RS naming (`exact->inexact` etc.); shim `:optional`/`let-optionals`/`receive`/`check-arg`; treat form-feed as whitespace (already fixed); defer arity rejection so `guard` can catch `apply` errors (already fixed); watch the VM control-op edge cases in `PRD/phase2/INSTRUCTION_LEVEL_CONTROL_OPS.md`.
- **Acceptance:** one integration test per SRFI exercising its headline forms; `./scripts/run_chibi_tests.sh` stays 1163/1163.

### L2 — Bundle the common `(chibi …)` libraries
Snow packages frequently `(import (chibi …))`; today only `(chibi test)` exists. Add, in rough priority order, porting from the reference at `~/Project/reference/chibi-scheme`:
- `(chibi string)`, `(chibi io)`, `(chibi pathname)` — mostly pure Scheme over R7RS + SRFIs present after L1.
- `(chibi filesystem)`, `(chibi process)` — need new primitives (filesystem/process); route file ops through the VFS `FileSystem` trait (`PRD/phase2/VFS_DESIGN.md`) so they stay testable.
- `(chibi uri)` — pure Scheme.
- **Acceptance:** import + smoke-test each library.

### L3 — Snow validation harness  *(proves the track)*
Pick 2–3 real **pure-Scheme** Snow packages whose dependencies are satisfied after L1/L2. Drop their `.sld`/`.scm` under `./.patina/lib/` (or `./lib/`) and add integration tests that import and exercise them. This is the acceptance bar for "Patina can use existing Snow libraries."
- **Acceptance:** new tests under `crates/patina-tests/` load and run the chosen packages end-to-end.

---

## 5. Sequencing within the track
**L0** (edge cases) → **L1** (SRFIs: pure-Scheme set first, then primitive-backed) → **L2** (`chibi` libs) → **L3** (validation). L1's primitive-backed SRFIs (125/143/151) can proceed in parallel with the pure-Scheme set. See `PRD/SNOW_AND_PERF_ROADMAP.md` for the M1–M4 interleave with Track P; note that Track P's GC (P6) is the cross-cutting unblocker that lets L3's real packages run long-running without leaking.

## 6. Risks & mitigations
- **Per-new-SRFI friction** (non-R7RS constructs, R5RS naming, control-op edges) → apply the resolved patterns in `PRD/phase2/SRFI_PORTING_ISSUES.md`; import each reference implementation incrementally with its own test.
- **Lenient unknown-clause handling hiding real errors** → gate behind a strictness flag and emit a visible warning, not silent skip.
- **New filesystem/process primitives** → implement behind the `FileSystem` trait for testability and future WASM.
- **Stale tracking docs** → reconcile `PRD/phase2/R7RS_LARGE_STATUS.md` (says SRFIs "not started") with `PRD/PARALLEL_TRACKS.md` (9 done) when updating SRFI status.

## 7. Verification (track-wide)
- Routine: `cargo build --release && ./scripts/run_chibi_tests.sh` (must stay 1163/1163) after every item.
- Per-library: a focused integration test exercising headline forms.
- End-to-end: the L3 harness loads and runs real Snow packages.
- Quality gate: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt`.
