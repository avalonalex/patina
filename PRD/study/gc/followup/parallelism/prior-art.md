# Adding shared-memory parallelism after the fact: what it cost other runtimes

Date 2026-10-01. Task key `par-prior-art`. Repository `main` at `28a94f8`, unmodified. Intended home: the
research corpus under `PRD/study/`, linked from decision 7's row in `PRD/GC_PRD.md`.

This report compares runtimes that **added** shared-memory parallelism to a single-threaded design with runtimes that
**designed it in**. For each it records what had to change, the elapsed time and team where known, the single-thread
and memory tax (at first and after recovery), the compatibility cost, and what the people involved say they would do
differently. §4 turns this into lessons for Patina; §5 calibrates the in-house estimates against the record.

It complements `../../research/threads-prior-art.md`, which covers *mechanisms* (contexts, polls, rendezvous,
barriers). This report covers *histories and costs*. The companion `itemize.md` in this directory itemizes Patina's
own cost.

**Conventions.** Numbers in brackets, such as [7], refer to the source list at the end. **[P]** marks a measurement
taken for this report on the development machine (Apple M4 Pro, macOS 27.2). **[I]** marks my inference. Dates are
release dates unless stated.

---

## 0. Bottom line

1. **Adding parallelism after the fact took years in every case, not months.**
   - OCaml: 8¾ years from first commit (March 2014) to OCaml 5.0 (December 2022) [1][5]. Restoring the features 5.0
     dropped took until 5.3–5.4 (2025) [2]. Its largest industrial user then needed 2½ more years to switch [4].
   - CPython: 4 years from the `nogil` fork (October 2021) to "supported but optional" in 3.14 (October 2025) [7][8].
     The stable ABI for that build arrives only with 3.15 (2026) [16]. Whether it becomes the default is undecided.
   - Julia: 5 years, from thread-safety work in 2014 to composable threading in 1.3 (2019) [41].
   - Racket: 15 years, from futures (2010) to parallel threads (9.0, November 2025), with a rewrite onto Chez Scheme
     in between [23][25][27].
   - Ruby: Ractors are still experimental 5 years after 3.0 (2020) [18][21]. A per-Ractor GC was proposed in
     2026 [22].
   - Gambit's SMP scheduler has been opt-in for 9 years [32]. Lua never added shared-memory threads [38].
2. **The teams were small; the estimates overran.**
   - Racket's places took "approximately two graduate-student years, which is at least four times longer than we
     originally expected" [26].
   - Julia's runtime work took "much longer than expected — nearly two years" [41].
   - PEP 703 projected a 5–6% single-thread tax [7]. The first release that shipped it (3.13) measured about 40%; the
     second (3.14) measured 1–8% [9][10].
3. **When the retrofit is done well, the single-thread tax converges to 1–10%.**

   | Runtime | Tax |
   |---|---|
   | OCaml 5 | 3.5% geomean in the paper [1]; "3%-ish" at Jane Street, but some programs ran 10–20% slower at first [4] |
   | CPython 3.14 | 1% (macOS arm64) to 8% (x86-64 Linux) on average [10] |
   | Racket 9 | up to 6–8% [23] |
   | GHC (2005) | about 6% [34] |
   | Erlang (2008) | about 10% [37] |
   | Ruby per-Ractor GC (proposed) | 2–4% [22] |
   | Chez, threaded build with one thread | 1.5% geomean [P, §6] |

   Naive retrofits cost far more:
   - Python 1.5 free threading: at least 30% slower [12];
   - GHC with atomic thunk updates: +50% on average and +300% at worst; an earlier attempt +100% [34];
   - the Gilectomy reached parity with CPython only on about seven cores [13].
4. **There is a memory tax too.**
   - CPython's free-threaded build uses 15–20% more memory [8], plus per-thread copies of specialized bytecode [11].
   - Some OCaml 5 programs at Jane Street used 10–20% more memory at first [4].
5. **The collector was the well-understood part.** The years went into everything around it:
   - **process-global state:** Racket audited 719 global variables [26];
   - **reference counting:** in CPython, biased reference counting and per-object locks are the largest
     costs [7];
   - **the C API:**
     - OCaml rejected its better-scaling minor collector because it broke the C API [1][3];
     - CPython had to replace borrowed-reference APIs [7] and define a new stable ABI [16];
   - **mutable library structures:** Racket's 6–8% sits in hash tables and ports [23];
   - **inline caches:** CPython 3.13 disabled its specializer [9];
   - **GC pacing on real workloads:** Jane Street [4];
   - **fences:** Chez 10.1 fixed fences missing on one platform [28].
6. **Two builds (single-threaded and threaded) last a decade or more.**
   - Erlang kept both from 2006 to 2018 and removed the non-SMP build partly because of the test burden [37].
   - GHC still ships two runtime systems, about 20 years on. An accepted proposal to make `-threaded` the default had
     not been implemented as of the 2022 discussion I found [36].
   - Chez has shipped threaded and non-threaded builds for at least 17 years (its native threads were described in
     2009 [28]); only 10.0 (2024) made threaded the default [28].
   - CPython plans two builds for years [14].
7. **Mechanisms built ahead of demand were mostly removed or never adopted.**
   - Caml Light's concurrent GC (Doligez–Leroy, 1993) was abandoned, and OCaml then stayed single-threaded for about
     25 years [6].
   - Perl's shared-everything `5005threads` were removed in 5.10 [42].
   - Erlang's hybrid heap was removed in R15B02 [37].
   - GHC's local-heap collector was published but not adopted [35].
   - Java added biased locking to recover the cost of its built-in per-object monitors, then removed it after about 16
     years as too complex [40].
   - Java's original memory model was unsound and had to be replaced 8 years later [40].
   - Java's monitors were tied to OS threads, which forced Loom's virtual threads to pin until JEP 491 in 2025 [40].
8. **Representations and interfaces chosen early made the later step cheap.**
   - Chez: one source for both builds, with a thread-context register and fences only in the threaded build [28].
   - OCaml: shipped a "no naked pointers" checker in 4.x, before 5.0 required it [2].
   - Racket CS: moved its thread scheduler and I/O layer from C into Racket, which it credits for parallel
     threads [23][27].
   - HotSpot: per-thread polls cost under 1% (JEP 312) [40].
9. **For Patina, prior art supports DESIGN §12's split**: interfaces written for N mutators now, mechanisms deferred
   behind a `threaded` feature (§4.1). It also shows four things that are cheap to decide now and expensive to
   retrofit, which the design does not yet settle (§4.2):
   - a language-level memory model;
   - a concurrency policy for each mutable library structure and runtime table;
   - an embedding rule that no interior reference escapes;
   - a single-thread tax budget for the `threaded` build, measured continuously.

   These are candidate GitHub issues (§7).
10. **Calibration [I].** Prior art overran its estimates by roughly 2–4× (§5). The itemized estimate in `itemize.md`
    is 8–16 engineer-months after stages 0–9, or 6–12 with its §7 additions. For planning, treat those figures as a
    floor, and expect on the order of 1–3 engineer-years of focused effort and 2–4 calendar years before a supported,
    optional threaded build reaches a tax of 5% or less. Also expect a second build that lives for a decade. This is
    an analogy, not a measurement.

---

## 1. Summary table

"Retrofit" means a runtime that started single-threaded. Taxes are single-thread slowdowns of the parallel-capable
build against the single-threaded build or release.

