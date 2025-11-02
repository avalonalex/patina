# Patina REPL Features

Inspired by Chez Scheme's excellent "Expeditor" expression editor, Patina provides a rich, modern REPL experience.

## Current Features

### 1. Syntax Highlighting

Real-time color-coded syntax highlighting as you type:

- **Keywords** (cyan, bold): `define`, `lambda`, `if`, `cond`, `let`, etc.
- **Built-ins** (yellow): `+`, `-`, `cons`, `car`, `map`, `for-each`, etc.
- **Strings** (green): `"hello world"`
- **Numbers** (magenta): `42`, `3.14`, `#xFF`
- **Booleans** (blue, bold): `#t`, `#f`
- **Comments** (gray, dimmed): `; this is a comment`
- **Parentheses** (white, dimmed): `(`, `)`, `[`, `]`

### 2. Multi-line Editing

The REPL automatically detects incomplete expressions and allows multi-line input:

```scheme
patina> (define factorial
...       (lambda (n)
...         (if (= n 0)
...             1
...             (* n (factorial (- n 1))))))
```

The expression is only evaluated when parentheses are balanced.

### 3. Intelligent Parenthesis Balancing

- Automatically detects when you have unmatched opening parentheses
- Continues to the next line for input
- Handles strings and comments correctly (doesn't count parens inside them)

### 4. Persistent History

- Command history is saved to `~/.patina_history`
- History persists across sessions
- Navigate history with Up/Down arrow keys
- Search history with Ctrl+R (reverse search)

### 5. History-based Hints

As you type, the REPL shows gray hints from your command history:

```scheme
patina> (defi▌ne factorial...
         ^^^^^^ hint from history
```

### 6. Line Editing

Full Emacs-style keybindings:

- `Ctrl+A` / `Ctrl+E` - Beginning/end of line
- `Ctrl+K` - Kill to end of line
- `Ctrl+U` - Kill to beginning of line
- `Ctrl+W` - Kill word backwards
- `Alt+F` / `Alt+B` - Forward/backward word
- `Ctrl+L` - Clear screen
- Arrow keys for navigation

### 7. Input Control

- `Ctrl+C` - Cancel current input
- `Ctrl+D` - Exit REPL
- `(exit)` - Exit command

## Comparison with Chez Scheme's Expeditor

| Feature | Chez Expeditor | Patina REPL | Status |
|---------|----------------|-------------|---------|
| Syntax highlighting | ✅ | ✅ | Complete |
| Multi-line editing | ✅ | ✅ | Complete |
| Auto-indentation | ✅ | ⚠️ | Basic (no smart indent yet) |
| Paren balancing | ✅ | ✅ | Complete |
| Persistent history | ✅ | ✅ | Complete |
| History search | ✅ | ✅ | Complete |
| Tab completion | ✅ | ⏳ | Planned |
| Expression navigation | ✅ | ⏳ | Planned |
| S-expr awareness | ✅ | ⏳ | Planned |

## Planned Features

### Short-term
- [ ] Tab completion for symbols
  - Built-in functions
  - User-defined variables
  - Keywords
- [ ] Smart auto-indentation
  - Indent based on expression structure
  - Align with opening parenthesis
- [ ] Bracket matching highlight
  - Highlight matching paren when cursor is on it

### Medium-term
- [ ] S-expression navigation
  - `Alt+Left/Right` to jump between s-exprs
  - `Alt+Up/Down` to move up/down expression tree
- [ ] Macro expansion preview
  - See what macros expand to
- [ ] Documentation hints
  - Show procedure signatures
  - Display docstrings

### Long-term (Notebook Mode)
- [ ] Session management
  - Save/load REPL sessions
  - Replay commands
  - Export to file
- [ ] Rich output
  - Pretty-print data structures
  - Visualize lists/trees
  - Plot numerical data
- [ ] Cell-based editing (Jupyter-style)
  - Multiple input cells
  - Edit and re-evaluate cells
  - Output below each cell
- [ ] Literate programming support
  - Mix code and markdown
  - Export to markdown/HTML

## Usage Examples

### Basic Arithmetic
```scheme
patina> (+ 1 2 3)
6

patina> (* (+ 2 3)
...        (- 10 5))
25
```

### Defining Functions
```scheme
patina> (define (square x)
...       (* x x))
#<unspecified>

patina> (square 5)
25
```

### Working with Lists
```scheme
patina> (define nums (list 1 2 3 4 5))
#<unspecified>

patina> nums
(1 2 3 4 5)
```

## Technical Implementation

### Architecture

```
REPL Module
├── Highlighter   - Syntax highlighting with nu-ansi-term
├── Validator     - Parenthesis balance checking
├── Hinter        - History-based hints
└── Completer     - Auto-completion (TODO)
```

### Libraries Used

- **rustyline** - Line editing and history management
- **nu-ansi-term** - Terminal color formatting
- **dirs** - Cross-platform home directory finding

### Customization

The REPL can be customized by modifying `src/repl/`:

- `highlighter.rs` - Color schemes and syntax rules
- `validator.rs` - Expression completion logic
- `mod.rs` - Key bindings and editor configuration

## Performance

The REPL is designed to be responsive even for long expressions:

- Syntax highlighting is incremental
- History search is optimized with rustyline's built-in indexing
- Validation only checks parenthesis balance (O(n) on input length)

## Troubleshooting

### History not saving
Check that you have write permissions to `~/.patina_history`

### Colors not showing
Ensure your terminal supports ANSI colors. Most modern terminals do.

### Ctrl+C not working
On some terminals, you may need to press Ctrl+C twice to interrupt.

## Future: Notebook Mode

See [NOTEBOOK_DESIGN.md](./NOTEBOOK_DESIGN.md) for our vision of a terminal-based notebook interface inspired by Jupyter but designed for Scheme.
