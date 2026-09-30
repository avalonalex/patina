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

## Current Status

Both backends achieve **100% R7RS-small compliance**:
- **VM backend:** 1226/1226 chibi r7rs-tests.scm passing
- **Tree-walker:** 1226/1226 chibi r7rs-tests.scm passing
