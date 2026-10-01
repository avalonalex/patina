# Probes retained from the GC study

These are the small programs and scripts written during the GC study (2026-09-30 to 2026-10-01, `main` at
`28a94f8`) that its reports and the issues filed from it rely on. They are a record, not a test suite:

- nothing runs them;
- they are not kept compiling against later revisions;
- the numbers they produced are in the reports, not here.

Stage 0 vendors the maintained harness into the repository: the `gc` mode of `scripts/benchmarks.py`, the
`gc-census` feature and `scripts/gc_probes/`. Use that harness for new measurements.

Every file here was written for the study. Each one was read before it was kept, and anything copied or adapted from
third-party code was left out (see the table below). The files fall under the repository's license. Two items are
worth naming:

- `followup/i1/lib/t/g.sld` renames the reference `guard` from R7RS §7.3, as `lib/scheme/base/exceptions.scm`
  does.
- `followup/prior-art-chez/bench.ss` writes `fib` and `tak` in their standard one-line forms.

## Changes made when the files were retained

The files were copied unchanged except for path rewrites, so that no local absolute path remains:

- **Rust crates.** Path dependencies point at `crates/` by relative path, for example
  `../../../../../crates/patina-core`.
- **Two embedding probes.** `primitives-embedding/src/main.rs` and `src/bin/leak.rs` load `lib` relative to the
  working directory, so run them from the repository root.
- **`[workspace]` tables.** Each crate already had an empty `[workspace]` table except `followup/par-measure` and
  `followup/review-pubstore`, which now have one. Without it, Cargo would take these crates for members of the
  repository's workspace.
- **The repository.** It is `~/Project/patina`: `os.path.expanduser('~/Project/patina…')` in Python and
  `~/Project/patina` in shell. A release binary is expected at `~/Project/patina/target/release/patina` wherever a
  script used one built from `28a94f8` in a scratch target directory.
- **The study's working directory.** Paths into it are now relative to the script, following the mapping below.
  - `workload-demographics/instrumented/REPRODUCE.sh` and `workloads/*.py` take `SCRATCH` to be
    `probes/workload-demographics`.
  - `followup/perturb-wl/` looks for the workloads under `workload-demographics/instrumented/workloads/`.
- **`demographics.patch`.** The old side of each file names `crates/…` relative to the repository. The new side is
  unchanged (`demographics/crates/…`), so `REPRODUCE.sh` applies it as before.

Lock files and `rust-toolchain.toml` copies were dropped, because the repository's toolchain file applies. Build with
`CARGO_TARGET_DIR` outside the repository, since only the root `/target` is ignored. Several scripts write outputs
beside themselves, so copy a directory out of the repository before running it. Scripts that drive the oracles
expect `chibi-scheme`, `gosh` (Gauche) and `chez` (Chez Scheme) on `PATH`.

## Layout

Directories are named by the study key of the report that uses them.

