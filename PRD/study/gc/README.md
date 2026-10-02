# GC redesign study

The research record behind [`PRD/GC_PRD.md`](../../GC_PRD.md).

- **Dates:** research on 2026-09-30; design, reviews and follow-up studies on 2026-10-01.
- **Revision:** `main` at `28a94f8`. The study read and measured it and did not modify it. The contract study
  (`followup/contract/`) also read `PRD/GC_PRD.md` at `f82e8e8` on branch `gc-prd`, a commit that adds only `PRD/`
  to that revision. The premature-collection study (`followup/premature/`) read `gc-prd` at `f82e8e8` and
  `3682302`, which also add only `PRD/`.
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

The owner also asked five follow-up questions:

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
5. **Premature collection.** Live values got collected in the current implementation. How do we prevent it? This
   was answered on 2026-10-01, after the contract study, in `followup/premature/`.

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
   - Premature collection:
     - git, issue and PR mining over the git history since GC stage 1 (`b908f16`, 2026-07-31), all 406 merged PRs
       and all 204 issues (`git-cases.md`, `gh-cases.md`);
     - an inventory of today's defenses and remaining hazards, with an embedding probe (`defenses-hazards.md`);
     - a merged catalogue of the cases, classified into root-cause classes (`catalogue.md`);
     - a prevention map, and a ranked list of what can land now, measured on scratch copies of the tree
       (`prevention.md`, `do-now.md`);
     - two review rounds of those two, whose 17 findings were verified (`ANSWER.md`, Appendix A);
     - the synthesis, `ANSWER.md`.
9. **Issues.** The observed defects were drafted (`design/ISSUES_DRAFT.md`) and filed after a search for duplicates
   (see below). The premature-collection study's live defect and its do-now list were filed the same way.

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

The issues filed from the study are #603–#618 and #620–#626, plus a comment on #597 and comments on #605 and #609.
The defect IDs in the first table come from the study:

- A1–A9 are in `design/ISSUES_DRAFT.md`.
- I1–I6 are in `followup/steady/SECTION.md`.
- N1 and P4 are items 1 and 4 of the list "Problems to file as GitHub issues" in `followup/parallelism/ANSWER.md`.

Where a report says "nothing here has been filed", or `followup/premature/` calls its incident A7 "not filed" or
"unfiled", it was written before the filing.

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