| Runtime | Kind | Elapsed time | Team (where known) | Single-thread tax: first → recovered | Memory tax | Compatibility cost | Two builds? |
|---|---|---|---|---|---|---|---|
| OCaml 4 → 5 | retrofit (shared heap, domains) | Mar 2014 → Dec 2022; features restored through 5.4 (Oct 2025) | 9 paper authors plus the Tarides multicore team [1][2] | paper 3.5% (ParMinor) / 4.9% (ConcMinor) [1]; production 10–20% on some programs → fixed over 2.5 years [4] | paper: *less* than 4.06 (allocator change) [1]; production +10–20% on some programs [4] | naked pointers removed; `&Field` became `volatile`; many platforms and features temporarily dropped [2] | no: one runtime, by design [1] |
| CPython (PEP 703) | retrofit (free-threading) | Oct 2021 fork → Oct 2024 experimental → Oct 2025 supported; default undecided [7][8] | Sam Gross; Meta committed 3 engineer-years to the end of 2025 [15]; core team; Quansight for the ecosystem [17] | PEP estimate 5–6% [7] → 3.13 ≈40% [9] → 3.14 1–8% [10] | 15–20% (target ≤20%) [8] | new APIs replacing borrowed references [7]; extensions must declare support; no stable ABI until `abi3t` in 3.15 [16] | yes, "for years" [14] |
| Ruby | retrofit (isolated Ractors over one GC; M:N threads) | Ractors Dec 2020 → still experimental Dec 2025 [18][21] | Koichi Sasada, principally [19][22] | per-Ractor GC proposal: +2–4% real workloads [22] | — | M:N is off by default on the main Ractor "because of compatibility issue" [19] | runtime switches, not builds |
| Racket | retrofit ×3 (futures 2010, places 2011, parallel threads 2025) | futures: 1 expert-week + 1 academic quarter [25]; places: ≈2 grad-student-years [26]; parallel threads: after the CS rebuild (2017–2021) [23][27] | Flatt plus students and collaborators | futures and places: "minimal" [25][26]; parallel threads ≤6–8% [23] | places duplicate per-place state [26] | atomic mode no longer excludes parallel threads; memory model exposed (but kept safe) [23] | BC and CS coexisted until Aug 2025 [23] |
| Chez Scheme | designed with split builds | threads long-standing; parallel GC in 10.0 (2024) [28] | Dybvig; Flatt for the parallel GC [28][30] | threaded build 1.5% geomean, 3.5–4.5% on barrier and port paths [P] | — | none at the source level | yes, permanently; threaded default only since 10.0 [28] |
| Guile | pthreads added in 1.8 (2006); BDW in 2.0 (2011) [31] | — | — | — | — | `scm_without_guile` rules; continuations usable only in their creating thread [31] | `--without-threads` configure option [31] |
| Gambit | green threads designed in; SMP experimental | SMP branch by early 2017; merged as opt-in Aug 2017; still opt-in Aug 2026 [32] | Feeley | not published | — | — | yes (`--enable-smp`) |
| GHC | concurrency designed in; parallel RTS retrofitted | 2005 paper → GHC 6.6 (2006); parallel GC 2008 [34] | 3 paper authors [34] | CAS on thunks +50% average (+100% in an earlier attempt) → lock-free about 6% [34] | — | none for Haskell code | yes, about 20 years and counting [36] |
| Erlang/BEAM | per-process heaps designed in; SMP retrofitted | SMP in R11B (May 2006), tuned through R13B; non-SMP removed in OTP 21 (2018) [37] | OTP team at Ericsson | one-scheduler SMP VM about 10% slower (2008) [37] | — | none for Erlang code | yes, 12 years [37] |
| Lua | never | lock hook since about 2001, still a no-op by default [38] | — | 0 | — | — | n/a |
| JavaScript | isolation designed in; sharing added piecemeal | Workers → SAB (ES2017) → disabled Jan 2018 → re-enabled behind COOP/COEP (2020–21) → structs at Stage 2 [39] | — | 0 for non-users | separate heaps per worker | security (Spectre) forced a two-year rollback [39] | per-isolate |
| Java | designed in (1996) | memory model replaced 2004; biased locking 2006–2022; Loom 2017 → 2023 → JEP 491 in 2025 [40] | — | uncontended monitor cost led to biased locking, later judged not worth its complexity [40] | lock state in every header; compact headers needed new locking (10–20% less live data) [40] | double-checked locking broken before JSR-133 [40] | no |
| Julia | retrofit | 2014 → 1.3 (2019); parallel GC marking in 1.10 (2023) [41] | about 5 named contributors [41] | — | — | tasks could not migrate between threads at 1.3 [41] | runtime flag |
| Perl | retrofit ×2 | 5005threads (1998) removed in 5.10; ithreads since 5.8 [42] | — | — | an interpreter clone per thread [42] | ithreads "officially discouraged" [42] | build flag |

---

## 2. Histories

### 2.1 OCaml 4 → 5: the best-documented retrofit

**What had to change.** The minor heap became one per domain, carved from one reservation. The shared major heap
became mostly-concurrent and non-moving, with per-domain size-segregated allocation. `caml_modify` gained a deletion
barrier. Effect-handler fibers were added. Systhreads came to share a domain lock. (Mechanisms:
`threads-prior-art.md` §6.) The runtime was effectively rewritten [3].

The C API decided the design. The team built two minor collectors:
- **ConcMinor**: private minor heaps behind a read barrier;
- **ParMinor**: a stop-the-world parallel minor collection.

The paper says: "One primary culprit was the C API changes required by our original concurrent minor collector, which
motivated us to build alternative designs" [1]. Sivaramakrishnan's retrospective says the team rejected more scalable
GC designs "since it broke the C FFI compatibility" [3]. OCaml 5 shipped ParMinor.

