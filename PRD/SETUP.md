# Development Setup Guide

This guide explains how to set up Patina for development and testing with maximum reproducibility across different platforms.

## Quick Start

### Option 1: Using Docker (Recommended for Reproducibility)

```bash
# Build and run tests
docker-compose up patina-test

# Interactive development environment
docker-compose run patina-dev bash
```

Inside the container:
```bash
cargo test          # Run all tests
cargo run           # Start REPL
cargo build --release
```

### Option 2: Local Installation

#### Prerequisites
1. Rust (1.70 or later)
2. chibi-scheme (optional, for R7RS compliance testing)

#### Install Rust
```bash
# Using rustup (recommended)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Install chibi-scheme

**macOS (Homebrew)**
```bash
brew install chibi-scheme
```

**Ubuntu/Debian**
```bash
sudo apt-get update
sudo apt-get install chibi-scheme
```

**Arch Linux**
```bash
sudo pacman -S chibi-scheme
```

**From Source**
```bash
git clone https://github.com/ashinn/chibi-scheme.git
cd chibi-scheme
make
sudo make install
```

**Windows**
- Use WSL2 with Ubuntu and follow Linux instructions
- Or use Docker (recommended)

#### Build and Test
```bash
# Clone the repository
git clone https://github.com/yourusername/patina.git
cd patina

# Build
cargo build

# Run all tests (requires chibi-scheme)
cargo test

# Run only Patina tests (no chibi-scheme needed)
cargo test --lib
cargo test --test scheme_runner

# Skip chibi-scheme tests explicitly
SKIP_CHIBI_TESTS=1 cargo test
```

## Test Categories

### 1. Core Tests (No External Dependencies)
These tests always run and don't require chibi-scheme:
```bash
cargo test --lib              # Unit tests
cargo test --test scheme_runner   # Patina integration tests
```

### 2. Compliance Tests (Requires chibi-scheme)
These tests compare Patina with chibi-scheme:
```bash
cargo test --test file_runner
```

To skip if chibi-scheme is unavailable:
```bash
SKIP_CHIBI_TESTS=1 cargo test
```

## CI/CD

The project includes GitHub Actions workflows that:
- Test on Ubuntu and macOS
- Install chibi-scheme automatically
- Run full test suite
- Verify tests work without chibi-scheme (fallback mode)

See `.github/workflows/ci.yml` for details.

## Docker Details

### Building the Image
```bash
docker build -t patina:latest .
```

### Running Tests
```bash
docker run --rm patina:latest cargo test
```

### Interactive Development
```bash
docker run -it --rm -v $(pwd):/app patina:latest bash
```

### Using Docker Compose
```bash
# Run tests
docker-compose up patina-test

# Development shell with volume mounting
docker-compose run patina-dev bash

# Clean up
docker-compose down -v
```

## Troubleshooting

### chibi-scheme not found
**Error:** `chibi-scheme not found in PATH`

**Solutions:**
1. Install chibi-scheme (see installation instructions above)
2. Skip chibi-scheme tests: `SKIP_CHIBI_TESTS=1 cargo test`
3. Use Docker: `docker-compose up patina-test`

### Platform-specific Issues

**macOS: Homebrew installation fails**
```bash
brew update
brew install chibi-scheme
```

**Ubuntu: Package not found**
```bash
sudo apt-get update
sudo apt-get install software-properties-common
sudo apt-get install chibi-scheme
```

**Windows: Use WSL2 or Docker**
- Install Docker Desktop
- Use `docker-compose up patina-test`

## Editor Setup

### VS Code
Install extensions:
- rust-analyzer
- CodeLLDB (for debugging)
- Even Better TOML

### Vim/Neovim
Use rust.vim or rust-tools.nvim

### Emacs
Use rustic-mode

## Development Workflow

1. **Make changes** to source files
2. **Run tests** frequently:
   ```bash
   cargo test
   ```
3. **Check formatting**:
   ```bash
   cargo fmt
   ```
4. **Run linter**:
   ```bash
   cargo clippy
   ```
5. **Build release**:
   ```bash
   cargo build --release
   ```

## Version Pinning

The project uses:
- **Rust:** Minimum 1.70 (specified in CI)
- **chibi-scheme:** Any version from package manager
- **Dependencies:** Locked in `Cargo.lock` (commit this file)

For absolute reproducibility, use Docker with pinned base image.

## Contributing

When adding tests:
1. Add Patina-only tests to `tests/scheme_runner.rs` (no external deps)
2. Add compliance tests to `tests/file_runner.rs` (requires chibi-scheme)
3. Update `.github/workflows/ci.yml` if new dependencies are needed
4. Document any platform-specific requirements here

## Platform Support Matrix

| Platform | Patina Tests | chibi-scheme Tests | Docker |
|----------|--------------|-------------------|---------|
| macOS | ✅ | ✅ (brew) | ✅ |
| Linux | ✅ | ✅ (apt/pacman) | ✅ |
| Windows | ⚠️ WSL2 | ⚠️ WSL2 | ✅ |

✅ = Fully supported
⚠️ = Requires additional setup

## FAQ

**Q: Do I need chibi-scheme to work on Patina?**
A: No! Most tests work without it. Use `SKIP_CHIBI_TESTS=1` or test with `--test scheme_runner`.

**Q: Why use chibi-scheme specifically?**
A: It's a lightweight, standards-compliant R7RS implementation. But the infrastructure could support other implementations too.

**Q: Can I use this in CI/CD without Docker?**
A: Yes! See `.github/workflows/ci.yml` for examples of installing chibi-scheme in GitHub Actions.

**Q: How do I regenerate expected outputs?**
A: `cargo test --test file_runner -- --ignored`

## Next Steps

After setup:
- Read [TESTING.md](TESTING.md) for test infrastructure details
- Check [NEXT_STEPS.md](NEXT_STEPS.md) for feature roadmap
- Run `cargo run` to try the REPL