The premature-collection study (`followup/premature/`) numbers its incidents A1–A7 independently of
`ISSUES_DRAFT.md`. `ANSWER.md` uses `catalogue.md`'s labels; `git-cases.md` and `gh-cases.md` keep their own, so
`gh-cases.md`'s A2, A3 and B1 are the catalogue's A4 (#130), A6 (#605) and A3 (#47). The study's live incident A7,
and the issues that `ANSWER.md` §4 lists to file (A–F there, covering the items of its ranked do-now list), were
filed on 2026-10-01:

| Study item | Issue | Subject |
|---|---|---|
| Incident A7 | #620 | Bindings in a host-built environment are freed by a later collection, and the VM's `Backend::eval` ignores the environment it is given |
| Items 1, 2 and 9 (A) | #621 | Make every stale reference panic in debug and `gc-check` builds |
| Item 3 (B) | #622 | Put every Rust re-entry into the evaluator under a clippy rule |
| Item 4 | comment on #605 | Widens #605 to every public path that hands the host a raw value or environment, a handle table that traces its handles, and tests over all 10 `eval_*` methods |
| Item 5 (C) | #623 | Name every traced field, and test each with a sentinel |
| Items 6 and 7 (D) | #624 | Assert the deferral protocol and the no-collection windows |
| Item 8 (E) | #625 | Mark retired registers with `DEAD_SLOT`, and add a zeal lane |
| Item 10 (F) | #626 | Run more programs under GC stress |
| The #609 entry | comment on #609 | Adds a naive-fixpoint mark oracle to #609's acceptance |

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

`followup/premature/`: live values that got collected in the current implementation, and how to prevent it.

- [`ANSWER.md`](followup/premature/ANSWER.md): the synthesis. The history of the seven incidents (A1–A7); the
  design properties that made them possible; prevention by construction, detection and process, sequenced against
  the PRD's stages; the ranked do-now list; what stays uncatchable; and, in Appendix A, the two review rounds'
  findings and what each changed.
- [`git-cases.md`](followup/premature/git-cases.md): every case in the git history since GC stage 1 (`b908f16`,
  2026-07-31): live values freed or latent unrooted holders, near relatives, protections added with a holder, and
  fixes to the detectors.
- [`gh-cases.md`](followup/premature/gh-cases.md): the same history mined from issues, PRs, review-round commit
  messages and failed CI runs, with the failures of the detection machinery itself.
- [`defenses-hazards.md`](followup/premature/defenses-hazards.md): the defenses that exist today, the hazards that
  remain, and which defense sees which. Its embedding probe found the host-built environment that became A7.
- [`catalogue.md`](followup/premature/catalogue.md): the merged catalogue (incidents A1–A7, near relatives B1–B6,
  protections, detector failures and a timeline) and the root-cause taxonomy, classes R1–R8 and X, with each
  class's latent entries.
- [`prevention.md`](followup/premature/prevention.md): a prevention map. For each class, what prevents, flags or
  catches it; the cross-cutting techniques (capability and brand, static analysis, zeal, poison, Miri, fuzzing,
  mutation testing, differential lanes) evaluated once; a process tier; and what to do now and by stage.
- [`do-now.md`](followup/premature/do-now.md): what can land on today's collector, measured on scratch copies of
  the tree. Probe results per build, the measures with their costs, those evaluated and not recommended, and the
  issues to file.

`probes/`: the retained probe programs.

- [`probes/README.md`](probes/README.md): maps each working path the reports cite to its place in `probes/`, or
  says why it was not kept.

## Paths in the reports

The reports were written in a working directory outside the repository. Their citations of probe programs now name
the retained copies under `probes/`, and fetched third-party sources are cited by upstream project and path.
`probes/README.md` maps each working path to its retained location and says what was not kept. Four kinds of path
remain as written:

- relative names of outputs that were not retained, such as `results/…` and `out/…`;
- `$SCRATCH` in `gaps/workload-demographics.md` §10, which names the probe directory, as it does in the retained
  `probes/workload-demographics/instrumented/REPRODUCE.sh`;
- `PRD/future/GC_STAGE5_PRD.md` in `design/DESIGN.md`, `design/ISSUES_DRAFT.md`, `design/proposal-jitfirst.md` and
  `design/REVIEW_DISPOSITIONS.md`, where it names the file those drafts planned to rewrite. The file is now
  `PRD/ARCHIVE/GC_STAGE5_PRD.md`. Citations of its content elsewhere point there, with line numbers shifted by its
  12-line banner.
- `scratch/…`, `<scratch>` and `$S` in `followup/premature/`, which name that study's working directory: its copies
  of the tree (`base` and `exp` to `exp5`) and their patches, its probe crates, its PR and issue dumps, and its
  outputs. None of it was retained.

The contract study (`followup/contract/`) cites `PRD/GC_PRD.md` as §n:L, section and line, at `f82e8e8`, before the
PRD took in the amendments that study proposed. Read those lines at that commit.

The premature-collection study's `ANSWER.md` describes `PRD/GC_PRD.md` at `3682302`, before the PRD took in that
study's amendments. Its "In the PRD?" and "PRD" columns, and the gaps they name (among them the `Trace` derive, the
host-environment handles and the detection measures filed as #621–#626), are stale wherever the PRD took them in.

## Not retained

Only small probe programs and scripts written during the study were kept, in `probes/`. These were left out:

- **Third-party source caches.** Cranelift and Wasmtime (`src-cache/`), Whippet (`whippet-src/`), Go (`go-src/`),
  Lua and LuaJIT, .NET, Lean, the Rust GC crates (`rust-src/`), and V8, SpiderMonkey, JavaScriptCore, Nova and Boa
  files. Also the Chez Scheme source tree and its two builds, the OCaml changelogs, and the papers and their text
  extractions. The reports name the upstream revisions they read.
- **Copies of the repository used for instrumentation.** The patched copies for the store-mix counters, for the
  demographics census and for the parallelism perturbations, and the premature-collection study's copies (`base`
  and the experiments `exp` to `exp5`). The census patch itself is kept as
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
