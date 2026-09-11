# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

Patina is an R7RS-small Scheme interpreter written in Rust. The default backend is a register-based bytecode VM (`patina-vm`). A CPS tree-walking interpreter (`patina-tree-walker`) is also available via `--tree-walker`. Both backends pass 1226/1226 chibi R7RS tests.

All runtime values are `TaggedValue` — NaN-boxed 8-byte `Copy` types. No `Value` enum exists (fully removed). Macros and derived forms (`let`, `cond`, `do`, etc.) are implemented in Scheme (`lib/scheme/base/*.scm`), not as special forms.

## Workspace Structure

```
patina/
├── lib/scheme/             # R7RS .sld library files + .scm macro implementations
├── test-lib/               # third-party libraries the test lanes supply with
│                           # `-A`, NOT bundled — see test-lib/README.md
└── crates/
    ├── patina-core/        # TaggedValue, Heap, Environment, CoreExpr, CpsExpr, scope sets
    ├── patina-runtime/     # Backend trait, LibraryRegistry, internal stdlib primitives
    ├── patina-ir/          # ExprVisitor, CPS transform, re-exports CoreExpr types
    ├── patina-frontend/    # Lexer, Parser, Desugarer, SourceMap
    ├── patina-macros/      # syntax-rules with Racket-style scope-set hygiene
    ├── patina-pipeline/    # StandardPipeline orchestration
    ├── patina-tree-walker/ # Tree-walking Backend (CPS evaluator, primary backend)
    ├── patina-interpreter/ # High-level Interpreter<B: Backend> API
    ├── patina-repl/        # rustyline REPL + script runner binary
    ├── patina-tests/       # integration tests, and the tests/scheme suite files
    └── patina-compat/      # third-party compatibility harness over compat/vendor/ (Track L)
```

**Dependency flow:**
```
patina-repl → patina-interpreter → patina-tree-walker → patina-runtime → patina-core
                                 ↗  patina-frontend    ↗                ↗
                                    patina-pipeline                    /
                                    patina-macros ─────────────────────
                                    patina-ir ─────────────────────────
patina-tests → patina-interpreter
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

# Specific crate
cargo test --package patina-frontend

# Lint / format
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt
```

### CI runs the full gate; a healthy local build is cheap too

`.github/workflows/ci.yml` runs on **every push to main and every PR**, in
3–5 minutes, and covers more than a local run does:

| Job | What it runs |
|---|---|
| Test Suite | `cargo test --all --lib --tests` on **ubuntu and macos** (`SKIP_CHIBI_TESTS=1`) |
| R7RS Compliance | `run_chibi_tests.sh` **and** `run_chibi_tests_tree_walker.sh` |
| GC differential | `run_gc_differential.sh` on release **and** on debug with poison assertions |
| Rustfmt / Clippy | `cargo fmt --check`, `clippy --all-targets --all-features -D warnings` |
| Suite oracles | `run_suite_oracles.sh` under chibi 0.12 and Gauche 0.9.15, pinned and built from source, against `DIVERGENCES.tsv` |

The GC lanes, the macOS/Linux split and the suite oracles have no local
equivalent that anyone runs by hand, so pushing is *stronger* verification
than the commands below, not weaker. Check it with `gh run list --limit 3` or
`gh run watch`.

**Locally, the whole Rust gate costs well under a minute — when `target/` is
healthy.** Measured 2026-09-11 on 10-core Apple silicon, 51 test binaries,
after touching `patina-vm/src/runtime/vm_state.rs`, which every test binary
links, so this is the worst realistic case:

| Command | Time |
|---|---|
| `cargo build --release` — the repro, the chibi lanes, the benchmarks | **3.2 s** |
| `cargo test -p patina-tests --test <one file> --no-run` | **0.5–5.5 s** |
| `cargo test --all --lib --tests` | **2.5 s** to build, **29 s** to run |
| `cargo clippy --all-targets --all-features` | **1.3 s** |
| any of them again with no edit in between | 0.1 s |
| from `cargo clean`: release, all tests, clippy | 11 s, 28 s, 6 s |