| Directory | Report | Contents |
|---|---|---|
| `gc-impl/` | `understand/gc-impl.md` | Probe crate (sizes, growth, closures, RSS, embedding); `scm/` holds the pause, trigger, churn and library-body programs |
| `heap-repr/` | `understand/heap-repr.md` | Type-size and census crate; fixnum, flonum, cons and `gc-stats` programs |
| `vm-runtime/` | `understand/vm-runtime.md` | Type-size crate; call/cc, closure, recursion and loop programs |
| `offheap/` | `understand/offheap.md` | Off-heap probe crate; `tools/scan.py` and `scan2.py`, the handle-hazard scanners |
| `tree-walker/` | `understand/tree-walker.md` | Counting-allocator probe crate |
| `primitives-embedding/` | `understand/primitives-embedding.md` | Embedding use-after-free and leak probes; `tools/`, the heap-call-site counters cited as "in the scratchpad" |
| `jit-readiness/` | `understand/jit-readiness.md` | Type-size crate; deep-recursion probe; two opcode-histogram programs |
| `immix-mmtk/`, `java-hotspot/`, `ocaml-gambit/`, `racket-larceny/`, `rust-gcs/`, `whippet-misc/` | `research/*.md` | One type-size crate each |
| `continuation-representation/` | `gaps/continuation-representation.md` | Toy prototype of representations T, A and C′ (`src/main.rs`); capture and reinstatement probes |
| `barrier-remset/` | `gaps/barrier-remset-resolution.md` | Store-mix workloads (`work/*.scm`) |
| `finalization-weak-semantics/` | `gaps/finalization-weak-semantics.md` | Oracle programs (`progs/`), drivers, embedder-teardown crate |
| `global-binding-cells/` | `gaps/global-binding-cells.md` | Binding census crate; rebinding probes for Patina (`.scm`) and Chez (`.ss`) |
| `lib-load-mem/` | `gaps/library-load-memory.md` | Sampling heap profiler and phase driver (`src/`); symbolize and classify scripts; import-set programs |
| `workload-demographics/` | `gaps/workload-demographics.md` | Address-space reservation probe (`Cargo.toml`, `src/main.rs`); `instrumented/` holds the census patch, `REPRODUCE.sh`, the drivers and analysis scripts, and four extra workloads |
| `design-verify/` | `design/DESIGN.md`, `design/ISSUES_DRAFT.md` | Repros of the group-A defects (`run/`, `repro/`), every one except A6, which is documentation drift; the re-run scripts used before filing |
| `design-review/` | `design/ISSUES_DRAFT.md`, DESIGN.md's `scripts/gc_probes/` | C probes from the performance and JIT reviews: decommit, `madvise`, address-space cycling, `MAP_JIT`, address-space placement |
| `followup/steady/` | `followup/steady/audit.md`, `SECTION.md` | `run.py` harness and batch scripts; steady-state probes for Patina (`probes/`) and Chez (`chez/`); `interp-churn` crate |
| `followup/i1/` … `i6/`, `followup/n1/` | issues #611–#617 | Repro programs and generators for I1–I6 and N1 |
| `followup/p4/` | issue #618 | Shared-current-ports probe crate; chibi comparison in C |
| `followup/par-measure/`, `agg.py`, `atomtax/`, `contend/`, `pubfence/`, `sendcheck/`, `review-pubstore/`, `perturb_patch.py`, `perturb-wl/`, `chez/asm.ss`, `prior-art-chez/`, `scm_globals.py`, `nonsend_types.py`, `rev-answer/`, `review-par/` | `followup/parallelism/*.md` | Microbenchmarks; `Send` check; perturbation patch and its timing driver; Chez probes; library and type scans; stack-depth generator (the repro for N1) |
| `followup/probes/`, `followup/review-ss/`, `followup/rebind/` | `followup/steady/prior-art.md`, `SECTION.md`, issue #603 | Live-set churn and depth probes; review probes for the steady-state section; one Chez rebinding probe |

## Where each working path went

The reports first cited paths in the study's working directory; their citations now name the retained locations
below, or say that a file was not retained. This table records the mapping. In the working directory, a path written
`PRD/study/gc/scratch-crates/X`, `$SCRATCH/scratch-crates/X` or `scratch-crates/X` was the same place, and so was
`followup/scratch/X`, written `../scratch/X` or `scratch/X` from inside `followup/`. General rules:

- `scratch-crates/<key>/` → `<key>/`;
- `instrumented/` → `workload-demographics/instrumented/`;
- `design/verify/` → `design-verify/`;
- `followup/scratch/<x>` → `followup/<x>`.

Within a retained directory, files not listed below were kept. "Generated" means a script here produces the file.

