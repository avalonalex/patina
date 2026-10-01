# GC redesign study

The research record behind [`PRD/GC_PRD.md`](../../GC_PRD.md).

- **Dates:** research on 2026-09-30; design, reviews and follow-up studies on 2026-10-01.
- **Revision:** `main` at `28a94f8`. The study read and measured it and did not modify it. The contract study
  (`followup/contract/`) also read `PRD/GC_PRD.md` at `f82e8e8` on branch `gc-prd`, a commit that adds only `PRD/`
  to that revision.
- **Machine:** every measurement was taken on one machine: Apple M4 Pro (8 performance and 4 efficiency cores),
  24 GiB, macOS 27.2 arm64 with 16 KiB pages, Rust 1.97.1 release builds. The oracles were chibi-scheme 0.12,
  Gauche 0.9.15 and Chez Scheme 10.
- **Status:** a dated record, not maintained. `PRD/GC_PRD.md` combines the design and the plan, and it supersedes
  [`design/DESIGN.md`](design/DESIGN.md) wherever they differ. DESIGN.md's own table, "Where each part goes",
  predates the owner's decision of 2026-10-01 to keep a single GC PRD. Under that decision
  `PRD/future/GC_STAGE5_PRD.md` moved to `PRD/ARCHIVE/`, and `docs/GC_DESIGN.md` remains the as-built description
  of the current collector, rewritten as stages land. Work items are GitHub issues.

## Question

The owner set three goals for a new collector:

- learn from prior art;
- change Patina's value and object representations freely where that helps;
- design for a Cranelift JIT.

The owner also asked four follow-up questions:

1. **Threading model.** Which model should SRFI 18 threads use, and what does the GC owe it? This was answered
   during the research, in `research/threads-*.md`: M:1 green threads now, with the collector's interfaces ready
   for N mutators.
2. **Cost of shared-memory parallelism.** What would it cost to add shared-memory parallelism later? This was
   answered after the design, in `followup/parallelism/`.
3. **Steady state.** What is steady state, what memory contract follows from it, and which heap and stack limits
   should apply? This was answered after the design, in `followup/steady/`.
4. **The GC contract.** What is the contract between the GC and the rest of the runtime, and how do concurrency and
   a Cranelift JIT affect it? This was answered on 2026-10-01, after `PRD/GC_PRD.md` was written, in
   `followup/contract/`.

## Method

1. **Codebase mapping** (`understand/`). The source at `28a94f8` was read area by area. Throwaway probe crates
   measured type sizes, allocation counts and timings.
2. **Prior-art research** (`research/`). Collectors, rooting, barriers and JIT integration were studied in Chez,
   Racket, Larceny, OCaml, Gambit, HotSpot, Immix and MMTk, the JavaScript engines, Whippet, Go, .NET, Lua, Perceus,
   Rust GC crates and Cranelift. Three further reports cover threading: prior art, the cost of each model, and a
   judge's recommendation.
3. **Gap studies with measurements** (`gaps/`). Questions the first two steps left open were answered by
   measurement:
   - an instrumented build took the allocation census, survival rates and store mix;
   - a sampling heap profiler examined library loading;
   - probes compared Patina's behaviour with chibi, Gauche and Chez.
4. **Digest** (`DIGEST.md`). Steps 1–3 were condensed into one brief for the design panel.
5. **Design panel.** Five architects each argued one starting philosophy (`design/proposal-*.md`). Four judges
   reviewed the proposals: GC theory, JIT, Rust soundness and migration. `design/DESIGN.md` is the synthesis, and its
   Appendix A records the judges' findings.
6. **Adversarial review.** Five reviewers (R7RS semantics, JIT feasibility, migration, performance, Rust soundness)
   raised 66 findings. Each was verified, and all were accepted (`design/REVIEW_DISPOSITIONS.md`; DESIGN.md
   Appendix C).
7. **Completeness passes.** Two rounds checked the design for missing obligations (`REVIEW_DISPOSITIONS.md`,
   "Completeness round 1" and "Completeness round 2").
