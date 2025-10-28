# Reproducibility Solutions for Patina Testing

This document explains how we solved the reproducibility challenges with external dependencies (specifically chibi-scheme).

## The Problem

Initial test design had a critical dependency on chibi-scheme installed via Homebrew on macOS:

### Issues
1. **Platform lock-in** - Only works on macOS with Homebrew
2. **Manual setup** - Contributors must install chibi-scheme themselves
3. **Version drift** - Different Homebrew versions may have different chibi-scheme versions
4. **CI/CD failures** - GitHub Actions and other CI systems don't have chibi-scheme by default
5. **Cross-platform incompatibility** - Linux uses apt/pacman, Windows requires WSL2

## Multi-Layered Solution

We implemented a **graceful degradation** approach with multiple fallback layers:

### Layer 1: Docker (Best Reproducibility)
```bash
docker-compose up patina-test
```

**Benefits:**
- ✅ Identical environment on all platforms (Linux, macOS, Windows)
- ✅ Pinned versions of all dependencies
- ✅ No manual installation required
- ✅ Works in any CI/CD system

**Files:**
- `Dockerfile` - Defines reproducible build environment
- `docker-compose.yml` - Easy orchestration
- `.dockerignore` - Optimizes build

### Layer 2: CI/CD with Automatic Setup
```yaml
# .github/workflows/ci.yml
- name: Install chibi-scheme (Ubuntu)
  if: matrix.os == 'ubuntu-latest'
  run: |
    sudo apt-get update
    sudo apt-get install -y chibi-scheme
```

**Benefits:**
- ✅ Automated dependency installation
- ✅ Tests both with and without chibi-scheme
- ✅ Multi-OS testing (Ubuntu, macOS)
- ✅ No contributor action required for CI

**Files:**
- `.github/workflows/ci.yml` - GitHub Actions configuration

### Layer 3: Optional Dependency with Graceful Fallback
```rust
fn skip_if_chibi_unavailable() {
    if !is_chibi_scheme_available() {
        if std::env::var("SKIP_CHIBI_TESTS").is_ok() {
            println!("Skipping test: chibi-scheme not available");
            return;
        }
        panic!("Install chibi-scheme or set SKIP_CHIBI_TESTS=1");
    }
}
```

**Benefits:**
- ✅ Tests work without chibi-scheme: `SKIP_CHIBI_TESTS=1 cargo test`
- ✅ Core functionality always testable
- ✅ Clear error messages with installation instructions
- ✅ Contributors can work without external dependencies

**Implementation:**
- Runtime detection of chibi-scheme availability
- Environment variable override
- Helpful error messages with platform-specific instructions

### Layer 4: Test Separation
```
tests/
├── scheme_runner.rs  ← No external dependencies (Patina only)
├── file_runner.rs    ← Requires chibi-scheme (R7RS compliance)
└── schemes/          ← R7RS-compliant test files
```

**Benefits:**
- ✅ Core tests (`--test scheme_runner`) always run
- ✅ Compliance tests (`--test file_runner`) are optional
- ✅ Clear separation of concerns
- ✅ Can develop/test Patina without chibi-scheme

## Usage Matrix

| Scenario | Command | Requirements |
|----------|---------|--------------|
| **Full test suite** | `cargo test` | chibi-scheme |
| **Core tests only** | `cargo test --lib --test scheme_runner` | None |
| **Skip compliance** | `SKIP_CHIBI_TESTS=1 cargo test` | None |
| **Docker (recommended)** | `docker-compose up patina-test` | Docker |
| **CI/CD** | Auto-installs chibi-scheme | None (automated) |

## Platform-Specific Instructions

### macOS
```bash
brew install chibi-scheme  # Optional
cargo test                 # Full suite
```

### Linux (Ubuntu/Debian)
```bash
sudo apt-get install chibi-scheme  # Optional
cargo test                          # Full suite
```

### Linux (Arch)
```bash
sudo pacman -S chibi-scheme  # Optional
cargo test                   # Full suite
```

### Windows
```bash
# Option 1: Docker (recommended)
docker-compose up patina-test

# Option 2: WSL2
wsl --install
# Then follow Linux instructions
```

### Any Platform (Docker)
```bash
docker-compose up patina-test  # Always works
```

## Version Pinning Strategy

### Strict (Docker)
- Base image: `rust:1.75-slim` (pinned)
- chibi-scheme: From Debian packages (stable version)
- All dependencies: Locked in `Cargo.lock`

### Flexible (Native)
- Rust: Any 1.70+ (specified in CI)
- chibi-scheme: Any version from package manager
- Dependencies: Locked in `Cargo.lock`

**Rationale:** chibi-scheme is R7RS-compliant across versions, so minor version differences are acceptable.

## CI/CD Strategy

Our GitHub Actions workflow tests three scenarios:

1. **Full test suite (Ubuntu)** - Install chibi-scheme, run all tests
2. **Full test suite (macOS)** - Install chibi-scheme, run all tests
3. **Without chibi-scheme (Ubuntu)** - Skip compliance tests, verify core functionality

This ensures:
- ✅ Tests work on multiple platforms
- ✅ Tests work without external dependencies
- ✅ No contributor sees unexpected failures

## Future Improvements

### 1. Multiple Reference Implementations
Support testing against:
- chibi-scheme (current)
- Guile
- Racket (with R7RS mode)
- MIT Scheme

**Implementation:**
```rust
enum ReferenceScheme {
    Chibi,
    Guile,
    Racket,
}

fn run_reference_scheme(impl: ReferenceScheme, file: &Path) -> Result<String>
```

### 2. Binary Snapshot Testing
Pre-generate expected outputs and commit to repo:
```
tests/
└── snapshots/
    ├── arithmetic_basic.txt
    ├── lists_basic.txt
    └── ...
```

**Benefits:**
- No external dependency needed
- Faster tests
- Explicit tracking of behavior changes

### 3. Self-Hosting
Once Patina is feature-complete, use Patina to test itself:
```bash
patina tests/schemes/arithmetic/basic.scm
```

## Best Practices

### For Contributors
1. **Start simple**: Use `cargo test --lib` to test core functionality
2. **Install chibi-scheme** (optional): For full compliance testing
3. **Use Docker** (recommended): For identical environment

### For CI/CD
1. **Always test without chibi-scheme**: Verify core functionality
2. **Test with chibi-scheme**: Verify R7RS compliance
3. **Use Docker in deployment**: Ensure reproducibility

### For Releases
1. **Docker image**: Publish official Docker image with all dependencies
2. **Binary releases**: Include platform-specific binaries
3. **Documentation**: Clear installation instructions per platform

## Conclusion

The multi-layered approach ensures:
- ✅ **Reproducibility** - Docker provides identical environments
- ✅ **Flexibility** - Works with or without chibi-scheme
- ✅ **Accessibility** - Low barrier to contribution
- ✅ **Reliability** - Tests always pass in CI/CD
- ✅ **Maintainability** - Clear separation of test types

Contributors can start testing immediately without any external dependencies, while CI/CD and Docker provide full reproducibility when needed.