**Breaking changes.**
- Naked pointers were removed. A checker mode shipped in 4.x first (#9534) [2].
- In the C interface, `&Field(v, i)` became `volatile value *` (#11255) [2].
- Deprecated stdlib functions were removed [3].

**Features dropped in 5.0 and restored later** [2]:

| Release | Restored |
|---|---|
| 5.1 (Sep 2023) | RISC-V, s390x and Cygwin; GC mark prefetching; frame pointers |
| 5.2 (May 2024) | POWER; compaction; part of statmemprof |
| 5.3 (Jan 2025) | the MSVC port; full statistical memory profiling |
| 5.4 (Oct 2025) | cleanup at exit |

The 32-bit ARM and i386 native backends were removed outright in 5.1 [2].

**Time and people.**
- The project began in March 2014 as Stephen Dolan's side project during his PhD. A commenter in the community thread
  recalls a standstill and a restart [5].
- It merged into mainline in January 2022 and shipped as 5.0 in December 2022 [3]. That is "nine years of work from
  when the original multi-core project was born to when it actually got upstreamed" [4].
- The 5.0 changelog credits eight named people plus "the Tarides multicore team" [2].
- Funding came from fellowships, Jane Street and others [1].
- The memory model was designed alongside the runtime (Dolan et al., PLDI 2018), and the retrofit inherited it [1].

**Single-thread tax.**
- *Paper, against 4.06.1:* ConcMinor 4.9% and ParMinor 3.5% geomean slower. ConcMinor's extra cost comes from its
  read barrier [1].
- *Paper, outliers:*
  - `game_of_life` was 20% slower, because exceptions became more expensive once the exception-pointer register was
    repurposed;
  - memory use was 54–61% *lower*, because the new allocator replaced next-fit; stock 4.10's best-fit allocator closes
    that gap [1].
- *In production:* Jane Street adopted 5.x "after 2.5 years of research and engineering effort". The headline cost was
  "on the order of like 3%-ish", but "some programs … were running 10% to 20% slower" and "some programs were using
  10% to 20% more memory" [4]. The causes were:
  - GC pacing;
  - floating garbage in the unified mark-sweep;
  - transparent huge pages;
  - stack checks for effects.
- Sivaramakrishnan lists regressions reported by Frama-C, Pyre, EasyCrypt and Infer. Tarides backported the OCaml 5
  allocator to OCaml 4 so it could be tested in isolation [3].

**What they would do differently** (Minsky [4]):
- test on large real programs earlier, because the benchmarks "didn't … embody a wide enough set of the potential
  different possible kind of program behaviors";
- build focused benchmarks that expose specific misbehaviours;
- prototype bad solutions faster, instead of "too much high-quality software engineering";
- rethink from first principles when incremental pacing fixes pile up.

**Prior attempt.** Doligez and Leroy built a concurrent generational GC for multithreaded Caml Light (POPL 1993). It
was abandoned, and OCaml stayed single-threaded until 5.0 [6].

### 2.2 CPython: free-threading (PEP 703), and the attempts before it

**Earlier attempts.**
- Greg Stein's free-threading patches against Python 1.4/1.5 (1996) and Adam Olsen's python-safethread "exhibited a
  sharp drop in single-thread performance (at least 30% slower), due to the amount of fine-grained locking" [12].
- Larry Hastings's Gilectomy (2016–2017) reached parity with CPython only "running on around seven cores to keep up
  with CPython on one". It was abandoned [13].

**What had to change** [7]:
- biased reference counting (owner-thread non-atomic, others atomic);
- immortal objects;
- deferred reference counting for functions and modules;
- mimalloc in place of pymalloc, with heaps separated by object kind so the GC can walk them;
- per-object mutexes and `Py_BEGIN_CRITICAL_SECTION`;
- lock-free optimistic reads of lists and dicts, made safe by QSBR-delayed page reuse;
- a non-generational, stop-the-world cycle collector;
- new C-API functions that return strong references (`PyList_FetchItem` and others) in place of borrowed-reference
  APIs.

"The largest contribution to execution overhead is biased reference counting followed by per-object locking" [7].

**Timeline and people.**
- Sam Gross announced the `nogil` fork (based on 3.9) in October 2021 and later rebased it on 3.12 [7].
- PEP 703 was accepted in 2023 under a phased plan [14]:
  - short term: experimental;
  - 1–2+ years: supported but not the default;
  - 5+ years: the default.
- The Steering Council kept the right to "change our mind if it turns out … too disruptive for too little gain", and
  said: "We do not want another Python 3 situation" [14].
- Meta committed "three engineer-years … between the acceptance of PEP 703 and the end of 2025" [15].
- 3.13 (October 2024) shipped it as experimental. PEP 779 made it supported in 3.14 (October 2025) [8].
- Phase III (the default) has no criteria and no date; PEP 779 leaves it to a future PEP [8].

**Single-thread tax over time.**

| Source | Tax |
|---|---|
| PEP estimate, `nogil-3.12` [7] | 6% on Skylake and 5% on Zen 3 with one thread; 8% and 7% with several |
| 3.13 [9] | "about 40% on the pyperformance suite", mostly because "the specializing adaptive interpreter (PEP 659) is disabled" |
| 3.14, at release [8] | about 10% on Linux and Windows, about 3% on macOS; the hard target is 15% |
| 3.14.8 docs [10] | "about 1% on macOS aarch64 to 8% on x86-64 Linux" |

To make the specializer safe, 3.14 added **thread-local bytecode**: each thread has its own copy of specialized
bytecode, and disabling it "also disables the specializing interpreter" [11].

**Memory tax.** About 15–20% more (geomean), against a 20% target [8]. The documented causes [10]:
- immortal interned strings;
- larger headers for non-GC objects (`None` is 32 bytes against 16);
- delayed frees under QSBR;
- mimalloc's per-heap overhead;
- queued biased-refcount deallocation.

**Ecosystem cost.**
- Extensions built for the GIL build "will fail to load (or crash)" on the free-threaded build [16].
- Free-threaded wheels need their own ABI tag (`cp313t`, `cp314t`).
- No stable ABI exists for the free-threaded build until PEP 803's `abi3t` (final March 2026, for 3.15), and adopting
  it "will require extension authors to make significant changes to their code" [16].
- Quansight-Labs maintains a tracker and ported much of the scientific stack: NumPy, SciPy, Cython, pybind11, PyO3
  and others [17].

### 2.3 Ruby: a GVL, then Ractors, then M:N, then a per-Ractor GC

**History.**
- Ruby 1.8 had green threads.
- 1.9 (2007, YARV) moved to 1:1 native threads under a Global VM Lock [19].
- Ractors arrived in 3.0 (December 2020), marked experimental with a runtime warning [18].
- 3.3 (December 2023) added an M:N thread scheduler. It is disabled on the main Ractor "because of compatibility
  issue (and stableness issue of the implementation)" [19].
- 3.4 (December 2024) added a modular GC, loadable through `RUBY_GC_LIBRARY`, with an experimental MMTk library [20].
- 4.0 (December 2025) reduced "contention on a global lock", introduced `Ractor::Port`, and still says "We aim to
  remove its 'experimental' status next year" [21].

**GC.** Every Ractor still shares one stop-the-world GC. Sasada's Feature #22227 (2026) proposes a heap per
Ractor [22]:
- a local GC with no VM lock;
- a global GC only for shareable objects;
- a containment invariant enforced by recording edges from shareable to unshareable objects.

Its numbers [22]:
- single-Ractor overhead of +2–4% on real workloads (+7% on an allocation microbenchmark);
- at 16 Ractors, JSON parsing takes 3.32× the wall time of one master Ractor, against 10.17× on master today and
  3.33× for forked processes.

So for five years, Ractors that allocate heavily scaled worse than `fork`.

**Lesson.** Ruby chose isolation to avoid making the VM thread-safe. It still needed years of lock-contention work,
and is now building per-Ractor heaps.

### 2.4 Racket: futures, places, a rewrite, then parallel threads

**Futures (OOPSLA 2010)** [25].
- The paper describes Racket BC as "roughly 100k lines of C".
- "Our own attempts to map Racket-level threads to OS-level threads failed due to the complexity of the runtime
  system."
- Futures run in parallel only until they reach an unsafe operation, which is a "slow-path barricading" technique.
- Cost: 41 expert person-hours and 536 non-expert hours, "one week of expert time, and one academic quarter of
  non-expert time". The non-expert hours include 480 of "exploration and discovery" (Figure 10).

**Places (DLS 2011)** [26].
- The audit covered **719 global variables**:
  - 337 were read-only singletons;
  - about 155 could be shared;
  - 227 had to become place-local.
- Threading a context structure through the whole runtime "would have required extensive modifications to function
  signatures and code flow". Instead, places used OS thread-local storage, with a pointer to the table kept in a
  register for the GC and the JIT.
- Effort: "approximately two graduate-student years, which is at least four times longer than we originally
  expected."
- Earlier work to introduce concurrency into the runtime had needed "months of additional testing and use … to
  uncover many race conditions that escaped detection by the test suite". Places were more reliable because little
  is shared between them.
- A bottleneck appeared in the OS: `mprotect` takes a process-wide page-table lock. Concurrent GCs in several places
  contended on it until allocation was batched into larger blocks.

**Racket CS (2017–2021)** [27][23].
- Racket was rebuilt on Chez Scheme: 2 years to an ICFP 2019 report passing all but 26 of 813,950 tests, then the
  default in 8.0 (2021).
- The rebuild moved Racket's roughly 15k-line thread layer and 15k-line I/O layer from C into Racket [27].
- Racket added foreign-thread activation and compare-and-set to Chez [27].
- Flatt parallelized Chez's collector in September 2020, with per-segment owners and sweeper threads [30]. It shipped
  in Chez 10.0 [28].

**Parallel threads (Racket 9.0, November 2025)** [23]. What had to change, in the blog's words:
- "new implementations of the Racket thread scheduler and I/O layer in Racket itself (instead of C)";
- "making Racket's coroutine thread scheduler cooperate more with the future scheduler";
- "making the I/O layer safe for Chez Scheme threads";
- "making locks fine-grained enough to enable parallelism, and also keeping the cost of needed synchronization as
  low as possible";
- adding fences on weak-memory platforms.

Cost and limits [23][24]:
- "up to 6-8% for programs that do not use them". The worst rows are mutable-hash benchmarks: ×0.98–0.96 and
  ×0.94–0.92 against 8.18.
- I/O locks remain "too coarse-grained" (×1.3–1.6 on directory hashing).
- "All collections (including minor collections) synchronize all threads", which limits scaling beyond 6–8 tasks.
- Parallelism "exposed some bugs in our existing core libraries".

**Lesson.** Racket's first direct attempt at OS threads failed. Each later step was cheap only because the previous
one had restructured the runtime: places localized the globals, and Racket CS moved the scheduler and I/O out of C.

### 2.5 Chez Scheme: a split build designed in, measured here

**Design.** Chez builds threaded and non-threaded machine types from one source.
- The threaded build keeps a thread context `tc` in a register.
- It emits a store-store fence (`dmb ishst` on arm64) before a remembered store (`s/cpprim.ss:652-664`,
  `s/arm64.ss:2012`).
- It makes parameters per-thread.
- It takes mutexes on the slow paths.

The manual says non-threaded builds "are faster for single-threaded applications" without giving a number
(`csug/preface.stex:36-40`).

**History.** Dybvig described the thread system in "A Scheme for native threads" (2009) (`csug/csug.bib:519`). Two
later changes came in the 10.x series [28]:
- 10.0 (2024) added parallel collection, enabled automatically when several threads are active, and made threaded
  the configure default (`release_notes.stex:429-433,516-523`);
- 10.1 fixed fences missing on tppc32le and in the portable bytecode variant (`release_notes.stex:3284-3289`). Even a
  designed-in threaded build had fence bugs years later.

**Measured tax [P].** §6 has the method. With one thread, the threaded build costs 0–1% on calls and allocation,
3.5–3.9% on pointer stores into old objects (the fenced path), 4.4% on string-port output, and 1.5% geomean.
Hashtable operations were not slower. This is the closest analogue to Patina's planned `threaded` cargo feature.

### 2.6 Guile: pthreads from 1.8, collector from BDW

- Guile 1.0 had cooperative user-level threads. 1.8 (2006) replaced them with POSIX threads, keeping Guile's own GC.
  2.0 (2011) switched to the conservative Boehm–Demers–Weiser collector [31].
- "Up to Guile version 1.8, a thread blocked in guile mode would prevent the garbage collector from running."
  Embedders had to bracket blocking calls with `scm_without_guile`. From 2.0, BDW's signal-based stop-the-world
  removed that requirement [31].
- A continuation may be used only in the thread that created it [31].
- No single-thread tax has been published. Guile's later Whippet work exists partly to get precise, parallel and
  moving collection without BDW (`threads-prior-art.md` §5).

**Lesson.** Delegating threads to a conservative collector made the retrofit cheap. The price was imprecision,
non-moving objects, and a restriction on cross-thread continuations.

### 2.7 Gambit: green threads designed in, SMP still opt-in

- Feeley's 1993 PhD was on futures on large shared-memory multiprocessors [33]. Gambit's production threads are
  nonetheless green, and Feeley wrote SRFI 18 around them.
- The local checkout shows the SMP work's history [32]:
  - commits in January–February 2017 port fixes "from smp branch";
  - in August 2017 the SMP scheduler was merged behind `--enable-smp`, described as "currently experimental";
  - `configure.ac:1289-1293` still defaults to `no` (checkout of 2026-08-27);
  - SMP fixes continue, for example in October 2022.
- The SMP design uses per-processor allocation areas and a phased parallel GC behind `BARRIER()`s
  (`threads-prior-art.md` §3).
- No overhead figure is published.

**Lesson.** Even with the language's own threads author and a scheduler written for it, SMP has stayed opt-in for
9 years.

### 2.8 GHC: concurrency designed in, the parallel RTS added later

- Concurrent Haskell's green threads predate parallelism. The shared-memory parallel RTS came in 2005 (Harris, Marlow
  and Peyton Jones) and shipped in GHC 6.6 (2006) [34]. Parallel copying GC followed in 2008.
- **Thunk evaluation was the expensive part** [34]:
  - adding two CAS instructions per thunk "increases execution time by an average 50% with a maximum of 300%";
  - "in an earlier complete (but now-bit-rotted) implementation, we observed execution time increasing by 100% when
    locking thunks";
  - a lock-free scheme, which tolerates duplicate evaluation, brought the cost to "around 6% of runtime", with
    outliers (`treejoin` +41%).
- Marlow and Peyton Jones's local-heap collector (ISMM 2011) beat stop-the-world in throughput, but its benefits were
  "offset to some extent by the extra work … to maintain the global-heap invariant" [35]. As far as I could find, it
  was never adopted in mainline [I].
- **Two runtimes ever since.** The non-threaded RTS is still the default for executables. An accepted proposal to make
  `-threaded` the default lists the problems of the non-threaded RTS (blocking FFI calls, no IO manager, deadlocks) but
  had not landed as of 2022, held up by test-suite work [36].

### 2.9 Erlang/BEAM: per-process heaps designed in, SMP added later

- Process isolation and per-process heaps date from the beginning. Parallel execution came with the SMP emulator in
  R11B (May 2006). It was transparent to programs and was tuned over the following releases (R12B–R13B), with the
  run queue and ETS locks as the bottlenecks [37].
- "The SMP VM with only one scheduler is slightly slower (10%) than the non SMP VM", because it locks shared data
  structures (Lundin, 2008, quoted in [37]).
- Both emulators were maintained for 12 years. Non-SMP was deprecated in OTP 20 and removed in OTP 21 (2018). The
  OTP team did "not want to spend time on implementing the dirty-scheduler feature for threaded non-smp", and "the
  different variants also increase our test-scope by quite a lot" [37].
- Shared-heap and hybrid-heap experiments (Johansson, Sagonas and Wilhelmsson) were shipped as options and removed in
  R15B02 (2012). The team had "no immediate plans to make hybrid heap and SMP work together" [37].

**Lesson.** Isolation designed in made the step to parallelism transparent to programs. Even so, it cost about 10%
of single-thread speed and 12 years of two builds.

### 2.10 Lua: never

- Since about 2001, Lua has had a global-lock hook: `lua_lock` and `lua_unlock` around every API entry. It compiles
  to `((void) 0)` unless an embedder defines it (`llimits.h` in 5.4.6, lines 259–266) [38].
- Ierusalimschy described the whole core as "one single critical region". The hooks are macros, not functions, so
  that programs which do not need threads pay nothing [38].
- Parallelism comes from libraries such as Lanes, which use one separate Lua state per OS thread and copy values
  between states through "keeper states" [38].

**Lesson.** A GIL-shaped hook can be designed in for free. Nobody built a shared heap on it.

### 2.11 JavaScript: isolation designed in, sharing added piecemeal

- Workers are separate isolates with separate heaps.
- `SharedArrayBuffer` and `Atomics` (ES2017) share bytes only. All browsers disabled SAB on 5 January 2018 for
  Spectre. It returned on desktop Chrome 67 with site isolation, then everywhere only behind COOP/COEP cross-origin
  isolation (Firefox 79, July 2020; Chrome on all platforms by 91–92, 2021) [39].
- Objects arrive through the TC39 structs proposal (Stage 2; draft December 2024). It rests on the rule "there are no
  references from shared objects to non-shared objects", with `Atomics.Mutex` and `Atomics.Condition` [39].
- WebAssembly's shared-everything threads are at Phase 1 [39].
- Engine cost: Wingo's survey of V8's collector puts "preparation for multiple JavaScript and WebAssembly mutator
  threads" at **about 20% of GC development effort** over the period. One example is 64-bit alignment of shared
  objects to prevent tearing under pointer compression [39].

**Lesson.** Even with isolation designed in, demand for sharing arrived. Retrofitting it into a single-mutator GC is
consuming about a fifth of V8's GC team's effort before any user-visible shared objects ship.

### 2.12 Java: designed in from 1.0, and still paying for the original choices

- **Memory model.** The 1996 memory model was unsound: final fields could be seen uninitialized, and double-checked
  locking was broken. JSR-133 replaced it in Java 5 (Final Release September 2004) [40].
- **Per-object monitors.** Every object is lockable. Synchronized collections (`Vector`, `Hashtable`) made uncontended
  locking hot, and biased locking (around 2006) was added to remove the atomics. JEP 374 (JDK 15) deprecated it:
  "performance gains seen in the past are far less evident today", and "biased locking introduced a lot of complex
  code … an impediment to making significant design changes". It was removed in JDK 18 [40].
- **Headers.** Stack-locking overwrote the mark word. Compact object headers (JEP 450) had to change locking so "locking
  operations no longer overwrite the mark word". Early adopters see 10–20% less live data from compact headers
  overall [40].
- **Monitors tied to OS threads.** Loom was proposed in October 2017 and virtual threads went final in JDK 21
  (September 2023). Until JEP 491 (JDK 24, 2025) a virtual thread inside `synchronized` pinned its carrier, because
  "the JVM … tracks which platform thread holds the monitor, not which virtual thread" [40].
- **Per-thread polls.** Thread-local handshakes (JEP 312) replaced a global safepoint page for under 1% [40].

**Lesson.** Designing parallelism in did not make the original choices free. A per-object lock, a weak memory model,
and lock ownership by OS thread were all re-engineered decades later.

### 2.13 Others, briefly

- **Julia.** Making the runtime thread-safe began around 0.3 (2014). `@threads` was experimental in 0.5 (2016), and
  composable task parallelism shipped in 1.3 (2019): "it took much longer than expected — nearly two years". At 1.3,
  tasks could not migrate between threads. Parallel GC marking came in 1.10 (2023) [41].
- **Perl.** `5005threads` (shared everything, 1998) were deprecated in 5.8 and removed in 5.10. Interpreter threads
  clone the interpreter, and "the use of interpreter-based threads in perl is officially discouraged" [42].

---

## 3. Patterns, with the numbers side by side

### 3.1 Where the time went

| Cost centre | Evidence | Size |
|---|---|---|
| Process-global runtime state | Racket places: 719 globals audited, 227 localized; threading a context struct judged too invasive, so OS TLS instead [26] | 2 grad-student-years, 4× the estimate [26] |
| Reference counting | CPython: biased RC is the largest single-thread cost, per-object locks second [7] | 30%+ in the old attempts [12]; 5–8% now [7][10] |
| C API / FFI exposure | OCaml chose ParMinor to keep the C API [1][3]; naked pointers removed; `&Field` volatile [2]. CPython: borrowed references replaced [7]; ABI incompatible until `abi3t` [16] | OCaml: a GC design choice. CPython: about 3 years for the stable ABI |
| Shared mutable library structures | Racket: hash tables and ports carry the 6–8% [23]; Erlang: ETS table locks the bottleneck [37] | 2–8% single-thread; scaling limits |
| Specialization, inline caches, JIT | CPython 3.13 disabled the specializer (≈40%) [9]; 3.14 per-thread bytecode copies [11] | one release cycle; extra memory |
| Thunks and lazy updates (GHC) | CAS per thunk +50% average, +300% worst [34] | solved by tolerating duplicate evaluation (6%) |
| GC pacing on real workloads | OCaml at Jane Street: 10–20% time and memory on some programs; 1.5 years of fixes [4] | 2.5 years to production |
| Fences and the memory model | Java: 8 years to a sound model [40]; Chez 10.1 fence fixes [28]; Racket added fences [23] | ongoing |
| Race testing | Racket BC needed months of extra testing after adding concurrency [26]; Erlang cited test scope [37] | months per change |

### 3.2 Single-thread tax: naive against engineered

| | Naive | Engineered |
|---|---|---|
| CPython | ≥30% (1.5 era) [12]; Gilectomy needed about 7 cores to match one [13]; 3.13 ≈40% [9] | 1–8% (3.14) [10] |
| GHC | +50% average and +300% worst (CAS); +100% (earlier attempt) [34] | about 6% [34] |
| OCaml | ConcMinor 4.9% [1] | ParMinor 3.5% [1]; production 10–20% → fixed [4] |
| Racket | direct OS threads abandoned [25] | ≤6–8% [23] |
| Erlang | — | about 10% (2008) [37] |
| Chez | — | 1.5% geomean, 3.5–4.5% on fenced stores and ports [P] |

### 3.3 Two builds, and how long they lasted

| Runtime | Two builds | How it ended |
|---|---|---|
| Erlang | 2006–2018 (12 years) | non-SMP removed; test scope and feature parity cited [37] |
| GHC | about 2006–today (20 years) | `-threaded` default accepted, not implemented as of 2022 [36] |
| Chez | 2009 or earlier – today (17+ years) | threaded became the default in 10.0; non-threaded remains [28] |
| CPython | 2024 – Phase III, years away | Phase III undecided [8][14] |
| Gambit | 2017 – today (9 years) | SMP still opt-in [32] |
| Racket | BC and CS 2019–2025 | BC dropped Aug 2025 [23] |

### 3.4 Built in early, later removed or reworked

- Caml Light's concurrent GC (1993): abandoned [6].
- Perl `5005threads` (1998): removed in 5.10 [42].
- Erlang hybrid and shared heaps: removed in R15B02 [37].
- GHC local heaps (2011): not adopted [35] [I].
- Java biased locking (around 2006–2022): removed [40].
- Java's original memory model (1996–2004): replaced [40].
- Java monitors keyed to platform threads: reworked by JEP 491 [40].

All of these are **mechanisms**: a collector variant, a heap layout, a lock protocol. Section 0 item 8 lists the
**representation** choices that later steps reused.

---

## 4. Lessons for Patina: what to defer and what to decide now

Patina's state, from `../../design/DESIGN.md` §12 and E.7, `../../research/threads-patina-cost.md` §1 and
`itemize.md`:
- `SharedHeap = Rc<RefCell<Heap>>`;
- 588 `Rc<`, 115 `RefCell<` and 19 `thread_local!` statics;
- 787 heap-borrow sites;
- mutator accounting held on `Heap`;
- every id counter already a process-global atomic;
- an embedding API being redesigned around `Owned` handles and branded `with(&mut self)` (decision 13).

### 4.1 What prior art says the design already gets right

| DESIGN §12 rule | Prior art that rewards it | Prior art that paid for lacking it |
|---|---|---|
| A `Mutator` (carrier) context separate from `GreenThread`; no runtime state in `thread_local!` | Chez `tc`, OCaml `Caml_state`, Gambit `___ps` | Racket places fell back to OS TLS because threading a context was too invasive at that point [26] |
| Runtime graphs move into the traced heap; never `Rc`→`Arc` | Chez, OCaml, Gambit (no RC) | CPython: RC dominates the tax [7]; earlier attempts ≥30% [12] |
| Interfaces written for N mutators; mechanisms behind `threaded`, compiled out by default | Chez split build: 1.5% even when on [P] | Erlang and GHC carried two *divergent* runtimes for 12–20 years [36][37] |
| A stop-the-world parallel collection target, with no read barrier | OCaml chose ParMinor over ConcMinor for C-API compatibility and speed [1]; Racket and Chez stop-the-world [23] | GHC local heaps' complexity [35] |
| SRFI 18 mutexes owned by thread objects in the heap | — | Java monitors keyed to platform threads blocked Loom until JEP 491 [40] |
| Inline caches fit in one word; code installed through one function | HotSpot handshakes for patching [40] | CPython 3.13 ≈40% with the specializer off; 3.14 per-thread bytecode copies [9][11] |
| Deep-bound parameterization as heap data | Racket parameterizations migrate with continuations (`threads-prior-art.md` §2) | — |
| A deterministic two-mutator lane on one OS thread | — | Racket BC's months of race testing [26]; Erlang's test scope [37] |
| Defer OS-thread carriers until there is demand (decision 7) | Every mechanism built ahead of demand in §3.4 was removed or reworked | — |

### 4.2 What prior art says is expensive to retrofit and cheap to decide now (not yet in the design)

1. **A language-level memory model for racy programs.** Java took 8 years to fix its memory model [40]. OCaml designed
   its memory model alongside the runtime [1]. Racket guarantees memory safety for racy code with fences [23]. DESIGN
   §12 lists the *mechanisms* (`HeapSlot`, the funnel's release fence, sub-word atomics, acquire loads), but no
   *statement* of what a racy Scheme program may observe. A short normative paragraph would fix the target for
   `HeapSlot` and the JIT's allocation sequences without implementing anything. Its likely content:
   - no crash;
   - no torn `TaggedValue`;
   - no observation of an uninitialized object;
   - mutex and condition-variable operations give happens-before;
   - everything else is unspecified, but every read returns some written value.
2. **A concurrency policy for each mutable library structure and runtime table.** Racket's 6–8% is in hash tables and
   ports [23]; CPython locks every dict and list [7]; Chez documents hashtables and buffered ports as *not*
   thread-safe, so user code locks them [28]. Patina's candidates:
   - hashtables (SRFI 69 and 125, R6RS);
   - string and file ports;
   - parameter objects;
   - records with mutable fields;
   - the symbol table;
   - the library registry;
   - the code store;
   - the macro expander's tables.

   Deciding now between "unsynchronized; user locks" (Chez, no tax) and "internally synchronized" (Racket/CPython,
   with a tax) costs a paragraph. Discovering it later costs an audit like Racket's 719 globals.
3. **An embedding rule that no interior reference escapes.** OCaml lost naked pointers and made `&Field` volatile [2];
   CPython replaced borrowed-reference APIs and needs a new stable ABI [7][16]. Decision 13's `Owned` handles and
   branded `with` already point this way, and stage 3 deletes `vector_slice_mut`, `get_string_chars_mut` and
   `get_bytevector_mut`. The rule should be written as an invariant for every future host-facing API: no `&`/`&mut`
   into heap words outlives one operation, and no pointer or index is retained across a call that may collect.
4. **A single-thread tax budget for the `threaded` build, measured from the day the feature exists.** Stage 9 budgets
   1% for the N-ready choices with `threaded` *off*. Nothing budgets the build with `threaded` *on* and one mutator.
   Chez's 1.5% [P], Racket's ≤6–8% and CPython's 1–8% bracket the target. A kill criterion of the K-series kind (for
   example ≤3% geomean with one mutator, ≤5% memory) would keep the build honest while no lane runs it. Without one,
   Erlang's and GHC's long-lived divergent builds are the precedent.
5. **Benchmarks that represent real programs, and a steady-state memory metric, before the step to N.** Jane Street's
   10–20% regressions were invisible to the benchmark suite [4]. The owner's steady-state concept gives a memory
   baseline that a threaded build can be compared against, per workload.
6. **Blocking sections are entered only with no unrooted values.** DESIGN §12 lists this as a `threaded` obligation.
   Guile ≤1.8 shows the failure mode: one blocked thread stopped every collection [31]. The rule belongs in the
   rooting contract from stage 3, as the design already places it.

### 4.3 What to keep deferred, with prior-art reasons

- **OS-thread carriers, handshakes, the `Send`/`Sync` heap, and the TSan, loom and Miri lanes** (DESIGN §12,
  "Deferred"). Every mechanism built ahead of demand in §3.4 was removed or reworked. Racket and OCaml built theirs
  when users asked.
- **A concurrent or per-thread minor GC.** OCaml measured ConcMinor and rejected it [1]; GHC's local heaps were not
  adopted [35]; Ruby is building one only because Ractors make heap partitioning a semantic guarantee [22].
- **A threaded tree-walker.** Every runtime above kept one execution engine parallel-capable. Racket dropped BC rather
  than make it parallel [23].

---

## 5. Calibrating Patina's estimates [I]

The in-house estimates are:
- `threads-patina-cost.md`: 9–18 engineer-months from today;
- DESIGN E.7: 4–9 engineer-months after stage 9;
- `itemize.md`: 8–16 engineer-months after stages 0–9, or 6–12 with its §7 additions.

All are built by analogy. Prior art suggests four adjustments.

- **Overrun.** Racket's places came in at 4× the estimate [26]. Julia's runtime work was "much longer than expected"
  [41]. OCaml's expected-to-actual ratio is unknowable but spanned 9 years [4]. CPython's tax estimate was off by 7×
  in the first release [7][9]. A ×2–×4 planning factor is consistent with the record.
- **Recovery after the first release.** OCaml spent about 2 years (5.1–5.4) restoring dropped features [2], and its
  largest user 2.5 years on performance [4]. CPython needed one more release to go from about 40% to single digits
  [9][10]. Budget a recovery phase of at least the same length as the build phase.
- **What dominates.** In every retrofit the GC mechanisms were a minority of the work. The majority was:
  - global state;
  - reference counting;
  - the host API;
  - library data structures;
  - caches and specialization;
  - testing.

  Patina's redesign removes much of the first three (stages 1–5, 4g, decision 13). The last three remain.
- **Second build.** Expect the `threaded` build to live alongside the default for a long time (12–20 years at Erlang
  and GHC). Its cost is CI time and test scope, which Erlang named as a reason to merge [37]. Chez's model (one source,
  a small compile-time delta, threaded made the default once cheap) is the one to copy.