8. **Follow-up studies** (`followup/`). Each answer was reviewed before it closed.
   - Parallelism cost: an itemized cost, measurements and prior art, combined in `ANSWER.md`.
   - Steady state: an audit and prior art, combined in `SECTION.md`.
   - The GC contract: two extractions of the contract, one of the collector as built and one of the PRD's; three
     impact analyses, for concurrency, a Cranelift JIT and other planned features; an adversarial check, whose 22
     findings were verified and applied to the notes; and the synthesis, `ANSWER.md`. The gaps in its §4 are
     rules of the design contract, so it proposes them as amendments to the PRD, with text, rather than as issues.
9. **Issues.** The observed defects were drafted (`design/ISSUES_DRAFT.md`) and filed after a search for duplicates
   (see below).

## Labels

Each report defines its evidence labels in its header, and that definition governs. The common ones:

| Label | Meaning |
|---|---|
| **[V]** | Verified in source, by measurement or in a primary source. DIGEST.md's **[V\*]** marks a fact the digest re-checked. |
| **[S]** | Read in source at the pinned revision (path:line). |
| **[P]** | Measured on Patina, or on an oracle, on the study machine, with the workload named. |
| **[I]** | Inference, judgement or design proposal. |
| **[M]** | Measured in a probe crate (see `probes/`) or on the development machine. |
| **[E]** | Engineering estimate. Effort is in focused engineer-weeks for one engineer who knows the codebase. |
| **[A]** | Analogy: a published figure for another runtime. |

Less common labels:

- **[D]**: quoted from a repository document or another report, not re-run.
- **[C]**: cited from a primary source.
- **[R]**: recalled and not re-verified (`java-hotspot.md`).
- `whippet-misc.md` uses [src], [doc], [meas] and [inf].
- `followup/contract/jit.md` uses [cg §x] for a Cranelift fact checked in `research/cranelift-gc.md` §x, and [rev]
  for one checked only by the design review.

## Issues filed from the study

The issues filed from the study are #603–#618, plus a comment on #597. The defect IDs come from the study:

- A1–A9 are in `design/ISSUES_DRAFT.md`.
- I1–I6 are in `followup/steady/SECTION.md`.
- N1 and P4 are items 1 and 4 of the list "Problems to file as GitHub issues" in `followup/parallelism/ANSWER.md`.

Where a report says "nothing here has been filed", it was written before the filing.

Not filed with this batch:

- Items 2 and 3 of the same list in `ANSWER.md`: three `Rc<CodeObject>` clone/drop pairs per VM call, and SipHash
  in `symbol_table`. They are interpreter-cost observations, not GC defects, and remain candidates for Track P
  issues.
- Items 5 and 6, the ST checklist. They are design work, not defects. `PRD/GC_PRD.md` §22 assigns them to the
  issues of stages 3, 6 and 9 and to cost row R25.

| ID | Issue | Defect |
|---|---|---|
| A1 | #605 | An embedder's value is freed by a later collection |
| A2 | #604 | Dropping an interpreter leaks its heap |
| A3 | #608 | `eq?` on current ports |
| A4 | #607 | Running out of file descriptors (EMFILE) |
| A5 | #603 | Record of rebinding divergences |
| A6 | comment on #597 | Documentation drift |
| A7 | #606 | The collection trigger is blind to bytes |
| A8 | #609 | Ephemeron chain cost depends on order |
| A9 | #610 | `define-library` recognised by spelling |
| I1 | #611 | Interned aliases leak from `guard` |
| I2 | #612 | Provenance grows on re-`eval` of `case` |
| I3 | #613 | Lookup of macro-introduced definitions is quadratic |
| I4 | #614 | A redefined library is retained |
| I5 | #615 | `environment` churn reaches 2.9 GiB |
| I6 | #616 | Pause and RSS after a dropped peak |
| N1 | #617 | Deep nesting overflows the stack and aborts |
| P4 | #618 | Two interpreters share the current ports |

## Index

Top level:

- [`README.md`](README.md): this file.
- [`DIGEST.md`](DIGEST.md): the research digest for the design panel. It covers Patina today, the hard
  requirements a collector must preserve, the design space axis by axis, five candidate architectures, prior-art
  pitfalls, and the decisions only the owner can make.

`understand/`: how Patina works today.

