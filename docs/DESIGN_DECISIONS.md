# Design Decisions Q&A

Answers to key design questions about Patina's notebook mode.

## Q: Is tui-textarea useful?

**Absolutely YES!**

### What is tui-textarea?

A multi-line text editor widget for `ratatui` (the modern terminal UI framework).

### Key Features:
- ✅ **Vim emulation** - Modal editing (normal/insert modes)
- ✅ **Yank/paste** - System clipboard integration
- ✅ **Search** - Regex search with highlighting
- ✅ **Undo/redo** - Built-in history
- ✅ **Line numbers** - With customizable styling
- ✅ **Syntax agnostic** - Works with any text
- ✅ **Multiple backends** - crossterm, termion, termwiz

### Why We Need It:

For notebook cells, we need **proper multi-line editing**. tui-textarea gives us:

```rust
use tui_textarea::TextArea;

// Create editor for code cell
let mut editor = TextArea::default();
editor.set_block(Block::default().borders(Borders::ALL).title("Code"));
editor.set_line_number_style(Style::default().fg(Color::DarkGray));

// Users can now:
// - Edit multiple lines naturally
// - Use Vim keybindings (hjkl, i, a, o, dd, yy, p, etc.)
// - Search with /pattern
// - Undo/redo with u/Ctrl+r
```

### Alternatives Considered:

1. **Roll our own** - Too much work, reinventing the wheel
2. **Simple text input** - No multi-line support
3. **tui-rs widgets** - Basic, no advanced features

**Decision: Use tui-textarea** - It's mature, maintained, and feature-complete.

## Q: How to design notebook format?

**Use S-expressions!** A novel approach that's never been done before.

### Why S-expressions vs JSON (Jupyter)?

| Aspect | Jupyter (.ipynb) | Patina (.scm.nb) |
|--------|------------------|------------------|
| Format | JSON | S-expressions |
| Human readable | ⚠️ Verbose | ✅ Clean |
| Loadable as code | ❌ No | ✅ Yes! |
| Git diffs | ❌ Terrible | ✅ Beautiful |
| Language native | ❌ External | ✅ Homoiconic |
| Comments | ❌ No | ✅ Natural |
| Extensible | ⚠️ Limited | ✅ Macros |

### Example Comparison:

**Jupyter (.ipynb):**
```json
{
  "cells": [
    {
      "cell_type": "code",
      "execution_count": 1,
      "metadata": {},
      "outputs": [
        {
          "data": {
            "text/plain": ["6"]
          },
          "execution_count": 1,
          "metadata": {},
          "output_type": "execute_result"
        }
      ],
      "source": ["(+ 1 2 3)"]
    }
  ],
  "metadata": {
    "kernelspec": {
      "display_name": "Scheme",
      "language": "scheme",
      "name": "scheme"
    }
  },
  "nbformat": 4,
  "nbformat_minor": 5
}
```

**Patina (.scm.nb):**
```scheme
(notebook
  (metadata (title "My Analysis"))

  (cell code
    (+ 1 2 3))

  (outputs
    (cell-1 (value 6))))
```

### Revolutionary Idea: Notebooks AS Programs

```scheme
;; analysis.scm.nb is a valid Scheme library!
(import (analysis))

;; Use definitions from the notebook
(my-function data)  ; Works!

;; Include notebooks in notebooks
(notebook
  (import-notebook "common.scm.nb")
  (cell code
    (use-shared-function)))
```

### Format Specification:

See [NOTEBOOK_FORMAT.md](NOTEBOOK_FORMAT.md) for complete details.

**Key principles:**
1. **Homoiconicity** - Code is data, data is code
2. **Simplicity** - Just S-expressions
3. **Composability** - Use full Scheme features
4. **VCS-friendly** - Meaningful diffs

## Q: How to handle system commands elegantly?

**Make everything a function!** No magic syntax.

### Jupyter's Problem:

```python
# Awkward special syntax
!ls -la              # Shell command
%cd /tmp             # Magic command
%%bash               # Cell magic
result = !cat file   # Capture output
```

