# Patina VM: Testing

**Status:** All tests passing — 1226/1226 R7RS chibi tests on both backends, ~1400 internal tests

---

## 1. Test Layers

```
Layer 4: R7RS compliance (chibi-scheme r7rs-tests.scm)
            ↑ ./scripts/run_chibi_tests.sh (VM, default)
Layer 3: Shared integration tests — every case runs on BOTH backends
            ↑ cargo test --package patina-tests
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

# Shared integration tests (runs every case on both backends)
cargo test --package patina-tests

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
(1226 tests) against the VM backend (the default). This is the primary
correctness gate. `./scripts/run_chibi_tests_tree_walker.sh` runs the same suite
against the tree-walker backend.

### Layer 3 — Shared Integration Tests

The helpers in `crates/patina-tests/tests/common/mod.rs` evaluate each program
on **both** backends and hold both to the same expectation, so a divergence
fails a test instead of waiting to be found by hand. There is no backend
feature flag — one `cargo test` run covers the tree-walker and the VM.

A *known* divergence is quarantined explicitly with the `_on` helper variants
(`assert_program_eval_to_on(On::Vm, …)`), each commented with the reason and a
pointer to the tracking doc. Those call sites are the inventory of known
backend divergences — Track Q §7's track-level metric — and
`rg 'On::(Vm|TreeWalker)' crates/patina-tests` lists them all.

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

## 5. Profiling and Perf Measurement

The workflow behind every Track P item (history and rankings live in
`PRD/TRACK_P_PERFORMANCE_PRD.md`). Rule one: **profile first** — every
lever in that PRD that skipped this step turned out to be mis-ranked.

### Sampling profile (macOS `sample`)

```bash
# 1. Release build with debug symbols (needed for readable stacks)
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release

# 2. Write a workload that runs 10-30s (size the input so the hot loop
#    dominates; library load is ~negligible after warmup)

# 3. Launch it, then sample the pid for 10s
./target/release/patina workload.scm > /dev/null &
sleep 3   # skip startup
/usr/bin/sample $! 10 -file target/profiles/<name>.txt
wait

# 4. Read the "Sort by top of stack" section at the bottom of the file —
#    that is the self-time ranking. The call-tree above it shows who calls
#    whom. Convention: keep artifacts in target/profiles/ (untracked).
```

Alternative: `samply record ./target/release/patina workload.scm` opens an
interactive Firefox Profiler UI (`scripts/profile_benchmark.sh` wraps this
for a few canned microbenchmarks).

### Measuring a change (the drift problem)

Single Criterion runs drift ±5-10% on µs-scale benches; single wall-clock
runs drift a few percent. **Never compare one run against a stored
number.** Validate with an interleaved A/B: alternate main-binary and
branch-binary runs ×3 and compare medians. For Criterion:
`--save-baseline` on main, bench the branch against it, then re-bench main
against its own baseline to measure the drift floor.

Cross-branch A/B gotcha: the binary resolves `./lib` **before**
`$PATINA_HOME/lib`, so when the two branches' `lib/` trees differ, run
each binary with its cwd inside its own checkout (e.g. a `git worktree`
for main).

### Scoreboard sweeps (r7rs-benchmarks)

The external harness protocol — copied binary + `PATINA_HOME`, subset
list, Chibi baseline — is documented in `PRD/TRACK_P_PERFORMANCE_PRD.md`
§1.2 (and the sweep-hygiene notes in §1.4). Run it after a perf item
lands, not per-commit.
