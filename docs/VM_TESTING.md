# Patina VM: Testing

**Status:** All tests passing — 1163/1163 R7RS chibi tests, ~1400 internal tests

---

## 1. Test Layers

```
Layer 4: R7RS compliance (chibi-scheme r7rs-tests.scm)
            ↑ ./scripts/run_chibi_tests.sh (VM, default)
Layer 3: Shared integration tests
            ↑ cargo test --package patina-tests --features vm-backend
Layer 2: VM-specific unit tests
            ↑ cargo test --package patina-vm
Layer 1: Crate-level unit tests (compiler passes, runtime)
            ↑ inline #[test] modules in patina-vm source
```

## 2. Running Tests

```bash
# R7RS compliance — VM (default backend, primary verification)
cargo build --release && ./scripts/run_chibi_tests.sh

# R7RS compliance — tree-walker
cargo build --release && ./scripts/run_chibi_tests_tree_walker.sh

# VM acceptance (shared integration tests)
cargo test --package patina-tests --features vm-backend

# VM crate unit tests
cargo test --package patina-vm

# All Rust tests
cargo test --all --lib --tests

# Lint
cargo clippy --all-targets --all-features -- -D warnings
```

## 3. Layer Details

### Layer 4 — R7RS Compliance

`./scripts/run_chibi_tests.sh` runs the chibi `r7rs-tests.scm` suite
(1163 tests) against the VM backend (the default). This is the primary
correctness gate. `./scripts/run_chibi_tests_tree_walker.sh` runs the same suite
against the tree-walker backend.

### Layer 3 — Shared Integration Tests

`patina-tests` crate with `vm-backend` feature flag. Tests use
`Interpreter<VmBackend>` instead of `Interpreter<TreeWalkerBackend>`.

### Layer 2 — VM Unit Tests

`cargo test --package patina-vm` runs VM-specific tests covering compiler
passes, runtime behavior, and continuation semantics.

### Layer 1 — Inline Tests

Individual compiler pass modules and runtime modules contain inline `#[test]`
functions testing specific transformations.

## 4. What Not To Test in VM Tests

- **Frontend behavior** (parsing, macro expansion) — tested by `patina-frontend`
  and `patina-macros`
- **Primitive correctness** — tested by `patina-primitives`
- **Tree-walker internals** — not the VM's concern

VM tests focus on: compilation correctness, execution correctness,
continuation semantics, and tail call behavior.