Taken together, a planning figure for "supported, optional, ≤5% tax" is on the order of **1–3 engineer-years of focused
work over 2–4 calendar years** after stage 9 for a team of one or two. Most of the uncertainty lies in the library and
cache rows, not the collector. Decisions 1–4 of §4.2 are the cheapest way to shrink it.

---

## 6. Measurement [P]: Chez Scheme threaded against non-threaded on Apple M4 Pro

**Question.** What does Chez's designed-in threading support cost a single-threaded program on arm64? This is the
closest analogue to Patina's planned `threaded` feature: one source, a thread-context register, store-store fences
before remembered stores, and per-thread parameters.

**Builds.** Both builds come from one source tree: upstream ChezScheme at `7d82bd86` (2026-09-05), version 10.5.0,
with Racket's vendored `nanopass`, `lz4`, `zlib` and `stex`. They are `tarm64osx` (threaded; `(threaded?)` ⇒ `#t`) and
`arm64osx` (non-threaded; ⇒ `#f`). Both were built from portable bytecode by a sibling task, in
the study working directory's `chez/src-t` and `src-nt` (not retained). I used the binaries read-only.

**Programs.** `PRD/study/gc/probes/followup/prior-art-chez/bench.ss`, run with `--script` at the default optimize level. The
measure is CPU milliseconds per kernel. There are 5 runs, alternating builds per run, and the table reports medians.
Raw data is in `results.tsv`; `run.sh` reproduces it.

