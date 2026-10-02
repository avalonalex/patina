#!/bin/bash
# Reproduce the workload-demographics measurements (Patina main @ 28a94f8, macOS arm64).
# Retained copy: SCRATCH is now the probe directory (PRD/study/gc/probes/workload-demographics).
# The script writes bin/, demographics/, workloads/out/ and target/ beside itself, so copy that
# directory outside the repository before running it. Nothing else writes into the repo.
set -euo pipefail
SCRATCH=$(cd "$(dirname "$0")/.." && pwd)
REPO=~/Project/patina
I=$SCRATCH/instrumented; D=$I/demographics; W=$I/workloads

# 1. Baseline release binary from the repo, into the scratch target dir.
(cd $REPO && CARGO_TARGET_DIR=$SCRATCH/target cargo build --release -p patina-repl --bin patina)
mkdir -p $I/bin && cp $SCRATCH/target/release/patina $I/bin/patina-base

# 2. Instrumented copy (cp -R, then apply demographics.patch), built into its own target dir.
#    (scheme_tests test-lib compat spec examples docs scripts are copied only for step 6.)
mkdir -p $D && (cd $REPO && cp -R Cargo.toml Cargo.lock rust-toolchain.toml crates lib $D/)
(cd $I && patch -p0 -d . < demographics.patch) || true   # already applied in this scratch tree
(cd $D && CARGO_TARGET_DIR=$D/target cargo build --release -p patina-repl --bin patina)
cp $D/target/release/patina $I/bin/patina-demog

# 3. Workloads: Larceny R7RS src + common.scm (assembled into $W/src), reduced inputs in $W/inputs,
#    adapted Larceny GC programs and extras in $W/extra. run1.sh BIN NAME runs one.
L=~/Project/reference/larceny/test/Benchmarking/R7RS
for f in $L/src/*.scm; do b=$(basename $f .scm); [ $b = common ] || cat $f $L/src/common.scm > $W/src/$b.scm; done

# 4. Timing baseline (GC on vs PATINA_GC=0, /usr/bin/time -l, 3 interleaved reps) + pause-mode run.
(cd $W && ./drive_time.py)
# 5. Full instrumentation: default policy and PATINA_GC_STRESS = 16K/64K/256K/1M/4M allocations.
(cd $W && ./drive_full.py 6)
#    Tree-walker contrast:
for b in nboyer deriv fibfp destruc; do
  PATINA_DEMOG=full PATINA_DEMOG_OUT=$W/out/$b/tw.txt PATINA_DEMOG_GCLOG=$W/out/$b/tw.csv \
    $W/run1.sh $I/bin/patina-demog $b --tree-walker > $W/out/$b/tw.stdout 2>&1
done
python3 $W/analyze.py > $W/out/tables.md; python3 $W/agg.py; python3 $W/work.py; python3 $W/today.py

# 6. Heap instances per `cargo test -p patina-tests` process.
(cd $REPO && cp -R scheme_tests test-lib compat spec examples docs scripts $D/)
(cd $D && SKIP_CHIBI_TESTS=1 PATINA_DEMOG_HEAPS=$W/out/heaps.log CARGO_TARGET_DIR=$D/target \
   cargo test -p patina-tests --no-fail-fast > $W/out/cargo_test.log 2>&1)

# 7. Address-space reservation probe.
(cd $SCRATCH && CARGO_TARGET_DIR=$SCRATCH/target cargo build --release)
$SCRATCH/target/release/mmap-probe

# 8. Inventory checks: repo workloads.json through compare_source(); Larceny R7RS suite at count=1.
python3 $W/repo_bench/run_repo_workloads.py
python3 $W/larceny_scan/scan.py