- [`gc-impl.md`](understand/gc-impl.md): today's collector. A non-moving, stop-the-world mark-sweep with safe
  points and deferral; pauses that scale with the arena high-water mark; a trigger that counts allocations.
- [`heap-repr.md`](understand/heap-repr.md): the value encoding, heap layout and object catalogue with measured
  sizes; where identity escapes; what raw pointers, headers or moving would cost.
- [`vm-runtime.md`](understand/vm-runtime.md): the VM runtime as the GC and a JIT see it. Covers frames held as
  data, the register file, continuations and the per-instruction poll.
- [`offheap.md`](understand/offheap.md): value holders outside the heap arenas; the `Rc` graph that leaks an
  interpreter's heap on drop; Rust code that holds handles across allocation.
- [`tree-walker.md`](understand/tree-walker.md): the CPS tree-walker as the GC sees it, and its options under a
  new collector.
- [`primitives-embedding.md`](understand/primitives-embedding.md): how primitives, the front end, macros and the
  embedding API touch the heap, including the embedding API's missing rooting.
- [`jit-readiness.md`](understand/jit-readiness.md): Patina's JIT plans, the register VM as a Cranelift target,
  and a numbered GC–JIT contract.

`research/`: prior art.

- [`chez.md`](research/chez.md): Chez Scheme's storage manager. BiBOP segments; allocation that never collects;
  young objects copied and old ones marked in place; a card table holding generation bytes.
- [`racket-larceny.md`](research/racket-larceny.md): Racket BC's 3m collector, the Racket CS changes to Chez's
  collector, and Larceny's research collectors.
- [`ocaml-gambit.md`](research/ocaml-gambit.md): OCaml 5 and Gambit. Polling through one limit word, frame
  descriptors, `caml_modify`, and Gambit's continuations and allocation classes.
- [`java-hotspot.md`](research/java-hotspot.md): HotSpot's collectors, TLABs, oop maps, safepoints and Loom stack
  chunks, with a verdict on each.
- [`immix-mmtk.md`](research/immix-mmtk.md): the Immix family and MMTk. The recommendation is to adopt Immix-style
  collection without depending on MMTk.
- [`js-engines.md`](research/js-engines.md): GC and rooting in V8, SpiderMonkey and JavaScriptCore, with Nova and
  Boa.
- [`whippet-misc.md`](research/whippet-misc.md): Whippet, Go, .NET, Lua and LuaJIT, and Perceus/Lean reference
  counting.
- [`rust-gcs.md`](research/rust-gcs.md): GC designs in Rust (gc-arena/piccolo, starlark, Nova, Wasmtime, rune)
  and the no-GC-window discipline they share.
- [`cranelift-gc.md`](research/cranelift-gc.md): how a Cranelift JIT integrates with a GC: user stack maps,
  safepoints, Wasmtime's collectors, and guidance for a baseline JIT.
- [`threads-prior-art.md`](research/threads-prior-art.md): threads in Scheme and dynamic-language runtimes. The
  models, SRFI 18 semantics, and what the GC and the JIT had to do.
- [`threads-patina-cost.md`](research/threads-patina-cost.md): what green threads, shared-heap OS threads or
  isolates would each cost Patina.
- [`threads-recommendation.md`](research/threads-recommendation.md): the judge's recommendation. One mutator now
  (SRFI 18 as M:1 green threads), with GC interfaces designed for N mutators.

`gaps/`: open questions answered by measurement.

- [`workload-demographics.md`](gaps/workload-demographics.md): allocation census, survival and store mix over the
  Larceny R7RS benchmarks and extra workloads, taken with an instrumented build.
- [`global-binding-cells.md`](gaps/global-binding-cells.md): a binding-model specification for global binding
  cells, with rebinding probes against the oracles.
- [`continuation-representation.md`](gaps/continuation-representation.md): continuation representations under a
  moving GC and a Cranelift JIT, using Scheme probes and a toy Rust prototype.
- [`barrier-remset-resolution.md`](gaps/barrier-remset-resolution.md): resolves the reports' conflicting advice on
  the write barrier and remembered set, and measures the store mix.
- [`library-load-memory.md`](gaps/library-load-memory.md): why loading the R7RS-large libraries peaks at 627 MiB
  RSS. A sampling heap profiler attributes the peak to deferral, provenance tables and scope copying.