| Kernel | What it stresses | Non-threaded (ms) | Threaded (ms) | Threaded / non-threaded |
|---|---|---|---|---|
| `fib 40` | calls, trap checks | 370 | 373 | 1.008 |
| `tak` ×12 000 | calls | 743 | 743 | 1.000 |
| `alloc-churn` | allocation, minor GC | 899 | 904 | 1.006 |
| `store-old` | 300 M `vector-set!` of fresh pairs into an old vector (barrier + fence) | 1131 | 1171 | 1.035 |
| `setcar-old` | `set-car!` of fresh vectors into old pairs (barrier + fence) | 1002 | 1041 | 1.039 |
| `hash-ops` | eqv-hashtable set and ref | 1006 | 991 | 0.985 |
| `port-ops` | string output ports | 864 | 902 | 1.044 |
| `param-ops` | parameter reads | 1619 | 1627 | 1.005 |
| **geomean** | | | | **1.015** |

**Reading.**
- With one thread, the threaded build's cost is concentrated where Patina's would be: the fenced store path (+3.5–3.9%)
  and ports (+4.4%).
- Calls, allocation and parameter reads cost 0–1% even though `tc` is indirect. Thread parameters cost nothing
  measurable here.
- The store kernels allocate a fresh object per store, which dilutes the fence's share. A kernel that stores
  pre-existing objects would show more of it.
