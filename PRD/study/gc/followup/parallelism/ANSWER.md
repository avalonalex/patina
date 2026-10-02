# What would shared-memory parallelism cost Patina later?

Date 2026-10-01, revised the same day after two reviews (dispositions at the end). Repository `main` at `28a94f8`,
unmodified. This answers the owner's request to "understand the cost of adding shared-memory parallelism in the
future". It synthesizes three studies in this directory, which belong in the research corpus under
`PRD/study/gc/followup/parallelism/`:
- `itemize.md`: 31 cost rows, each costed from today's code and again after the redesign. This answer adds three
  rows (R32–R34) and re-prices R26.
- `measure.md`: the single-thread tax, measured.
- `prior-art.md`: what other runtimes paid.

It also draws on `design/DESIGN.md` §12, decision 7 and E.7, on `steady/SECTION.md` (steady state and limits), and
on `research/threads-*.md`. Where this answer and the three studies differ, this answer holds; their errata notes
point here.

**Labels.**
- **[M]**: measured on the development machine (Apple M4 Pro, arm64, macOS 27.2, Rust 1.97.1, release). "[M, Chez]"
  means Chez Scheme measured on that machine.
- **[E]**: an engineering estimate or judgement. Every effort figure is an [E]: focused time for one engineer who
  knows the codebase.
- **[A]**: an analogy, taken from a published figure for another runtime.

Part 1 is for the owner. Part 2 is the section to paste into `PRD/GC_PRD.md`. After them come notes for the editor
(reconciliations, design text to change, issues to file, sources) and the review dispositions. None of those
belong in the PRD.

---

# Part 1. For the owner

## Bottom line

Expect years, not months. After the GC redesign, the itemized work is 9–18 engineer-months. Other runtimes'
histories say to plan on two to four times that, plus a recovery phase: about 2–9 engineer-years in all. The range
is wide because the overrun factor rests on one quantified retrofit. The run-time cost to a single-threaded program
is small but not yet pinned down. The redesign removes nearly half of the engineering.

| Starting point | Work [E] | Confidence |
|---|---|---|
| Today's code, before or instead of the redesign | 79.5–145.5 weeks, **18–34 engineer-months**; plus 12–19 weeks for the SRFI 18 green threads it builds on | low |
| After redesign stages 0–9 and P | 38–77 weeks, **9–18 engineer-months**; plus 6–11 weeks for the SRFI 18 library | low–medium |
| The same, with the cheap additions below | 29–61.5 weeks, **7–14 engineer-months**, after 5–8 weeks spent on the additions during the redesign (net saving 1–10 weeks) | low–medium |
| Planning figure, the build: 9–18 months × 2–4 [A] | **1.5–6 engineer-years** (1.1–4.8 with the additions) | low |
| Planning figure, recovery after the first supported release [A, E] | **0.4–3 engineer-years**: a quarter to half of the build, spread over about as long again | low |
| **Planning figure, total** | **about 2–9 engineer-years** (1.4–7 with the additions), to a supported, optional build with at most 5% tax, recovered | low |

**The relative claim is solid.** 41.5–68.5 of the weeks counted from today (10–16 months) are work the redesign does
anyway: the block heap, the `Cx` codemod, precise rooting, heap continuations, global cells and the thread/carrier
split. Doing parallelism first would build those twice. Nothing in the redesign has to be undone.

**The absolute figures are weak, and the planning figure is a judgement.**
- The base sums 34 row estimates.
- The 2–4× factor has one quantified datum. Racket's places took "at least four times" their estimate, and places
  are share-nothing, message-passing parallelism. The other histories give no ratio:
  - Julia reports "much longer than expected";
  - OCaml's 8¾ years has no estimate to compare against;
  - CPython reached "supported" inside the window its Steering Council set at acceptance, but after two years of
    prototyping.
- Recovery is priced from elapsed-time histories:
  - OCaml restored the features 5.0 dropped over about 2 years;
  - Jane Street took 2.5 years to adopt OCaml 5;
  - CPython took one release to go from about 40% to 1–8% tax.

  No history gives recovery's engineering time, so its quarter-to-half share is an [E].
- After stage 9, re-estimate the rows using the redesign's own measured overrun (K14). That replaces the borrowed
  factor with data from this codebase.
- These figures supersede decision 7's 4–9 months after stage 9 and ST's 9–18 months from today (see the
  reconciliations below).

## The ongoing single-thread tax

- **Measured [M]** in a patched build of today's interpreter, on 11 workloads. A twelfth workload, a code-layout
  outlier, is excluded; with it, the first figure below is +2.0%.
  - DESIGN §12's ordering obligations cost **+1.9% geomean**. The patch converted only some sites: acquire loads in
    `car`, `cdr`, `vector-ref`, cell reads, closure free variables and code ids, and release stores of heap values.
  - A fence after every allocation costs **+0.4%**.
  - Relaxed atomic heap words, sub-word atomics, the poll word and a locked block pool compile to today's
    instructions: **0**.
- **Estimated after the redesign [E]:**
  - **interpreter: ≈2–5%** if the absolute cost per instruction holds, on an interpreter assumed (not measured) to be
    ~30% faster.
    - **Up to ~7%** if the acquire loads' latency is exposed: 0.53 dependent loads per instruction × 0.54 ns on
      nboyer, at about 4 ns per instruction.
    - More if global-cell and record-field loads, which the patch did not convert, lengthen the chains.
    - The stage-5 re-measurement (an addition below) decides; until then the PRD states a range.
  - **JIT: 0–4%** with plain, dependency-ordered loads; acquire loads would cost 3–60%.
  - **x86-64:** about 0.
  - **A separate non-threaded build:** 0.
- **Hazards [M, microbenchmarks]:**
  - element-wise atomic copies are 3.2× slower per word;
  - an ordering point (`dmb` or `stlr`) sometimes costs 15–55 ns:
    - it appears after a fresh object is stored into an old holder, intermittently across process launches, for
      reasons not established;
    - in-situ code did not reproduce it: Chez's fenced `set-car!` of fresh pairs runs at 1.84 ns per iteration, and
      Patina's per-allocation fence costs +0.4%;
    - the S6 spike measures it;
  - an acquire load adds 0.54 ns on a dependent chain;
  - a global intern lock made Chez's `string->symbol` 50% slower [M, Chez].
