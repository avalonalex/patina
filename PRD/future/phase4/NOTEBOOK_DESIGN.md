# Patina Notebook Mode - Design Document

A terminal-based notebook interface inspired by Jupyter, designed specifically for Scheme REPL sessions.

## Vision

Create a rich terminal UI that combines:
- Jupyter's cell-based editing model
- Emacs org-mode's literate programming
- Chez Scheme's expression-aware editing
- Modern terminal UI capabilities

## Why Terminal-Based?

1. **Performance** - No browser overhead, instant startup
2. **Integration** - Works seamlessly with terminal workflow
3. **Remote-friendly** - SSH-compatible, works over tmux/screen
4. **Accessibility** - Pure text, screen reader friendly
5. **Lightweight** - No web stack required

## Core Concepts

### 1. Cells

Two types of cells:

**Code Cells**
```scheme
┌─ [1] ─────────────────────────────────┐
│ (define (fibonacci n)                 │
│   (if (< n 2)                         │
│       n                               │
│       (+ (fibonacci (- n 1))          │
│          (fibonacci (- n 2)))))       │
└───────────────────────────────────────┘
Output:
#<procedure fibonacci>
```

**Markdown Cells**
```markdown
┌─ [M] ─────────────────────────────────┐
│ # Fibonacci Implementation            │
│                                       │
│ This implements the classic recursive │
│ Fibonacci algorithm.                  │
└───────────────────────────────────────┘
```

### 2. Session State

- Each session has a persistent environment
- Cells can be re-evaluated in any order
- Environment state is tracked and visualized
- Can save/load entire sessions

### 3. Navigation

Vim-style modal editing:

- **Normal Mode**: Navigate between cells
- **Edit Mode**: Edit within a cell
- **Command Mode**: Session operations

## User Interface

### Layout

```
┌─────────────────────────────────────────────────────────────────┐
│ Patina Notebook - fibonacci.scm                    [Modified]   │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│ ┌─ [1] Code ──────────────────────────────────────┐ [1.2s] ✓   │
│ │ (define (fib n)                                 │            │
│ │   (if (< n 2) n                                 │            │
│ │       (+ (fib (- n 1)) (fib (- n 2)))))         │            │
│ └─────────────────────────────────────────────────┘            │
│ Output:                                                         │
│ #<procedure fib>                                                │
│                                                                  │
│ ┌─ [2] Code ──────────────────────────────────────┐ [0.3s] ✓   │
│ │ (map fib '(0 1 2 3 4 5 6 7 8))                  │            │
│ └─────────────────────────────────────────────────┘            │
│ Output:                                                         │
│ (0 1 1 2 3 5 8 13 21)                                           │
│                                                                  │
│ ┌─ [3] Code ──────────────────────────────────────┐ [Edit]     │
│ │ ▌                                                │            │
│ │                                                  │            │
│ └─────────────────────────────────────────────────┘            │
│                                                                  │
├─────────────────────────────────────────────────────────────────┤
│ [Normal] Cell 3/3 | Line 1/1 | Press ? for help                │
└─────────────────────────────────────────────────────────────────┘
```

### Key Bindings (Normal Mode)

```
Navigation:
  j/k         - Move to next/previous cell
  gg/G        - Go to first/last cell
  Ctrl+D/U    - Page down/up

Cell Operations:
  Enter       - Enter edit mode
  Shift+Enter - Execute cell and move to next
  Alt+Enter   - Execute cell and insert below

  a           - Insert cell below
  b           - Insert cell above
  dd          - Delete cell
  yy          - Copy cell
  p           - Paste cell below

  m           - Convert to markdown cell
  c           - Convert to code cell

Session:
  :w          - Save session
  :q          - Quit
  :e [file]   - Load session
  :export     - Export to various formats
```

### Key Bindings (Edit Mode)

```
Basic:
  Esc         - Return to normal mode
  Ctrl+Enter  - Execute cell

  Emacs-style editing (from rustyline)
  Tab         - Auto-complete
  Ctrl+Space  - Show documentation
```

## Technical Architecture

### Components

```
┌─────────────────────────────────────────┐
│            Notebook Manager             │
│  - Session state                        │
│  - Cell management                      │
│  - Execution coordinator                │
└─────────────────────────────────────────┘
                  │
     ┌────────────┼────────────┐
     │            │            │
┌────▼────┐  ┌───▼────┐  ┌───▼────┐
│  TUI    │  │ Editor │  │  Eval  │
│ (ratatui)│  │(rustyline)│  │ Engine │
└─────────┘  └────────┘  └────────┘
```

### Libraries

- **ratatui** - Terminal UI framework (successor to tui-rs)
- **crossterm** - Terminal manipulation
- **serde** - Session serialization
- **syntect** - Enhanced syntax highlighting (optional)

### Data Model

