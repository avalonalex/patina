# Workload demographics for the Patina GC redesign (gap: workload-demographics)

Revision measured: `main` at `28a94f8`.

Machine: Apple M4 Pro, 12 cores, 24 GB, macOS 27.2 arm64, 16 KiB pages.

Tags: **[V]** means measured or verified in source for this report; **[I]** means inference. Repo references are `file:line` at `28a94f8`.

Where things are:
- The instrumentation is retained in `PRD/study/gc/probes/workload-demographics/instrumented/`.
- The patch is `instrumented/demographics.patch` (13 files).
- The commands are in `instrumented/REPRODUCE.sh`.
- Raw outputs were in `instrumented/workloads/out/<workload>/<config>.{txt,csv,stdout}`, and the script-generated tables in `out/tables.md`, `out/datasurv.md` and `out/work_table.md`; neither was retained, and this report carries their numbers.

---

## 0. Summary

1. **Benchmarks.**
   - The repo's own suite cannot exercise the collector. All 41 `workloads.json` cases pass on a release build, but the longest takes 99 ms, and only 4 cases collect at all, at most 4 times [V].
   - `~/Project/r7rs-benchmarks` does not exist [V].
   - Larceny's R7RS suite is local. Of its 74 programs, 58 complete at count 1, 14 exceed 30 s at full input size, `read0` gives a wrong result and `set` errors [V].
   - I built a 20-workload GC set from Larceny R7RS, Larceny GC (`gcold`, `queue3`) and three extras. Each takes 0.9–3.8 s on the VM [V].