- [`finalization-weak-semantics.md`](gaps/finalization-weak-semantics.md): what the sweep does for free today, the
  weak, ephemeron and finalization semantics owed against chibi and Gauche, and mechanisms that fit a copying
  nursery.

`design/`: the panel and its synthesis.

- [`proposal-bounded.md`](design/proposal-bounded.md): RegionGen, a bounded-pause generational mark-region
  collector. Its starting point was OCaml 5.
- [`proposal-chez.md`](design/proposal-chez.md): a hybrid collector faithful to Chez.
- [`proposal-evolve.md`](design/proposal-evolve.md): a non-moving mark-region heap first, with evacuation and
  generations added as measured switches.
- [`proposal-jitfirst.md`](design/proposal-jitfirst.md): the design derived from the machine code a Cranelift tier
  should emit.
- [`proposal-mmtk.md`](design/proposal-mmtk.md): a collector shaped like MMTk, with mmtk-core as a gated engine. It
  concludes against running on MMTk at the start.
- [`DESIGN.md`](design/DESIGN.md): the recommended design, v2 after review. Part I is the contract (§0–§14, §17
  decisions). Part II is the plan (§15 stages, §16 kill criteria K1–K16). Part III holds the work items
  (appendices B, D, E, H and F), and Part IV the review record (A, C and G).
- [`REVIEW_DISPOSITIONS.md`](design/REVIEW_DISPOSITIONS.md): how each of the 66 adversarial-review findings and
  the two completeness rounds was resolved.
- [`ISSUES_DRAFT.md`](design/ISSUES_DRAFT.md): issue drafts with placeholder numbers and a map of which PR closes
  each issue. The defects in it were filed as listed above.

`followup/parallelism/`: what shared-memory parallelism would cost later (decision 7).

- [`ANSWER.md`](followup/parallelism/ANSWER.md): the synthesis.
  - Part 1 answers the owner.
  - Part 2 drafts the PRD section "Threading readiness and future parallelism (decision 7)", with notes for the
    editor.
  - It ends with the review dispositions. Where it differs from the three studies below, it holds.
- [`itemize.md`](followup/parallelism/itemize.md): 31 cost rows, each costed from today's code and again after
  the redesign.
- [`measure.md`](followup/parallelism/measure.md): the single-thread tax, measured three ways:
  - microbenchmarks;
  - perturbed Patina builds;
  - Chez built with and without threads.
- [`prior-art.md`](followup/parallelism/prior-art.md): what adding parallelism after the fact cost other
  runtimes, and a calibration of the in-house estimates against that record.

`followup/steady/`: steady state, the memory contract and limits.

- [`SECTION.md`](followup/steady/SECTION.md): the PRD section "Steady state, the memory contract, and limits". It
  holds:
  - the definition, SS1–SS5;
  - the contract, M1–M5;
  - the limits and their verification;
  - kill criteria K17–K19;
  - owner decisions SD1–SD9;
  - observed defects I1–I6.
- [`audit.md`](followup/steady/audit.md): every source of unbounded growth today and in the redesign, measured
  against chibi, Gauche and Chez.
- [`prior-art.md`](followup/steady/prior-art.md): how production runtimes define and enforce steady state and
  heap and stack limits.

`followup/contract/`: the contract between the GC and the rest of the runtime, and what concurrency and a
Cranelift JIT do to it.

- [`ANSWER.md`](followup/contract/ANSWER.md): the synthesis. It sets the contract today against the PRD's, party
  by party; sorts concurrency by who may touch heap words while a mutator runs; says what a Cranelift JIT relies on
  and what each GC stage changes in emitted code; lists fourteen gaps (three major), with proposed PRD text; and
  records the corrections the adversarial check made to the notes below.
- [`today.md`](followup/contract/today.md): the contract as built, from the code. What today's collector promises
  (P1–P8), what every other party must do (O1–O17), what enforces each clause, issues #604–#618 by clause, and
  the clauses nothing enforces.
- [`prd-contract.md`](followup/contract/prd-contract.md): the contract in `PRD/GC_PRD.md`, by party and direction.
  The collector's promises (C1–C19); obligations on the VM, JIT code, Rust code, the tree-walker, embedders and
  anyone adding an object kind; the pluggability seam; the N-mutator rules; and gaps found while extracting.
