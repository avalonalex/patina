# Concurrency and the GC contract: what each kind of concurrency keeps, changes and breaks

Source: `PRD/GC_PRD.md` at branch `gc-prd`, HEAD `f82e8e8` (no Rust source differs from `main` `28a94f8`). Clause ids
come from `prd-contract.md` in this directory (C1–C19 collector promises, V1–V14 VM, J1–J16 JIT, R1–R15 Rust code,
T1–T7 tree-walker, E1–E11 embedders, K-1–K-10 object kinds, N1–N16 the N-mutator rules) and from `today.md` (P1–P8,
O1–O17, the collector as built). The PRD's §18.6 additions, also numbered C1–C17 there, are written "addition C*n*"
here, as §19 writes them; a bare C*n* is always a collector promise. **§n:L** is section n of the PRD at line L. Paths without a crate prefix are under
`crates/`.

**Labels.** **[M]** measured (the PRD's §18 label; **[P]** is the same thing in §1–§17). **[E]** an engineering estimate
taken from the PRD or the study. **[I]** this note's own inference or estimate, not in the PRD. **[A]** an analogy from
another runtime's published figure. **Not priced** means neither the PRD nor the study costs it.

**Features.** (a) SRFI 18 green threads, M:1. (b) Isolates: one interpreter per OS thread, sharing nothing. (c) A
shared mutator token: several OS threads, one runs Scheme at a time. (d) Shared-memory M:N parallel mutators.
(e) Parallel stop-the-world marking (GC threads only). (f) Incremental marking. (g) Concurrent marking. (h) Concurrent
relocation (ZGC/Shenandoah style).

---

## 0. Summary

| | PRD status | Contract effect | Effort | Run-time tax |
|---|---|---|---|---|
| (a) green threads M:1 | **Decided** (decision 7, §2:202); stage 9 | additive; the collector still sees one `Mutator`; nothing breaks | stage 9 7–10 wk [I] + SRFI 18 library 6–11 wk [I] (§19:1878-1879); from today 12–19 wk [E] (§18.3:1703) | ≈0 unused; Tick polling ≤ 1% or it is confined to scheduler builds (K7, §20:1946) |
| (b) isolates | Optional, decision 7's cheaper alternative (§18.1:1650-1655) | per-heap contract unchanged; no `threaded` obligation; **limits are per heap, with no process budget** | 3–5 wk after 5e [E]; 6–20 with the 2–4× factor, 12–20 at Racket's 4× [A]; `Send` wrapper 1–2 wk [E] | 0 |
| (c) mutator token | Optional first step after the trigger (§18.7:1801-1807) | collector sees one running mutator; safe regions (R14) become load-bearing; Rust tables locked | 9.5–15.5 wk [E] (19–62 with factor), inside the 38–77 | no heap-word atomics; table locks 1–3% on I/O loops under `threaded` [E] (§18.4:1729); hand-off latency unmeasured |
| (d) M:N parallel mutators | **Proposed "not now"** (decision 7, §2:203) | collector clauses stay (STW); heap words go atomic, publication becomes an invariant, collection needs a handshake | 38–77 wk after stages 0–9, P (9–18 months) [E]; 29–61.5 with decision 7c; 79.5–145.5 from today [E]; planning figure ≈2–9 engineer-years [A, E] (§18.3:1701-1719) | interpreter ≈2–5%, up to ~7% [E]; +1.9% measured on today's interpreter [M]; JIT 0–4% arm64; 0 in a non-threaded build (§18.4:1733) |
| (e) parallel STW marking | **Budgeted stage P**, triggered by measurement (§19:1880) | internal to `collect`; no mutator-side clause changes | 4–6 wk [I] | 0 on the mutator; gate ≥ 2.5× faster marking on 4 workers, identical output |
| (f) incremental marking | Not planned; optional on a latency goal (decision 6, §2:200; §19:1882); one ABI byte and two rules reserved | barrier semantics change: a barrier on every heap that may mark incrementally; SATB variant loses the value filter and needs a JIT recompile | not priced in the PRD; the `proposal-bounded` candidate priced it 8–12 wk [E] (`design/proposal-bounded.md:923`) | SATB: the 64% of heap stores the value filter removes [P] (§5:335) reach the armed test; `proposal-bounded`'s abandon gate was 3% geomean [E] |
| (g) concurrent marking | **Excluded** by decision 6 (§1:124; §3:273) | [I] breaks the brand's soundness argument for the concurrent phase, determinism, and forces the `threaded` heap-word obligations with one mutator | not priced | (f)'s barrier + the `threaded` tax (≈2–5% interpreter [E]) with one mutator [I] |
| (h) concurrent relocation | **Excluded** (§1:88; §14:1288 "load and read barriers", §14:1293 "racing parallel evacuation") | breaks forwarding in word 0, the value encoding's use of low bits, raw `eq?`, raw bulk reads, determinism of addresses | not priced | read barriers average ~5.4% [A] (§3:273; `research/java-hotspot.md:214`) on top of (g) |

**The sorting question** is who may touch heap words while a mutator can run:

| Class | Features | What touches the heap concurrently | Contract consequence |
|---|---|---|---|
| 1 | (a), (b), (c), (e) | nothing: one mutator thread per heap at a time, and the collector only while it is stopped | the contract holds; changes are additive and mostly pre-reserved |
| 2 | (d) | other mutators; the collector still only stop-the-world | collector promises hold; heap-word access, publication and the safepoint protocol change |
| 3 | (f) | the collector, between mutator steps on the same thread | the barrier's meaning changes; the epilogue, frames and brand hold |
| 4 | (g), (h) | the collector, at the same time as the mutator | the clauses that rest on "the world is stopped whenever the collector reads or writes" break |

---

## 1. Matrix: contract area × feature

**S** the clause stays as written. **C** it changes, within what the PRD states or reserves, or by additive text.
**B** it breaks: an invariant, exclusion or assumption must be withdrawn or rewritten. **—** not applicable (the
feature excludes that backend or area).

| Contract area (clauses) | (a) green M:1 | (b) isolates | (c) token | (d) M:N | (e) par. STW mark | (f) incremental | (g) concurrent mark | (h) concurrent reloc. |
|---|---|---|---|---|---|---|---|---|
| **Who collects, when** (C1, C2, C5, R1, R5, V8, V13) | S: polls shared with `PREEMPT`; switch only at `reentry_depth == 0` (§12:1042) | S, per heap | C: the token holder collects; other carriers sit in safe regions | C: request → every carrier acknowledges at a poll or in a safe region → collect → release (N5); per-carrier capability (addition C2) | S: inside `collect` | C: a cycle is a start, slices and a final pause, each at a poll through `&mut Heap` | **B**: the trace runs without `&mut Heap` | **B**: as (g), and objects move outside pauses |
| **Rust values, the `'gc` brand** (R2–R4, R8, R9, E1–E3) | S | S (moving an interpreter needs E11's wrapper) | C: safe regions only with no unrooted value (R14) | C: same per carrier; `Owned: Send` (addition C13, moved to R28 or isolates) for host threads | S | S: between slices a `Cx` value was reachable at the snapshot or allocated since [I] | **B**: "no `Value<'gc>` alive while a collection runs" (§11.3:893-894) is false during the trace [I] | **B**: every Rust load of a heap reference needs a barrier; `&[HeapSlot<'gc>]` bulk reads cannot stay raw [I] |
| **Store funnel, write barrier** (R6, R7, V9, C19, J9, J10) | S | S | S | C: `ldclrb` disarm, soft limit divided among carriers (§10:733, 738-739) | S | **B** (C19): every heap that may mark incrementally needs a barrier, so `BarrierKind::None` goes; SATB also needs `value_filter_bit = None` and old-value logging (§14:1248, 1282) | **B**: as (f), plus fresh objects published to the marker | **B**: as (g), and a store must write a healed reference |
| **Heap words, publication, memory model** (N2–N4, N11, N12, K-8) | S (plain accessors) | S | S: the mutex hand-off orders every heap word (§18.7:1801-1804) | C: the `threaded` build: relaxed `AtomicU64` slots, `MetaByte` RMW, sub-word atomics, Acquire loads in Rust, publication per `GcAttrs.publication` (§18.2:1664) | S: workers run only while mutators are stopped; CAS on `STATE` during GC only (DESIGN H.4) | S: one thread | **B**: the `threaded` heap-word obligations become mandatory with one mutator [I] | **B**: as (g), plus CAS-installed forwarding [I] |
| **Allocation, pacing** (C2–C4, J9 alloc, §15, N7) | S; stacks charged under `max_heap` (§17.3:1484) | S per heap; **no process-wide budget** | S | C: pacing sums carriers (R32); block-pool mutex; floating garbage (§18.5:1752) | S | C: allocate-black or a top-at-mark-start rule during a cycle; slices paced by bytes [I] | C: (f) plus allocation outrunning the marker [I] | C: (g) plus a relocation reserve [I] |
| **Moving, identity** (C7–C9, J6, §5) | S | S | S | C: `HASHED` by `fetch_or` (already behind `MetaByte`, §7:480); evacuation stays STW | S: destinations stay sequential (§14:1285) | S: evacuation only in the final pause [I] | S: same [I] | **B**: word-0 forwarding (§5:365-367) races with `car` of headerless pairs; low-bit tags leave no colour bits; BFG hash under concurrent copy; addresses depend on scheduling |
| **Weak refs, finalization, epilogue, code liveness** (C10–C13) | C: `FinalKind::Thread`; blocked-forever threads rooted (decision 23) | S; F1's process-wide `PortTable` registry is what makes `exit` correct across isolates (§9.6:609) | C: finalizers by the holder; `PortTable` locked | C: the leader drains before releasing the world; registration from any carrier (addition C3) | C: sharded ephemeron fixpoint; host payloads on the collecting thread (§14:1208) | C: epilogue in the final pause; continuations darkened on reinstatement (§13:1074-1075); SATB needs a read barrier on weak accessors [I] | **B**: as (f), concurrently; the §14:1288 read-barrier exclusion needs an exception [I] | **B**: weak tables rekey forwarded keys concurrently [I] |
| **Frames, stacks, continuations** (V1–V5, V10–V12, J5) | C: a stack per thread (256 MiB, §18.1:1628 stack rule); per-thread watermark and `ran_since_gc`; `return_into` covers scheduler deliveries (§11.1:839-843) | S | S: stacks belong to green threads, not carriers | C: cross-carrier invocation; deliveries under the target thread's lock; stacks scanned as work packets (R18) | S: frame walks stay on the collecting thread | C: register stacks are unbarriered (§10:757), so scan at the snapshot or incrementally behind the watermark [I] | C: as (f), concurrent stack processing [I] | **B**: capture's `memcpy` (V11) must heal frames first [I] |
| **Poll, safe regions, `Mutator` ABI** (R13, R14, J7, N1, N5, N14, E6) | C: `ticks` 0x40, `PREEMPT`/`TERMINATE` bits, `thread` 0x48 per switch, `set_limit` on switch | S: one `InterruptHandle` per isolate | C: `safepoint_state` 0x45 used; safe regions real; remote posts from other carriers (§12:998) | C: `handshake` 0x70, `HANDSHAKE` bit, `quiesce_epoch` 0x68, `fenced_ap` 0x60 if addition C1 is chosen (§14:1237-1239) | S | C: `barrier_mode` 0x44 set during cycles; the ABI has no SATB buffer fields [I] | C: as (f); `HeapShared: Sync` | **B**: a load-barrier mask or phase word the ABI lacks [I] |
| **Pauses, determinism, steady state, limits** (C6, C14, C15, C17) | C: the frames term sums all rooted stacks; Tick keeps lanes byte-identical (§12:1031-1034); thread limits (§17.3:1483-1484) | C: every limit is per heap (§17.3) | C: lanes byte-identical only with one carrier | C: same; footprint terms per carrier (§18.5); first-step gates (§18.7:1796-1799) | C: b divided by workers (§9.10:696); `workers=4` lane byte-identical | C: new slice and final-pause budgets; floating garbage raises footprint; deterministic if paced by bytes [I] | **B**: completion depends on thread timing (C6) [I] | **B**: addresses, and so eq-table order, depend on scheduling [I] |
| **Embedding, `Send`/`Sync`, host payloads** (E4–E11, K-5, N15, R15) | S: `GreenThread` asserted at 9 | C: E11 wrapper; host primitives and payloads `Send`; stdin, exit-status and `exit` semantics | C: `GreenThread`, `Mutator`, machine state `Send` | C: host primitives and payloads `Send + Sync`; host threads attach; a separate `Send + Sync` payload type (§14:1183) | C: `!Send` payloads keep host tracing on one thread | S | C: payloads traced only in pauses | C: same |
| **JIT tier contract** (J1–J16) | C: invalidation re-derives `ret` in every thread's stack (§13:1079-1081); a switch is a `Transfer` (§11.2:864) | S: per-heap code reservation (§7:457); the trampoline keeps the caller's `x21` (§11.2:874-877) | C: each carrier enters through the trampoline; an `isb` at hand-off for code another carrier installed [I] | C: dual-mapped code (J11), `quiesce_epoch`, `WATCHED` deopt by handshake, address-dependency loads (J15); `desc.entry` ordering unstated | S | C or **B**: SATB means a recompile with the filter off (J8 freeze); incremental update needs nothing more on a heap that already emits `GranuleLog` [I] | **B**: (f) plus publication with one mutator | **B**: a load barrier at every heap load in every tier (J16 withdrawn) |
| **Tree-walker** (T1–T7) | C: a thread is a `StepResult` reported through `pinned` (§18.1:1629) | S | — (stays M:1, `!Send`, N8) | — (excluded, N8) | C: parallel heap marking; `pinned` roots on the collecting thread | — (whole-heap STW for good, T1) | — | — (`pinned` roots are never updated, T3) |
| **Collector obligations and assumptions** (§14:1204-1209; `prd-contract.md` §8) | S: one `Mutator` | S | S: one running mutator | C: N entries in `MutatorSet` (conformance already tests two, §14:1214-1215) | C: internal; "never call a `HostPayload` off the collecting thread" bounds it | C: logs persist across slices; sticky minors during a cycle [I] | **B**: "collect runs only at a poll … no `Value<'gc>` alive" and "pacing inputs never time" fail [I] | **B**: `forwarded`, `copy_to`, `SlotVisitor::slot` race with mutators [I] |

---

## 2. Per feature

### (a) SRFI 18 green threads, M:1

**Status.** Decided 2026-10-01 (decision 7, §2:202): M:1, VM first, interfaces written for N carriers. Built at stage
9 (§19:1878); the SRFI 18 library is its own row outside the totals (§19:1879).

**Stays.** Every collector promise C1–C19, because the collector still sees exactly one `Mutator` (N1; §18.1:1613-1619).
Every R, J, K and E clause. C6 determinism: preemption uses `PollKind::Tick`, a deterministic tick quantum at the same
poll sites, and no timer runs under `PATINA_DETERMINISTIC=1` (§12:1031-1034).

**Changes** (all specified in the PRD):
- **N1, the carrier/thread split.** A `GreenThread` owns its register stack, `ThreadGcState` (watermark,
  `ran_since_gc`), frames, re-entry fields, dynamic environment as heap data and scheduler links; a switch saves
  `reg_top` and calls `set_limit` (§18.1:1613-1619). `allocs_since_gc`, `gc_threshold`, `gc_pending` and
  `gc_defer_depth` leave `Heap` at stage 3.
- **V1, C17: stacks.** Each green thread has a fixed, lazily committed 256 MiB reservation, or a slot in a chunked one,
  chosen by stage 9's 100,000-thread Linux probe; a per-heap stack address-space bound (100,000 × 256 MiB would be 25
  TiB) and a threads limit, both raising at `thread-start!` (§18.1:1628; §17.3:1483-1484; M2 row §17.2:1455).
- **V10, V12, C14: roots.** The scheduler is a root provider for every started, non-terminated thread (N13); a major's
  frames term `a·(frames in all rooted stacks)` now sums over threads; a minor scans only frames above the watermark of
  threads that ran (§9.2:549-551; §11.1:835-845). `return_into` covers scheduler deliveries.
- **C11.** Every `GreenThread` is registered `FinalKind::Thread`; termination queues it at once, so its stack returns
  at the next poll without a collection (§18.1:1627).
- **C15.** Threads blocked for ever on unreachable objects stay rooted until they terminate (decision 23, proposed;
  §2:223), and are listed outside the steady-state contract (§17.1:1431).
- **V13 and the Rust re-entry limit.** Preemption is deferred while `reentry_depth > 0` (§12:1042); blocking inside a
  Rust re-entry raises on either backend (§18.1:1629). Precise rooting does not lift this: a green thread cannot be
  suspended while its state is in Rust frames on the one OS stack (`research/threads-recommendation.md` §1.5).
- **R14.** Safe regions exist from stage 9 but are no-ops with one mutator; blocking I/O stalls every green thread until
  a helper pool exists (§12:1044-1048; §18.1:1629).
- **Semantics.** Deep-bound `parameterize` and per-thread current ports are a behaviour change in their own PR with
  `DIVERGENCES.tsv` rows (§1:156-158; §19:1844-1845).
- **T6.** A tree-walker green thread is one suspended `StepResult` in the thread table, reported through `pinned`,
  switching only at the outermost trampoline safepoint (§18.1:1629).
- **J.** A thread switch is a `Transfer` that resumes elsewhere (§11.2:864); invalidation re-derives `ret` in every
  green thread's stack (§13:1079-1081), so its cost grows with thread count [I]. Tick costs 2 instructions at
  back-edges; above 1% over the limit fold, Tick is confined to deterministic-scheduler builds (K7, §20:1946).

**Breaks.** None.

**Already reserved.** `thread` (0x48) and `ticks` (0x40) in the `Mutator`; the `PREEMPT`, `TERMINATE` and `SIGNAL`
event bits (§12:994); `MutatorSet::threads()` and `ThreadGcState` (§14:1135-1145); `FinalKind::Thread`
(§14:1195); the `T_THREAD`, `T_MUTEX`, `T_CONDVAR` and time layouts (§6:410-413); the two-mutator lane, then the
seeded simulation lane (§14:1275; §18.6:1775).

**Cost.** Stage 9: 7–10 weeks [I] (§19:1878). SRFI 18 library on M:1: 6–11 weeks [I], outside the totals
(§19:1879). From today's code instead: 12–19 weeks [E] (§18.3:1703; VM 8–12, tree-walker +2–3, non-blocking I/O
+2–4, `research/threads-patina-cost.md` §2.9).

**Today's design instead** (`research/threads-patina-cost.md` §2):
- Extract a thread context from `VmState` (`ExecutionState` plus `pending_escape`, `reentry` and the other re-entry
  fields and `scratch_args`, `patina-vm/src/runtime/vm_state.rs:54-106,188`), and generalize `gc_pending:
  Rc<Cell<bool>>` (`patina-core/src/heap/mod.rs:395`) into an interrupt word with a preempt bit.
- Split `impl GcRoots for VmState` per thread; `retire_registers` applies per thread
  (`patina-vm/src/runtime/vm_state/gc_roots.rs:47-68`). Every collection scans every thread's whole register file:
  there is no watermark, and the pause already grows with every root set (P8).
- Switch only where the VM may collect today (`is_outermost`, `vm_state.rs:1287-1310`); inside any `GcDeferGuard`
  scope (`patina-core/src/heap/gc.rs:218-269`) neither collection nor a switch can happen.
- Replace shallow parameters (`Parameter { values: Rc<RefCell<Vec<TaggedValue>>> }`, `heap/mod.rs:174-177`) with deep
  binding, and move the `thread_local!` current ports (`patina-primitives/src/primitives/io/ports.rs:41-45`) into the
  dynamic environment.
- Switch by `mem::swap` of the execution state, since `capture_full`/`restore` clone every component
  (`patina-vm/src/runtime/execution_state.rs:239-261`). Cross-thread continuations work because every thread shares
  one `VmState` and its weak continuation tables (`gc_roots.rs:8-24`).
- Thread objects would be new `HeapObjectData` variants, each another arm in the hand-written trace (O14).

### (b) Isolates: one interpreter per OS thread, sharing nothing

**Status.** Decision 7's cheaper alternative, optional (§18.1:1650-1655; E11). DESIGN's verdict: "with one mutator per
heap they avoid every `threaded` obligation" (`design/DESIGN.md:1343-1347`). The PRD's own position is §18.1:1650-1655,
which reverses DESIGN's "need `Interpreter: Send`" (an isolate is built on its own thread); where they differ the PRD
holds (§22:2022).

**Stays.** The whole contract, per heap: C1–C19, V, J, R, T, K, N (one mutator). E9 (many heaps per process: one VA
reservation per heap; the entry trampoline saves and restores the caller's pinned register, §7:443-450;
§11.2:874-877). E6 (no process-wide GC singleton; an `InterruptHandle` per heap, §12:1005-1012).

**Changes.**
- **E11.** Each isolate builds its interpreter on its own thread, so no `Interpreter: Send` is needed; a host that
  moves an interpreter uses an audited `unsafe impl Send` wrapper, sound only if no `Rc` reachable from it is shared, no
  `Owned` stays behind and the host's primitives and payloads are `Send` (§18.1:1651-1653). Addition C13 (`Owned: Send`, a
  deferred-release queue drained at the owner's poll, 2–3 days [E]) moved here (§18.6:1784).
- **R15 and F1.** "No runtime state in `thread_local!`" and F1's process-wide weak registry of `PortTable`s
  (§9.6:609) are what make `exit` from one isolate flush every isolate's files. The registry is touched from several
  OS threads, so it needs a lock [I].
- **Semantics to define:** stdin lookahead, the exit status, `exit` inside an isolate, and copied messages
  (§18.1:1653-1654). Procedures, ports, environments and continuations cannot be copied
  (`research/threads-patina-cost.md` §4.3).
- **Native stack.** Threads a host spawns get 2 MiB, where Rust recursion aborts at about 240 nested `let`s [M]
  (§18.7:1827-1828); the depth guard ([#617]) turns it into a catchable error, and isolate threads need an explicit
  stack size (rule 7, §18.2:1667).
- **JIT.** Code reservations are per heap (§7:457), so Linux's process-wide `mprotect` touches only the installing
  isolate's own pages, and macOS `MAP_JIT` toggling is per thread (§18.1:1644-1645). The dual mapping is not needed
  unless isolates share code [I]. Sharing compiled code needs `CodeBody: Send + Sync` (asserted at 4e, N15) with
  constants per heap.

**Breaks.** None in the GC contract. **Gaps the PRD does not cover** [I]:
- **Limits are per heap.** `max_heap` defaults to min(16 GiB, 75%·B) for each heap (§17.3), so N isolates may together
  commit N × 75% of the machine. The process has no budget; E5's near-limit and memory-pressure hooks
  (§11.5:956) are the only lever a host has.
- **Address-space fitting is per heap.** Under `RLIMIT_AS` "the heap reservation gets what remains" (§17.3), so the
  first isolate can take what later ones need; they then fall back to smaller sizes. Each isolate reserves its heap
  ceiling (16 GiB by default), the main register stack (up to 8 GiB) and a 256 MiB store buffer, about 24 GiB of VA
  [I].
- **GC workers are per heap.** Stage P spawns workers per heap above about 32 MiB live (DESIGN H.4), so N isolates
  times W workers can oversubscribe the cores.

**Already reserved.** E9; F1's process-wide registry; `InterruptHandle: Send + Sync` (N15, stage 3); `CodeBody: Send +
Sync` (stage 4e); teardown (F5, E7), without which every dropped isolate leaks its heap.

**Cost.** 3–5 weeks after 5e [E]; 6–20 with the 2–4× factor, or 12–20 at Racket places' 4× [A] (§18.1:1654-1655).
The `Send` wrapper: 1–2 weeks [E]. A shared compiled-library cache: 4–8 weeks more, optional (`design/DESIGN.md:1928-1929`).

**Today's design instead.** It already works in principle: each `Interpreter` owns its `SharedHeap` and is built on its
own thread (`research/threads-patina-cost.md` §4.1; spawn proxy 11.1 ms and about 12 MB per process [M]). What fails:
- `STDIN_*` and `OUTPUT_FILES` are `thread_local!` (`patina-core/src/port.rs:157-185`), so isolates split stdin
  lookahead and `exit` flushes only the calling thread's files; the exit status is process-wide
  (`patina-runtime/src/exit_status.rs:30,33`).
- A dropped interpreter leaks its heap ([#604]), so isolate churn leaks.
- No heap limit exists, and the trigger counts objects ([#606]), so one isolate can exhaust the machine.

### (c) A shared mutator token

**Status.** The optional first step after decision 7's trigger: N carriers, one runs Scheme at a time, blocking I/O and
FFI in safe regions (§18.7:1801-1807, 1809). Prerequisites: stages 0–9, the SRFI 18 row with its helper pool, and the
simulation lane C8.

**Stays.** The collector promises C1–C19: one mutator runs at a time, and "the holder collects"
(`followup/parallelism/ANSWER.md:448`). N2–N4: "handing the token over is a mutex release and acquire, which orders
every heap word", so there are no atomic heap words and no publication ordering (§18.7:1802-1803). V, K. The `'gc`
brand per carrier.

**Changes.**
- **R14 becomes load-bearing.** A safe region may be entered only with no unrooted heap value on the Rust stack or in
  JIT SSA; blocking primitives are `Transfer`; output is formatted first and input read before anything is allocated,
  so `read` must lex into Rust-owned tokens (§12:1044-1048; §18.1:1643). A `Leaf` never enters one.
- **N9, C3.** Rust-side state reached from safe regions (port buffers, the `PortTable`, the per-heap tables) takes the
  wrapper's locks (§18.7:1803-1804).
- **N15.** `GreenThread`, `Mutator` and machine state become `Send` (`ANSWER.md:451`). Host payloads move with the
  token, so they must be `Send` too [I]. The tree-walker stays M:1 and `!Send` (N8), so the token is for VM heaps only.
- **R13, E6.** Other carriers post through the remote protocol (`event.fetch_or(SeqCst)`, §12:998); `InterruptHandle`
  posts reach the holder; `exit` may come from a non-main carrier (`ANSWER.md:452`).
- **N7.** Carriers are spawned at the main thread's stack size (R33).
- **C6.** Lanes stay byte-identical only with one carrier; token interleavings go to the seeded simulation lane.
- **J.** Each carrier enters JIT code through the entry trampoline, which installs the `Mutator` in `x21`/`r15`. Only
  the holder runs JIT code, so the W^X toggle never overlaps execution and the dual mapping of §18.1:1644-1645 is
  arguably unnecessary under the token [I]. arm64 still needs a context-synchronizing `isb` on the acquiring core before
  it runs code another carrier installed [I] (`quiesce_epoch`'s concern, `itemize.md` row J).

**Breaks.** None. **Gaps** [I]: a carrier is a `Mutator` (Terms, line 75) and "N carriers share one mutator token"
(§18.7:1801), so there are N `Mutator`s. §12's `InterruptCell` holds one `AtomicPtr<Mutator>` (§12:1006), so a post
must be redirected to the token holder at each hand-off; `followup/parallelism/ANSWER.md:452` prices that (R24/R28, 0.5–1 week) but §12's
contract does not state it. A host primitive declared `Leaf` that blocks would hold the token through a blocking
call; the declaration is trusted (`prd-contract.md` §11 gap 2; the sharper form, a wrong `NoAlloc`, is `followup/contract/ANSWER.md` §4, A2).

**Already reserved.** `safepoint_state` (0x45: `Running`/`AtSafepoint`/`InSafeRegion`); the safe-region API (stage
9); addition C3's wrapper (stage 2); the simulation lane (addition C8); the remote-post half of the poll protocol (R13).

**Cost.** 9.5–15.5 weeks [E], 19–62 with the factor, counted inside the 38–77 (§18.7:1805-1806; breakdown in
`ANSWER.md:446-456`: safe regions 1.5–2.5, carrier pool and hand-off 2–3, locks 1–2, `Send` machine state 1.5–3,
`exit` and interrupts 0.5–1, stacks 0.5, tests 2–3, hand-off latency 0.5).

**Today's design instead.** A token over today's heap is a GIL on `Rc<RefCell<Heap>>` (`heap/mod.rs:51`). Moving the
machine between OS threads needs either `Rc` → `Arc` across 588 `Rc<` and 115 `RefCell<` sites [S]
(`research/threads-patina-cost.md` §1), or one audited "the token makes it exclusive" `unsafe impl Send`. The second is
unsound while a blocked carrier still holds any `Rc` into the shared graph, such as an `Rc<Port>` held across a
blocking read [I]. `Rc` values held in `thread_local!`s (`EMPTY_REENTRY`, `EMPTY_HANDLERS`, the current ports,
`PENDING_ESCAPE`) end up in machine state that moves between threads [I]. Deferral also interacts badly: `gc_defer_depth` is
shared on the heap (`heap/mod.rs:417`), so while one carrier blocks inside a deferred scope, no other carrier's loop
is ever outermost and the heap never collects [I].

### (d) Shared-memory M:N parallel mutators

**Status.** Proposed "not now" (decision 7, §2:203), with start conditions (§18.7:1790-1794): a named workload isolates
cannot serve; stages 0–9 and SRFI 18 on M:1 shipped with their matrix rows and the simulation lane green; the stage-5
tax re-measurement within 5% geomean; a re-estimated planning figure. Decisions 7a (one build or two) and 7b
(terminating a thread blocked in a system call) are open (§2:204-205).

**Stays.** Collection stays stop-the-world (C1 with a handshake in front). Allocation never collects (C2). Nothing
moves outside a pause, and evacuation destinations stay sequential (C7, §14:1285). Precise roots and the brand per
carrier (R1–R4). The funnel as the one Rust store path (R6). Frames as the only home of values, and `(code, pc)` as
resume truth (J1, J5). One production collector (decision 24). The epilogue order (C13).

**Changes.**
- **C1, R1, N5.** The capability is per carrier (addition C2, §18.6:1767); a collection is request, acknowledge at a
  poll or in a safe region, collect, release; posters write only atomics (§18.2:1665). The `HANDSHAKE` bit and the
  `handshake` word exist (§12:994; §14:1239). **R5/K16** windows become stall time for every carrier; the
  time-to-safepoint metric (addition C12) measures it (§18.6:1765).
- **N2–N4, N11, N12.** Under `threaded`: `HeapSlot` becomes relaxed `AtomicU64` (the same machine code), `MetaByte`
  RMWs become `fetch_or`/`fetch_and`, `string-set!` and `bytevector-u8-set!` become atomic sub-word stores, Rust loads
  heap references with Acquire, and every store that may make an object reachable by another carrier is ordered
  after its initializing stores through `GcAttrs.publication`, chosen by stage 6's spike (§18.1:1640-1648;
  §18.2:1664). Publication is a safety matter because hole data is never zeroed (§8:507).
- **C2, §15.** Pacing sums every carrier's allocation (R32); the store-buffer soft limit is divided among carriers
  (§10:738-739); the block pool's mutex becomes contended. §18.5 lists the per-carrier footprint terms.
- **C10, C11.** The leader drains finalizers before releasing the world; registration happens from any carrier
  (addition C3); `PortTable` is locked (`itemize.md` R20).
- **V10, V12.** Scheduler deliveries go through `return_into` under the target thread's lock; `thread-terminate!` of a
  thread running on another carrier posts `TERMINATE`; cross-carrier continuations get matrix rows
  (`itemize.md` R21). Root scanning stays on the collecting thread unless stacks become work packets (R18).
- **E, N15.** Host primitives and payloads become `Send + Sync`; host threads attach as carriers; VM heaps need their
  own `Send + Sync` payload trait-object type, because tree-walker payloads stay `!Send` (§14:1183; §18.6:1770).
- **N9.** Rust-implemented state synchronizes internally (ports, tables, registries, promise forcing, the source map,
  R34); Scheme-built structures do not, and users lock them, as in Chez (§18.2:1669). Three bundled libraries keep shared
  mutable state (§18.7:1824-1826).
- **J.** A multi-carrier JIT (§18.3:1704): a dual-mapped `memfd` code reservation on Linux (J11); code reuse only after
  every carrier passes a quiescent point (`quiesce_epoch`); `WATCHED` invalidation deoptimizes fragments on other
  carriers by handshake; JIT loads may rely on address dependencies (J15); allocation groups fence under addition C1's rule if
  stage 6 chooses it.
- **C6.** Lanes stay byte-identical only on the threaded build with one carrier; N carriers are tested as a seeded
  simulation (protocols only), then loom, TSan and arm64 lanes (§18.7:1815-1817).

**Breaks.** No collector promise. What stops holding is the M:1 simplifications: R14's "no-ops under one mutator", and
plain `MetaByte` and `HeapSlot` access. **Gaps** (`prd-contract.md` §11):
- The ordering of a `desc.entry` write seen by another carrier is not stated (gap 4).
- A host primitive declared `Leaf` that blocks would stall every carrier's collection (gap 2).

**Already reserved.** The `Mutator` per carrier (N1); `fenced_ap` 0x60, `quiesce_epoch` 0x68, `handshake` 0x70 and
`safepoint_state` 0x45 (§14:1233-1239); `GcAttrs.publication` (§14:1251); the `threaded` feature with accessor bodies
from stage 3, linted by `--all-features` clippy (§18.1:1645-1647); `MetaByte` and `HeapSlot`; `HeapSlot::compare_exchange`
(5d) and the bulk-range accessors (C4, C5); the Dekker-pair poll protocol (§12:997-1003); per-carrier collect
capability (addition C2); the CI check against new `thread_local!` (additions C9, C11); the memory model (N11, §18.2:1672-1673); the two-mutator
lane and the seeded simulation lane; the stage-P worker loop callable from any thread, so parked carriers can mark (addition C7).

**Cost.** After stages 0–9 and P: 38–77 weeks, 9–18 engineer-months [E]; 29–61.5 with decision 7c's additions, which
cost 5–8 weeks during the redesign [E]. From today's code: 79.5–145.5 weeks, 18–34 months [E], of which 41.5–68.5 is
work the redesign does anyway. Planning figure with the 2–4× overrun factor and recovery: about 2–9 engineer-years
[A, E] (1.4–7 with the additions) (§18.3:1701-1719). Single-thread tax: measured +1.9% geomean on today's interpreter
over 11 workloads [M]; estimated ≈2–5%, up to ~7%, on the interpreter after the redesign, JIT 0–4% on arm64, ≈0 on
x86-64, 0 in a non-threaded build [E] (§18.4:1726-1733). Footprint unmeasured; CPython +15–20%, OCaml 5 +10–20% [A]
(§18.5:1740-1741).

**Today's design instead.** The §18.3 "from today" column, with sources:
- `Vec` arenas relocate on `push` (`heap/mod.rs:304-316`), so a concurrent `car` reads freed memory: R1, 11–18 weeks.
- 14 `Rc` payload kinds and 5 interior-mutable variants in `HeapObjectData`; 787 heap-borrow sites
  (`research/threads-patina-cost.md` §1).
- `GcDeferGuard`'s counter lives on the shared heap (`heap/gc.rs:218-269`). Deferral cannot work with N mutators: a
  carrier in a deferred scope cannot park, so every other carrier waits, and deadlocks if that carrier waits on a lock
  a parked carrier holds (`research/threads-patina-cost.md` §3.2).
- `MarkBits` are plain bitsets (`heap/gc.rs:88-124`); `GcVisitor` dedups `Rc` graphs with `FxHashSet`s
  (`heap/gc.rs:425-440`).
- A closure names its code by an id into one `VmState`'s `code_store`, so the code store must become machine-global.
- 19 `thread_local!` statics (§18.3:1695).
- Measured: locking today's heap costs at least +4.1% at one thread, and one shared count costs 97 ns at 4 threads and
  370–550 ns at 8 [M] (§18.4:1732).

### (e) Parallel stop-the-world marking (GC worker threads only)

**Status.** Budgeted as stage P, triggered by measurement: it starts only if `large-live`'s major at 1 GiB live
exceeds 100 ms after stage 5, as the 120–320 ms estimate predicts [I] (§19:1880; §9.10:707-709). It does not depend
on decision 7 (§18:1580-1582).

**Stays.** Every mutator-facing clause. Workers run only while every mutator is stopped and get raw views of the
reservation, metadata and block table, so "no `threaded` build is needed" (DESIGN H.4,
`design/DESIGN.md:2194-2197`). C6 holds by construction: a non-moving mark finds the same objects in any order, the
per-worker sums commute, and evacuation destinations stay sequential, so addresses and identity hashes never depend on
the schedule; a `workers=4` lane must match one worker byte for byte (§14:1283-1286).

**Changes**, all inside `collect`:
- marking sets `STATE` with a CAS loop on the metadata byte, during GC only; `KEYHINT` is set with `fetch_or` in the same
  byte, so a key marked while another worker registers an ephemeron on it is seen by one of the two;
- work stealing over the mark stack's 4 KiB segments; the ephemeron fixpoint sharded by key;
- roots, frame walks and host payloads stay on the collecting thread, as "never call a `HostPayload` off the collecting
  thread" requires (§14:1208). This caps the parallel part for payload-heavy heaps, such as tree-walker heaps with
  `pinned` roots [I];
- workers are per heap, spawned lazily above about 32 MiB live, parked between collections and joined at teardown
  (E7);
- C14's b ≤ 0.35 ms per live MiB is divided by the worker count (§9.10:696).

**Breaks.** None. The PRD rejects the two variants that would break something: racing evacuation with CAS forwarding
(addresses would depend on the schedule) and Chez's ownership-partitioned collector (§3:274; §14:1293).

**Already reserved.** `STATE` value 5 (`BUSY`) for future parallel evacuation (§7:477); `PATINA_GC_WORKERS` (§14:1276);
addition C7, the worker loop callable from any thread, 2–3 days inside P (§18.6:1776).

**Cost.** 4–6 weeks [I] (§19:1880). Gate: marking ≥ 2.5× faster on 4 workers, output identical to 1. If `large-live`
measures under 100 ms, the stage is dropped and the total falls by 4–6 weeks (§19:1928-1929). Mutator tax: 0.

**Today's design instead.** No priced row covers GC-only parallel marking of today's heap; the nearest is
`itemize.md` R18, 3–6 weeks [E], for atomic marking and replacing the `FxHashSet` dedup over `Rc` graphs.
- `Heap` holds `Rc`, `RefCell` and `Cell` payloads, so `&Heap` is not `Sync`; worker threads could share it only
  through `unsafe`.
- `GcVisitor` clones `Rc`s onto its worklists (`cont_worklist: Vec<Rc<CpsContinuation>>`, `heap/gc.rs:426`), a
  non-atomic refcount write.
- The ephemeron fixpoint is quadratic ([#609], `heap/gc.rs:1050-1094`) and broadcasts weak continuation ids to every
  provider.
- **It would also buy little.** Today's worst pauses are sweep and `Drop`, not marking: 178 ms after a library load,
  about 18 ms of it sweep and the rest releasing 2.9 M `Drop` payloads; queue3's 41 ms, mostly sweep [P] (§1:100-104).
  Sweep runs `Drop` inside the pause (P6) and cannot be parallelized while payloads are `Rc`.

### (f) Incremental marking (one thread, mark slices at polls)

**Status.** Not planned. Decision 6, decided: throughput first, stop-the-world; "a hard latency target would add
incremental marking through `barrier_mode`" (§2:200). It is an optional row "on a latency goal" (§19:1882). What is
reserved: the `barrier_mode` byte, "slow paths only (0 generational; reserved: incremental update, SATB)"
(§14:1232); "SATB would need `value_filter_bit = None` and a JIT recompile" (§14:1281-1282); "a future incremental
mode must darken a continuation on reinstatement" (§13:1074-1075).

**Stays.** C1 in form: each slice and the final pause run at a poll through `&mut Heap`, with every Scheme value in VM
memory. C2: allocation never collects. C5: `(gc)` still runs a full major, which finishes a cycle in progress.
C7–C9: nothing moves except in the final pause [I]. V1–V5, J1, J5. R1–R4 and R8: between slices a `Value<'gc>` in a
`Cx` was either reachable at the snapshot or allocated since, so it survives [I]. T1: tree-walker heaps stay whole-heap
STW "for good" (§11.4:938).

**Changes.**
- **The barrier (R6, V9, J8–J9), and C19 breaks.** Today's plan arms the barrier only in generational mode, and a
  non-generational heap reports `BarrierKind::None`, so JIT code emits nothing (§10:757-760). An incremental cycle needs a
  barrier on every heap that may run one. `GcAttrs.barrier` must be `GranuleLog` from heap creation, because JIT code
  bakes it in [I]. Two variants:
  - **SATB** (deletion barrier): log the old value. Overwriting a pointer with a fixnum deletes an edge, so the value
    filter (`tbz` on bit 2 of the *new* value, §10:725) is unsound: `value_filter_bit = None` (§14:1248) and a JIT
    recompile. The cold block must load the granule's old words and push them; the frozen ABI has no SATB buffer
    pointer, unlike `proposal-bounded`'s `satb_cur`/`satb_soft` (`design/proposal-bounded.md:781`) [I]. Arming every
    old pointerful granule at the snapshot is an O(heap) metadata pass on a whole-heap heap [I].
  - **Incremental update** (insertion barrier): log a heap value stored into an already-marked holder. The value filter
    stays sound, and marking already arms what it marks in generational mode (§9.1:540). The cost moves to the final
    pause, which re-scans logged granules and roots, so the a·(frames) term stays in the pause unless stacks are
    scanned behind the watermark [I].
- **Allocation during a cycle (C2, J9).** Objects born during marking must survive the cycle's sweep. Sticky marking
  treats `STATE` 0 as young-unmarked, so this needs allocate-black (a metadata write on the fast path, which changes
  the JIT allocation sequence) or a per-block top-at-mark-start rule in the lazy sweep [I].
- **Minors during a cycle.** Under sticky marks "old" means `STATE` equals the current epoch (§9.2:549). A major that
  flips the epoch at its start makes every unmarked old object look young to an interleaved minor. The rotating epochs
  could let a minor treat the previous and current epoch as old, a collector-internal change [I].
- **Weak references (C10).** Under SATB a mutator that reads an unmarked ephemeron key (`ephemeron-key`, a weak-table
  lookup) and stores it elsewhere creates an edge that SATB does not log. Weak accessors then need a read barrier that
  marks on read, as SpiderMonkey's does (`research/js-engines.md:184`). That contradicts "load and read barriers"
  in §14:1288, so the exclusion needs a narrow exception [I]. Incremental update catches the store instead.
- **Continuations (V11).** Reinstatement copies a `T_CONT`'s words into the unbarriered register stack, hence the
  reserved darken rule (§13:1074-1075).
- **Stacks (V10).** Register-stack stores are not barriered (§10:757). Stacks are scanned at the snapshot, which keeps
  the frames term in the start pause, or incrementally behind the watermark, through the `return_into` choke point
  (`design/proposal-bounded.md:923`, "incremental stack scanning via `ret_wm`") [I].
- **Pauses and pacing (C6, C14, C15).** A third pause shape (slice, final pause) joins the one budget form of
  decision 6. Floating garbage, objects that die during a cycle, raises footprint above F(L) [I]. Slices paced by
  allocated bytes keep the lanes deterministic, because the trigger inputs stay "bytes and counts, never time"
  (§15:1299-1300) [I].
- **The collector's obligations.** Logs persist across slices instead of being drained or dropped per collection
  (§14:1208) [I].

**Breaks.** C19 (barrier honesty) for heaps that may mark incrementally. J8's freeze: SATB needs a recompile with the
filter off. §14:1288's read-barrier exclusion, under SATB [I]. Decision 6's "one budget form" gains a pause shape.

**Already reserved.** `barrier_mode` (0x44); `value_filter_bit: Option<u8>`; the darken rule; the watermark return
barrier and the `return_into` choke point (V10); field logging, whose cold path is shared by any barrier variant
(§10:715-718); rotating mark epochs (§7:477).

**Cost.** Not priced in the PRD. The `proposal-bounded` candidate priced it as a gated "stage 10" at 8–12 weeks [E],
with gates of p99 major ≤ 3 ms at 50 MB live and ≤ 3% geomean throughput, building only if STW major p99 exceeds 25 ms
and abandoning if throughput costs more than 3% or the matrix and ephemeron tests need more than 2 weeks of fixes
(`design/proposal-bounded.md:923,966`). Need, by the PRD's own estimates: majors of about 5–15 ms at the measured
maximum of 50 MB live, and 120–320 ms at 1 GiB, for which stage P is the answer (§9.10:707-709). Prior art: Racket CS
gave up true incremental marking, and Larceny's regional collector costs about 1.8× elapsed time [A] (§3:273;
`DIGEST.md:892`).

**Today's design instead.**
- **No barrier exists.** Records, parameters and promises are written through cloned `Rc` handles outside any heap API
  (`patina-primitives/src/primitives/records.rs:262`, `parameters.rs:168,267`, `lazy.rs:134`, cited at §10:792-794);
  `vector_slice_mut` exists; environment bindings are `RefCell` writes in Rust structs; the tree-walker writes
  `Rc<Environment>` frames on every `set!`. So the stage-3 funnel comes first.
- **Allocate-black.** Allocation pops LIFO free lists (`heap/mod.rs:705`), so a slot freed last cycle and reused
  during marking has an index inside `MarkBits`, which are sized at the start of each collection (`heap/gc.rs:117-124`)
  and discarded after it; they would have to persist across slices and mark at allocation.
- **The weak continuation tables.** Their soundness rests on "every store touch confined to one instruction dispatch
  and nested loops defer" (`gc_roots.rs:21-24`); captures between slices break that premise [I].
- **`Rc` graphs.** `GcVisitor`'s `seen_envs`/`seen_conts` dedup (`heap/gc.rs:428-433`) assumes a frozen graph.
- **The pauses stay.** Sweep is eager and runs `Drop` inside the pause (P6), so the measured 41 ms and 178 ms pauses
  would remain in the final pause.

### (g) Concurrent marking (a GC thread marks while the mutator runs)

**Status.** Excluded: "throughput first (decided), so stop-the-world with no incremental or concurrent marking"
(§1:124); "No incremental or concurrent marking now; an SATB arm reserved for slow paths" (§3:273). The DESIGN's
`proposal-bounded` candidate excluded it outright (`design/proposal-bounded.md:813`).

**Stays.** Everything (f) keeps, for the start and final pauses. J1: values live only in VM frames, so a concurrent
marker never needs native stack maps. The "colorless stack" lesson of `research/java-hotspot.md:269-270` already
applies.

**Changes beyond (f).**
- **C1, R1, R3: the capability splits.** A start pause (roots, at a poll, `&mut Heap`), a concurrent trace on a GC
  thread holding only a shared view (`HeapShared: Sync`, which N15 defers to decision 7), and a final pause (remark,
  ephemeron fixpoint, the epilogue, at a poll) [I]. During the trace, the soundness of Rust-held values rests on SATB
  plus allocate-black, not on the brand [I].
- **Heap words (N2–N4): the `threaded` obligations arrive with one mutator.** The marker and the mutator touch the same
  metadata byte: the collector writes `STATE`, `END` and `KEYHINT`, the mutator disarms `LOG` and sets `HASHED`
  (§7:477-481), so both sides need atomic RMWs. The marker reads freshly published objects, and hole data is never
  zeroed (§8:507), so an unordered read can trace stale words from a dead object; rule 4's publication applies with
  one mutator, and §18.3:1687 calls it "a memory-safety matter" [I]. Heap words must be relaxed atomics: a racy plain
  access is undefined behaviour in Rust (`research/threads-recommendation.md` §3.1). The §18.4 tax (≈2–5%
  interpreter [E]) then applies to single-mutator programs [I].
- **Host payloads (K-5).** They are `!Send` and traced only on the collecting thread (§14:1183, 1208), so they are
  traced in the pauses [I].
- **Pacing.** Marking must start early enough, and the mutator can outrun the marker; a fallback to a STW finish is
  needed [I].

**Breaks.**
- **C6 determinism.** When concurrent marking finishes depends on thread timing, so which collection breaks an
  ephemeron or queues a port finalizer does too. Byte-identical lanes need a deterministic stand-in, such as running
  the marker synchronously in slices, which is (f) [I]. Stage P's schedule-independence argument covers marking inside
  a pause, not marking beside a mutator.
- **The collector's assumptions** (`prd-contract.md` §8): "collect runs only at a poll, holding `&mut Heap` … no
  `Value<'gc>` is alive on the Rust stack" and "inputs to pacing are bytes and counts, never time" both fail for the
  concurrent phase [I].
- **§14:1288**, as in (f), and **J8/J16**: SATB in emitted code, plus publication in allocation groups with a single
  mutator [I].
- **The M:1 premise.** "Isolates … avoid every `threaded` obligation" (`design/DESIGN.md:1347`) stops being true of a
  heap that runs concurrent marking [I].

**Already reserved.** What (f) has, plus everything (d) reserved for heap words: `HeapSlot`, `MetaByte`,
`GcAttrs.publication` and the `threaded` feature.

**Cost.** Not priced in the PRD. The sourced parts add to 14–24 weeks before any overrun factor: (f)'s 8–12 weeks [E,
`proposal-bounded`, whose incremental stage had an abandon gate and no GC thread]; the heap-word, bulk-operation and
publication work of §18.3:1687, 2–5 weeks after the redesign [E]; loom, TSan and arm64 lanes, which (d) also needs
(R30, 4–7 weeks after the redesign [E]). The concurrent marker thread, SATB-buffer hand-off, a termination protocol, a
STW fallback and the deterministic stand-in are **unpriced**.
Tax: (f)'s barrier plus the threaded tax with one mutator [I].

**Today's design instead.** Not reachable without the redesign:
- `Rc<RefCell<Heap>>` cannot be shared with a GC thread (`heap/mod.rs:51`);
- concurrent reads of growing `Vec` arenas are undefined behaviour (§18.3:1685);
- `GcVisitor` mutates `Rc` refcounts as it traverses (`heap/gc.rs:426-427`);
- there is no barrier.

### (h) Concurrent relocation (ZGC/Shenandoah style)

**Status.** Excluded twice: "load and read barriers" and "racing parallel evacuation" are on §14's exclusion list
(§14:1288, 1293). §1:88 lists ZGC's and Shenandoah's load barriers and concurrent compaction among the things "not
taken (throughput first, decision 6)". The cost reason: read barriers average about 5.4% [A] (§3:273, from Yang et al.,
ISMM 2012, `research/java-hotspot.md:214`).

**Stays.** J1 (roots are tagged VM frames, barrier-free, the ZGC "colorless roots" property). J6's cases 1, 2 and 4:
embedded addresses are only in spaces that never move (immortal, own unit's descriptors, the NMS). C8: the LOS, NMS,
immortal space, pinned blocks and tree-walker heaps never move. Allocation never collects (C2).

**Breaks.**
- **C7, §5:365-367: forwarding.** An evacuated object's word 0 holds the new address. Pairs are headerless, so word 0 is
  the `car`: writing it while a mutator reads `car` is a race. Concurrent copying needs off-object forwarding tables
  per block, as ZGC uses [I].
- **§5's encoding (decision 4).** Heap references carry a 4-bit tag in the low bits, folded into the load
  displacement (§5:318). ZGC's colored pointers use 16 low metadata bits (`research/java-hotspot.md:263`). Colors would
  have to go in high bits, costing the tag-in-displacement addressing unless the hardware ignores the top byte, or every
  load checks a metadata table, as Shenandoah's load-reference barrier does [I].
- **R2, R3, R6: Rust access.** Every heap-reference load through `Cx` heals or barriers; bulk reads "tied to a `&Cx`
  borrow" (§11.3:918) cannot stay raw slices; the funnel must store only healed references [I].
- **`eq?`.** Today and in the PRD, `eq?` is a word compare. It stays correct only under a strict to-space invariant
  (every loaded reference healed); otherwise it needs Shenandoah's former compare barrier [A, I].
- **C9: identity hash.** The BFG extension is installed when a `HASHED` object is copied. Under concurrent copying the
  hash-taking `fetch_or` and the copy race, so a CAS protocol is needed, probably through the reserved `BUSY` state
  [I].
- **C6: determinism.** Destinations depend on scheduling, so hashes taken after a move, and with them eq-table order,
  differ between runs. That is exactly why stage P keeps destinations sequential (DESIGN H.4) [I].
- **V11: capture.** Capture's `memcpy` copies frames that may still hold stale references, so stacks must be processed
  (healed) before they are copied or walked, as JEP 376 does [I].
- **J8, J16.** `GcAttrs` has no load-barrier field. Every tier emits a barrier at every heap load. Code is "never
  patched at GC" (J6), so ZGC's per-phase patched immediates become a mask loaded from the `Mutator`, a word the ABI
  lacks [I].
- **T1, T3.** Tree-walker roots are reported by value through `pinned` and never updated, so tree-walker heaps can
  never take part.

**Already reserved.** Little, by design: `STATE` 5 = `BUSY` (§7:477), `can_move` in `GcAttrs`, the BFG hash (§9.8),
and colorless frames (J1).

**Cost.** Not priced. It presupposes (g) and adds load barriers in three tiers (interpreter, Rust, JIT), a forwarding
scheme for headerless objects and a revisit of decision 4. Run-time tax: ~5.4% average for read barriers [A], on top
of (g)'s.

**Today's design instead.** Impossible. Even STW moving is impossible today: "Implementations must be **non-moving**:
live slots may never be relocated, because raw arena indices escape" (`heap/gc.rs:199-207`). `GcVisitor::visit` takes
values by copy, so no root can be updated (P1). The identity hash is the heap index, "a sound identity because the
collector does not move objects" (`heap/mod.rs:2532-2541`). SRFI 69 stores those hashes in Scheme vectors.

---

## 3. Cross-cutting findings

1. **The contract's load-bearing sentence** is C1 together with R3: collection happens only at a poll, through the
   driver's `&mut Heap`, with every value in VM memory, so no `Value<'gc>` can be alive while it runs (§9:528-532;
   §11.3:887-894). Features (a)–(f) keep it, (d) per carrier after a handshake and (f) once per slice. (g) and (h)
   cannot keep it [I]. The PRD's stated reason for excluding them is throughput (decision 6; §1:124; §3:273); §14's
   exclusion list (§14:1288-1293) names load and read barriers and racing parallel evacuation, not concurrent marking.

2. **What the PRD reserved is concentrated where it decided or budgeted.** (a), (c), (d) and (e) have reserved ABI
   words, accessors, event bits, lanes, a cargo feature and rules (N1–N16). (f) has one byte (`barrier_mode`), one
   `Option` (`value_filter_bit`) and one rule (darken on reinstatement). (g) and (h) have nothing beyond what (d) and
   (f) reserve. Several representation choices make (g) and (h) dearer on purpose, for throughput (decision 6):
   - headerless pairs with forwarding in word 0;
   - low-bit tags folded into the displacement;
   - the value filter on the barrier fast path;
   - no load-barrier field in `GcAttrs`.

3. **Dependency order.** (a) → (c) → (d) on the mutator side: the token needs stage 9 and the SRFI 18 row; (d)'s first
   step is the threaded build with one carrier (§18.7:1809-1813). (e) is independent, and (d) reuses it: carriers as
   workers (addition C7), parallel minors in the scaling step. (f) → (g) → (h) on the collector side. (g) also needs (d)'s
   heap-word obligations, even with one mutator. (b) is independent of all of them.

4. **Determinism.** The plan stays byte-identical for (a) (Tick), (e) (`workers=4` lane) and (f), if slices are paced
   by bytes [I]. It stays byte-identical for (c) and (d) only with one carrier. (g) and (h) need a deterministic
   stand-in mode, and (h) also makes addresses, and so hash order, schedule-dependent [I].

5. **Gaps this analysis found** [I]:
   - **Isolates.**
     - Limits, address-space fitting and stage-P workers are per heap, and no process-wide budget or worker pool
       exists.
     - F1's process-wide `PortTable` registry is touched from several OS threads.
   - **The token.** `InterruptCell`'s single `AtomicPtr<Mutator>` must follow the holder at each hand-off; priced
     (`followup/parallelism/ANSWER.md:452`) but not in §12.
   - **Host payloads.** "Never call a `HostPayload` off the collecting thread" constrains (e) and (g) (under (d), §14:1183
     already requires a `Send + Sync` payload type):
     - under (e), payload tracing stays serial;
     - under (g), payloads are traced only in pauses.
   - **Incremental and SATB marking.**
     - `BarrierKind::None` heaps cannot run it.
     - SATB needs `value_filter_bit = None`, an SATB buffer the ABI lacks, a read barrier on weak accessors that §14
       excludes, and an arming pass.
     - Sticky minors and an in-progress major share the `STATE` field.
   - **Code entry under (d).** Cross-carrier ordering of `desc.entry` is unstated (`prd-contract.md` §11 gap 4).
   - **Trusted host-primitive classes.** A `Leaf` declaration is trusted (gap 2). Under (c) a blocking `Leaf` holds
     the token; under (d) it stalls every carrier's collection.

6. **Today's design versus the PRD's, in one line each.** Today's design can host (b) with fixes to `thread_local!`
   stdin, exit flushing and teardown, and (a) at 12–19 weeks with full-stack scans and switches only at outermost safe
   points. It needs most of the redesign before (c), (d), (e) pay, at 18–34 engineer-months for (d) from today. It
   cannot host (f) without first building a store funnel, or (g) and (h) at all. Its one collector promise that matters
   most here, non-moving P1, is what the redesign gives up at stage 8, and only stop-the-world.

[#604]: https://github.com/avalonalex/patina/issues/604
[#606]: https://github.com/avalonalex/patina/issues/606
[#609]: https://github.com/avalonalex/patina/issues/609
[#617]: https://github.com/avalonalex/patina/issues/617