So run the full local gate whenever you want the answer before CI has it —
writing a PR description that states a result, bumping `rust-toolchain.toml`,
or a change whose blast radius you cannot bound. Clippy and `cargo test` do not
evict each other's artifacts, and the workspace's only non-default feature
(`patina-tree-walker/verbose-tracing`) gates no code, so neither is a reason
to rebuild (measured 2026-09-06).

**When those numbers are 100× worse, `target/` has rotted: run `cargo clean`.**
On macOS, cargo's default `split-debuginfo = "unpacked"` leaves each test
binary's debug info in its object files beside it in `target/debug/deps`, and
nothing deletes the old ones, so every full test build leaves about 1,100
`*.rcgu.o` files behind. By 2026-09-11 the main checkout held 1.9 million of
them (69 GB on disk). rustc scans that directory on every invocation, so a
full test rebuild took **220 s** and clippy **220 s** where a healthy tree
takes 2.5 s and 1.3 s. Deleting `target/debug/incremental` alone did not help.

That rot, not the number of test binaries, is what this section used to
measure — 493 s for `cargo test`, 580 s for clippy, explained as "87 binaries
× ~6 s". An A/B on one machine settled it: in fresh target directories the
tree before #193 rebuilt its 87 test binaries in 4 s and today's tree its 51 in
2 s, while today's tree took 220 s in the rotted directory. A test binary costs
about 0.05 s per full rebuild.

Check with `find target/debug/deps -name '*.o' | wc -l`: thousands are
normal, a million is the state above. `cargo clean` took 256 s to delete 1.9
million files, and everything rebuilds from scratch in about 45 s. The root
fix would be `split-debuginfo = "packed"` in `[profile.test]`, which leaves no
object files, but `dsymutil` then runs for every binary and a full test
rebuild takes 9.8 s instead of 2.5 s — measured, not adopted.

**#193's outcome, corrected.** It took the workspace from 87 test binaries to
**51** (`find crates -path '*/tests/*.rs' -not -path '*/tests/*/*' | wc -l`),
across 56 suite files and 1757 rows. Its build-time case was the rotted
directory, so the binaries it removed saved about 2 s a rebuild, not minutes.
On a healthy target a test is cheap to add in either form: a row in an
existing `.scm` file needs no rebuild (0.1 s), a new `.scm` file needs a
`SUITE` entry and so recompiles the driver (1.0 s), and a new `.rs` file
compiles and links in 0.4 s. What #193 bought is the rest of its case: one
suite that every backend runs, and chibi and Gauche arbitrating every row
through `crates/patina-tests/tests/scheme/DIVERGENCES.tsv`.

## Documentation

**Do not create new markdown files without user approval.**

**Active planning docs:**
- `scheme_tests/reports/larceny_triage.md` — **the open defect queue.** Start here
  for macro/hygiene work: the hygiene queue (families 36 and 38) closed
  2026-08-31 with the matrix at 28 of 28, but the doc still records what each
  step did, the acceptance criteria, and the approaches measured and rejected
  — and the non-hygiene families are still open. Two PRs were closed for
  skipping it.
- `PRD/phase2/R7RS_LARGE_STATUS.md` — **the bundling policy and edition
  tracker.** The answer to "does Patina ship this library, and why (not)":
  Red 16/17, Tangerine 4/8 as of 2026-09-01, with the policy (standard-track
  + runtime-forced + demanded legacy aliases + the standard testing API —
  SRFI 64, added 2026-09-06 and not shipped until #193's Phase 0; other leaf
  libraries stay out) that Track L's L1 defers to. Check it before bundling
  anything.
