# Patina Documentation Index

Complete documentation for the Patina Scheme R7RS interpreter and notebook system.

## Quick Start
- [QUICKSTART.md](../QUICKSTART.md) - Get up and running in 5 minutes
- [README.md](../README.md) - Project overview and features

## REPL
- [REPL_FEATURES.md](REPL_FEATURES.md) - Rich REPL with syntax highlighting
  - Multi-line editing
  - Persistent history
  - Parenthesis balancing
  - Emacs keybindings

## Notebook Mode (Planned)

### Overview
- [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) - **START HERE!** Answers key questions:
  - Is tui-textarea useful? (YES!)
  - How to design notebook format? (S-expressions!)
  - How to handle system commands? (Functions, not magic!)

### Detailed Specifications
- [NOTEBOOK_FORMAT.md](NOTEBOOK_FORMAT.md) - Complete S-expression format spec
  - Notebook structure
  - Cell types (code, markdown, output)
  - Metadata and dependencies
  - On-disk format options
  - Literate programming features

- [SYSTEM_INTEGRATION.md](SYSTEM_INTEGRATION.md) - Three-tier command integration
  - **Tier 1**: Native Scheme (ls, find, tree)
  - **Tier 2**: Tabular API (ps, df, git-log)
  - **Tier 3**: Shell catch-all (everything else)
  - Why this beats Jupyter's `!` and `%`

- [NATIVE_COMMANDS.md](NATIVE_COMMANDS.md) - **NEW!** Native command design
  - File operations returning structured data
  - Table API for tabular output
  - Cross-platform, type-safe, composable
  - Examples of all three tiers

### Implementation
- [TUI_IMPLEMENTATION.md](TUI_IMPLEMENTATION.md) - How to build the TUI
  - Using `ratatui` framework
  - Integrating `tui-textarea` for editing
  - Cell rendering and navigation
  - Key bindings (Vim-style)
  - Execution and output

- [NOTEBOOK_DESIGN.md](NOTEBOOK_DESIGN.md) - Original vision document
  - UI mockups
  - Feature roadmap
  - Comparison with Jupyter

## Development
- [NEXT_STEPS.md](../NEXT_STEPS.md) - Implementation roadmap
  - Immediate priorities (lambda!)
  - R7RS compliance todos
  - Notebook implementation phases
  - Future features (gradual typing, reactive, logic)

- [PROJECT_SUMMARY.md](../PROJECT_SUMMARY.md) - Technical overview
  - Architecture
  - Statistics
  - Component breakdown

## Examples
- [demo.scm](../examples/demo.scm) - Basic Scheme examples
- [sample-notebook.scm.nb](../examples/sample-notebook.scm.nb) - Tutorial notebook
- [data-analysis.scm.nb](../examples/data-analysis.scm.nb) - Realistic analysis
- [system-integration-demo.scm.nb](../examples/system-integration-demo.scm.nb) - **NEW!** Three-tier system integration

## Documentation Statistics

- **7 design documents** (~4,300 lines)
- **4 example notebooks** (~500 lines)
- **Total documentation**: 4,800+ lines

## Key Innovations

### 1. S-expression Notebooks
First notebook format that's native Scheme:
```scheme
(notebook
  (metadata (title "Analysis"))
  (cell code (+ 1 2 3))
  (cell markdown "# Results"))
```

**Benefits:**
- Homoiconic (code is data)
- Git-friendly diffs
- Loadable as Scheme libraries
- Composable with macros
- No JSON bloat

### 2. System Commands as Functions
No magic syntax like Jupyter's `!` or `%`:
```scheme
;; Bad (Jupyter)
!ls -la
%cd /tmp

;; Good (Patina)
(shell "ls -la")
(cd "/tmp")
(git 'status)
(pipe "cat" "grep" "wc")
```

**Benefits:**
- Consistent function call syntax
- Fully composable
- Proper error handling
- Type-safe return values
- Extensible with macros

### 3. TUI with Professional Editing
Using `tui-textarea` for cell editing:
- Vim keybindings
- Multi-line editing
- Syntax highlighting
- Search with regex
- Undo/redo

## Comparison with Jupyter

| Feature | Jupyter | Patina |
|---------|---------|--------|
| Format | JSON (.ipynb) | S-expr (.scm.nb) |
| Loadable as code | ❌ | ✅ |
| Git diffs | ❌ Terrible | ✅ Beautiful |
| System commands | `!cmd`, `%magic` | `(shell "cmd")` |
| Extensibility | Python plugins | Scheme macros |
| Terminal UI | ❌ | ✅ Native |
| Cell dependencies | ❌ | ✅ Tracked |
| Language agnostic | ✅ | ❌ Scheme-only |

## Implementation Status

### ✅ Complete (v0.1.0)
- Core interpreter (lexer, parser, evaluator)
- Rich REPL with syntax highlighting
- Multi-line editing
- Persistent history
- Basic primitives and special forms

### 🔜 Next (v0.2.0)
- Lambda and closures
- More special forms (let, cond, case)
- Tail call optimization
- R7RS compliance tests

### 📋 Planned (v0.3.0+)
- TUI notebook mode
- S-expression parser
- System integration library
- Dependency tracking
- Export formats

## Reading Order

**For users:**
1. [QUICKSTART.md](../QUICKSTART.md)
2. [REPL_FEATURES.md](REPL_FEATURES.md)
3. [sample-notebook.scm.nb](../examples/sample-notebook.scm.nb)

**For contributors:**
1. [PROJECT_SUMMARY.md](../PROJECT_SUMMARY.md)
2. [NEXT_STEPS.md](../NEXT_STEPS.md)
3. Implementation docs

**For designers/researchers:**
1. [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) - Key Q&A
2. [NOTEBOOK_FORMAT.md](NOTEBOOK_FORMAT.md) - Format spec
3. [SYSTEM_INTEGRATION.md](SYSTEM_INTEGRATION.md) - Command integration
4. [NOTEBOOK_DESIGN.md](NOTEBOOK_DESIGN.md) - Original vision

## Contributing

See areas that need work in [NEXT_STEPS.md](../NEXT_STEPS.md).

Priority:
1. **Implement lambda** - Most important!
2. Add more primitives
3. Improve error messages
4. Write comprehensive tests

## License

MIT License - See LICENSE file

---

**Welcome to Patina!** 🦀✨

A Scheme R7RS interpreter with a vision for the future of literate programming.