- **Comparisons:**
  - Chez's threaded build is +0.8–3.5% over its non-threaded build, in two measurement sets here [M, Chez]. The
    +3.5% includes interning; its fenced-store loops ran −1.4% to +4.1%.
  - OCaml 5: 3.5%. CPython 3.14: 1–8%. Racket 9: up to 6–8% [A].
- **Memory: not measured, and not negligible.**
  - CPython's free-threaded build uses 15–20% more memory. Its documentation lists six causes, of which two are
    reference counting.
  - Patina has analogues of the others:
    - per-carrier allocation-buffer waste and block caches, like mimalloc's per-heap overhead;
    - epoch-deferred reuse of code memory, like QSBR's deferred frees;
    - floating garbage while N carriers allocate between pacing decisions.
  - Jane Street saw +10–20% on some OCaml 5 programs [A].
  - Part 2 lists the per-carrier terms, and the decision record adds a footprint gate.
- **Locking today's heap does not work.**
  - An `RwLock` on the heap and `Arc` on code objects cost at least +4.1% on one thread [M]. That is a lower bound:
    payload `Arc`s and per-object synchronization were not perturbed.
  - One shared count costs 97 ns at 4 threads [M]: more threads, slower program.

## What the redesign already pays for

- **Stage 5:** a non-moving heap with no `Rc` or `RefCell` inside objects, and a buffer per carrier (10.5–17 weeks of
  the from-today cost).
- **Stage 3:** the `Mutator`, the `Cx` codemod, and the one store funnel where atomics and ordering go (8–12 weeks).
- **Stages 2, 3 and 4e:** precise rooting, which removes the deadlock of a thread that cannot stop while Rust holds
  unrooted values; continuations that refer to no carrier.
- **Stage 9:** the thread split, deep-bound `parameterize`, and safe regions.

## What remains, and why

What remains exists because several threads run Scheme at once:
- the stop-the-world handshake and the M:N scheduler (lost wake-ups, and deadlocks among the collector, mutexes and
  I/O);
- publication ordering, whose bugs show only on weak-memory hardware;
- shared runtime tables, including Rust-implemented promise forcing and the source map;
- pacing across carriers, and native stack depth on carriers;
- testing (4–7 weeks) and first-pass tuning (3–6 weeks), then the recovery phase.

Other runtimes report the same: the collector was the smaller part.

## Risks

- **Deterministic lanes.**
  - Real threads are not byte-identical, so every lane stays byte-identical on the threaded build with one carrier.
  - Several carriers are tested first as a seeded simulation on one OS thread, which checks protocols but cannot
    observe reordering.
  - Ordering is then checked under loom (once decision 14 allows it), ThreadSanitizer (nightly only, and blind to
    standalone fences) and arm64 lanes (x86-64 hides publication bugs).
- **Dynamic-state transfers.** Cross-thread continuations and `thread-terminate!` inside `dynamic-wind` sit where
  this codebase's defects have clustered (#157–#163).
- **The ecosystem and embedding.**
  - Host primitives and payloads become `Send + Sync`.
  - Host threads attach as carriers, bringing native stacks Patina does not size.
  - Three bundled libraries keep library-wide mutable state that carriers would race on: SRFI 128's comparator
    registry, SRFI 27's default random source, and the R6RS hashtable layer's table of immutable copies.
  - Stating these rules before embedders depend on the API costs days. Later it is a breaking release, as OCaml's
    and CPython's C APIs found.
- **Native stack.**
  - Rust recursion in the expander and compiler aborts the whole process at about 980 nested `let`s with an 8 MiB
    stack, and at about 240 with 2 MiB, Rust's default for spawned threads [M].
  - Carriers must therefore be spawned with an explicit stack size, and the recursion guarded (R33).
  - Today's abort is a defect to file now: chibi and Gauche run 1,000 nested `let`s.
- **A long-lived second build.** Erlang kept two for 12 years; GHC has kept two for about 20.

## Cheap additions now

The additions cost about 5–8 engineer-weeks, spread over the stages. They save 9–15.5 weeks after the trigger, a net
saving of 1–10 weeks, and add about 4–9% to the redesign's 88–122 weeks. The main ones:
- fields reserved in the `Mutator` ABI and in `GcAttrs` before stage 6 freezes them;
- bulk-range accessors;
- a per-carrier collect capability;
- a seeded multi-carrier simulation lane;
- a written memory model;
- S6's publication spike;
- a re-measurement of the tax on the stage-5 representation.

## When to start

Not now. Start when all of these hold:
- a named workload needs a multicore speedup over shared mutable data, and isolates cannot give it;
- the redesign and SRFI 18 on M:1 have shipped, with their matrix rows passing;
- the tax measured on the stage-5 representation is within 5%;
- the engineering years of the planning figure are available, as re-estimated after stage 9.

After the start, the first step (the threaded build with one carrier) must pass time and footprint gates, each
within 5%.

There are two cheaper options:
- **Isolates, for share-nothing work.**
  - 3–5 weeks after stage 5e [E]; 6–20 weeks with the same factor, or 12–20 if Racket's 4× applies (its places are
    the closest analogue).
  - Each interpreter is built on its own thread, so no `Interpreter: Send` is needed.
- **The token, for more I/O or FFI concurrency than the helper pool gives.**
  - Several carriers share one mutator token, and only one runs Scheme at a time.
  - 9.5–15.5 weeks [E], or 19–62 with the factor.
  - There is no race on Scheme heap words, but Rust-side port and table state still needs locks.

---

# Part 2. PRD section (for `PRD/GC_PRD.md`)

## Threading readiness and future parallelism (decision 7)

**Status: not a goal now.**
- SRFI 18 ships as M:1 green threads, and the GC's interfaces are written for N carriers over one heap.
- Detail: `PRD/study/gc/followup/parallelism/` (`ANSWER.md`, `itemize.md`, `measure.md`, `prior-art.md`) and
  `PRD/study/gc/research/threads-*.md`.