- Run-to-run spread was 1–5%, so only the store and port rows are clearly above noise.

**Not measured.** Multi-threaded runs, parallel GC, contention, and memory.

---

## 7. Candidate GitHub issues observed while writing this

The owner has asked that observed problems be filed as issues. These are proposals; nothing has been filed.

1. **GC/threads: state a memory model for racy programs** (§4.2 item 1). It belongs in `PRD/GC_PRD.md` beside
   decision 7, and constrains `HeapSlot`, the funnel fence and the JIT's allocation sequences.
2. **GC/threads: decide the concurrency policy for each mutable library structure and runtime table** (§4.2 item 2):
   hashtables, ports, parameters, mutable records, the symbol table, the library registry, the code store and the
   expander's tables.
3. **Embedding: write the "no interior reference escapes" invariant** into decision 13 and `docs/GC_DESIGN.md`
   (§4.2 item 3).
4. **GC/threads: add a tax budget and kill criterion for the `threaded` build with one mutator** (§4.2 item 4), and a
   lane that measures it once the feature has bodies.

---

## 8. Limits of this report

- Team sizes are known only for OCaml (paper and changelog credits), CPython (Meta's 3 engineer-years plus the named
  lead), Racket (the futures paper's person-hours) and GHC (paper authors). Elsewhere they are unknown.
- I found no published single-thread tax for Gambit SMP, Guile's pthreads, Ruby's Ractor-era changes to mainline, or
  Julia's threading.
- That GHC never adopted local heaps is my reading; I found no explicit statement.
- The GHC `-threaded`-by-default status is as of the 2022 discussion I found [36]. I could not confirm whether a
  2024–2026 release changed it.
- The `ocaml.org` changelog page did not resolve during this session, so OCaml release facts come from the `Changes`
  files on GitHub [2].
- §6 is a microbenchmark set on one machine with 5 runs. It measures the threaded build's single-thread tax, not
  scaling.
- Shared working-directory collision: before seeing the sibling task's build, I extracted a copy of the Chez source into the
  root of the working directory's `chez/` (not retained). That overwrote the root-level files with identical ones from the same commit, and
  replaced the root's `nanopass`, `lz4`, `zlib`, `stex` and `zuo` with Racket's vendored copies. The sibling's builds
  in `src-t/` and `src-nt/` were already complete and were not touched.

---

## Sources

Local (`~/Project/reference/…`, read-only):
- ChezScheme `7d82bd86`: `csug/preface.stex:36-40`, `csug/csug.bib:519`, `release_notes/release_notes.stex:429-433,516-523,3284-3289`,
  `s/cpprim.ss:652-675`, `s/arm64.ss:924-927,2012`.
- gambit `ab6f255c` (2026-08-27): `configure.ac:1289-1299`; `git log --grep=smp`.
- racket: `racket/src/ChezScheme/{nanopass,lz4,zlib,stex}` (vendored, used for the build).
- This corpus: `../../research/threads-prior-art.md`, `../../research/threads-patina-cost.md`,
  `../../research/threads-recommendation.md`, `../../design/DESIGN.md` §12 and E.7, `./itemize.md`.
- Measurement: `PRD/study/gc/probes/followup/prior-art-chez/{bench.ss,run.sh}` (`results.tsv` was not retained). The extracted
  paper texts and the OCaml `Changes` files were not retained; the sources are cited below.

Web:
1. Sivaramakrishnan, Dolan, White, Jaffer, Kelly, Sahoo, Parimala, Dhiman, Madhavapeddy. *Retrofitting Parallelism onto
   OCaml.* ICFP 2020. https://arxiv.org/abs/2004.11663 (the extracted PDF text was not retained).
2. OCaml `Changes`, branches 5.0 and trunk. https://raw.githubusercontent.com/ocaml/ocaml/5.0/Changes,
   https://raw.githubusercontent.com/ocaml/ocaml/trunk/Changes (#10831, #11255, #11904, #11418, #11712, #11642,
   #11827, #11144, #12276, #12193, #11911, #12954, #12964).
3. KC Sivaramakrishnan, notes on OCaml 5 adoption. https://hackmd.io/@kayceesrk/rJ5GtDTO3
4. Yaron Minsky, *The Saga of Multicore OCaml*, Jane Street tech talk. https://www.janestreet.com/tech-talks/the-saga-of-multicore-ocaml/
5. *When did the multicore OCaml project actually start?* https://discuss.ocaml.org/t/when-did-the-multicore-ocaml-project-actually-start/10053
6. Doligez and Leroy, *A concurrent, generational garbage collector for a multithreaded implementation of ML*, POPL
   1993. https://xavierleroy.org/bibrefs/Doligez-Leroy-gc.html. Its abandonment is reported in *Objective Caml for
   Multicore Architectures*, https://arxiv.org/pdf/2006.05862 (via search; not read in full).
7. PEP 703, *Making the Global Interpreter Lock Optional in CPython.* https://peps.python.org/pep-0703/
8. PEP 779, *Criteria for supported status for free-threaded Python.* https://peps.python.org/pep-0779/
9. Python 3.13 HOWTO, free threading. https://docs.python.org/3.13/howto/free-threading-python.html
10. Python 3.14 HOWTO, free threading (3.14.8). https://docs.python.org/3.14/howto/free-threading-python.html
11. Python 3.14 command line, `-X tlbc` / `PYTHON_TLBC`. https://docs.python.org/3.14/using/cmdline.html
12. Python FAQ, *Can't we get rid of the Global Interpreter Lock?* https://docs.python.org/3/faq/library.html
13. LWN, Language Summit 2018 (Gilectomy). https://lwn.net/Articles/754577/
14. Steering Council notice on PEP 703. https://discuss.python.org/t/a-steering-council-notice-about-pep-703-making-the-global-interpreter-lock-optional-in-cpython/30474
15. *A fast, free threading Python* (Meta's commitment). https://discuss.python.org/t/a-fast-free-threading-python/27903/99
16. PEP 803, *"abi3t": Stable ABI for Free-Threaded Builds.* https://peps.python.org/803
17. Quansight-Labs free-threaded compatibility tracking (via search); free-threaded wheels tracker. https://hugovk.dev/free-threaded-wheels/
18. Ruby 3.0.0 release. https://www.ruby-lang.org/en/news/2020/12/25/ruby-3-0-0-released/
19. Ruby M:N thread scheduler (Feature #19842, Ruby 3.3; text from the repository's thread documentation).
    https://bugs.ruby-lang.org/issues/19842; Koichi Sasada, RubyKaigi 2008 (YARV threads). https://rubykaigi.org/2008/pdf/rubykaigi2008_ko1.pdf
20. Ruby 3.4.0 release (modular GC, MMTk). https://www.ruby-lang.org/en/news/2024/12/25/ruby-3-4-0-released/
21. Ruby 4.0.0 release (Ractor). https://www.ruby-lang.org/en/news/2025/12/25/ruby-4-0-0-released/
22. Ruby Feature #22227, per-Ractor GC. https://bugs.ruby-lang.org/issues/22227
23. Racket blog, *Parallel Threads in Racket v9.0* (November 2025). https://blog.racket-lang.org/2025/11/parallel-threads.html
24. Racket Discourse, *Help test via snapshots: parallel threads.* https://racket.discourse.group/t/help-test-via-snapshots-parallel-threads/3920
25. Swaine, Tew, Dinda, Findler, Flatt. *Back to the Futures: Incremental Parallelization of Existing Sequential
    Runtime Systems.* OOPSLA 2010. https://users.cs.northwestern.edu/~pdinda/Papers/oopsla10.pdf
26. Tew, Swaine, Flatt, Findler, Dinda. *Places: Adding Message-Passing Parallelism to Racket.* DLS 2011.
    https://users.cs.northwestern.edu/~pdinda/Papers/dls11.pdf
27. Flatt et al. *Rebuilding Racket on Chez Scheme (Experience Report).* ICFP 2019.
    https://www-old.cs.utah.edu/plt/publications/icfp19-fddkmstz.pdf
28. Chez Scheme User's Guide and release notes (local paths above); Dybvig, *A Scheme for native threads* (Symposium in
    Honor of Mitchell Wand, 2009), cited at `csug/csug.bib:519`.
29. This report, §6.
30. Racket repository history, parallel Chez GC commits (September 2020), via the mirror at
    https://gitea.suzanne.soy/suzanne.soy/racket
31. Guile manual: *Threads*, *Blocking*, *Multi-Threading*. https://www.gnu.org/software/guile/manual/html_node/Threads.html,
    https://www.gnu.org/software/guile/manual/html_node/Blocking.html; Guile 2.0 and BDW (LWN). https://lwn.net/Articles/428288/
32. Gambit, local checkout (above); `INSTALL.txt`. https://raw.githubusercontent.com/gambit/gambit/master/INSTALL.txt
33. Marc Feeley, *An Efficient and General Implementation of Futures on Large Scale Shared-Memory Multiprocessors*,
    PhD thesis, Brandeis University, 1993. https://mathgenealogy.org/id.php?id=62754
34. Harris, Marlow, Peyton Jones. *Haskell on a Shared-Memory Multiprocessor.* Haskell Workshop 2005.
    https://www.microsoft.com/en-us/research/wp-content/uploads/2005/09/2005-haskell.pdf
35. Marlow, Peyton Jones. *Multicore Garbage Collection with Local Heaps.* ISMM 2011.
    https://www.cs.tufts.edu/~nr/cs257/archive/simon-marlow/local-gc.pdf
36. GHC proposal 0240, *Compile with threaded RTS by default.* https://ghc-proposals.readthedocs.io/en/latest/proposals/0240-threaded-by-default.html;
    https://discourse.haskell.org/t/make-ghc-threaded-the-default-rts/4220/10
37. Erlang/OTP R11B release. https://erlang.org/pipermail/erlang-announce/2006-May/000029.html; *Some facts about
    Erlang and SMP* (Lundin, 2008). https://erlang.org/pipermail/erlang-questions/2008-September/038231.html;
    *Deprecation and removal of the non-smp emulator* (2017). https://erlang.org/pipermail/erlang-questions/2017-April/092036.html;
    *Why is hybrid heap gone in R15B02?* https://erlang.org/pipermail/erlang-questions/2012-November/070567.html
38. Lua 5.4.6 `llimits.h`. https://raw.githubusercontent.com/lua/lua/v5.4.6/llimits.h; lua-l on `lua_lock` (2001).
    https://lua-users.org/lists/lua-l/2001-02/msg00101.html; Lua Lanes. https://git.lua4.win/lanes/plain/docs/index.html
39. Chrome, *Enabling SharedArrayBuffer.* https://developer.chrome.com/blog/enabling-shared-array-buffer; TC39
    proposal-structs. https://github.com/tc39/proposal-structs, https://tc39.es/proposal-structs/; Wingo, *The last
    couple years in V8's garbage collector* (2025). https://wingolog.org/archives/2025/11/13/the-last-couple-years-in-v8s-garbage-collector;
    WebAssembly shared-everything threads. https://github.com/WebAssembly/shared-everything-threads
40. JSR-133. https://www.jcp.org/en/jsr/detail?id=133; Pugh, *The Java Memory Model* and *Double-Checked Locking is
    Broken*. https://www.cs.umd.edu/~pugh/java/memoryModel/; JEP 374. https://openjdk.org/jeps/374; JEP 450.
    https://openjdk.org/jeps/450; JEP 444. https://openjdk.org/jeps/444; JEP 491. https://openjdk.org/jeps/491;
    *CFV: New Project: Loom* (27 October 2017). https://mail.openjdk.org/pipermail/announce/2017-October/000238.html;
    JEP 312. https://openjdk.org/jeps/312
41. Julia blog, *Announcing composable multi-threaded parallelism in Julia* (July 2019).
    https://julialang.org/blog/2019/07/multithreading; *Julia 1.10 highlights*. https://julialang.org/blog/2023/12/julia-1.10-highlights/
42. perldoc `threads` and `Thread`. https://perldoc.perl.org/threads