- `PRD/MILESTONES.md` — project history and achievements
- `PRD/PHASE1_CLEANUP_PRD.md` — Phase 1 cleanup tracker (Priorities 1–5 status)
- `PRD/phase1/DELIMITED_CONTINUATIONS_DESIGN.md`
- `docs/GC_DESIGN.md` — garbage collection design for both backends (Collector/GcRoots traits, root inventory, staging); GC is always on since stage 4c
- `PRD/future/GC_STAGE5_PRD.md` — remaining GC pause work (weak continuation tables, immortal roots, nested-loop collection, generational)
- `PRD/macro/SYNTAX_CASE_DESIGN.md` — syntax-case design, and the
  resolve-once-before-the-backends decision recorded for that rewrite
- `PRD/ARCHIVE/numeric_research/NUMERIC_SUMMARY.md` — canonical numeric tower guide

**Feature docs:**
- `docs/MACRO_SYSTEM.md` — macro system architecture (scope sets, flip-scope
  algorithm), and the two instruments for hygiene work: `PATINA_SCOPE_TRACE`
  (what scopes a binding actually gets, and how a reference resolved) and
  `crates/patina-tests/tests/hygiene_matrix.rs` (28 shapes scored against chibi
  and Racket — the scoreboard a hygiene fix is measured by)
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

**Reference implementations:** chibi-scheme at `~/Project/reference/chibi-scheme`
- `tests/r7rs-tests.scm` — comprehensive R7RS test suite
- `lib/init-7.scm` — R7RS procedures implemented in Scheme

## Architecture: Critical Rules

**TaggedValue is `Copy`** — 8 bytes, never allocates for fixnum/bool/char/null. Never wrap in `Box` or `Rc`. The old `Value` enum is gone completely.

**RefCell borrow discipline** — never hold `borrow_mut()` across any call that might also borrow. Extract to a `let` first:
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

**Error formatting** — use `format_interpreter_error(&e, &source_map.borrow())` (from `patina-interpreter`) rather than `e.to_string()` to get caret-style source context and macro expansion chain.

## When Adding Features

**New primitive:**
1. Implement in `crates/patina-tree-walker/src/eval/primitives/<category>.rs`
2. Register in `primitives/mod.rs::install_primitives()`
3. Export from library builder in `crates/patina-runtime/src/stdlib/internal_<name>.rs`

**New Scheme library:**
- Internal Rust primitives: `crates/patina-runtime/src/stdlib/internal_<name>.rs`
- Library definition: `lib/scheme/<name>.sld`
- Scheme implementations: `lib/scheme/<name>/<file>.scm`

**New heap object type:**
1. Add variant to `HeapObjectData` in `crates/patina-core/src/heap/mod.rs`
2. Add type predicate + accessor on `Heap`
3. Add display in `crates/patina-core/src/debug_format.rs`

**New CoreExpr form** (rare — prefer macros):
1. Extend `CoreExprKind` in `patina-core/src/core_expr.rs`
2. Add desugaring in `patina-frontend/src/desugarer/mod.rs`
3. Add evaluation in `patina-tree-walker/src/eval/core_eval.rs`
4. Add visitor method to `ExprVisitor` in `patina-ir/src/visitor.rs`

## Error Types by Layer

| Layer | Type | Crate |
|-------|------|-------|
| Lexer | `LexError` | patina-frontend |
| Parser | `ParseError` | patina-frontend |
| Desugarer | `DesugarError` | patina-frontend |
| Evaluator | `EvalError` | patina-tree-walker |
| Interpreter | `InterpreterError<E>` | patina-interpreter |

## Current Status and Future Phases

**Phase 1 complete** — Tree-walker: 100% R7RS compliance.

**Phase 2 complete** — Bytecode VM (`patina-vm/`) is the default backend. 1226/1226 R7RS tests pass.
**Phase 3:** `syntax-case` procedural macros — see `PRD/macro/SYNTAX_CASE_DESIGN.md`.
**Phase 4:** Gradual typing (Typed Racket-style).
**Phase 5+:** Reactive streams, miniKanren logic programming.
