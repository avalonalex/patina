# AGENTS.md

Shared project instructions for Codex and Claude Code. Edit this file for rules
that apply to both tools; `CLAUDE.md` imports it. Keep tool-specific configuration
out of this file. See `docs/README.md` for maintenance and handoff guidance.

## Project Overview

Patina is an R7RS-small Scheme interpreter written in Rust. The default backend is a register-based bytecode VM (`patina-vm`). A CPS tree-walking interpreter (`patina-tree-walker`) is also available via `--tree-walker`. Verify current compliance with the test commands below.

All runtime values are `TaggedValue` — NaN-boxed 8-byte `Copy` types. No `Value` enum exists (fully removed). Macros and derived forms (`let`, `cond`, `do`, etc.) are implemented in Scheme (`lib/scheme/base/*.scm`), not as special forms.

## Workspace Structure

```
patina/
├── lib/scheme/             # R7RS .sld library files + .scm macro implementations
├── test-lib/               # third-party libraries the test lanes supply with
│                           # `-A`, NOT bundled — see test-lib/README.md
└── crates/
    ├── patina-core/        # TaggedValue, Heap, Environment, CoreExpr, CpsExpr, scope sets, SourceMap
    ├── patina-runtime/     # Backend trait, LibraryRegistry, internal stdlib primitives
    ├── patina-ir/          # ExprVisitor, CPS transform, re-exports CoreExpr types
    ├── patina-frontend/    # Lexer, Parser, Desugarer
    ├── patina-macros/      # syntax-rules with Racket-style scope-set hygiene
    ├── patina-pipeline/    # Legacy tree-walker embedding facade
    ├── patina-primitives/  # Shared backend-agnostic primitive implementations
    ├── patina-vm/          # Register-based bytecode VM (default backend)
    ├── patina-tree-walker/ # CPS tree-walking backend (--tree-walker)
    ├── patina-interpreter/ # High-level Interpreter<B: Backend> API
    ├── patina-repl/        # rustyline REPL + script runner binary
    ├── patina-tests/       # integration tests, and the tests/scheme suite files
    └── patina-compat/      # third-party compatibility harness over compat/vendor/ (Track L)
```

## Development Commands

The Rust version is pinned in `rust-toolchain.toml` and rustup applies it
automatically inside the repo, so local builds and CI use the same compiler.
Bumping it is a deliberate one-line change — run the full gate below in the
same PR, since a newer clippy usually finds something.

```bash
# Build
cargo build --release

# Run REPL / script
cargo run --release
./target/release/patina script.scm

# Routine verification (preferred — fast, covers R7RS compliance)
cargo build --release && ./scripts/run_chibi_tests.sh

# All Rust tests (no doc-tests)
cargo test --all --lib --tests

# Integration tests only
cargo test --package patina-tests

# The suite files under chibi and Gauche, checked against the divergence
# register (crates/patina-tests/tests/scheme/DIVERGENCES.tsv). Run it after
# touching any tests/scheme/*.scm; a missing oracle is skipped loudly.
./scripts/run_suite_oracles.sh
./scripts/run_suite_oracles.sh --list      # what they answer, checking nothing

# Larceny's R7RS suites (second opinion; not vendored — LGPL — so this runs
# from ~/Project/reference/larceny, which the script tells you how to fetch)
./scripts/run_larceny_tests.sh            # R7RS-small + Red edition, VM
./scripts/run_larceny_tests.sh --r6rs     # (r6rs …) emulation libraries
# A change that moves a Larceny tally re-pins that suite's rows in
# scheme_tests/reports/larceny_gc_stress.tsv, on every lane it moves, in the
# same PR: the nightly GC stress lane holds each suite to them. The script
# above warns when they differ; edit the rows' tallies, or re-pin them with
./scripts/run_larceny_gc_stress.sh --update-baseline [--tree-walker] [--r6rs] SUITE...

# Specific crate
cargo test --package patina-frontend

# Lint / format (the release clippy lints the shipped build, without the
# stale-reference checks that debug and --all-features compile in)
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --release --all-targets -- -D warnings
cargo fmt --all -- --check
```

