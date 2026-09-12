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
    ├── patina-core/        # TaggedValue, Heap, Environment, CoreExpr, CpsExpr, scope sets
    ├── patina-runtime/     # Backend trait, LibraryRegistry, internal stdlib primitives
    ├── patina-ir/          # ExprVisitor, CPS transform, re-exports CoreExpr types
    ├── patina-frontend/    # Lexer, Parser, Desugarer, SourceMap
    ├── patina-macros/      # syntax-rules with Racket-style scope-set hygiene
    ├── patina-pipeline/    # StandardPipeline orchestration
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

# Specific crate
cargo test --package patina-frontend

# Lint / format
cargo clippy --all-targets --all-features -- -D warnings
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

`.github/workflows/ci.yml` is the source of truth for the full gate:

| Job | What it runs |
|---|---|
| Test Suite | `cargo test --all --lib --tests` on **ubuntu and macos** (`SKIP_CHIBI_TESTS=1`) |
| R7RS Compliance | `run_chibi_tests.sh` **and** `run_chibi_tests_tree_walker.sh` |
| GC differential | `run_gc_differential.sh` on release **and** on debug with poison assertions |
| Rustfmt / Clippy | `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings` |
| Suite oracles | `run_suite_oracles.sh` under chibi 0.12 and Gauche 0.9.15, pinned and built from source, against `DIVERGENCES.tsv` |

For GC/rooting changes, also run `scripts/run_gc_differential.sh` against release
and debug builds; the debug lane enables poison assertions. For backend semantics,
run both chibi backend scripts. Check CI results when a branch is pushed; a push
alone does not establish that checks passed.

For measured build timings and stale `target/` troubleshooting, see
`docs/TEST_ORGANIZATION.md`, “Local build performance”. Timings are machine-specific.

## Documentation

**Do not create new markdown files without user approval.** A user request to
create or restructure agent instruction files authorizes the files needed for that task.
Prefer updating existing docs for other work.

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
- `PRD/ARCHIVE/phase1_cleanup_2026_03/PHASE1_CLEANUP_PRD.md` — archived Phase 1 cleanup tracker
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

**Reference implementations:** a local chibi-scheme checkout may be at
`~/Project/reference/chibi-scheme`; do not assume it exists on another machine.
- `tests/r7rs-tests.scm` — comprehensive R7RS test suite
- Chibi’s `lib/init-7.scm` (inside that external checkout) — R7RS procedures implemented in Scheme

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
1. Implement shared primitives in `crates/patina-primitives/src/primitives/<category>.rs`.
2. Register in the category’s `register()` and ensure it is called by `primitives/mod.rs::register_all()`.
3. Export from the library builder in `crates/patina-runtime/src/stdlib/internal_<name>.rs`.
4. For control primitives, check each backend’s dispatch/ApplyContext implementation; test both backends.

**New Scheme library:**
- Internal Rust primitives: `crates/patina-runtime/src/stdlib/internal_<name>.rs`
- Library definition: `lib/scheme/<name>.sld`
- Scheme implementations: `lib/scheme/<name>/<file>.scm`

**New heap object type:**
1. Add variant to `HeapObjectData` in `crates/patina-core/src/heap/mod.rs`
2. Add type predicate + accessor on `Heap`
3. Add display in `crates/patina-core/src/debug_format.rs`
4. Update GC tracing/root handling and test collection in both backends; read `docs/GC_DESIGN.md`.

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