Labels: **[M]** measured (Apple M4 Pro, arm64); **[E]** estimate; **[A]** analogy. Effort is [E], in focused
engineer-weeks.

### Cost

Target: N OS-thread carriers over one heap; SRFI 18 threads scheduled M:N; stop-the-world collection with parallel
marking.

| Item (`itemize.md` rows; R32–R34 added here) | Why | From today | After 0–9, P | Basis |
|---|---|---|---|---|
| Non-moving heap, no `Rc`/`RefCell` payloads, allocation buffers (R1, R2, R14) | racy reads of a growing arena are undefined behaviour | 11–18 | 0.5–1 | 4 `Vec` arenas; 14 `Rc` payload kinds; stage 5 |
| Heap API, collect capability, `VmState` split, `Send`/`Sync` (R10, R11) | a context per carrier; collection stops all | 10–17 | 3–6 | 767 borrow sites; stage 3 |
| Atomic slots, bulk operations, publication (R12, R13) | unzeroed holes make publication a memory-safety matter | 3–6 | 2–5 | the funnel; [M] costs; S6 decides placement |
| Rooting windows, time-to-safepoint (R16, R17) | a carrier that cannot stop stalls all | 5–11 | 2.5–6 | 21 `GcDeferGuard` uses; stages 2, 3, 4e |
| Handshake, safe regions, blocking I/O (R15) | one `read(2)` would block every collection | 4–7 | 2–3 | ~35 blocking primitives; stage 9 |
| Shared runtime tables: namespaces, code store, libraries, expander, ports, interner, caches (R3–R9, R22) | every carrier mutates them | 13–23 | 8–15.5 | stages 4a–4e; Racket's tax sits here [A] |
| Parallel GC with carriers as workers; store buffers; weak references (R18–R20) | serial GC caps 8 carriers at 3.2–6.9× (Amdahl) | 5–9 | 2.5–5 | stages P, 7; OCaml [A] |
| Pacing across carriers (R32) | N allocators between collections; floating garbage | 1–2 | 1–2 | pacing was one named cause of Jane Street's 2.5-year adoption (elapsed time, not effort) [A] |
| Continuations across carriers (R21) | SRFI 18 defines them | 3–5 | 1–2 | stages 4e, 9 |
| M:N scheduler (R26) | queues, parking, mutexes | 4–10 | 3–8 | stage 9; Loom (2017–2025), Gambit SMP (opt-in for 9 years) and Ruby's M:N (off by default) are elapsed-time records, not effort data [A] |
| Global state, bundled libraries, tree-walker kept M:1 (R23–R25) | 19 `thread_local!`s; 3 bundled libraries with shared state | 2.5–5 | 1.5–2.5 | source counts |
| Debugger, embedding, memory model (R27–R29) | all-stop; host threads attach | 4.5–7 | 2.5–4.5 | stages 2, 3; JNI [A] |
| Native stack depth on carriers (R33) | Rust recursion aborts the process; spawned threads get 2 MiB | 1–2 | 1–2 | ~980 nested `let`s at 8 MiB, ~240 at 2 MiB [M] |
| Promise forcing and the source map (R34) | Rust-implemented state that every carrier changes | 0.5–1.5 | 0.5–1.5 | `force` in a VM stub frame (#476); `Rc<RefCell<SourceMap>>`; GHC's thunk updates [A] |
| Tests (R30) | simulation, loom, TSan, arm64 lanes | 6–10 | 4–7 | none exist |
| First-pass tuning and scaling (R31) | contention, false sharing | 6–12 | 3–6 | [M] tax rows |
| **Total** | | **79.5–145.5 (18–34 months)** | **38–77 (9–18 months)** | |
| With the additions below | | — | **29–61.5 (7–14 months)**, after 5–8 weeks spent during the redesign | |
| SRFI 18 on M:1, which this presupposes (S) | | 12–19 | 6–11 | stage 9 |
| A multi-carrier JIT, if a JIT exists (J) | | 6–10 to retrofit | 2–4 | §11 contract |

**R33** has three parts:
- spawn carriers with an explicit stack size equal to the main thread's;
- add a Rust-recursion depth guard to the expander, compiler and printer that raises a Scheme error;
- check the remaining stack when a host thread attaches.

The guard also fixes today's abort. If it lands as that defect's fix, R33 drops to about 0.5 weeks.

**R34** has two parts:
- **Promises.** Classify them under rule 9. R7RS iterative forcing updates `done` and `value` and re-points chains,
  so racing forcers can see `done = #t` beside the old thunk. Allow duplicate evaluation with one atomic publish of
  the forced state (GHC's lock-free choice), or else use a CAS-based state protocol.
- **The source map.** Every `load`, `eval` and `read` that records locations changes it. Give it a lock, or per-document
  ownership.

**Calibration [A, E].** The planning figure is the itemized base times an overrun factor, plus a recovery phase.
- **Base:** 38–77 weeks.
- **Factor: 2–4×.** This is a judgement, anchored by one quantified retrofit, Racket's places (share-nothing,
  message-passing), which took at least 4× its estimate.
  - Julia ("much longer than expected") and OCaml (8¾ years) give no ratio.
  - CPython reached "supported" within the window set at its acceptance, after two years of prototyping.
- **Build:** 1.5–6 engineer-years (1.1–4.8 with the additions). Calendar time is this divided by the staffing.
- **Recovery after the first supported release:**
  - It lasts about as long again in calendar time [A]: OCaml spent about 2 years restoring dropped features, Jane
    Street 2.5 years adopting, and CPython one release reducing its tax.
  - It runs at a quarter to half the build's intensity [E]: 0.4–3 engineer-years.
  - R31 stays as first-pass tuning inside the build.
- **Total:** about 2–9 engineer-years (1.4–7 with the additions).
- **Re-estimate:** after stage 9, replace the factor with the redesign's own overrun (K14).

This supersedes decision 7's 4–9 months.

### Single-thread tax (the `threaded` build with one carrier)

| Mechanism | Measured [M] | After the redesign [E] |
|---|---|---|
| Relaxed atomic heap words, sub-word stores, poll word, locked block pool | same machine code: 0 | 0 |
| Acquire loads of heap references in Rust, release stores of heap values | +1.9% geomean on today's interpreter, 11 workloads (−0.1% to +3.6%; code-layout noise ±4%). Sites converted: `car`, `cdr`, `vector-ref`, cell reads, closure free variables, code ids | interpreter ≈2–5% if the absolute cost holds, up to ~7% if load latency is exposed; global-cell and record-field loads not yet measured; re-measured at the stage-5 exit. JIT: 0 with address-dependency loads (3–60% with acquire loads) |
| Publication ordering | after every allocation: +0.4% (interpreter). Microbenchmarks show an intermittent 15–55 ns per ordering point after fresh objects are stored into old holders; it is unexplained and was not reproduced in situ | 0–1% on heap-valued stores, judging by Chez (fenced-store loops −1.4% to +4.1% [M, Chez]); S6 decides the placement |
| Bulk copy and fill | element-wise atomics 3.2–3.3× slower per word | 0, with one word-atomic routine and one fence per range |
| Port and table locks | +0.7 to +3 ns, uncontended | 1–3% on I/O-bound loops, under `threaded` only |
| Interner lock | +0.7 ns (`parking_lot`) to +5.1 ns (`std`) on a 17 ns lookup; Chez's global mutex +50% [M, Chez] | ≈ 0, sharded |
| Context held in `thread_local!` | +0.51 ns per access | 0 (`Cx`, `x21`) |
| Locking today's heap (`RwLock`, `Arc` on code objects) | at least +4.1% at one thread (a lower bound: payload `Arc`s and per-object synchronization not perturbed); a shared count costs 97 ns at 4 threads and 370–550 ns at 8 | not used |
| **Total** | | **interpreter ≈2–5% (up to ~7%), JIT 0–4% on arm64; ≈ 0 on x86-64; 0 with a non-threaded build** |

Comparisons:
- Chez, threaded against non-threaded: +0.8–3.5% across two measurement sets; the +3.5% includes interning
  [M, Chez].
- OCaml 5: 3.5%. CPython 3.14: 1–8%. Racket 9: up to 6–8% [A].

### Footprint and limits under N carriers

Memory has not been measured. CPython's free-threaded build costs +15–20%, and only two of its six documented causes
are reference counting. Jane Street saw +10–20% on some OCaml 5 programs [A].

The steady-state footprint is the heap term (F(L), unchanged by N) plus these terms:

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

The steady-state limits table gains two rows:

| Limit | Counts | Default | At the limit |
|---|---|---|---|
| Native (Rust) stack | Rust recursion in the reader, expander, compiler and printer | the OS thread's stack (the main thread's `ulimit -s`; carriers spawned at the same size) | a catchable error from the depth guard; today, a process abort [M] |
| Stack address space | the green-thread stack reservations of one heap | set with the stack arena (decision 15) | `thread-start!` raises, as the thread limit does |