| Working path | Retained at | Not retained, and why |
|---|---|---|
| `scratch-crates/gc-impl/` (`src/main.rs`, `src/bin/*.rs`) | `gc-impl/` | |
| `scratch-crates/*.scm`, `scratch-crates/lib/churnlib.sld` | `gc-impl/scm/` | `ctak.scm`, `deriv.scm`, `nb2.scm`: copies of the repository's Gabriel-derived `crates/patina-tests/bench_programs/{ctak,deriv,nboyer}.scm` with a `gc-stats` line added |
| `scratch-crates/heap-repr/` | `heap-repr/` | |
| `scratch-crates/vm-runtime/` | `vm-runtime/` | `closure.sample.txt` (profile) |
| `scratch-crates/offheap/`, `offheap-tools/scan.py`, `scan2.py` | `offheap/`, `offheap/tools/` | `offheap-tools/*.out` (outputs) |
| `scratch-crates/tree-walker/` | `tree-walker/` | `fib.out`, `fib.sample` (output, profile) |
| `scratch-crates/primitives-embedding/`; the scratchpad scripts `count_heap.py`, `fn_classify.py`, `tv_structs.py`, `handlers.py` | `primitives-embedding/`, `primitives-embedding/tools/` (with `count_heap_all.py` and the input `heap_methods.txt`) | |
| `scratch-crates/jit-readiness/` | `jit-readiness/` | `hist/*.txt` (opcode histograms); `hist/{ctak,deriv,fib,nboyer,nqueens,primes,sum,tak}.scm` (copies of `crates/patina-tests/bench_programs/`, which come from ecraven/r7rs-benchmarks: Gabriel and other third-party benchmarks) |
| `scratch-crates/{immix-mmtk,java-hotspot,ocaml-gambit,racket-larceny,rust-gcs,whippet-misc}/` | same names | `immix-mmtk/*.txt`, `tree.json`, `pdf2txt*` (text extracted from third-party papers, an API listing, a PDF tool) |
| `scratch-crates/js-engines/` | — | `src/` holds V8, SpiderMonkey, JavaScriptCore, Nova and Boa source files; `thesis.txt` is a third-party thesis |
| `scratch-crates/chez/seginfo.c` | — | re-declares Chez's `seginfo` struct from Chez's `c/types.h` to print its size (adapted third-party code) |
| `scratch-crates/continuation-representation/` | `continuation-representation/` | `results.txt` (output, quoted in the report); `probes/tmp.err`; `probes/ctakdeep.scm` (wraps a retyped Gabriel `ctak`) |
| `scratch-crates/barrier-remset/work/` | `barrier-remset/work/` | `work/*.txt` (outputs). `barrier-remset/patina/`, a patched copy of the repository that carried the store-mix counters, is replaced by the `gc-census` feature |
| `scratch-crates/finalization-weak-semantics/` | `finalization-weak-semantics/` | `run-{chibi,gauche,patina}/`: per-implementation copies of `progs/` and their outputs. `run.sh` expects them: copy `progs/` into each, renaming `.tmpl` to `.scm` and replacing `GCLIB` with `(patina debug)`, `(chibi ast)` or `(only (gauche base) gc)`. `run-patina/eph-chain.scm` and `data.txt` were kept, moved into `progs/`. Also dropped: `suites/` (scratch copies of the chibi and Larceny suites, third-party); `larceny-logs/`, `*.log`, `larceny-*.txt`, `teardown/out-*` (outputs); `bin/patina` (binary) |
| `scratch-crates/global-binding-cells/` | `global-binding-cells/` | |
| `scratch-crates/lib-load-mem/` | `lib-load-mem/` | `out/` (heap profiles of up to 27 MB, symbolized stacks, census output); `progs/rb-inline.scm` (Marc Nieper-Wißkirchen's rbtree library inlined); `progs/rb-forms.txt`, `s146-forms.txt` (forms extracted from third-party library sources) |
| `scratch-crates/workload-demographics/` | `workload-demographics/` | |
| `scratch-crates/target/` | — | build output |
| `instrumented/REPRODUCE.sh`, `instrumented/demographics.patch` | `workload-demographics/instrumented/` | |
| `instrumented/workloads/*.py`, `run1.sh`, `repo_bench/run_repo_workloads.py`, `larceny_scan/scan.py` | `workload-demographics/instrumented/workloads/` | `repo_bench/*.scm` (generated from the repository's `workloads.json`); `larceny_scan/result.txt` (output) |
| `instrumented/workloads/extra/` | `…/workloads/extra/` (`deeprec`, `empty`, `eqtable`, `libload`) | `gcold.scm`, `queue3.scm`: adapted Larceny GC benchmarks, LGPL. The drivers still list them |
| `instrumented/workloads/src/`, `inputs/`, `larceny_scan/inputs/` | — | Larceny R7RS benchmark sources and inputs, LGPL (AGENTS.md). Step 3 of `REPRODUCE.sh` assembles them from a Larceny checkout |
| `instrumented/workloads/out/`, `instrumented/bin/`, `instrumented/demographics/` | — | raw outputs (the tables are in the report); binaries; the patched copy of the repository that `REPRODUCE.sh` rebuilds |
| `design/verify/run/` | `design-verify/run/` | outputs (`out.txt`, `time.txt`, `td*.txt`, `eq*.txt`). `data.txt` is an input and was kept |
| `design/verify/repro/` | `design-verify/repro/` | |
| `design/verify/embed/` | `primitives-embedding/` | identical to `scratch-crates/primitives-embedding/`, so kept once |
| `followup/scratch/run/` | `design-verify/run/` | the same probes as `design/verify/run/`; its extra `emfile5k.scm`, `emfile5k-out.scm`, `eq3.scm` and `lib2.scm` were added there. Outputs dropped |
| `followup/scratch/{rerun.sh,rerun2.sh,rss1.sh,emfile128.sh,embed.sh}` | `design-verify/` | |
| `scratchpad/review-perf/{madv,cycle,decommit}.c`, `scratchpad/review-jit/mapjit.c`, `scratchpad/judge-probe/mm.c` | `design-review/` | `review-perf/src/`, `fibfx.body` (Larceny copies); `review-jit/*.rs`, `*.isle` (Cranelift source files); binaries; `fibfp.sample.txt` |
| `followup/scratch/steady/` | `followup/steady/` | `results/` (JSONL results, generated `stats-*.scm`, profiles) |
| `followup/scratch/i1/` | `followup/i1/` | generated programs of up to 10 MB (`m_*.scm`, `g1000.scm`, `g3k.scm`, `g25k.scm`, `g100k.scm`, `gs.scm`, `t.scm`, `cz*.ss`; `meas.py` and `gen.py` write them); outputs (`1`, `err.txt`) |
| `followup/scratch/i2/` | `followup/i2/` | `s.txt` (output) |
| `followup/scratch/i3/` | `followup/i3/` | `f10000.scm`, `f20000.scm`, `f40000.scm` (`(def-counter counter)` repeated N times); `*.sample.txt`; `table.txt`, `final.txt` |
| `followup/scratch/i4/` | `followup/i4/` | about 2,100 generated programs (`bare-*`, `import-*`, `call-*`, `f-*`, `g-*`, `o-*`, `c-*`, `mac-*`, `macnouse-*`, `nogc-*`, `x-only-*`), written by `gen*.py`, `final.py`, `genchez.py` or variants of them; outputs |
| `followup/scratch/i5/`, `i6/` | `followup/i5/`, `followup/i6/` | outputs (`out*`, `err.txt`, `fifo.*`, `prof.txt`) |
| `followup/scratch/n1/` | `followup/n1/` | `let_*.scm`, `shapes/` (generated by `gen2.py`); `res_*`, `bt_vm.txt`, `oracles.txt`, `out.txt` (outputs) |
| `followup/scratch/p4/` | `followup/p4/` | `chibi2` (binary), `err.txt` |
| `followup/scratch/rebind/` | `followup/rebind/c8.ss` | the other files are identical to `global-binding-cells/probes/` |
| `followup/scratch/probes/` | `followup/probes/` | `*.out`, `*.err`, `*.txt` (outputs) |
| `followup/scratch/review-par/` | `followup/review-par/` | `deepcode.scm`, `deepread.scm`, `dc_*.scm` (generated nesting programs; `gen.py N` writes the `let` shape) |
| `followup/scratch/review-ss/` | `followup/review-ss/` | `trace.txt`, `sample1.jsonl` (outputs) |
| `followup/scratch/par-measure/` and `agg.py` | `followup/par-measure/`, `followup/agg.py` | |
| `followup/scratch/{atomtax,contend,pubfence,sendcheck,review-pubstore}/` | `followup/<same>/` | `review-pubstore/results*.txt` (outputs) |
| `followup/scratch/{scm_globals.py,nonsend_types.py,perturb_patch.py}` | `followup/` | `nonsend_types.out` (output) |
| `followup/scratch/order_section.py` | — | a fragment already merged into `perturb_patch.py` |
| `followup/scratch/patina-perturb/`, `target*/`, `target-pb-*/` | — | copy of the repository (`perturb_patch.py` recreates it); build outputs |
| `followup/scratch/perturb-wl/` | `followup/perturb-wl/` | `results*.json`, `results_5reps.tsv`, `instrs.json`, `demog_*.txt`, `drive.log`, `perturb_table.md` (outputs). The Larceny-derived workloads it also ran are not retained (see `instrumented/` above) |
| `followup/scratch/chez/` | `followup/chez/asm.ss` | the Chez Scheme source tree and its threaded and non-threaded builds (third-party); `chez/bench/bench.ss` and `drive.py`, because `bench.ss` retypes the Gabriel `deriv` and `ctak` benchmarks; build logs and results |
| `followup/scratch/prior-art-chez/` | `followup/prior-art-chez/` | `results.tsv` (output) |
| `followup/scratch/rev-answer/` | `followup/rev-answer/` | `nest1000.scm`, `nest10000.scm` (identical to `followup/review-par/gen.py 1000` and `10000`) |
| `followup/scratch/{src,refs,pdf,prior-art-src}/` | — | fetched third-party sources, papers and their text, OCaml changelogs |
| `followup/scratch/results/`, `grep.txt`, `dedup*.txt`, `rss.txt`, `build-*.log` | — | outputs |
| `followup/scratch/{grep.sh,manifest.py}` | — | the duplicate searches run before filing issues. The filed issues record their searches |
| `followup/scratch/{t.scm,i6-keys.scm}` | — | one-line scratch checks, cited nowhere |
| `followup/scratch/llimits-5.4.6.h` | — | Lua header, third-party |
| `src-cache/`, `whippet-src/`, `go-src/`, `rust-src/`, `lua/`, `luajit/`, `dotnet/`, `lean/`, `pdf/`, `perceus.pdf` | — | third-party source caches and papers. The reports name the upstream revisions |