### Verification expectations

Use focused tests during iteration. For code changes, run the routine verification
above and the affected Rust tests. For a toolchain bump or a change whose scope
cannot be bounded, run the full Rust test, clippy, and format checks. For documentation-only
changes, check links, paths, and `git diff --check`; a Rust rebuild is unnecessary.
Report commands actually run, failures, and skipped checks. Never treat missing
external oracles as a passing comparison. If chibi is unavailable,
`SKIP_CHIBI_TESTS=1 cargo test --all --lib --tests` matches CI’s Rust lane,
but omits those external comparisons.

`.github/workflows/ci.yml` is the source of truth for the full gate. It skips
pushes and pull requests whose changes are confined to `PRD/`; mixed changes
run the full gate. See `docs/TEST_ORGANIZATION.md`, "CI trigger scope", for
GitHub's path-filter limits and required-check considerations.

| Job | What it runs |
|---|---|
| Test Suite | `cargo test --all --lib --tests` on **ubuntu and macos** (`SKIP_CHIBI_TESTS=1`), then `run_gc_stress_tests.sh` on the same debug build: 16 GC- and control-relevant targets under `PATINA_GC_STRESS=16` and `scheme_suite.rs` at 4096, each with its test count and a minimum of collections pinned (#626) |
| R7RS Compliance | `run_chibi_tests.sh` **and** `run_chibi_tests_tree_walker.sh`, then `patina-compat check-smoke` on both backends, then the steady-state lane's per-PR rows (`run_steady_state.py --quick`, #652) |
| GC differential | `run_gc_differential.sh` on release built with `--features patina-core/gc-check` at stress 1, after the positive controls of the stale-reference checks (#621), the GC protocol checks (#624) and the retired-register checks (#625) — the defer-balance control also in the plain release build — **and** on debug at stress 16 |
| GC zeal (`gc-zeal.yml`, path-filtered and weekly) | `run_gc_zeal.sh` on the same release `gc-check` build — the control suite files under `PATINA_GC_ZEAL=entry`, both backends — and `finished_forms_release_code` under zeal, for changes to the VM's runtime, compiler or types, the heap or `TaggedValue`, the library loader or registry, the tree-walker's evaluator, the toolchain or the lane itself, and weekly on `main` |
| Rustfmt / Clippy | `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo clippy --release --all-targets -- -D warnings` for the plain release build without the checks; `scripts/check_gc_trace_names.py`, that the trace code names every field (#623); and the GC census's tests, with its feature (#651) |
| Suite oracles | `run_suite_oracles.sh` under chibi 0.12 and Gauche 0.9.15, pinned and built from source, against `DIVERGENCES.tsv` |
| Nightly (`nightly.yml`, daily and when its lanes change) | `run_larceny_gc_stress.sh`, R7RS and `(r6rs ...)`, on each backend: Larceny's suites at the pinned commit under stress in the release `gc-check` build, each tally held to `scheme_tests/reports/larceny_gc_stress.tsv` (#626); and every row of the steady-state lane (`run_steady_state.py`, #652) |

For GC/rooting changes, also run `scripts/run_gc_differential.sh` against release
and debug builds, and `scripts/run_gc_stress_tests.sh` after the Rust tests;
debug builds, and release built with
`--features patina-core/gc-check`, compile in the stale-reference checks (#621),
which panic on a use of a freed or reused slot, and fill a register its
liveness map retired with `DEAD_SLOT`, which panics when read (#625). For
changes to register liveness maps, safe points or collection triggers, also
run `scripts/run_gc_zeal.sh` on the release `gc-check` build. For backend semantics,
run both chibi backend scripts. Check CI results when a branch is pushed; a push
alone does not establish that checks passed.

For measured build timings and stale `target/` troubleshooting, see
`docs/TEST_ORGANIZATION.md`, “Local build performance”. Timings are machine-specific.

## Documentation

**Do not create new markdown files without user approval.** A user request to
create or restructure agent instruction files authorizes the files needed for that task.
Prefer updating existing docs for other work.

**PRDs are high level; work items are GitHub issues** (owner decision,
2026-09-19). A PRD says what a track is for, where it stands and which rules it
leaves behind. It does not hold the narrative of an individual defect: the
repro, the measurement against the references, the diagnosis and the notes for
a fix go in an issue, and the PRD carries one line linking to it. This is the
written form of the issue-first habit — file the issue before the fix, and let
the PR close it. `PRD/ARCHIVE/TRACK_L_LEFTOVERS.md` is the model: a short ledger
where the record it replaced, now `PRD/ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md`, had grown
past 2,500 and was carrying entries nobody could find — two defects were
re-diagnosed from scratch in 2026-09 that it had already recorded. **Search the
issues and the archive before filing**, for the same reason.

**Active planning docs:**
- `scheme_tests/reports/larceny_triage.md` — **the open defect queue.** Start here
  for macro/hygiene work: the hygiene queue (families 36 and 38) closed
  2026-08-31 with the matrix at 28 of 28, but the doc still records what each
  step did, the acceptance criteria, and the approaches measured and rejected
  — and the non-hygiene families are still open. Two PRs were closed for
  skipping it.
- `PRD/MILESTONES.md` — project history and achievements
- `PRD/ARCHIVE/phase1_cleanup_2026_03/PHASE1_CLEANUP_PRD.md` — archived Phase 1 cleanup tracker
- `PRD/phase1/DELIMITED_CONTINUATIONS_DESIGN.md`
- `docs/GC_DESIGN.md` — the collector as built today, on both backends (Collector/RootProvider traits, root inventory, staging); GC is always on since GC_DESIGN's stage 4c (2026-08-03). Rewritten as the redesign's stages land
- `PRD/GC_PRD.md` — design and plan for the GC redesign: representation, MarkRegion collector, JIT contract, steady state and limits, threading readiness
- `PRD/study/` — research records; `PRD/study/gc/` is the GC redesign study behind `PRD/GC_PRD.md`
- `PRD/macro/SYNTAX_CASE_DESIGN.md` — syntax-case design, and the
  resolve-once-before-the-backends decision recorded for that rewrite
- `PRD/ARCHIVE/numeric_research/NUMERIC_SUMMARY.md` — canonical numeric tower guide

**Completed compatibility work:**

- `PRD/ARCHIVE/R7RS_LARGE_STATUS.md` — Red and Tangerine completed 2026-09-30
  with #577 and #578. Preserves dated library coverage and verification
  references; later R7RS-large work belongs in issues and the syntax-case design.
- `PRD/ARCHIVE/TRACK_L_LEFTOVERS.md` — Track L completed 2026-09-29 with #551.
  The final ledger preserves dated corpus and Larceny measurements, standing
  rules and separate follow-ups. The earlier working record is
  `PRD/ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md`; both are historical references,
  not active queues. New findings belong in GitHub issues.

**Feature docs:**
- `docs/README.md#library-bundling-policy` — **the standing bundling policy.**
  R7RS-large libraries (including drafts) and SRFIs are eligible, Patina's own
  extensions remain bundled, and libraries specific to another implementation
  stay external. Eligibility does not require immediate implementation. Check
  it before bundling anything; it also records provenance and test requirements.
- `docs/MACRO_SYSTEM.md` — macro system architecture (scope sets, flip-scope
  algorithm), and the two instruments for hygiene work: `PATINA_SCOPE_TRACE`
  (what scopes a binding actually gets, and how a reference resolved) and
  `crates/patina-tests/tests/hygiene_matrix.rs` (139 shapes in two tables — 28
  use-site binders scored against chibi and Racket, 111 generated and
  library-imported macros scored against chibi and Gauche — the scoreboard a
  hygiene fix is measured by)
- `docs/TEST_ORGANIZATION.md` — test structure and categories
- `docs/reference_impls/` — notes on Chibi, Chez, Gauche reference implementations

**VM backend docs (Phase 2A — complete):**
- `docs/VM_DECISIONS.md` — settled architecture decisions (master reference)
- `docs/VM_ISA.md` — instruction set architecture and semantics
- `docs/VM_COMPILER.md` — 2 pre-passes + 5-pass compiler pipeline
- `docs/VM_RUNTIME.md` — VmState, execution loop, control primitives, and the
  two instruments for control-flow work: §5.6's table of which dynamic state
  each transfer saves, restores or truncates, with the oracle panel (Chez,
  chibi, Gauche, Guile, Racket) that measured it, and its executable
  counterpart `crates/patina-tests/tests/control_flow_matrix.rs` — 24 transfer
  shapes, each naming the external implementations behind its expected answer.
  Read both before touching `dynamic-wind`, prompts or continuations: they are
  scoreboards, and a fix that improves one row while breaking another fails
  them, which is how this area's defects have usually arrived
- `docs/VM_TESTING.md` — testing layers and commands

**Reference implementations:** a local chibi-scheme checkout may be at
`~/Project/reference/chibi-scheme`; do not assume it exists on another machine.
- `tests/r7rs-tests.scm` — comprehensive R7RS test suite
- Chibi’s `lib/init-7.scm` (inside that external checkout) — R7RS procedures implemented in Scheme

## Architecture: Critical Rules

**TaggedValue is `Copy`** — 8 bytes, never allocates for fixnum/bool/char/null. Never wrap in `Box` or `Rc`. The old `Value` enum is gone completely.

**The heap is reached through a context, and only a driver collects** (#703, #702) — the safe crates read and allocate through `Cx<'gc>` (`patina_core::Cx`): reads take `&Cx` and allocation `&mut Cx`, so the borrow checker refuses an interior borrow held across an allocation, and a `Value<'gc>` cannot outlive the context it came from. A value written into a heap object goes through the store funnel (`Cx::store`), never through a cloned `Rc` payload. A primitive takes the context the machine makes for it (#706), and a host crosses its words into one (`NoGcScope::cross`). Only the holder of the carrier's `GcDriver` collects, and nothing beneath a context can name it: a loop entered while a context is live is nested, opens a `NoGcScope` and has no driver, so no call made beneath a context collects. Nothing outside the trusted crates (core, the VM, the tree-walker) keeps a `SharedHeap` across calls (#707): a loader borrows the heap for the call, and a host reaches it through its interpreter. Inside the trusted crates, which still borrow the heap's `RefCell` directly, never hold `borrow_mut()` across a call that might also borrow. Extract to a `let` first:
```rust
// WRONG — borrow_mut() lives through the if-let body:
if let Some(v) = heap.borrow_mut().method() { heap.borrow()... }
// CORRECT:
let v = heap.borrow_mut().method();
if let Some(v) = v { heap.borrow()... }
```

**`SourceLocation::source` is `Arc<str>`** (not `Rc`) — required by `Backend::Error: Send + Sync + 'static`.

**Macros vs special forms** — `let`, `cond`, `case`, `do`, `and`, `or`, `when`, `unless`, `case-lambda`, `define-record-type` are macros in `lib/scheme/base/*.scm` and `.sld` files. The CoreExpr IR has 13 variants: `Literal`, `Var`, `Quote`, `Quasiquote`, `Lambda`, `If`, `Set`, `Begin`, `Define`, `Import`, `Expand`, `App`, `Apply`. `define-syntax` is compiled during desugaring — there is no `DefineSyntax` CoreExpr variant.

**Library primitives** must be registered in both the primitive registry AND the library builder in `patina-runtime/src/stdlib/internal_<name>.rs`.

**An import installs a binding, not a value** — the importer and the library share one location, so what the library assigns later the importer sees (R7RS §5.2, #406). Resolve modifier names with `ImportSet::resolve_bindings` in `crates/patina-runtime/src/import_set.rs`, then install each original export with `Library::import_into` (through `import_export` on the VM, to preserve primitive-shadow invalidation). Program/library imports, `eval`/`load`, and `environment` share that policy; a new path must too. Never use `env.define(name, export_value)`, which freezes a copy. See `docs/TEST_ORGANIZATION.md`, “Import modifier policy and context coverage”, for the policy table. Primitives registered from Rust share like everything else: chibi and Gauche agree that a program's `(set! list-copy …)` reaches the libraries that imported it, and copying them left a re-exporting library's importers stale (`Owner` in `patina-core/src/environment.rs` has the measurement).

**A fast path keys on the binding, never on the spelling** — nothing in Patina decides what a call means from the *name* it was written with: `apply`'s lowering (#443), the tree-walker's `call/cc` (#441) and the VM's `call-with-values` and `dynamic-wind` sequences (#442) each ask what the name is bound to, the last behind a guard that falls back to an ordinary call once the procedure is rebound. So do the machines' control primitives: each picks `call/cc`, `force` and the rest by the qualified name of the binding they came from, so a host's primitive of the same name in a library of its own is an ordinary primitive (#710). So do the backends before they hand a top-level form to the library loader: `define-library` and `library` head a library only while unbound (#610, `patina_frontend::is_define_library_form`), and a bare `import` is loaded by the backend only while its head is bound to the `import` marker (#677, `patina_frontend::top_level_import_sets`). Keep it so. The desugarer renames a library macro's *references* to imported procedures where it emits them (#438, `Desugarer::early_bound`), so a recogniser keyed on the spelling misses them, and one that ignores the binding misses a program's own definition. `patina_core::by_spelling`, which exempted such names from the renaming, went with the last of them.

**A procedure that calls back into the program is written in Scheme, or runs the procedure as a frame of the machine** — a Rust primitive's frame cannot be part of a continuation. A continuation captured inside a procedure the program passed it (a comparator, `call-with-port`'s procedure) and re-entered after the primitive returned has nothing to return into: the VM answered stray internal values, the tree-walker abandoned the form, where chibi and Gauche resume the call (#471). So `map`, `for-each`, `member`/`assoc` with a comparator, `call-with-port` and the file variants are Scheme (`lib/scheme/base/higher_order.scm`, `lib/scheme/file/`), over primitives for the part that calls nothing back, and a new procedure that takes a procedure argument is too. So are `parameterize`'s install and restore of a procedure a program wrote to act as a parameter: `%parameterize-swap!` calls nothing from Rust, and for a list with one has the machine call `%parameterize-each` (`lib/scheme/base/parameters.scm`, read and evaluated on first use; #784, #817). Where Scheme costs too much, the machine runs the callee: the VM in a stub frame as its control primitives do — `force` (#476): Scheme cost 80% on the VM, the stub 8–9%; the tree-walker's `force` is native CPS — or, for a primitive of either backend, a resumable primitive (`PrimitiveFn::new_resumable`) returns `patina_primitives::Step::Call` and is resumed with the result, the VM running the call in `resume_stub`'s frame and the tree-walker under a `ResumePrimitive` continuation — the parameter converters' `make-parameter`, `%parameter-convert` and `%parameter-set!` (#478), and `(patina internal io)`'s `call-with-input-file` and `call-with-output-file`, made resumable so that their opens collect and retry (#607). `eval` and `load` return `Step::Eval`, a datum for the machine to evaluate: the VM compiles it into a closure the stub frame calls, the tree-walker runs it on the trampoline it is on (#477). Every procedure above now runs its callee as Scheme or as a frame of the machine, but some primitives still call back into the program from Rust, on a nested loop through `ApplyContext::apply_in`, beneath their context (#706). A program reaches them only by importing them: the internal libraries' originals that the Scheme versions replaced, `(patina internal lists)`'s `member`/`assoc` with a comparator and `(patina internal io)`'s `call-with-port`. The cost of the Scheme ones, measured 2026-09-24 on the VM: a loop of nothing but two-argument `member`/`assoc` 19% slower, one of comparator `member` 75%; the chibi suite unchanged.

**A comment that argues "no safe point here" comes with an `AssertNoGc` at the same lines** — a window whose soundness rests on where the GC safe points are, rather than on a root (a value held only in a Rust local until the next write, a weak-table entry and its handle not yet both reachable), is asserted, not only argued. Open a `patina_core::AssertNoGc` over it: every poll site panics while one is open, in debug and `gc-check` builds, even in a nested loop that could not have collected (#624). A window that closes partway through a callee is passed to it by value and dropped where it closes, as the VM's `push_wind_step` does before it calls the thunk. Only the holder of the carrier's `GcDriver` collects, and only with no `NoGcScope` open (#702): every nested entry into a loop opens one and has no driver. A Rust scope or value that holds heap values across a call that can evaluate defers with `GcDeferGuard::holding`, a `NoGcScope` whose drop panics if a collection ran inside it. The exception is a primitive: `patina-primitives` has no `GcDeferGuard`, and the values a primitive holds across `ApplyContext::apply_in` (a comparator `member`'s list) are its context's, which the `NoGcScope` of the nested loop that runs the call keeps live, or, on the tree-walker's detached `ApplyContext for Evaluator`, which no loop runs above, the holder's guard each of its methods takes (`docs/GC_DESIGN.md` §7). Review a "no safe point" comment without the scope as a missing assertion. See `docs/GC_DESIGN.md` §7.

**A walk of code grows its stack, and holds the code to a depth** — every stage between the reader and a backend walked code by recursing on it, a native frame or more per level and no check, so code nested about 1,000 deep overflowed the main thread's stack and aborted the process on both backends, from 243 levels of `let` on a 2 MiB thread (#617). A recursive walk of what a program wrote — a datum, a macro's pattern or template, `CoreExpr`, `CpsExpr`, the VM compiler's trees — calls `patina_core::walk::ensure_sufficient_stack` at its recursive entry (`ensure_sufficient_stack_if` where most nodes are leaves), which moves it to a new segment when the stack runs low; a tree type's `Drop` asks `has_stack_room` and hands its children to `drop_deep` when there is none; and a walk up a chain, as an environment's parents, is a loop. Where the walk meets the program's code it also refuses a form nested past `walk::MAX_FORM_DEPTH` with a syntax error a program can catch: the desugarer counts the forms it is inside (`open_forms`), the macro expander what is left of the limit below the use it expands (`Expander::at_depth`), and the macro compiler, the quasiquote derivation, `cond-expand` requirements, import sets and library declarations count their own nesting. A new walk of code does the same. Data a program builds is not code: `quote` keeps a datum of any depth, and only a walk of code is held to the limit.

**A host keeps a value through a handle** — a `TaggedValue` roots nothing once the evaluation that answered it returns, so a later evaluation that collects can free it (#605). The embedding API answers `Owned` handles (the `eval_*_owned` methods, `Interpreter::lookup`), which the heap's handle table (`patina-core/src/heap/handles.rs`) roots until each is dropped; read one with `display_tagged(&value)` or `raw_value(&value)`. A host makes values in `Interpreter::with`, whose context lets one leave only as a handle (`cx.hold`), and calls a procedure with `Interpreter::call`, a driver entry over handles, which applies it as a top-level form without consulting a binding. It registers a primitive of its own with `Interpreter::register_primitive` or `register_resumable`, which export it from a library of the host's for a program to import, and charges what it holds outside the heap for its values through `Interpreter::external_bytes` (#710). An environment the host builds is held the same way, by the `OwnedEnvironment` that `Interpreter::new_environment` answers (#620); one from `Environment::with_parent` is not rooted between calls. A host binds through the interpreter too: `Interpreter::define` and `define_in` go through `Backend::define`, which on the VM marks the primitive a definition rebinds, as the `Define` instruction does; `Environment::define` and `set` write the binding alone, which VM code compiled while the name was a primitive's never sees (#673). New code calls the `_owned` forms; the bare-value `eval_*` methods are deprecated, and a test file that still calls them carries `#![allow(deprecated)]`. The suffix is temporary: at the GC redesign's stage 5e the plain names answer handles (`PRD/GC_PRD.md`, decision 13).

**Error formatting** — use `format_interpreter_error(&e, &source_map.borrow())` (from `patina-interpreter`) rather than `e.to_string()` to get caret-style source context and macro expansion chain.

## When Adding Features

**New primitive:**
1. Implement shared primitives in `crates/patina-primitives/src/primitives/<category>.rs`. A primitive takes the context (`fn(&mut Cx<'gc>, &[Value<'gc>])`, registered with `PrimitiveFn::new_cx`), and reaches the heap only through it; a store goes through the funnel (`Cx::store`). `docs/VM_DECISIONS.md` §6 has the signatures (#706).
2. Register it after every primitive already registered: at the end of the `register_last()` that `primitives/mod.rs::register_all()` calls last, after `debug::register`, when that function is its category’s (today it is `parameters::register_last`), and otherwise in a function of its category’s that `register_all()` calls after it. Never append to a `register_last()` that another one follows: a primitive registered among the others moves every later one’s index, and a shift of one cost `string-ports` about 1% in cycles at the same instructions (#608).
3. Export from the library builder in `crates/patina-runtime/src/stdlib/internal_<name>.rs`.
4. For control primitives, check each backend’s dispatch/ApplyContext implementation; test both backends.
5. If it calls a procedure the program passed it, it does not make the call from Rust: write it in Scheme over a primitive, or where that costs too much make it resumable (`PrimitiveFn::new_resumable`), handing each call to the machine as a `Step::Call` (see "A procedure that calls back into the program" above).
6. If it needs a collection at its own call — `(gc)`, an open retried after `EMFILE` (#607) — it is resumable and answers `Step::Collect`, with what it needs afterwards in the step's `state`; the machine suspends the call and collects where a safe point may, or posts and counts the collection where collection is deferred (#639, `docs/GC_DESIGN.md` §7). `Heap::request_gc` only arms the next safe point.

**New Rust call into the evaluator, or environment built from Rust** (#622) — anything the workspace `clippy.toml` lists under `disallowed-methods`: `ApplyContext::apply_proc` and `eval_expr`, `run_synchronously`, the VM's `execute`, `execute_nested`, `run_loop_until`, `run_loop_until_outcome` and `across_reentry`, the tree-walker's trampoline entries and `eval`'s expansion, library loading on either backend, `Backend::eval*` and `call`, `Interpreter::eval_*` and `call`, the deprecated `Pipeline` and `SimpleInterpreter` adapters, and `Environment::with_parent`. A call made beneath a context is not on the list, since the brand covers it: a higher-order primitive's `ApplyContext::apply_in` and `load_scheme_library` take its context, so it holds only values of it across them and no borrow (#709), and nothing beneath a context collects (#707).
1. Prefer not to make one: a primitive hands the call to the machine (`Step::Call`, `Step::Eval`; see "A procedure that calls back into the program" above).
2. Otherwise put `#[expect(clippy::disallowed_methods, reason = "…")]` on the narrowest `let`, match arm or statement around the call, never `allow`. A tail call is bound in a `let` that carries the `expect` and returned (clippy's `let_and_return` skips a `let` with attributes, measured on 1.97.1); only a function whose one statement is the call, a wrapper, takes the `expect` on the function, since there it covers nothing else. The reason says what the frame holds across the call and why that is safe: it holds nothing it reads afterwards; a root reaches it (a library being loaded is rooted by its registry entry, #677); a guard on the data protects it (`GcDeferGuard::holding`, as `ParsedLibrary` takes), or the data are values of a context, beneath which nothing collects (`NoGcScope::cross`, as an expansion's are, #704); the state is in the machine (a pushed frame, `Step::Call`, `resume_stub`); or the call runs on a nested loop that defers. A site that is none of these says so.
3. A function that runs Scheme from Rust goes on the list when a caller can hold heap values across it outside a boundary that already carries a reason; otherwise the `expect` inside it suffices. So `expand_for_eval` and `eval_step` are listed, for `resumable_step`, which holds a primitive's state and the step's stacks across them, while the registry's `apply_*` methods are not (their callers are the dispatch loops and the detached context, each with its reason, and a primitive calls back beneath its context), nor are the constructors (bootstrap on a fresh heap). A listed public entry also goes into `crates/patina-interpreter/src/reentry_lint_control.rs`, whose `expect`s fail clippy when an entry stops matching.
4. A new `thread_local!` goes on its crate root's `#![expect(clippy::disallowed_macros)]` list with what it holds. Clippy takes that lint only at a crate root, so once a crate has one, the lint passes every later one; `every_thread_local_is_listed_at_its_crate_root` in `reentry_lint_control.rs` fails until the static is named there.

Test crates, the REPL and test and example targets are exempt (`clippy.toml`'s header). `docs/GC_DESIGN.md` §7 has the reasoning and the sites that are not safe today.

**New Scheme library:**
- Internal Rust primitives: `crates/patina-runtime/src/stdlib/internal_<name>.rs`
- Library definition: `lib/scheme/<name>.sld`
- Scheme implementations: `lib/scheme/<name>/<file>.scm`

**New heap object type:**
1. Add variant to `HeapObjectData` in `crates/patina-core/src/heap/mod.rs`. A payload that changes after the object is made goes in an `Interior`, which only core can borrow: a store into it is a funnel slot (`Cx::store`), and a read an accessor that answers by value (#708). A payload holding an `Interior`, `Rc`, `RefCell` or `Cell` is shared state that stage 5 takes out, and goes on `SHARED_PAYLOADS` in `crates/patina-interpreter/src/reentry_lint_control.rs`, whose test fails until it is listed (C9, #711).
2. Add type predicate + accessor on `Heap`
3. Add display in `crates/patina-core/src/debug_format.rs`
4. Update GC tracing/root handling and test collection in both backends; read `docs/GC_DESIGN.md`.
5. Trace it by name (#623, `docs/GC_DESIGN.md` §5.4): its arm in
   `trace_object_children`, and any trace function it calls, takes every
   field apart with no `..` and no catch-all arm, and a field or payload
   that holds no value is written `field: _` or `Variant(_)` with a comment
   saying what it holds instead.
   `scripts/check_gc_trace_names.py` enforces both and lists the trace
   functions; a new one joins the list. Then add a sentinel test that builds
   the variant by a literal with a fresh value in each traced field, reachable
   only through it, collects, and checks each survived (`heap::sentinels`) —
   and run it once with the trace line deleted, to see it fail (#164's rule).
   The same two rules hold for a new field in any traced struct or root
   provider (`VmState`, `Environment`, `CpsContinuation`, …): a destructure
   cannot judge whether a field needs tracing, and a sentinel can. A
   structure outside the heap derives `Trace` instead of a hand-written
   trace (`patina_core::Trace`, #686): the derive takes it apart by name, a
   field whose type has no `Trace` does not compile, and a field that holds
   no value is `#[trace(skip, reason = "…")]`. Its sentinel test is the
   same, run once with the field marked `skip`.

**New CoreExpr form** (rare — prefer macros):
1. Extend `CoreExprKind` in `patina-core/src/core_expr.rs`
2. Add desugaring in `patina-frontend/src/desugarer/mod.rs`
3. Update visitors/CPS lowering in `patina-ir/src/` and evaluation in `patina-tree-walker/src/eval/cps_eval/`.
4. Update compilation in `patina-vm/src/compiler/` and runtime handling as needed.
5. Test both backends.

## Error Types by Layer

| Layer | Type | Crate |
|-------|------|-------|
| Lexer | `LexError` | patina-frontend |
| Parser | `ParseError` | patina-frontend |
| Desugarer | `DesugarError` | patina-frontend |
| Tree-walker | `EvalError` | patina-tree-walker |
| VM | `VmError` | patina-vm |
| Interpreter | `InterpreterError<E>` | patina-interpreter |

## Working across agents

- Read `git status` before editing; preserve user and other-agent changes.
- Use separate worktrees when Codex and Claude Code work concurrently. Avoid
  sharing a build target when either session may clean it.
- For handoffs, record the task, branch/worktree, changed files, checks run, and
  remaining work in the conversation or an existing planning doc. Tool session
  history and private memory are not a shared project specification.
- Keep durable decisions in the existing design/planning docs linked above.
  Treat dated counts, timing measurements, and archived plans as historical context;
  confirm current behavior in source and tests.
