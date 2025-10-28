# Patina Quick Start Guide

Welcome to Patina! This guide will get you up and running in 5 minutes.

## Installation

```bash
# Clone the repository (if not already done)
git clone <your-repo-url>
cd patina

# Build the project (release mode for better performance)
cargo build --release

# The binary will be at target/release/patina
```

## Your First Session

```bash
# Start the REPL
cargo run --release
```

You'll see a welcome screen:

```
Patina Scheme R7RS Interpreter
Version 0.1.0

Features:
  • Multi-line editing with auto-indentation
  • Syntax highlighting
  • Persistent history
  • Parenthesis balancing

Commands:
  (exit) or Ctrl+D to quit
  Ctrl+C to cancel current input

patina>
```

## Try These Examples

### Basic Arithmetic
```scheme
patina> (+ 1 2 3)
6

patina> (* (+ 2 3) (- 10 5))
25
```

### Variables
```scheme
patina> (define x 42)
#<unspecified>

patina> (define y 58)
#<unspecified>

patina> (+ x y)
100
```

### Conditionals
```scheme
patina> (if (< x 50) "small" "large")
"small"
```

### Multi-line Editing
Notice how the REPL waits for you to complete the expression:

```scheme
patina> (if (> x 0)
...         (+ x 10)
...         (- x 10))
52
```

The `...` prompt indicates the expression is incomplete. You can use arrow keys to navigate and edit!

### Lists
```scheme
patina> (define my-list (list 1 2 3 4 5))
#<unspecified>

patina> my-list
(1 2 3 4 5)

patina> (car my-list)
1

patina> (cdr my-list)
(2 3 4 5)
```

### Nested Expressions
```scheme
patina> (cons 'a (cons 'b (cons 'c '())))
(a b c)
```

## REPL Features You'll Love

### 1. Syntax Highlighting
As you type, you'll see:
- **Keywords** in cyan (define, if, lambda, etc.)
- **Built-ins** in yellow (+, cons, car, etc.)
- **Strings** in green
- **Numbers** in magenta
- **Comments** in gray

### 2. History
- Press **Up/Down** arrows to navigate history
- Press **Ctrl+R** to search history
- History is saved to `~/.patina_history`

### 3. Editing
All Emacs keybindings work:
- **Ctrl+A** - Go to beginning of line
- **Ctrl+E** - Go to end of line
- **Ctrl+K** - Kill to end of line
- **Ctrl+U** - Kill entire line
- **Alt+F/B** - Move forward/backward by word

### 4. Smart Parenthesis
The REPL knows when your expression is complete:
```scheme
patina> (+ 1        # Incomplete - waits for more
...        2        # Still incomplete
...        3)       # Complete! Evaluates now
6
```

## What's Not Implemented Yet

Currently missing (see NEXT_STEPS.md):
- `lambda` - function definitions
- `let`, `let*`, `letrec` - local bindings
- `cond`, `case` - advanced conditionals
- Full numeric tower (only integers and floats work)
- Macros
- I/O operations
- And more...

But the foundation is solid and ready for these features!

## Running Examples

```bash
# There's a demo file with examples
cat examples/demo.scm

# You can copy-paste from it into the REPL
# Or load it (once file loading is implemented!)
```

## Next Steps

1. **Read the docs**
   - [REPL_FEATURES.md](docs/REPL_FEATURES.md) - Detailed REPL guide
   - [NEXT_STEPS.md](NEXT_STEPS.md) - Implementation roadmap
   - [NOTEBOOK_DESIGN.md](docs/NOTEBOOK_DESIGN.md) - Future vision

2. **Explore R7RS**
   - Download [Chibi Scheme tests](https://github.com/ashinn/chibi-scheme/blob/master/tests/r7rs-tests.scm)
   - Read the [R7RS specification](http://www.scheme-reports.org/)

3. **Contribute**
   - Implement `lambda` (most important!)
   - Add more primitives
   - Improve error messages
   - Write tests

## Troubleshooting

### Colors not showing
Make sure your terminal supports ANSI colors. Most modern terminals do.

### History not saving
Check permissions on `~/.patina_history`

### Ctrl+C doesn't work
Some terminals require double Ctrl+C

## Getting Help

In the REPL:
- Type `(exit)` or press Ctrl+D to quit
- Ctrl+C to cancel current input
- Check out the examples in `examples/demo.scm`

## Have Fun!

Patina is a learning project. Experiment, break things, and learn about interpreters and Scheme!

The REPL experience is designed to be pleasant. Multi-line editing with syntax highlighting makes writing Scheme code enjoyable even in the terminal.

Happy hacking! 🦀✨
