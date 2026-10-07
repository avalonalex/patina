# Garbage collector: design and plan

| | |
|---|---|
| Date | 2026-10-01, repository `main` at `28a94f8` |
| Status | Proposed. The owner decisions of 2026-10-01 are marked decided in §2; the other rows there are proposed defaults awaiting owner review, except the two Open rows (7a, 7b). Amended 2026-10-01 after the GC–runtime contract study ([`ANSWER.md`](study/gc/followup/contract/ANSWER.md)) and the premature-collection study ([`ANSWER.md`](study/gc/followup/premature/ANSWER.md)) |
| Supersedes | The stage-5 PRD (formerly under `PRD/future/`, now [`PRD/ARCHIVE/GC_STAGE5_PRD.md`](ARCHIVE/GC_STAGE5_PRD.md)); §21 says where each of its items went |
| As built | [`docs/GC_DESIGN.md`](../docs/GC_DESIGN.md) describes the collector Patina runs today. It is rewritten as each stage lands; this file is the target and the plan |
| Research record | [`PRD/study/gc/`](study/gc/README.md), indexed by its README: the digest (`DIGEST.md`); studies of Patina today (`understand/`), of prior art (`research/`) and of six gaps (`gaps/`); the five candidate proposals, the recommended design this file condenses ([`DESIGN.md`](study/gc/design/DESIGN.md)), its review dispositions and the issue drafts (`design/`); the follow-ups on parallelism cost ([`ANSWER.md`](study/gc/followup/parallelism/ANSWER.md)), steady state ([`SECTION.md`](study/gc/followup/steady/SECTION.md)), the GC–runtime contract ([`ANSWER.md`](study/gc/followup/contract/ANSWER.md)) and premature collection, the live values collected so far, their causes and their prevention ([`ANSWER.md`](study/gc/followup/premature/ANSWER.md)), in `followup/` |
| Work items | GitHub issues. The present-day defects are filed and mapped to stages in §17.5 and §19; stage issues are filed as each stage starts (§22) |

