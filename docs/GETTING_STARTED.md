# Getting Started with Patina

Welcome to Patina! This guide will get you up and running quickly.

## Installation

### Prerequisites

- **Rust** 1.70 or later
- **chibi-scheme** (optional, for R7RS compliance testing)

### Install Rust

```bash
# Using rustup (recommended)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Install chibi-scheme (Optional)

Only needed if you want to run comparison tests against the R7RS reference implementation.

**macOS:**
```bash
brew install chibi-scheme
```

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install chibi-scheme
```

**Arch Linux:**
```bash
sudo pacman -S chibi-scheme
```

### Build Patina

```bash
# Clone the repository
git clone <your-repo-url>
cd patina

# Build in release mode for better performance
cargo build --release

# The binary will be at: target/release/patina
```

### Verify Installation

```bash
# Run tests
cargo test

# Skip chibi-scheme tests if not installed
SKIP_CHIBI_TESTS=1 cargo test
```

## Your First Session

### Start the REPL

```bash
cargo run --release
```

You'll see:

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

### Try These Examples

#### Basic Arithmetic
```scheme
patina> (+ 1 2 3)
6

patina> (* (+ 2 3) (- 10 5))
25
```

#### Variables
```scheme
patina> (define x 42)

patina> (define y 58)

patina> (+ x y)
100
```

#### Lambda Functions (NEW!)
```scheme
patina> ((lambda (x) (* x 2)) 21)
42

patina> (define square (lambda (x) (* x x)))

patina> (square 7)
49
```

#### Closures
```scheme
patina> (define make-adder
...       (lambda (n)
...         (lambda (x) (+ x n))))

patina> (define add5 (make-adder 5))

patina> (add5 10)
15
```

#### Conditionals
```scheme
patina> (if (< x 50) "small" "large")
"small"

patina> (cond
...       ((< x 0) "negative")
...       ((= x 0) "zero")
...       (else "positive"))
"positive"
```

#### Lists
```scheme
patina> (define my-list (list 1 2 3 4 5))

patina> (car my-list)
1

patina> (cdr my-list)
(2 3 4 5)

patina> (cons 0 my-list)
(0 1 2 3 4 5)
```

## REPL Features

### Syntax Highlighting

As you type, you'll see color-coded syntax:
- **Keywords** in cyan (define, if, lambda, etc.)
- **Built-ins** in yellow (+, cons, car, etc.)
- **Strings** in green
- **Numbers** in magenta
- **Comments** in gray

### Multi-line Editing

The REPL knows when your expression is incomplete:

```scheme
patina> (define factorial
...       (lambda (n)
...         (if (<= n 1)
...             1
...             (* n (factorial (- n 1))))))

patina> (factorial 5)
120
```

The `...` prompt indicates continuation. Use arrow keys to navigate!

### Command History

- **Up/Down arrows** - Navigate history
- **Ctrl+R** - Search history
- History saved to `~/.patina_history`

### Editing Shortcuts

All Emacs keybindings work:
- **Ctrl+A** - Beginning of line
- **Ctrl+E** - End of line
- **Ctrl+K** - Kill to end of line
- **Ctrl+U** - Kill entire line
- **Alt+F/B** - Move forward/backward by word

## What's Implemented

**Currently working** (as of 2025-11-02):
- ✅ Lambda with full closures
- ✅ Basic special forms (quote, if, define, set!, begin, cond)
- ✅ Arithmetic (+, -, *, /)
- ✅ Comparisons (=, <, >, <=, >=)
- ✅ Lists (cons, car, cdr, list, null?, pair?)
- ✅ Type predicates (eq?, equal?, boolean?, number?, etc.)

**Coming soon:**
- 🚧 let, let*, letrec (local bindings)
- 🚧 and, or (boolean operators)
- 🚧 apply, map, for-each (higher-order functions)
- 🚧 More list operations (append, reverse, length)

See [FEATURE_STATUS.md](FEATURE_STATUS.md) for complete status.

## Running Examples

Check out the examples directory:

```bash
# Lambda demonstration
cargo run --example test_lambda --release

# View example code
cat examples/test_lambda.rs
```

## Docker (Alternative Setup)

If you prefer Docker for reproducibility:

```bash
# Build and run tests
docker-compose up patina-test

# Interactive development
docker-compose run patina-dev bash
```

Inside container:
```bash
cargo test          # Run all tests
cargo run           # Start REPL
```

## Troubleshooting

### Colors not showing
Make sure your terminal supports ANSI colors. Most modern terminals do.

### History not saving
Check permissions on `~/.patina_history`

### chibi-scheme tests failing
Skip them if not needed:
```bash
SKIP_CHIBI_TESTS=1 cargo test
```

### Build errors
Update Rust:
```bash
rustup update
```

## Next Steps

1. **Explore the language**
   - Try more complex examples
   - Experiment with closures
   - Write recursive functions

2. **Read the documentation**
   - [Feature Status](FEATURE_STATUS.md) - What's implemented
   - [API Reference](API.md) - Using Patina in your Rust code
   - [Development Guide](DEVELOPMENT.md) - Contributing

3. **Learn Scheme**
   - [R7RS Specification](http://www.scheme-reports.org/)
   - [Chibi-scheme tests](https://github.com/ashinn/chibi-scheme)
   - [Scheme Wiki](http://community.schemewiki.org/)

4. **Contribute**
   - See [DEVELOPMENT.md](DEVELOPMENT.md) for architecture
   - Check [FEATURE_STATUS.md](FEATURE_STATUS.md) for what needs work
   - Write tests (see [TESTING.md](TESTING.md))

## Getting Help

- **In the REPL**: Type `(exit)` or Ctrl+D to quit
- **Documentation**: See [docs/README.md](README.md) for all guides
- **Issues**: Report bugs on GitHub

## Have Fun!

Patina is designed to be a pleasant Scheme development experience. The REPL with syntax highlighting and multi-line editing makes writing Scheme code enjoyable.

Happy hacking! 🦀✨