- [`concurrency.md`](followup/contract/concurrency.md): eight kinds of concurrency, from M:1 green threads to
  concurrent relocation. For each: what it keeps, changes and breaks in the contract, its PRD status, its effort
  and its run-time tax.
- [`jit.md`](followup/contract/jit.md): the contract seen from a Cranelift JIT, by tier and feature. Covers the
  Cranelift features needed, optional and avoided; what the shapes the PRD excludes would cost; what today's design
  would demand of the same JIT; and JIT-specific gaps.
- [`other.md`](followup/contract/other.md): other planned features against the contract, each with a verdict: FFI,
  syntax-case, the debugger and hooks, guardians and weak hash tables, a boot image and AOT, delimited
  continuations and effect handlers, long REPL sessions, and general tail calls.

`probes/`: the retained probe programs.

- [`probes/README.md`](probes/README.md): maps each working path the reports cite to its place in `probes/`, or
  says why it was not kept.

## Paths in the reports

The reports were written in a working directory outside the repository. Their citations of probe programs now name
the retained copies under `probes/`, and fetched third-party sources are cited by upstream project and path.
`probes/README.md` maps each working path to its retained location and says what was not kept. Three kinds of path
remain as written:

- relative names of outputs that were not retained, such as `results/…` and `out/…`;
- `$SCRATCH` in `gaps/workload-demographics.md` §10, which names the probe directory, as it does in the retained
  `probes/workload-demographics/instrumented/REPRODUCE.sh`;
- `PRD/future/GC_STAGE5_PRD.md` in `design/DESIGN.md`, `design/ISSUES_DRAFT.md`, `design/proposal-jitfirst.md` and
  `design/REVIEW_DISPOSITIONS.md`, where it names the file those drafts planned to rewrite. The file is now
  `PRD/ARCHIVE/GC_STAGE5_PRD.md`. Citations of its content elsewhere point there, with line numbers shifted by its
  12-line banner.

The contract study (`followup/contract/`) cites `PRD/GC_PRD.md` as §n:L, section and line, at `f82e8e8`, before the
PRD took in the amendments that study proposed. Read those lines at that commit.

## Not retained

Only small probe programs and scripts written during the study were kept, in `probes/`. These were left out:

- **Third-party source caches.** Cranelift and Wasmtime (`src-cache/`), Whippet (`whippet-src/`), Go (`go-src/`),
  Lua and LuaJIT, .NET, Lean, the Rust GC crates (`rust-src/`), and V8, SpiderMonkey, JavaScriptCore, Nova and Boa
  files. Also the Chez Scheme source tree and its two builds, the OCaml changelogs, and the papers and their text
  extractions. The reports name the upstream revisions they read.
- **Copies of the repository used for instrumentation.** The patched copies for the store-mix counters, for the
  demographics census and for the parallelism perturbations. The census patch itself is kept as
  `probes/workload-demographics/instrumented/demographics.patch`, and the perturbation script as
  `probes/followup/perturb_patch.py`.
- **Larceny benchmark sources.** These are LGPL; see AGENTS.md. This covers the R7RS benchmark sources and inputs,
  the adapted `gcold` and `queue3` programs, and the working copies of the chibi and Larceny test suites. It also
  covers copies of the Gabriel-derived programs (`ctak`, `deriv`, `nboyer` and others) that the repository already
  carries in `crates/patina-tests/bench_programs/`. `probes/README.md` lists each.
- **Build outputs, profiles and raw results.** Target directories, binaries, `sample` profiles, heap profiles of up
  to 27 MB, JSONL and TSV result files, and generated program families of up to 10 MB. Each generated family is kept
  as the generator that produced it. The reports carry the numbers.

The measurement harness is to be vendored in stage 0:

- the `gc` mode of `scripts/benchmarks.py`;
- the `gc-census` cargo feature, which succeeds `demographics.patch`;
- `scripts/gc_probes/`, drawn from `probes/`, for example `design-review/*.c` and the continuation toy in
  `continuation-representation/`.

Those in-tree copies, not `probes/`, are what later stages measure with.
