# Patina Documentation

Documentation for **completed and implemented features** in Patina.

## Working with coding agents

The repository uses one shared instruction source:

```text
AGENTS.md       Shared project rules, architecture constraints, and commands
CLAUDE.md       Claude Code entry point; imports @AGENTS.md
docs/           Detailed design and testing references, read when relevant
PRD/            Planning and decision history
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

## Current Status

Both backends achieve **100% R7RS-small compliance**:
- **VM backend:** 1226/1226 chibi r7rs-tests.scm passing
- **Tree-walker:** 1226/1226 chibi r7rs-tests.scm passing