### Rules the plan keeps

1. **Carriers and green threads are separate.** The `Mutator` owns the buffers, roots, poll and safepoint state. A
   `GreenThread` owns its stack and dynamic environment. No runtime state lives in `thread_local!`.
2. **Heap words have one way in.** `HeapSlot`/`MetaByte` cover every access, and the funnel covers every store. No `&`
   or `&mut` into heap words outlives one operation or crosses the embedding API.
3. **No shared read-modify-write on a hot path.** Heap objects carry no `Rc`, `RefCell` or `Drop` payloads. Each
   carrier's mutable data sits on its own 128-byte line.
4. **Publication is an invariant; S6 decides how it is enforced.** Under `threaded`:
   - **(a) Ordering.** Every store that may make an object reachable by another carrier is ordered after that object's
     initializing stores, whatever the heap's `BarrierKind`.
   - **(b) Mechanism.** The mechanism is a `GcAttrs.publication` property (`FenceAtAllocation | ReleaseOnHeapStore |
     FenceOnHeapStore`), separate from the generational barrier.
   - **(c) Elision.** A store may skip publication only when its holder has not escaped since it was allocated. A
     holder escapes when its reference is stored into the heap or a global cell, passed to a non-`Leaf` call, or
     returned.
   - **(d) Bulk ranges.** A bulk range copy of heap values publishes with one store-store fence before the range.
     This is a named exception to (g).
   - **(e) Loads in Rust.** Rust code (the interpreter, primitives and frontend) loads heap references with Acquire.
   - **(f) Loads in JIT code.** Only JIT code may rely on address dependencies. It does so under a documented
     exception that forbids value speculation and equality substitution of loaded references.
   - **(g) Fences.** Prefer release and acquire operations where they suffice. Each standalone fence (the bulk range,
     C1's watermark if chosen, `set_limit`) is listed, and loom and the arm64 lanes check it, because
     ThreadSanitizer cannot.
5. **Protocols are written for N.** Request, acknowledge (at a poll or in a safe region), collect, release. Posters
   write only atomics. Safe regions are entered with no unrooted value. A mutex that must block tries first, then
   deactivates.
6. **One entry point per shared structure:** interning, namespaces, code installation. Inline caches are one traced
   word. Continuations refer to no carrier.
7. **Limits are per heap.**
   - Pacing sums every carrier's allocation (R32).
   - The store-buffer soft limit is divided among carriers.
   - Green-thread stacks share a per-heap bound on stack address space.
   - Carriers' native stacks have an explicit size, and Rust recursion is depth-guarded (R33).
8. **The tree-walker stays M:1 and `!Send`.**
9. **Concurrency policy.**
   - Under `threaded`, state implemented in Rust synchronizes internally: ports, tables, registries, promise forcing
     and the source map.
   - In the default build it is plain, behind C3's one wrapper.
   - Structures built in Scheme (hashtables, records, vectors) do not synchronize, and users lock them, as in Chez.
10. **The two-mutator lane stays green from stage 9 on.**

### Cheap additions adopted

| Addition (`itemize.md` §7) | Stage | Cost | Saves |
|---|---|---|---|
| Time-to-safepoint metric beside K16 (C12) | 0 | 1 day | makes R17 measurable; 0.5 week |
| Per-heap Rust tables behind one wrapper; loading entries record their thread (C3, C15) | 2 | 1–2 days | 1.5–2.5 weeks |
| Per-carrier collect capability (C2) | 3 | 2–3 days | 1–2 weeks |
| Reserve `fenced_ap`, a quiescence epoch and a handshake word in `Mutator`, and a `publication` field in `GcAttrs`; assert 128-byte alignment (C1, C10) | 3; frozen at 6 | 1–2 days | an ABI break; 0.5–1 week |
| Bulk-range accessors; `HeapSlot::compare_exchange` (C4, C5) | 3, 5d | 3–4 days | 1–2 weeks, and the 3.2× bulk loss |
| A CI check against new `thread_local!`, `static mut` and heap-side `Rc`/`RefCell`/`Cell` (today's 19 `thread_local!` statics allowlisted), plus `Send`/`Sync` assertions as each type first satisfies them (C9, C11) | 3, then 4e and 9 | 1–2 days | 0.5–1 week |
| Memory model: no crash, tearing or uninitialized object; every read returns some written value; happens-before only through mutexes, condition variables, thread start and join | 3 | 1–2 days | R29, 0.5–1 week |
| One read-mostly namespace API (C6) | 4b | 1–2 days | 1–1.5 weeks |
| Tax re-measurement on the new representation: the ordering perturbation, with global-cell and record-field loads added (a run, not a lane) | 5 exit | 2–3 days | the PRD's tax figure; start criterion 3 |
| S6 publication spike: four placements × four shapes, at least 10 process launches each, plus conformance tests | 6 | 3–5 days | decides rule 4's mechanism |
| A seeded multi-carrier simulation lane, generalizing the two-mutator lane (C8) | 9 | 1–2 weeks | 2–3 weeks |
| Stage P's worker loop callable from any thread, keeping H.4's segment stealing (C7) | P | 2–3 days inside P | 0.5–1 week |
| Threaded lanes run on arm64 (C17) | with the first threaded lane | 0 now | bugs that x86-64 hides |

The additions total about 5–8 engineer-weeks. C8 and C12 pay for themselves under M:1, and loom models wait for
decision 14 (dependencies).

**C11 by stage.**
- **Stage 3:** assert only types that already qualify, such as `InterruptHandle` and `PrimitiveRegistry`.
- **Stage 4e:** `CodeBody`.
- **Stage 9:** `GreenThread`, if its stack handle is plain data.
- **Decision 7:** `HeapShared: Sync` waits for it, because it needs the `unsafe impl` that §12 defers.
- **VM-heap payloads:** a `Send + Sync` bound first needs a separate payload trait-object type per heap kind,
  because tree-walker payloads stay `!Send`.

**C1** is the standalone-fence variant of rule 4: its watermark is sound only if `fenced_ap` advances at a
store-store fence. If S6 chooses release stores, C1 is dropped and its reserved word stays unused.

**The S6 spike.**
- **Placements:**
  - a `dmb ishst` per allocation group;
  - `stlr` on heap-valued stores;
  - `dmb ishst; str`;
  - C1's filtered fence.
- **Shapes:**
  - a tight `cons` loop;
  - a tail-building `set-cdr!` loop;
  - fresh objects stored into an old vector or hashtable, including Chez's `(set-car! old (cons i i))`;
  - bulk ranges.
- **Conformance tests:**
  - `BarrierKind::None` under `threaded`;
  - a store into a fresh holder that has already escaped.

Moved out of the additions:
- **C13** (`Owned: Send`) moves to R28 or to isolates. It needs a deferred-release queue drained at the owner's poll
  (2–3 days then).
- **C14** (stacks from one reservation) moves to the steady-state work, conditional on a Linux probe.
- **C16** (bundled libraries) moves to R25.

### Decision record

**Decision 7 (2026-10-01): not now.** SRFI 18 ships as M:1, with N-ready interfaces and the additions above.

**Isolates cover share-nothing parallelism.**
- **Cost:** 3–5 weeks after stage 5e [E]; 6–20 weeks with the 2–4× factor, or 12–20 if Racket's places (the closest
  analogue, at least 4×) apply.
- **No `Interpreter: Send`:** each isolate builds its interpreter on its own OS thread, and the compiler checks that
  the spawn closure captures only `Send` data.
- **A host that moves an interpreter between threads** uses an audited `unsafe impl Send` wrapper (1–2 weeks). It is
  sound only if:
  - no `Rc` reachable from the interpreter is shared outside it;
  - no `Owned` handle stays on the old thread, which a live-handle count can check, or else C13's queue is used;
  - the host's primitives and payloads are `Send`.
- **Converting the remaining `Rc`s instead** (3–6 weeks, plus atomic counts on every clone) is rejected.

**Start only when all of these hold:**
1. **Need:** a named workload needs a multicore speedup over shared mutable data that isolates cannot give.
2. **Readiness:** stages 0–9 and SRFI 18 on M:1 have shipped. The matrix rows for switches, cross-thread
   continuations and terminate are green on both backends, and the simulation lane is green.
3. **Estimated tax:** the stage-5 re-measurement is within 5% geomean on the GBS (cycles, with confidence intervals).
4. **Capacity:** the planning figure, re-estimated after stage 9, is available.

**Gates of the first step** (the threaded build with one carrier, every lane byte-identical):
- **Time:** within 5% geomean on the GBS (cycles, with confidence intervals).
- **Footprint:** committed bytes at the end of each steady-state probe's window within 5% of the default build
  (geomean).
- **Per carrier:** the footprint increment is measured and stated.
- **Cost of making it run:** R12's "run the feature and fix what breaks", 1–3 weeks.

**Cheaper first step: the token.** N carriers share one mutator token. Blocking I/O and FFI calls run in safe regions,
and one carrier runs Scheme at a time.
- **No atomic heap words or publication ordering.** Handing the token over is a mutex release and acquire, which orders
  every heap word.
- **Locks still needed.** Rust-side state reached from safe regions takes C3's wrapper locks: port buffers, the
  `PortTable`, the per-heap tables.
- **There is no race on Scheme heap words**, but there are Rust-side locks.

| Work for the token (parts of rows, counted inside the totals) | Weeks [E] |
|---|---|
| R15: safe-region entry and exit; the holder collects; leaving a region waits for the token | 1.5–2.5 |
| R26: carrier pool, park and unpark, handoff, wake-ups from completed calls, compensating carriers | 2–3 |
| R7 and R9: locks on port buffers, the `PortTable` and the per-heap tables; stdin lookahead process-wide | 1–2 |
| R10 and R11: `GreenThread`, `Mutator` and machine state `Send`; one driver loop per carrier | 1.5–3 |
| R24 and R28: `exit` from a non-main carrier; `InterruptHandle` posts to the holder | 0.5–1 |
| R33: carriers spawned at the main thread's stack size | 0.5 |
| R30: token interleavings in the simulation lane; TSan on the Rust-side locks; stress lanes | 2–3 |
| R31: handoff latency, measured | 0.5 |
| **Total** | **9.5–15.5 (19–62 with the factor)** |

The token's prerequisites are stages 0–9, row S (which includes the helper pool) and C8.

**Path after the trigger.** Each step ships alone:
1. **The token** (optional).
2. **The threaded build with one carrier**, with its gates.
3. **The token released for VM heaps**, after the simulation lane covers N carriers, with TSan and arm64 lanes.
4. **Scaling:** carriers as GC workers, parallel minors, contention fixes.

**Re-estimate** after stage 9, scaling the remaining rows by the redesign's own overrun (K14).

**Open until then:**
- **One build or two.** One build only if the threaded build's tax is within the plan's 1% rule (cycles, with
  confidence intervals) and its footprint within SS2's tolerance. Otherwise keep a non-threaded build, as Chez did
  until 10.0. A higher threshold needs its own owner-decision row.
- **Terminating a thread blocked in a system call.** May `thread-terminate!` of such a thread wait for the call to
  return?

**Stop rules:**
- A first-step gate still above 5% after re-placing fences and loads: keep two builds, and do not release the token
  for parallel mutation.
- A step overrunning its re-estimate twofold: stop at the last step shipped.

---

## Notes for the editor (not for the PRD)

### Reconciliations

**Effort after stage 9.** Decision 7's 4–9 months (19–36 weeks, ST's items left after stage 9) becomes 38–77 weeks.
The rows split three ways:

| Part | Rows | Weeks |
|---|---|---|
| Comparable to ST's items | — | 18–34.5, matching ST's 19–36 |
| Not priced by ST | R5, R6, R9, R17, R25–R29 and R32–R34 | 13.5–28.5 |
| Counted by ST as delivered by stages 1–9, but this study finds work left | R4 (0.5–1), R11's split (2–4), R16's windows (1.5–4), R18 (1.5–3), R21 (1–2) | 6.5–14 |

**Effort from today.** ST's 9–18 months, quoted in decision 7, was 35–63 weeks raw, rounded up for risk. This study's
79.5–145.5 weeks (18–34 months) supersedes it and is about 2.3× ST's raw sum. The increase splits roughly in half:

| Part | Weeks | Detail |
|---|---|---|
| ST's own items, re-priced | +20–35 (55–98 against 35–63) | `Rc`/`RefCell` work split by structure, 13–23 against 8–14; call-site migration with the `VmState` split, 8–13 against 4–6; testing and tuning, 12–22 against 8–16; §12's obligations, which ST counted only after stage 9, 3–6 |
| Rows ST did not itemize | +24.5–47.5 | scheduler, parallel GC, continuations, libraries, finalization, debugger, embedding, pacing, native stacks, promises |

The factor is applied explicitly here, so no risk rounding is added.

