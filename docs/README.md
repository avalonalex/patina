# Patina Documentation

Documentation for **completed and implemented features** in Patina.

## Working with coding agents

The repository uses one shared instruction source:

```text
AGENTS.md       Shared project rules, architecture constraints, and commands
CLAUDE.md       Claude Code entry point; imports @AGENTS.md
docs/           Detailed design and testing references, read when relevant
PRD/            Planning and decision history, kept high level; work items are GitHub issues
```

Edit [AGENTS.md](../AGENTS.md) when a rule should apply to both tools. Codex
discovers that filename natively; Claude Code loads it through the import in
[CLAUDE.md](../CLAUDE.md). This avoids maintaining two copies. Keep startup
instructions concise and put detailed explanations in existing documentation.
See the official [Codex instruction discovery guide](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
and [Claude Code memory guide](https://code.claude.com/docs/en/memory).

If a subsystem eventually needs separate instructions, put an `AGENTS.md` and a
thin `CLAUDE.md` containing `@AGENTS.md` in that directory. Add these only for
rules specific to that subtree. Discovery differs: Codex builds its startup chain
from the repository root to the working directory, while Claude also loads nested
files when it reads that subtree. Keep critical cross-cutting rules in the root;
when working from the root, explicitly read any relevant nested instructions.
Codex's default combined instruction limit is 32 KiB, and an `AGENTS.override.md`
takes precedence over `AGENTS.md` in the same directory.

When switching tools:

- Start a fresh session from this repository and ask it to summarize the loaded
  project instructions and verification commands. In Claude Code, `/memory`
  helps inspect the loaded instruction files. Check that both see the shared rules.
- Carry over a short handoff: task, branch/worktree, changed files, test results,
  and next steps. Private memory and session checkpoints are not portable task state.
- Use separate Git worktrees for simultaneous editing, and inspect the diff before
  handing work over. Share durable decisions through existing project docs.
- Configure and verify each tool's permissions and integrations separately.
  Sharing Markdown does not migrate hooks, MCP configuration, or tool permissions.
- Keep personal settings and session files out of commits.
  `.claude/settings.local.json` is ignored and removed from tracking; local copies
  remain usable. This does not remove earlier committed versions from history.
  Put intentional team settings in `.claude/settings.json` after reviewing them.
- Reference Scheme checkouts under `~/Project/reference/` are machine-specific.
  Follow the scripts' setup instructions and report missing external comparisons.

## Contents

| Document | Description |
|----------|-------------|
| [MACRO_SYSTEM.md](MACRO_SYSTEM.md) | Macro system architecture (syntax-rules, hygiene, scope sets) |
| [VFS_DESIGN.md](VFS_DESIGN.md) | Filesystem abstraction, public `(patina filesystem)` API, and current boundaries |
| [TEST_ORGANIZATION.md](TEST_ORGANIZATION.md) | Test structure, running tests, and test guidelines |
| [reference_impls/](reference_impls/) | Notes on reference Scheme implementations (Chibi, Chez, Gauche, Koka) |

### VM Backend (Phase 2A — complete)

| Document | Description |
|----------|-------------|
| [VM_DECISIONS.md](VM_DECISIONS.md) | Settled architecture decisions (master reference) |
| [VM_ISA.md](VM_ISA.md) | Instruction set architecture and semantics |
| [VM_COMPILER.md](VM_COMPILER.md) | 2 pre-passes + 5-pass compiler pipeline |
| [VM_RUNTIME.md](VM_RUNTIME.md) | VmState, execution loop, control primitives |
| [VM_TESTING.md](VM_TESTING.md) | Testing layers and commands |

## Library bundling policy

**Bundle R7RS-large libraries (including drafts) and SRFIs. Keep libraries specific to another
Scheme implementation external.** Owner decision, 2026-09-12; this replaces the earlier restriction
to edition members plus selected exceptions.

- **R7RS-large draft membership or being a SRFI is sufficient for eligibility.** A SRFI need not
  belong to an R7RS-large edition, require runtime support, or meet a corpus popularity threshold.
- **Implementation-specific APIs stay out of the shipped bundle:** Chibi, Gauche, Gambit and Chez
  libraries remain external dependencies even when pure Scheme or needed by our test lanes.
  Those consumers supply them through `-A`, `-I` or `PATINA_LIBRARY_PATH`.
- **Judge the API, not the origin of its implementation.** A SRFI implementation sourced from
  Chibi is eligible under its SRFI interface. Port or internalize any implementation-specific
  helpers instead of shipping the foreign implementation's public library namespace. SRFI 130's
  inlined string helpers are the existing example.
- **Eligibility sets scope; demand and implementation cost set order.** This is not a claim that
  every SRFI already ships, or a requirement to implement all of them immediately. Runtime and
  FFI requirements can still defer an eligible library. Record the version of any draft implemented.

Patina's own public extensions and internal support libraries remain part of Patina. Other
third-party libraries are obtained separately; see the
[acquisition workflow](../PRD/future/PACKAGE_MANAGER_DESIGN.md) delivered by
[#195](https://github.com/avalonalex/patina/issues/195). SRFI 64 falls under the
general SRFI rule; `(chibi test)` remains external.

When bundling third-party code, preserve its license notices, record provenance,
and pin it in
[`bundled_provenance.rs`](../crates/patina-tests/tests/bundled_provenance.rs).
Register its upstream suite in the
[suite inventory](../scheme_tests/upstream/README.md) or document why it cannot
run; excluding a bundled package from the compatibility corpus must not discard
its test coverage. Keep the bundled SRFI 64 copy pinned and its summary wording
stable, since the compatibility classifier consumes it. SRFI 64 drivers must
read the runner counts or call `test-exit`: `test-end` alone can return
successfully after a failure or unexpected pass.

Red and Tangerine coverage was completed on 2026-09-30; the
[archived edition tracker](../PRD/ARCHIVE/R7RS_LARGE_STATUS.md) preserves the
library tables and verification references. This does not claim completion of
R7RS-large as a whole. A review against the
[current report's volumes and fascicles](https://r7rs.org/large/) is deferred
in [#580](https://github.com/avalonalex/patina/issues/580). New library work
belongs in GitHub issues; procedural macro work remains in the
[syntax-case design](../PRD/macro/SYNTAX_CASE_DESIGN.md).

## Changing the embedding API

The embedding API changes by [#601](https://github.com/avalonalex/patina/pull/601)'s
convention, so that a host's code keeps compiling across a change. What the
convention covers, the promise, is drawn by
[#831](https://github.com/avalonalex/patina/issues/831), as the owner ruled
on its questions on 2026-11-05:
- **Inside:** the item's name and signature change only by the convention.
  A type marked "type inside, members raw" keeps its name, so a promised
  signature that names it keeps compiling, while its fields, methods and
  impls are raw.
- **Raw:** promised nothing beyond the rules documented on it. It changes
  without a deprecated twin.
- A type a promised signature names that the tables leave out, as one an
  inside `Cx` method or `CoreExprKind` names, is inside.
- Inside promises a signature, not a root. `Backend`'s methods and
  `Interpreter`'s `global_env()`, `raw_environment()` and `raw_value()` take
  or answer bare words and environments, which nothing roots between calls
  (#605); `Backend`'s keep the rules its "# Words" section gives.

What `patina-interpreter` exports, at its root unless a module is named:

| Item | Side | Note |
|------|------|------|
| `Interpreter<B>`; `VmInterpreter` (vm), `TreeWalkInterpreter` (tree-walker) | inside | the bare-value `eval_*` methods are deprecated (#605), and `heap()` (#831) for `gc_stats()`, `take_interrupt()`, `display_notation()` and the routes its note names; the `#[doc(hidden)]` `heap_probe()` is raw |
| `GcCounts` | inside | `gc_stats()`'s; `#[non_exhaustive]`, so a later field is additive |
| `HeapProbe` | raw, `#[doc(hidden)]` | `heap_probe()`'s, for the teardown tests |
| `InterpreterError<E>`, `ProgramOutcome`, `WithSourceMap<T>`, `PrimSpec` | inside | |
| `AsValue` | inside, sealed | its `#[doc(hidden)]` `value_in` names `Heap`, so is raw |
| `format_write_tagged`, `format_display_tagged`, `format_interpreter_error`, `format_parse_error_with_source`, `format_error_with_source`, `format_backend_error_with_source`, `HasSourceLocation` | inside | the first two take `&SharedHeap`; `Interpreter`'s `display_tagged()` and `display_notation()` take a handle instead |
| `Owned`, `OwnedEnvironment` | inside, opaque | `new_environment()`'s holds an environment the interpreter made; the deprecated holders hold any of the heap's until 5e |
| `SharedHeap`, `new_shared_heap` | inside as names | the alias's definition, `Rc<RefCell<Heap>>`, and what its `borrow()` reaches are raw; 5e deletes the `RefCell` |
| `TaggedValue`, `SourceLocation`, `SourceMap`, `CoreExpr`, `Arity`, `EvalError`, `RegisterError` | inside | |
| `InterruptHandle`, `event` | inside | |
| `ExternalAccounter` | inside, `new` raw | `new` takes the raw `ExternalBytes`; a host's accounter comes from `external_bytes()` |
| `LexError`, `ParseError`, `DesugarError`, `Lexer`, `Parser` | inside | |
| `Desugarer` | raw | |
| `prune_freed_locations` | inside, deprecated | a no-op, until 5e (#643) |
| `Backend` | inside | its methods change by the convention, and its words keep the "# Words" rules |
| `Environment` | type inside, members raw | `with_parent` is deprecated (#831) and goes at 5e; the workspace builds its frames with the `#[doc(hidden)]` `child_frame`, on #622's list; `for_temporaries` is hidden too |
| `Procedure` | raw | a heap payload no promised signature names; 4f changes its `CpsLambda` arm |
| `Prim`, `ApplyContext`, `CallArgs`, `HostPrimitives`, `PrimClass`, `ResumableStart`, `ResumableResume`, `Step` | inside | `Prim` is `CxHandler`, and `CallArgs` a `smallvec::SmallVec` |
| `VmBackend` (vm); `TreeWalker`, `Evaluator` (tree-walker) | type and setup inside, other members raw | setup: `new`, `with_fs`, `TreeWalker::from_evaluator`, `Default`, `add_library_search_path`, `prepend_library_search_path`, `bootstrap_error`; raw: the other inherent methods, `#[doc(hidden)]` ones included, and `Evaluator`'s pub fields |
| `VmBackendError` (vm) | inside | |
| `SimpleInterpreter`, `StandardPipeline` (legacy-pipeline; also under `simple` and `legacy`) | inside, deprecated | the `simple` module holds only `SimpleInterpreter` |
| `Pipeline`, `PipelineError` (legacy-pipeline; also under `legacy`), `legacy::{PipelineResult, EvaluationStrategy}` | inside | not deprecated, so 5e does not remove them or the `legacy` module; their fate is not #831's |

Not re-exported, so a host that names one imports it from its crate: the
types promised signatures name, and the raw types a host reaches from them:

| Item | Side | Note |
|------|------|------|
| `patina_core::Cx` | inside, by its method rule | a method is inside unless its signature names a raw type (`get_object`'s `HeapObjectData`, `register_root`'s `RootProvider`, `get_procedure`'s `Procedure`, …), which makes it raw; an `Environment` or a `Library` keeps it inside. `hold_environment` and `track_environment` are deprecated (#831) |
| `patina_core::Value` | inside | through `with`, `Prim` and `ApplyContext` |
| `patina_core::FileSystem` | inside | through the `with_fs` constructors |
| `patina_runtime::{HasDiagnostic, Diagnostic, DiagnosticKind}` | inside | through `Backend::Error`'s bound |
| `patina_runtime::LibraryError` | inside | `bootstrap_error` answers one |
| `patina_primitives::{PrimitiveFn, TaggedStep, StepFn}`, `patina_core::{CollectKind, SigintForwarding}` | inside | through `HostPrimitives`, `ApplyContext`, `Step::Collect` and `InterruptHandle::forward_sigint` |
| `patina_core::CoreExprKind` | inside | `CoreExpr`'s `kind` |
| `patina_frontend::error::FrontendError`, `patina_macros::MacroError` | inside | through `PipelineError` (legacy-pipeline) |
| `patina_core::Library` | type inside, members raw | `ApplyContext::load_scheme_library` answers one; its exports are cells (#834) |
| `patina_core::Heap` | raw | `SharedHeap` stands for it, and `backend().heap()` reaches it; `hold_environment` is deprecated (#831), until 5e |
| `patina_core::NamespaceView` | raw | no promised signature names it; implemented over binding records (#834) |
| `patina_core::BindingCell` | raw | a location's token: `Library.exports`' values, and what `Cx::new_declaration_cell` (`#[doc(hidden)]`) answers and `Cx::import_cell_in_frame` takes; unbranded, so held only while the registry entry, a record or the context that made it keeps it (#834) |
| `patina_core::IntroducedRecord` | raw | what `Environment::introduced_records` answers, for 5c and the tests (#834) |
| `patina_core::heap::ExternalBytes` | raw | only the raw `ExternalAccounter::new` and `Heap::external_bytes_handle` name it |

`patina-runtime`'s library loader, which `patina-interpreter` does not
re-export, is workspace-internal, outside the promise (#831): its traits,
their registry, and the `&SharedHeap` a load is lent for the call (#707).

A change to an inside item takes four steps:
1. **Add the replacement beside the old API.**
2. **Mark the old one deprecated:** `#[deprecated(note = "use …")]`, naming
   its replacement. Existing callers then compile, with a warning that says
   what to do. #831's five are each pinned by a `compile_fail` doctest under
   `#![deny(deprecated)]`, beside a twin that compiles with the replacement
   (on `Interpreter::new_environment` and `Interpreter::heap`).
3. **Move the internal callers, and allow the rest:** an internal caller with
   an undeprecated route moves to it rather than taking an allow, as #831's
   callers of `heap()` moved to its replacements or to `backend().heap()`,
   and those of `with_parent` to `child_frame`. A caller with none takes
   `#[allow(deprecated)]`, so `clippy -D warnings` stays green, as
   `new_environment` does for `Heap::hold_environment` until 4b.8's holder.
   A test file that still calls the old API carries `#![allow(deprecated)]`.
4. **Keep `scripts/check_embedding_features.sh` green at each step.** A host
   built against each feature set, and an external `Backend`, still compile.

Removal waits for the stage the GC redesign names: the deprecated forms go at
stage 5e (`PRD/GC_PRD.md` §11.5, DESIGN E.3). A new driver entry also goes on
#622's clippy list, with its control in `reentry_lint_control.rs` (AGENTS.md).

## Current Status

Both backends achieve **100% R7RS-small compliance**:
- **VM backend:** 1226/1226 chibi r7rs-tests.scm passing
- **Tree-walker:** 1226/1226 chibi r7rs-tests.scm passing