```rust
struct Notebook {
    cells: Vec<Cell>,
    environment: Rc<Environment>,
    metadata: NotebookMetadata,
}

enum Cell {
    Code(CodeCell),
    Markdown(MarkdownCell),
}

struct CodeCell {
    id: CellId,
    source: String,
    output: Option<CellOutput>,
    execution_count: Option<usize>,
    metadata: CellMetadata,
}

struct CellOutput {
    result: Value,
    stdout: String,
    stderr: String,
    execution_time: Duration,
}
```

### Session Format

Sessions are saved as JSON (or S-expressions for meta-circularity):

```json
{
  "cells": [
    {
      "type": "code",
      "source": "(define x 42)",
      "execution_count": 1,
      "outputs": [
        {
          "type": "result",
          "data": "#<unspecified>"
        }
      ]
    },
    {
      "type": "markdown",
      "source": "# My Notes\n\nThis is a note."
    }
  ],
  "metadata": {
    "version": "0.1.0",
    "created": "2025-10-27T10:00:00Z",
    "modified": "2025-10-27T10:30:00Z"
  }
}
```

## Advanced Features

### 1. Dependency Tracking

Show which cells depend on which definitions:

```
[1] (define x 10)
    ↓
[2] (define y (* x 2))  ← depends on [1]
    ↓
[3] (+ x y)             ← depends on [1], [2]
```

Re-evaluating [1] marks [2] and [3] as "stale" - needing re-evaluation.

### 2. Visualization

Integration with visualization libraries:

```scheme
[1] (define data '(1 4 2 8 5 7))

[2] (display (plot/bar data))
```

Output:
```
    8 ┤  █
    7 ┤  █ █
    6 ┤  █ █
    5 ┤  █ █ █
    4 ┤█ █ █ █
    3 ┤█ █ █ █
    2 ┤█ █ █ █
    1 ┤█ █ █ █ █ █
      └─────────────
```

### 3. Rich Output Types

```scheme
; Tables
(display-table
  '((name age city)
    ("Alice" 30 "NYC")
    ("Bob" 25 "SF")))

; Trees
(display-tree '(a (b (c d)) (e f)))

; Graphs
(display-graph
  '((A → B)
    (B → C)
    (B → D)
    (C → D)))
```

### 4. Export Formats

- **Scheme script** - Pure `.scm` file with code only
- **Markdown** - Literate programming style
- **HTML** - Static web page with syntax highlighting
- **PDF** - Via markdown → pandoc
- **Org-mode** - For Emacs users

### 5. Collaborative Features (Future)

- **Shared sessions** - Multiple users in same notebook
- **Version control** - Git-friendly diff format
- **Comments** - Annotate cells with discussions

## Implementation Phases

### Phase 1: Basic TUI (v0.2.0)
- [ ] Basic terminal UI with ratatui
- [ ] Multiple cells with navigation
- [ ] Execute cells in order
- [ ] Save/load sessions

### Phase 2: Rich Editing (v0.3.0)
- [ ] Integrated editor in TUI
- [ ] Syntax highlighting in cells
- [ ] Cell type switching (code/markdown)
- [ ] Copy/paste/delete cells

### Phase 3: Session Management (v0.4.0)
- [ ] Dependency tracking
- [ ] Stale cell detection
- [ ] Re-evaluation strategies
- [ ] Export to multiple formats

### Phase 4: Visualization (v0.5.0)
- [ ] ASCII art plots
- [ ] Table rendering
- [ ] Tree visualization
- [ ] Custom display hooks

### Phase 5: Collaboration (v1.0.0)
- [ ] Shared sessions
- [ ] Real-time collaboration
- [ ] Comments and annotations

## Design Inspirations

### Jupyter
- Cell-based execution model
- Rich output types
- Notebook persistence

### Emacs Org-mode
- Literate programming
- Mix of text and code
- Export to multiple formats
- Tangling and weaving

### Observable
- Reactive cells
- Dependency tracking
- Visual dataflow

### Chez Scheme Expeditor
- S-expression awareness
- Smart editing
- Expression navigation

## Alternative: Simple Session Recording

Before implementing full notebook mode, we can add a simpler "session recording" feature:

```scheme
patina> ,record start session1.scm

[Recording to session1.scm]

patina> (define x 10)
patina> (+ x 5)
15

patina> ,record stop

[Session saved to session1.scm]

patina> ,replay session1.scm

[Replaying session1.scm...]
(define x 10) => #<unspecified>
(+ x 5) => 15
```

This is much simpler to implement and provides 80% of the value.

## Questions to Consider

1. **Mode switching** - How to make it discoverable?
2. **Long outputs** - How to handle large results?
3. **Performance** - Can TUI handle 100+ cells?
4. **Keybindings** - Vim-like, Emacs-like, or hybrid?
5. **Remote use** - How does it work over SSH with latency?

## Conclusion

A terminal-based notebook for Scheme would be unique in the ecosystem. It combines the power of literate programming with the immediacy of a REPL, all in a fast, lightweight terminal interface.

The incremental approach (starting with session recording, then basic TUI, then full notebook) allows us to deliver value quickly while building toward the vision.