**Tax.**
- ST estimated 5–15% for the VM without measuring it.
- Measured: +1.9% for the ordering obligations (11 workloads), and at least +4.1% for brute force on today's
  representation. That +4.1% is a lower bound and does not show ST's figure was pessimistic.
- Estimate after the redesign: ≈2–5%, up to ~7%.

**Fences.**
- `itemize.md` §0, §3 and §5 ("double digits on arm64") and `measure.md` §0, §2.5 and §6.4 ("15–54 ns after
  streaming stores to memory not recently written"; "10–100× on a tight `cons` loop") are superseded.
  - The raw sweep contradicts the stated cause: a 64 KiB window, which fits L1 and is rewritten every 4,096
    iterations, cost 46.7 ns in one run and 16.0 in the other.
  - `review-pubstore` shows the tail intermittent across process launches. It appears with both `stlr` and
    `dmb ishst; str` when fresh objects are stored into old holders.
  - Chez's `set-car!` loop, which has that shape, ran at 1.84 ns per iteration.
- Store-side placement is the candidate to beat on fence count alone, because initializing stores are not fenced:
  heap-valued stores run about 5 per 1,000 instructions at the median, against 36–258 allocations. Its soundness
  conditions are rule 4 (b)–(d). S6 decides.

**Two Chez measurement sets,** both taken on this machine over different benchmark sets:
- `measure.md`: +0.8% geomean over 14 benchmarks, +3.5% over 15 with `intern`; `setcar` −1.4%, `vecset` +4.1%.
- `prior-art.md`: +1.5% over 8 kernels; `store-old` +3.5%, `setcar-old` +3.9%.

**`itemize.md` §8 ordering.** §8 places the token after the threaded build. It does not need it (see the decision
record).

**The tree-walker's 15–40% tax.** This is ST's estimate and was not measured. It does not matter, because the
tree-walker stays M:1.

### Design text to change when merging into `PRD/GC_PRD.md`

- **§12, line 1325.** Replace "with the funnel as the one place a threaded build adds its publication fence besides
  the JIT's allocation groups" with "with the funnel as the one Rust place a threaded build enforces publication
  (rule 4; mechanism chosen by S6)".
- **§12, lines 1331–1332.** Replace "acquire loads for heap references, or a documented dependency-ordering exception
  for JIT code, beside the funnel's release fence" with "acquire loads for heap references in Rust code; JIT code may
  instead rely on address dependencies under a documented exception (no value speculation or equality substitution);
  publication per rule 4".
- **§5** needs no change: it mentions no fence.
- **§11 `GcAttrs`:** add the `publication` field before the stage-6 freeze.
- **§8.1, §12 (`thread-start!` reserves its stack; the finalizer unmaps it) and decision 15.** These must agree with
  the steady-state section's "green-thread stacks carved from one per-heap arena". That row rests on the same
  unverified premise as C14: Linux's `vm.max_map_count`, with one mapping per stack. Linux merges adjacent anonymous
  mappings with identical flags, so whether separate reservations cost one mapping each depends on how stacks are
  committed and guarded.
  - Before choosing, run `blocked-threads` (100,000 threads), or 100,000 separate 256 MiB `MAP_NORESERVE` mappings,
    on Linux CI.
  - If it fails, use chunked reservations (k stacks per mapping, added lazily).
  - Either way, add the per-heap stack address-space bound: 100,000 threads × 256 MiB is 25 TiB.
- **H.4** is unchanged; C7 only makes the worker loop callable from any thread.
- **Decision 7's row** is replaced by Part 2's decision record.

### Problems to file as GitHub issues

None has been filed. Search the open issues and `PRD/ARCHIVE` first. Defects get the symptom and a minimal repro only.

1. **Defect: deep nesting aborts the process.**
   - A program of 1,000 nested `(let ((a 1)) …)` forms ends with "thread 'main' has overflowed its stack / fatal
     runtime error: stack overflow, aborting" (exit 134) on both backends with the default 8 MiB stack.
   - About 980 levels pass; with `ulimit -s 2048`, about 240.
   - chibi and Gauche run 1,000.
   - Repro: `PRD/study/gc/probes/followup/review-par/gen.py 1000`.
2. **Interpreter cost: three `Rc<CodeObject>` clone/drop pairs per VM call.** They cost about 3 ns of a ~38 ns call in a
   microbenchmark. They are at `vm_state.rs:575-579`, `execution_state.rs:55-82` and `execution_state.rs:134-142`.
3. **Interpreter cost: `symbol_table` uses SipHash** (`heap/mod.rs:319`). A lookup takes 17.0 ns, against 7.4 ns with
   `FxHash` (microbenchmark).
4. **Current ports are `thread_local!`** (`primitives/io/ports.rs:41-44`), so two interpreters on one OS thread share
   them. This was read in the source and not reproduced; write a repro before filing.
5. **ST checklist (design, not defects).** One work item per line, each with one line in the PRD:
   - the memory-model statement;
   - the concurrency policy (rule 9, including promises and the source map);
   - the tax and footprint gates;
   - S6's publication checklist (placements, shapes, conformance tests, the `GcAttrs.publication` reservation, C1's
     coupling);
   - the stack-reservation probe on Linux and the stack address-space bound.