2. **What is allocated: 348 M objects over the 20 workloads** [V].
   - By count: closures 35.3%, pairs 27.9%, boxed flonums 19.8%, vectors 8.8%, `MutableCell`s 5.3%, records 1.0%, identifiers 0.9%. Strings are almost absent (2.9 K allocations).
   - By bytes in the hypothetical layout: closures 39.7%, vectors 36.3% (mostly gcold's 816 B vectors), pairs 11.6%, flonums 8.3%.
   - The same allocations cost about **2.05×** as many bytes in today's representation [I, estimate].
3. **Object sizes** (hypothetical layout, gcold excluded) [V].
   - 56% of objects are exactly 16 B, 80% are ≤32 B, 97% are ≤64 B and 99.2% are ≤128 B. The mean is 26.9 B.
   - Only 148 of 348 M objects exceed 8 KiB.
4. **Survival** [V]. Survival here means the share of objects allocated in an interval that are live at its end; the first collection is excluded because it contains bootstrap.
   - At an interval of 64 K allocations (≈1–1.5 MB):
     - 10 of 20 workloads are at ≤1.5% (11 at ≤2.3%, counting gcbench).
     - hashtable0, earley, dynamic, eqtable and gcold are at 13–25% by count.
     - nboyer is at 43%, mperm at 58%, and queue3 and deeprec at 100%.
   - A 16× larger interval (1 M) helps the retaining workloads only modestly.
   - **Closures, cells and flonums almost never survive.** With them excluded, survival at 64 K is 41–56% for hashtable0, dynamic, earley and eqtable, and 100% for mperm. Today's favourable nursery numbers rest partly on garbage the compiler creates.
5. **Nursery payoff** [V numbers, I verdict].
   - Today's adaptive collector marks 0.49–0.60 objects per allocated object on each of the 10 workloads whose live heap exceeds about 30 K objects.
   - It also sweeps 1.1–3.9 arena slots per allocation on every workload.
   - A 64 K nursery would copy 0.01–0.25 objects per allocation on seven workloads (gcbench, gcold, nucleic, hashtable0, dynamic, earley, eqtable). It would copy about as many as today on nboyer and mperm, and 1.7–1.8× as many on queue3 and deeprec.
   - Verdict: a bump-allocated nursery pays mainly by removing sweep, free-list allocation and fat slots. It pays in tracing only for a minority of workloads, and it needs adaptive sizing or pretenuring. This is consistent with Wingo's nboyer result (whippet-misc §1.6).
6. **Stores and barrier filters** [V].
   - Heap stores run at 0.1–70 per 1,000 VM instructions (249 during libload's parsing); the median workload is about 13/1,000.
   - The immediate-value test filters 64% of heap stores in aggregate.
   - After the immediate filter and a young-target filter, 11.4% of all heap stores remain at the 64 K view (median workload 0.25%), and 10.8% are old→young edges.
   - Those edges go to very few objects: at most 36 distinct targets per 64 K interval on average, and nboyer's 888 K edges hit one cell.
   - The edges come from boxed `set!` variables (`MutableCell`) and hash-bucket vectors.
7. **Flonums** [V]. Boxed flonums are 19.8% of all allocations, and 79–99.8% of allocations in fibfp, mbrot and nucleic. They essentially never survive a 64 K interval.
8. **Other rates** [V].
   - **Identity hashing.** `identity-hash` is never called by the Larceny-derived workloads, because the SRFI 128 eq-comparator hashes structurally. The `(srfi 69)` `(make-hash-table eq?)` path makes one call per insert or lookup (1.66 M in eqtable), and every one is derived from the heap index.
   - **Continuations.** Only ctak and fibc capture continuations: 1.2–1.9 M captures, each copying 211–323 registers.
   - **Deep stacks.** At a collection, deeprec has 960 K frames and 11.5 M registers.
9. **Current collector** [V].
   - GC on costs −35% to +17.6% wall time against GC off. Off is slower on ctak and fibc because the process reaches 5 GB RSS.
   - Mean pause is 0.15–20 ms. The worst pauses are 41 ms (queue3) and 178 ms (the first collection after loading 25 libraries).
   - Pauses add up to 2.4–21.7% of run time.
   - The count-based trigger lets gcold reach 628 MB RSS with at most 12 MB live (hypothetical layout).
10. **Process facts** [V].
    - Up to **318** `Heap`s are live in one `cargo test -p patina-tests` process; 4,200 are created across 276 processes.
    - 272 of the 276 processes exit with every heap they created still live.
    - Reserving 64 × 16 GiB `PROT_NONE` takes 0.8 ms. Reserving 4,096 × 16 GiB (64 TiB), or a single 64 TiB region, also succeeds.
    - Only 1 of 64 returned regions was 4 GiB-aligned.

---

## 1. Method

### 1.1 Instrumented copy [V]

**Build.** `cp -R` of `Cargo.toml Cargo.lock rust-toolchain.toml crates lib` into `instrumented/demographics/`, built with `CARGO_TARGET_DIR=…/demographics/target`. The new module is `crates/patina-core/src/demog.rs`; 12 existing files gain hooks.

**Modes.** `PATINA_DEMOG=full` turns on the census, epochs, survival scan and stores. `PATINA_DEMOG=pause` records collection timing only. Output goes to `PATINA_DEMOG_OUT` (summary), `PATINA_DEMOG_GCLOG` (one CSV row per collection) and `PATINA_DEMOG_HEAPS` (heap counts).

**Allocation census.** Hooks sit in `alloc_pair`, `alloc_vector`, `alloc_string_chars` and `alloc_object` (`heap/mod.rs:703-714,770-781,831-842,1469-1480`). They record:
- pairs;
- vectors and strings by length;
- all 28 `HeapObjectData` variants;
- `VmClosure` by free-variable count, `Record` by field count, `Values` by length.

**Hypothetical layout used for byte counts.**

| Object | Size |
|---|---|
| pair | 16 B |
| vector | 8 + 8n |
| string | 8 + round8(4n) |
| flonum | 16 |
| closure | 16 + 8·fv |
| record | 8 + 8·fields (the header is the RTD) |
| cell | 16 |
| identifier, parameter, promise, ephemeron | 24 |
| `Rc`-wrapping "foreign" objects | 16 |
| bignum | 8 + 8·digits |

**Survival.**
- Each arena slot has a side record holding its allocation sequence number and the collection count at the time it was allocated.
- In `MarkSweepCollector::collect` (`gc.rs:1105-1118`), a scan runs between mark and sweep. It classifies every non-free slot as live or dead and young or old, with its bytes, class and age.
- The scan's time is excluded from the pause. The pause is mark + sweep, the same quantity `last_pause_micros` measures. The VM safepoint total (retire + collect) is timed separately in `maybe_collect` (`vm_state.rs:1287`).
- Fixed intervals reuse `PATINA_GC_STRESS=n` (`gc.rs:301-317`) with n = 16 K, 64 K, 256 K, 1 M and 4 M. A full mark at interval N yields exactly the survivors a nursery of N allocations would have, given a perfect remembered set.

**Stores.** Every listed channel is hooked:
- `set_car`, `set_cdr`, `vector_set`;
- the VM `VectorSet` (its only `vector_slice_mut` user, `vm_state.rs:2380`);
- `write_mutable_cell`, `set_vm_closure_free_var`, `promise_update`, `break_ephemeron`;
- `%record-set!` (`records.rs:253`);
- both parameter installs (`parameters.rs:144,155→167`);
- primitive and VM promise forcing (`lazy.rs:134`, `control.rs:~2083`);
- `Environment::define`, `set_slot_value` and the scoped `set` path (`environment.rs:712,620,920`);
- the tree-walker continuation `define` (`continuation.rs:239`, de-duplicated against `define`).

Each store records:
- whether the value is immediate;
- target and value youth under six views: "since the last real collection", and "since the last N-allocation boundary" for the five values of N. One run therefore gives every nursery size.
- the distinct target objects of old→young edges in each interval, a measure of remembered-set pressure.

Environment slots are off-heap, so they are treated as never-young targets.

**Also counted:** `identity-hash` (immediate / by index / by `Rc`), `equal-hash` and its heap-index fallbacks, `capture_full`, `restore` and `capture_delimited` (frames and registers), VM instructions, and frames and registers at each safe point.

**Validation** [V]:
- Every run had exactly one `Heap` (all 151 summaries).
- Every workload answer was correct.
- `WriteCell` is 2.2% of nboyer dispatches (8.87 M / 402.7 M), matching jit-readiness §2.3.
- GC-off RSS matches the today-layout byte estimate of §4.4: fibfp 1,400 vs 1,388 MB, mbrot 2,684 vs 2,674 MB.

### 1.2 Caveats

1. **Deferred regions stretch intervals.** Collection happens only at outermost safe points (gc-impl §5). `libload` allocated 7.50 M objects across its 25 library loads before the first collection, so in-load survival cannot be sampled. Only post-load survival can: 0.29% of 7.5 M [V].
2. **The mix is a property of today's compiler** [V facts, I consequence].
   - Internal `define`s allocate closures on every call: quicksort's `partition` makes three per call, so 93% of quicksort's allocations are closures.
   - Mutated captured variables are boxed in cells.
   - `%make-record` first builds a field vector (`lib/scheme/base/records.scm:32`, `records.rs:138-182`). gcbench's 3.31 M length-4 vectors are exactly its 3.31 M records.

   Better closure conversion, or a JIT, would remove the shortest-lived garbage and raise survival fractions (§5.3).
3. **Bootstrap is negligible** [V]. `(scheme base read write time)` costs 25.9 K allocations and 16 K stores, with no collection. It is included in all totals.
4. **Timing** [V]. Timings are single-machine wall clock: medians of 3 interleaved on/off runs. A second full pass agreed within about 0.05 s.

---

## 2. (a) Benchmark inventory

| Source | What it is | Status on the release `patina` |
|---|---|---|
| `crates/patina-tests/bench_programs/*.scm` + `workloads.json` | 10 programs plus `common.scm`; 41 cases (40 criterion, 18 quick, 25 compare) | **All 41 pass** through the `compare_source` wrapper of `scripts/benchmarks.py`, run on the base binary [V]. The longest are `fib/30` 99 ms, `sboyer/0` 65 ms and `nboyer/0` 54 ms. **Only 4 cases ever collect** (`nqueens/10` 3, `deriv/1000_iter` 1, `nboyer/0` 4, `sboyer/0` 2) [V] |
| `benches/scheme_benchmarks.rs` | Criterion over the same cases, plus phase lanes: `startup/bootstrap_and_drop`, `frontend/parse_expand_lower`, `allocation_gc/list_256` (1000×256-pair probe, `:177-268`) | Not run as a harness. `run_benchmarks.sh` and `bench_compare.sh` call `benchmarks.py`, which needs a git checkout and writes to the repo's `target/benchmark-runs` (`benchmarks.py:228-231`) |
| Larceny R7RS (`test/Benchmarking/R7RS`: 74 `src/*.scm` + `common.scm`) | A program is `src/X.scm` + `common.scm` (`hide`, `run-r7rs-benchmark`), with input on stdin from `inputs/X.input` (`bench:201,210`). The `prefix/` and `suffix/` harness files exist only in `test/Benchmarking/CrossPlatform/` (17 prefix files) and are not used by this suite [V] | Count = 1, full inputs, 30 s limit: **58 ok**, **14 timeout**, **`read0` wrong result**, **`set` error**. The timeouts are cpstak, ctak, earley, gcbench, graphs, ilist, lattice, lseq, ntakl, stream, tak, takl, text and vecsort. The `set` error is `%record-ref: expected record, got procedure` in SRFI 128's comparator accessors. Every timeout I reran with reduced input completed [V] |
| Larceny GC (`test/Benchmarking/GC/*.sch`) | gcbench, gcold, perm, queue3, pueue4, nboyer, sboyer, earley, nucleic2, dynamic, twobit (23.8 K lines), graphs, lattice, paraffins | **Adapted:** gcold and queue3, by prepending a small R7RS `run-benchmark` (`workloads/extra/`). **Taken from their R7RS ports instead:** gcbench, perm (as `mperm`), nboyer, sboyer, earley, nucleic and dynamic. **Not adapted:** twobit and pueue4 |
| `~/Project/r7rs-benchmarks` (cited by `PRD/TRACK_P_PERFORMANCE_PRD.md`) | — | **Missing** [V] |

**Chosen GC set.** Inputs are in `workloads/inputs/`; numbers in parentheses are the reduced sizes.

| Kind | Workloads |
|---|---|
| Allocation-heavy | nboyer (3), deriv (300 K), gcbench (depth 16) |
| Mutation-heavy | destruc (200; `set-car!`), quicksort (50; `vector-set!`), gcold (8 MB live, 150 steps; captured-variable `set!` and old-tree `vector-set!`) |
| Large live heap | mperm (4:9:2:1), queue3 (100 lists of 200 K two-element vectors, 10 live), gcold |
| Flonum-heavy | fibfp (5 × fib 30.0), mbrot (20), nucleic (3) |
| Continuation-heavy | ctak (30 × 18 12 6), fibc (5 × 25), generator (100) |
| Deep non-tail recursion | `deeprec` (extra): build and sum a 1 M-element list recursively, 10 times |
| Library loading | `libload` (extra): import 25 R7RS-large libraries, then 200 K conses |
| Supplementary | hashtable0 (SRFI 125), `eqtable` (extra: SRFI 69 `(make-hash-table eq?)` with 5 × 100 K pair keys), dynamic (20), earley (12) |
| Baseline | `empty` (the four imports only) |

---

## 3. (e) Current-collector baseline

Run times come from the base binary: medians of 3 interleaved runs, peak RSS from `/usr/bin/time -l`. Pauses come from one instrumented `PATINA_DEMOG=pause` run.

| workload | GC on s | GC off s | GC cost % | RSS on MB | RSS off MB | collections | mean pause ms | max pause ms | pause % of run | safepoint max ms |
|---|---|---|---|---|---|---|---|---|---|---|
| nboyer | 2.28 | 2.21 | 3.2 | 111 | 170 | 20 | 3.26 | 11.93 | 2.8 | 11.94 |
| deriv | 0.89 | 0.85 | 4.7 | 15 | 297 | 334 | 0.15 | 0.77 | 5.0 | 1.73 |
| destruc | 2.78 | 2.73 | 1.8 | 18 | 478 | 265 | 0.26 | 0.97 | 2.5 | 1.05 |
| quicksort | 2.26 | 2.15 | 5.1 | 23 | 1936 | 268 | 0.64 | 1.01 | 7.4 | 2.00 |
| gcbench | 3.83 | 3.38 | 13.3 | 56 | 1709 | 228 | 2.19 | 4.37 | 12.8 | 4.38 |
| gcold | 1.42 | 1.37 | 3.6 | 628 | 4696 | 27 | 7.46 | 29.26 | 14.1 | 29.26 |
| mperm | 1.67 | 1.61 | 3.7 | 272 | 282 | 8 | 7.47 | 21.78 | 3.6 | 21.80 |
| queue3 | 1.16 | 1.01 | 14.9 | 286 | 779 | 9 | 20.04 | 41.07 | 15.0 | 41.09 |
| fibfp | 1.48 | 1.48 | 0.0 | 17 | 1400 | 308 | 0.17 | 0.78 | 2.4 | 0.86 |
| mbrot | 1.96 | 1.88 | 4.3 | 18 | 2684 | 532 | 0.25 | 0.85 | 7.1 | 1.73 |
| nucleic | 2.14 | 2.06 | 3.9 | 30 | 716 | 392 | 0.39 | 1.54 | 7.1 | 2.57 |
| ctak | 1.46 | 2.24 | −34.8 | 107 | 5247 | 58 | 5.49 | 7.97 | 21.7 | 7.97 |
| fibc | 1.63 | 2.22 | −26.6 | 202 | 4791 | 37 | 6.23 | 9.71 | 14.2 | 9.71 |
| generator | 3.03 | 2.88 | 5.2 | 25 | 1105 | 327 | 0.63 | 1.50 | 6.7 | 1.59 |
| deeprec | 0.87 | 0.74 | 17.6 | 215 | 272 | 7 | 13.36 | 18.53 | 10.5 | 23.68 |
| libload | 1.88 | 1.64 | 14.6 | 628 | 591 | 4 | 56.17 | 177.52 | 11.9 | 177.67 |
| hashtable0 | 2.40 | 2.17 | 10.6 | 76 | 909 | 119 | 2.81 | 5.42 | 13.1 | 5.42 |
| dynamic | 1.58 | 1.47 | 7.5 | 55 | 534 | 82 | 1.98 | 2.73 | 10.2 | 2.73 |
| earley | 1.35 | 1.25 | 8.0 | 322 | 977 | 23 | 5.56 | 22.14 | 9.2 | 22.15 |
| eqtable | 1.34 | 1.25 | 7.2 | 90 | 390 | 23 | 5.12 | 8.87 | 8.6 | 8.88 |
| empty | 0.01 | 0.01 | 0 | 11 | 11 | 0 | – | – | – | – |

**Where pause time goes** (from `out/*/pause.csv`) [V]:
- **Sweep dominates when the arena high-water mark is large:**
  - queue3: mean sweep 13.7 ms (max 30.5) against mark 6.3 ms;
  - gcold: sweep 6.8 ms (max 27.5) against mark 0.7 ms;
  - libload: the first post-load sweep takes 176 ms over 4.62 M pair slots and 2.89 M object slots.
- **Mark dominates in two cases:**
  - ctak and fibc: mark averages 5.1–5.9 ms with about 470 live heap objects. [I] The cost is weak continuation side-table work: about 33 K captures per interval, pruned inside the mark phase.
  - deeprec: mark averages 12.3 ms, mostly scanning roots in 960 K frames and 11.5 M registers.

**Count trigger** [V]. gcold's young garbage is 100-element vectors (816 B each), and each counts as one allocation. RSS reaches 628 MB with GC on, against a maximum live set of 12.1 MB in the hypothetical layout.

---

## 4. (b) Allocation demographics

### 4.1 Census

Default policy, whole process including bootstrap. Percentages are shares of allocation count.

| workload | VM instrs | allocs | allocs / 1K instr | hyp. MB | B/obj | pair | vector | flonum | closure | cell | Values | ident | record | other | flonum % of bytes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| nboyer | 402.7M | 14.66M | 36.4 | 260 | 18.6 | 89.2 | 0.0 | 0.0 | 10.6 | 0.0 | 0.0 | 0.1 | 0.0 | 0.0 | 0.0 |
| deriv | 126.0M | 21.94M | 174.1 | 372 | 17.8 | 72.6 | 1.4 | 0.0 | 19.1 | 5.5 | 1.4 | 0.1 | 0.0 | 0.0 | 0.0 |
| destruc | 303.1M | 17.41M | 57.4 | 363 | 21.8 | 49.7 | 0.0 | 0.0 | 36.9 | 13.2 | 0.0 | 0.1 | 0.0 | 0.0 | 0.0 |
| quicksort | 308.8M | 17.57M | 56.9 | 874 | 52.2 | 0.2 | 0.0 | 1.2 | 92.8 | 5.7 | 0.0 | 0.1 | 0.0 | 0.0 | 0.4 |
| gcbench | 555.9M | 44.00M | 79.2 | 1220 | 29.1 | 30.2 | 7.5 | 0.3 | 39.4 | 15.0 | 0.0 | 0.1 | 7.5 | 0.0 | 0.2 |
| gcold | 154.4M | 7.68M | 49.7 | 4009 | 547.2 | 2.4 | 81.2 | 0.0 | 15.0 | 0.0 | 0.0 | 1.4 | 0.0 | 0.0 | 0.0 |
| mperm | 250.2M | 11.79M | 47.1 | 258 | 22.9 | 57.5 | 0.0 | 0.0 | 28.3 | 14.1 | 0.0 | 0.2 | 0.0 | 0.0 | 0.0 |
| queue3 | 141.6M | 20.05M | 141.6 | 459 | 24.0 | 0.1 | 99.8 | 0.0 | 0.0 | 0.0 | 0.0 | 0.1 | 0.0 | 0.0 | 0.0 |
| fibfp | 114.4M | 20.23M | 176.8 | 309 | 16.0 | 0.1 | 0.0 | 99.8 | 0.0 | 0.0 | 0.0 | 0.1 | 0.0 | 0.0 | 99.8 |
| mbrot | 135.5M | 34.90M | 257.5 | 780 | 23.4 | 0.1 | 0.0 | 78.9 | 20.6 | 0.3 | 0.0 | 0.0 | 0.0 | 0.0 | 53.9 |
| nucleic | 190.1M | 25.76M | 135.5 | 516 | 21.0 | 2.1 | 3.0 | 80.7 | 13.9 | 0.1 | 0.0 | 0.2 | 0.0 | 0.0 | 61.4 |
| ctak | 31.0M | 3.86M | 124.4 | 103 | 27.9 | 0.7 | 0.0 | 0.0 | 49.5 | 0.0 | 0.0 | 0.4 | 0.0 | 49.5 (VmContinuationRef) | 0.0 |
| fibc | 69.6M | 2.47M | 35.4 | 47 | 20.0 | 1.0 | 0.0 | 0.0 | 49.2 | 0.0 | 0.0 | 0.6 | 0.0 | 49.2 (VmContinuationRef) | 0.0 |
| generator | 482.0M | 21.47M | 44.5 | 880 | 43.0 | 2.1 | 0.0 | 0.0 | 96.9 | 0.7 | 0.0 | 0.2 | 0.0 | 0.0 | 0.0 |
| deeprec | 150.0M | 10.03M | 66.8 | 153 | 16.0 | 99.9 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.1 | 0.0 | 0.0 | 0.0 |
| libload | 18.8M | 7.70M | 408.9 | 140 | 19.0 | 62.5 | 0.0 | 1.1 | 6.1 | 1.8 | 0.0 | 28.4 | 0.0 | 0.1 | 0.9 |
| hashtable0 | 239.6M | 25.49M | 106.4 | 563 | 23.2 | 32.7 | 0.0 | 0.0 | 56.1 | 11.0 | 0.0 | 0.2 | 0.0 | 0.0 | 0.0 |
| dynamic | 136.7M | 15.81M | 115.7 | 384 | 25.5 | 35.1 | 0.0 | 0.0 | 62.4 | 1.5 | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |
| earley | 170.1M | 11.43M | 67.2 | 735 | 67.4 | 27.4 | 0.0 | 0.0 | 58.5 | 12.9 | 0.0 | 1.3 | 0.0 | 0.0 | 0.0 |
| eqtable | 153.9M | 13.31M | 86.5 | 306 | 24.1 | 46.3 | 0.0 | 0.0 | 48.6 | 4.9 | 0.0 | 0.2 | 0.0 | 0.0 | 0.0 |
| empty | 113 | 26K | — | 0.5 | 19.0 | 61.9 | 0.0 | 0.0 | 0.1 | 0.0 | 0.0 | 36.5 | 0.0 | 1.4 | 0.0 |

**Aggregate** (20 non-empty workloads) [V]:

| class | pair | vector | string | flonum | closure | cell | identifier | record | other | total |
|---|---|---|---|---|---|---|---|---|---|---|
| allocations | 97.1M (27.9%) | 30.6M (8.8%) | 2,903 (0.0%) | 69.0M (19.8%) | 122.8M (35.3%) | 18.4M (5.3%) | 3.0M (0.9%) | 3.3M (1.0%) | 3.4M (1.0%) | 347.6M |
| hyp. bytes | 1,481 MB (11.6%) | 4,616 MB (36.3%) | 0 | 1,052 MB (8.3%) | 5,049 MB (39.7%) | 280 MB (2.2%) | 68 MB (0.5%) | 126 MB (1.0%) | 55 MB (0.4%) | 12.7 GB |

**Notes on the census:**
- `VmContinuationRef` is 49.5% of ctak's and 49.2% of fibc's heap allocations, one per `call/cc`.
- `Values` comes only from the Larceny harness `hide` (`common.scm`): 301 K, in deriv.
- `Identifier`s are 28% of libload's 7.7 M allocations and 36.5% of bootstrap.
- [V] Rate at today's speed: 1.5–25 M allocations/s, 29–544 hypothetical MB/s (gcold 2.8 GB/s because of its 816 B vectors). A 64 K-allocation interval takes 2.7–43 ms of mutator time.
- [I] A JIT that is 5–10× faster would allocate at roughly 0.3–4 GB/s. Nursery size should then follow the measured allocation rate rather than a fixed object count.

### 4.2 Lengths (sum over all workloads) [V]

| bucket | 0 | 1 | 2 | 3 | 4 | 5–8 | 9–16 | 17–32 | 33–64 | 65–128 | 129–256 | 257–1024 | 1025–8192 | >8192 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| vectors | 3 | 752 | 20.30M | 1.72M | 3.31M | 208 | 203K | 41 | 122 | 5.09M | 13 | 27 | 39 | 97 |
| strings | 39 | 138 | 288 | 67 | 275 | 259 | 235 | 1087 | 473 | 42 | 0 | 0 | 0 | 0 |
| closures by free vars | 3.64M | 20.35M | 36.24M | 17.75M | 9.19M | 31.27M | 2.26M | 2.10M | 0 | 0 | 0 | 0 | 0 | 0 |
| records by fields | 0 | 700 | 1 | 0 | 3.31M | 142 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| Values by length | 0 | 0 | 301K | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

**Vectors:**
- length 2: queue3 and deriv's harness;
- length 4: gcbench's record field vectors;
- 65–128: gcold's dead young vectors;
- length 3: nucleic and gcold tree nodes.

**Closures:**
- 1–4 free variables in 68% of closures, 5–8 in 26%, 17–32 in 2% (earley).
- Per workload:
  - nboyer: 1 fv 32%, 4 fv 68%;
  - quicksort: 5–8 fv 86%;
  - gcbench: 2–3 fv 76%;
  - earley: 9–32 fv 58%.

### 4.3 Sizes in the hypothetical layout [V]

Columns are % of allocation count by size bucket, then three byte-share columns.

| workload | ≤16 B | ≤24 | ≤32 | ≤48 | ≤64 | ≤128 | ≤256 | >256 | >8K count | bytes in ≤32 B % | bytes in >256 B % |
|---|---|---|---|---|---|---|---|---|---|---|---|
| nboyer | 89.2 | 3.6 | 0.0 | 7.2 | 0.0 | 0.0 | 0.0 | 0 | 0 | 81.4 | 0.0 |
| deriv | 83.5 | 11.0 | 5.5 | 0.0 | 0.0 | 0.0 | 0.0 | 0 | 0 | 100.0 | 0.0 |
| destruc | 62.9 | 7.3 | 23.5 | 6.2 | 0.0 | 0.0 | 0.0 | 0 | 0 | 88.6 | 0.0 |
| quicksort | 7.1 | 0.1 | 0.0 | 13.0 | 79.7 | 0.1 | 0.0 | 0 | 51 | 2.2 | 0.4 |
| gcbench | 45.5 | 0.1 | 15.0 | 37.6 | 0.0 | 1.8 | 0.0 | 0 | 1 | 41.6 | 0.1 |
| gcold | 2.4 | 1.4 | 14.9 | 14.9 | 0.0 | 0.0 | 0.0 | 66.3 | 0 | 1.0 | 97.9 |
| mperm | 71.6 | 3.2 | 3.1 | 22.1 | 0.0 | 0.0 | 0.0 | 0 | 0 | 57.6 | 0.0 |
| queue3 | 0.1 | 99.9 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0 | 0 | 100.0 | 0.0 |
| fibfp | 99.9 | 0.1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0 | 0 | 100.0 | 0.0 |
| mbrot | 79.3 | 0.0 | 9.9 | 0.3 | 0.6 | 9.8 | 0.0 | 0 | 0 | 67.7 | 0.1 |
| nucleic | 84.5 | 2.1 | 7.1 | 1.7 | 1.0 | 3.5 | 0.0 | 0 | 0 | 77.7 | 0.0 |
| ctak | 50.1 | 0.4 | 0.0 | 49.5 | 0.0 | 0.0 | 0.0 | 0 | 0 | 29.1 | 0.0 |
| fibc | 50.2 | 49.8 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0 | 0 | 100.0 | 0.0 |
| generator | 3.6 | 0.9 | 47.2 | 0.4 | 47.6 | 0.4 | 0.0 | 0 | 0 | 37.0 | 0.0 |
| deeprec | 99.9 | 0.1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0 | 0 | 100.0 | 0.0 |
| libload | 65.7 | 31.9 | 2.3 | 0.1 | 0.0 | 0.0 | 0.0 | 0 | 8 | 99.4 | 0.2 |
| hashtable0 | 49.2 | 30.2 | 13.4 | 4.8 | 2.4 | 0.0 | 0.0 | 0 | 48 | 83.8 | 2.1 |
| dynamic | 39.3 | 22.3 | 21.9 | 16.3 | 0.1 | 0.1 | 0.0 | 0 | 0 | 73.4 | 0.0 |
| earley | 40.3 | 1.3 | 3.5 | 13.1 | 7.8 | 9.9 | 24.3 | 0 | 0 | 11.7 | 0.0 |
| eqtable | 51.2 | 25.1 | 12.4 | 7.5 | 3.8 | 0.0 | 0.0 | 0 | 40 | 75.5 | 3.3 |
| **all** | 55.0 | 12.6 | 10.9 | 9.7 | 7.7 | 1.8 | 0.8 | 1.47 | 148 | 39.9 | 31.1 |

**Cumulative, excluding gcold:**

| up to | objects | bytes |
|---|---|---|
| 16 B | 56.2% | 33.4% |
| 32 B | 79.9% | 57.8% |
| 64 B | 97.3% | 89.2% |
| 128 B | 99.2% | 95.1% |
| 256 B | 100% | 99.7% |

Mean 26.9 B; 36.6 B with gcold included.

### 4.4 Today's bytes vs the hypothetical layout [I, estimate]

Today's representation was priced as follows:
- 72 B object slots;
- 24 B vector and string slots, plus a separate buffer rounded to 16 B;
- a buffer for closure `free_vars`;
- an `Rc<RefCell<Vec>>` box plus buffer per record.

The same allocations total **26.1 GB against 12.7 GB (2.05×)**. The ratio is 1.0× for deeprec and gcold, and 3.1–4.5× for flonum- and continuation-heavy code. GC-off RSS matches this estimate where nothing is freed [V].

---

## 5. (b) Survival and ages

### 5.1 Survival rate by interval length (count % / bytes %; collection #0 excluded) [V]

| workload | default policy | 16K | 64K | 256K | 1M | 4M |
|---|---|---|---|---|---|---|
| nboyer | 27.3 / 23.5 | 49.6 / 42.7 | 43.4 / 37.3 | 37.2 / 32.1 | 30.2 / 26.2 | 22.4 / 19.9 |
| deriv | 0.1 / 0.1 | 0.5 / 0.4 | 0.1 / 0.1 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 |
| destruc | 0.4 / 0.3 | 1.4 / 1.0 | 0.4 / 0.3 | 0.1 / 0.1 | 0.0 / 0.0 | 0.0 / 0.0 |
| quicksort | 0.0 / 0.4 | 0.1 / 0.5 | 0.0 / 0.4 | 0.0 / 0.4 | 0.0 / 0.3 | 0.0 / 0.1 |
| gcbench | 1.5 / 2.0 | 3.4 / 4.7 | 2.3 / 3.1 | 1.3 / 1.7 | 0.5 / 0.6 | 0.0 / 0.0 |
| gcold | 7.0 / 0.4 | 14.9 / 0.9 | 15.3 / 0.9 | 8.9 / 0.5 | 6.7 / 0.4 | n/a |
| mperm | 63.3 / 44.3 | 57.4 / 40.1 | 57.8 / 40.3 | 58.7 / 40.9 | 58.3 / 40.7 | 61.5 / 43.1 |
| queue3 | 56.0 / 56.0 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 51.0 / 51.0 |
| fibfp | 0.0 / 0.0 | 0.1 / 0.1 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 |
| mbrot | 0.0 / 0.1 | 0.1 / 0.2 | 0.0 / 0.1 | 0.0 / 0.1 | 0.0 / 0.1 | 0.0 / 0.0 |
| nucleic | 0.8 / 0.9 | 1.4 / 1.6 | 0.8 / 0.9 | 0.3 / 0.3 | 0.1 / 0.1 | 0.1 / 0.1 |
| ctak | 0.0 / 0.0 | 0.1 / 0.1 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 | n/a |
| fibc | 0.0 / 0.0 | 0.1 / 0.1 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 | n/a |
| generator | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 |
| deeprec | 56.4 / 56.4 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 32.5 / 32.5 |
| libload (post-load) | 0.8 / 0.8 | 3.2 / 3.2 | 0.8 / 0.8 | n/a | n/a | n/a |
| hashtable0 | 12.4 / 10.7 | 13.4 / 11.4 | 13.3 / 11.3 | 12.8 / 10.9 | 11.1 / 9.7 | 6.5 / 5.5 |
| dynamic | 19.1 / 12.0 | 20.7 / 13.1 | 20.2 / 12.7 | 19.0 / 11.9 | 10.1 / 6.3 | 2.8 / 1.8 |
| earley | 12.6 / 2.9 | 14.8 / 3.7 | 13.9 / 3.2 | 13.4 / 3.1 | 12.7 / 2.9 | 12.6 / 2.9 |
| eqtable | 23.7 / 19.0 | 24.9 / 19.8 | 24.7 / 19.7 | 22.6 / 18.1 | 19.1 / 15.6 | 6.9 / 5.2 |

**Which classes survive** [V]:
- Closures and cells survive at 0.0–0.4% in every workload, and flonums at ≤0.7%.
- Survivors are almost all pairs or vectors: nboyer pairs 48.5%, hashtable0 pairs 40.9%, dynamic pairs 57.6%, earley pairs 53.3%, mperm pairs 100%, gcold vectors 18.1%.
- gcbench's survivors are almost all records: 28.3% of records survive a 64 K interval, while its pairs, vectors, closures and cells survive at 0.0%.

### 5.2 Per-allocation trace work: today vs a copying nursery

Today's figures are the default adaptive policy, `max(65,536, 2 × live slots)` (`gc.rs:1001-1003`). The nursery figure s(N) is the objects copied per allocation, i.e. the survival rate [V numbers, I verdict].

| workload | marked per alloc today | slots swept per alloc today | s(N) count 64K / 256K / 1M | s(N) bytes 64K / 256K / 1M | trace work |
|---|---|---|---|---|---|
| nboyer | 0.554 | 1.39 | 0.434 / 0.372 / 0.302 | 0.373 / 0.321 / 0.262 | about equal |
| deriv | 0.009 | 1.07 | 0.001 / 0.000 / 0.000 | 0.001 / 0.000 / 0.000 | both ≈0; sweep dominates |
| destruc | 0.013 | 1.12 | 0.004 / 0.001 / 0.000 | 0.003 / 0.001 / 0.000 | both ≈0 |
| quicksort | 0.007 | 1.63 | 0.000 / 0.000 / 0.000 | 0.004 / 0.004 / 0.003 | both ≈0 |
| gcbench | 0.496 | 1.93 | 0.023 / 0.013 / 0.005 | 0.031 / 0.017 / 0.006 | nursery ≈20× less |
| gcold | 0.492 | 2.31 | 0.153 / 0.089 / 0.067 | 0.009 / 0.005 / 0.004 | nursery less (3–50×) |
| mperm | 0.583 | 1.08 | 0.578 / 0.587 / 0.583 | 0.403 / 0.409 / 0.407 | about equal |
| queue3 | 0.600 | 1.50 | 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 | **nursery ≈1.7× more** |
| fibfp | 0.007 | 1.36 | 0.000 / 0.000 / 0.000 | 0.000 / 0.000 / 0.000 | both ≈0 |
| mbrot | 0.008 | 1.43 | 0.000 / 0.000 / 0.000 | 0.001 / 0.001 / 0.001 | both ≈0 |
| nucleic | 0.199 | 2.10 | 0.008 / 0.003 / 0.001 | 0.009 / 0.003 / 0.001 | nursery ≈25× less |
| ctak | 0.007 | 1.38 | 0.000 / 0.000 / 0.000 | 0.000 / 0.000 / 0.000 | both ≈0 |
| fibc | 0.007 | 1.34 | 0.000 / 0.000 / 0.000 | 0.000 / 0.000 / 0.000 | both ≈0 |
| generator | 0.010 | 1.84 | 0.000 / 0.000 / 0.000 | 0.000 / 0.000 / 0.000 | both ≈0 |
| deeprec | 0.567 | 1.37 | 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 | **nursery ≈1.8× more** |
| libload | 0.011 | 3.90 | 0.008 / n/a / n/a | 0.008 | both ≈0 |
| hashtable0 | 0.490 | 3.68 | 0.133 / 0.128 / 0.111 | 0.113 / 0.109 / 0.097 | nursery ≈4× less |
| dynamic | 0.486 | 2.26 | 0.202 / 0.190 / 0.101 | 0.127 / 0.119 / 0.063 | nursery 2.4–5× less |
| earley | 0.565 | 1.63 | 0.139 / 0.134 / 0.127 | 0.032 / 0.031 / 0.029 | nursery ≈4× less |
| eqtable | 0.499 | 1.88 | 0.247 / 0.226 / 0.191 | 0.197 / 0.181 / 0.156 | nursery ≈2× less |

### 5.3 Survival with compiler-generated garbage removed [V numbers, I relevance]

Today the VM allocates a closure per internal `define` per call, and boxes mutated captured variables in cells. If a compiler or JIT stopped doing that, these classes would vanish from the denominator.

| workload | all classes 64K / 1M | w/o closures + cells | w/o closures + cells + flonums | closures + cells share of allocs |
|---|---|---|---|---|
| nboyer | 43.4 / 30.2 | 48.5 / 33.7 | 48.5 / 33.7 | 11% |
| gcbench | 2.3 / 0.5 | 5.0 / 1.0 | 4.7 / 0.7 | 54% |
| gcold | 15.3 / 6.7 | 18.1 / 7.9 | 18.1 / 7.9 | 15% |
| mperm | 57.8 / 58.3 | **100 / 100** | 100 / 100 | 42% |
| hashtable0 | 13.3 / 11.1 | **40.9 / 34.1** | 40.9 / 34.1 | 67% |
| dynamic | 20.2 / 10.1 | **56.2 / 29.5** | 56.2 / 29.5 | 64% |
| earley | 13.9 / 12.7 | **53.2 / 50.1** | 53.2 / 50.1 | 71% |
| eqtable | 24.7 / 19.1 | **53.5 / 41.5** | 53.5 / 41.5 | 54% |
| deriv, destruc, nucleic | ≤0.8 | ≤0.9 | ≤3.2 | 14–50% |
| quicksort, generator, ctak, fibc | 0.0 | 0.0–0.1 | 0.0 | 49–98% |

### 5.4 Ages (stress runs; cohorts born after collection #0)

Definitions:
- **2nd**: P(alive at its 2nd collection | alive at its 1st).
- **3rd**: P(alive at its 3rd collection | alive at its 2nd).
- **dead-old**: objects that died after surviving at least one collection, as a % of first-time survivors. This is the garbage a promote-at-first-survival scheme hands to the major collector.

| workload | 64K: 2nd / 3rd / dead-old | 256K | 1M |
|---|---|---|---|
| nboyer | 85.7 / 93.6 / 76.5 | 77.0 / 90.9 / 72.8 | 64.8 / 86.1 / 66.6 |
| deriv | 0.0 / – / 100 | 0.0 / – / 100 | 0.0 / – / 100 |
| destruc | 13.1 / 45.3 / 100 | 0.0 / – / 100 | 0.0 / – / 100 |
| quicksort | 7.7 / 75.8 / 99.6 | 11.2 / 48.4 / 97.9 | 0.0 / – / 100 |
| gcbench | 54.2 / 77.1 / 87.6 | 38.9 / 77.1 / 84.2 | 56.3 / 100 / 43.7 |
| gcold | 53.5 / 76.3 / 66.1 | 70.5 / 98.5 / 41.6 | 88.8 / 95.7 / 26.1 |
| mperm | 100 / 100 / 80.1 | 100 / 100 / 81.6 | 100 / 93.6 / 90.3 |
| queue3 | 100 / 100 / 89.6 | 100 / 100 / 91.8 | 98.4 / 3.3 / 99.9 |
| fibfp | 5.2 / 55.0 / 100 | 4.3 / 53.0 / 100 | 3.3 / 58.8 / 99.6 |
| mbrot | 18.1 / 100 / 100 | 46.9 / 100 / 100 | 54.0 / 0.0 / 95.8 |
| nucleic | 36.8 / 53.3 / 93.7 | 25.7 / 86.6 / 95.4 | 45.1 / 85.5 / 90.2 |
| ctak | 5.8 / 0.0 / 99.9 | 0.0 / – / 100 | n/a |
| fibc | 5.4 / 53.3 / 100 | 4.2 / 0.0 / 99.2 | n/a |
| generator | 17.1 / 15.4 / 98.8 | 0.0 / – / 100 | 0.0 / – / 100 |
| deeprec | 100 / 100 / 81.4 | 100 / 100 / 84.8 | 20.7 / 0.0 / 95.4 |
| hashtable0 | 97.3 / 97.0 / 91.3 | 89.0 / 86.9 / 91.8 | 49.7 / 19.1 / 95.9 |
| dynamic | 93.9 / 95.2 / 95.9 | 84.6 / 32.9 / 95.9 | 0.0 / – / 100 |
| earley | 96.6 / 99.1 / 8.0 | 98.1 / 99.4 / 5.1 | 98.3 / 99.8 / 2.5 |
| eqtable | 97.2 / 97.6 / 90.9 | 92.7 / 80.9 / 95.1 | 42.4 / 38.5 / 100 |

### 5.5 Reading the survival data [V numbers, I interpretation]

**There are two populations.**
- In the retaining workloads (nboyer, mperm, queue3, deeprec, hashtable0, dynamic, earley, eqtable), an object that survives one 64 K interval survives the next 85–100% of the time.
- In the churn workloads, survivors die almost at once, but there are only tens to hundreds of them per collection.

Consequences:
- **Promotion policy.** Promote-on-first-survival (sticky mark bits) wastes little in either population.
- **The old generation will need regular majors.** "dead-old" is 65–100% everywhere except earley, so promoted data mostly dies before exit.
- **Promotion volume is high where survival is high.** It is 37% of allocated bytes in nboyer, 40% in mperm and 100% while queue3 or deeprec are building their structures.
- **A bigger interval helps little.** Going from 64 K to 1 M cuts survival at most about 1.5× for the retaining workloads.

---

## 6. (c) Store mix and barrier filters

### 6.1 Per workload (default policy)

Heap stores are all in-heap channels. Env stores are off-heap environment slots.

| workload | heap stores | per 1M instr | env stores | env per 1M | imm % | young-target % actual / 64K / 1M | old→young % actual / 64K / 1M | top sites |
|---|---|---|---|---|---|---|---|---|
| nboyer | 8.91M | 22,123 | 413 | 1 | 89.6 | 0.5 / 0.5 / 8.1 | 9.97 / 9.97 / 8.67 | cell 100% |
| deriv | 1.23M | 9,728 | 405 | 3 | 0.4 | 100 / 100 / 100 | 0 / 0 / 0 | cell 98% |
| destruc | 21.25M | 70,124 | 407 | 1 | 83.9 | 88.3 / 88.3 / 99.3 | 0.79 / 0.79 / 0.05 | set_car 80%, cell 11%, set_cdr 10% |
| quicksort | 5.79M | 18,745 | 418 | 1 | 81.0 | 33.5 / 33.5 / 85.8 | 1.04 / 1.04 / 0.00 | VectorSet 82%, cell 17% |
| gcbench | 34.79M | 62,588 | 409 | 1 | 71.2 | 99.9 / 99.8 / 100 | 0.11 / 0.18 / 0.00 | VectorSet 76%, cell 19%, record 5% |
| gcold | 9.90M | 64,136 | 410 | 3 | 35.1 | 37.5 / 37.2 / 44.0 | 51.1 / 51.4 / 45.1 | cell 63%, VectorSet 35% |
| mperm | 5.33M | 21,297 | 415 | 2 | 0.1 | 63.5 / 32.7 / 61.6 | 36.5 / 67.3 / 38.4 | cell 99% |
| queue3 | 28K | 198 | 550 | 4 | 17.4 | 99.6 / 99.6 / 99.7 | 0.36 / 0.36 / 0.34 | set_car/cdr |
| fibfp | 25K | 214 | 405 | 4 | 18.3 | 100 / 100 / 100 | 0 | set_car/cdr (bootstrap) |
| mbrot | 257K | 1,897 | 409 | 3 | 45.8 | 56.7 / 56.7 / 63.6 | 0 | cell 44%, VectorSet 44% |
| nucleic | 131K | 688 | 924 | 5 | 13.3 | 100 / 99.6 / 100 | 0 | set_car/cdr 76%, cell 23% |
| ctak | 28K | 890 | 407 | 13 | 18.1 | 100 | 0 | set_car/cdr |
| fibc | 25K | 358 | 411 | 6 | 18.3 | 100 | 0 | set_car/cdr |
| generator | 10.37M | 21,519 | 532 | 1 | 98.6 | 99.2 / 99.2 / 99.9 | 0 | cell 99% |
| deeprec | 16K | 104 | 400 | 3 | 18.2 | 100 | 0 | set_car/cdr |
| libload | 4.70M | 249,316 | 5,253 | 279 | 18.0 | 100 / 99.8 / 100 | 0 | set_car/cdr 96% (parser) |
| hashtable0 | 4.92M | 20,526 | 760 | 3 | 41.4 | 64.1 / 61.1 / 75.9 | 24.0 / 26.5 / 13.7 | cell 57%, VectorSet 28%, record 12% |
| dynamic | 1.51M | 11,056 | 619K | 4,528 | 12.6 | 46.5 / 41.7 / 78.1 | 1.23 / 0.93 / 1.22 | set_car 72%, cell 15%, set_cdr 13% |
| earley | 1.76M | 10,344 | 413 | 2 | 2.8 | 100 | 0 | cell 84% |
| eqtable | 2.35M | 15,292 | 489 | 3 | 21.6 | 58.3 / 35.2 / 68.1 | 22.4 / 43.6 / 15.7 | VectorSet 49%, cell 28%, record 21% |

### 6.2 Funnel by site (sum over the 20 workloads) [V]

| site | stores | imm % | young target % (actual / 16K / 64K / 256K / 1M / 4M) | old→young % of stores (same views) |
|---|---|---|---|---|
| set_car | 20.89M | 81.6 | 85.5 / 79.0 / 85.1 / 93.4 / 97.8 / 99.5 | 0.14 / 0.08 / 0.12 / 0.11 / 0.09 / 0.02 |
| set_cdr | 5.09M | 39.6 | 95.0 / 91.6 / 94.9 / 98.7 / 99.7 / 99.9 | 3.09 / 4.48 / 3.09 / 0.79 / 0.19 / 0.05 |
| vector_set (primitive) | 1,732 | 0.0 | 100 | 0 |
| VM VectorSet | 37.37M | 85.2 | 84.7 / 81.8 / 83.0 / 87.8 / 94.8 / 98.9 | 4.83 / 6.95 / 6.55 / 4.91 / 2.79 / 0.48 |
| write_mutable_cell | 47.26M | 43.9 | 63.9 / 60.0 / 60.3 / 61.6 / 66.6 / 81.2 | 16.7 / 20.3 / 20.2 / 19.4 / 15.4 / 5.0 |
| record_set | 2.71M | 41.1 | 61.5 / 58.5 / 59.0 / 59.5 / 67.0 / 92.0 | 0.04 / 0.40 / 0.11 / 0.03 / 0.01 / 0.00 |
| env_define (globals) | 15K | 25.7 | n/a (off-heap) | 74.3 |
| env_set_slot_value | 618K | 90.3 | n/a | 9.65 |

**Across all heap stores** (113.3 M) [V]:

| View | young-target % | heap-valued with an old target % | old→young % |
|---|---|---|---|
| actual | 76.1 | 9.3 | 8.7 |
| 64K | 73.9 | 11.4 | 10.8 |
| 1M | 83.1 | 7.6 | 7.4 |

- The immediate filter removes 64.2%.
- Medians across workloads at 64K: imm 18.3%, young-target 99.4%, old-target heap-valued 0.25%, old→young 0.09%.
- The parameter, promise, ephemeron-break and closure-free-variable channels had **zero** stores in every workload.

### 6.3 Remembered-set pressure and the sources of edges [V]

Edges are old→young stores into heap targets.

| workload | old→young edges (64K view) | distinct targets per 64K interval, mean / max | edges (1M view) | distinct per 1M interval, mean / max | source |
|---|---|---|---|---|---|
| nboyer | 888K | 1.0 / 1 | 772K | 1.0 / 1 | one boxed variable |
| destruc | 168K | 11.6 / 13 | 10K | 11.6 / 12 | set_cdr |
| gcbench | 62K | 2.4 / 10 | 209 | 2.8 / 10 | VectorSet, record |
| gcold | 5.09M | 7.8 / 29 | 4.47M | 26.6 / 75 | `(set! aexport (make-vector 100 0))` on a captured `let*` var. Only 689 come from `vector-set!` into old trees |
| mperm | 3.58M | 1.9 / 3 | 2.04M | 2.4 / 3 | cells |
| hashtable0 | 1.30M | 1.1 / 2 | 672K | 1.2 / 2 | VM VectorSet: fresh bucket alist into the old bucket vector |
| eqtable | 1.03M | 0.9 / 2 | 369K | 1.2 / 2 | same |
| dynamic | 14K | 36.3 / 1,551 | 18K | 482 / 1,825 | set_car; plus 60K old→young global `set!`s into env slots |

### 6.4 Tree-walker contrast [V]

| workload | backend | allocs | pair % | procedure/closure % | flonum % | cell % | env stores |
|---|---|---|---|---|---|---|---|
| nboyer | VM | 14.66M | 89.2 | 10.6 | 0 | 0 | 413 |
| nboyer | TW | 14.66M | 89.2 | 10.6 | 0 | 0 | 525.5M |
| deriv | VM | 21.94M | 72.6 | 19.1 | 0 | 5.5 | 405 |
| deriv | TW | 20.74M | 76.8 | 20.3 | 0 | 0 | 159.3M |
| destruc | VM | 17.41M | 49.7 | 36.9 | 0 | 13.2 | 407 |
| destruc | TW | 15.11M | 57.3 | 42.6 | 0 | 0 | 448.1M |
| fibfp | VM / TW | 20.23M | 0.1 | 0 | 99.8 | 0 | 405 / 141.4M |

- On the TW, captured `set!` goes through `env_set_scoped`: 8.87 M in nboyer, identical to the VM's cell writes.
- The TW's per-call `Environment`s are off-heap `Rc`s that these counts do not see.

### 6.5 Barrier conclusions [I unless marked]

- **Test the value's tag first.** It removes 64% of heap stores in aggregate: cell writes in loops (nboyer 90%, generator 99%) and fixnum `vector-set!` (gcbench, quicksort) [V].
- **Then test the target's youth.** In the median workload, 99.4% of heap stores at 64 K target a young object [V]. Exceptions are the long-lived-cell and hash-bucket programs. With a contiguous nursery this is a single compare on the address.
- **Then test a logged bit.** Edges are so concentrated (≤36 distinct targets per 64 K interval on average) that a per-object or per-field logged bit, as in Whippet's field logging, makes the slow path take O(10) hits per minor collection.
  - A sequential store buffer *without* de-duplication would log up to 45 K entries per 64 K interval in gcold, 5 M over the run.
  - Unconditional card marking would re-dirty the same few cards millions of times, which is cheap but not free.
- **Fast-path frequency.** Barrier fast-path sites run at 0.1–70 per 1,000 VM instructions. Instruction cost of the fast path dominates; the remembered set does not. No read barrier is indicated.
- **Environments.** VM globals take 1–13 stores per 1 M instructions, except dynamic at 4,528/1 M with 9.65% old→young. A barrier hook on `define`/`set!`, or a small dirty-environment list, is enough.
- **TW environments as heap objects.** They would be the dominant barrier sites, with environment stores 7–36× the heap allocation count. These would be mostly initializing stores into young environments.

---

## 7. (d) Other rates

### 7.1 Hashing and continuations

Only the workloads with non-zero values are listed [V].

| workload | identity-hash: imm / by-index / by-Rc | equal-hash calls / index fallbacks | call/cc captures | frames / regs copied per capture (mean) | max regs | restores (mean regs) |
|---|---|---|---|---|---|---|
| ctak | 0 / 0 / 0 | 0 / 0 | 1.91M | 15.3 / 211 | 248 | 1.43M (214) |
| fibc | 0 / 0 / 0 | 0 / 0 | 1.21M | 18.6 / 323 | 456 | 1.21M (323) |
| hashtable0 | 0 / 0 / 0 | 1.40M / 0 | 6 | 2.5 / 28 | 32 | 0 |
| eqtable | 0 / **1.66M** / 0 | 0 / 0 | 0 | – | – | – |
| quicksort, generator | 0 | 0 | 1 | 2 / 24 | 24 | ≤1 |

No workload made a delimited capture.

**Identity hashing** [V].
- `make-eq-comparator` is `(make-comparator #t eq? #f default-hash)` (`lib/srfi/128/128.body1.scm:188-189`). `default-hash` sends pairs and vectors to `equal-hash` (`128.body2.scm:116-120`). So SRFI 125/128 eq-tables never call `identity-hash` (`equality.rs:66-79`).
- SRFI 69 `(make-hash-table eq?)` picks `hash-by-identity` → `identity-hash` (`srfi-69-impl.scm:118-120,147`). That path makes one call per insert or lookup, and every call is by heap index, the location-dependent kind.
- The `equal-hash` heap-index fallback (`heap/mod.rs:2668-2670`) fired 0 times in every workload.

**Continuations** [V]. `capture_full` (`execution_state.rs:239-251`) copies the entire register file each time:
- ctak: 1.91 M × 211 registers ≈ 3.2 GB of copying;
- restores in ctak: another 1.43 M × 214 registers;
- the `(scheme generator)` workload captured only once.

### 7.2 Per collection (default policy)

| workload | GCs | mean interval | live pairs mean / max | live vectors mean / max | live objects mean / max | live hyp. MB max | frames mean / max | registers mean / max | arena slots at last GC (pair / vector / string / object) |
|---|---|---|---|---|---|---|---|---|---|
| nboyer | 20 | 733K | 405K / 1.42M | 88 / 88 | 594 / 595 | 21.7 | 62 / 75 | 852 / 1,018 | 3.87M / 89 / 44 / 250K |
| deriv | 334 | 66K | 145 / 173 | 0 / 1 | 417 / 420 | 0.0 | 7 / 10 | 150 / 195 | 48K / 898 / 42 / 21K |
| destruc | 265 | 66K | 373 / 654 | 0 | 413 / 414 | 0.0 | 5 / 6 | 69 / 77 | 40K / 2 / 45 / 33K |
| quicksort | 268 | 66K | 0 | 3 / 4 | 447 / 452 | 0.2 | 7 / 11 | 102 / 176 | 41K / 5 / 43 / 66K |
| gcbench | 228 | 193K | 19 / 20 | 1 / 2 | 96K / 130K | 4.9 | 14 / 22 | 234 / 344 | 80K / 20K / 56 / 293K |
| gcold | 27 | 285K | 0 | 139K / 396K | 466 / 483 | 12.1 | 7 / 18 | 163 / 332 | 184K / 984K / 71 / 322K |
| mperm | 8 | 1.47M | 859K / 2.67M | 1 | 436 / 443 | 40.7 | 12 / 13 | 198 / 216 | 4.02M / 5 / 52 / 1.30M |
| queue3 | 9 | 2.23M | 0 | 1.34M / 2.20M | 460 | 50.3 | 4 / 4 | 60 / 60 | 28K / 6.60M / 44 / 17K |
| fibfp | 308 | 66K | 0 | 0 | 445 / 452 | 0.0 | 22 / 27 | 288 / 349 | 23K / 1 / 43 / 66K |
| mbrot | 532 | 66K | 0 | 76 | 428 / 431 | 0.1 | 5 / 6 | 69 / 74 | 28K / 153 / 43 / 66K |
| nucleic | 392 | 66K | 380 / 563 | 2,194 / 2,515 | 10K / 12K | 0.3 | 60 / 100 | 705 / 1,161 | 59K / 4,516 / 34 / 76K |
| ctak | 58 | 67K | 0 | 0 | 436 / 442 | 0.0 | 16 / 19 | 219 / 256 | 26K / 3 / 47 / 66K |
| fibc | 37 | 67K | 0 | 0 | 427 / 432 | 0.0 | 19 / 24 | 328 / 410 | 24K / 2 / 43 / 66K |
| generator | 327 | 66K | 78 / 81 | 0 | 503 / 504 | 0.0 | 5 / 6 | 66 / 70 | 56K / 1 / 51 / 66K |
| deeprec | 7 | 1.43M | 812K / 1.71M | 0 | 395 | 26.1 | **759K / 960K** | **9.1M / 11.5M** | 5.14M / 0 / 32 / 9,873 |
| libload | 4 | 1.93M | 16K / 17K | 160 | 5,059 / 5,075 | 0.5 | 2 / 2 | 17 / 22 | 4.62M / 983 / 746 / 2.89M |
| hashtable0 | 119 | 214K | 104K / 307K | 3 / 4 | 635 / 643 | 6.2 | 9 / 13 | 163 / 208 | 505K / 16 / 71 / 418K |
| dynamic | 82 | 193K | 92K / 143K | 0 | 1,212 / 1,432 | 2.2 | 358 / **2,656** | 4,189 / **31,878** | 263K / 1 / 189 / 215K |
| earley | 23 | 497K | 280K / 1.29M | 88 / 92 | 478 / 528 | 19.7 | 13 / 22 | 277 / 506 | 1.55M / 181 / 35 / 1.58M |
| eqtable | 23 | 579K | 288K / 461K | 1 / 2 | 449 / 454 | 8.5 | 6 / 9 | 75 / 109 | 836K / 12 / 49 / 526K |

**Readings** [V]:
- Stacks at a safe point are small, except under deep recursion. In deeprec, scanning 11.5 M registers is most of a 12–15 ms mark. [I] A nursery collection needs a stack watermark that scans only frames pushed since the last collection; without one, every minor costs O(stack).
- Live heaps peak at 50 MB (queue3), 41 MB (mperm), 26 MB (deeprec) and 22 MB (nboyer) in the hypothetical layout.
- Arena high-water marks are far larger than the live heap. queue3 keeps 6.6 M vector slots. libload keeps 4.6 M pair and 2.9 M object slots for 17 K and 5 K live.

---

## 8. (f) Process facts

### 8.1 Heaps per test process [V]

**Method.** A `HeapGuard` field in `Heap::with_capacity` counts constructions and drops. I copied `scheme_tests test-lib compat spec examples docs scripts` in, because the tests read them, and ran `SKIP_CHIBI_TESTS=1 cargo test -p patina-tests --no-fail-fast`: 48 test binaries, 550 passed, 0 failed, 5 ignored.

**Results.**
- **Max live in one process: 318** (`hygiene_matrix`). Next: 315 (`scheme_suite`), 245 (`spliced_imports`), 196, 190, 189.
- 4,200 heaps across 276 processes; `hygiene_metamorphic` re-executes itself 229 times.
- **Only 4 of 276 processes ever drop a heap.** Everywhere else `live_at_exit == total`.
- [I] The likely reason is the `Rc` cycle heap → `VmClosure.globals: Rc<Environment>` → `Environment.heap: SharedHeap` (heap-repr §7). The criterion `startup/bootstrap_and_drop` lane probably leaks a heap per iteration.

### 8.2 Address-space reservation on this machine [V]

The probe is `PRD/study/gc/probes/workload-demographics` (`src/main.rs`), using raw `mmap` FFI.

| test | result |
|---|---|
| page size | 16,384 |
| 64 × 16 GiB, `PROT_NONE`, `MAP_PRIVATE\|MAP_ANON` | ok in 0.8 ms; RSS unchanged at 2 MB; VSZ +1 TiB |
| commit and touch 4 × 64 MiB (`mprotect` RW) | RSS 264 MB, 11 ms |
| `MADV_FREE` + `PROT_NONE` on those | RSS back to 2 MB immediately |
| touch the last page of a 16 GiB region | ok |
| keep reserving 16 GiB until failure (cap 4,096) | **all 4,096 (64 TiB) ok**, 56 ms |
| 64 × 16 GiB with `MAP_NORESERVE` | ok, no observable difference |
| single 1 TiB / 8 TiB / 64 TiB region | ok (0.6 / 5.5 / 50 ms) |
| 64 × 16 GiB RW, lazily committed | ok, RSS unchanged |
| returned regions 4 GiB-aligned | **1 of 64** (the first, at `0x7000000000`) |

**What this means for one reservation per heap** [I]. It is feasible in address space even at 318 heaps × 16 GiB ≈ 5 TiB, under three conditions:
1. Commit lazily and decommit eagerly. 318 heaps each committing even a 4 MB nursery would cost 1.3 GB.
2. Make heaps actually die. Today their reservations would accumulate for the life of the process.
3. Over-reserve and trim whenever alignment matters, for a pointer-compression cage or an aligned nursery.

---

## 9. Conclusions for the redesign

### 9.1 A nursery is likely to pay, but not universally [I, from §5]

**What it buys.** A bump-allocated young space removes today's dominant per-allocation costs: a free-list pop, 1.1–3.9 arena slots swept per allocation, and 72 B object slots. These apply to every workload, including the many whose live heap is tiny and whose tracing is already near zero. On top of that, it cuts trace work:
- about 20× on gcbench and nucleic;
- 2–5× on hashtable0, dynamic, earley and eqtable;
- 3–50× on gcold.

**Where it buys nothing or loses.**
- It is neutral on nboyer and mperm.
- It loses on queue3 and deeprec, which have 100% survival at every interval up to 1 M while they build their structures.
- §5.3 shows the tracing advantage shrinks sharply once a compiler stops allocating per-call closures and cells. Data survival is 41–56% on the hash and parse workloads.

**What the design needs.**
- Survival-triggered adaptation: grow the nursery, pretenure by allocation site (natural in a JIT), or bypass the nursery when survival exceeds about 30–50%.
- A stack watermark.
- Continuation side tables that are not reprocessed in full at every collection.

**Sizing and promotion.**
- About 64 K–256 K objects (≈1.5–6 MB in the hypothetical layout), scaled with allocation rate. Larger nurseries buy little.
- Promote-on-first-survival, with sticky mark bits, is adequate.
- Keep the generational mode switchable and measure it with interleaved A/B runs. Wingo's 2025 nboyer result is reproduced in spirit here: nboyer's survival is 37–50% at small nursery sizes.

### 9.2 Barrier: a post-write generational barrier with three filters [I, from §6]

- **The filters, in order:** (1) the value is a heap reference; (2) the target is old; (3) the field or object is not yet logged. Only then take the slow path to a de-duplicated remembered set.
- **No SATB or read barrier is needed for generational use.**
- **Expected cost:** the slow path runs at most tens of times per minor GC. The fast path runs 0.1–70 times per 1,000 VM instructions.
- **Hot sites:** cell writes (`WriteCell`) and `VectorSet` matter most; `set-car!` and `set-cdr!` come next.
- **Environment `define` and `set!`:** these need a hook, or must become heap stores.

### 9.3 Granule and line size [I, from §4.3]

| Choice | Value | Basis |
|---|---|---|
| Granule | 16 B | 56% of objects are exactly 16 B; pairs and flonums carry no header |
| Line | 128 B (256 B also fine) | 99.2% of objects are ≤128 B; mean 27 B |
| Medium objects (128 B–8 KiB) | handled within lines or blocks | rare by count (0.8% excluding gcold; 2.3% with it), but can dominate bytes (gcold) |
| Large-object threshold | 8 KiB | 148 of 348 M objects exceed it |

### 9.4 Flonum boxing

- **Measured** [V]: 19.8% of all allocations, and 79–99.8% in float code. Flonums survive at ≤0.7%, and each costs 72 B today.
- **Options** [I]: immediate floats (self-tagging) or a 16 B box in a bump nursery. Either removes most of the cost, and immediates would also remove most of the nursery traffic in float code.

### 9.5 Heap sizing

- **Problem** [V]: the trigger counts objects. gcold's 816 B vectors let RSS reach 628 MB with 12 MB live.
- **Fix** [I]:
  - Trigger on bytes, including malloc'd payloads for as long as they exist.
  - Size the old space from live bytes after a major, with about 2× headroom. Today's 2 × live (counted in slots) marks about 0.5 objects per allocation on large-heap workloads.
  - Decommit memory after spikes. libload's arenas keep their 7.5 M-slot high-water mark for the life of the process.

---

## 10. Reproduction

Everything is in `PRD/study/gc/probes/workload-demographics/instrumented/REPRODUCE.sh`. The core commands, with `SCRATCH` the probe directory `PRD/study/gc/probes/workload-demographics` (copied outside the repository first, as that script says) and `REPO=~/Project/patina`:

```bash
# base binary (repo sources, the probe directory's target dir)
cd $REPO && CARGO_TARGET_DIR=$SCRATCH/target cargo build --release -p patina-repl --bin patina
cp $SCRATCH/target/release/patina $SCRATCH/instrumented/bin/patina-base
# instrumented copy (cp -R + demographics.patch), its own target dir
cd $SCRATCH/instrumented/demographics && CARGO_TARGET_DIR=$PWD/target cargo build --release -p patina-repl --bin patina
cp target/release/patina ../bin/patina-demog
# one workload: run1.sh BIN NAME [--tree-walker]; env PATINA_DEMOG=full|pause, PATINA_DEMOG_OUT, PATINA_DEMOG_GCLOG, PATINA_GC_STRESS
cd $SCRATCH/instrumented/workloads
./drive_time.py            # GC on vs off x3 interleaved (/usr/bin/time -l) + pause-mode run -> out/timing.json, out/*/pause.*
./drive_full.py 6          # default + PATINA_GC_STRESS=16384,65536,262144,1048576,4194304 -> out/*/{default,s16K..s4M}.*
python3 analyze.py > out/tables.md; python3 agg.py; python3 work.py; python3 datasurv.py; python3 today.py
# heaps per test process
cd $SCRATCH/instrumented/demographics && SKIP_CHIBI_TESTS=1 PATINA_DEMOG_HEAPS=../workloads/out/heaps.log \
  CARGO_TARGET_DIR=$PWD/target cargo test -p patina-tests --no-fail-fast
# mmap probe
cd $SCRATCH && CARGO_TARGET_DIR=$SCRATCH/target cargo build --release && $SCRATCH/target/release/mmap-probe
# inventory
python3 $SCRATCH/instrumented/workloads/repo_bench/run_repo_workloads.py
python3 $SCRATCH/instrumented/workloads/larceny_scan/scan.py
```

---

## 11. Open questions

1. **How much allocation survives the planned compiler and JIT work?** Closures and cells are 40% of allocations today. This decides whether "half the workloads survive at ≤1.5%" still holds, or whether the §5.3 numbers (41–56% data survival) become the norm.
2. **How should queue-shaped and deep-recursion programs be handled?** The options are dynamic nursery sizing, allocation-site pretenuring, or a non-generational mode. Real user programs are needed; the suite offers only synthetic cases.
3. **Do strings matter?** String-, I/O- and reader-heavy workloads (`read0`, `wc`, `cat`, `string`, `slatex`) were not measured, and strings were almost absent here. Is the UTF-32 string layout GC-relevant for real users?
4. **Can continuations stop copying whole register files?** Candidates are segmented or one-shot stacks. ctak and fibc spend their pause time on continuation tables, not on the heap.
5. **Why do heaps leak in tests?** Should the redesign require heap teardown, given one reservation per heap?
