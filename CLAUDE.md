# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

Patina is an R7RS-small Scheme interpreter written in Rust. The default backend is a register-based bytecode VM (`patina-vm`). A CPS tree-walking interpreter (`patina-tree-walker`) is also available via `--tree-walker`. Both backends pass 1226/1226 chibi R7RS tests.

All runtime values are `TaggedValue` — NaN-boxed 8-byte `Copy` types. No `Value` enum exists (fully removed). Macros and derived forms (`let`, `cond`, `do`, etc.) are implemented in Scheme (`lib/scheme/base/*.scm`), not as special forms.

## Workspace Structure

```
patina/
├── lib/scheme/             # R7RS .sld library files + .scm macro implementations
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
    ├── patina-tests/       # ~1400 integration and compliance tests
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

### CI runs the full gate — don't serialize it locally

`.github/workflows/ci.yml` runs on **every push to main and every PR**, in
3–5 minutes, and covers more than a local run does:

| Job | What it runs |
|---|---|
| Test Suite | `cargo test --all --lib --tests` on **ubuntu and macos** (`SKIP_CHIBI_TESTS=1`) |
| R7RS Compliance | `run_chibi_tests.sh` **and** `run_chibi_tests_tree_walker.sh` |
| GC differential | `run_gc_differential.sh` on release **and** on debug with poison assertions |
| Rustfmt / Clippy | `cargo fmt --check`, `clippy --all-targets --all-features -D warnings` |

The GC lanes and the macOS/Linux split have no local equivalent that anyone
runs by hand, so pushing is *stronger* verification than the commands above,
not weaker.

**So run locally only what tells you whether your specific change works** — a
release build, the repro you are chasing, and the test file you touched — then
push and let CI do the matrix. Check it with `gh run list --limit 3` or
`gh run watch`.

Run the full local gate when the answer has to be in hand before the run
finishes: writing a PR description that states a result, bumping
`rust-toolchain.toml`, or a change whose blast radius you cannot bound (a
`CoreExpr`/`CpsExpr` variant, `Environment`, the desugarer). Otherwise
re-running `cargo test --all` after each edit costs 5–10 minutes an iteration
to learn what a push would have told you anyway.

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
  + runtime-forced + demanded legacy aliases; leaf libraries stay out) that
  Track L's L1 defers to. Check it before bundling anything.
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
- `docs/VM_RUNTIME.md` — VmState, execution loop, control primitives
- `docs/VM_TESTING.md` — testing layers and commands
- The two instruments for control-flow work, and the thing to read before
  touching `dynamic-wind`, prompts or continuations: `docs/VM_RUNTIME.md` §5.6
  (which dynamic state each transfer saves, restores or truncates, measured
  against six implementations) and
  `crates/patina-tests/tests/control_flow_matrix.rs` (20 transfer shapes
  scored against five). Both are scoreboards — a fix that improves one row and
  breaks another fails them, which is how this area's defects have usually
  arrived.

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
