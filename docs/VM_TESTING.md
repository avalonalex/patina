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

`./scripts/run_larceny_tests.sh` is the second opinion: Larceny's R7RS test
suites (Will Clinger's rewrite of Racket's R6RS suite — 33 suites covering
R7RS-small and the Red-edition libraries, plus an R6RS lane behind `--r6rs`).
They are LGPL, so they are not vendored; the script runs them from a reference
checkout outside the repo and prints the fetch command when it is missing. It
is local and on-demand, not a CI gate; its reports land in
`scheme_tests/reports/larceny*.md`. See Track L §L5.3 for the baseline and the
defect queue it produced.

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

### Checked benchmark lanes

Use Python 3 and the pinned Rust toolchain. The runners work from any current
working directory, use this checkout's isolated libraries, enable normal GC,
and disable scope tracing. Cargo chooses the executable even when
`CARGO_TARGET_DIR` is set. Backend names are part of every Criterion ID.

| Category / ID below `<backend>/` | Inside the clock | Outside the clock |
|---|---|---|
| `end_to_end/<workload>` | `eval_program`: parse, expand, compile/lower, execute, automatic GC | Interpreter bootstrap, workload definitions, source construction, correctness checks, result formatting |
| `phases/startup/bootstrap_and_drop` | Fresh interpreter construction, base-library bootstrap, teardown | OS process launch; this is cold interpreter state, **not** a cold filesystem/cache measurement |
| `phases/frontend/parse_expand_lower` | Read a named-let sum, macro expansion, VM compilation or CPS lowering; intermediate cleanup | Fresh bootstrapped heap for each iteration, execution, output and interpreter destruction |
| `phases/execution/sum_100` | Repeated calls to an already loaded sum procedure, Scheme driver loop, timer overhead, automatic GC | Bootstrap, driver parsing/compilation, answer check after the final clock read |
| `phases/allocation_gc/list_256` | Repeated construction and length traversal of 256-pair lists, driver loop, automatic GC | Same exclusions as execution; the separate fixed-size GC counter probe |

The two execution lanes use Criterion's `iter_custom` and Scheme's monotonic
`current-jiffy` clock. They are steady-state procedure measurements, including
call/loop overhead; they do not isolate a single bytecode instruction. The
frontend lane uses per-iteration batched setup to avoid an ever-growing heap.
Its wall time includes that untimed bootstrap, so it is slower to run than its
reported frontend time suggests. These categories are independent workloads;
their medians are not additive.

```bash
# 18 single-sample execution comparisons, with checked answers on both backends.
# This is a smoke check; one row is not statistical evidence of a speedup.
./scripts/bench_compare.sh --quick

# Four phase measurements, plus one representative end-to-end workload.
# Repeat with --backend tree-walker for the other backend.
./scripts/run_benchmarks.sh --quick --backend vm \
  --filter 'phases/|end_to_end/r7rs/sum/1000$'

# All 40 end-to-end workloads and four phases with longer sampling.
./scripts/run_benchmarks.sh --backend vm

# Optional archival copy: choose a NEW filename; existing baselines are protected.
./scripts/run_benchmarks.sh --quick --backend vm --filter phases/ \
  --output /tmp/patina-vm-phases.json

# No timing suite: offline failure-injection tests, already included in CI.
python3 -B -m unittest discover -s scripts/tests -p 'test_benchmarks.py'

# Run every Rust benchmark body once, including all semantic prechecks.
PATINA_ISOLATED_LIBRARIES=1 cargo bench -p patina-tests --bench scheme_benchmarks -- --test
PATINA_ISOLATED_LIBRARIES=1 PATINA_BENCH_BACKEND=tree-walker \
  cargo bench -p patina-tests --bench scheme_benchmarks -- --test
```

Each runner creates a fresh `target/benchmark-runs/<UTC timestamp>-<unique>/`.
Only a complete successful run produces `report.json`. Interpreter/Cargo
failures retain diagnostics and exit nonzero; missing, nonfinite, nonpositive,
or unchecked results also fail. Criterion reports come from its structured
sample/estimate files, matched against the harness's correctness ledger, never
from parsing terminal tables. A filter matching nothing is an error. No report
or history from an earlier run is overwritten, and VM samples cannot reuse a
tree-walker baseline. `--filter` is a substring in the comparison runner and a
Criterion regex in the Criterion runner.

Reports record UTC time, full Git revision, dirty state, toolchain, platform,
CPU when available, build overrides, backend, workload-manifest hash, sampling
settings, expected answers and extra semantic checks. Both runners share
`crates/patina-tests/bench_programs/workloads.json`; definitions load before the
clock starts. The comparison runner measures execution within a fresh process
for each case; its list/setup boundaries now agree with Criterion, so old
comparison results must not be mixed with these measurements.

The allocation report also includes a separate probe of **1,000 calls × 256
pairs**, after a requested full collection. Snapshots before/after the workload
and after another requested collection report arena slots, free slots, symbols,
allocations since the latest collection, collection count and the latest swept
count. Subtract collection counts to see automatic collections during the
probe, which also includes parsing/compilation of its driver and answer check.
Slot counts are not bytes or RSS; `allocations_since_gc` is not total
allocation count, and `last_swept` is not cumulative. No GC pause latency or
allocation-throughput claim can be made from these counters.

The dated 2026-09-30 baseline is linked from
[the performance report](../benchmark_reports/performance.md). Older reports
remain historical records. There is no CI timing threshold; the interleaved
measurement procedure below still applies before making a performance claim.

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