**Contents.**
- [1. Executive summary](#1-executive-summary) and [2. Owner decisions](#2-owner-decisions).
- **Part I, the design contract (§3–§18):** [3. Prior art](#3-prior-art-what-we-take-from-whom) ·
  [4. Thesis](#4-thesis-and-key-bets) · [5. Value encoding](#5-value-encoding) · [6. Object model](#6-object-model) ·
  [7. Heap organisation](#7-heap-organisation) · [8. Allocation](#8-allocation) · [9. Collection](#9-collection) ·
  [10. Write barrier](#10-write-barrier) · [11. Roots](#11-roots-and-rooting) ·
  [12. Safepoints](#12-safepoints-and-polling) · [13. Continuations](#13-continuations-and-stacks) ·
  [14. Pluggability](#14-pluggability-contract) · [15. Pacing](#15-heap-sizing-pacing-and-observability) ·
  [16. Testing](#16-testing-and-verification) ·
  [17. Steady state and limits](#17-steady-state-the-memory-contract-and-limits) ·
  [18. Threading](#18-threading-readiness-and-future-parallelism-decision-7).
- **Part II, the plan (§19–§22):** [19. Stages](#19-stages) ·
  [20. Risks and kill criteria](#20-risks-mitigations-and-kill-criteria) ·
  [21. The stage-5 PRD's items](#21-where-the-stage-5-prds-items-went) · [22. Work items](#22-work-items).

**Reading paths.**
- **Owner:** the header, [§1](#1-executive-summary), [§2](#2-owner-decisions), [§19](#19-stages),
  [§20](#20-risks-mitigations-and-kill-criteria) ([K9](#k9), [K14](#k14)),
  [§17.1](#171-definition)–[§17.3](#173-limits), [§18.3](#183-cost) and [§18.7](#187-decision-record).
- **Implementer:** [§3](#3-prior-art-what-we-take-from-whom)–[§16](#16-testing-and-verification), then
  [§17](#17-steady-state-the-memory-contract-and-limits) and
  [§18](#18-threading-readiness-and-future-parallelism-decision-7).
- **Stage author:** the stage's row in [§19](#19-stages), the sections it names, and [§22](#22-work-items).

**Conventions.** **[P]** marks a measurement of Patina, or of an oracle where one is named, on one machine (Apple M4
Pro, macOS 27.2 arm64, 16 KiB pages, release build at `28a94f8`, 2026-10-01 unless stated), with the workload named.
Provenance: DESIGN Appendix B, which stage 0 re-measures after bringing in the harness, the census and the probes; for
[§17](#17-steady-state-the-memory-contract-and-limits), `PRD/study/gc/followup/steady/audit.md` and the linked issues;
for [§18](#18-threading-readiness-and-future-parallelism-decision-7), `PRD/study/gc/followup/parallelism/measure.md`.
**[S]** marks a fact read in source, at `28a94f8` or an upstream path; **[I]** marks judgement or estimate; **[A]**
marks an analogy from a published figure for another runtime.
[§18](#18-threading-readiness-and-future-parallelism-decision-7) also uses its source study's **[M]** (measured) and
**[E]** (estimate), defined there. Effort is in focused engineer-weeks [I]. "DESIGN" followed by a section or appendix
(H.1, E.2, Appendix G) points into [`PRD/study/gc/design/DESIGN.md`](study/gc/design/DESIGN.md), which keeps the
mechanism walkthroughs, derivations, inventories and work-item detail this file leaves out. **Where DESIGN or
`PRD/study/gc/design/ISSUES_DRAFT.md` differ from this file, this file holds**; [§22](#22-work-items) lists the known
differences. Stage numbers are this plan's; `docs/GC_DESIGN.md`, AGENTS.md and the archived stage-5 PRD use the earlier
GC staging (stages 1–4c, then "5+"). K*n* is a kill criterion of [§20](#20-risks-mitigations-and-kill-criteria). "The
matrix" is `crates/patina-tests/tests/control_flow_matrix.rs` (64 rows [S]); "the hygiene matrix" is
`crates/patina-tests/tests/hygiene_matrix.rs` (139 shapes).

**Terms.**

| Term | Meaning |
|---|---|
| L | live bytes after the last major: marked bytes plus LOS and external bytes ([§15](#15-heap-sizing-pacing-and-observability), [§17.1](#171-definition)) |
| B | the memory budget: the smaller of physical RAM and the cgroup limit ([§17.3](#173-limits)) |
| F(L) | the footprint a steady program may reach: 1.1·(L + max(8 MiB, 2·L)) plus the free reserve ([§17.1](#171-definition)) |
| GBS | the GC benchmark set ([§16](#16-testing-and-verification)) |
| MMU | minimum mutator utilization: the smallest share of any window of a given length (here 10 ms) left to the program ([§15](#15-heap-sizing-pacing-and-observability)) |
| SATB | snapshot-at-the-beginning marking, an incremental discipline whose barrier logs overwritten values; reserved for slow paths, not built ([§10](#10-write-barrier), [§14](#14-pluggability-contract)) |
| BFG extension | Bacon–Fink–Grove: an object moved after its identity hash was taken carries the original hash in a trailing granule ([§9.8](#98-identity-hash)) |
| Granule; block | the 16 B unit of allocation and side metadata; blocks are 32 KiB ([§7](#7-heap-organisation)) |
| Mark-region | allocation by bumping into the holes between marked objects, with a lazy sweep that reads only metadata ([§7](#7-heap-organisation), [§9.3](#93-marking-order-lazy-sweep)) |
| Sticky marks | generations without copying: a marked object stays marked, and so old, until the next major ([§9.2](#92-minor-collection-sticky-marks-generational-heaps-only)) |
| Field logging; store buffer (SSB) | the write barrier logs the first store into an armed granule of an old object in a per-mutator sequential store buffer ([§10](#10-write-barrier)) |
| Watermark | a return barrier in the register stack: after a minor, the frames below it hold no young reference ([§11.1](#111-vm-frames-and-the-stack-watermark)) |
| SOS, LOS, NMS | the small-object, large-object and non-moving spaces ([§7](#7-heap-organisation)) |
| `Cx` | the context through which Rust code reads, writes and allocates heap values; nothing reachable from it can collect ([§11.3](#113-rust-code-the-soundness-boundary)) |
| Store funnel | `Cx::store`, the one Rust path that writes a value into a heap object ([§10](#10-write-barrier)) |
| `NoGcScope` | a window in which collection is deferred: confined, counted and bounded by [K16](#k16) ([§11.3](#113-rust-code-the-soundness-boundary), [§12](#12-safepoints-and-polling)) |
| `Leaf`, `Transfer` | the two helper classes: a `Leaf` never collects, runs Scheme, transfers control or blocks; a `Transfer` may, and the calling code resumes wherever the helper answers ([§11.2](#112-jit-frames-by-tier-the-tier-contract)) |
| Fragment | a unit of JIT code ending in a tail call (`CallConv::Tail`); a return goes through a trampoline, not a native `ret` ([§11.2](#112-jit-frames-by-tier-the-tier-contract)) |
| Carrier; green thread | a carrier is a `Mutator`, the context with which one OS thread runs Scheme; a green thread is an SRFI 18 thread scheduled on a carrier ([§18.1](#181-the-m1-design-and-its-n-ready-interfaces)) |
| Zeal | torture modes that collect, move or invalidate at every opportunity ([§14](#14-pluggability-contract)) |
| Strangler | stage 5's kind-by-kind migration, with the new heap beside the old arenas until 5e deletes them ([K10](#k10)) |
| Design A; C′ | continuation representations: A copies the stack into one immutable heap object at capture; C′ freezes frames into immutable chunks that are thawed lazily ([§13](#13-continuations-and-stacks)) |
| Variant R; variant C | global bindings. Under R a later `define` or import re-points the binding record that earlier code holds (today's behaviour); under C references bind to cells at compile time, as in chibi, Chez and Racket ([§11.6](#116-global-bindings-under-variant-r)) |
| Points A–D | where loading can collect: A between a library body's forms; B between libraries; D inside the forms of a body loaded from a top-level import, in the outermost loop. C, an import met mid-form, cannot ([§11.3](#113-rust-code-the-soundness-boundary)) |

## 1. Executive summary

**The owner's questions, answered.**

| Question | One-line answer | Where |
|---|---|---|
| What should be learned from prior art, Chez and the latest Java collectors included? | Taken: Chez's allocation-never-collects contract, tag-in-displacement addressing, layout generator and ¾-live evacuation rule; HotSpot's one GC interface for every tier, its generational evidence and its GCLocker lesson. Not taken: Chez's 19 spaces and store buffer, HotSpot's catalogue of collectors, and ZGC's and Shenandoah's load barriers and concurrent compaction (throughput first, decision 6) | [§3](#3-prior-art-what-we-take-from-whom), [§4](#4-thesis-and-key-bets) |
| How should the freedom to change representations be used? | Fully, because the measured costs are representational: 61-bit fixnums, raw tagged addresses, headerless pairs, self-tagged flonums, and objects with no Rust `Drop` payload. Observable effects are recorded as divergences | [§5](#5-value-encoding), [§6](#6-object-model); decisions 4, 5, 21; decision 19 |
| What does a Cranelift JIT need from the GC? | Scheme values in VM register frames at every call that can collect, so no Cranelift stack maps; a pinned `Mutator`; an ABI frozen after a throwaway spike | "For the JIT" below; [§11.2](#112-jit-frames-by-tier-the-tier-contract), [§13](#13-continuations-and-stacks), [§14](#14-pluggability-contract); stage 6; decisions 8, 9, 20, 25; [K6](#k6), [K7](#k7), [K11](#k11) |
| Is a pluggable design like Java's sensible? | The contract yes; a catalogue of collectors no | [§14](#14-pluggability-contract); decision 24 |
| What would shared-memory parallelism cost later? | Itemized, 9–18 engineer-months after the redesign; about 2–9 engineer-years with overrun and recovery; a single-thread tax of ≈2–5%. Proposed: not now | [§18.3](#183-cost), [§18.4](#184-single-thread-tax-the-threaded-build-with-one-carrier); decisions 7, 7c |
| What is steady state, and which limits apply? | Five clauses tested on deterministic readings; a memory contract that bounds every source of growth; limits derived from the machine's budget, raising a catchable condition that names what grew | [§17](#17-steady-state-the-memory-contract-and-limits); decision 15; SD1–SD9; stage 5g |
| Live values got collected in the current implementation: how do we prevent it? | In three layers, in this order: detection on today's collector now, so the next one panics where it happens ([#621]–[#626]), and no stage moves a collection point before it is in CI; the two live defects fixed at stage 2 ([#605], [#620]); then the bug made unwritable: rooted handles and the `Trace` derive (stage 2), the collect capability and the value brand (stage 3), continuations as heap objects (4e) and generated heap-kind traces (5a) | [§16](#16-testing-and-verification), [§14](#14-pluggability-contract), [§19](#19-stages); [§11.5](#115-embedding-api) |
| Where do the design and the plan live? | In this one file; `docs/GC_DESIGN.md` stays the description of the collector as built | decision 22 |
| Where is the research? | `PRD/study/gc/`, indexed by its README | [study README](study/gc/README.md) |

Decided on 2026-10-01: rows 1, 2, 6 (priority), 7 (threading model), 12, 14, 15 (scope) and 22 of
[§2](#2-owner-decisions). Every other row is a proposed default, except the Open rows 7a and 7b.

1. **Representation first, collector second.** The measured costs are the representation (72 B enum slots, `Rc`
   payloads, relocating `Vec` arenas, `Rc<RefCell<Heap>>`, a sweep over the arena high-water mark), not mark-sweep:
   2.05× the bytes of a headered layout [P, 348 M-object census], and a 178 ms worst pause [P, first collection after
   loading 25 R7RS-large libraries], about 18 ms of it sweep and the rest releasing 2.9 M Rust `Drop` payloads and
   pruning provenance.
2. **Value word:** 61-bit fixnums under tag `000`; an exact heap bit; 4-bit heap tags from 16 B alignment; self-tagged
   flonums if they pay ([K6](#k6)); raw tagged addresses. **Objects:** headerless pairs, procedures and records; one
   immutable header elsewhere; no GC bits and no Rust `Drop` payload in any object; one layout specification generates
   size, trace, copy, verify and the JIT offsets.
3. **Heap:** one VA reservation per heap; 16 B granules with a side metadata byte; 32 KiB blocks; mark-region allocation
   with a lazy metadata-only sweep; a large-object space with young and old lists; a non-moving space for descriptors,
   record types, symbols, cells and binding records; an immortal space only for what the binary and the program text
   bound; decommit with hysteresis.
4. **Allocation never collects** (Chez runs a moving generational GC under the same contract); the fast path is
   `add; cmp; b.hi`; a user-sized request that finds the heap full collects and retries once, at its call.
5. **One production collector, `MarkRegion`:** non-moving and whole-heap first; sticky generations and Immix-style
   evacuation are per-heap settings fixed at creation, whose defaults measured gates set (decision 25); a copying
   nursery is the fallback ([K4](#k4)). The barrier is field logging on a per-granule armed bit, 4 aarch64
   instructions for heap values and 1 for immediates.
6. **Roots are precise.** VM register frames hold every Scheme value, tagged, at every non-`Leaf` call in every JIT
   tier; no Cranelift stack maps. Rust holds raw values freely inside a `Cx`, which cannot collect; only the driver's
   `&mut Heap` can. **One poll word:** posting an event zeroes the stack limit that every frame entry checks.
7. **Weakness:** ephemerons resolved by key in one fixpoint; a fixed epilogue; a finalization registry; a weak symbol
   table; GC-time port flush and `EMFILE` collect-and-retry, as chibi and Gauche do. **Continuations** are immutable
   value-only heap objects (design A).
8. **Pacing and pauses:** throughput first (decided), so stop-the-world with no incremental or concurrent marking.
   Pacing is in bytes plus external bytes, deterministic; the whole-heap interval is `max(8 MiB, 2·L)`, L being the live
   bytes after the last major; minors and majors are budgeted in one form (a constant plus terms in frames, logged
   granules and marked MiB), no term growing with dead objects or heap size; parallel stop-the-world marking (stage P)
   is budgeted, because a single-threaded major at 1 GiB live is estimated at 120–320 ms [I].
9. **Steady state is a contract ([§17](#17-steady-state-the-memory-contract-and-limits)).** A program with bounded live
   data never hits a limit; one that grows hits one and is told what grew. Clauses SS1–SS5 define steady state on
   deterministic readings; rules M1–M5 bound every source of growth (run-time objects are mortal, Rust tables bounded or
   pruned, external bytes counted, no lookup scanning history, empty memory returned). Patina fails steady state today
   in the ways listed in [§17.5](#175-present-day-failures).
10. **Limits:** `max_heap` = min(16 GiB, 75%·B), B the smaller of RAM and the cgroup limit, counting external bytes; an
    opt-in soft target; a progress guard instead of thrashing; a catchable `&heap-exhausted` naming what grew;
    `&stack-exhausted`; an idle trim at waits (5e; thread waits at stage 9). Most limits land in sub-stage 5g.
11. **Pluggability is a contract** ([§14](#14-pluggability-contract)): HotSpot's GC interface without its catalogue of
    collectors. One production collector; `NullGc` only in the conformance suite; no load barriers, no `dyn` on fast
    paths, no code patching, no MMTk dependency. Patina has one JIT and one collector, so a data description the emitter
    switches on suffices; a nursery or an MMTk adapter can still plug in through `Collector<M>`.
12. **Threads and parallelism ([§18](#18-threading-readiness-and-future-parallelism-decision-7)):** SRFI 18 as M:1 green
    threads (decided) over a carrier/thread split, protocols written for N mutators. Shared-memory parallelism is **not
    now** (proposed). Like for like, both itemized with no overrun factor: the redesign is 99–138 engineer-weeks, and
    parallelism after it 38–77 more (29–61.5 with decision 7c's additions) [I]. Other runtimes' histories put the
    planning figure at about 2–9 engineer-years with overrun and recovery [A, I]. A threaded build's single-thread tax
    measured +1.9% on today's interpreter [P] and is estimated at ≈2–5%, up to ~7%, after the redesign [I].
13. **What it buys.** Stage 5's exit gates ([K9](#k9)), against the stage-4 baseline: GBS geomean ≥ 5% faster in cycles;
    peak RSS ≥ 30% lower on libload, gcold and queue3; queue3's max pause under 15 ms (41 ms today [P]); the post-load
    major at most half its baseline; `Drop`-carrying allocations ≤ 1%; every class A–C steady-state row green. Later
    gains come only behind measured gates ([K1](#k1) generations, [K3](#k3) evacuation, stage P pauses). If stage 5 runs
    2× over its estimate, whole-heap `MarkRegion` ships and stages 7–8 wait ([K14](#k14)).
14. **What users and embedders will notice.** New default limits, a behaviour change: a program whose heap grows past
    min(16 GiB, 75%·B), which today runs until the machine's memory is exhausted, raises `&heap-exhausted` unless
    `--heap-max` or `PATINA_HEAP_MAX` raises the limit (5e); the main stack is capped at min(8 GiB, 25%·B), raised by
    `--stack-max` or `PATINA_STACK_MAX` (4d). GC-time port flushing becomes observable (4a). Semantic changes, each in
    its own PR with its `DIVERGENCES.tsv` rows (decision 19): port `eq?` ([#608], 4a), symbol-keyed `eq?` table order
    (5c), flonum `eq?` and flonum-keyed ephemerons (5f), deep-bound `parameterize` (9), and the six define-after-use
    tests that flip under variant C (after stage 5). Embedders get `Owned` and environment handles, `Interpreter::call`
    and mandatory teardown (stages 2–3), and the `Backend` trait migrates one step per stage under [#601]'s deprecation
    convention. From stage 2 the VM evaluates in the environment it is given, as the tree-walker does, so a `define`
    evaluated in a host environment stops landing in the global one ([#620]).

**For the JIT.** JIT code keeps every Scheme value in the VM's register frames, tagged, at every call that can collect,
so Cranelift stack maps are never needed. One context, the `Mutator`, sits in a pinned register (`x21` on aarch64, `r15`
on x64), set by a generated entry trampoline, and code is never patched at GC. A heap's `GcAttrs` are fixed when it
is created (decision 25), so no installed body runs under a barrier it was not compiled for. Allocation is
`add; cmp; b.hi`, the barrier 4 instructions, a call a tag test plus 2 dependent loads, a global 2 loads under variant
R or 1 under variant C ([§11.6](#116-global-bindings-under-variant-r)); polls are free at calls and 2 instructions at
back-edges.
**Trade-off:** returns go through a trampoline (the fragment model, [§11.2](#112-jit-frames-by-tier-the-tier-contract));
stage 6 measures it against native call/ret, and above 8% ([K11](#k11)) a separate native design is opened. **JIT-only
cost here:** stage 6, 3–5 weeks on an unmerged branch beside stage 5. It freezes the `Mutator` ABI, the helper classes,
the frame contract and the entry trampoline, not object layouts, which `declare_layouts!` regenerates into the JIT
offset table. **No JIT PRD exists yet;** until one does, [§11.2](#112-jit-frames-by-tier-the-tier-contract),
[§13](#13-continuations-and-stacks), [§14](#14-pluggability-contract) and stage 6 are the GC's contract with it (see
[§10 of `PRD/VM_OPTIMIZATION_ROADMAP.md`](VM_OPTIMIZATION_ROADMAP.md#10-jit-compilation)).

**The plan ([§19](#19-stages)).** Stage 0 brings in the benchmark mode, census, probes and steady-state lane; 1 fixes
today's measured blow-ups on the current collector; 2 makes roots slot-based and embedding sound; 3–4 build the core
every candidate design needs; 5 is the new heap, with limits in 5g; 6 the JIT ABI spike; 7 and 8 generations and
evacuation behind measured gates; 9 threads readiness; P parallel marking. About 99–138 engineer-weeks in all [I],
roughly 23–32 months for one engineer (14–21 weeks less with two, [§19](#19-stages)); 5–8 of those weeks are decision
7c's threading additions. The figure is itemized, with no overrun factor; [K14](#k14) is its only calibration.

## 2. Owner decisions

**Needed before stages 0–3:** 7c, 19 and SD1 (stage 0), 16 and 21 (stage 1), 13 (stage 2) and 9 (stage 3). Every other
row can wait for the stage in its "Needed by" column.

**How to decide.** Change a row's Status to "Decided YYYY-MM-DD", or write the alternative chosen. A row still Proposed
when its Needed-by stage starts blocks that stage's issue.

Decided rows carry their date; the Open rows 7a and 7b have no default yet; every other row is a proposed default. The
decision column gives the decision or default in one sentence and the section that specifies it. SD1–SD9 come from the
steady-state study and keep their labels; SD2 is folded into decision 11.

| # | Decision | Status | Decision, or **proposed default** | Consequence of the alternative | Precedent | Needed by |
|---|---|---|---|---|---|---|
| 1 | Tree-walker role | Decided 2026-10-01 | **Kept, and allowed to lag:** whole-heap, non-moving, collecting only at its outermost trampoline, M:1 and `!Send` ([§11.4](#114-the-tree-walker-it-stays-correct-and-may-lag), [§18](#18-threading-readiness-and-future-parallelism-decision-7)) | Full parity (heap frames) would cost 6–12 weeks and make tree-walker `define`s the dominant barrier site (12.48 M on nboyer [P]) | — | — |
| 2 | Global rebinding semantics | Decided 2026-10-01 | **Variant R at stage 4b, variant C after stage 5**, both specified in [§11.6](#116-global-bindings-under-variant-r); C is its own [§19](#19-stages) row | Staying on R costs the JIT a second dependent load per global, or `WATCHED` on record and cell. C gives one-load JIT globals and guards that recover when a program restores the procedure (today's shadow bit never clears) | C: chibi, Chez and Racket bind references at compile time | row C needs decision 3 settled first |
| 3 | When `define` rebinds; mid-program imports | Proposed | **Case by case** over six shapes (p7, p8, p9, q1, q2/q3, d1), recorded under R at stage 0 with their `DIVERGENCES.tsv` rows ([#603]); under C most follow all the oracles and p9 follows chibi (cases: DESIGN E.2) | The oracles disagree on p9, so no single rule such as "bind at expansion time" describes C | chibi, Gauche and Chez disagree on p9 | row C (after 5) |
| 4 | Value encoding | Proposed | **61-bit fixnums, an exact heap bit, 4-bit heap tags, sign-symmetric self-tagged flonums subject to [K6](#k6), raw addresses** ([§5](#5-value-encoding)) | NaN-boxing loses 61-bit fixnums; boxed flonums keep 19.8% of allocations [P]; offset addressing adds one `add` per access | Chez (tags in the displacement); Gambit (self-tagged flonums) | 5a ([K6](#k6) before 6) |
| 5 | Strings and bignums | Proposed | **UTF-32 inline strings (O(1) `string-set!`) and inline bignum limbs** ([§6](#6-object-model)) | UTF-8 halves string bytes but rewrites the string primitives (today's 252 heap mentions in `strings.rs`/`characters.rs` keep working under UTF-32); `num-bigint` payloads would need finalization | — | 5d |
| 6 | Pauses: priority | Decided 2026-10-01 | **Throughput first; stop-the-world; bounded** ([§9](#9-collection), [§9.10](#910-pause-budgets)) | A hard latency target would add incremental marking through `barrier_mode`, with the barrier compiled into every heap that may run it ([§14](#14-pluggability-contract)) | Chez and Racket CS collect stop-the-world | — |
| 6 | Pauses: what "bounded" means | Proposed | **One budget form for minors and majors, with no term growing with dead objects or heap size**, checked by per-workload max pause and MMU(10 ms), the minimum mutator utilization over 10 ms windows ([§9.10](#910-pause-budgets)) | Another form changes the gates of stages 5, 7 and P; parallel marking (stage P) shortens majors without a latency target | Larceny's MMU log | 5 exit; 7b |
| 7 | SRFI 18 threading model | Decided 2026-10-01, by the research the owner delegated it to | **M:1 green threads, VM first, with interfaces written for N carriers over one heap** ([§18.1](#181-the-m1-design-and-its-n-ready-interfaces)) | — | Gambit, SRFI 18's reference implementation | 9 |
| 7 | Shared-memory parallelism | Proposed, pending owner review | **Not now**; start only when [§18.7](#187-decision-record)'s four conditions hold. Parallel stop-the-world marking (stage P) does not depend on this | Shared memory now or soon costs [§18.3](#183-cost)'s figures: 38–77 engineer-weeks itemized after stages 0–9 and P [I], about 2–9 engineer-years with overrun and recovery [A, I]. Isolates for share-nothing work: 3–5 weeks after 5e [I], starting no earlier than 5g, whose `MemoryBudget` they share | the retrofit histories of [§18.3](#183-cost) (OCaml, Racket places, CPython) | [§18.7](#187-decision-record)'s trigger |
| 7a | One threaded build or two | Open until [§18.7](#187-decision-record)'s trigger | — (one build only if the threaded build's tax is within the plan's 1% rule and its footprint within SS2's tolerance) | A higher threshold needs its own row | Chez kept a non-threaded build until 10.0 | [§18.7](#187-decision-record)'s trigger |
| 7b | `thread-terminate!` of a thread blocked in a system call | Open until [§18.7](#187-decision-record)'s trigger | — (may it wait for the call to return?) | — | — | [§18.7](#187-decision-record)'s trigger |
| 7c | Cheap threading additions | Proposed | **Adopt [§18.6](#186-cheap-additions-decision-7c-proposed-scheduled-in-19)'s additions** in stages 0, 2, 3, 4b, 4e, 5, 6, 9 and P: 5–8 engineer-weeks [I] during the redesign | Drop them: the redesign's total falls by 5–8 weeks, and parallelism, if ever started, costs 9–15.5 weeks more (a net loss of 1–10 weeks) | — | 0 |
| 8 | JIT frame model and tiers | Proposed | **Fragments are the only baseline**; every tier publishes tagged values at every suspension point; native call/ret only as a separate design if [K11](#k11) fires ([§11.2](#112-jit-frames-by-tier-the-tier-contract)) | Native call/ret needs native-stack abandonment at every `Transfer`, a depth cap and its own gates; native maps need a walker and deoptimization at capture | V8 Sparkplug; SpiderMonkey Baseline | 4d (frame header); 6 |
| 9 | Async interrupts in JIT code | Proposed | **Yes:** free at calls, 2 instructions at back-edges, posted through a per-heap `InterruptHandle` that the REPL's SIGINT handler uses ([§12](#12-safepoints-and-polling)) | Back-edge polls go (0.3–1.9% saved in tight loops) and Ctrl-C fails in tight loops | V8 `StackGuard`; OCaml `young_limit` | 3 (`InterruptHandle`); 6 |
| 10 | Identity hashing | Proposed | **Side-metadata `HASHED` plus the BFG extension on move; heap-base-relative; a symbol hashes by its name** ([§9.8](#98-identity-hash)) | Pin-on-hash blocks evacuation on eq-table heaps; GC-rehashed native tables mean rewriting SRFI 69/125 in Rust | JDK compact headers (JEPs 450, 519) | 5c |
| 11, SD2 | Weakness scope and the symbol table | Proposed | **SRFI 124 and 254 ephemerons now; SRFI 125 weak tables over ephemerons; guardians and transport cells after stage 7; a weak symbol table hashed by name** ([§9.5](#95-weak-references-and-ephemerons-with-no-on-rescans), [§17.2](#172-the-memory-contract)) | A strong symbol table, as in chibi and Gauche, keeps 32–48 B per symbol for ever, and the symbol rows of the steady-state lane become class D | Chez's weak oblist (flat at 49.4 MiB over 1 M symbols [P, Chez]) | 5a; 5c |
| 12 | Port finalization | Decided 2026-10-01 | **Flush and close at GC; `EMFILE` collect-and-retry where collection is possible; GC timing declared observable** ([§9.6](#96-finalization-and-ports-f1f7)) | Without it F2 and F3 stay divergent from chibi and Gauche, and descriptor exhaustion stays ([#607]) | chibi; Gauche | — |
| 13 | Embedding API | Proposed | **`Owned` and environment handles, branded `with`, `Interpreter::call`, branded host primitives with a declared helper class (`Leaf` or `Transfer`, never `NoAlloc`), `interrupt_handle()`, mandatory teardown, many heaps per process, `Backend` migrated one step per stage, memory hooks, and an optional `MemoryBudget` shared by several heaps** ([§11.5](#115-embedding-api), [§17.3](#173-limits)) | Scoped-only handles block long-lived host references; without `call` a host must evaluate source text to call a procedure; without a declared class the JIT treats every host primitive as `Transfer`; a trusted host `NoAlloc` lets safe Rust that allocates corrupt the heap under JIT code (an `unsafe` host `NoAlloc` declaration, or a context type with no allocation methods, would put that obligation on the embedder or on the type checker, and would save the write-back of `ap` and the reload of `ap` and `alloc_limit` around each host-primitive call [I]); without teardown every dropped interpreter leaks a reservation | Wasmtime and V8 handles | 2 (handles, teardown); 3 (the rest); 5e (`notify_idle`); 5g (near-limit and memory-pressure hooks, the shared budget) |
| 14 | Dependencies; MMTk | Decided 2026-10-01, by the research the owner delegated it to: MMTk not adopted | **`patina-gc` depends on `libc` only**; loom models and any other new dependency wait for an explicit approval ([§14](#14-pluggability-contract)) | An MMTk spike is 4–6 weeks, needs crate downloads, lives outside the default workspace, and MMTk's single instance per process conflicts with many heaps per process | in-tree collectors (Chez, OCaml) | 5a |
| 15 | Footprint and limits: scope | Decided 2026-10-01 | **Limits are part of the effort, organised around steady state** ([§17](#17-steady-state-the-memory-contract-and-limits)) | — | — | — |
| 15 | Footprint and limits: values | Proposed | **Whole-heap interval `max(8 MiB, 2·L)`, L being the live bytes after the last major; √L only for generational majors after an A/B; decommit with 2-major hysteresis; `max_heap` = min(16 GiB, 75%·B), its reservation from the heap ceiling; heaps given one `MemoryBudget` drawing these from its total; exhaustion raising `&heap-exhausted` or `&stack-exhausted`** ([§7](#7-heap-organisation), [§15](#15-heap-sizing-pacing-and-observability), [§17.3](#173-limits)). SD4–SD9 set the rest | Today only RAM limits a program, so these defaults are new limits; without a shared budget, N heaps or isolates in one process each default to 75%·B, and the process can be killed before any of them raises `&heap-exhausted`; larger ones cost VA per heap (318 heaps × 16 GiB is already about 5 TiB [P]), and none at all would need chained reservations ([K13](#k13)'s fallback, one more load in the barrier). √L without a nursery costs 1.15–2× more full marking and a 32 MiB first major [P] | .NET (75% of the container limit); Chez `heap-reserve-ratio` (decommit) | 4d; 5e; 5g |
| 16 | Front-end sequencing | Proposed | **Unread provenance stores deleted in stage 1; identifiers as ids and inline provenance in stage 4c, before the new heap; lazy scope propagation as a parallel front-end project** ([§19](#19-stages)) | Later: libload keeps 316 MiB of provenance [P], and syntax objects keep `Drop` | — | 1 |
| 17 | syntax-case timing | Proposed | **After stage 5**, with an `ExpansionContext` root provider (a literal pool and epoch-checked memos) that retires the last `NoGcScope` at point C; sooner if [K16](#k16) fires there ([§11.3](#113-rust-code-the-soundness-boundary)) | Earlier: expansion needs rooting before the funnel exists | — | after 5 |
| 18 | Continuation end state | Proposed | **Design A** at stage 4e; C′ only if post-JIT capture-at-depth benchmarks demand it ([K15](#k15)) ([§13](#13-continuations-and-stacks)) | C′ earlier: O(1) capture sooner, at medium-high risk to the matrix | Larceny and Gambit (A); Chez (C′) | 4e |
| 19 | Recording divergences | Proposed | **Record now the rebinding cases of decision 3 under R ([#603]); then each semantic change, in its own PR, with its oracle-scored rows** ([§19](#19-stages)'s rules) | — | — | 0 |
| 20 | JIT code memory | Proposed | **Patina's own `MAP_JIT` reservation, W^X toggled only in `install_code`, replaced bodies freed by epoch, per-unit freeing through slabs under a cap** ([§9.7](#97-code-liveness-including-the-eager-release-contract), [§17.2](#172-the-memory-contract)); stage 6 freezes only the interface | `cranelift-jit`'s `JITModule` cannot free single functions, so long REPL sessions would grow | — | 6 (interface); the JIT track's first merged stage (allocator and cap) |
| 21 | Budget order | Proposed | **Representation first** (stages 1–5), algorithms second (7–8) ([§4](#4-thesis-and-key-bets)) | Collector-first builds an algorithm on 72 B slots and `Rc` payloads that must be rewritten anyway | — | before 1 |
| 22 | Home of the design and the plan | Decided 2026-10-01 | **One PRD file, this one**, superseding the archived stage-5 PRD; `docs/GC_DESIGN.md` stays the as-built description; the research record is `PRD/study/gc/`; work-item detail lives in the stage issues ([§22](#22-work-items)) | — | — | — |
| 23 | Threads blocked for ever | Proposed | **Rooted until they terminate**, through the scheduler's root provider, and scored by hand against Gambit and chibi on the `blocked-threads` probe (100 K threads blocked for ever on fresh mutexes nothing else reaches) ([§18.1](#181-the-m1-design-and-its-n-ready-interfaces)) | Collecting such a thread frees its stack and table entry through `FinalKind::Thread`, but diverges from both oracles (observable through Gambit's `thread-group->thread-list`, and through memory under SRFI 18 alone) and needs a `DIVERGENCES.tsv` row against each | Gambit (thread groups); chibi (its list of paused threads) | 9 |
| 24 | Pluggability | Proposed | **One production collector, `MarkRegion`, behind `Collector<M>` and `GcAttrs`; `NullGc` only in the conformance suite; no startup-selectable collectors** ([§14](#14-pluggability-contract)) | A HotSpot-style catalogue needs per-collector emitter hooks in every tier and a test matrix per collector; HotSpot itself is retiring modes (JEPs 474, 490) | HotSpot's GC interface (JEP 304), without its catalogue | 5a |
| 25 | A heap's policy at run time | Proposed | **Fixed for the heap's life:** its policy bits and `GcAttrs` come from `HeapConfig` at creation; a measured gate changes the default for new heaps; `install_code` refuses a body compiled against other attributes ([§9](#9-collection), [§14](#14-pluggability-contract)) | Run-time adaptivity: a flip becomes a posted `POLICY` event that invalidates every installed body (§13) and runs a major arming every pointerful granule | HotSpot fixes its barrier set when the VM starts | 6 |
| SD1 | SS1–SS5 as acceptance criteria | Proposed | **Yes, on deterministic readings**, phased in from stage 0 as [§19](#19-stages) lists; wall-clock pauses, MMU and RSS stay nightly signals ([§17.1](#171-definition)) | Guideline only | — | 0 |
| SD3 | Run-time cells, records, symbols | Proposed | **Mortal** (M1, [§17.2](#172-the-memory-contract)) | Immortal: about 8 KiB leaked per `environment` call, and pauses grow | — | 4b |
| SD4 | What `max_heap` counts | Proposed | **External bytes included**; under a shared `MemoryBudget`, every member heap's bytes count against its total ([§17.3](#173-limits)) | Heap only (HotSpot, .NET, V8): Rust-table leaks get past the limit | — | 5e |
| SD5 | Stacks | Proposed | **The main stack separate, default min(8 GiB, 25%·B); green-thread pages charged** ([§11.1](#111-vm-frames-and-the-stack-watermark), [§17.3](#173-limits)) | One budget for all (Gambit, Erlang, Go) | JVM, V8, OCaml | 4d |
| SD6 | Progress guard | Proposed | **Counts majors paced at a `max_heap` or shared-budget clamp, and bytes** ([§17.3](#173-limits)) | Time-based (HotSpot, V8, Go): not deterministic | HotSpot's overhead limit, without its time term | 5g |
| SD7 | Idle trim | Proposed | **On: at most one idle major per paced cycle, with back-off; only prompts and `notify_idle` in deterministic runs** ([§17.1](#171-definition)) | Off (G1 before JEP 346) or a timer (ZGC 300 s, V8 8 s) | G1 periodic collection (JEP 346), counted in waits rather than time | 5e |
| SD8 | Soft target | Proposed | **Opt-in, in bytes** ([§17.3](#173-limits)) | `GOMEMLIMIT` with a CPU limiter | HotSpot's `SoftMaxHeapSize` | 5g |
| SD9 | Exit status on exhaustion | Proposed | **The status of any uncaught error** ([§17.3](#173-limits)) | A distinct status (Go 2, HotSpot 3, V8 134) | — | 4d |

**Part I, the design contract (§3–§18).** Every stage builds toward these sections; a stage that needs to change
them changes this text in its PR.

## 3. Prior art: what we take from whom

The last column condenses DESIGN Appendix G, which gives the detail and the source of each rejection.

| Decision | Taken from | Primary sources | Not taken, and why |
|---|---|---|---|
| Allocation never collects; collection only at polls; Rust holds raw values between polls | Chez Scheme; gc-arena's "mutation XOR collection" | Chez `c/alloc.c`, `s/library.ss:1194-1240`; https://github.com/kyren/gc-arena | Allocation that may collect, with handles everywhere (SpiderMonkey's exact rooting, V8 `HandleScope`, Racket BC `xform`): a rooting obligation on ~1,300 Rust call sites |
| Raw tagged addresses, tag folded into the displacement; 16 B alignment; headerless pair, procedure (code first) and record (type first) | Chez (`TYPE(x,t)`, closure code word) | Chez `s/cmacros.ss:478-481,822-829`, `c/types.h` | Today's per-arena `u32` indices; V8's 32-bit compressed fields and NaN-boxing, both of which lose 61-bit fixnums |
| Self-tagged flonums, judged with inline flonum operations in both arms | Melançon, Serrano, Feeley, OOPSLA 2025 (Gambit) | https://arxiv.org/abs/2411.16544 | Boxed flonums (19.8% of allocations [P]); NaN-boxing; judging by interpreter wall time, which generic numeric dispatch dominates |
| 16 B granules with a side metadata byte; mark-region allocation; lazy sweep that reads only metadata | Whippet nofl/mmc; Immix; RC Immix | https://github.com/wingo/whippet (`src/nofl-space.h`, `doc/collector-mmc.md`); https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf; https://www.steveblackburn.org/pubs/papers/rcix-oopsla-2013.pdf | Chez's 19 spaces × 8 generations and its 3-level radix lookup; size-class free lists (OCaml 5), +7% retired instructions in RC Immix |
| Opportunistic evacuation with a free-block reserve | Immix; Whippet `mmc.c`; Chez/Racket CS mark-in-place; Wingo's heap-growth livelock | Whippet `src/mmc.c:656-715`; Chez `c/gc.c:35-110,1010-1036`; https://wingolog.org/archives/2025/05/22/whippet-lab-notebook-guile-heuristics-and-heap-growth | Full copying (2× space); never moving (Wingo's fragmentation livelock has no other cure) |
| Sticky mark bits for generations, behind a measured switch | Demers et al., POPL 1990; MMTk StickyImmix (the Ruby and Julia defaults); Whippet mmc; JSC's eden collections. Weighed: Wingo's losses on nboyer and splay and Go's rejection of generational barriers, against HotSpot, where every collector family went generational (ZGC, JEP 439, its non-generational mode removed by JEP 490; Shenandoah, JEP 521) | https://wingolog.org/archives/2025/02/09/baffled-by-generational-garbage-collection; https://go.dev/blog/ismmkeynote; https://openjdk.org/jeps/439; https://openjdk.org/jeps/490; https://openjdk.org/jeps/521; https://github.com/mmtk/mmtk-core (`src/plan/sticky/immix`); https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/ | Assuming either answer: a switch and a measurement instead ([K1](#k1)); a copying nursery first, which needs the largest set of prerequisites ([K4](#k4)'s fallback) |
| Field-logging barrier with armed-bit polarity, an inline slow path, a per-mutator store buffer | LXR; Whippet; MMTk `FieldBarrier` | https://arxiv.org/abs/2210.17175; Whippet `api/gc-barrier.h:61-91`; https://wingolog.org/archives/2024/10/03/preliminary-notes-on-a-nofl-field-logging-barrier; https://www.steveblackburn.org/pubs/papers/barrier-ismm-2012.pdf; https://openjdk.org/jeps/522 | Chez's store buffer (0.5–1.0 entries per barrier store [P]); cards (they need heap parsability and misfire under sticky marks); OCaml's ref table, which does not deduplicate (24,504 entries against 5 on nboyer [P]) |
| One GC interface consumed by every execution tier; barriers expanded late from one definition; initializing stores unbarriered | HotSpot's GC interface (JEP 304: `CollectedHeap`, and a `BarrierSet` realized per tier as `BarrierSetAssembler`, `BarrierSetC1`, `BarrierSetC2`); JEP 475; G1's `on_slowpath_allocation_exit` | https://openjdk.org/jeps/304; https://openjdk.org/jeps/475 | HotSpot's several production collectors chosen at startup, and per-collector code-generation hooks in each compiler: Patina has one JIT and one collector (decision 24); expanding barriers early (10–20% of C2 compile time, JEP 475) |
| Windows that cannot collect are confined, counted and bounded, never silently extended | HotSpot's GCLocker, whose stalls JEP 423 removed from G1 by region pinning | https://openjdk.org/jeps/423 | Silently extending a window that cannot collect (GCLocker's stalls) |
| Ephemerons resolved by key; never older than key or value; one fixpoint | Whippet `gc-ephemeron.c`; SRFI 124, SRFI 254 | https://wingolog.org/archives/2025/01/09/ephemerons-vs-generations-in-whippet; https://srfi.schemers.org/srfi-254/srfi-254.html | Round-based rescans: O(n²) ([#609]) |
| A finalization registry split by generation; Rust-only finalizers; guardians fill queues | OCaml custom blocks; Chez guardians | https://openjdk.org/jeps/421; https://wingolog.org/archives/2024/07/22/finalizers-guardians-phantom-references-et-cetera | Java-style finalizers (deprecated by JEP 421: resurrection, unpredictable latency) |
| Young and old lists in the large-object space | MMTk's treadmill (`alloc_nursery`, `collect_nursery`) | https://github.com/mmtk/mmtk-core (`src/util/treadmill.rs`) | Releasing LOS runs only at majors: a dead young object over 8 KiB would wait up to 256 minors |
| Identity-hash state in side metadata, extension word on move | Bacon, Fink, Grove (ECOOP 2002); JDK compact headers | https://openjdk.org/jeps/450 (experimental); https://openjdk.org/jeps/519 (product); https://wiki.openjdk.org/display/lilliput/Main | Pinning on first hash (it blocks evacuation on eq-table heaps); a moved-hash side table (a probe on every call); GC-rehashed native tables (Patina's tables are Scheme) |
| JIT values published as tagged values to VM frames before every non-`Leaf` call; no native stack maps | V8 Sparkplug; SpiderMonkey Baseline; the Guile 3 JIT | https://v8.dev/blog/sparkplug; Cranelift `cranelift/codegen/src/ir/user_stack_maps.rs:6-13`; https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html | Cranelift user stack maps in any tier; raw slots in published frames; conservative stack scanning (the [#423] tests forbid it) |
| Precise roots everywhere; no conservative scanning | V8 handles, SpiderMonkey `Rooted<T>` | https://webkit.org/blog/7122/introducing-riptide-webkits-retreating-wavefront-concurrent-garbage-collector/ | JSC's conservative, non-moving scan: Patina's references would be its worst case |
| Two helper classes, `Leaf` and `Transfer` | V8 Sparkplug, SpiderMonkey Baseline (runtime calls resume at bytecode resume points) | https://v8.dev/blog/sparkplug | A class that may collect but must return to the same fragment: preemption, signals and deoptimization all resume elsewhere |
| A minor-GC stack watermark as a strided return barrier | JEP 376; Loom return barriers; OCaml `Already_scanned` | https://openjdk.org/jeps/376; https://openjdk.org/jeps/444 | A compare on every `Return`; a minor forced whenever the stack grows; lowering one frame per cold path |
| Poll folded into the stack limit; owner-only reset with a re-check | V8 `StackGuard`; OCaml `young_limit`; Lin et al. on yieldpoint costs | V8 `src/execution/stack-guard.cc:38,169`; OCaml `runtime/domain.c:387-396,2022-2058`; https://doi.org/10.1145/2754169.2754187 | HotSpot's poll page, a trap Cranelift cannot express as a safepoint; today's per-instruction poll (+1.1–1.4%) |
| A deterministic tick quantum for preemption | Chez `%trap` | Chez `s/cmacros.ss:2119`, `s/library.ss:1194-1240` | Timer-driven preemption in the test lanes |
| Triggers in bytes plus external bytes; whole-heap interval 2·L; √L majors only beside a nursery; MemBalancer opt-in | Chez phantom bytes; OCaml custom memory; Racket CS; Kirisame, Shenoy, Panchekha | Racket `racket/src/cs/rumble/memory.ss:35-56,141-160`; https://arxiv.org/abs/2204.10455 | Counting objects (today); the √L rule without a nursery (1.15–2× more full marking [P]); time-based sizing in deterministic lanes |
| Decommit with hysteresis, in coalesced runs, after the pause | Chez `heap-reserve-ratio` | Chez `c/segment.c:460-484` | Decommitting to `reserve + 1·L` after every major (it re-faults the next cycle, 25–60 µs per MiB [P]); syscalls inside the pause; `MADV_FREE` alone, which leaves RSS unchanged on macOS [P] |
| Continuations as immutable traced snapshot objects (design A) | Larceny's stack-cache flush; Gambit; Chez (for C′) | Hieb, Dybvig, Bruggeman, PLDI 1990; Chez `c/schsig.c:58-150` | Segments or C′ first (medium-high risk to the matrix); Cranelift `stack_switch` (x64 only, one-shot) |
| Fast paths as data; static selection; spaces composed into plans; a null collector as lower bound | Whippet `api/gc-attrs.h`; MMTk plans; JEP 318 | https://openjdk.org/jeps/318; https://openjdk.org/jeps/474 | A catalogue of production collectors (ZGC, Shenandoah and Lua each retired a mode for its maintenance cost); `dyn` on fast paths |
| One declarative layout specification generates every traversal | Chez `s/mkgc.ss` (fused collection and accounting made "a GC with accounting about twice as fast" in Racket CS) | Chez `s/mkgc.ss:1-118`; https://github.com/cisco/ChezScheme/commit/37a515ca274e5f16510c1608ff37a9dae58ebd27; https://github.com/racket/racket/commit/282ec8125afe4ef05135e440ec72ee7d0f9d6f8d | Hand-written tracers: Patina's went exponential before deduplication (6.8 s per GC at depth 26) |
| A context that cannot collect; collection only through the driver's `&mut Heap`; a `'gc` brand; root scopes and owned handles only at boundaries | gc-arena; Wasmtime `RootScope`/`OwnedRooted`; Nova's `NoGcScope` | https://docs.wasmtime.dev; Wasmtime `crates/wasmtime/src/runtime/gc/enabled/rooting.rs:255-299,1379-1409`; https://trynova.dev/blog/garbage-collection-is-contrarian | Shadow stacks everywhere; conservative scanning of the Rust stack (Alloy); a brand without a separate collect capability, which lets values survive a nested collection |
| The mutator is the carrier, not the green thread; protocols written for N mutators; blocking calls leave the mutator | Go's per-P `mcache`; Loom carrier threads; OCaml 5 domains; Chez thread contexts | https://go.dev/src/runtime/mcache.go; https://openjdk.org/jeps/444; https://arxiv.org/abs/2004.11663; Chez `c/thread.c` (`Sdeactivate_thread`) | OS threads now; isolates as a premise of the GC |
| SRFI 18 objects on one shared heap; threads blocked for ever rooted until they terminate | SRFI 18; Gambit (thread groups hold every non-terminated thread); chibi (a global list of blocked threads) | https://srfi.schemers.org/srfi-18/srfi-18.html; Gambit `lib/_thread#.scm:1346`, `lib/_thread.scm:1649`; chibi `lib/srfi/18/threads.c:165-206` | Collecting a thread blocked for ever on unreachable objects: it diverges from Gambit and chibi (decision 23) |
| A weak symbol table; a memory budget from the container limit; an overhead-limit guard; idle uncommit | Chez (weak oblist); .NET (75% of the container limit); HotSpot's overhead limit; G1 periodic GC (JEP 346), ZGC uncommit | `PRD/study/gc/followup/steady/prior-art.md`; https://openjdk.org/jeps/346 | A strong symbol table (chibi, Gauche; decision 11); time-based overhead limits and idle timers, which are not deterministic (SD6, SD7) |
| A bespoke in-tree engine with MMTk-shaped seams | MMTk's binding decomposition | https://github.com/mmtk/mmtk-core | MMTk as the engine: one instance per process against 318 heaps in one test process [P], GC on worker threads, an untagged `ObjectReference` that forces pair headers (decision 14) |
| Patina's own code reservation with `MAP_JIT`; W^X toggled in one install function | — | Cranelift `cranelift/jit/src/memory/system.rs`, `cranelift/jit/src/backend.rs:164,206` | `cranelift-jit`'s `JITModule`: no `MAP_JIT`, and it frees only whole modules |
| No incremental or concurrent marking now; an SATB arm reserved for slow paths | Racket CS; LXR | https://arxiv.org/abs/2112.07880 | Load barriers and concurrent compaction (ZGC, Shenandoah: about 5.4% average read-barrier cost), against decision 6's throughput first; Larceny's regional collector (about 1.8× elapsed time) |
| Parallel stop-the-world marking on per-heap GC workers; evacuation destinations chosen sequentially | HotSpot Parallel and G1; MMTk work packets; Whippet's work-stealing tracer | Whippet `doc/collector-mmc.md`; https://wingolog.org/archives/2025/07/08/guile-lab-notebook-on-the-move | Chez's ownership-partitioned parallel collector (no speed-up with one mutator); racing evacuation with CAS forwarding (addresses would depend on the schedule) |

## 4. Thesis and key bets

Build first the common core every candidate design needs: a stable block heap; headers and inline payloads with no
Rust `Drop`; byte pacing; a slot-based root contract; a context type in place of `Rc<RefCell<Heap>>`, with collection
reachable only from the driver; global cells; continuations as heap objects; traced code liveness. The first
production collector is the plainest that removes the measured costs, **non-moving, whole-heap mark-region**;
generations and evacuation come later on the same space, metadata byte, barrier and JIT ABI, each a per-heap setting
fixed at the heap's creation, whose default a pre-registered measurement turns on (decision 25). The JIT-facing
contract is frozen after a throwaway Cranelift spike.

| # | Bet | If wrong |
|---|---|---|
| B1 | **Representation dominates**; a copying nursery is second-order on Patina's bimodal survival | [K9](#k9) stops after stage 5 and re-plans; [K4](#k4) builds the copying nursery |
| B2 | **Allocation never collects**, through the JIT era | [K16](#k16) bounds the windows that cannot collect and adds a collection point at the offending site; making allocation collect is not the fallback (230–890 Rust functions hold values across allocation [S]) |
| B3 | **A granule field-logging barrier costs under 1% in the interpreter and at most 2% in JIT code** | [K2](#k2): the card plus young-filter runner-up |
| B4 | **Fragmentation is containable without moving** until evacuation lands | [K3](#k3) pulls evacuation forward |
| B5 | **No tier needs Cranelift stack maps or raw slots in published frames** | [K12](#k12): more `Leaf` helpers and inlining; native maps only through a separately approved design |
| B6 | **Self-tagged flonums pay** (≥ 70% immediate on float code; fewer bytes and cycles with inline flonum operations in both arms; ≤ 1% elsewhere) | [K6](#k6) reverts to 16 B boxes before the ABI freeze |

**Candidates considered.** Five architects each argued one starting philosophy, and this design is the synthesis
(DESIGN Appendix A records the panel's findings).

| Proposal | Starting philosophy | Kept | Dropped |
|---|---|---|---|
| [`proposal-chez.md`](study/gc/design/proposal-chez.md) | Chez-faithful hybrid: a copying nursery for every type, promotion into typed old spaces marked in place, ¾-live evacuation | allocation never collects; polls where every value is in VM memory; one declarative layout specification; the ¾-live evacuation rule; per-block pinning | the copying nursery as the first collector (it is [K4](#k4)'s fallback, since queue3 and deeprec survive 100%); typed old spaces; frozen-chunk continuations first (C′ comes later, if [K15](#k15) allows) |
| [`proposal-bounded.md`](study/gc/design/proposal-bounded.md) | RegionGen: a bounded-pause generational mark-region collector, starting from OCaml 5 | Whippet-style mark-region with a metadata-only lazy sweep; armed-bit field logging with an SATB arm in the slow path; pause budgets | incremental marking as a planned stage (no latency goal, decision 6); the chunked copying nursery; a register-stack scan budget, which would force 10³–10⁴ extra minors on deeprec; a fuel counter as the poll word (kept only as Tick mode for SRFI 18) |
| [`proposal-evolve.md`](study/gc/design/proposal-evolve.md) | A non-moving mark-region heap first, with evacuation and generations added as measured switches | the plan's spine: the common core, whole-heap non-moving `MarkRegion`, sticky generations and evacuation as per-heap switches, a measured win per stage | pin-on-hash, replaced by the BFG extension |
| [`proposal-mmtk.md`](study/gc/design/proposal-mmtk.md) | MMTk-shaped, with mmtk-core as a gated engine | MMTk-shaped seams (`ObjectModel`, plans, `BumpPointer` offsets); its own conclusion against MMTk on day one | the MMTk adapter stage (decision 14) |
| [`proposal-jitfirst.md`](study/gc/design/proposal-jitfirst.md) | Representation and ABI derived from the machine code a Cranelift tier should emit | the value word with one heap bit, VM frames as the only home of values, the poll folded into the stack limit, the ABI frozen after a spike | closures whose word 0 is a machine entry (word 0 is a descriptor reference, so code liveness is marking); the interpreter calling convention redesigned with continuations before the spike; a copying nursery as the second collector |

## 5. Value encoding

*Implements decision 4 (proposed).*

`Word` (today `TaggedValue`) stays `#[repr(transparent)] u64` and `Copy`.

```
 low bits      meaning                         decode / layout
 ...xxxx 000   fixnum, 61-bit signed           n = w >> 3 (arithmetic)
 ...xxxx 001   immediate (specials, chars,     low byte = sub << 3 | 001; payload in bits 8–63
               object headers)
 ...sxxx 010   flonum, self-tagged, band A     see below; bit 3 is the sign
 ...sxxx 011   flonum, self-tagged, band B
 ...a    1xx   HEAP REFERENCE (bit 2 set)      4-bit tag k = w & 15, address = w − k (16 B aligned)
```

| `w & 15` | Kind | Word 0 of the object | Field *i* load |
|---|---|---|---|
| `0100` | pair (headerless) | car | car `[w−4]`, cdr `[w+4]` |
| `0101` | procedure (headerless) | code-descriptor reference (a `1111` value) | free variable *i* at `[w+3+8i]` |
| `0110` | vector | header | element *i* at `[w+2+8i]` |
| `0111` | record (headerless) | record-type reference | field *i* at `[w+1+8i]` |
| `1100` | string (pointer-free) | header | UTF-32 unit *i* at `[w−4+4i]` |
| `1101` | symbol (non-moving, mortal) | header | — |
| `1110` | bytevector (pointer-free) | header | byte *i* at `[w−6+i]` |
| `1111` | other headered object | header | word *k* at `[w−15+8k]` |

- **Fixnum `000`** lets a JIT add tagged words with `adds`/`b.vs`; a tagged index is a byte offset for 8-byte
  elements. **`#f` is `0x01`**, so `if` is one compare; today's specials keep their bit patterns
  (`crates/patina-core/src/tagged_value.rs:91-110` [S]).
- **One exact heap bit:** the barrier's value filter is one `tbz w, #2`, removing 64% of heap stores [P, store-mix
  census]. Seven kinds get primary tags, so their type tests need no header load (calls are 10–31% of dispatches [P]).
- **Code references are values** (`1111` references to code descriptors): code liveness is ordinary marking. The aarch64
  sequences (a checked `car` in 4 instructions, `vector-ref` in 10) are confirmed by stage 6; detail: DESIGN H.1.

**Flonums.** With `raw = f64::to_bits(d)` and `K = 1 << 60`: `encode(d) = rotate_left(raw − K, 4)`, immediate iff
`(w & 7) ∈ {0b010, 0b011}`; `decode(w) = rotate_right(w, 4) + K`. Exponent tops `011` and `100` land on tags `010` and
`011`, so every double with |d| in [2⁻²⁵⁵, 2²⁵⁷), of either sign, is immediate (Melançon, Serrano and Feeley's
three-band variant minus the ±0/subnormal band, which keeps the heap bit exact). Other doubles are 16 B boxes: ±0.0,
±∞ and the canonical NaN use **immortal canonical boxes**, so producing them never allocates. The encoding is
canonical, so `eqv?` is a bit compare for immediates. Observable changes (`eq?` on equal in-band flonums; SRFI 69 `eq?`
tables finding equal flonum keys; flonum-keyed ephemerons never breaking) are checked against the oracles in stage
5f and recorded in `DIVERGENCES.tsv` where they differ. The encoding is judged only with type-specialised flonum
arithmetic in both arms, because generic numeric dispatch is about 51% of fibfp's samples [P] ([K6](#k6)).

**Immediates (`001`).**

| Low byte | Value | Notes |
|---|---|---|
| `0x01`, `0x09`, `0x11`, `0x19`, `0x21` | `#f`, `#t`, `()`, eof, unspecified | unchanged |
| `0x29` | default-object | |
| `0x31` | `BWP`, a broken ephemeron's key and value | never reaches Scheme: `ephemeron-key` and `ephemeron-datum` answer `#f` for it (`crates/patina-tests/tests/ephemerons.rs:29-30` [S]) |
| `0x39` | `UNBOUND`, a global-cell placeholder | never reaches Scheme |
| `0x41` | char | `(cp << 8) \| 0x41`; moves here from today's tag `010` |
| `0x81` | object header | `len << 16 \| type << 8 \| 0x81`; never a value |
| `0xD9` | `DEAD_SLOT` | fill for dead register slots in the debug-poison and zeal lanes (§11.1) |
| `0xE9` | `FWD_CHECK` | debug check word written into evacuated objects |
| `0xF1` | today's `FORWARDED` import marker | deleted with global cells (stage 4b) |
| `0xF9` | `GC_POISON` | debug and torture poison (exists today) |

**Forwarding** state lives in the side metadata byte (`FORWARDED`, §7), not in object words. An evacuated object's
word 0 holds the new untagged address, and a visitor rewrites a slot as `new | (old & 15)`, identically for
headerless and headered objects. Stages 5b–5e decode a transitional tag map while objects leave the arenas.

## 6. Object model

**Header** (headered kinds only): `length / payload (48 bits) | type (8) | 0x81`, an immediate, written once by the
constructor. **No GC state and no mutable flags live in a header**: mark, end, log, hash and forwarding live in side
metadata. Immutability variants use distinct type codes (`T_VECTOR` = 1, `T_VECTOR_IMM` = 2, both below 32).

**Invariant W (value-only words).** Every word of every object the barrier can log, and every word of a captured
continuation, is a value or has low bits `000` (reads as a fixnum): headers are immediates, descriptor and record-type
references are values, constructors write padding as fixnum 0. A minor re-reads both words of a logged granule as
values (§10), so W makes granule logging sound; capture clears dead slots and no tier keeps raw slots in a published
frame (§11.2), so the core traces a continuation word by word without parsing frames (§13). A `CodeDesc`'s raw fields
are declared raw in the layout specification, never scanned, and never a barrier target.

**Layouts.** Sizes round up to 16 B; "pf" means pointer-free; "NMS" is the non-moving space (§7).

| Kind | Tag | Words | Bytes; space |
|---|---|---|---|
| pair | `0100` | car, cdr | 16 |
| procedure: VM closure | `0101` | descriptor ref, fv₀…fvₙ₋₁ | 8+8n |
| procedure: primitive | `0101` | descriptor ref (its primitive descriptor), pad | 16; immortal, with its descriptor |
| procedure: parameter | `0101` | descriptor ref (PARAM), converter, value vector (transitional shallow binding); from stage 9: descriptor ref, converter, global-value cell | 32 |
| procedure: continuation | `0101` | descriptor ref (CONT_INVOKE), `CONT` object | 16 |
| procedure: tree-walker lambda | `0101` | descriptor ref (TW_LAMBDA), host id (fixnum) | 16 |
| vector | `0110` | header(len), e₀… | 8+8n |
| record | `0111` | record-type reference, f₀…fₙ₋₁ | 8+8n |
| string | `1100` | header(len), UTF-32 units | 8+4n, pf |
| symbol | `1101` | header(len), hash (fixnum: fixed-seed hash of the UTF-8 name), UTF-8 bytes | 16+len, pf; NMS, allocated old (immortal when the binary names it) |
| bytevector | `1110` | header(len), bytes | 8+n, pf |
| flonum box | `1111` | header, f64 | 16, pf |
| bignum | `1111` | header(limbs; sign by type code), u64 limbs | 8+8n, pf |
| ratnum / complex | `1111` | header, num, den / re, im | 32 |
| cell (box, `MutableCell`) | `1111` | header, value | 16 |
| record type | `1111` | header, name, parent, uid, field names, nfields (fixnum), flags (fixnum), pointer mask | 64; NMS |
| identifier | `1111` | header(scope-set id in the length bits), symbol, source id (fixnum) | 32 |
| ephemeron | `1111` | header, key, value (reported through `SlotVisitor::ephemeron`, never as two strong slots), GC link (collector-private: fixnum 0 outside a collection, never traced) | 32 |
| promise | `1111` | header, box → `[hdr PBOX][done (fixnum)][value-or-thunk]` (SRFI 45 sharing) | 16 + 32 |
| values | `1111` | header(n), v₀… | 8+8n |
| condition | `1111` | header(kind), message, irritants, extra | 32 |
| port | `1111` | header, port id (fixnum) | 16 |
| host handle | `1111` | header(kind), host id (fixnum), optional literal vector | 16–32 |
| environment specifier | `1111` | header, namespace id, flags | 32 |
| thread (SRFI 18) | `1111` (`T_THREAD`) | header, thread id and state (fixnum: new, runnable, blocked, terminated; id −1 when no `GreenThread` exists), name, specific, thunk (until started) or result, end exception, joiners (a waiter queue) | 64 |
| mutex | `1111` (`T_MUTEX`) | header, name, specific, owner (a thread, or a state immediate: not owned, abandoned, not abandoned), waiters | 48 |
| condition variable | `1111` (`T_CONDVAR`) | header, name, specific, waiters | 32 |
| time (SRFI 18) | `1111` | header, f64 seconds | 16, pf |
| continuation (design A) | `1111` | header(words), meta (fixnums), dynamic-state references, frame words | variable; LOS above 8 KiB |
| code descriptor | `1111` (`T_CODE`) | see below | 48 + 8·nconst; NMS (primitive descriptors: immortal) |
| global cell (never a value) | — | header, value, name symbol, namespace id + flags (fixnum; includes `WATCHED`) | 32; NMS, allocated old |
| binding record (variant R only; never a value) | — | header, cell reference, name symbol, flags (fixnum; includes `WATCHED`) | 32; NMS, allocated old |

Waiter queues are heap lists, written through the store funnel; a thread object's `GreenThread` (register stack,
frames) is Rust-owned in the per-heap thread table (§18).

**Code descriptor** (`T_CODE`, a heap object referenced by ordinary values): `w0` header (nconst); `w1` entry (raw:
JIT body or interpreter trampoline; the tier-up target); `w2` resume (raw: `*const ResumeTable`, return pc → resume
entry); `w3` body (raw: `*const CodeBody` with bytecode and safepoint maps, owned by the per-heap `CodeStore`); `w4`
meta (fixnum: nfree, nregs, arity, kind); `w5` unit (fixnum: unit id, generation); `w6…` constants (values, traced
and updatable; nested descriptors appear here as ordinary references). JIT code loads constant *k* with one load
through the descriptor reference it holds. Descriptors are born old, written only by initializing stores, and pushed
on the remember-whole list (§10); inline caches live outside them, in the `CodeStore` (§11.2).

**No heap object owns a Rust value** (today 19 of 28 variants carry `Drop`, and 47% of allocations [P]). Rust-owned
resources live in per-heap side tables reached by a `u32` id (`PortTable`, `CodeStore`, the thread table,
`HostPayloadTable` for tree-walker closures and continuations, macro bodies, libraries, namespaces of environment
specifiers and FFI objects), each entry registered in the finalization registry (§9.6). Bignums keep inline limbs. A
generated `const` assertion fails the build if any laid-out type needs `Drop`.

**One layout specification.** `declare_layouts!` (defined in `patina-gc`, invoked in `patina-core`) generates the
`ObjectModel` implementation (`size`, `trace`, `copy`, `verify`), the debug printer, the datum writer's kind view and
the JIT offset table, as Chez's `mkgc.ss` does, so misfiling a value-bearing variant as a leaf becomes a compile error
rather than today's use-after-free (`crates/patina-core/src/heap/mod.rs:137-141` [S]). The Rust structures that stay
off the heap get the same guarantee from the `Trace` derive (§14).

## 7. Heap organisation

**One virtual-address reservation per heap**, sized from the **heap ceiling** (`HeapConfig::heap_ceiling`, by default
max(`max_heap`, the default `max_heap`), fitted under `RLIMIT_AS`; data plus metadata and block table, rounded to a 4
MiB run). `max_heap` defaults to min(16 GiB, 75%·B), where B is the smaller of physical RAM and the cgroup limit, and
counts committed data, LOS runs, metadata and external bytes (SD4); it is set by `PATINA_HEAP_MAX`, `--heap-max` or
`HeapConfig::max_heap`, and reaching it follows §17.3's steps. The reservation is mapped read-write with lazy commit
(`MAP_NORESERVE` on Linux), one VMA per heap, so `vm.max_map_count` is never at risk; 4,096 reservations of 16 GiB
succeed on the development machine, and one test process holds up to 318 live heaps [P]. `Drop for Heap` unmaps it,
which needs stage 2's teardown fix ([#604]). Memory returned is measured as resident size and `phys_footprint`.

```
reservation:  [ metadata: 1 byte per 16 B granule ][ block table: 16 B per block ][ data: blocks + LOS page runs ]
meta_bias = meta_base − (data_base >> 4);   meta(a) = *(meta_bias + (a >> 4))        // one shift, one load
separate:     per-mutator store buffer (256 MiB VA, lazily committed, guard page at the hard end);
              green-thread register stacks (main: min(8 GiB, 25%·B); others 256 MiB; §11.1, §18);
              per-heap code reservation (MAP_JIT on macOS arm64)
```

**Spaces**, composed in the style of MMTk plans:

| Space | Holds | Allocation | Collection | Moves? |
|---|---|---|---|---|
| Small-object space (SOS) | objects up to 8 KiB | bump into holes; objects over 256 B through an overflow allocator that uses empty blocks only | mark-region; lazy sweep | no (stages 5–7); opportunistic evacuation from stage 8 |
| Large-object space (LOS) | objects over 8 KiB | 16 KiB page runs, first fit by size class; a new run joins the **young list** | young runs classified after every collection, old runs after majors; dead runs go to a recycle cache after the pause | never; buffers lent to FFI live here |
| Non-moving space (NMS) | code descriptors and record types (the former descriptor space); symbols, global cells and binding records; a future boot image's mutable objects (the immortal row) | `alloc_old` into dedicated blocks, bump into holes | mark-region with hole reuse, swept at majors only; code units released by the registry; the interner and namespaces pruned in epilogue step 6; reported by [K3](#k3) | never, not even under evacuation; JIT code may embed these addresses under §11.2's fourth case |
| Immortal space | only what the binary and program text bound (M1): primitive procedures and their descriptors, canonical flonum boxes, core syntax, a future boot image's immutable objects that reference nothing mortal (the image's cells, binding records, parameters, promise boxes and tables, which a program can mutate, and the objects that reference them are allocated old in the NMS through `alloc_old` and the remember-whole list) | append-only blocks with state `IMMORTAL` | never swept and never scanned: whatever it references is immortal too (a symbol that core syntax or a primitive names is interned here at startup) | never; JIT code may embed these addresses |

Granules are 16 B (56% of objects are exactly 16 B [P]); blocks 32 KiB; the metadata byte is the line table, at
granule size; the medium threshold is 256 B and the LOS threshold 8 KiB. The other parameters (recyclable threshold,
free and evacuation reserves, initial commit, decommit unit) and the 16 B block-table entry: DESIGN H.3.

**The metadata byte**, one per granule (6.25% of the data region):

| Bits | Field | Where it is valid | Writer |
|---|---|---|---|
| 0–2 | `STATE`: 0 = free or young-unmarked; 1, 2, 3 = rotating mark epochs; 4 = `FORWARDED`; 5–7 reserved (5 = `BUSY` for future parallel evacuation) | start granule | collector |
| 3 | `END`: last granule of a marked object | last granule | collector (marker) |
| 4 | `KEYHINT`: an ephemeron is pending on this key | start granule | collector |
| 5 | `HASHED`: an identity hash was taken | start granule | mutator, through `MetaByte` |
| 6 | `LOG`: armed; the first store into this granule must be logged | every granule | the collector arms it; the mutator barrier disarms it through `MetaByte` |
| 7 | `HASH_MOVED`: the copy carries a trailing extension granule holding the original hash | start granule | collector |

**Pinning is per block**, as in Chez's `must_mark`: a block with a non-zero pin count, or pinned this cycle, is never
an evacuation candidate. Identity hashing never pins (§9.8), so pins are rare (FFI buffers, mostly in the LOS, and
debugger event payloads).

**Decommit** has hysteresis and never runs inside the pause. A block becomes a candidate after 2 consecutive majors
empty; the heap keeps at least `(target − L) + free_reserve` of committed empty capacity. Whole 4 MiB-aligned runs are
decommitted data and metadata together; empty blocks inside runs that still hold a survivor, one by one, data pages
only. The epilogue queues the work and the poll slow path does it after the world is released, rate-limited and logged
as a non-mutator interval (an idle major completes it before its wait, §17.1). Linux uses `MADV_DONTNEED`; macOS
`mmap(MAP_FIXED)` over the range, because `MADV_FREE` alone leaves resident size unchanged there [P]. LOS runs pass
through a recycle cache of up to 32 MiB. Register stacks and store buffers follow the same rule, observed at majors and
at returns to the top level.

## 8. Allocation

**The contract stays: allocation never collects.** Today `note_alloc` only raises a flag
(`crates/patina-core/src/heap/mod.rs:581-584` [S]), and 230–890 Rust functions hold unrooted values across allocation
and are correct only because of this. Allocation sites are never safepoints: JIT code publishes and reloads nothing
around them, and initializing stores need no barrier (JEP 475's condition). The slow path refills, overdrafts, posts
events and fails only at hard limits; it never collects and never runs Scheme.

**Fast paths.** In Rust, `alloc_small(bytes)` (a multiple of 16, at most 256) bumps `Mutator.ap` against `alloc_limit`
and otherwise calls the cold `alloc_slow`; the receiver is `&mut Cx` while the `Vec` arenas can relocate, `&Cx` once
they are gone (5e). Constructors write **every word**, padding as fixnum 0; hole data is never zeroed, only a hole's
metadata bytes (debug builds fill holes with `GC_POISON`). In JIT code `ap` and `limit` (`Mutator` offsets 0 and 8,
MMTk's `BumpPointer` offsets) stay in SSA; one `add; cmp; b.hi` covers a basic block's summed allocation (OCaml's
Comballoc); tags are applied with `add`, never `orr`; the refill is a `Leaf` helper. Only `NoAlloc` helpers keep
`ap`/`limit` in SSA across a call, which a call-graph test and a debug assertion enforce. Only the runtime's own helpers
can be `NoAlloc`, because the call-graph test sees their bodies; a host primitive never is (§11.5). A conformance test
checks that the emitter writes every word of every inline-allocated kind (listing: DESIGN H.1).

**Slow path** `alloc_slow(m, bytes, kind)`, in order:

| Step | Rule |
|---|---|
| 1. Over 8 KiB | the LOS; the run joins the young list. User-sized requests go through `try_alloc` (step 6) |
| 2. Over 256 B | the overflow allocator, empty blocks only |
| 3. Otherwise, lazy-sweep the current block | from its cursor: a granule whose `STATE` is the current epoch starts a live object (skip to its `END`); any other run is a hole, whose metadata is zeroed and which becomes `(ap, limit)` |
| 4. Then | the next recyclable block, a free block, or newly committed blocks; a block whose `counted_epoch` is stale holds nothing live, and its metadata is cleared as it is taken, not in the pause |
| 5. Accounting at refill | `bytes_since_gc += hole_bytes`; crossing the nursery budget or the major target makes an owner post of `GC_MINOR` or `GC_MAJOR` (§12) and allocation continues; [K16](#k16) bounds the windows in which a posted collection cannot run |
| 6. **User-sized requests are fallible, and retried after a collection** | `make-vector`, `make-string`, `make-bytevector`, string-port growth and `read-string` call `try_alloc(bytes) -> Result<_, Oom>` before any visible change. On `Oom` the machine treats the failure as `Step::Collect` (the mechanism `EMFILE` uses, §9.6): a full major at the call's return pc, where the arguments are still in the suspended frame, then one more call; only a second `Oom` raises `&heap-exhausted`. A request larger than `max_heap` − reserve − the uncollectable floor raises at once (§17.3). These primitives stay `Leaf` in JIT code, whose `Transfer` path for the status does the same; where collection is deferred (`NoGcScope`, a nested tree-walker trampoline) the first `Oom` raises. **Capture** (§13) does the same through `try_alloc` and never draws on the emergency reserve |
| 7. Hard ceiling `max_heap` | a small infallible allocation dips into a 4 MiB emergency reserve and posts `HEAP_EXHAUSTED`; the next poll follows §17.3's steps (a maximal major, then a catchable `&heap-exhausted` if still over; uncaught, a non-zero exit). If the reserve runs out before a poll, the process flushes ports (F1) and aborts with a diagnostic and a non-zero exit |

## 9. Collection

All collection is stop-the-world and deterministic: single-threaded in a fixed trace order, or, under stage P, with GC
workers whose marks, broken ephemerons and addresses do not depend on the schedule. It runs only at a poll (§12), with
every Scheme value in VM-managed memory, and only from code holding the driver's `&mut Heap` (§11.3). **Per-heap
policy** `{ generational, evacuation }` is a runtime field read only by slow paths: VM heaps default to `{false, false}`
and a measured gate changes the default for new heaps, never a running heap's bits; tree-walker heaps stay
`{false, false}`. **Policy is fixed for a heap's life** (decision 25): the bits and the heap's `GcAttrs` (§14) come from
`HeapConfig` when the heap is created, because JIT code compiles the barrier kind in (§10); the adaptive bypass (§9.2)
and the fragmentation trigger (§9.4) choose which collections run, not the attributes. (Run-time adaptivity, if ever
wanted, would be a posted `POLICY` event serviced at a poll: it invalidates every installed body through §13's path,
then runs a major that arms every pointerful granule.) Step-by-step algorithm: DESIGN H.2.

### 9.1 Major collection

Retire every mutator's allocation buffer; **flip** the mark epoch (1 → 2 → 3 → 1, so no clearing pass); **drop** every
mutator's store buffer, range entries and remember-whole list; trace roots (pinning roots first; every rooted thread's
frames, ignoring watermarks, and dynamic-state slots; root scopes; handles; the global and library namespaces, which
reach their binding records and cells); mark (`STATE` set to the epoch, keeping `KEYHINT | HASHED | HASH_MOVED`; `END`
set; in generational mode `LOG` armed on every granule of a pointerful object; live granules counted per block under a
`counted_epoch` stamp); run the weak fixpoint and the epilogue (§9.5, §9.9). Cells are ordinary marked objects, not a
root region, so pause work does not grow with cell history, and a generational major re-derives every `LOG` bit by
marking. The **catch-up sweep** of blocks whose `swept_epoch` lags (so a stale epoch cannot alias), the NMS sweep and
**block classification** (empty, recyclable, full; decommit candidates; dead LOS runs) run after the world is
released, in slices from the poll slow path.

### 9.2 Minor collection (sticky marks; generational heaps only)

"Old" means the start byte holds the current epoch, "young" `STATE` 0; minors do not flip the epoch. **Roots:** the
frames above the watermark of each thread with `ran_since_gc` (§11.1); every rooted thread's dynamic-state slots;
root scopes and handles; the remember-whole lists and range entries; young host-payload registrations; machine roots.
Each store-buffer entry is a granule whose two words are re-read as values (invariant W), their young referents marked
and the granule re-armed; range entries are scanned whole and re-armed. **Marking is promotion**: a survivor becomes
old in place, so promotion cannot fail. Weak processing covers young entries only. **Young LOS runs:** marked ones move
to the old list, unmarked ones are released into the recycle cache after the pause and counted in `bytes-reclaimed`
(MMTk's treadmill), so a dead young capture, large vector or string-port buffer does not wait for a major. Lazy sweep
then re-sweeps only blocks allocated into since the last GC, and each thread's watermark is reset.

Sticky loses where a copying nursery pays nothing for dead young objects ([K3](#k3), [K4](#k4)); the pre-planned fix is
a `NurserySpace` (§14). An **adaptive bypass** with hysteresis runs majors only while minors keep more than 30% of the
budget alive, and **backstops** force a major after 256 consecutive minors or max(1 GiB, 64·L) of nursery allocation,
which bounds how long ports, code and dead-key ephemerons wait.

### 9.3 Marking order, lazy sweep

A pair pushes its cdr and continues with its car, so the mark stack is bounded by car-nesting depth; large objects push
range entries processed 256 words per pop; past a cap the marker sets `RESCAN` on the blocks of unscanned objects and
later rescans them by start bytes, so no heap parsability is needed. **Lazy sweep** runs per block, on demand (§8 step
3), at most once per cycle; it reads only metadata, writes only the holes it returns, never reads a dead object and
never runs a destructor. The debug-poison and zeal lanes add an eager poisoning sweep and a quarantine (§16).

### 9.4 Evacuation and pinning (stage 8)

Evacuation starts at a major when the previous cycle's **fragmentation** (free granules not in holes of at least 256 B,
over committed SOS granules, measured after marking; [K3](#k3)'s metric) exceeds 10%, and stops below 5% (Whippet
`mmc.c:656-715`). A **footprint trigger** also evacuates the sparsest unpinned blocks whenever footprint − F(L)
exceeds max(64 MiB, 25%·F(L)) at 2 consecutive majors (§17.1). Candidates are SOS blocks under ¾ live with no pin
(Chez/Racket CS's `use_marks` rule); pinning roots are traced first. A reachable object in a candidate is copied once:
its start byte becomes `FORWARDED`, its word 0 the new address, and a `HASHED` object gets its extension granule
(§9.8); when the reserve runs out, the rest are marked in place. The LOS, the NMS, the immortal space, pinned blocks and
tree-walker heaps never move. The `move-all` torture mode makes every unpinned block a candidate and poisons
from-space.

### 9.5 Weak references and ephemerons, with no O(n²) rescans

*Implements decision 11 (proposed).*

| Rule | Content |
|---|---|
| Key liveness | `is_live(K)` holds when K is an immediate, an immortal object, already marked, or, in a minor, old (NMS objects included). **Symbol keys:** an ephemeron keeps a symbol key alive, so symbol-keyed ephemerons never break and symbol-keyed weak tables answer as today (where symbols are re-marked at every collection, `crates/patina-core/src/heap/gc.rs:449-465` [S]), although the symbol table is weak (SD2) |
| Key-indexed resolution (Whippet `gc-ephemeron.c`) | the generated trace reports an ephemeron only through `SlotVisitor::ephemeron` (§14). One whose key is not live is chained onto `pending[K]` through its link word and `KEYHINT` is set on K; marking (or copying) an object with `KEYHINT` pops the waiters and traces their values, so the work is linear in either order (today quadratic, [#609]) |
| **An ephemeron is never older than its key or value** | ephemerons are allocated only young, never in the LOS, NMS or immortal space, and promotion is age-monotone, so a minor examines only young ephemerons and treats old and immediate keys as live; no remembered-set edge exists for ephemerons |
| **`(gc)` is always a full major and collects at its call** | a `Transfer` primitive returning `Step::Collect(Major)`, serviced before the caller's next instruction with the caller suspended at a mapped return pc (§12). Three [#423] tests need this once the per-instruction poll goes (`crates/patina-tests/tests/ephemerons.rs:94,109,127` [S]), and the same file requires a dead key of any age to break after one `(gc)` (`ephemerons.rs:23,52,213,226` [S]). Inside `NoGcScope`, `(gc)` posts and counts a deferred poll |
| Breaking; one fixpoint | breaking writes `BWP` into key and value (§5). **One fixpoint** covers host payloads, ephemerons and, later, SRFI 254 guardians: sequencing weak kinds separately was a use-after-free (commit `1d18c49`). `reference-barrier` stays an opaque use in JIT code (a `Leaf`, `NoAlloc` identity helper) |

### 9.6 Finalization and ports (F1–F7)

**Registry** (part of the contract, §14): `FinalRegistry { young, old }` of entries
`{ obj, kind: Port | CodeUnit | HostPayload | Thread | Foreign, id }`, created only by slow-path constructors. After a
minor, a young entry whose object was marked or forwarded moves to `old`, and one **neither forwarded nor marked** is
queued, which is correct under sticky marking, in-place promotion and copying alike; after a major, unmarked old
entries are queued too. A runtime may also queue an entry it knows is finished (a terminated thread, §18). Queued
entries run in id order **after the pause and before Scheme resumes**, in the poll slow path, Rust-only (no
allocation, no Scheme): ports flush, close and free their slot, `MemoryFs` writers commit, threads release their
stacks, namespaces of dead environment specifiers go.

| Rule | Content |
|---|---|
| F1 | `end_process` and `emergency-exit` flush every open file port of every heap in the process, through a process-wide weak registry of `PortTable`s that prunes dropped heaps (today's `OUTPUT_FILES` covers every heap on the thread, `crates/patina-core/src/port.rs:173,212-233` [S]) |
| F2 | a collection that proves a file port dead flushes and closes it before Scheme resumes |
| F3 | each open, unclosed file port charges 8 KiB of external bytes; opens minus closes since the last major reaching min(128, `RLIMIT_NOFILE`/4) posts a major; on `EMFILE`/`ENFILE` the open returns `Step::Collect` with the file name as its state and opens once more when resumed, as chibi does (`eval.c:1300-1323`); `load` reads again the same way ([#607]). The loader's reads retry once points A and B can collect (stage 2); until then they, `include`'s and opens where collection is deferred raise on the first `EMFILE`, a documented limit (`docs/GC_DESIGN.md` §6) |
| F4 | finalization runs no Scheme and allocates nothing |
| F5 | dropping the heap finalizes every entry ([#604]) |
| F6 | a port is one canonical object, and the `current-*-port` parameters hold it ([#608], [#618]) |
| F7 | a `Foreign` finalizer never blocks: one that may (`sqlite3_close`, say) is handed to the blocking-I/O helper pool (the SRFI 18 row of §19) or runs in a safe region (§12), never in the poll slow path, where it would stall every green thread. F7 binds from stage 9; until then no green threads exist, so a finalizer that may block runs in the poll slow path and stalls only the program's one thread |

GC-time flushing is observable, so the tests that pin F1–F6 live outside the byte-identical differential lane.

### 9.7 Code liveness, including the eager-release contract

Code is released per unit (a top-level form and the lambdas compiled with it). Edges into a unit are ordinary
references to its descriptors (procedure word 0, frame `code` words in live stacks and captured continuations, nested
descriptors in constants), so a unit is live if any of its descriptors is marked; `live_closures`,
`gc_freed_closure_code_ids` and `RETIRED_VM_CLOSURE_CODE` go. Until stage 4e, `Rc` counts keep units loaded. The
`CodeStore` references descriptors weakly.

**The eager-release contract ([#338]/[#352]).** `crates/patina-tests/tests/finished_forms_release_code.rs:72-99` [S]
runs 2,000 forms **with no collection** and requires each form's code gone as soon as the form finishes. So each unit
carries an **`escaped` bit**, set whenever a reference to one of its descriptors can outlive the form: `MakeClosure` of
a member; a capture over a frame of the unit; a `Step::Eval` closure; any other holder (debugger hook, profiler sample,
tracer snapshot, JIT inline cache), which obtains descriptor references only through `CodeStore::escape`. A finished
form whose unit never escaped is released at once (debug builds poison its descriptors' raw fields and assert nothing
references it); an escaped unit waits for a complete major. Inline caches hold traced descriptor references (strong, or
weak and cleared at epilogue step 5; stored as §11.2 says), so a reused address never matches a stale entry.

**JIT bodies** go with their unit and are freed by epoch: tier-up and invalidation are `Transfer` helpers, so no native
activation resumes a replaced body, which is freed only when the outermost driver sees no JIT activation on the native
stack, after the fix-up walk (§13); a nested driver never frees bodies (§11.3). Freeing is batched with W^X toggled
once, into size-segregated slabs with a coalescing list, under a cap (decision 20), with BTI landing pads if enabled.
The zeal mode `jit-invalidate` invalidates every body at every poll.

### 9.8 Identity hash

*Implements decision 10 (proposed).*

```
identity-hash(v):
  immediate                → mix64(bits)
  symbol                   → stored hash (a fixed-seed hash of the name, so a re-interned symbol hashes the same)
  HASHED clear             → set HASHED (MetaByte::fetch_or); return mix64((addr − heap_base) ^ seed)
  HASHED, not HASH_MOVED   → mix64((addr − heap_base) ^ seed)
  HASH_MOVED               → the u64 in the trailing extension granule
```

Hashing relative to the heap base is identical across runs despite ASLR. **No pinning:** evacuation copies a `HASHED`
object into `size + 16`, stores the original hash in the extension granule and sets `HASH_MOVED` (Bacon–Fink–Grove, as
in JDK compact headers), which works for headerless objects and costs nothing for objects never hashed. SRFI 69's
stored hashes (`lib/srfi/69/srfi-69-impl.scm:118-120` [S]) stay valid; `equal-hash`'s heap-index fallback becomes
`identity-hash`. A symbol's hash changes from the heap index to its name hash in its own PR (it reorders symbol-keyed
`eq?` tables).

### 9.9 The epilogue, in a fixed order

This order is a contract obligation of every collector (§14); minors restrict every step to young entries.

1. Trace to completion.
2. **One fixpoint:** newly reached host payloads → ephemerons resolved by key → (later) guardians, which resurrect
   *before any breaking*.
3. Break the remaining pending ephemerons: key and value become `BWP`.
4. (Later) SRFI 254 transport cells.
5. Weak-key tables and weak inline-cache entries: rekey forwarded keys, drop dead ones.
6. Prune the weak-id and host-payload tables; at majors also the symbol interner and the namespaces' weak entries
   (placeholders whose cell is still `UNBOUND` and that no live link table holds; from 5c, reclaimable introduced
   definitions, §11.6).
7. Classify the finalization registry (§9.6) and queue the dead.
8. Code release (majors only).
9. Queue the post-pause work: dead LOS runs (after every collection: young runs after minors, young and old after
   majors) and, after majors, the catch-up sweep, the NMS sweep, block classification and decommit candidates.
10. Statistics and pacing: L is the marked bytes plus LOS and external bytes; `bytes-reclaimed` includes released LOS
    runs; the in-pause work counters of §17.1.
11. Release the world. Queued finalizers run at the poll before Scheme resumes; post-pause work runs in rate-limited
    slices at later polls (an idle major completes it before its wait, §17.1). All of it is logged as non-mutator
    intervals.

### 9.10 Pause budgets

*Implements decision 6's budget form (proposed).*

Both kinds of pause have a budget of one form: a constant plus terms in work the program controls, and **no term in
dead objects, committed heap size or the arena high-water mark** (decision 6). Stage 5a's mark microbenchmark
(records, closures, vectors, deep stacks) measures a, b and c; stage 7b extends it with store-buffer entries to measure
c_min and d. The budgets are checked on the GBS, `deep-descent`, `deep-unwind` and `large-live` (0.5 and 1 GiB live)
by per-workload max pause and MMU(10 ms).

| Pause | Budget | Constants |
|---|---|---|
| Major | c + a·(frames in all rooted stacks) + b·(live MiB) | c ≤ 1 ms; a ≤ 15 ms per million frames (≈13 ns per frame with its registers [P, deeprec]); b ≤ 0.35 ms per live MiB single-threaded (3–8 ns per object at the 26.9 B mean), divided by the worker count under stage P |
| Minor | c_min + a·(frames above the watermarks) + d·(logged granules) + b·(survivor MiB) | c_min ≤ 0.25 ms; a and b as for majors, each rooted thread's dynamic-state slots counting as one frame and young LOS survivors as survivor bytes; d ≤ 10 ns per logged granule, the logged granules being the store buffers' exact entries plus the granules covered by range entries and remember-whole objects |

- **The frames terms have no limit:** a million-frame descent that then allocates costs about 13 ms in one minor.
  Forcing a minor whenever the stack grows is rejected (10³–10⁴ extra minors on deeprec); `deep-descent` and
  `deep-unwind` measure the cost.
- **The logged-granules term is bounded by the store buffer's soft limit**, about 1.25 ms / d rounded to a power of
  two (128 K granules, 1 MiB of entries, at d = 10 ns), set by stage 7b; a range entry or remember-whole object that
  would pass it is not logged, and the next collection is forced major (a major re-derives every bit).
- **The survivor term** is bounded by the nursery budget plus overdraft (4 MiB, about 1.4 ms), except that a surviving
  young LOS object is charged like any survivor.
- **Expected values** [I]: a per-workload max minor pause of at most 2 ms on the GBS; majors of about 5–15 ms at the
  measured maximum of 50 MB live; at 1 GiB live, about 40 M objects (2³⁰ / 26.9 B), so 120–320 ms single-threaded,
  inside b's budget but long, which is why stage P is budgeted. Today's worst pauses (queue3's 41 ms, mostly sweep;
  178 ms after a library load, mostly `Drop` and provenance [P]; a peak's sweep cost kept by every later pause,
  [#616]) are made of terms this design removes from the pause. Per-workload estimates: DESIGN E.4.

## 10. Write barrier

**Kind.** Pre-write **field logging** at granule granularity, with armed-bit polarity: fresh memory is unarmed, so
stores into young holders fall through; the first store into an armed granule of an old object logs that granule's
exact address into a per-mutator sequential store buffer (SSB) and disarms it. It needs no heap parsability (pairs
have no header) and deduplicates by construction (slow paths per run in simulation: nboyer 5, hashtab 131 K [P]).

**aarch64 fast path.** Holder `x0` (tagged), value `x1`, field at byte offset `OFF`, tag `T`; `x20` = `meta_bias`,
loaded once per fragment from `[x21, #0x10]` as a hoistable `readonly` load.

```
      add   x2, x0, #(OFF − T)        ; slot address (the store needs it anyway)
      tbz   x1, #2, 1f                ; [1] immediate value → no barrier
      lsr   x3, x2, #4                ; [2] granule index
      ldrb  w4, [x20, x3]             ; [3] metadata byte
      tbnz  w4, #6, log_cold          ; [4] LOG armed → cold block
1:    str   x1, [x2]
```

Four instructions for heap values, one for immediates; two fields in one granule share one entry. The cold block
disarms the granule (`ldclrb` under `threaded`), appends its address at `remset_cur`, and at the soft limit makes an
owner post of `GC_MINOR`. It is call-free (no tier declares stack maps, §11.2); stage 6 measures a `PreserveAll` stub
against it.

**Store buffer.** 256 MiB of reserved VA per mutator, lazily committed, with a guard page at the hard end (the Rust twin
checks the end). Its **soft limit is derived from the minor-pause budget** (§9.10) and is divided among carriers when
there are several (§18). **Bulk stores** (`store_range`, `fill_range`: `vector-fill!`, `vector-copy!`, string-port
growth) log one range entry and disarm its granules with one metadata `memset`; range entries and remember-whole objects
count their granules against the soft limit. If the poll slow path finds the buffer past its soft limit while
collection is deferred (`NoGcScope`, the `HEAP_EXHAUSTED` overdraft), it **discards** the buffer and forces the next
collection to be a major (§9.1).

**Filters, in order:** static elision (literal immediates; results of boolean-producing operations, never arithmetic,
which can overflow to bignums; initializing stores into objects allocated with no safepoint since); the dynamic value
filter (bit 2); a young holder (unarmed: 99.4% of the median workload's stores [P]); a granule already logged this
cycle (disarmed).

**Arming.** Marking arms every granule of every pointerful object it marks, in generational mode only, cells and
binding records included; minors re-arm what they process; sweep clears freed granules. Objects born old are exactly
those made by `alloc_old` (descriptors, record types, symbols, cells, binding records, a future boot image's mutable
objects, §7): created unarmed and, when pointerful, pushed on the **remember-whole list**, which the next minor scans
whole before arming them (G1's compensation). Immortal objects reference nothing mortal and need neither. Every other
allocation, LOS and overflow-allocator objects included, is born young.

**No barrier on** strings, bytevectors, bignums and symbols (pointer-free); register-stack and frame stores (roots,
covered by the watermark); continuation objects (immutable after capture); descriptors after creation; inline-cache
words, which live outside the heap and hold only NMS or immortal references and immediates (§11.2). A heap whose policy
is not generational reports `BarrierKind::None` for its whole life (§9): JIT code emits nothing, and the Rust funnel's
branch is never taken. (A future incremental mode is the exception: a heap that may run it reports `GranuleLog` from
creation, §14.)

**`WATCHED` cells.** A cell's flags word carries `WATCHED` once JIT code depends on its value; a store into a `WATCHED`
cell takes a `Transfer` cold path that invalidates the dependent bodies and deoptimizes the executing fragment before
stale code can observe the new value (§13). Until a JIT exists, the interpreter guards its inline primitive sites per
binding (decision 2).

**The Rust store funnel** is the only way to write a value into a heap object:

```rust
impl<'gc> Cx<'gc> {
    #[inline(always)]
    pub fn store<S: SlotOf<'gc>>(&mut self, slot: S, v: Value<'gc>) {
        let a = slot.addr();                               // PairCar, PairCdr, VectorElem (bounds-checked),
        if v.raw() & 0b100 != 0 {                          // CellValue, RecordField, ClosureFree (fix-up only),
            let m = self.mutator().meta(a);                // GlobalCellValue, PromiseBox, ParamValue
            if m.load() & LOG != 0 { self.log_granule_cold(a, m) }  // #[cold]: the twin of the JIT slow path
        }
        unsafe { HeapSlot::at(self.base(), a).store(v.raw()) }      // plain; relaxed AtomicU64 under `threaded`
        if S::IS_CELL { self.check_watched(slot) }         // #[cold] Transfer path when WATCHED (JIT era)
    }
    pub fn store_range(&mut self, v: Vector<'gc>, start: usize, src: impl ExactSizeIterator<Item = Value<'gc>>);
    pub fn fill_range(&mut self, v: Vector<'gc>, start: usize, len: usize, x: Value<'gc>);  // one range entry
}
/// Initializing stores: only through a token that holds the mutator borrow, so no safepoint can intervene,
/// and only with values of the same heap.
pub struct Fresh<'m, 'gc> { obj: Value<'gc>, _m: PhantomData<&'m mut Mutator> }
impl<'m, 'gc> Fresh<'m, 'gc> { pub fn init(&mut self, word: u32, v: Value<'gc>); pub fn finish(self) -> Value<'gc>; }
```

`HeapSlot::at` derives its pointer from the per-heap base (`base.with_addr(a)`), preserving provenance; debug builds
assert in `Fresh::init` that the holder is young or pre-logged. `Heap::vector_slice_mut`, `get_string_chars_mut` and
`get_bytevector_mut` are deleted, and records, parameters and promises, stored today through cloned `Rc` handles that
bypass the heap (`crates/patina-primitives/src/primitives/records.rs:262`, `parameters.rs:168,267`, `lazy.rs:134` [S]),
are written through the funnel.

## 11. Roots and rooting

### 11.1 VM frames and the stack watermark

**The register stack never relocates.** Each green thread owns a fixed reservation, committed on demand: the main
thread's defaults to min(8 GiB, 25%·B) (8 GiB holds about 60 M frames; 1.75 GiB on a 7 GB CI runner still holds
today's 10 M-frame recursion, 1.37 GB [P]), other green threads 256 MiB, set by `PATINA_STACK_MAX`, `--stack-max` and
`HeapConfig` (decision 15, SD5); green-thread stacks follow §18.1's stack rule. Within 1 MiB of the
cap the frame-entry check raises a catchable `&stack-exhausted`, the last MiB being the handler's room; uncaught, it
exits non-zero.

Frames are **interleaved and position-independent**, with one header for the interpreter, JIT code and captured
continuations:

```
fp+0   ret      raw     cached resume address in the caller (JIT); 0 in interpreter frames; 0 in captured copies
fp+8   link     fixnum  caller distance in bytes (fp − caller_fp) [3–34] | nregs [35–50] | flags [51–62]
                        (stub kind; WM = "the return out of this frame crosses the watermark")
fp+16  code     value   reference to the code descriptor
fp+24  meta     fixnum  pc of the last suspension point [3–34] | return_reg (result register in the caller) [35–50]
fp+32  closure  value
fp+40  r0 … rₙ₋₁ tagged values only, in every tier, at every suspension point (§11.2)
```

The header is 40 B, today's `CallFrame` size; `closure` becomes a traced value. The interpreter keeps its calling
protocol (`return_reg` in the callee's frame); prompts, handlers and winds record byte offsets from the stack base. The
JIT delivers a result as the first `CallConv::Tail` argument of the caller's resume entry (`x2` on aarch64, `rdi` on
x64 [S]).

**Frame invariants.**

| # | Invariant | Content |
|---|---|---|
| 1 | Window initialization | every frame push, stub frames included, writes an immediate into every non-parameter slot of its window, so a window never holds a stale word from a popped frame |
| 2 | Maps at every suspension point | the return pc of every call (including `CallPrimitive` and every inline primitive opcode, whose slow path or shadow deoptimization calls from that pc, `crates/patina-vm/src/runtime/control.rs:3467-3489` [S]); the pc after every instruction that can raise (`raise_step_stub` stays over the erroring frame while handler code runs, collects and captures, `control.rs:909-978` [S]); pc 0, where frame-entry polls are serviced; every `Transfer` helper site. The table is **dense**: a `pc → u16` map index per body (`0xFFFF` = never a suspension point) into deduplicated bitsets |
| 3 | Clearing, not skipping | the VM's root provider visits each live slot through the map, plus `code` and `closure`, and **overwrites each dead slot** with `UNSPECIFIED` (`DEAD_SLOT` in the debug-poison and zeal lanes): a wrong map can then retain garbage, which the [#423] tests catch, but never expose a freed referent. Stub frames carry an all-live map; a frame at a `0xFFFF` pc panics in debug builds and has every slot visited in release builds; debug builds assert in `reg_at` that no instruction reads a `DEAD_SLOT`; capture clears dead slots in the copy too (§13) |
| 4 | Other readers of raw registers | the `StepTracer` (`crates/patina-vm/src/tracer.rs:269-286` [S]), watchpoints, debugger hooks and `--dump` read registers only at suspension points, where 1 and 3 make every word a value or an immediate (the datum writer renders `DEAD_SLOT` as `#<dead>`); per instruction that holds only in the interpreter, which is why attached hooks pin the interpreter tier (§11.2) |
| 5 | Verifier | every frame pc reached during a scan or a capture has a map, and every visited register decodes to an immediate or a start granule |

**The stack watermark (generational heaps)** is a return barrier. After a minor every frame below the executing frame is
clean, so its caller becomes the watermark: the collector sets the `WM` flag in that frame's `link` word (interpreter)
or swaps its `ret` for a trampoline, saving the real target in the thread **as `(code, pc)`**, never as a raw address
(JIT). Every `Return` reads both words anyway, so normal returns pay nothing; the cold path lowers the watermark by a
stride of k frames (k = 64–256, tuned in stage 7). A tail call copies `link` and `ret`; nothing else overwrites a `ret`
that holds the trampoline (invalidation's re-derivation skips it, §13), and the saved `(code, pc)` stays valid whichever
body serves it. **Every write into, or activation of, a suspended frame** (value delivery, `ResumeWindJump`, raise
stubs, abort landing, delimited append, scheduler deliveries, debugger writes) goes through `return_into(frame)`, which
lowers the watermark first and sets the owning thread's `ran_since_gc`; reinstating a continuation resets the watermark
to the stack base. A minor scans only frames above the watermark of threads that ran, which removes deeprec's per-minor
scan of 11.5 M registers [P]. The verifier asserts that no frame below a watermark holds a young reference; [K8](#k8)
can switch the watermark off, leaving minors correct and slower.

### 11.2 JIT frames, by tier: the tier contract

*Implements decision 8 (proposed).*

**In every tier, VM register-stack frames are the only home of Scheme values at every suspension point (every
non-`Leaf` call), and there they hold only tagged values.** No tier uses Cranelift user stack maps, a native frame
walker, an unwinder or `stack_switch`, and derived pointers never live across a suspension point. The fragment model
(`CallConv::Tail` fragments with `return_call_indirect`) is the only baseline; native call/ret is outside this
contract ([K11](#k11)). Tier 1 caches VM registers in SSA between suspension points and stores dirty values before
each; a later tier 2 may unbox and derive within a block but publishes **tagged values only**, so the same
`(code, pc)` means the same thing whichever tier wrote the frame, capture stays a `memcpy`, and invariant W holds.

**Helper classes**, from a machine-checked `#[helper(class = …, noalloc)]` attribute and a table the JIT reads:

| Class | Contract | Examples |
|---|---|---|
| `Leaf` | never collects, runs Scheme, transfers control, blocks or enters a safe region; values stay in SSA across the call; an error comes back as a status, after which the fragment takes a `Transfer` path to raise it (or, for `Oom`, to collect and retry, §8). `NoAlloc` marks runtime helpers that do not touch the allocation buffer; a host primitive is never `NoAlloc` (§11.5) | the refill, bignum promotion, `eqv?`/`equal?`, flonum boxing, the user-sized constructors, non-blocking heap-only primitives |
| `Transfer` | anything else. The fragment publishes dirty values, `pc` and `ap`; the helper services pending events and answers `(Continue \| Target(entry), value)`, and the fragment reloads and continues or tail-calls the target (the interpreter's helpers answer the next `(code, pc)`). So a thread switch, a signal, a debugger stop or a deoptimization resumes elsewhere without any native frame resuming stale state | the poll slow path, calls of closures from helpers, resumable and higher-order primitives, `apply`, the deoptimization call of a shadowed inline primitive, `call/cc` and capture, continuation invocation, raise, the wind operations, abort, `(gc)`, blocking I/O, tier-up and invalidation, `WATCHED` stores |

**Embedded addresses.** Machine code embeds an address as an immediate only if it is (1) in the immortal space (§7);
(2) a descriptor of the body's own unit, freed with the body; (3) guarded by a `WATCHED` dependency that invalidates
the body before the referent can die; or (4) an NMS object (symbol, cell, binding record, descriptor, record type) that
the unit's constants or link table reference, so it lives while the body lives and never moves. Under variant R a cell
reached through a re-pointable record stays under rule (3). Anything else is one load from the descriptor's traced
constants, since a reused address could satisfy a stale type test. **The `Mutator` is never embedded** (it is always
in `x21`/`r15`), and code is never patched at GC; tier-up and invalidation write `desc.entry` (data, published under
§18.2 rule 4 (h)).

**Inline caches** live in a per-body table owned by the `CodeStore`, outside the heap, and are traced with their unit
(weak entries are cleared at epilogue step 5, §9.7). Their words hold only NMS or immortal references and immediates,
so no store into them needs a barrier and nothing they name moves; a cache that needs any other referent stores it
through the funnel into a heap object.

**The entry trampoline.** With `enable_pinned_reg`, Cranelift never saves the pinned register, yet `x21`/`r15` are
callee-saved for the Rust caller (`aarch64/abi.rs:1411-1417`, `x64/abi.rs:1140-1143` [S]). Rust enters JIT code through
a generated trampoline that saves the old pinned value, sets the `Mutator`, calls the `Tail` body and restores the old
value, so nested drivers and many heaps per process never corrupt a caller's `x21`.

**Panics never unwind through a fragment.** Cranelift frames carry no unwind registration, so a helper or host
primitive called from JIT code catches a Rust panic at its boundary and turns it into a fatal status, or aborts the
process.

**Debugging pins the interpreter tier** (`PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §10.1). Attaching a `StepTracer`,
breakpoint, watchpoint or `DebugHook` happens only at a top-level-form boundary and sets a per-heap **tier-policy
flag**: nothing tiers up, no JIT body is entered, and running fragments leave their bodies through the `Transfer`
invalidation path (§13); detaching clears it at the next form boundary. A paused hook that evaluates Scheme re-enters
the machine beneath a `Cx`, so its evaluation runs under `NoGcScope` and its allocation is counted by [K16](#k16).

### 11.3 Rust code: the soundness boundary

**Capabilities.** The type system, not a runtime counter, decides who may collect.
- **`&mut Heap`** is the only capability that can collect: the VM's and tree-walker's outermost driver loops, the
  loader at points A and B (below), `eval_datum`, `run_forms`, and the embedding API's `eval_*` and `call` hold it.
- **`Cx<'gc>`** is the mutation and allocation context, created by `heap.mutate(|cx| …)` (HRTB-branded, gc-arena's
  shape) or by a driver for one primitive call. **No method on `Cx` can collect.** A nested driver entry reachable from
  a `Cx` (point C, the residual `apply_proc` fallbacks, a paused debugger's evaluation, `across_reentry` until stage
  4e, an FFI callback until callbacks become collecting driver entries, "FFI callbacks" below) runs its polls
  deferred: `NoGcScope` by type. So **a `Value<'gc>` cannot be alive while a collection runs**: the borrow checker
  refuses `&mut Heap` while any `Cx<'gc>` borrows the heap, and roots carry values across. Such an entry also runs
  only the interpreter tier, as attached hooks do (§11.2), and only the outermost driver frees JIT bodies (§9.7), so no
  body is freed while a fragment beneath it is suspended on the native stack.
- **The VM and tree-walker cores are a trusted island**: their loops hold `&mut Heap` and raw `Word`s in VM-managed
  memory, resting on the root-provider contract, generated traces (`declare_layouts!` for heap kinds, §6; the `Trace`
  derive for every Rust structure a root provider walks, §14), the verifier, the poison lanes and zeal. JIT entry
  passes the heap as a raw pointer derived from the driver's `&mut Heap`, reborrowed by each helper (Stacked Borrows,
  checked by Miri on the Rust twins). Until 5e the capability is a `GcDriver` token over the `RefCell`-wrapped arena
  heap.
- **`NoGcScope` is a type with a counter behind it.** The type is the rule: nothing beneath a `Cx` can name
  `&mut Heap`, so safe Rust there can neither collect nor forget a scope. The counter is the rule's run-time form for
  the trusted island, which reaches the heap through a raw pointer: each nested entry opens a `NoGcScope` guard that
  increments the `Mutator`'s `no_gc_depth` and records its site for [K16](#k16)'s counters, and the poll slow path
  collects only at depth 0 (§12). An island entry that forgets its guard is caught in debug builds, which count live
  `Cx`s and assert at every collection that none is live; the zeal-entry lane reaches that assertion at every poll
  site.

```rust
pub type Prim = for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>]) -> Result<Value<'gc>, EvalError>;
// Value<'gc> = #[repr(transparent)] word + PhantomData<fn(&'gc ()) -> &'gc ()>  (invariant brand)
// Resumable primitives return Step<'gc>: Done | Call | Eval | Collect { kind, state }  (resumed with whether it collected)
```

The brand is pinned by trybuild compile-fail tests (a value used after a may-collect call, a slice held across
`load_library`, a value in a `static`, a value escaping `interp.with`) in primitives, frontend, macros, the embedding
API and `patina-compat`. Values stored long-term outside the core crates (`CoreExpr`/`CpsExpr` literals, macro
literals, `Step` state) use a `pub unsafe` raw-word API under a `NoGcScope` or a traced root, or the **`CoreExpr`
literal pool**, a traced table shared by both backends and scoped to one compilation (M2).

**The unsafe boundary.** `#![forbid(unsafe_code)]` in `patina-primitives`, `patina-frontend`, `patina-macros`,
`patina-runtime`, `patina-ir`, `patina-pipeline`, `patina-interpreter` and `patina-compat` (none uses `unsafe` for heap
access today [S]), and `deny` with an audited allow for `patina-repl`'s two `std::env::set_var` calls; unsafe heap
access is confined to `patina-core`, `patina-gc`, `patina-vm`, `patina-tree-walker` and the JIT crate. `Word → Value`
and raw slot access are `pub unsafe fn`; release-mode reservation range checks guard the trust boundaries;
`PENDING_ESCAPE` leaves `thread_local!`. **Borrowing:** reads take `&Cx`; mutation and allocation take `&mut Cx` until
5e, `&Cx` after; bulk reads return `&[HeapSlot<'gc>]` tied to a `&Cx` borrow; no public API returns `&BigInt`,
`&mut Vec<char>` or `&mut [Word]` into the heap.

**Across a collection**, only these hold values: resumable-primitive state (in the VM `resume_stub` frame or the
tree-walker's `ResumePrimitive` continuation, both traced); a **`RootScope<'h>`**, which wraps the `&'h mut Heap`
capability so `mem::swap` cannot break LIFO, and whose `Rooted` values carry `{heap_id, generation}`, checked on `get`;
an **`Owned` handle** (`{heap_id, index, generation}` plus a `Weak<HandleTable>`, so a `Drop` after teardown is a
no-op); `RootToken` and `PinToken`, which hold a `Weak` to their table.

**Loading and nested loops.** A bare top-level `(import …)` is recognized **by the binding of `import`**, never its
spelling, and processed outside the desugarer, so library bodies collect between forms (points A and B) and inside them
(point D, in the outermost loop); `define-library` is recognized the same way ([#610]) and its forms reach a collection
point ([#614]). Nested VM loops stay deferred until stage 4e deletes the weak continuation tables, whose soundness rests
on "nested loops defer" (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:21-28` [S]); then loops the VM enters from
its own driver level may collect, and loops beneath a `Cx` stay deferred for good. Point C (an import met mid-form) and
nested tree-walker trampolines keep `NoGcScope`. `%parameterize-swap!` stops calling back at stage 4a. Detail: DESIGN
H.5.

**FFI callbacks.** A synchronous C→Scheme callback is a driver entry like `Interpreter::call`. An FFI call that may call
back is a `Transfer`: it marshals its arguments first, holds only `Owned` handles and `PinToken`s across the C call and
no `Cx`, and the callback enters the machine behind a continuation barrier (a continuation captured inside it cannot be
reinstated once the C frame has returned), so a C event loop that calls Scheme for ever collects like any program.
Until that entry exists, a callback runs beneath the FFI primitive's `Cx` under `NoGcScope`, a [K16](#k16) site, and
in the interpreter tier only.

### 11.4 The tree-walker: it stays correct and may lag

Its heaps are `HeapPolicy { generational: false, evacuation: false }` **for good**: whole-heap and non-moving, sharing
the object model, primitives, allocator and funnel, collecting only at its outermost trampoline safepoint (nested
trampolines keep `NoGcScope`). Its `Rc<Environment>` frames, `ContValue` chains, `StepResult` fields, `CpsExpr`
literals and the pending escape are reported by value through `SlotVisitor::pinned`, sound because nothing in its heaps
moves. `CpsLambda`/`CpsContinuation` are host-payload ids; globals go through cells; an `Environment` is charged as
external bytes only while a registered host payload holds it, so `(fib 25)`'s 1.09 M environments [P] never drive a
collection. A green thread is one suspended `StepResult` (§18.1). **A red tree-walker lane blocks the stage that turned
it red.**

### 11.5 Embedding API

*Implements decision 13 (proposed).*

| Entry | Contract |
|---|---|
| `Interpreter::eval_*`, `interp.with` | `eval_*` returns `Owned`, which fixes the use-after-free of an `eval_str` result read after a later collecting `eval_program` ([#605]). [#605] also covers every other public path that hands the host a raw value or environment (`global_env().get`, `backend()`, `evaluator()`): each returns a handle or is documented as the raw layer, unrooted between calls, and the handle table traces its handles rather than only marking them. `interp.with(\|cx: &mut Cx<'gc>\| …)` is branded and takes `&mut self`, so nothing that may collect runs inside it |
| Host environments | a rooted environment handle from [#605]'s table, in which both backends evaluate; today a host-built `Environment::with_parent` loses its bindings at a later collection, and the VM's `Backend::eval` ignores the environment it is given ([#620]). At 4b it becomes a namespace handle (DESIGN E.3) |
| `Interpreter::call(&mut self, f: &Owned, args: &[Owned]) -> Result<Owned, InterpreterError<_>>` | a driver entry like `eval_*`: it may collect while the callee runs, with the arguments rooted by their handles, and a continuation captured inside it behaves as one captured inside `eval_*`. `Cx` has no `call`; a primitive needing a callback returns `Step::Call` |
| Host primitives | `register_primitive(lib, name, PrimSpec { f: Prim, arity, class })` and `register_resumable(lib, name, arity, f)`. `class` declares the helper class the JIT reads, `Leaf` or `Transfer` (`Transfer` for one that blocks; resumable means `Transfer`). **The JIT treats every registered host primitive as allocating:** it writes `ap` back before the call and reloads `ap` and `alloc_limit` after, whatever its class, since only the runtime's own helpers in `HelperTable`, checked by the call-graph test (§16), may be `NoAlloc`. A host `Leaf` is trusted only not to block: its `Cx` can neither collect nor call Scheme. A procedure argument is called only through `Step::Call`; registration exports the primitive from the library `lib`, so a program reaches it through `import` and fast paths key on that binding, never on the spelling |
| Memory hooks | an external-bytes accounter from stage 3 (V8's `ExternalMemoryAccounter`, Chez's phantom bytevectors); `notify_idle()` from 5e; from 5g a near-limit callback that may raise `max_heap` up to the heap ceiling and, under a shared `MemoryBudget`, no further than the budget's remaining total (V8 raises only inside its pre-reserved cage), a memory-pressure notification, and `HeapConfig`'s optional `Arc<MemoryBudget>`, shared by every heap created with it (§17.3) |
| `Interpreter::interrupt_handle()`; `Backend` | the `InterruptHandle` of §12. The public `Backend` trait migrates one step per stage under [#601]'s deprecation convention, with `scripts/check_embedding_features.sh` green at each (detail: DESIGN E.3) |
| Teardown | dropping the interpreter tears its heap down: finalizers run, ports flush, the reservation is unmapped ([#604]). Current ports belong to the interpreter, never to the OS thread ([#618]). `patina-compat`'s reader uses `Heap::new_standalone()`, with collection off and no driver. Plugins never see `HeapIndex` or the encoding |

### 11.6 Global bindings under variant R

*Implements decision 2 (decided).*

Every off-heap holder of a value today has a stated fate (DESIGN E.1, carried to stage 0's tracking issue); the global
environment's is a design. Variant R (stage 4b) keeps today's "follow the name" semantics (decision 2): code compiled
before a later `define` over an import, or before a later import, sees the new binding ([#603] records where the
oracles differ).

| Element | Rule |
|---|---|
| Binding records | every name in a namespace (the global environment, a library's environment, an environment specifier) maps to a `BindingRecord { cell }` in the NMS: own definitions, imports and placeholders alike. `Library.exports` maps names to cells, resolved one hop to the owning cell. A variable alias stops being a name at 4b (the reference links the definition's own record); keyword aliases are deduplicated by target ([#611]) |
| Link tables | each code unit keeps a table of record references indexed by its global operands, filled at load; a name with no binding yet gets a placeholder record whose cell holds `UNBOUND`. `LoadGlobal`, `StoreGlobal` and `Define` take a link index; a read is `link[k] → record → cell → value` |
| Re-pointing | a `define` over an import allocates a new cell and re-points `record.cell`; a `define` over the namespace's own binding stores into its cell (R7RS's "acts like `set!`"); an import re-points `record.cell` to the exporter's cell. Earlier code holds the record, so it sees the change when the `define` runs |
| Ownership and mortality (M1, M2) | namespaces own their records and cells; the global and library namespaces are roots, and an environment specifier's namespace is a charged host payload that dies with the specifier or its last unit ([#615]). A placeholder is a weak entry while its cell is still `UNBOUND` and no live link table holds its record. **Macro-introduced definitions stay strong at 4b**, with an indexed lookup ([#613]), because a template can be their only reference (the `def-getter` shape, §17.4). From 5c such an entry is reclaimed only when no live link table references its record **and** no live identifier carries the expansion scope that introduced it, which 4c's weak scope-set table answers. Variant C keeps these rules |
| What goes; what stays | `GlobalCacheEntry`, the `env_id` cache, `frame_globals`, `FORWARDED`, `Owner` links and `VmClosure.globals` go. The shadow bitsets and `mark_if_*` stay: they latch when a record is re-pointed or a cell store replaces a primitive, and drive the per-site deoptimization ([#442]). Stage 4b measures the path against `frame_globals` rather than assuming a gain |
| The JIT | under R it emits the two dependent loads, or embeds the cell with `WATCHED` on both record and cell; under C a JIT global is one load. The tree-walker reads root bindings through the same records |

**Variant C** (decision 2: after stage 5, as §19's row C; how it answers the six rebinding shapes is decision 3).
References bind to cells at compile time, as in chibi, Chez and Racket. C deletes the records' indirection, the shadow
bitsets and `mark_if_*`, but keeps per-binding guards, because a program's `set!` of an imported primitive still
writes the library's shared cell ([#406]): `CallPrimitive`, the inline operations and `JumpUnlessCellHolds` take their
fast path only while `cell.value == expected`, keeping the tail-shape deoptimization, and JIT sites rely on `WATCHED`
(§10). The two set-after-use tests stay green; six define-after-use tests flip, each with an oracle-scored
`DIVERGENCES.tsv` row.

## 12. Safepoints and polling

*Implements decision 9 (proposed).*

**Mutator words** (offsets in §14): `reg_limit` (`AtomicUsize`; non-zero values written only by the owner through
`set_limit`, zero by any poster), `event` (`AtomicU32`; remote posters `fetch_or`, the owner swaps), `pending`
(`Cell<u32>`, owner posts only) and `ticks` (`Cell<i32>`, Tick mode only). Relaxed atomic loads and stores are plain
`ldr`/`str`, so the atomics cost nothing and a signal handler writing them is sound. **Event bits:** `GC_MINOR`,
`GC_MAJOR`, `PREEMPT`, `SIGNAL`, `DEBUGGER`, `TERMINATE`, `HANDSHAKE` (reserved), `FINALIZERS_PENDING`,
`DECOMMIT_PENDING`, `HEAP_EXHAUSTED`, `STACK_GROW`.

**Protocol.** Owner posts (allocation slow path, barrier soft limit, finalizers, decommit, stack growth):
`pending |= bit; reg_limit.store(0, Relaxed)`. Remote posts (signal handlers, timers, profilers; later, other
carriers): `event.fetch_or(bit, SeqCst); reg_limit.store(0, SeqCst)`. `set_limit(new)`, used by the poll slow path and
by a green-thread switch: store `new`, `fence(SeqCst)`, and store 0 again if `event` or `pending` is non-zero. The poll
slow path takes `pending.take() | event.swap(0, SeqCst)`, services it, then calls `set_limit(real_limit)`. This is
OCaml's protocol (`runtime/domain.c:387-396,2022-2058`): the fence makes a Dekker pair, so **no request is ever
lost**, and latency is bounded by the next frame entry, back-edge or `Transfer` return.

**`InterruptHandle`** (a heap's handle for signal handlers, timers and profilers, which cannot borrow an `Interpreter`;
there is no process-wide GC singleton): `InterruptHandle(Arc<InterruptCell { mutator: AtomicPtr<Mutator>, posting:
AtomicUsize }>)`, `Send + Sync + Clone`. `post(ev) -> bool` is async-signal-safe (lock-free atomics only, no
allocation, no panic path): `posting.fetch_add(1)`; load `mutator`; if non-null, `event.fetch_or(ev)` and
`reg_limit.store(0)`; `posting.fetch_sub(1)`; all `SeqCst`. Teardown stores null into `mutator`, waits until `posting`
reads 0, then frees the `Mutator`, so a poster either is waited for or does nothing. The REPL's SIGINT handler uses its
interpreter's handle through a `sigaction` wrapper in `patina-core`. Servicing `SIGNAL` raises a non-continuable
`&interrupt` at the poll, also under `NoGcScope`, where GC events stay deferred. `mutator` names the carrier that runs
Scheme: under M:1 the one carrier; when carriers share a mutator token (§18.7), it follows the holder.

**Where polls are.** Codegen emits forward jumps only, so every loop passes through a closure call or tail call (which
enters its callee through the check) or a `Transfer` operation (which polls on return). `Leaf` primitive sites cannot
loop and need no poll.

| Site | Interpreter | JIT | Extra cost |
|---|---|---|---|
| **frame entry**: every non-tail call, every tail call (self or not, whatever the window), `Apply`, `TailApply` | after the header and arguments are stored and the window initialized: `top > reg_limit`; an event is serviced at the callee's pc 0, whose map lists the parameters, `code` and `closure` | callee prologue: `ldr x9,[x21,#0x30]; add x10,fp,#FRAME; cmp x10,x9; b.hi cold` | **0**: the stack-overflow check is needed anyway (V8's `StackGuard` fold) |
| self tail call (every loop) | the frame is re-entered at pc 0 through the same check | `ldr x9,[x21,#0x30]; cbz x9, cold` at the back-edge | 2 instructions |
| **every `Transfer` return**: generic calls of non-closures, continuation reinstatement, abort landing, raise-stub push, `Step` results, `(gc)` | the helper services pending events before answering the next `(code, pc)` | the same, inside the helper | — |
| return to the driver | services pending work | the same | — |

**No collection sees a half-built frame**: the frame-entry check may detect an event early, but servicing waits until
the frame is complete (today `TailCall` stages arguments in a Rust buffer, `vm_state.rs:1601-1630` [S]). The cold path
tells a genuine stack limit from a posted event (`reg_limit == 0`). The interpreter's per-instruction `gc_pending` load
(+1.1–1.4%, as `docs/GC_DESIGN.md` §6.1 at `28a94f8` reports; not re-run) goes in stage 3. Allocation and barrier slow
paths only post. Zeal `entry` services an event at every poll site.

**Deterministic mode.** GC requests come only from byte thresholds and counts (§15), so the differential lanes run the
poll code production ships. `PollKind::Tick` exists only for SRFI 18's deterministic scheduler: poll sites also
decrement `ticks` (Chez's `%trap`) and expiry posts `PREEMPT`. `ticks` counts poll sites as the bytecode places them, in
every tier: a JIT back-edge decrements as the self tail call it compiles would, and an inlined callee as its frame
entry would, so a seeded schedule interleaves identically with the JIT on or off; a tier that counts otherwise keeps
JIT-on runs out of the byte-identical scheduler lanes. No timer runs under `PATINA_DETERMINISTIC=1`, the default in test
lanes ([K7](#k7) measures Tick's cost).

**Nested Rust loops** (replacing today's `GcDeferGuard` sites). The poll slow path collects only with the `&mut Heap`
capability and `no_gc_depth == 0`; entries beneath a `Cx` run deferred. A deferred event stays posted while allocation
overdrafts; a store buffer past its soft limit is discarded with the next collection forced major (§10); `GcStats`
keeps the high-water bytes allocated since the last serviced poll and under one `NoGcScope`, with the opening site,
which [K16](#k16) bounds, so the GCLocker failure mode (JEP 423) stays visible, confined and measured. A driver about to
enter a known `NoGcScope` site first runs a major when headroom is below that site's high-water mark (§17.3).
Green-thread preemption is deferred while `reentry_depth > 0`.

**Safe regions.** `enter_safe_region`/`leave_safe_region` wrap blocking I/O and the future FFI; with one mutator they
are no-ops (Chez `Sdeactivate_thread`). One may be entered only when no unrooted heap value is on the Rust stack or in
JIT SSA: a `Leaf` helper never enters one, blocking primitives are `Transfer`, and the blocking part works on Rust
buffers (output formatted first; input read before anything is allocated, unlike today's `read`, which lexes while the
parser holds a half-built datum). The restructuring is required only before N > 1.

## 13. Continuations and stacks

*Implements decision 18 (proposed).*

**Representation: design A.** A continuation is **one immutable, variable-size heap object** (`T_CONT`): meta words as
fixnums (kind, `deliver_reg`, the depth-at-capture byte offsets, `exit_status`, re-entry boundary ids), references to
the captured winds, handlers, prompts and parameterization, and the captured frame words. The continuation procedure
is a `0101` object whose descriptor is `CONT_INVOKE`, so invoking one is an ordinary call (a `Transfer`).

**Capture** is a `memcpy` of `[base, top]` into an object allocated through `try_alloc` (collect-and-retry on `Oom`,
§8), then a pass over the copy that clears dead slots through the maps plus the `call/cc` `dst` hole (clearing that
hole fixed a 296 MB leak, `crates/patina-vm/src/runtime/control.rs:641-654` [S]), zeroes every `ret` and clears `WM`
flags. **The copy is value-only**, so invariant W holds and **the core traces a `T_CONT` word by word, like a vector**,
without parsing frames; the frame and map formats stay private to the VM, and code references in captured frames keep
their units alive by ordinary marking. Captures over 8 KiB go to the LOS, where minors release the dead ones (§9.2).

**Cost.** Today a capture at depth 1000 costs **24 µs and about 171 KB** [P, `samedepth1000`]. Design A is estimated at
about 88 KB per capture at that depth [I], from a continuation toy (5.0 µs and 64–80 KB in its model of today; design A
about 5.8× below that model, with a bare `memcpy` and 24 B frames) corrected for the dead-slot pass and 40 B headers;
stage 4e re-runs the toy with the real pass before fixing per-probe targets. **What this deletes:**
`VmContinuationRef`, `VmDelimitedContinuationRef`, both weak side tables, the VM's `trace_weak_ids`/`sweep_weak`, and
the rule "store touched within one dispatch", with it the reason nested VM loops must defer. **What it fixes:** captures
become heap bytes, where stage 1 charges a side-table snapshot to its handle (`VmContinuationRef { id, bytes }`, [#606];
before it, 80 K captures at depth 1000 reached 5.7 GB unseen by the trigger [P]).
Continuations are born young and written only by initializing stores, so they need no barrier; reinstatement copies
frames out and resets the watermark to the base; a future incremental mode must darken a continuation on reinstatement
(OCaml's fibers).

**JIT interaction.** **`(code, pc)` is the authoritative resume point**; `ret` is a cache, re-derived on reinstatement
from `desc.resume[pc]` (or the interpreter trampoline). Tier-up writes `desc.entry` and answers the new entry.
Invalidation (a `WATCHED` store, a tier-down, an attached debugger hook) marks dependent bodies invalid, deoptimizes the
executing fragment and re-derives the `ret` of every frame of the affected descriptors in every green thread's stack,
except a frame whose `ret` is the watermark trampoline: it keeps the trampoline, whose saved `(code, pc)` target (§11.1)
resolves to the current body when it fires, so the watermark survives invalidation. Replaced bodies are freed by epoch
(§9.7). Captured copies hold no `ret`, so a multi-shot continuation never jumps into discarded code, and nothing unwinds
native frames, which holds because fragments are the only baseline.

**Control semantics are unchanged:** multi-shot `call/cc` copies out on every invoke; `dynamic-wind` travels one thunk
per step through stub frames; delimited capture covers `[prompt_offset .. top]` and relocates by byte-offset arithmetic.
The matrix and `escape_from_primitive.rs` gate stage 4e on both backends.

**Later and optional: C′** freezes frames above a frozen watermark into immutable chunks thawed lazily through an
underflow stub frame (toy: 57 ns per capture at depth 1000), reusing this frame format and the watermark machinery; it
comes only after the baseline JIT and only if the capture-at-depth probes show a need ([K15](#k15)).

## 14. Pluggability contract

*Implements decision 24 (proposed).*

**Principle.** The contract is pluggable; the collectors are not a catalogue. Fast paths are data that the JIT inlines
and `#[inline(always)]` Rust twins share (Whippet's `gc-attrs.h`; JEP 475); the collector is selected statically,
per-heap policy is a runtime field read only by slow paths and fixed for the heap's life (§9), and nothing on a fast
path is `dyn`. Against HotSpot's GC interface (JEP 304), `Collector<M>` plays `CollectedHeap` and `GcAttrs.barrier` the
per-tier `BarrierSet`, with `emit_alloc`, `emit_store` and `emit_poll` as the per-tier halves. Patina **adopts** one
barrier contract consumed by every tier and late barrier expansion from one definition; it **rejects** several
production collectors chosen at startup behind virtual calls (HotSpot itself retires modes for their maintenance cost,
JEPs 474 and 490) and per-collector code-generation hooks: with one JIT and one collector, a data description the
emitter switches on is enough.

```rust
// crate patina-gc: depends on libc only; no VM or core dependency; Miri-testable.

/// Implemented by `declare_layouts!` in patina-core (`CoreModel`) and by a test model in the conformance suite.
/// Every method requires `obj` to be the start of a live object of this heap, which is why each is `unsafe`.
pub unsafe trait ObjectModel: 'static {
    const LAYOUT: &'static LayoutTable;                                    // tags, header types, JIT offsets
    unsafe fn size(obj: Address, tag: Tag, meta: MetaByte) -> usize;       // + 16 if HASH_MOVED
    unsafe fn trace<V: SlotVisitor>(obj: Address, tag: Tag, v: &mut V);   // writable slots; T_CONT word by word;
                                                                           // T_EPHEMERON only through `ephemeron`
    unsafe fn copy_to(obj: Address, tag: Tag, to: Address, extra: usize);
    unsafe fn verify(obj: Address, tag: Tag) -> Result<(), LayoutError>;
}

/// Slot-only visitor: frames are walked by the VM's RootProvider, not here. Slots are interior-mutable, so
/// providers keep `&self`.
pub trait SlotVisitor {
    fn slot(&mut self, s: &HeapSlot);               // precise and updatable
    fn slots(&mut self, s: &[HeapSlot]);
    fn pinned(&mut self, v: Word);                  // precise, never updated; pins the block for this cycle
    fn host(&mut self, id: HostId);                 // edge to a host payload, traced in the fixpoint
    /// Emitted for T_EPHEMERON in place of two strong slots. A collector traces `value` only once `key` is live and
    /// may break both (§9.5); a visitor that is not a collector may visit both.
    fn ephemeron(&mut self, e: Address, key: &HeapSlot, value: &HeapSlot);
}
pub enum RootPass { Minor, Major }
pub trait RootProvider { fn trace(&self, pass: RootPass, v: &mut dyn SlotVisitor); }
// Registration is open: RootSet::register(Box<dyn RootProvider>) → RootToken, which holds a Weak to the set.

/// The heap's carriers and green threads, owned by the heap and lent to `collect`: one `Mutator` under M:1, two in
/// the two-mutator lane, N later. The VM's root provider walks frames, reading each thread's watermark here and
/// re-installing it as it finishes that thread.
pub struct MutatorSet { /* mutators: Vec<Box<Mutator>>, threads: ThreadTable */ }
impl MutatorSet {
    pub fn mutators(&mut self) -> impl Iterator<Item = &mut Mutator>;  // retire ap/limit; drain (minor) or drop
                                                                       // (major) the store buffer, range entries and
                                                                       // remember-whole list; deferral high-water counters
    pub fn threads(&mut self) -> impl Iterator<Item = &mut ThreadGcState>;      // every rooted green thread
}
pub struct ThreadGcState { pub ran_since_gc: bool, pub watermark: usize /* byte offset */, pub stack: Range<Address> }

pub unsafe trait Collector<M: ObjectModel>: Sized + 'static {
    fn new(cfg: &HeapConfig, vm: Box<dyn VirtualMemory>) -> Result<Self, HeapError>;
    fn attrs(&self) -> GcAttrs;                                             // per heap, fixed (§9); the JIT reads it
    fn bind_mutator(&self, m: &mut Mutator);
    fn alloc_slow(&self, m: &Mutator, bytes: usize, k: AllocKind) -> NonNull<u8>;            // never collects
    fn try_alloc(&self, m: &Mutator, bytes: usize, k: AllocKind) -> Result<NonNull<u8>, Oom>; // user-sized; §8
    fn alloc_old(&self, m: &Mutator, bytes: usize, k: AllocKind) -> NonNull<u8>;             // NMS; remember-whole
    unsafe fn log_slow(&self, m: &Mutator, granule: Address);            // Rust twin of the inline slow path
    unsafe fn log_range(&self, m: &Mutator, obj: Address, start: usize, len: usize);  // bulk stores
    unsafe fn identity_hash(&self, m: &Mutator, obj: Address) -> u64;    // core's safe wrapper takes Value<'gc>
    unsafe fn pin(&self, obj: Address) -> PinToken;
    unsafe fn is_live(&self, obj: Address) -> bool;   // the immortal space (primitives and their descriptors,
                                                      // canonical boxes, core syntax): true
    unsafe fn forwarded(&self, obj: Address) -> Option<Address>;
    fn requested(&self, m: &Mutator) -> Option<CollectionKind>;
    fn collect(&mut self, kind: CollectionKind, mutators: &mut MutatorSet, roots: &RootSet,
               weak: &mut WeakRegistry) -> GcStats;
}
```

**Linkage.** `patina-gc` defines the traits, `MarkRegion<M: ObjectModel>` and, in its conformance suite only,
`NullGc<M>`; `patina-core` generates `CoreModel` and defines `pub type ActiveGc = MarkRegion<CoreModel>;`, and
downstream crates name `Heap`, never `Heap<C>`. `collect` takes `&mut self`, reachable only through the `&mut Heap`
capability (§11.3), and is lent everything per carrier and per thread it must retire, drain or reset.
`VirtualMemory` is a trait so that Miri can run on a `Vec`.

**Weak processing and finalization are part of the contract.** A second collector (`NullGc`, a future
`NurserySpace`, an MMTk adapter) implements §9.5–§9.9 from these types and obligations alone:

```rust
pub struct WeakRegistry {           // per heap; lent to `collect`; no collector keeps a copy
    pub ephemerons: EphemeronTable, // per collection: pending[K] chains, KEYHINT bookkeeping, the to-break list
    pub host: HostPayloadTable,
    pub finals: FinalRegistry,
    // stage 7 or later (decision 11): pub guardians: GuardianTable
}
// Not `Send`: tree-walker payloads stay `!Send`; a threaded VM heap needs its own `Send + Sync` payload type (§18.6).
pub trait HostPayload {                        // tree-walker lambdas and continuations, macro bodies, libraries,
    fn trace(&self, v: &mut dyn SlotVisitor);  // namespaces, FFI; `slot`s, or `pinned` in tree-walker heaps;
                                               // collecting thread only
    fn external_bytes(&self) -> usize;         // charged at registration, credited at finalization (§15)
}
impl HostPayloadTable {
    pub fn register(&mut self, p: Box<dyn HostPayload>) -> HostId; // joins `young`
    pub fn store(&mut self, id: HostId, write: impl FnOnce(&mut dyn HostPayload)); // the one way to write a value
                                               // into a payload: runs `write`, then `dirty(id)` if the payload is old
    pub(crate) fn dirty(&mut self, id: HostId); // called only by `store`: a mutated old payload rejoins `young`
    pub fn trace_payload(&self, id: HostId, v: &mut dyn SlotVisitor);
    pub fn take(&mut self, id: HostId) -> Box<dyn HostPayload>;     // by its finalizer only
}
pub enum FinalKind { Port, CodeUnit, HostPayload, Thread, Foreign }
impl FinalRegistry {
    pub fn register(&mut self, obj: Address, kind: FinalKind, id: u32);   // slow-path constructors only; `young`
    pub fn queue(&mut self, id: u32);          // the runtime knows the object is finished (a terminated thread)
    pub fn drain(&mut self) -> impl Iterator<Item = (FinalKind, u32)> + '_;     // queued entries, in id order
    pub fn drain_all(&mut self) -> impl Iterator<Item = (FinalKind, u32)> + '_; // teardown (F5)
}
```

On a generational heap, a host payload written other than through `store` (through interior mutability, say) breaks
the contract; `PATINA_GC_VERIFY_ROOTS` catches it (§16). Tree-walker heaps never run minors (§11.4), so their payloads
need no `store`.

**Off-heap traces are generated, never written by hand.** Every `HostPayload`, and every Rust structure a root
provider walks (`VmState`, `Library`, `Environment` until 4b, `CompiledMacro`, `CpsContinuation`, `ContValue`, the
wind, handler and prompt records), gets its `trace` from a `Trace` derive that names every field: a field whose type
has no `Trace` does not compile, and a field is left out only by `#[trace(skip, reason = "…")]`, so a new field cannot
go untraced silently. `declare_layouts!` closes this gap for heap kinds (§6), not for these structures, and both
incidents of a missed trace edge so far (two edges in [#38], one in [#47]) were in hand-written off-heap traces, the
second hidden from every dynamic check because its values were also reachable another way. A third edge,
`CompiledMacro.foreign_expansions`, stayed untraced until [#623] traced it (`trace_compiled_macro`, pinned by the
sentinel test `compiled_macro_fields`). A proc-macro derive needs `syn` and `quote` as
direct dependencies, which wait for decision 14's approval; without them, a `macro_rules!` declaration in
`declare_layouts!`' style generates the same code. It lands at stage 2 with the slot visitor, so 4b–4e reshape these
structures under it, and 4f does not start without it: the tree-walker never moves (§11.4), so `move-all` never checks
its payloads' traces. Until then, and for heap kinds until `declare_layouts!` (5a), [#623] names every traced field
and tests each with a sentinel.

| Obligation of every `Collector` | Content |
|---|---|
| Order | §9.9's eleven steps, in that order, in every collection; a step may be empty (`NullGc` leaves all but statistics and release empty), never reordered |
| Tracing | host edges reported through `host(id)` are traced through `trace_payload`, on the collecting thread, inside the one fixpoint of step 2, with ephemerons resolved by key and, later, guardians resurrecting before breaking |
| Safety, always | never break an ephemeron whose key is live; never queue a reachable registered object or payload; never finalize or decommit inside `collect`; never call a `HostPayload` off the collecting thread; leave every ephemeron link word at fixnum 0; retire every mutator's buffer and drain or drop every mutator's logs |
| Completeness, in collections reported `complete` (every `MarkRegion` major, `(gc)` included; never `NullGc`) | every ephemeron with an unreachable key is broken, and every unreachable registered object and payload is queued. Minors restrict steps 2–7 to young entries and treat old, immortal and immediate keys as live |

**The conformance suite** runs on `MarkRegion<TestModel>`, `NullGc<TestModel>` and `ActiveGc`: allocation and tracing; a
live-key ephemeron never broken, a dead-key one broken after a `complete` collection; a 16 K ephemeron chain resolved
with linear work (counted); mark sets checked against a naive-fixpoint reference marker ([#609]); host-payload edges
traced and an unreached payload queued; a young value stored into an old payload through `store` surviving the next
minor; `drain` in id order, `drain_all` at teardown, nothing finalized inside `collect`; the epilogue order; with two
`Mutator`s, both buffers retired and both logs drained.

**The JIT ABI: `Mutator`, `#[repr(C)]`, 128-byte aligned, in `x21` (`r15` on x64)**, offsets asserted in CI and frozen
after the spike.

| Offset | Field | Type | Read by |
|---|---|---|---|
| 0x00 | `ap` | `Cell<usize>` | inline allocation (MMTk `BumpPointer` offset) |
| 0x08 | `alloc_limit` | `Cell<usize>` | inline allocation |
| 0x10 | `meta_bias` | `usize` (per-heap constant) | barrier fast path (hoisted) |
| 0x18 | `remset_cur` | `Cell<*mut usize>` | barrier slow path |
| 0x20 | `remset_soft` | `Cell<*mut usize>` | barrier slow path |
| 0x28 | `reg_top` | `Cell<*mut Word>` | calls (current green thread) |
| 0x30 | `reg_limit` | `AtomicUsize` | call check, back-edge poll; non-zero values written only through `set_limit` |
| 0x38 | `event` | `AtomicU32` | poll slow path |
| 0x3C | `pending` | `Cell<u32>` | barrier slow path (owner posts) |
| 0x40 | `ticks` | `Cell<i32>` | Tick mode only |
| 0x44 | `barrier_mode` | `u8` | slow paths only (0 generational; reserved: incremental update, SATB) |
| 0x45 | `safepoint_state` | `AtomicU8` | protocol (`Running`/`AtSafepoint`/`InSafeRegion`) |
| 0x48 | `thread` | `Cell<*mut GreenThread>` | helpers |
| 0x50 | `heap` | `*const HeapShared` | helpers |
| 0x58 | `status` | `Cell<u32>` | `Leaf` helper error and `Oom` status |
| 0x60 | `fenced_ap` | `usize`, reserved | §18 rule 4's standalone-fence variant, if stage 6 chooses it |
| 0x68 | `quiesce_epoch` | `u64`, reserved | deferred code-memory reuse under N carriers |
| 0x70 | `handshake` | `AtomicU32`, reserved | the stop-the-world handshake under N carriers |

```rust
#[repr(C)] #[derive(Clone, Copy)]
pub struct GcAttrs {
    pub alloc: AllocAttrs,          // BumpPointer { ap_off: 0x00, limit_off: 0x08, granule: 16, max_inline: 256 }
    pub barrier: BarrierKind,       // None | GranuleLog { bias_off: 0x10, shift: 4, log_bit: 6,
                                    //   cur_off: 0x18, soft_off: 0x20, pending_off: 0x3C, limit_off: 0x30 }
                                    // | Card { .. } (runner-up) | Call(extern "C" fn) (fallback; Leaf)
    pub value_filter_bit: Option<u8>,           // Some(2); None only if an SATB mode is ever configured
    pub poll: PollKind,             // LimitFold { limit_off: 0x30 } | Tick { ticks_off: 0x40, limit_off: 0x30 }
    pub watermark: WatermarkKind,   // None | ReturnBarrier { trampoline: usize, link_flag_bit: u8, stride: u16 }
    pub publication: Publication,   // FenceAtAllocation | ReleaseOnHeapStore | FenceOnHeapStore; read only under
                                    //   `threaded`, independent of `barrier` (§18 rule 4); stage 6 chooses
    pub can_move: bool, pub can_pin: bool,
    pub initializing_stores_need_barrier: bool, // false for every shipped configuration
    pub layout: &'static LayoutTable,
    pub helpers: &'static HelperTable,          // class (Leaf | Transfer) and NoAlloc per runtime helper
}
```

The JIT's emitters switch on these values at compile time; the interpreter's `#[inline(always)]` Rust versions are
generated from the same table, and the conformance tests compare the two. `install_code` records the `GcAttrs` a body
was compiled against and refuses a body whose attributes differ from its heap's, so no body runs under a policy it was
not compiled for (§9); a contract test checks the refusal.

**One collector type, runtime modes.** No mutually exclusive cargo features exist, so CI's
`cargo clippy --all-targets --all-features` stays green. Every mode is a runtime knob of the one
`MarkRegion<CoreModel>` binary, as `PATINA_GC` is today; `NullGc` is never linked into it.

| Mode | Selected by | Purpose |
|---|---|---|
| whole-heap / sticky generational / with evacuation | per-heap policy, set from `HeapConfig` at creation (§9): generational once the generational experiment says so ([K1](#k1)); evacuation from stage 8 | production |
| off | `PATINA_GC=0`: no automatic trigger, but `(gc)` still runs a full major, as `GcMode::Off` does today | the differential reference run |
| null | `PATINA_GC=null`: no automatic trigger, `(gc)` a no-op, `BarrierKind::None`; a run that reaches `max_heap` fails | measurement only: the lower bound (JEP 318) |
| stress | `PATINA_GC_STRESS=n`: a collection every n allocations until 5e, every n polls after | differential lane |
| zeal | `PATINA_GC_ZEAL=major\|minor\|alternate\|move-all\|entry` (`jit-invalidate` once a JIT exists), `PATINA_GC_VERIFY=1`, `PATINA_GC_VERIFY_ROOTS=1` | torture lanes |
| debug-poison | debug builds: eager poisoning sweep, hole quarantine, `DEAD_SLOT` fill | use-after-free detection |
| two-mutator | `PATINA_GC_MUTATORS=2`: two `Mutator`s alternate deterministically on one OS thread | N-readiness lane, generalized at stage 9 into a seeded multi-carrier simulation |
| GC workers | `PATINA_GC_WORKERS=n` (stage P); 1 in the deterministic lanes | large live heaps |
| standalone | `Heap::new_standalone()` | `patina-compat`'s reader |

Today's mark-sweep is not ported; its oracle role passes to the off mode, the verifier and, during stage 5, the arena
build itself. **Future collectors plug in** as follows. A **copying nursery** ([K4](#k4)) is a `NurserySpace` composed
into `MarkRegion`, never armed, promoting in place when survival is high or to-space short. **Incremental marking**
(only on a latency goal) uses `barrier_mode` in slow paths, so it needs `GcAttrs.barrier = GranuleLog` from creation on
every heap that may run it (a whole-heap heap emits no barrier whose slow path could switch, §10), and a rule for sticky
minors during an in-progress major, which share the `STATE` epoch (§9.2); SATB would also need `value_filter_bit = None`
and a JIT recompile. **Parallel stop-the-world marking** (stage P) uses per-heap GC workers, their number drawn from the
heap's memory budget (§17.3), spawned above a live-size threshold, CAS marking on the metadata byte during GC only, work
stealing and a sharded ephemeron fixpoint; roots stay on the collecting thread, evacuation destinations stay sequential,
and a `workers=4` lane must match the single-threaded lanes byte for byte (DESIGN H.4). An **MMTk-backed** collector is
not adopted (decision 14).

**Explicitly excluded:** load and read barriers; conservative scanning of VM, JIT or Rust stacks; collection inside
allocation or barrier slow paths, or reachable from a `Cx`; `dyn` on fast paths; patching machine code at GC, or
anywhere outside `install_code`; process-wide GC singletons (a `MemoryBudget` that several heaps share is passed in by
the embedder, §17.3); Java-style finalizers; Cranelift user stack maps, frame walkers and `stack_switch`; raw slots in
published frames, and native frames holding Scheme values across a suspension point (native call/ret needs its own
design, [K11](#k11)); embedding the `Mutator`, or an address outside §11.2's four cases; racing parallel evacuation;
more than one production collector.

## 15. Heap sizing, pacing and observability

**Triggers are in bytes**, counted at refill and at LOS allocation, plus **external bytes** (Chez's phantom bytes,
OCaml's `caml_alloc_custom_mem`): 8 KiB per open, unclosed file port; the Rust size of code bodies, macro bodies and JIT
code; tree-walker environments and namespaces held by registered host payloads; string-port buffers; what an embedder
reports through the accounter. Inputs are bytes and counts, never time, so pacing is deterministic. L is the live bytes
after the last major plus LOS and external bytes (M3).

| Case | Pacing |
|---|---|
| Whole-heap heaps (stages 5–6, tree-walker heaps for good, generational heaps in bypass) | the next major after **`max(8 MiB, 2·L)`** bytes of allocation, keeping the peak near 3× live. Racket CS's `L + 8192·√L` rule sits on an 8 MB-per-place nursery (`racket/src/cs/rumble/memory.ss:35-39` [S]); without one it would mean 1.15–2× more full marking than 2·L on the large-live GBS workloads, and its 32 MiB first major would lift small programs from 15–23 MB peaks to about 45 MB [P] |
| Generational heaps (stage 7 on) | majors at `L + max(8 MiB, min(8192·√L, 2·L))` only once an interleaved A/B shows it beats 2·L (about 1.8× live at peak at 100 MB, 1.26× at 1 GB); a fixed 4 MiB nursery budget (about 150 K objects [P]), adapting only through the deterministic bypass (§9.2) |
| Near the ceiling | the target never exceeds `max_heap` minus the emergency reserve, a shared `MemoryBudget`'s remaining total (§17.3), or the opt-in soft target (SD8); collections become more frequent instead (§17.3) |
| Adaptive pacing, opt-in (`PATINA_GC_PACING=adaptive`) | a MemBalancer major target `L + clamp(c·√(L·g/s), 0.25·L, 3·L)` (g the smoothed promotion rate, s the marking speed; EWMA with 10% hysteresis, because resizing at every GC is "giving control of your stereo's volume knob to a hyperactive squirrel", https://wingolog.org/archives/2024/09/18/whippet-progress-update-feature-complete), and a nursery sized toward the minor budget; never in test lanes, and the default only if it wins an A/B on RSS at constant GC time |
| Free reserve | after a major at least `max(8 blocks, 5%)` empty blocks, or the heap grows; two consecutive majors that each free under 1% raise the target ×1.5 (Wingo's livelock fix), and the boost decays at the first major that frees more than 10% (M5). Idle trim: §17.1 |

**Observability (stage 0 on).** Per collection, `GcStats` records kind, reason and `complete`; phase times; bytes
allocated, promoted, marked and freed; block counts, fragmentation and occupied against marked bytes; store-buffer
entries, range entries and logged granules; pinned blocks, mark-stack peak, frames scanned, deferred polls, buffer
discards and the in-pause work counters; **[K16](#k16)'s high-water marks** (bytes allocated between a posted event
and its servicing poll, and inside one `NoGcScope`, each with its site) beside a time-to-safepoint metric. Outside the
pause it logs finalizers, catch-up sweep, classification, LOS release, decommit, refill time, watermark cold returns,
idle majors and time in bypass. Cumulatively: the GC-time fraction, per-workload max pause for minors and majors, and
an **MMU** from every non-mutator interval at 1–100 ms windows (Larceny `gc_mmu_log.c`), kept in a ring buffer per
window; comparisons use per-workload max pause and MMU(10 ms), never pooled percentiles. Outputs: `PATINA_GC_LOG`
(CSV), `PATINA_GC_TRACE` (JSON phase events for the debugger hook system), and representation-independent
`(gc-stats)` keys from stage 1 (`live-bytes`, `committed-bytes`, `bytes-allocated`, `bytes-reclaimed`, `collections`,
`minors`, `majors`, `pause-max-us`, `mmu-10ms`, each limit beside its use), plus §17's per-owner, footprint and budget
keys.

## 16. Testing and verification

**Differential lanes** extend `scripts/run_gc_differential.sh`: every mode (off, default, stress, zeal, move-all,
two-mutator, GC workers) must be byte-identical to off on the chibi suite (`EXPECTED_TOTAL` 1226 [S]), in release and
debug-poison builds, on both backends wherever the mode applies; port-finalization and limits tests stay outside. Each
lane, named test and rewritten reclamation proof joins `docs/TEST_ORGANIZATION.md` in the stage that creates it
(drafts: DESIGN Appendix F). **Reclamation proofs** assert representation-independent keys (`live-bytes` after a full
`(gc)`, `committed-bytes` across churn, `collections`, and `bytes-reclaimed` > 0 against vacuous passes), from stage 1.

**The heap verifier** (`PATINA_GC_VERIFY=1`, before and after each collection under zeal) checks every heap-bit word
against a start granule of a matching kind, `END` bits against sizes, invariant W, remembered-set completeness (every
old granule holding a young reference is logged, in a range, or on the remember-whole list; cells armed after a
generational major), no young reference below a watermark and each thread's watermark frame still carrying its `WM` flag
or trampoline, a map at every frame pc, no reachable `FORWARDED`, `GC_POISON` or `DEAD_SLOT` word, `live_granules`, code
units, `HASH_MOVED` extensions and the LOS young list; `PATINA_GC_VERIFY_ROOTS=1` also checks every `RootScope`, `Owned`
and `RootProvider` word, and that every old host payload reaching a young referent has rejoined `young` (§14's `store`).
**Poison under lazy sweep:** from 5b the debug-poison and zeal lanes add an eager metadata-driven poisoning sweep, a
2-collection quarantine of freed holes, `PROT_NONE` on wholly free 4 MiB runs in single-interpreter zeal lanes, the
accessor poison assertions and the `DEAD_SLOT` fill, so a stale reference panics instead of reading reused memory
([#605]); these lanes compare program output, never identity-hash values. **Miri** runs on `patina-gc` and a
`patina-core` subset (funnel, `HeapSlot` slices, slot visitor, `Mutator` access, `RootScope`/`Owned`, the JIT-entry
handoff on the Rust twins), with strict provenance.

**Detection on today's collector.** Until stages 2–5 replace them with handles, the brand, generated traces, the
verifier and the poison lanes, these run on the arena collector, each detector shown to fire by a test of its own so
that a lane cannot go quietly blind:
- [#621]: every stale reference panics in debug and `gc-check` builds (a free slot reached by marking, a freed-slot
  check in every accessor, generation stamps in the value word), and the release GC lane runs the `gc-check` build at
  stress 1. The verifier (5a) and 5b's poisoning sweep, quarantine and `DEAD_SLOT` fill supersede it.
- [#622]: every Rust re-entry into the evaluator under a clippy rule, each existing call carrying its reason; the list
  is what stages 2 and 4e convert, and stage 3's brand supersedes it in the safe crates. It also brings C9's
  `thread_local!` check forward (§18.6).
- [#623]: every traced field named in trace code and tested with a sentinel reachable only through it, until the
  `Trace` derive (§14) for off-heap structures and `declare_layouts!` (5a) for heap kinds.
- [#624]: the deferral protocol asserted, and `AssertNoGc` around the windows only comments hold, checked at the poll
  sites, until stage 3's `NoGcScope` and zeal-entry.
- [#625]: retired VM registers filled with `DEAD_SLOT` and checked at every read and copy (§11.1's invariant 3, brought
  forward), with a zeal lane over the control suite.
- [#626]: more programs under stress: the GC- and control-relevant `cargo test` targets per PR, both Larceny lanes
  nightly in the `gc-check` build.

**Contract tests:** `Mutator` offsets, alignment and `GcAttrs`; emitters against their Rust twins once the spike exists;
a call-graph test of the helper table (no `Leaf` reaches a `Transfer` function, no `NoAlloc` an allocation); a host
`Leaf` primitive that allocates, called from JIT code, leaving the heap verifier clean (§11.5); `install_code` refusing
a body compiled against other `GcAttrs` (§14); embedded addresses limited to §11.2's four cases; no `Drop` in layout
payloads; trybuild tests of the brand; the conformance suite (§14); the `InterruptHandle` tests; a CI check against new
`thread_local!`, `static mut` and heap-side `Rc`/`RefCell`/`Cell` (§18.6). **Scoreboards** gate every stage: both chibi
scripts; the matrix on both backends with `escape_from_primitive.rs` and the raise-site tests; the hygiene matrix, with
§17.4's `def-getter` maker; `ephemerons.rs`, `finished_forms_release_code.rs`, `gc_vm.rs`, `gc_tree_walker.rs`,
`unclosed_output_ports.rs`; `vm_callprimitive.rs` with its two set-after-use tests;
`crates/patina-tests/tests/scheme/control/parameters.scm`; the error-location tests in `interpreter_api.rs`;
`run_suite_oracles.sh` with `DIVERGENCES.tsv`; both Larceny lanes; `patina-compat check-smoke`;
`check_embedding_features.sh`; the steady-state lane's per-PR rows.

**The GC benchmark set (GBS):** twenty measured workloads in eight groups (allocation, mutation, large live heap,
flonum, continuations, deep recursion, library loading, supplementary), most Larceny-derived and so run from
`~/Project/reference/larceny` (LGPL, not vendored, skipped loudly when absent), plus vendored Patina-authored probes
(among them `large-live` at 0.5 and 1 GiB live, `deep-descent`, `deep-unwind`, `samedepth1000`,
`retained-continuations`, `frag-mix`, `port-churn`, `blocked-threads`), the barrier programs, fixnum twins, I/O
workloads and a tree-walker subset, run through a `gc` mode of `scripts/benchmarks.py`; the list lives in
`docs/TEST_ORGANIZATION.md`. **Statistics:** ABA ordering over at least 10 rounds; thresholds under 2% judged in
instructions retired or cycles, never wall time; bootstrap 95% confidence intervals, a gate passing only if its interval
clears the threshold; `PATINA_GC=null` as the lower bound.

**Named experiments.** **The barrier-tax experiment:** the barrier armed with minors off (every object treated as old,
bits re-armed and the buffer drained at each collection) against `BarrierKind::None`, in instructions, on the barrier
programs and the GBS; the bet is under 1% ([K2](#k2)). **The generational experiment:** generational on; barrier on
with minors off; barrier off and whole-heap; each with the bypass on and off, on the GBS and the tree-walker subset.
Generational must win on throughput, or on per-workload max pause and MMU(10 ms) without losing throughput, to become
the default ([K1](#k1)).

**Each stage proves itself** by filing its issue first, keeping every lane and scoreboard green on both backends,
showing its acceptance numbers by interleaved A/B with confidence intervals, keeping the verifier clean under zeal, and
recording any behaviour change as an oracle-scored `DIVERGENCES.tsv` row.

## 17. Steady state, the memory contract, and limits

*Implements decision 15's values and SD1–SD9 (proposed).*

A program with bounded live data should never hit a limit; one that really grows should hit one and be told what
grew. Patina fails steady state today in the ways listed in §17.5; the design as first drafted would have made several
of them permanent. Detail and raw numbers: `PRD/study/gc/followup/steady/`
([`SECTION.md`](study/gc/followup/steady/SECTION.md), the audit `audit.md`, the limits survey `prior-art.md`).

### 17.1 Definition

| Term | Definition |
|---|---|
| Cycle; phase | a cycle is one iteration of what the program repeats: a request, a REPL form, an `eval`. A phase is a run of cycles doing the same work while the program's own live data (what its stacks, handles, ports, threads and nameable bindings reach) stays bounded; lane programs bound it by construction |
| L | `live-bytes` after a major: marked bytes, plus LOS bytes, plus external bytes (§15). `PATINA_GC_LOG` records it at every major; a window's L is its maximum |
| Footprint | `committed-bytes` (committed data blocks and LOS runs; arena capacity before 5e) plus `external-bytes`. Metadata and the block table, 1/16 of the data, are reported apart as `metadata-bytes`; `max_heap` counts all three |
| Warm-up | the first N cycles of a phase, excluded from every comparison. N is set per row so that the plateau has begun (Chez's slope probe rises from 102 MB to 124 MB before a plateau that holds for 16× more work) and, for rows that test SS2 or SS3, so that warm-up has seen 3 paced majors |

A phase is in **steady state** when these clauses hold over the window from cycle N to 4N. Values are read at the
window's two ends right after two forced `(gc)` calls, which collect at their call from stage 1, so the readings are
deterministic.

| # | Clause | Test | Failing today |
|---|---|---|---|
| SS1 | L follows the program, not its history | L(4N) − L(N) ≤ 3N·8 B + 64 KiB | symbol churn: 192 B per symbol [P] (chibi 166, Chez 0) |
| SS2 | Footprint ≤ F(L) | at every paced major in the window, footprint ≤ F(L) = 1.1·(L + max(8 MiB, 2·L)) + free reserve | the `environment` loop ([#615]) |
| SS3 | Pause work stationary | in-pause work counters (frames scanned, bytes marked, root-region words, blocks touched): the second half's maximum ≤ 2× the first half's | pauses after a dropped peak ([#616]) |
| SS4 | Operation cost flat | mutator CPU time per cycle over [3N, 4N) ≤ 1.5× that over [N, 2N), each segment the minimum of 3 runs | macro-introduced definitions ([#613]) |
| SS5 | Side structures bounded | each per-owner count in `(gc-stats)` at 4N within max(16, 5%) of N; byte-valued keys under SS1's bound | `symbols` per top-level `guard` ([#611]) |

Lane rows are classed by the audit's failure shapes: **A** a true leak (a fixed amount per cycle, for ever), **B** a
plateau set by a trigger blind to some bytes, **C** high-water retention (memory or pause cost of a peak never given
back), **D** live growth (a growing name set: outside the contract, kept as a cost regression check), **E** time
growth. SS1's bound catches a 32 B-per-cycle leak (one symbol in the new heap) from N ≈ 1,000, and today's smallest
class-A leak (192 B) with 24× margin. SS4's bound sits between flat cost (1.0) and cost proportional to history (2.33,
the shape of every class-E row; `hidden-define` fits 2.0 at N = 2,000 [I]); timing one process's loop excludes
start-up and library load. In F(L), 1.1 is [K3](#k3)'s pre-stage-8 fragmentation allowance and the free reserve is
max(8 blocks, 5%). A row that declares SS2 or SS3 needs 2 paced majors in each half-window, or it fails as vacuous, as
`bytes-reclaimed` > 0 guards the reclamation proofs.

**Phase change.** Suppose live size falls from L to L′, because a peak was dropped or a deep recursion returned:

| Rule | Content |
|---|---|
| Pause | the first major after the drop does in-pause work within SS3's budget for L′ |
| **Empty memory returns** | runs of blocks and LOS runs that the drop left empty are decommitted within 2 majors, or at one idle major (below); empty blocks inside runs that still hold a survivor are decommitted one by one, data pages only; register stacks follow after 2 observations. Footprint is then ≤ F(L′) + `sparse-bytes`, the committed bytes of non-empty blocks under 25% live |
| **Partly occupied blocks are reused, not returned, until stage 8** | a non-moving heap cannot give back a block that one survivor holds. From stage 8 the footprint trigger (§9.4) evacuates the sparsest unpinned blocks whenever footprint − F(L) exceeds max(64 MiB, 25%·F(L)) at 2 consecutive majors; the bound becomes F(L′) plus that excess plus pinned blocks |
| **Idle trim** (SD7) | a drop is invisible until something marks, and a program that idles after a peak allocates nothing that would pace a major, so entering a wait runs one **idle major** when footprint exceeds 64 MiB and the back-off allows. It completes its post-pause work (catch-up sweep, classification, decommit) before the wait, without the per-poll rate limit and with the 2-major hysteresis waived; recomputes the target from L′, as any major does; and neither counts toward nor resets the livelock boost or the progress guard |
| Back-off | at most one idle major per paced cycle. One that returns less than max(64 MiB, 25% of footprint) doubles the number of paced majors before the next (1, 2, 4, …); the count resets to 1 when an idle major returns that much, or when a paced major's L exceeds 1.5× the L at which the count last doubled. A program whose L never drops pays at most log₂ P + 1 useless idle majors over P paced ones |
| Waits | from 5e: the REPL prompt; `notify_idle()`, and `(notify-idle)` in `(patina debug)`; entry to a read primitive whose buffer is empty, on a port that is not a regular file, through `Step::Collect` before anything is consumed. From stage 9: a thread wait with nothing runnable, and a read that blocks mid-datum once the reader lexes before it builds. Where collection is deferred, no trim runs. Under `PATINA_DETERMINISTIC=1` only the prompt and `notify_idle` qualify, because whether a pipe's buffer is empty depends on how the writer's bytes arrive. G1 used a timer for this case (JEP 346); Patina counts waits |
| Boost decay | the livelock boost (×1.5 after two majors that each free under 1%) decays: at the first major that frees more than 10%, the target is recomputed from 2·L |

**Outside the contract:** growing name or live sets (class D: a fresh top-level name costs 365 B [P], the oracles
400–484; the lane fails a 20% regression); weak tables keyed by fresh symbols (class D: an ephemeron keeps a symbol key
alive, decision 11, §9.5); threads blocked for ever (decision 23); partly occupied blocks before stage 8; `NoGcScope`
windows, which may overshoot F(L) by [K16](#k16)'s bound.

### 17.2 The memory contract

| Rule | Content |
|---|---|
| M1 | the immortal space holds only objects bounded by the binary and the program text: primitives and their descriptors, canonical flonum boxes, core syntax, and a future boot image's immutable objects that reference nothing mortal; the image's cells, binding records, parameters, promise boxes and tables, and the objects that reference them, are allocated old in the NMS (`alloc_old`, remember-whole), because nothing scans the immortal space. Everything created at run time is mortal (SD3). Objects that must not move go to the non-moving space, which majors sweep and which is never evacuated |
| M2 | every Rust table keyed by name, id or address is bounded by the program text, or loses entries when their heap object dies (finalization or epilogue step 6); the holder inventory (DESIGN E.1) names each owner |
| M3 | Rust bytes that a heap object keeps alive count as external bytes while the object lives: in L, in the trigger, and in `max_heap` (SD4) |
| M4 | no lookup scans a table that grows with history |
| M5 | empty memory returns: empty runs within 2 majors (one at an idle trim), register-stack pages after 2 observations; partly occupied blocks are reused for allocation and, from stage 8, evacuated by the footprint trigger; boosts to the pacing target decay |

| Source | Guarantee and the design change it needs | Today | Stage |
|---|---|---|---|
| Symbols | Reclaimed when unreferenced (SD2). Allocated old in the NMS, so only majors reclaim them, and the interner is pruned only in the major epilogue (step 6). The stored hash is a fixed-seed hash of the UTF-8 name, so a re-interned symbol hashes the same; `identity-hash` of a symbol changes from the heap index to that hash in its own PR (§9.8). An ephemeron keeps a symbol key alive, so symbol-keyed weak tables answer as today (§9.5) | 192 B per symbol [P] | 5c |
| Global cells and binding records | Owned by namespaces; global and library namespaces are roots; placeholders weak; aliases bounded by program text; introduced definitions strong at 4b with an indexed lookup, reclaimed from 5c under §11.6's two tests; cells stop being a root region (§9.1), so pauses stop growing with cell history. Variant C keeps this | [#611]; [#613] | [#611] now; 4b own; 5c sweep |
| Environment specifiers; redefined libraries | A namespace is a charged host payload that dies with its specifier or last unit (specifiers are immutable, so nothing defines into them); `define-library` forms get a collection point. A macro that a library's generator defines holds the generator's environment in `foreign_expansions`, traced since [#623], so the environment and its values live as long as the macro: 4b counts that edge among a replaced namespace's holders, or replaces it with one that keeps only binding locations, all that early binding reads through it | [#615]; [#614] | 1 charge; 2 collection point; 4b |
| Provenance; scope sets | A document owns its location table and compacts it while streaming; expansion chains are interned per document; the scope-set table is weak (ids recycled) or scoped to one form, and reports which expansion scopes live identifiers still carry | [#612] | 1 chains; 4c |
| `CoreExpr` literal pool | Scoped to one compilation (§11.3) | — | 3 |
| Descriptors, record types | Mark-region with hole reuse inside the NMS; [K3](#k3) reports it | steady | 5c |
| JIT code | Freed per unit, through size-segregated slabs and a coalescing list; fragmentation shown in `GcStats`; capped; gated by `eval-lambda` and `eval-redefine`. Stage 6 is a spike on an unmerged branch, so it freezes only the interface: `install_code` and per-unit free | — | 6 interface; the JIT track's first merged stage (decision 20) |
| JIT embedded addresses | §11.2's fourth case: an NMS object that the unit's constants or link table reference may be embedded; under R a cell behind a re-pointable record stays under `WATCHED` | — | 5c, 6 |
| Heap blocks, LOS cache, mark segments | Empty runs returned within 2 majors (one at an idle trim); empty blocks in partly occupied runs decommitted one by one; partly occupied blocks reused, then evacuated by the footprint trigger (M5) | [#616] | 5e, 8 |
| Green-thread stacks | Fixed-size, never relocated, lazily committed, trimmed by page and charged, under a per-heap bound on stack address space; whether each thread gets its own reservation or a slot in a chunked one is decided by stage 9's probe (§18.1's stack rule). No guard page, since every push passes the frame-entry check (§11.1) | — | 9 |
| Ports | The `PortTable` registry prunes dropped heaps | steady | 4a |

The design also fixes the failures that §17.5 lists without an issue, and those of [#604] and [#606]. Slot tables keep
their peak length (4–16 B per slot).

Each per-owner key arrives with its table: `symbols`, `namespaces`, `cells`, `binding-records`, `code-units`,
`descriptor-bytes`, `jit-code-bytes`, `external-bytes` by kind, `scope-sets`, `documents`, `ports` and `threads`.
Alongside them `(gc-stats)` carries `metadata-bytes` and `sparse-bytes`; the in-pause work counters; from stage 0
`resident-bytes` and `cpu-us`; from 5e `memory-budget`.

### 17.3 Limits

A steady program stays under `max_heap` while F(L) plus metadata fits under `max_heap` − 4 MiB reserve. That allows L
up to about 27% of `max_heap`, or 4.3 GiB at 16 GiB. Beyond that the target clamps and majors come more often.

**Memory budget.** Defaults derive from B = min(physical RAM, the cgroup limit: v2 `memory.max`, v1
`memory.limit_in_bytes`), never from host RAM alone (.NET uses 75% of the container limit). When `RLIMIT_AS` is set,
the main stack, the store buffers and the JIT reservation are sized first and the heap reservation gets what remains;
`max_heap` is clamped to that reservation; startup falls back to smaller sizes rather than failing. B is detected, and
the main stack fitted under `RLIMIT_AS`, at stage 4d; the heap ceiling is fitted at 5e.

**Process budget** (5g). `HeapConfig` takes an optional embedder-owned `Arc<MemoryBudget>`. Every heap created with the
same budget draws its `max_heap`, register-stack reservations, JIT code and GC worker count from it: the B-derived
defaults (75%·B, 25%·B) apply to the budget's total, `RLIMIT_AS` fitting is against the budget's remaining address
space, each heap's target is also clamped by the budget's **remaining total** (its total less what the budget's other
heaps hold), and the heap whose allocation would pass the total follows the steps below. Without one, each interpreter
has its own budget, as now. It is not a singleton (§14): an embedder that wants one budget per process, as isolates do
(§18.1), passes the same `Arc` to each interpreter.

| Limit | Counts | Default | Knobs | At the limit | Stage |
|---|---|---|---|---|---|
| Hard heap | committed data, LOS, metadata, external bytes (SD4) | min(16 GiB, 75%·B) | `PATINA_HEAP_MAX`, `--heap-max`, `HeapConfig::max_heap` | the steps below | 5e, 5g |
| Heap ceiling | the heap's reservation | max(`max_heap`, the default `max_heap`), within `RLIMIT_AS` | `HeapConfig::heap_ceiling` | `max_heap`, and the embedder's raise of it, stop here | 5e |
| Soft heap | as the hard heap | off (SD8) | `PATINA_HEAP_SOFT_MAX`, `--heap-soft-max`, `HeapConfig::soft_max_heap` | clamps the target and never fails; lifts after 5 consecutive majors it clamped that each free < 2% | 5g |
| Register stack | one thread's frames | main: min(8 GiB, 25%·B), which is 1.75 GiB on a 7 GB runner and still holds the VM's 10 M frames (1.37 GB [P]); 256 MiB per green thread | `PATINA_STACK_MAX`, `--stack-max`, `HeapConfig` | `&stack-exhausted` within 1 MiB of the cap | 4d |
| Stack address space | the green-thread stack reservations of one heap | set at stage 9 with §18.1's stack rule | — | `thread-start!` raises, as the thread limit does | 9 |
| Threads | started, unterminated | charged stacks under `max_heap`; one `--stack-max` slot of address space each | optional `PATINA_MAX_THREADS` | `thread-start!` raises `&heap-exhausted` | 9 |
| Native (Rust) stack | Rust recursion in the reader, expander, compiler and printer | the OS thread's stack (the main thread's `ulimit -s`; carriers spawned at the same size) | — | a catchable error from a depth guard; today a process abort ([#617]) | now |
| Descriptors | open file ports | `RLIMIT_NOFILE` | OS; lanes pin it | post a major when opens − closes since the last major reach min(128, limit/4); on `EMFILE`, collect and retry once, then a file error ([#607]) | 1, 4a |
| JIT code | installed bodies | set by the JIT track's first merged stage | `PATINA_JIT_CODE_MAX` | stop tiering up; no error | first merged JIT stage |
| Address space | heap reservation (from the ceiling); main stack; 256 MiB store buffer per mutator; JIT reservation; green-thread stacks | fitted under `RLIMIT_AS` | — | smaller reservations, then [K13](#k13)'s fallback; the store buffer's soft limit collects first | 4d, 5a, 5e, 7, 9 |

**At the heap limit:**

| Step | Rule |
|---|---|
| 1. Target and immediate raise | the target never exceeds `max_heap` − reserve, nor a shared budget's remaining total. A user-sized request raises at once only if it exceeds `max_heap` − reserve − the uncollectable floor (the immortal space, metadata and block table, and the stacks of started threads), as Racket raises at once only for a request at least as large as the whole custodian limit, or one whose size is not a fixnum (`racket/src/thread/custodian.rkt:686-700`, `racket/src/cs/rumble/memory.ss:223-232` [S]). Every other failed request takes the single collect-and-retry of §8 step 6 |
| 2. Maximal major | before raising, one major also drops caches (expansion memos, unreachable units, string-port slack) and, from stage 8, evacuates, as G1 and V8 do |
| 3. Progress guard (SD6) | 5 consecutive *counted* majors that each free under 2% of `max_heap` raise at the next poll. A major is counted when allocation reached a target clamped by `max_heap` or by a shared `MemoryBudget`'s remaining total, not by `soft_max_heap`; forced `(gc)`, idle, descriptor-pressure, retry and maximal majors neither count nor reset the run; any major that frees at least 2% resets it. These are HotSpot's overhead-limit counts without the time term. chibi has no guard: at its cap it ran at 100% CPU until killed after 30 s [P, chibi 0.12] |
| 4. The condition | `&heap-exhausted` is catchable; its payload holds the limit, the managed bytes and the per-owner key that grew most since the previous major; uncaught, a non-zero exit (SD9). If the reserve runs out before a poll, the runtime flushes ports and aborts; to keep that rare, a driver about to enter a known `NoGcScope` site (an import met mid-form, a tree-walker library body) first runs a major when headroom is below that site's [K16](#k16) high-water mark |
| 5. Handler room | a test proves that a handler for either condition fits in the reserve; if one does not, the runtime unwinds without running handlers, as Lua and Guile do ([K19](#k19)) |

chibi thrashes and Gauche aborts (status 1) at their limits, so these tests stay outside the byte-identical lanes.

**Embedding hooks** are in §11.5 (a near-limit callback may raise `max_heap` only up to the heap ceiling and, under a
shared `MemoryBudget`, no further than the budget's remaining total, as V8 raises only inside its pre-reserved cage).
**Placement:** no new top-level stage; sub-stage 5g (3–4 engineer-weeks [I], about 1 of them the process budget, this
file's estimate) and items in named stages (about 2–3 more [I]), listed in §19.

### 17.4 Verification

**The steady-state lane** (stage 0, `crates/patina-tests/bench_programs/gc/steady/`; Chez variants for scoring):

| Group | Rows |
|---|---|
| REPL-style | redefinition streams; the `guard` stream run from a file, from stdin and in REPL `-i`; `eval-redefine`; `eval-lambda`; `macro-eval` (`case`, `guard`, the standard macros); `hidden-define`; `unbound-ref`; `load-repeat`; `reimport`; library redefinition |
| Server-style | `steady-alloc` (a 20 K-slot ring); `peak-then-drop` with `pause-after-peak`; `peak-then-sparse` (keeps every 4,096th vector of the peak); `peak-then-idle` (one `(notify-idle)` after a drop); `deep-then-steady`; `port-churn`; `file-port-churn` |
| Churn | `sym-churn`; `eval-fresh-names` (class D); `env-churn`; `env-lambda`; `form-eval-fresh`; `record-redefine`; `cont-churn` at depths 10, 100 and 1,000; `interp-churn` (Rust) |
| Threads (stage 9) | `thread-churn`; `blocked-threads`; a 100 K-thread probe on Linux |

**Protocol.** Each run is one process to 4N cycles. At N and at 4N the probe calls `(gc)` twice, then reads
`live-bytes`, footprint, the per-owner keys, `resident-bytes` and `cpu-us`; the segments [N, 2N) and [3N, 4N) are
timed by `cpu-us` less in-pause time. Every row runs 3 times, on both backends; N is set per row from a time budget,
and each row prints the smallest leak it can detect, 64 KiB/3N + 8 B per cycle. **Before stage 1** (no `live-bytes`,
and `(gc)` not yet collecting at its call) the lane reads `resident-bytes` (`/proc/self/statm`, or `task_info`'s
footprint) after a `(gc)` at N, 4N and 16N, takes the median of 3 runs and fits a slope, with a floor of max(4 MiB,
10%). Max RSS (`ru_maxrss`) varies by 3 MiB for identical work, so it never gates. **The runner** is portable
(per-child rusage from `os.wait4`, units normalised); a missing measurement fails the row; every probe runs under a
timeout and a memory cap (`--heap-max` from 5e, a resident-size watchdog before), and a cap hit is red; rows red today
run capped, at a reduced N, until their fixing stage. **Cost:** 0.6–380 µs per cycle on the VM and 7.5 ms per
`interp-churn` cycle [P]; at N = 2,000 (100 for `interp-churn`) about 3–4 minutes on the VM and 6–8 on the tree-walker
[I], so the full lane runs nightly and each PR runs the rows under 2 s, about a minute on both backends. **Not gates:**
wall-clock max pause, MMU(10 ms) and RSS series are nightly signals (median of 5 runs; differences under 1 ms ignored).
**Oracles** are context: the target is no worse than the best oracle that is steady on the row.

**Hygiene.** Stage 0 adds a hygiene-matrix maker whose introduced definition only macro templates reach: the
`def-getter` shape, `(define secret 42)` beside a getter macro and a setter macro, with no getter procedure. It prints
7 today on both backends, on chibi and on Gauche; from 5a it also runs under zeal-major, so reclaiming introduced
definitions at 5c cannot drop a binding that a template still reaches.

**The limits lane** (5g, outside the byte-identical lanes):

| Test | Pass condition |
|---|---|
| Step test (Go `TestMemoryLimit`) | L at 10–80% of `--heap-max`; committed ≤ `max_heap`; nothing raised while F(L) fits |
| Return to baseline (ZGC `TestUncommit`, in majors) | small, medium and LOS spikes, twice, return to F(L′); a sparse variant (one object kept per MiB of each spike) judged against F(L′) + `sparse-bytes` until stage 8, against stage 8's bound after |
| Idle in a real blocking read | the harness holds a pipe open and samples footprint and resident size from outside: ≤ F(L′) + `sparse-bytes` |
| Overhead limit (HotSpot) | exhaustion within 6 counted majors of the clamp, never a hang; L at 40% of `--heap-max` with 10 `(gc)` calls raises nothing |
| `large_request_after_garbage` | includes a drop-then-allocate case under `--heap-max 1G`: a 700 MiB table live at a major, then dropped, then `(make-vector 60000000)` succeeds |
| Allocation-failure zeal (Lua `EMERGENCYGCTESTS`); a downward `--heap-max` sweep over the chibi suite | to a documented floor per backend (the startup peak plus the largest [K16](#k16) window): above it, normal output or a clean `&heap-exhausted`; below it, only the abort diagnostic naming a `NoGcScope` site |
| Handler room | passes on both backends |
| `descriptor_exhaustion_retries` | passes on both backends |
| 10 M-deep recursion at the default cap | passes on the VM; the tree-walker runs 1 M frames (10 M would need about 9.6 GiB) |
| Budget detection | under a cgroup `memory.max` and under `ulimit -v`, startup succeeds with smaller reservations and `memory-budget` reports the limit |
| Several heaps, one budget | four interpreters sharing one `MemoryBudget`, one of them growing: it raises `&heap-exhausted` while the process's footprint stays under the budget's total, and the others run on; under `ulimit -v`, every heap starts |
| Determinism replay | two runs on one machine, on Linux and on macOS, give the same collection kinds and `live-bytes` per major; across the two systems only with the JIT off, from one working directory, with `--heap-max`, `ulimit -n 1024` and `PATINA_DETERMINISTIC=1` pinned, as in every lane that compares collection sequences |

**Soak:** from stage 5, a nightly hour of a REPL-like loop, slope-tested per major. Kill criteria
[K17](#k17)–[K19](#k19) are in §20.

### 17.5 Present-day failures

This is the one list of the ways Patina fails steady state today. A defect has a GitHub issue carrying its symptom,
repro and fix direction; the rest have no issue because the design fixes them at the stage named.

| Failure | Issue | Fixed at |
|---|---|---|
| A library macro with a private helper interns a new alias per top-level expansion | [#611] | now |
| Re-evaluating a quoted `case` datum with `eval` grows memory and time without bound | [#612] | 1, 4c |
| References to a macro-introduced top-level definition scan every earlier expansion | [#613] | ≤ 4b (indexed lookup) |
| A redefined library stays alive; a run of `define-library` forms never collects | [#614] | 2, 4b |
| A loop of `(environment '(scheme base))` plateaus near 2.9 GiB | [#615] | 1, 4b |
| After a dropped peak, every collection keeps the peak's sweep cost and the peak's memory stays resident | [#616] | 5e |
| Continuation churn plateaus, because the trigger does not see capture bytes | [#606] | 1, 4e |
| Dropping an interpreter leaks its heap | [#604] | 2 |
| Unreferenced symbols are never reclaimed: 192 B per symbol [P] (chibi 166, Chez 0) | no issue: fixed by design | 5c |
| Register stacks keep a deep recursion's pages: 143 MiB on the VM and 984 MiB on the tree-walker after 1 M frames [P] | no issue: fixed by design | 4d |
| Tree-walker environments are freed only when sweep drops their procedure, and the trigger sees them as an estimate, one frame a closure: frame chains, and frames only a continuation holds, are missed | [#637] (the estimate) | 4f |
| Tree-walker continuation captures are invisible to the trigger: a `call/cc` loop at depth 1,000 peaks at 2.4 GB | [#656] | 4e, 4f |
| On the tree-walker, a top-level variable assigned a literal keeps its previous value reachable | [#655] | a defect, fixed on its own |

## 18. Threading readiness and future parallelism (decision 7)

*Implements decisions 7 and 7c (proposed).*

**Status.** Decided: SRFI 18 ships as M:1 green threads, VM first, and the GC's interfaces are written for N carriers
over one heap. **Proposed, pending owner review: shared-memory parallelism is not a goal now**; this section records
what it would cost later and when to start. Parallel stop-the-world marking (stage P) needs GC worker threads, not
mutator threads, and does not depend on this decision. Detail: `PRD/study/gc/followup/parallelism/`
([`ANSWER.md`](study/gc/followup/parallelism/ANSWER.md), `itemize.md`, `measure.md`, `prior-art.md`) and
`PRD/study/gc/research/threads-*.md`.

**Bottom line** (`followup/parallelism/ANSWER.md` Part 1). Expect years, not months. After the redesign the itemized
work is 9–18 engineer-months; other runtimes' histories say to plan on two to four times that, plus a recovery phase,
about 2–9 engineer-years in all. The range is wide because the overrun factor rests on one quantified retrofit.

| Starting point | Work [E] | Confidence |
|---|---|---|
| Today's code, before or instead of the redesign | 79.5–145.5 weeks, **18–34 engineer-months**; plus 12–19 weeks for the SRFI 18 green threads it builds on | low |
| After redesign stages 0–9 and P | 38–77 weeks, **9–18 engineer-months**; plus 6–11 weeks for the SRFI 18 library | low–medium |
| The same, with the additions of §18.6 (decision 7c) | 29–61.5 weeks, **7–14 engineer-months**, after 5–8 weeks spent on the additions during the redesign (net saving 1–10 weeks) | low–medium |
| Planning figure, the build: 9–18 months × 2–4 [A] | **1.5–6 engineer-years** (1.1–4.8 with the additions) | low |
| Planning figure, recovery after the first supported release [A, E] | **0.4–3 engineer-years**: a quarter to half of the build, spread over about as long again | low |
| **Planning figure, total** | **about 2–9 engineer-years** (1.4–7 with the additions), to a supported, optional build with at most 5% tax, recovered | low |

**Labels in this section:** **[M]** measured on the development machine (Apple M4 Pro, arm64, macOS 27.2, release) on
2026-10-01, at `28a94f8`, with Rust 1.97.1; "[M, Chez]" is Chez Scheme measured there; **[E]** an engineering
estimate (every effort figure, in focused engineer-weeks for one engineer who knows the codebase); **[A]** as in the
Conventions. R1–R31 and C1–C17 are the rows of `itemize.md` §4.1 and §7; R32–R34 and the re-priced R26 are
`followup/parallelism/ANSWER.md`'s, which supersedes `itemize.md` where they differ.

### 18.1 The M:1 design and its N-ready interfaces

SRFI 18 needs a shared heap (threads share mutable state and may invoke each other's continuations) but not
parallelism; Gambit, its reference implementation, uses green threads. The carrier/thread split follows Go's per-P
allocation cache (`runtime/mcache.go`), Loom's carriers (JEP 444), OCaml 5's per-domain minor heaps (Sivaramakrishnan
et al., ICFP 2020) and Chez's thread contexts deactivated around blocking calls (`c/thread.c`). No OS threads are built
now; isolates stay optional.

**`Mutator` (carrier) and `GreenThread` are separate.** The `Mutator` owns the ABI block of §14, the allocation buffer,
the store buffer and remember-whole list, the root-scope stack and `no_gc_depth`, the handle table, safepoint state and
`current_thread`; under M:1 there is exactly one. A `GreenThread` owns its register stack and `ThreadGcState`
(watermark, `ran_since_gc`), its frames and re-entry fields, its dynamic environment (parameterization, current ports,
handlers, winds, prompts, as heap data) and scheduler links. A switch saves `reg_top` and installs the incoming limit
through `set_limit` (§12). `allocs_since_gc`, `gc_threshold`, `gc_pending` and `gc_defer_depth` leave `Heap`, and no
runtime state stays in `thread_local!`.

**SRFI 18 objects** (layouts in §6):

| Element | Rule |
|---|---|
| Objects | **thread** (`T_THREAD`): name, specific, thunk or result, end exception and joiners are heap data; its id names a `GreenThread` in the per-heap thread table (−1 before `thread-start!` and after termination); `thread-start!` takes its stack (the Stacks row below), so a thread never started holds no stack. **Mutex** and **condition variable**: waiter queues are heap lists of thread objects, written through the funnel. **Time**: a pointer-free box of seconds |
| Root rule | the scheduler is a root provider reporting every started, non-terminated thread (running, run queue, timer queue, blocked set); their frames and dynamic-state slots are roots in every collection (a minor scans only frames above the watermark of threads that ran). **A thread blocked for ever on objects nothing else reaches stays rooted until it terminates** (decision 23), as in Gambit, whose thread groups link every non-terminated thread (`lib/_thread#.scm:1346`, unlinked at termination, `lib/_thread.scm:1649`), and chibi, which keeps blocked threads on a global list (`lib/srfi/18/threads.c:165-206`). One-shot fibers or effect handlers, which nothing plans, would need a second root class: a stack owned by a heap object as a host payload and freed when that object dies, which this rule cannot express |
| Lifetime | every `GreenThread` is registered with `FinalKind::Thread`; termination queues the entry at once (`FinalRegistry::queue`), so a terminated thread, joined or not, gives its stack back at the next poll without a collection, its result staying in the heap object. An unreachable unterminated thread (only under decision 23's alternative) is queued by the collection that finds it; teardown drains every remaining entry (F5) |
| Stacks | **The stack rule.** Stage 9 runs `blocked-threads` at 100,000 threads (or 100,000 separate 256 MiB `MAP_NORESERVE` mappings) on Linux CI. If the probe passes, each green thread gets its own reservation; otherwise, fixed `--stack-max` slots carved from chunked reservations of 256 slots each (one VMA per 256 threads). Either way stacks are fixed-size, never relocated, lazily committed, trimmed and charged, under a per-heap bound on stack address space (100,000 threads × 256 MiB would be 25 TiB). The probe decides because Linux merges adjacent anonymous mappings with identical flags, so whether separate reservations each cost a mapping against `vm.max_map_count` (65,530) is unverified |
| Tree-walker; blocking | a tree-walker green thread is one suspended `StepResult` in the thread table, reported through `pinned`, switching only at the outermost trampoline safepoint. Under M:1, blocking inside a Rust re-entry raises (on either backend), and blocking I/O stalls every green thread until a helper pool or descriptor polling exists |

**N-ready now, at zero single-thread cost:** per-mutator allocation buffers, store buffers and remember-whole lists;
a block pool behind an uncontended mutex; mutators and threads lent to `collect` through `MutatorSet`; every mutator
read-modify-write of a metadata byte through `MetaByte` (plain under M:1, `fetch_and`/`fetch_or` under `threaded`);
heap words through `HeapSlot` (plain now, relaxed `AtomicU64` under `threaded`), with the funnel as the one Rust place
a threaded build enforces publication (rule 4; mechanism chosen by stage 6); a safepoint protocol written as request,
acknowledge, collect, release; posters that write only atomics; one interning function; traced one-word inline
caches; code installed through one function; the `Mutator` never embedded in machine code; continuations that refer to
no carrier state; the two-mutator lane (stage 9).

**Obligations of the `threaded` build** (before N > 1): atomic `u32`/`u8` sub-word stores for `string-set!` and
`bytevector-u8-set!`; acquire loads for heap references in Rust code, while JIT code may instead rely on address
dependencies under a documented exception (no value speculation or equality substitution); publication per rule 4;
safe regions entered only with no unrooted value on the Rust stack or in JIT SSA, with `read` lexing into Rust-owned
tokens first; and, because Linux `mprotect` is process-wide, a dual-mapped code reservation (a `memfd` with RW and RX
views) before OS-thread carriers (macOS `MAP_JIT` toggling is per thread [M, probe `mapjit.c`]). The `threaded` cargo
feature exists from stage 3 with the accessor bodies only, so CI's `--all-features` clippy lints it; no lane runs it
until stage 9, and enabling it needs decision 7. **Deferred until then:** OS-thread carriers, a `threaded` test lane,
handshakes, a `Send`/`Sync` heap, TSan, loom and Miri-concurrency lanes, freeze-and-slide stacks (C′).

**Isolates** (decision 7's cheaper alternative): one interpreter per OS thread, sharing nothing, built on its own thread
so no `Interpreter: Send` is needed; a host that moves an interpreter uses an audited `unsafe impl Send` wrapper (1–2
weeks [E]), sound only if no `Rc` reachable from it is shared outside, no `Owned` handle stays behind and the host's
primitives and payloads are `Send` (converting the remaining `Rc`s is rejected). They need defined semantics for stdin
lookahead, the exit status and copied messages, and parallelize only share-nothing work. Isolates in one process share
one `MemoryBudget` (§17.3), so N of them do not each default to 75%·B; they therefore start no earlier than 5g, which
brings the budget. Cost: 3–5 weeks after 5e [E]; 6–20 with the 2–4× factor below, or 12–20 at Racket's places' 4× (the
closest analogue) [A].

### 18.2 Rules the plan keeps

| # | Rule | Content |
|---|---|---|
| 1 | Carriers and green threads are separate | §18.1; no runtime state lives in `thread_local!` |
| 2 | Heap words have one way in | `HeapSlot`/`MetaByte` cover every access and the funnel every store; no `&` or `&mut` into heap words outlives one operation or crosses the embedding API |
| 3 | No shared read-modify-write on a hot path | no `Rc`, `RefCell` or `Drop` payloads in heap objects; each carrier's mutable data on its own 128-byte line |
| 4 | Publication is an invariant; stage 6 decides how it is enforced | under `threaded`: (a) every store that may make an object reachable by another carrier is ordered after that object's initializing stores, whatever the heap's `BarrierKind`; (b) the mechanism is `GcAttrs.publication` (`FenceAtAllocation \| ReleaseOnHeapStore \| FenceOnHeapStore`), separate from the generational barrier; (c) a store may skip publication only when its holder has not escaped since allocation (stored into the heap or a global cell, passed to a non-`Leaf` call, or returned); (d) a bulk range copy of heap values publishes with one store-store fence before the range, a named exception to (g); (e) Rust code loads heap references with Acquire; (f) only JIT code may rely on address dependencies, under a documented exception forbidding value speculation and equality substitution of loaded references; (g) release and acquire operations are preferred, and each standalone fence (the bulk range, C1's watermark if chosen, `set_limit`) is listed and checked by loom and the arm64 lanes, because ThreadSanitizer cannot; (h) every write of `desc.entry` (tier-up, and invalidation or tier-down back to the interpreter trampoline) is a release store, made after `install_code` has completed for the code it names, and a carrier context-synchronizes (`isb` on arm64) before it first enters code another carrier installed; stage 6 chooses where that synchronization happens |
| 5 | Protocols are written for N | request, acknowledge (at a poll or in a safe region), collect, release; posters write only atomics; safe regions are entered with no unrooted value; a mutex that must block tries first, then deactivates |
| 6 | One entry point per shared structure | interning, namespaces, code installation; inline caches are one traced word; continuations refer to no carrier |
| 7 | Limits are per heap, or per shared budget | several heaps share limits only through a `MemoryBudget` the embedder passes in (§17.3); pacing sums every carrier's allocation (R32); the store-buffer soft limit is divided among carriers; green-thread stacks share a per-heap address-space bound; carriers' native stacks have an explicit size, and Rust recursion is depth-guarded (R33) |
| 8 | The tree-walker stays M:1 and `!Send` | — |
| 9 | Concurrency policy | under `threaded`, state implemented in Rust synchronizes internally (ports, tables, registries, promise forcing, the source map); in the default build it is plain, behind C3's one wrapper. Structures built in Scheme (hashtables, records, vectors) do not synchronize; users lock them, as in Chez |
| 10 | The two-mutator lane stays green from stage 9 on | — |

**Memory model** (stated at stage 3): no crash, tearing or uninitialized object; every read returns some written
value; happens-before only through mutexes, condition variables, thread start and join.

### 18.3 Cost

Target: N OS-thread carriers over one heap; SRFI 18 threads scheduled M:N; stop-the-world collection with parallel
marking.

**Like for like**, both itemized with no overrun factor: the redesign is 99–138 weeks (§19), and parallelism after it
38–77 more (29–61.5 with decision 7c's additions). The calibrated planning figure follows the table.

| Item (rows of `itemize.md`) | Why | From today [E] | After stages 0–9, P [E] | Basis |
|---|---|---|---|---|
| Non-moving heap, no `Rc`/`RefCell` payloads, allocation buffers (R1, R2, R14) | racy reads of a growing arena are undefined behaviour | 11–18 | 0.5–1 | 4 `Vec` arenas; 14 `Rc` payload kinds; stage 5 |
| Heap API, collect capability, `VmState` split, `Send`/`Sync` (R10, R11) | a context per carrier; collection stops all | 10–17 | 3–6 | 767 borrow sites; stage 3 |
| Atomic slots, bulk operations, publication (R12, R13) | unzeroed holes make publication a memory-safety matter | 3–6 | 2–5 | the funnel; [M] costs; stage 6 decides placement |
| Rooting windows, time-to-safepoint (R16, R17) | a carrier that cannot stop stalls all | 5–11 | 2.5–6 | 21 `GcDeferGuard` uses; stages 2, 3, 4e |
| Handshake, safe regions, blocking I/O (R15) | one `read(2)` would block every collection | 4–7 | 2–3 | ~35 blocking primitives; stage 9 |
| Shared runtime tables: namespaces, code store, libraries, expander, ports, interner, caches (R3–R9, R22) | every carrier mutates them | 13–23 | 8–15.5 | stages 4a–4e; Racket's tax sits here [A] |
| Parallel GC with carriers as workers; store buffers; weak references (R18–R20) | serial GC caps 8 carriers at 3.2–6.9× (Amdahl) | 5–9 | 2.5–5 | stages P, 7; OCaml [A] |
| Pacing across carriers (R32) | N allocators between collections; floating garbage | 1–2 | 1–2 | a named cause of Jane Street's 2.5-year OCaml 5 adoption (elapsed time, not effort) [A] |
| Continuations across carriers (R21) | SRFI 18 defines them | 3–5 | 1–2 | stages 4e, 9 |
| M:N scheduler (R26) | queues, parking, mutexes | 4–10 | 3–8 | stage 9; Loom (2017–2025), Gambit SMP (opt-in for 9 years) and Ruby's M:N (off by default) are elapsed-time records, not effort data [A] |
| Global state, bundled libraries, tree-walker kept M:1 (R23–R25) | 19 `thread_local!`s; 3 bundled libraries with shared state (SRFI 128's comparator registry, SRFI 27's default random source, the R6RS hashtables' table of immutable copies) | 2.5–5 | 1.5–2.5 | source counts |
| Debugger, embedding, memory model (R27–R29) | all-stop; host threads attach | 4.5–7 | 2.5–4.5 | stages 2, 3; JNI [A] |
| Native stack depth on carriers (R33): carriers spawned at the main thread's stack size; a depth guard in the expander, compiler and printer; a stack check when a host thread attaches | Rust recursion aborts the process; spawned threads get 2 MiB | 1–2 | 1–2; about 0.5 once [#617]'s guard lands (§19's "now" row), which the totals below do not credit | ~980 nested `let`s at 8 MiB, ~240 at 2 MiB [M] |
| Promise forcing and the source map (R34): racing forcers can see `done = #t` beside the old thunk, so duplicate evaluation with one atomic publish (GHC's choice) or a CAS protocol; the source map locked or owned per document | Rust-implemented state that every carrier changes | 0.5–1.5 | 0.5–1.5 | `force` in a VM stub frame ([#476]); `Rc<RefCell<SourceMap>>`; GHC's thunk updates [A] |
| Tests (R30) | simulation, loom, TSan, arm64 lanes | 6–10 | 4–7 | none exist |
| First-pass tuning and scaling (R31) | contention, false sharing | 6–12 | 3–6 | [M] tax rows |
| **Total** | | **79.5–145.5 (18–34 months)** | **38–77 (9–18 months)** | |
| With the additions of §18.6 | | — | **29–61.5 (7–14 months)**, after 5–8 weeks spent during the redesign | |
| SRFI 18 on M:1, which this presupposes | | 12–19 | 6–11 | stage 9 |
| A multi-carrier JIT, if a JIT exists | | 6–10 to retrofit | 2–4 | §14 contract |

41.5–68.5 of the weeks counted from today (10–16 months) are work the redesign does anyway (the block heap, the `Cx`
codemod, precise rooting, heap continuations, global cells, the thread/carrier split): doing parallelism first would
build them twice, and nothing in the redesign has to be undone.

**Calibration [A, E].** Planning figure = itemized base × overrun factor + recovery. **Base** 38–77 weeks. **Factor**
2–4×, a judgement anchored by one quantified retrofit, Racket's places (share-nothing, message-passing), which took at
least 4× their estimate; Julia ("much longer than expected") and OCaml (8¾ years) give no ratio, and CPython reached
"supported" within the window set at its acceptance, after two years of prototyping. **Build** 1.5–6 engineer-years
(1.1–4.8 with the additions); calendar time is this divided by the staffing. **Recovery** after the first supported
release lasts about as long again in calendar time [A] (OCaml about 2 years restoring dropped features, Jane Street 2.5
years adopting, CPython one release reducing its tax) at a quarter to half the build's intensity [E]: 0.4–3
engineer-years. **Total about 2–9 engineer-years** (1.4–7 with the additions). After stage 9 the redesign's own
measured overrun ([K14](#k14)) replaces the borrowed factor. These figures supersede the design study's earlier
4–9 months after stage 9 and 9–18 months from today (reconciled in `followup/parallelism/ANSWER.md`'s editor notes).

### 18.4 Single-thread tax (the `threaded` build with one carrier)

| Mechanism | Measured [M] | After the redesign [E] |
|---|---|---|
| Relaxed atomic heap words, sub-word stores, poll word, locked block pool | same machine code: 0 | 0 |
| Acquire loads of heap references in Rust, release stores of heap values | +1.9% geomean on today's interpreter over 11 workloads (−0.1% to +3.6%; code-layout noise ±4%; +2.0% with a twelfth, code-layout outlier). Sites converted: `car`, `cdr`, `vector-ref`, cell reads, closure free variables, code ids | interpreter ≈2–5% if the absolute cost per instruction holds on an interpreter assumed ~30% faster; up to ~7% if load latency is exposed (0.53 dependent loads per instruction × 0.54 ns on nboyer, at about 4 ns per instruction); more if global-cell and record-field loads, not yet converted, lengthen the chains; re-measured at the stage-5 exit. JIT: 0 with address-dependency loads (3–60% with acquire loads) |
| Publication ordering | after every allocation: +0.4% (interpreter). Microbenchmarks show an intermittent 15–55 ns per ordering point after fresh objects are stored into old holders, unexplained and not reproduced in situ (Chez's fenced `set-car!` of fresh pairs: 1.84 ns per iteration [M, Chez]) | 0–1% on heap-valued stores, judging by Chez (fenced-store loops −1.4% to +4.1% [M, Chez]); stage 6 decides the placement |
| Bulk copy and fill | element-wise atomics 3.2–3.3× slower per word | 0, with one word-atomic routine and one fence per range |
| Port and table locks | +0.7 to +3 ns, uncontended | 1–3% on I/O-bound loops, under `threaded` only |
| Interner lock | +0.7 ns (`parking_lot`) to +5.1 ns (`std`) on a 17 ns lookup; Chez's global mutex +50% on `string->symbol` [M, Chez] | ≈ 0, sharded |
| Context held in `thread_local!` | +0.51 ns per access | 0 (`Cx`, `x21`) |
| Locking today's heap (`RwLock`, `Arc` on code objects) | at least +4.1% at one thread (a lower bound); a shared count costs 97 ns at 4 threads and 370–550 ns at 8 | not used |
| **Total** | | **interpreter ≈2–5% (up to ~7%), JIT 0–4% on arm64; ≈ 0 on x86-64; 0 with a non-threaded build** |

Comparisons: Chez threaded against non-threaded +0.8–3.5% over two measurement sets, the +3.5% including interning
[M, Chez]; OCaml 5 3.5%, CPython 3.14 1–8%, Racket 9 up to 6–8% [A].

### 18.5 Footprint and limits under N carriers

Memory has not been measured. CPython's free-threaded build costs +15–20%, only two of its six documented causes being
reference counting; Jane Street saw +10–20% on some OCaml 5 programs [A]. The steady-state footprint is F(L),
unchanged by N, plus:

| Term | Per | Size |
|---|---|---|
| Allocation-buffer waste | carrier | up to one 32 KiB block |
| Block cache, if R14 adds one | carrier | a few blocks, sized by R14's measurement |
| Store buffer | carrier | 256 MiB of address space, committed up to the soft limit, which is divided among carriers (about 1 MiB in all) |
| Mark-stack segments | GC worker or carrier | 4 KiB segments, while marking |
| OS thread stack | carrier | reserved at the explicit size R33 sets (8 MiB, like main), committed to the deepest Rust recursion |
| `Mutator` block | carrier | under 1 KiB, aligned to 128 B |
| Floating garbage | heap | grows with N carriers' allocation between pacing decisions (R32) |
| Deferred code-memory reuse | heap | replaced bodies kept until every carrier passes a quiescent point (C10) |
| Register stack | green thread | unchanged by N (decision 15) |

The native-stack and stack-address-space rows of §17.3 come from this analysis.

### 18.6 Cheap additions (decision 7c, proposed; scheduled in §19)

These serve the decided N-ready interfaces. Adopting them is decision 7c, proposed; §19's stages and totals assume
it.

| Addition | Stage | Cost [E] | Saves [E] |
|---|---|---|---|
| Time-to-safepoint metric beside [K16](#k16) (C12) | 0 | 1 day | makes R17 measurable; 0.5 week |
| Per-heap Rust tables behind one wrapper; loading entries record their thread (C3, C15) | 2 | 1–2 days | 1.5–2.5 weeks |
| Per-carrier collect capability (C2) | 3 | 2–3 days | 1–2 weeks |
| Reserve `fenced_ap`, a quiescence epoch and a handshake word in `Mutator`, and `publication` in `GcAttrs`; assert 128-byte alignment (C1, C10; §14) | 3; frozen at 6 | 1–2 days | an ABI break; 0.5–1 week |
| Bulk-range accessors; `HeapSlot::compare_exchange` (C4, C5) | 3, 5d | 3–4 days | 1–2 weeks, and the 3.2× bulk loss |
| A CI check against new `thread_local!` (that part brought forward by [#622]), `static mut` and heap-side `Rc`/`RefCell`/`Cell` (today's 19 `thread_local!` statics allowlisted), plus `Send`/`Sync` assertions as each type first satisfies them (C9, C11): `InterruptHandle` and `PrimitiveRegistry` at 3, `CodeBody` at 4e, `GreenThread` at 9; `HeapShared: Sync` waits for decision 7; VM-heap payloads need a separate `Send + Sync` payload trait-object type per heap kind, because tree-walker payloads stay `!Send` | 3, 4e, 9 | 1–2 days | 0.5–1 week |
| The memory model of §18.2 | 3 | 1–2 days | R29, 0.5–1 week |
| One read-mostly namespace API (C6) | 4b | 1–2 days | 1–1.5 weeks |
| Tax re-measurement on the new representation, with global-cell and record-field loads added (a run, not a lane) | 5 exit | 2–3 days | §18.4's figure; start condition 3 |
| Publication spike: placements (`dmb ishst` per allocation group; `stlr` on heap-valued stores; `dmb ishst; str`; C1's filtered fence) × shapes (a tight `cons` loop; a tail-building `set-cdr!` loop; fresh objects into an old vector or hashtable, including Chez's `(set-car! old (cons i i))`; bulk ranges), at least 10 launches each; conformance tests for `BarrierKind::None` under `threaded` and a store into an escaped fresh holder; code publication (`desc.entry` as a release store, `isb` before first entry, rule 4 (h)) | 6 | 3–5 days | decides rule 4's mechanism and where (h)'s synchronization happens |
| A seeded multi-carrier simulation lane, generalizing the two-mutator lane (C8) | 9 | 1–2 weeks | 2–3 weeks |
| Stage P's worker loop callable from any thread, keeping its segment stealing (C7) | P | 2–3 days inside P | 0.5–1 week |
| Threaded lanes run on arm64 (C17) | with the first threaded lane | 0 now | bugs that x86-64 hides |

The additions (decision 7c) total about 5–8 engineer-weeks (4–9% on top of the redesign) and save 9–15.5 weeks after the
trigger, a net saving of 1–10 weeks; C8 and C12 pay for themselves under M:1, and loom models wait for decision 14. C1
is the standalone-fence variant of rule 4 (its watermark is sound only if `fenced_ap` advances at a store-store fence);
if stage 6 chooses release stores, C1 is dropped and its word stays unused. Store-side placement is the candidate to
beat on fence count alone: heap-valued stores run about 5 per 1,000 instructions at the median, against 36–258
allocations [M]. Moved out: C13 (`Owned: Send`) to R28 or isolates; C14 (stacks from one reservation) to the
steady-state work, conditional on stage 9's Linux probe; C16 (the three bundled libraries) to R25.

### 18.7 Decision record

**Decision 7 (proposed 2026-10-01, pending owner review): not now.** SRFI 18 ships as M:1 with N-ready interfaces and,
under decision 7c, the additions above; isolates cover share-nothing parallelism. **Start only when all four hold:** (1)
a named workload needs a multicore speedup over shared mutable data that isolates cannot give; (2) stages 0–9 and SRFI
18 on M:1 have shipped, with the matrix rows for switches, cross-thread continuations and terminate green on both
backends and the simulation lane green; (3) the stage-5 tax re-measurement is within 5% geomean on the GBS (cycles, with
confidence intervals); (4) the planning figure, re-estimated after stage 9, is available.

**Gates of the first step** (the threaded build with one carrier, every lane byte-identical): time within 5% geomean on
the GBS (cycles, with confidence intervals); committed bytes at the end of each steady-state probe's window within 5%
of the default build, geomean; the per-carrier footprint increment measured and stated. Making it run is row R12's
"run the feature and fix what breaks", 1–3 weeks [E].

**A cheaper first step: the token.** N carriers share one mutator token; blocking I/O and FFI calls run in safe regions,
and one carrier runs Scheme at a time. The handover is a mutex release and acquire, which orders every heap word, so
there are no atomic heap words and no publication ordering; under the hand-off mutex the heap's `InterruptCell.mutator`
(§12) is re-pointed to the new holder's `Mutator`, so Ctrl-C and profiler posts reach the carrier running Scheme, not a
parked one. Rust-side state reached from safe regions (port buffers, the `PortTable`, the per-heap tables) still takes
C3's wrapper locks. It costs 9.5–15.5 weeks [E] (19–62 with the factor), counted inside the totals (safe regions,
carrier pool and handoff, locks, `Send` machine state, `exit` and interrupts from a non-main carrier, carrier stacks,
simulation and TSan lanes, handoff latency; breakdown in `followup/parallelism/ANSWER.md`), and needs stages 0–9, the
SRFI 18 row (with the helper pool) and C8.

**Path after the trigger**, each step shipping alone: (1) the token, optional; (2) the threaded build with one carrier,
with its gates; (3) the token released for VM heaps, after the simulation lane covers N carriers, with TSan and arm64
lanes; (4) scaling: carriers as GC workers, parallel minors, contention fixes. **Open until then:** decisions 7a and 7b
(§2). **Stop rules:** a first-step gate still above 5% after re-placing fences and loads → keep two builds and do not
release the token for parallel mutation; a step overrunning its re-estimate twofold → stop at the last step shipped.

**Testing order.** Every lane stays byte-identical on the threaded build with one carrier; several carriers are tested
first as a seeded simulation on one OS thread (protocols, not reordering), then under loom (once decision 14 allows),
ThreadSanitizer (nightly, blind to standalone fences) and arm64 lanes.

**Risks** (owner-facing, from `followup/parallelism/ANSWER.md` Part 1):
- **Deterministic lanes.** Real threads are not byte-identical, so lanes stay byte-identical only with one carrier, and
  the seeded simulation of several carriers checks protocols but cannot see reordering.
- **Dynamic-state transfers.** Cross-thread continuations and `thread-terminate!` inside `dynamic-wind` sit where this
  codebase's defects have clustered ([#157]–[#163]).
- **Embedding and the ecosystem.** Host primitives and payloads become `Send + Sync`, host threads attach as carriers,
  and three bundled libraries keep shared mutable state (SRFI 128's comparator registry, SRFI 27's default random
  source, the R6RS hashtables' table of immutable copies).
- **The native stack.** Rust recursion aborts the process at about 980 nested `let`s with an 8 MiB stack and about 240
  with 2 MiB [M]; carriers need an explicit stack size and the recursion a depth guard (R33, [#617]).
- **A long-lived second build.** Erlang kept two for 12 years; GHC has kept two for about 20.

**Part II, the plan (§19–§22).**

## 19. Stages

**Rules for every stage.**
- A GitHub issue first, filed when the stage starts and carrying the work-item detail: scope, files, numeric
  acceptance and effort (§22). The PR closes it, and this list keeps one line per stage. A defect gets its own issue
  with symptom and repro; the present-day ones are filed and listed in the Issues column.
- `docs/GC_DESIGN.md` is rewritten by each stage to describe the collector as it now stands. Every other repository
  document whose rule a stage supersedes is updated by that stage (listed below the table), and each new test lane
  is added to `docs/TEST_ORGANIZATION.md` when it is created. A stage that changes the contract changes §3–§18 here.
- Behaviour changes are scored against chibi and Gauche (and Gambit for thread semantics) and recorded in
  `DIVERGENCES.tsv`.
- Each semantic change is **its own PR**, never folded into a representation step: port identity and GC-time
  flushing, deep-bound `parameterize`, flonum `eq?` and flonum-keyed ephemerons, a symbol's name hash, variant C.
- **No stage removes a safety property before its replacement has landed**: `Rc` code liveness stays until traced
  code liveness (4e); nested-loop deferral stays until the weak continuation tables are gone (4e); dead-slot clearing
  stays for good.
- **No stage moves a collection point before the detectors that would see its failure are in CI.** Stage 1's byte
  trigger waits for [#621] and its release `gc-check` lane; stage 2 opens loading points A, B and D only once every
  loading-path entry on [#622]'s list is rooted, guarded on its data or held in the machine; and 4e lets driver-level
  nested loops collect only once every `apply_proc` and `across_reentry` entry on that list stays deferred or holds
  its values in the machine (§11.3).
- Embedding-API changes follow [#601]'s deprecation convention, one step per stage (stages 2 and 3).
- **Every stage ships measured value or stays neutral within its gate.** No geomean regression above 1% unless the
  stage declares a budget; regressions and gains under 2% are judged in instructions or cycles with confidence
  intervals (§16). **No stage turns a green steady-state row red** ([K17](#k17)).

Effort is from DESIGN Appendix D except where a row says otherwise; "Value on its own" is what the stage delivers if
the plan stops after it.

| # | Stage | Effort [I] | Limits and steady state | Threading readiness (§18.6 additions) | Gate | Value on its own | Issues |
|---|---|---|---|---|---|---|---|
| now | Ahead of the stages: two present-day defects | about 1–2, outside the totals | keyword aliases deduplicated by target; a Rust-recursion depth guard in the expander, compiler and printer (part of cost row R33, §18.3) | — | `symbols` flat across top-level `guard` forms; 1,000 nested `let`s run on both backends and deeper nesting raises a catchable error | two defects fixed independently of the redesign | [#611], [#617] |
| now | Ahead of the stages: detection on today's collector (§16): stale references panic in the check build, which the release GC lane runs; re-entries into the evaluator under clippy; every traced field named and tested with a sentinel; the deferral protocol and the no-collection windows asserted; `DEAD_SLOT` for retired registers and a zeal lane; more programs under stress | about 2–3, outside the totals | — | — | each detector's positive control fails when the detector is removed; no false positive on either backend; every stress run asserts that it collected | the next premature collection panics where it happens instead of reading as a legal value, so stages 1, 2 and 4e, which move collection points, run with detectors watching | [#621], [#622], [#623], [#624], [#625], [#626] |
| 0 | Ground truth: a `gc` mode for the benchmark runner, the GBS with `large-live`, the pause/MMU log, [K16](#k16)'s counters, the `gc-census` feature, the vendored probes; the measurement table and the off-heap holder inventory on the tracking issue (§22); the rebinding suite file with its `DIVERGENCES.tsv` rows | 3–4 | the steady-state lane and its portable runner; `resident-bytes` and `cpu-us`; the MMU ring buffer; the `def-getter` hygiene maker; the lane's resident-size form and the SS4 gate (SD1) | time-to-safepoint metric (addition C12) | the measurement table's baselines reproduced within their confidence intervals; no behaviour change; every steady-state row runs (red rows capped) | every later claim becomes measurable | [#647] (tracking), [#648], [#649], [#650], [#651], [#652], [#603]; drift: comment on [#597] |
| 1 | Quick wins on today's collector: byte trigger, `(gc)` collects at its call, `EMFILE` collect-and-retry with descriptor pressure, the unread provenance stores deleted | 4–5 | SS1, SS2 and SS5 at forced majors (SD1); environment-specifier namespaces charged as external bytes; expansion chains of shared links, each owned by one form's expansion ([#612]; interned per document at 4c); `(gc-stats)` shows each limit beside its use | — | trigger blindness and descriptor exhaustion fixed; reclamation proofs non-vacuous; GBS ±1% | the measured blow-ups fixed now; `(gc)`'s timing independent of the poll | [#606], [#607], [#612] (part), [#615] (part), [#643] |
| 2 | Root and boundary contract: slot visitor and the `Trace` derive for off-heap structures (§14), `CallFrame.closure` as a value, `Owned` and environment handles on the public paths that yield a value or environment, `backend()` and `evaluator()` documented as the raw layer (§11.5), heap teardown, top-level `import` and `define-library` recognized by binding, rooted loading | 6–8 (Appendix D's 5–7, plus about 1 for the derive) | `define-library` forms reach a collection point; teardown returns each dropped interpreter's memory | per-heap Rust tables behind one wrapper; loading entries record their thread (additions C3, C15) | embedder use-after-free (returned values, global reads, host environments) and teardown leak fixed; the VM evaluates in the environment it is given; no hand-written off-heap trace left; library bodies loaded through `import` collect; GBS ±1% | embedding is sound; library bodies collect; no leak; a new field cannot go untraced silently | [#604], [#605], [#610], [#614] (part), [#620] |
| 3 | `Mutator`, the collect capability, `Cx` and the store funnel; `#![forbid(unsafe_code)]` in the safe crates (§11.3); polls at frame entry; `InterruptHandle`; `Interpreter::call` and host primitives | 10–13 | the `CoreExpr` literal pool scoped to one compilation; the embedders' external-bytes accounter | per-carrier collect capability (addition C2); reserved `Mutator` words, `GcAttrs.publication`, 128-byte alignment (C1, C10); bulk-range accessors (C4); the CI check and the first `Send`/`Sync` assertions (C9, C11); the memory model; the `threaded` feature with accessor bodies only, linted by `--all-features` clippy | ABI, trybuild, interrupt and embedding tests on both backends; `ephemerons.rs:94,109,127`; geomean ≥ 0% in instructions | the JIT ABI object exists; who may collect is a type; hosts can call Scheme and register primitives soundly; Ctrl-C stops a tight loop | — |
| 4a | Canonical identity and ports | 4–5 | the `PortTable` registry prunes dropped heaps; the loader retries after `EMFILE` | — | `(eq? (current-output-port) (current-output-port))` ⇒ `#t`; `garbage_port_flushed_by_collection`, `descriptor_exhaustion_retries`, `one_port_one_object`, `dropped_interpreter_flushes_ports` and `exit_flushes_every_interpreter` on both backends; [#618]'s repro (two interpreters on one OS thread keep separate current ports) on both backends; I/O workloads ±1% | port semantics match the oracles; no primitive calls back for `parameterize` | [#608], [#618] |
| 4b | Global cells and binding records (variant R) | 3–4 | namespaces own records and cells, placeholders weak; a variable alias stops being a name; introduced definitions strong with an indexed lookup; environment-specifier namespaces die with their specifier or last unit; redefined libraries released | one read-mostly namespace API (addition C6) | the rebinding tests and decision 3's rows answer as today; the global path neutral or better than `frame_globals` | no `Rc` clone, `RefCell` borrow or `FORWARDED` hop on the global path, and no environment held by closures | [#613], [#614] (part), [#615] (part) |
| 4c | Identifiers as ids, inline provenance | 3–5 | document-owned location tables compacted while streaming; a weak (or per-form) scope-set table that reports which expansion scopes live identifiers carry | — | hygiene matrix 139/139; libload `malloc` peak ≤ 450 MiB; caret tests | the largest load-memory cost removed; syntax becomes `Drop`-free | [#612] (part) |
| 4d | Frames and stacks; the `return_into` choke point for every write into a suspended frame (§11.1; it lowers the watermark from stage 7); the stack cap and its knob | 4–5 | B detected; the main-stack default min(8 GiB, 25%·B), fitted under `RLIMIT_AS`; register-stack pages returned after 2 observations | — | matrix 64/64; raise-site tests; `finished_forms_release_code.rs` 9/9; 10 M-deep recursion | a JIT-ready stack, without changing the interpreter protocol or code lifetime | — |
| 4e | Continuations as heap objects; traced code liveness | 5–7 | the continuation plateau gone | `CodeBody: Send + Sync` asserted (addition C11) | the matrix; `finished_forms_release_code.rs` 9/9 with no collection; capture targets; a nested driver-level loop churn probe collects during the loop with a bounded heap | capture becomes byte-visible; code release no longer depends on sweep; nested loops can collect | — |
| 4f | Tree-walker host payloads | 2–3 | tree-walker environments charged only while a payload holds them | — | tree-walker lanes; no environment-driven collection on `(fib 25)` | nothing is dropped at sweep any more | — |
| 4g | Inline payloads | 2–3 | — | — | per-kind census | the heap is ready to move into blocks | — |
| — | Stage-4 exit | ≤ 1, not in the totals | — | — | the baselines stage 5 is judged against are re-measured: the GBS, the post-load major, the I/O workloads, the steady-state lane | stage 5 is judged against what it replaces | on stage 5's issue |
| 5 | The new heap, non-moving and whole-heap, kind by kind: 5a the `patina-gc` crate, spaces, `MarkRegion`, `NullGc`, verifier, Miri, the weak contract and its conformance suite; 5b pairs; 5c procedures and fixed-size kinds; 5d vectors, strings, bytevectors, bignums, the LOS; 5e arenas, old collector and `RefCell` deleted, pacing, decommit, `max_heap`, collect-and-retry; 5f inline flonum operations, then the final tag map and self-tagged flonums | 16–21 | 5a: the `def-getter` maker under zeal-major; 5c: the weak symbol table, name hash and old allocation (SD2), the NMS as mark-region with hole reuse, introduced definitions reclaimed, cells no longer a root region; 5e: the heap ceiling and its `RLIMIT_AS` fitting, idle trim at prompts, `notify_idle` and empty-buffer reads, per-block decommit, boost decay, SS3's counters and the phase-change clause, `memory-budget`; the nightly soak from 5 | `HeapSlot::compare_exchange` (addition C5, 5d); the tax re-measurement at the exit | lanes byte-identical per sub-stage; `large_request_after_garbage` on both backends; exit: [K9](#k9) and [K17](#k17); `large-live` reported against §9.10's budgets; 5f: [K6](#k6) | the representation win: about 2× fewer bytes, bump allocation, pauses proportional to live data, memory returned | [#609], [#616] |
| 5g | Limits (after 5e, independent of 5f) | 3–4 (§17.3; the process budget's week is this file's estimate) | external bytes in `max_heap`; the soft target; the process budget (`MemoryBudget`); the whole-limit request check; the maximal major; the progress guard; the condition payload; the `NoGcScope` pre-entry check; the near-limit and memory-pressure hooks; the handler-room test; the limits lane | — | the limits lane green, outside the byte-identical lanes; [K19](#k19) | a program that grows gets a catchable condition naming what grew, instead of exhausting the machine | — |
| C | Variant C (decision 2; after stage 5 and before the JIT track's first merged stage; its own PR; decision 3 settled first): the records' indirection, the shadow bitsets and `mark_if_*` deleted; per-binding guards on `cell.value == expected` for `CallPrimitive`, the inline operations and `JumpUnlessCellHolds`; JIT globals one load under `WATCHED` | about 2–3 (this file's estimate), outside the totals | — | — | the six define-after-use tests flip, with oracle-scored `DIVERGENCES.tsv` rows; `vm_callprimitive.rs`'s two set-after-use tests stay green; `run_suite_oracles.sh` green; ≤ 1% in instructions | one-load JIT globals; guards that recover when a program restores a procedure | filed when it starts |
| 6 | JIT ABI spike and freeze, on an unmerged branch | 3–5 | §11.2's fourth embedded-address case; the code-memory interface (`install_code`, per-unit free) | the publication spike, with code publication (§18.2 rule 4 (h)); the reserved words frozen | the spike answers its questions; [K6](#k6), [K7](#k7) and [K11](#k11) decided; `install_code` records each body's `GcAttrs` and refuses a mismatch (§14); against the Cranelift version it pins, the four facts this contract takes from the design review confirmed (prologues never save the pinned register, `Tail` results arrive in `x2`, `JITModule` has no `MAP_JIT` and frees only whole modules, single-bit tests lower to `tbz`/`tbnz`), and BTI landing pads checked | the JIT track can start without touching the GC | — |
| 7 | Sticky generations, behind a switch, with young and old LOS lists | 7–10 | the store buffer's soft limit counted in address-space fitting | — | the barrier-tax experiment ([K2](#k2)); the generational experiment decides the default ([K1](#k1)), set only through `HeapConfig` at a heap's creation (§9); [K8](#k8); per-workload max minor pause within §9.10's formula (constants measured in 7b); minors free dead young LOS runs (zeal-minor lane with an RSS bound); if JIT code is merged, `jit-invalidate` zeal on a generational heap with the verifier on (§13) | minor pauses proportional to survivors | — |
| 8 | Opportunistic evacuation | 6–9 | the footprint trigger; the sparse return-to-baseline row judged against stage 8's bound | — | move-all lane green for 4 weeks; [K3](#k3) | fragmentation cured; moving proven; the [K4](#k4) nursery becomes possible | — |
| 9 | Threads readiness: the `GreenThread` split; thread, mutex and condition-variable objects with their root rule and `FinalKind::Thread`, on both backends; deep-bound `parameterize`; safe regions | 7–10 | the Linux stack probe, then per-thread or chunked stacks as it decides (§18.1), under the per-heap stack address-space bound; idle trims at thread waits and mid-datum reads; the thread rows of the steady-state lane | the seeded multi-carrier simulation lane (addition C8); `GreenThread` asserted (C11) | two-mutator lane green; thread-lifetime tests on both backends; decision 23 recorded; ≤ 1% in instructions | SRFI 18 (M:1) can start | — |
| SRFI 18 | The SRFI 18 library on M:1, VM first, including the helper pool for blocking I/O (outside this plan) | 6–11 [I] (§18.3), outside the totals | — | — | matrix rows for thread switches, cross-thread continuations and `thread-terminate!` on both backends; if JIT code is merged, Tick-mode counts equal to the interpreter's (§12) | programs get SRFI 18 threads; §18.7's start condition 2 needs it | filed when it starts |
| P | Parallel stop-the-world marking (§14): triggered by measurement, budgeted | 4–6 | — | the worker loop callable from any thread (addition C7) | starts only if `large-live`'s major at 1 GiB live exceeds 100 ms after stage 5, which the estimate (120–320 ms) predicts; then marking ≥ 2.5× faster on 4 workers, with output identical to one worker | majors on large live heaps scale with cores, with no threading decision | — |
| JIT | The JIT track's first merged stage (outside this plan) | outside this plan | the code allocator (slabs, coalescing list), its cap `PATINA_JIT_CODE_MAX`, and the `eval-lambda` and `eval-redefine` rows | — | once the SRFI 18 row has landed, Tick-mode counts equal to the interpreter's (§12); once stage 7 has landed, `jit-invalidate` zeal on a generational heap with the verifier on (§13); the host-`Leaf` allocation test (§16). Stage 7 and the SRFI 18 row carry the same two gates, so each gates whichever of the two rows lands second | — | — |
| — | *Optional, each on its own trigger:* copying nursery ([K4](#k4)); C′ (decision 18, [K15](#k15)); boot-image static space (its immutable part only, §7); incremental marking (a latency goal, §14's prerequisites); evaluator-owned `StepRoots` for the tree-walker; native call/ret ([K11](#k11)); isolates (decision 7, §18.1) | — | — | — | — | — | filed when triggered |

**Documents each stage updates**, besides `docs/GC_DESIGN.md`, which every stage rewrites (the stage issues carry
the detail):
- **now (detection):** `docs/TEST_ORGANIZATION.md` (the release `gc-check` lane at stress 1, the zeal lane, the per-PR
  stress targets, the nightly Larceny lanes); AGENTS.md's CI table (the GC differential job's build and interval, the
  nightly job); AGENTS.md's "When Adding Features" and `docs/GC_DESIGN.md` §5, §7 and §11 (the checklists and review
  rules of the premature-collection study, [`ANSWER.md`](study/gc/followup/premature/ANSWER.md) §3.3).
- **0:** `docs/TEST_ORGANIZATION.md` (the `gc` benchmark mode, the GBS list, the census feature, the steady-state
  lane); AGENTS.md drift ("NaN-boxed", "24 transfer shapes").
- **1:** `docs/TEST_ORGANIZATION.md` (the rewritten reclamation proofs); README.md (the new `(gc-stats)` keys of
  `(patina debug)`); `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §6 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md` (the
  `locations` store is gone, so `prune_freed_locations` becomes a deprecated no-op, removed at 5e).
- **2:** `docs/VM_DECISIONS.md` §4 and §5 (the closure as a traced value; the root inventory); `PRD/FFI_DESIGN.md`
  (handles in place of exported `HeapIndex` and `SharedHeap`; pinning, `Foreign` finalization under F7, global handles
  for callbacks, callbacks as driver entries, §11.3; an FFI lane with a callback event-loop probe for [K16](#k16));
  `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §5.2 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md` (`DebugHook: GcRoots` becomes
  `DebugHook: RootProvider`).
- **3:** AGENTS.md (the `RefCell` borrow rule becomes the `Cx` and collect-capability rule); `docs/VM_DECISIONS.md` §6
  and §8 (the primitive signature; no `SharedHeap`); `PRD/FFI_DESIGN.md` (plugins through `register_primitive`);
  `docs/TEST_ORGANIZATION.md` (zeal-entry lane, trybuild tests); `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §5.2 and
  §10.1 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md` (`GcDeferGuard` and "no GC while paused" become `NoGcScope`,
  counted by [K16](#k16); the tier-policy flag).
- **4a:** `docs/TEST_ORGANIZATION.md` (port-finalization tests kept outside the differential lane);
  `DIVERGENCES.tsv`.
- **4b:** AGENTS.md ("An import installs a binding": records over shared cells replace forwarded slots and `Owner`);
  `docs/VM_DECISIONS.md` §3 (globals through link tables); `docs/TEST_ORGANIZATION.md` (the import-policy section's
  mechanism).
- **4c:** `docs/MACRO_SYSTEM.md` (`IdentifierData`, provenance).
- **4d:** `docs/VM_DECISIONS.md` §4 (frames in a reserved stack rather than `derive(Clone)` structs in a `Vec`);
  `docs/VM_RUNTIME.md` §2.1 and §4 (frame layout, call and return); README.md (`--stack-max`, `PATINA_STACK_MAX`,
  `&stack-exhausted`).
- **4e:** `docs/VM_DECISIONS.md` §4 (no continuation side tables or `VmContinuationRef`); `docs/VM_RUNTIME.md` §2.6
  and §6 (continuation objects, roots).
- **5:** AGENTS.md (the `TaggedValue` description; "New heap object type" becomes a `declare_layouts!` recipe);
  `docs/VM_DECISIONS.md` §2, §3, §5 and §9 (encoding, inline closures, the collector, inline strings);
  `docs/TEST_ORGANIZATION.md` (zeal-major, verifier and poisoning-sweep lanes); at 5e, README.md (`--heap-max`,
  `PATINA_HEAP_MAX`, `(notify-idle)`).
- **5g:** `docs/TEST_ORGANIZATION.md` (the limits lane); README.md (`--heap-soft-max`, `PATINA_HEAP_SOFT_MAX`,
  `&heap-exhausted`).
- **C:** AGENTS.md ("An import installs a binding" and "A fast path keys on the binding"); `docs/VM_DECISIONS.md` §3.
- **syntax-case (decision 17, after stage 5), before it starts:** `PRD/macro/SYNTAX_CASE_DESIGN.md`, whose syntax
  objects still hold the deleted `Value` enum in a `Drop` payload and whose transformers are applied from Rust: syntax
  objects become heap objects with no `Drop` payload (§6), and transformers run through the machine under decision
  17's `ExpansionContext` root provider.
- **6:** this file, §14 (the frozen offsets).
- **7:** `docs/TEST_ORGANIZATION.md` (zeal-minor lane). **8:** `docs/TEST_ORGANIZATION.md` (move-all lane).
- **9:** `docs/VM_RUNTIME.md` §5.6 (the `parameterize` rows); `docs/TEST_ORGANIZATION.md` (two-mutator and simulation
  lanes). **P:** `docs/TEST_ORGANIZATION.md` (workers lane).

**Totals.** Stages 0–9 and P total about 89–123 focused engineer-weeks [I] (the stage-4 exit's week is not counted;
stage 2's `Trace` derive adds 1 to DESIGN's figures). Steady state and limits add about 5–7 (5g and the items placed in
named stages), and decision 7c's threading additions 5–8, so about 99–138 in all [I], roughly 23–32 months for one
engineer. The figure is itemized, with no overrun factor; [K14](#k14) is its only calibration, and 5–8 of its weeks
depend on decision 7c. Outside it: the two "now" rows (about 1–2 for the defects, 2–3 for detection), row C (about
2–3), the SRFI 18 row (6–11 [I]) and the JIT track. Stage P is in the total because the pause estimate (§9.10) says
its trigger will fire; if `large-live` measures under 100 ms, the total falls by 4–6 weeks. Parallel work is limited by
shared files: only 4a and 4c can run beside 4d–4e, stage 6 beside stage 5, and stage P beside stages 7–9, so a second
engineer shortens the calendar by about 14–21 weeks [I]. The collector proper is about 4–6 kLOC through stage 7, plus
2–3 kLOC for stage 8 [I]; most of the effort is the common core every candidate design pays for.

## 20. Risks, mitigations and kill criteria

All measurements are interleaved A/B with confidence intervals (§16), on the programs the second table names.

| # | Risk | Mitigation | Kill criterion → change of course |
|---|---|---|---|
| <a id="k1"></a>K1 | Generational collection loses (Wingo on nboyer and splay; Go's rejection of generational barriers; queue3 and deeprec). The counterweight is HotSpot, where every collector family went generational: generational ZGC (JEP 439), whose non-generational mode JEP 490 removed, and generational Shenandoah (JEP 521) | a per-heap switch; bypass with hysteresis; strided watermark | sticky generations become the default only if the GBS geomean in cycles is ≥ 0% (or ≥ −0.5% with the confidence interval excluding −1%), **and** no GBS workload regresses by more than 3% in cycles, **and** the pause benefit holds per workload in max pause and MMU(10 ms). A pause win cannot buy back a throughput loss, and pooled percentiles are never used (with many minors, p95 is a minor pause by construction). Otherwise whole-heap stays the default, generational stays opt-in for one release, then is deleted unless some workload gains ≥ 5% |
| <a id="k2"></a>K2 | Barrier tax | the one-bit filter; static elision; `letrec*` unboxing removes most `WriteCell`s; range entries for bulk stores | the barrier-tax experiment > 1.5% in instructions → profile; > 3%, or JIT field logging > 2× the card cost on vecsort/hashtab → `BarrierKind::Card` with a young filter (invariant W makes card scanning sound) |
| <a id="k3"></a>K3 | Fragmentation before stage 8 | free reserve, medium overflow, granule holes, livelock rule | fragmentation (§9.4) > 10%, or occupied-block bytes over marked bytes > 1.5, at steady state on any workload, or a `frag-mix` livelock → stage 8 moves ahead of stage 7. (Committed/live is not used: pacing and the reserve keep it above 2 by construction) |
| <a id="k4"></a>K4 | The sticky nursery loses to a copying one | empty blocks freed without a scan; word-parallel sweep | after stage 7, lazy sweep plus refill > 4% of mutator time on any churn workload (deriv, destruc, fibfp, mbrot, generator, ctak, fibc, libload), or a prototype bump nursery reclaims ≥ 2× more bytes per GC-ms → build `NurserySpace` (§14). The prototype is scheduled only if the first condition is close (> 2%) |
| <a id="k5"></a>K5 | Writing `END` at mark time proves bug-prone | verifier; debug start-bitmap | two verifier escapes reach `main`, or mark time +3% → write begin/end metadata at allocation (Whippet; +2 stores in the fast path) |
| <a id="k6"></a>K6 | Self-tagged flonums do not pay | canonical boxes; constant folding; inline flonum operations first | immediate share < 70% on fibfp/mbrot/nucleic; or, **with inline flonum operations in both arms**, bytes allocated on those kernels not at least halved, or their cycles improved by less than half of the allocation-and-GC share their profile shows; or the spike's JIT float microkernel gains < 1.3×; or non-float cost > 1% in instructions → revert to 16 B boxes and keep `010`/`011` reserved. Decided before the stage 6 freeze |
| <a id="k7"></a>K7 | The 2-load call or the limit-fold poll costs too much | spike measurement | spike: 2-load call > 3% slower in cycles than a 1-load entry on fib/tak/nboyer → entry blocks with a branch patched only inside `install_code` (with icache maintenance). Tick mode > 1% over the fold → Tick only in deterministic-scheduler builds |
| <a id="k8"></a>K8 | The watermark return barrier breaks transfers or costs | one `return_into` choke point; strided lowering; matrix rows | any matrix row red under zeal-minor for > 1 week, or > 0.5% in instructions on deeprec or `deep-unwind`, where the barrier fires (fib/tak/nqueens at ≥ 1 s inputs are the control, where it should not) → raise the stride, or disable it (minors scan whole stacks; still correct), or enable it only above 10 K frames |
| <a id="k9"></a>K9 | B1 is wrong: representation does not pay | stage 5 is kind by kind and can stop | **Stage 5's exit gates**, every one against the stage-4 exit baseline (§19), never against today's measurement table: GBS geomean ≥ 5% faster (cycles, with CI); on **every** GBS workload GC CPU ≤ stage 4 + 5% and peak RSS ≤ max(stage 4 + 10%, stage 4 + 5 MB); peak RSS ≥ 30% lower on libload, gcold and queue3; gcold (RSS − 11 MB floor)/pacing target ≤ 1.15; queue3 max pause < 15 ms (41 today); the post-load major (a `(gc)` right after libload's imports) at most half its stage-4 baseline, with decommit outside the pause; memory returned after a load visible in resident size and footprint; `Drop`-carrying allocations ≤ 1% (census); every class A, B and C steady-state row green ([K17](#k17)). Kill: < 5% geomean **and** < 30% peak-RSS gain, or any per-workload GC-CPU or RSS gate failed without a fix → stop and re-plan around a copying nursery over a mark-in-place old space |
| <a id="k10"></a>K10 | The strangler mixes two heaps badly | one combined marker; per-kind sub-stages | a sub-stage cannot keep lanes byte-identical within two attempts, or mixed-trace pauses > +10% → convert the remaining kinds in one flag-day PR behind a temporary feature, deleted at 5e |
| <a id="k11"></a>K11 | The fragment model's trampolined returns are too slow | none inside this contract | spike: fragments > 8% slower in cycles than a native call/ret estimate on fib/tak/nboyer → open a **separate native call/ret design**, priced and gated on its own: native call/ret only between non-`Leaf` events; every `Transfer` helper, poll or collection abandons the native stack back to the driver through an SP-reset stub written outside Cranelift, and frames resume through their `(code, pc)` resume entries; a native depth cap that falls back to fragments; the watermark check on the driver's resume path; gates: the matrix, 10 M-deep recursion, the two-mutator lane and zeal-minor. This contract does not cover that design |
| <a id="k12"></a>K12 | Tier 2's rule (only tagged values at suspension points) costs too much | re-tagging is 1–6 instructions; out-of-band doubles boxed through a `Leaf` allocation | tier-2 measurements show > 15% loss on call-heavy float code → first more `Leaf` helpers and inlining; native stack maps only through a separately approved deopt-at-capture design that amends §14's exclusions |
| <a id="k13"></a>K13 | VA reservations fail on CI (Linux overcommit, map count, `RLIMIT_AS`) | RW + `MAP_NORESERVE`; one VMA per heap; `MADV_DONTNEED` decommit (no VMA split); reservations fitted under `RLIMIT_AS`; the many-heaps probe | any reservation failure → 4 GiB default with chained reservations and a per-chunk `meta_bias` (+1 load in the barrier) |
| <a id="k14"></a>K14 | Schedule overrun | each stage has standalone value | stage 5 at 2× its estimate (16–21 weeks [I], §19) → ship whole-heap `MarkRegion` (still the full representation win); defer 7 and 8. The measured overrun also replaces §18's borrowed factor after stage 9 |
| <a id="k15"></a>K15 | C′, if pursued, regresses | design A first, same frame format | > 10% slower than A on deeprec or generator, or a matrix row unrestored after 4 weeks → stay at A |
| <a id="k16"></a>K16 | B2 is wrong in practice: a window that cannot collect lets garbage grow (one huge form expanded under `NoGcScope` at point C or inside nested tree-walker trampolines; a Rust primitive building a large structure; polls deferred under `NoGcScope`; a C event loop calling Scheme through FFI callbacks) | `NoGcScope` confined to point C, the residual `apply_proc` fallbacks, nested tree-walker trampolines, a paused debugger's evaluation and, until they become driver entries (§11.3), FFI callbacks; overdraft with an emergency reserve; a pre-entry major when headroom is short (§17.3); `GcStats` keeps the high-water of bytes allocated since the last serviced poll and of bytes allocated under one `NoGcScope`, with the site (§15) | measured from stage 0 on the GBS, libload and both Larceny lanes, and, once FFI callbacks exist, on an FFI-callback event-loop probe in the FFI lane that the stage-2/3 `PRD/FFI_DESIGN.md` rewrite defines: any window above the larger of 64 MiB and 25% of the pacing target, or any lane reaching `HEAP_EXHAUSTED` → add a collection point at that site: bring decision 17's `ExpansionContext` root provider (a literal pool and epoch-checked memos) forward for point C, make the primitive resumable so it builds across `Step::Collect`, or root the nested tree-walker trampoline with evaluator-owned `StepRoots`, or make FFI callbacks driver entries (§11.3). Allocation itself never becomes a collection point |
| <a id="k17"></a>K17 | Steady state regresses, or a stage makes a leak permanent | the steady-state lane, per PR and nightly, on both backends | a stage that turns a green row red is blocked until it is green. Stage 5's exit ([K9](#k9)) also needs every class A, B and C row green on the VM, and on the tree-walker except rows allowed to lag (decision 1). Under SD2's alternative, `sym-churn` and the symbol part of `unbound-ref` are class D |
| <a id="k18"></a>K18 | Mortal run-time objects (M1) cost too much | NMS swept only at majors, after the pause | M1 costing over 1% geomean in instructions, or 10% on any major pause → sweep the non-moving space every k-th major (reclamation delayed by up to k majors) |
| <a id="k19"></a>K19 | The limits misfire | the reserve; the progress guard; the pre-entry major | handler room fails → unwind without running handlers. A GBS workload reaching the guard under default limits → a collection point ([K16](#k16)) or a corrected charge, never a larger default |

**Programs and metrics for each criterion.**

| Criterion | Programs | Metric |
|---|---|---|
| [K1](#k1) | the GBS, the barrier programs | cycles: geomean and per workload, with CIs; per-workload max pause and MMU(10 ms) |
| [K2](#k2) | vecsort, hashtab, queue, tree, strport, letrec, plus the GBS (barrier-tax experiment); the spike's store microkernels | instructions (interpreter); cycles (JIT) |
| [K3](#k3) | `frag-mix` and the GBS | fragmentation and occupied/marked after marking |
| [K4](#k4) | deriv, destruc, fibfp, mbrot, generator, ctak, fibc, libload | refill and lazy-sweep time over mutator time (GC log) |
| [K5](#k5) | verifier lanes; the mark microbenchmark | escapes; mark ns |
| [K6](#k6) | fibfp, mbrot, nucleic and their fixnum twins; the spike's float microkernel | immediate share; bytes allocated; collections; cycles |
| [K7](#k7) | spike: fib, tak, nboyer at ≥ 1 s | cycles |
| [K8](#k8) | deeprec, `deep-unwind`; control: fib, tak, nqueens at ≥ 1 s | instructions; cold-path returns |
| [K9](#k9) | the GBS, the I/O workloads, `small-heap`; the post-load `(gc)`; the stage-4 exit baseline; the steady-state lane | cycles; GC CPU; peak RSS, footprint and resident size per workload; post-load max pause; SS1–SS5 |
| [K10](#k10) | lanes; mixed-trace pauses | byte identity; max pause |
| [K11](#k11) | spike: fib, tak, nboyer | cycles |
| [K12](#k12) | tier-2 float kernels (later) | cycles |
| [K13](#k13) | `many-heaps` on Linux and macOS CI; budget detection under cgroup and `ulimit -v` | reservation failures |
| [K14](#k14) | — | elapsed engineer-weeks |
| [K15](#k15) | deeprec, generator; the matrix | cycles; rows restored |
| [K16](#k16) | the GBS, libload, both Larceny lanes (R7RS and the `--r6rs` emulation), the tree-walker subset; once FFI callbacks exist, an FFI-callback event-loop probe | `GcStats` high-water bytes since the last serviced poll and under one `NoGcScope`, per site; `HEAP_EXHAUSTED` events |
| [K17](#k17) | the steady-state lane (§17.4), both backends | SS1–SS5 at the window ends; red and green rows per stage |
| [K18](#k18) | the GBS; the mark microbenchmark and `large-live` | instructions, geomean; max major pause |
| [K19](#k19) | the limits lane; the GBS under default limits | handler room; progress-guard events |

**Risks without a numeric threshold:**
- **Hidden references** (Julia's lesson). Mitigation: the off-heap holder inventory; generated traces, the `Trace`
  derive for off-heap structures (§14, stage 2) and `declare_layouts!` for heap kinds (5a), with [#623]'s sentinel
  tests until they land; the poison lanes, and [#621]'s stale-reference checks before them; the verifier; and the
  move-all lane, which covers VM heaps only (§14). A missed edge that a second path masks escapes every dynamic check,
  `VERIFY_ROOTS` included, since it sees only the words a provider reports.
- **Single-mutator shortcuts creeping in.** Mitigation: §18's rules, the two-mutator and simulation lanes, the
  `threaded` feature linted in CI, the CI check against new `thread_local!`s, and review of new heap payloads.
- **Port-finalization and limits flakiness.** Mitigation: those tests are kept out of the byte-identical lane.
- **Raw addresses turning rooting bugs into undefined behaviour.** Mitigation: collection only through the
  `&mut Heap` capability (§11.3), `#![forbid(unsafe_code)]` in the safe crates, release range checks at trust
  boundaries, the debug-poison sweep and quarantine, `PROT_NONE` from-space, the verifier and Miri.

## 21. Where the stage-5 PRD's items went

`PRD/ARCHIVE/GC_STAGE5_PRD.md` tracked the work its era staged as "5+". Each item now lives here:

| Item there | Status there | Where it lives now |
|---|---|---|
| Priority 1, item 1: weak continuation side tables | done 2026-08-05 | the tables themselves are deleted at stage 4e, when continuations become heap objects (§13) |
| Priority 1, item 2: `code_store` constants never evicted, root tracing growing with session length | open | stage 4e (code descriptors with traced constants, units released per unit, §9.7) and stage 5c (descriptors in the NMS); the "immortal set" it proposed is replaced by M1 (§17.2) |
| Priority 2: nested-loop collection | open | stage 2 (rooted loading at points A and B, top-level imports outside the desugarer) and stage 4e (driver-level nested VM loops collect); what stays deferred is confined and bounded by [K16](#k16) (§11.3, §12). Its acceptance test (collections during a nested churn loop, with a bounded heap) is stage 4e's gate; `map` has been Scheme since [#471], so the nested-`map` shape now arises only in library bodies (stage 2's gate) and nested tree-walker trampolines ([K16](#k16)) |
| Priority 2b: VM register precision ([#423]) | done 2026-09-28 | its maps generalize at stage 4d: maps at every suspension point, dead-slot clearing in the scan (§11.1) |
| Priority 3: lazy sweep | open | stage 5 (mark-region with a metadata-only lazy sweep, §9.3) |
| Priority 3: non-moving generational | open | stage 7 (sticky generations behind a switch, the barrier-tax and generational experiments, [K1](#k1), [K2](#k2)) |
| Priority 3: weak symbol table | open | stage 5c (SD2; §9.5, §17.2) |
| Priority 4: residual trigger cost | open, optional | stage 3 (the per-instruction poll is removed; polls at frame entry, §12) |
| Non-goal: moving or compacting collection | ruled out there | reversed: opportunistic evacuation at stage 8 (§9.4), once references are addresses and identity hashes survive moves (§9.8) |
| Non-goal: concurrency | ruled out there | stop-the-world stays; parallel stop-the-world marking at stage P; shared-memory parallelism is decision 7 (§18) |

## 22. Work items

Stage issues are filed when each stage starts, after searching the issues and `PRD/ARCHIVE/`, and each PR closes its
issue; this file keeps one line per stage. Stage 0's issue is the tracking issue: it carries DESIGN Appendix B's
measurement table and E.1's off-heap holder inventory. The PR-level detail drafted during the study (the first PRs of
stages 0–2, scopes, files, numeric acceptance and effort per stage) is in `PRD/study/gc/design/ISSUES_DRAFT.md` and in
DESIGN Appendices B, D, E, F and H; once a stage's issue exists, it, not the draft, is the source of truth. E.7 (the
threading model) and the ST checklist of `PRD/study/gc/followup/parallelism/ANSWER.md` go into the issues of the stages
that implement them: the memory model and rule 9's concurrency policy into stage 3's, the publication checklist into
stage 6's, and the stack probe and the stack address-space bound into stage 9's; the bundled libraries with shared state
stay with cost row R25.

**Where DESIGN.md (including Appendices D, E and H) or ISSUES_DRAFT.md differ from this file, this file holds.** Both
predate the steady-state, parallelism, contract and premature-collection follow-ups, so correct a stage issue drafted
from them on these known points before filing it:
- Symbols, global cells, binding records, record types and code descriptors are mortal and live in the NMS (M1,
  decision 11, SD3), not in an immortal or descriptor space, as DESIGN §3, §4, Appendix D rows 4b and 5a, E.1 and E.6
  and ISSUES_DRAFT's S0 holder inventory have it.
- Cells are not a root region, and the epilogue re-arms nothing at step 9 (§9.1, §9.9), unlike DESIGN §6.1, §6.10 and
  H.2.
- Decommit also works per block, data pages only, inside partly occupied runs (§7), not only on whole 4 MiB runs
  (DESIGN §4, H.3).
- JIT code may embed addresses in four cases (§11.2), not DESIGN §8.2's three.
- Green-thread stacks follow §18.1's stack rule, not DESIGN §8.1, §12 and decision 15.
- Limits derive from B, and the heap reservation from the heap ceiling (§7, §17.3); the idle trim and the rest of §17
  have no counterpart in DESIGN, whose decision 15 used RAM.
- Publication follows §18.2 rule 4; DESIGN §12 placed one fence in the funnel.
- Decision 7's cost is §18.3's, which supersedes DESIGN's 4–9 months after stage 9 (its decision 7 and E.7); decision
  7c's additions are new.
- A heap's policy is fixed at creation (§9), where DESIGN §6 has gates flip it, and a host primitive is never `NoAlloc`
  (§11.5), unlike DESIGN §8.5 and F.3's embedding test.
- A boot image's static space holds its immutable objects only, its mutable ones going to the NMS (§7, M1), where
  DESIGN §15 and Appendix D list an unrestricted boot-image static space; isolates also wait for 5g's shared budget
  (§18.1), not only for 5e as Appendix D has it.
- Rules that DESIGN states without the stricter form here: `ticks` counts the bytecode's poll sites in every tier (§12;
  DESIGN §9); invalidation keeps the watermark trampoline (§13; DESIGN §10); a value is written into a host payload
  only through `HostPayloadTable::store`, which `PATINA_GC_VERIFY_ROOTS` checks (§14, §16; DESIGN §11 has `dirty`
  alone); incremental marking needs `GranuleLog` from creation (§10, §14; DESIGN §11); every write of `desc.entry` is a
  release store (§18.2 rule 4 (h); DESIGN §8.2).
- No counterpart there: the process budget (§17.3); F7; FFI callbacks as driver entries, the nested-driver tier rule
  and `NoGcScope`'s type and counter (§11.3); inline caches outside descriptors and the panic rule (§11.2); weak tables
  keyed by fresh symbols as class D (§17.1); `InterruptCell` following the token (§18.7); the stage-6 check of the
  Cranelift facts (§19); the `Trace` derive for off-heap structures (§14, stage 2).
- Stage 2 gives handles to the public paths that yield a value or environment, host-built environments included, and
  documents `backend()` and `evaluator()` as the raw layer ([#605], [#620], §11.5), where DESIGN E.3's stage 2 adds
  handles only for the `eval_*` results and leaves host environments to 4b's namespace handle.

[#38]: https://github.com/avalonalex/patina/pull/38
[#47]: https://github.com/avalonalex/patina/pull/47
[#157]: https://github.com/avalonalex/patina/issues/157
[#163]: https://github.com/avalonalex/patina/issues/163
[#338]: https://github.com/avalonalex/patina/issues/338
[#352]: https://github.com/avalonalex/patina/issues/352
[#406]: https://github.com/avalonalex/patina/issues/406
[#423]: https://github.com/avalonalex/patina/issues/423
[#442]: https://github.com/avalonalex/patina/issues/442
[#471]: https://github.com/avalonalex/patina/issues/471
[#476]: https://github.com/avalonalex/patina/issues/476
[#597]: https://github.com/avalonalex/patina/issues/597
[#601]: https://github.com/avalonalex/patina/pull/601
[#603]: https://github.com/avalonalex/patina/issues/603
[#604]: https://github.com/avalonalex/patina/issues/604
[#605]: https://github.com/avalonalex/patina/issues/605
[#606]: https://github.com/avalonalex/patina/issues/606
[#607]: https://github.com/avalonalex/patina/issues/607
[#608]: https://github.com/avalonalex/patina/issues/608
[#609]: https://github.com/avalonalex/patina/issues/609
[#610]: https://github.com/avalonalex/patina/issues/610
[#611]: https://github.com/avalonalex/patina/issues/611
[#612]: https://github.com/avalonalex/patina/issues/612
[#613]: https://github.com/avalonalex/patina/issues/613
[#614]: https://github.com/avalonalex/patina/issues/614
[#615]: https://github.com/avalonalex/patina/issues/615
[#616]: https://github.com/avalonalex/patina/issues/616
[#617]: https://github.com/avalonalex/patina/issues/617
[#618]: https://github.com/avalonalex/patina/issues/618
[#620]: https://github.com/avalonalex/patina/issues/620
[#621]: https://github.com/avalonalex/patina/issues/621
[#622]: https://github.com/avalonalex/patina/issues/622
[#623]: https://github.com/avalonalex/patina/issues/623
[#624]: https://github.com/avalonalex/patina/issues/624
[#625]: https://github.com/avalonalex/patina/issues/625
[#626]: https://github.com/avalonalex/patina/issues/626
[#637]: https://github.com/avalonalex/patina/issues/637
[#643]: https://github.com/avalonalex/patina/issues/643
[#647]: https://github.com/avalonalex/patina/issues/647
[#648]: https://github.com/avalonalex/patina/issues/648
[#649]: https://github.com/avalonalex/patina/issues/649
[#650]: https://github.com/avalonalex/patina/issues/650
[#651]: https://github.com/avalonalex/patina/issues/651
[#652]: https://github.com/avalonalex/patina/issues/652
[#655]: https://github.com/avalonalex/patina/issues/655
[#656]: https://github.com/avalonalex/patina/issues/656