6. **ST checklist: bundled libraries with library-wide mutable state** (for R25):
   - `srfi/128/128.body2.scm:42-48`: the comparator registry, two variables;
   - `r6rs/hashtables.atop69.scm:147,214`: `immutable-hashtables`, updated by every immutable `hashtable-copy`;
   - `srfi/27.scm:39-56,555`: the default random source's state vector, six `vector-set!`s per draw.

   Benign duplicates, which are not to be fixed:
   - `srfi/27.scm:290,384`: a lazy initialization;
   - `r6rs/hashtables.atop69.scm:40,56`: a cache.

   No lost update has been shown under M:1 on the VM, and the tree-walker's switch granularity is a stage-9
   question.

### Sources

- `followup/parallelism/itemize.md`: §0, §2, §4.1 (rows R1–R31, S and J), §5, §6, §7 (C1–C17), §8, §9 and §10.
- `followup/parallelism/measure.md`: §0, §2, §3 (the perturbed builds), §4 (Chez built both ways), §6 and §7.
- `followup/parallelism/prior-art.md`: §0, §1, §2, §3, §4, §5 (calibration) and §6.
- `followup/steady/SECTION.md`: the definition, the memory-contract table and the limits table.
- `design/DESIGN.md`: §5, §7, §8.1, §11, §12, §15, §16, decisions 6, 7, 14 and 15, E.7 and H.4.
- `research/threads-prior-art.md`, `research/threads-patina-cost.md` and `research/threads-recommendation.md`.
- Python 3.14 free-threading HOWTO, "Memory Usage and Performance Overhead" (six causes):
  https://docs.python.org/3.14/howto/free-threading-python.html
- Chez Scheme at `~/Project/reference/ChezScheme`, `7d82bd86`: `s/arm64.ss` (`%ap` is `%r21`) and `s/cmacros.ss`
  (`default-collect-trip-bytes`, 8 MiB).
- The repository at `28a94f8`:
  - `.github/workflows/ci.yml`;
  - `patina-vm/src/runtime/control.rs:707,2017` (`force`);
  - `patina-interpreter/src/lib.rs:169,490` (`SourceMap`);
  - `lib/` as cited in issue 6.
- Measurement programs: `PRD/study/gc/probes/followup/`. Raw results, binaries and the perturbed copy were not retained.
  - From the studies: `par-measure/`, `patina-perturb/`, `perturb-wl/`, `chez/`, `prior-art-chez/` and
    `results/barrier_sweep_run{1,2}.txt`.
  - From the reviews: `review-pubstore/results*.txt` and `review-par/` (the stack-depth probes).
  - From this revision: `rev-answer/` (`totals.py` for the row sums, `scm_toplevel.py` for the library re-scan, and
    the nesting probes).

---

## Review dispositions

Review A, the calibration and modelling review:

- **A1 (major), planning figure:** **fixed.**
  - The figure is now derived in the text: base × 2–4. The original 1.3–5.3 years becomes 1.5–6 after A8, A11 and
    A13 add rows.
  - The text says the factor rests on one quantified, message-passing retrofit.
  - Recovery has one treatment. R31 stays as first-pass tuning, and a separate recovery phase (a quarter to half of
    the build) is priced and included in Part 1's and Part 2's totals.