Problems:
- Inconsistent syntax (`!` vs `%` vs `%%`)
- Not composable
- Language mixing
- Hard to extend
- Poor error handling

### Patina's Solution:

```scheme
;; Clean, first-class functions
(shell "ls -la")              ; Execute command
(cd "/tmp")                   ; Change directory
(define result
  (shell-capture "cat file")) ; Capture output

;; Composable!
(pipe
  "cat data.txt"
  "grep ERROR"
  "wc -l")

;; Extensible!
(define (git command . args)
  (apply shell "git" (symbol->string command) args))

(git 'status)
(git 'log #:max-count 5)
```

### System Integration Library:

```scheme
(library (patina system)
  (export
    ;; Shell
    shell sh $
    shell-capture shell-lines
    pipe

    ;; File system
    cd pwd ls
    file-read file-write
    directory-list

    ;; Git
    git git-status git-log git-commit

    ;; Process
    process-start process-wait process-kill
    parallel

    ;; Environment
    getenv setenv
    with-directory with-environment))
```

### Why This is Better:

1. **Consistent** - Everything is a function call
2. **Composable** - Chain operations naturally
3. **Extensible** - Define your own abstractions
4. **Type-safe** - Return proper Scheme values
5. **Error handling** - Use `guard` and exceptions
6. **Documentation** - Just docstrings

### Examples:

**Bad (Jupyter):**
```python
!git status
!git log | head -5
files = !ls *.py
```

**Good (Patina):**
```scheme
(git 'status)
(pipe "git log" "head -5")
(define files (shell-lines "ls *.scm"))
```

**Even Better (Patina with DSL):**
```scheme
;; Define your own abstractions!
(define-syntax deployment
  (syntax-rules (build test deploy)
    ((deployment
       (build CMD)
       (test CMD)
       (deploy CMD))
     (begin
       (shell-check CMD)
       (shell-check CMD)
       (shell-check CMD)))))

(deployment
  (build "cargo build --release")
  (test "cargo test")
  (deploy "./deploy.sh"))
```

### Domain-Specific Commands:

Instead of generic shell, create domain libraries:

```scheme
(library (patina docker)
  (export docker-ps docker-run docker-stop))

(library (patina kubernetes)
  (export k8s-get k8s-apply k8s-delete))

(library (patina data)
  (export csv->alist json->scheme))

;; Now notebooks are clean:
(import (patina docker)
        (patina kubernetes)
        (patina data))

(docker-ps)
(k8s-get 'pods)
(define data (csv->alist "sales.csv"))
```

## Summary: Three Pillars

### 1. **tui-textarea** - YES!
Provides professional multi-line editing in cells with Vim emulation.

### 2. **S-expression Format** - Novel!
Notebooks are valid Scheme programs. Homoiconic, git-friendly, composable.

### 3. **System Commands as Functions** - Elegant!
No magic syntax. Everything is a first-class function. Fully composable.

## Implementation Priority

1. ✅ **Phase 1: Core Interpreter** (DONE)
   - Lexer, parser, evaluator
   - Rich REPL with syntax highlighting

2. 🔜 **Phase 2: Language Features** (NEXT)
   - Lambda and closures
   - More special forms (let, cond, etc.)
   - Tail call optimization

3. 📋 **Phase 3: TUI Notebook** (v0.2.0)
   - Integrate ratatui + tui-textarea
   - S-expression parser
   - Cell execution

4. 📋 **Phase 4: System Integration** (v0.3.0)
   - (patina system) library
   - Shell, file, git, process commands

5. 📋 **Phase 5: Advanced** (v0.4.0+)
   - Dependency tracking
   - Visualizations
   - Export formats

## Why This Matters

Patina notebooks will be **unique in the ecosystem**:

- **Only** Scheme notebook with S-expression format
- **Only** terminal notebook with homoiconic format
- **Only** notebook where system commands are first-class
- **Only** notebook that's a valid program

This is a genuinely novel contribution to both:
- Scheme community (modern tooling)
- Literate programming (terminal-native, S-expr format)

---

Ready to build something unique! 🚀