- **A2 (major), isolates uncalibrated:** **fixed.**
  - Isolates now cost 6–20 weeks, or 12–20 at Racket's 4×, which is the closest analogue.
  - `Interpreter: Send` is avoided by building each interpreter on its own thread.
  - For hosts that move interpreters, an audited `unsafe impl Send` wrapper (1–2 weeks) is chosen over `Rc`
    conversion.
- **A3 (major), the 15–54 ns trap:** **fixed.**
  - Verified: the 64 KiB window gave 46.74 ns against 15.95 ns; Chez's `ap` is in `%r21`, its collect trip is 8 MiB,
    and its `setcar` loop ran at 1.84 ns.
  - The trap is relabelled an unexplained, intermittent microbenchmark anomaly that in-situ code did not reproduce.
  - Placement is argued by fence count, and the decision is left to S6 (see B2).
  - Chez's fenced-store figures are now −1.4% to +4.1%, and Chez's overall figure +0.8–3.5%.
  - S6 gets Chez's loop, and the "10–100×" claim is dropped.
- **A4 (major), interpreter tax:** **fixed.**
  - The estimate is now ≈2–5%, up to ~7%, with the unconverted load sites named.
  - The workload count is 11; computed, the geomean is +1.90%, or +2.04% with namedlet.
  - The stage-5 re-measurement is added as the precondition for the PRD's number.
- **A5 (major), memory:** **fixed.**
  - The CPython attribution is corrected against the 3.14 HOWTO: six causes, two of them reference counting.
  - Patina's analogues are listed, and the full per-carrier table replaces "at most 32 KiB".
  - A footprint gate is added to the first step, the stop rules and "one build or two".
- **A6 (major), rule 4 and R13:** **fixed.** Rust code must use Acquire, and only JIT code may rely on address
  dependencies. `itemize.md` R13 is corrected to match.
- **A7 (major), the token:** **fixed.**
  - The token is itemized at 9.5–15.5 weeks, with its prerequisites listed.
  - The race claim is corrected.
  - The token also turns out not to need the `threaded` heap accessors, so the path now allows it first.
- **A8 (major), native stack depth:** **fixed.**
  - Verified: 978 levels pass and 990 abort at 8 MiB; 242 pass and 253 abort at 2 MiB. Both backends abort at 1,000,
    which chibi and Gauche run.
  - R33 and a limits row are added, and the defect is listed as issue 1.
- **A9 (minor), reconciliations:** **fixed.** The increase is split into comparable, unpriced and "delivered" rows,
  and the 9–18 months from today is superseded with the 2.3× explained.
- **A10 (minor), gross against net:** **fixed.**
  - The net saving (1–10 weeks) and the up-front cost (5–8 weeks, 4–9% of the redesign) are shown.
  - The stage-5-exit timing of the threaded build is replaced by a perturbation run that needs no runnable threaded
    build. Timing that build moves to the first step, where R12 prices making it run.
- **A11 (minor), promises and the source map:** **fixed.** R34 (0.5–1.5 weeks) is added. Verified: `force` is a VM
  stub frame (`control.rs:707,2017`), and the source map is an `Rc<RefCell<SourceMap>>` (`lib.rs:169,490`).
- **A12 (minor), the +4.1% figure:** **fixed.** It is labelled a lower bound, and the "ST was pessimistic" claim is
  removed.
- **A13 (minor), scheduler and pacing analogues:** **fixed.** R26 is widened to 3–8 weeks (4–10 from today), pacing
  gets its own row (R32, 1–2), and the histories are labelled elapsed time.
- **A, overall:** the primitive tally is 306 (282 + 19 + 5, recounted), not 304. **Fixed** in `itemize.md` §2.

Review B, the normative-text and staging review:

- **B1 (major), rule 4 tied to the barrier:** **fixed.**
  - Publication is decoupled from the generational barrier through `GcAttrs.publication`.
  - Elision applies only to holders that have not escaped.
  - Conformance tests are on S6's checklist.
- **B2 (major), rule settled before its spike:** **fixed.**
  - Rule 4 is now an invariant, and S6 chooses the placement and instruction from at least 10 launches. The bimodal
    tail is verified in `review-pubstore`.
  - The Chez citation is corrected: Chez emits `dmb ishst; str`.
  - Replacement text for §12's two sentences is given, and §5 is dropped (verified: no fence in §5).
- **B3 (major), C1 with release stores:** **fixed, with one change.**
  - C1 is defined as the standalone-fence variant, and the bulk-range fence is a named exception.
  - The change: the seeded simulation lane runs on one OS thread and cannot observe reordering. Fences are therefore
    covered by loom (decision 14) and the arm64 lanes, and the simulation lane covers protocols.
- **B4 (major), "may use acquire":** **fixed** (with A6), adding the ban on value speculation and equality
  substitution.
- **B5 (major), C11 at stage 3:** **fixed as proposed.**
- **B6 (minor), C14:** **partly fixed.**
  - The Linux probe was not run here: this session has no Linux host.
  - C14 leaves the additions and is made conditional on the probe.
  - The stack address-space bound is added to rule 7 and the limits rows, and the §8.1, §12 and decision-15
    amendments are listed.
  - The steady-state section adopts the arena on the same unverified premise; that is flagged for its editor.
- **B7 (minor), C16 scan:** **fixed.**
  - Verified: 10 of 16 hits were internal defines or load-time `set!`s.
  - A column-0 re-scan with container mutators (`rev-answer/scm_toplevel.py`), plus reading the files, gives three
    shared structures and two benign duplicates.
  - The M:1 claim is dropped, and the tree-walker's granularity is deferred to stage 9.
- **B8 (minor), rule 9 qualifier:** **fixed.**
- **B9 (minor), the 3% threshold:** **fixed.** One build only within the 1% rule and SS2's footprint tolerance;
  otherwise an owner-decision row.
- **B10 (minor), C7:** **fixed.** C7 is restricted to stage P, and minors move to the scaling step.
- **B11 (minor), C13:** **fixed by moving it** to R28 or to isolates, with a deferred-release queue costed at
  2–3 days.
