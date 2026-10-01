# The GC contract, and what concurrency and a Cranelift JIT do to it

**Question (owner).** What is the contract between the GC subsystem and the rest of the runtime? How will future
features, such as some kind of concurrency or a Cranelift-based JIT, affect the current design?

**Sources.** `PRD/GC_PRD.md` at branch `gc-prd`, HEAD `f82e8e8`. That commit adds only `PRD/`, so every Rust line below
is also `main` `28a94f8`. The PRD is the authority. Where `PRD/study/gc/design/DESIGN.md` differs from it, the PRD holds
(§22:2022).

**Citations.**
- **§n:L** is section n, line L of `PRD/GC_PRD.md`.
- Source paths are crate-relative: `core:` is `crates/patina-core/src/`, `vm:` `crates/patina-vm/src/`, `tw:`
  `crates/patina-tree-walker/src/`, `prim:` `crates/patina-primitives/src/`, `rt:` `crates/patina-runtime/src/`,
  `interp:` `crates/patina-interpreter/src/`.
- `gc.rs` is `core:heap/gc.rs`, and `mod.rs` is `core:heap/mod.rs`.

**Labels.**
- **[I]** marks this note's own inference.
- **[E]** marks an engineering estimate taken from the PRD or the study.
- **[A]** marks an analogy to another runtime.
- An unlabelled statement is PRD text or source.

**Clause ids** come from the working notes in this directory:
- `today.md`: **P1–P8** are what today's collector promises; **O1–O17** are what everyone else must do.
- `prd-contract.md`:
  - **C1–C19** are the PRD collector's promises.
  - **V** obligations fall on the VM interpreter, **J** on JIT code, **R** on Rust code, **T** on the tree-walker and
    **E** on embedders.
  - **K-1–K-10** fall on anyone adding an object kind.
  - **N1–N16** are the N-mutator rules.
- The PRD numbers its own §18.6 threading additions C1–C17 as well. This note writes them as "addition C*n*", as §19
  does. **K*n*** without a hyphen is a kill criterion (§20). **A1–A14** are the gaps proposed in §4.

**Status.** Decisions 1, 2, 6 (priority), 7 (M:1 threading model), 12, 14, 15 (scope) and 22 are decided (§1:97-98).
Everything else is a proposed default, including:
- decision 8 (JIT frame model);
- decision 9 (async interrupts);
- decision 13 (embedding);
- decision 20 (JIT code memory);
- decision 24 (pluggability);
- shared-memory parallelism "not now" (decision 7, second row).

The JIT-facing part of the contract freezes only after the stage-6 spike (§1:166-170).

---

## 0. The answer in brief

- **Today the contract is mostly implicit.** The collector:
  - never moves an object, and collects only at an outermost safe point when a flag is up;
  - takes a closed list of root providers;
  - frees with Rust `Drop` inside the pause.

  Everything else in the runtime keeps its side through conventions:
  - `GcDeferGuard` around any Rust scope that holds values across a re-entry;
  - complete root providers;
  - one-dispatch atomicity for continuation side tables;
  - "do not hold a value across an eval" for hosts.

  Roughly seven of these clauses have nothing checking them (`today.md` §5).
- **The PRD makes the contract explicit, and mostly typed.** Its centre is one sentence. Collection happens only at a
  poll, only from code holding the driver's `&mut Heap`, and only with every Scheme value in VM-managed memory, so "a
  `Value<'gc>` cannot be alive while a collection runs" (§9:528-532; §11.3:887-894). The other clauses follow from it:
  - allocation never collects (§8);
  - one store funnel and one barrier definition shared by every tier (§10; §14);
  - precise frames with maps at every suspension point (§11.1);
  - one poll word (§12);
  - continuations as immutable heap objects (§13);
  - a pluggable `Collector<M>` with a fixed epilogue (§14);
  - a steady-state and limits contract (§17).
- **Concurrency.** The PRD sorts concurrency by who may touch heap words while a mutator runs:
  - **Additive, with the hooks already reserved:** green threads (decided), isolates, a shared mutator token and
    parallel stop-the-world marking.
  - **Shared-memory M:N mutators:** these change how heap words are accessed and published and add a handshake. They
    keep every collector promise, and cost 38–77 engineer-weeks after the redesign [E].
  - **Incremental marking:** changes the meaning of the barrier.
  - **Concurrent marking and concurrent relocation:** break the centre sentence [I]. The PRD excludes them for
    throughput (decision 6).
- **A Cranelift JIT consumes the contract; it does not reshape it.** Every tier publishes tagged values into VM frames at
  every non-`Leaf` call, and `(code, pc)` is the resume truth. So the JIT needs no stack maps, no unwinder and no
  deoptimization metadata, and evacuation is invisible to emitted code. Generations are *not* invisible: they arm a
  barrier that a body must have been compiled with. That is the largest gap found here (A1).
- **Gaps.** Three are worth a PRD amendment before the stages that touch them, and eleven smaller ones are worth a line
  each (§4):
  - (A1) a heap's policy can flip at run time, while JIT bodies have its barrier kind compiled in;
  - (A2) an embedder's `NoAlloc` declaration is unchecked and can corrupt the heap from safe Rust;
  - (A3) every limit is per heap, with no process budget, although isolates and many heaps per process are first-class.

---

## 1. The contract

### 1.1 One picture (the PRD's target, with today's mechanism noted per layer)

Arrows pointing down are promises the collector makes. Arrows pointing up are obligations on the rest of the runtime.

```
 L0  HOST / EMBEDDER (§11.5)
     eval_* / call -> Owned     with(|cx| ..)     register_primitive(PrimSpec { f, arity, class })
     interrupt_handle()   external-bytes accounter   near-limit hook   Drop = teardown (F5)
     today: eval_* returns a bare TaggedValue; nothing a host holds is rooted; no teardown (O13; #604, #605)
        |  driver entry: may collect                         ^  Owned results; &heap-exhausted,
        v                                                    |  &stack-exhausted, &interrupt
 L1  DRIVERS hold &mut Heap, the ONLY capability that can collect (§11.3:887-898)
     VM outermost loop | tree-walker outermost trampoline | loader points A, B, D | run_forms | eval_* / call
     today: GcController::safe_point, gated by is_outermost from a GcDeferGuard count (gc.rs:381-395, :219-268)
        |                                         |
        | lends Cx<'gc>: read, allocate,          | poll slow path, no_gc_depth == 0 (§12:1036-1037):
        | store; no method can collect            |   collect(kind, &mut MutatorSet, &RootSet, &mut WeakRegistry)
        v                                         v
 L2  MUTATORS (never collect)                    L3  COLLECTOR  patina-gc: Collector<M>, MarkRegion (§14)
     +----------------------------------+
     | Rust: Cx, store funnel, Fresh,   |--alloc--->  alloc_slow / try_alloc / alloc_old : never collect (§8)
     |   RootScope, Owned, NoGcScope    |--store--->  log_slow / log_range : barrier slow path (§10)
     | VM interpreter: tagged frames,   |--post---->  Mutator.pending|event ; reg_limit := 0 (§12)
     |   maps at every suspension point |--hash---->  identity_hash (BFG; never pins) ; pin -> PinToken
     | JIT tiers: the same frames;      |<--data----  GcAttrs + Mutator ABI (x21 / r15), read at compile time
     |   Leaf | Transfer helpers        |<--slots---  RootProvider::trace(pass, &mut dyn SlotVisitor)
     | tree-walker: pinned roots, ids   |<--final---  FinalRegistry drained at the poll, after the pause,
     +----------------------------------+             before Scheme resumes; Rust-only (§9.6, §9.9)
     today: primitives get &SharedHeap and                |  size / trace / copy / verify
     cannot reach a safe point; stores bypass             v
     the heap through Rc handles (O8)              L4  OBJECT MODEL  declare_layouts! -> CoreModel (§6)
                                                       no Drop payloads; invariant W; Rust resources by u32 id
                                                   today: 28-variant enum, 19 with Drop; hand-written trace (O14)
```

**What crosses each arrow, in one line each.**
- **L1→L3.** Only `&mut Heap` reaches `collect`.
- **L2→L3.** Allocation, the barrier slow path and posting never collect; they refill, log or raise a flag (§8:499-503;
  §12:997-1003).
- **L3→L2.** The collector hands over its fast paths as data (`GcAttrs`, the `Mutator` offsets), so nothing on a fast
  path is `dyn` (§14:1096-1104).
- **Roots.** These come back through `RootProvider` and `SlotVisitor` (§14:1120-1133). Weak processing and finalization
  follow the fixed 11-step epilogue (§9.9).

### 1.2 Clauses by party: today against the PRD

Enforcement letters: **T** type or compile-time check, **R** release runtime check, **D** debug assertion, **L** a test
lane, **C** convention only.

| Party | Clause | Today (where; enforced) | PRD (where; stage; enforced) |
|---|---|---|---|
| Collector → all | When collection happens | P2: only at an outermost safe point with the pending flag up (`gc.rs:381-395`): VM per instruction (`vm:runtime/vm_state.rs:1196-1209`), tree-walker per step (`tw:eval/cps_eval/mod.rs:227-230`). T by structure, R by borrow panic | C1: only at a poll (frame entry, back-edge, `Transfer` return, driver) and only through `&mut Heap` (§9:528-532; §12:1014-1029). T, stage 3. The per-instruction poll goes |
| | Allocation | P2: never collects; `note_alloc` raises a flag (`mod.rs:576-584`) | C2 kept "through the JIT era" (B2, §4:288). C3: user-sized `try_alloc`, a major, one retry (§8:523), stage 5e |
| | Trigger | P3: object count, max(65,536, 2 × live slots) (`mod.rs:570-584`) | bytes plus external bytes, `max(8 MiB, 2·L)`, never time (§15:1297-1305), stage 1 |
| | Moving, identity | P1: never moves; the slot index is identity (`gc.rs:199-207`; `mod.rs:2532-2534`). C | C7–C9: nothing moves before stage 8. The LOS, NMS, immortal space, pinned blocks and tree-walker heaps never move. The BFG hash survives moves (§9.4; §9.8). L (move-all) |
| | Weak references | P7: ephemerons and weak continuation ids in one round-based O(n²) fixpoint (`gc.rs:1050-1094`) | C10, C13: a key-indexed linear fixpoint and an 11-step epilogue every collector follows (§9.5; §9.9), stage 5a. L (conformance) |
| | Finalization | P6: Rust `Drop` at sweep, inside the pause | C11: `FinalRegistry`, Rust-only, after the pause and before Scheme resumes; F1–F6 (§9.6), stages 1–5a |
| | Pauses, memory | P8: pause ∝ arena high-water mark; no limits | C14–C17: one budget form with no dead-object term; SS1–SS5, M1–M5, `max_heap` (§9.10; §17) |
| Drivers | Roots | O1: closed arrays, `[state, registry]` (`vm_state.rs:1305-1308`) and four providers (`tw:eval/cps_eval/mod.rs:129-130`); abort if the registry is borrowed. C for completeness | V12, R8: open `RootSet::register` → `RootToken` (§14:1131-1133), stage 2. L (`VERIFY_ROOTS`) |
| | Rust frames holding values across re-entry | O2: `GcDeferGuard` (`gc.rs:219-268`); nested loops never collect (#614). C for new sites | R5: `NoGcScope` beneath a `Cx`, counted, bounded by K16 (§11.3:891-893; §12:1036-1042; §20:1955). From stage 4e, nested VM loops at driver level collect |
| VM interpreter | Frames | O5: per-pc root bitsets; `retire_registers`, then the whole register file traced (`vm:runtime/vm_state/gc_roots.rs:45-76`); `CallFrame.closure` a bare index | V2–V7: a 40 B header, maps at every suspension point, dead slots cleared, a verifier (§11.1:807-833), stage 4d |
| | Continuations | O4: weak side tables under a one-dispatch rule (`gc_roots.rs:8-28`) | V11: one immutable `T_CONT`, filled by `memcpy` (§13:1054-1075), stage 4e |
| | Code liveness | O6: sweep reports dead closures' code ids (`vm_state.rs:536-571`) | C12: a unit is live while one of its descriptors is marked; an `escaped` bit keeps eager release (§9.7), stage 4e |
| | Stores | none: records, parameters and promises are written through `Rc` handles | V9, R6: the funnel, with a barrier generated from `GcAttrs` (§10), stage 3 (funnel), stage 7 (armed) |
| JIT | Tier contract | no JIT; one prose rule for "a compiled driver" (`vm:runtime/control.rs:23-29`) | J1–J16 (§11.2; §13; §14), frozen at stage 6. Section 3 below |
| Rust code | Primitive shape | O8: `fn(&SharedHeap, &[TaggedValue])`, GC-atomic by signature; higher-order handlers re-enter deferred (`prim:registry.rs:10-21`) | R2–R4: `Prim = for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>])`. `Cx` can neither collect nor call; callbacks go through `Step::Call` (§11.3:900-904). T (trybuild), stage 3 |
| | Values across a collection | O15: reachable, confined or dead. C | R8: only roots, `RootScope`, `Owned`, `RootToken`/`PinToken` and resumable state (§11.3:921-925). T |
| | Loading | O11: `ParsedLibrary` defers for its whole life (`rt:library_loader.rs:191-215`) | R11: collects at points A, B and D (§11.3:927-934), stage 2 |
| | Unsafe | — | R10: `#![forbid(unsafe_code)]` in eight crates (§11.3:912-917). T |
| Tree-walker | Roots | O7: `StepResult` plus a `PENDING_ESCAPE` thread-local root | T1–T7: whole-heap and non-moving for good; `pinned` roots; host-payload ids (§11.4), stages 2–4f |
| Embedders | Host API | O13: nothing rooted, no teardown (`interp:lib.rs:278-298`) | E1–E11: `Owned`, `with`, `call`, `register_primitive` with a helper class, `InterruptHandle`, mandatory teardown (§11.5), stages 2–3 |
| Object kinds | Adding one | O14: a hand-placed arm in `trace_object_children` (`gc.rs:695-794`); "a value-bearing variant misfiled as a leaf is a use-after-free, not a compile error" (`mod.rs:137-141`) | K-1–K-4: `declare_layouts!` generates size, trace, copy, verify and the JIT offsets; no `Drop`; invariant W (§6), stage 5 |
| Threads | Isolation | O17: `Rc<RefCell<Heap>>` is `!Send` (`mod.rs:51`); thread-locals are shared across heaps (`prim:primitives/io/ports.rs:41-45`, #618) | N1–N16: a `Mutator`/`GreenThread` split, rules 1–10 (§18.1-§18.2), stages 3 and 9 |

**The net change.** The PRD turns today's unchecked conventions into checks:
- deferral becomes `NoGcScope` and the `'gc` brand, checked by trybuild (R3, R5);
- leaf classification becomes generated traces (K-1);
- root completeness gets a verifier, `VERIFY_ROOTS` and zeal (§16:1334-1339);
- thread-locals get a CI check (R15).

Two things remain trusted:
- **The VM and tree-walker cores**, a "trusted island" resting on the verifier, the poison lanes and zeal
  (§11.3:895-898).
- **An embedder's declared helper class** (A2).

---

## 2. Concurrency

### 2.1 Four classes, by who touches heap words while a mutator runs

| Class | Features | What runs concurrently with Scheme | Contract consequence |
|---|---|---|---|
| 1 | (a) SRFI 18 green threads, M:1; (b) isolates; (c) a shared mutator token; (e) parallel stop-the-world marking | nothing: one mutator per heap at a time, and the collector only while the world is stopped | holds; changes are additive and mostly pre-reserved |
| 2 | (d) shared-memory M:N mutators | other mutators; the collector still stops the world | collector promises hold; heap-word access, publication and the safepoint protocol change |
| 3 | (f) incremental marking | the collector, in slices between mutator steps on the same thread | the barrier's meaning changes; the epilogue, frames and brand hold |
| 4 | (g) concurrent marking; (h) concurrent relocation | the collector, at the same time as the mutator | [I] the centre sentence (C1 with R3) fails for the concurrent phase |

### 2.2 Feature × contract matrix

**S**: the clause stays. **C**: it changes, within what the PRD states or reserves. **B**: it breaks. **—**: not
applicable. Detail and line cites: `concurrency.md` §1.

| Contract area (clauses) | (a) green | (b) isolates | (c) token | (d) M:N | (e) par. mark | (f) incremental | (g) conc. mark | (h) conc. reloc. |
|---|---|---|---|---|---|---|---|---|
| Who collects, when (C1, C2, R1, R5) | S; switch only at `reentry_depth == 0` (§12:1042) | S per heap | C: the holder collects | C: request, acknowledge, collect, release (N5); per-carrier capability (addition C2) | S (inside `collect`) | C: slices and a final pause, each at a poll | B [I] | B [I] |
| `'gc` brand, Rust values (R2–R4, R8) | S | S | C: safe regions only with no unrooted value (R14) | C, per carrier | S | S [I] | B [I]: a value is alive while the trace runs | B [I] |
| Funnel and barrier (R6, V9, C19, J9) | S | S | S | C: `ldclrb` disarm; soft limit split (§10:733, 738-739) | S | **B**: every heap that may mark needs a barrier (A7); SATB drops the value filter (§14:1248, 1282) | B | B |
| Heap words, publication (N2–N4) | S | S | S: the mutex hand-off orders heap words (§18.7:1802-1803) | C: `threaded` build; rule 4 (§18.2:1664) | S | S | B [I]: atomics with one mutator | B [I] |
| Allocation, pacing (C2–C4, §15) | S | S, but per heap only (A3) | S | C: pacing sums carriers (R32) | S | C [I]: allocate-black or top-at-mark-start | C | C |
| Moving, identity (C7–C9) | S | S | S | C: `HASHED` by `fetch_or` | S: sequential destinations (§14:1285) | S | S | **B**: word-0 forwarding races `car`; addresses depend on the schedule [I] |
| Weak, finalization, epilogue (C10–C13) | C: `FinalKind::Thread` (§18.1:1627) | S | C: locks | C: the leader drains | C: sharded fixpoint | C: darken on reinstatement (§13:1074-1075) | B [I] | B [I] |
| Frames, stacks, continuations (V1–V5, V10–V12) | C: a stack and watermark per thread | S | S | C: cross-carrier delivery under lock | S | C [I] | C [I] | B [I]: capture must heal frames |
| Poll, safe regions, `Mutator` ABI (R13, R14, J7, N14) | C: `ticks`, `thread`, `PREEMPT` | S | C: safe regions become real; `InterruptCell` re-pointed (A14) | C: `handshake`, `quiesce_epoch`, `fenced_ap` | S | C: `barrier_mode` | C | B [I]: no load-barrier word |
| Pauses, determinism, limits (C6, C14, C17) | C: Tick keeps lanes byte-identical | C: limits per heap (A3) | C: byte-identical only with one carrier | C: as (c) | C: b ÷ workers; a `workers=4` lane | C [I] | B [I] | B [I] |
| Embedding, `Send` (E4–E11, R15) | S | C: E11's `unsafe impl Send` wrapper | C: machine state `Send` | C: payloads `Send + Sync` (§14:1183) | S | S | C | C |
| JIT tier contract (J1–J16) | C: a switch is a `Transfer`; invalidation in every thread's stack | S | C: `isb` at hand-off (A10) | C: dual mapping, `quiesce_epoch`, rule 4(f) | S | C/B: SATB means recompiling | B | B: a load barrier at every load |
| Tree-walker (T1–T7) | C: a thread is a `StepResult` (§18.1:1629) | S | — (stays M:1) | — | C | — | — | — |

### 2.3 Status, what is pre-reserved, and cost

| Feature | PRD status | Pre-reserved | Effort | Single-thread tax |
|---|---|---|---|---|
| (a) green threads | **Decided** (decision 7, §2:202); stage 9 | `ticks` 0x40 and `thread` 0x48 (§14:1231, 1234); the `PREEMPT`, `TERMINATE` and `SIGNAL` bits (§12:994); `ThreadGcState` and `MutatorSet::threads` (§14:1135-1145); `FinalKind::Thread`; the thread, mutex and condvar layouts (§6:410-413); the two-mutator lane | stage 9: 7–10 wk; the SRFI 18 library: 6–11 wk [I] (§19:1878-1879) | ≈0; Tick at most 1%, or confined to scheduler builds (K7, §20:1946) |
| (b) isolates | Optional (§18.1:1650-1655) | one reservation per heap (§7:443-450); the entry trampoline keeps the caller's pinned register (§11.2:874-877); a per-heap `InterruptHandle` (§12:1005-1012); F1's process-wide `PortTable` registry (§9.6:609); teardown (F5) | 3–5 wk after 5e [E]; 6–20 with the 2–4× factor [A]; the `Send` wrapper 1–2 wk | 0 |
| (c) token | Optional first step after the trigger (§18.7:1801-1807) | `safepoint_state` 0x45; the safe-region API (§12:1044-1048); addition C3's wrapper; remote posts (§12:998) | 9.5–15.5 wk [E], counted inside (d)'s total | locks: 1–3% on I/O loops under `threaded` [E] (§18.4:1729) |
| (d) M:N | **Proposed "not now"** (§2:203; start conditions §18.7:1789-1794) | `fenced_ap`, `quiesce_epoch` and `handshake` (§14:1237-1239); `GcAttrs.publication` (§14:1251); `HeapSlot`/`MetaByte`; the `threaded` feature, linted from stage 3 (§18.1:1645-1647) | 38–77 wk after stages 0–9 and P [E], 29–61.5 with decision 7c; planning figure about 2–9 engineer-years [A, I] (§18.3) | interpreter ≈2–5% (up to ~7%); JIT 0–4% on arm64; 0 in a non-threaded build (§18.4:1733); +1.9% measured on today's interpreter |
| (e) parallel STW marking | Budgeted stage P, on a trigger (§19:1880) | `STATE` 5 = `BUSY` (§7:477); `PATINA_GC_WORKERS`; addition C7 | 4–6 wk [I] | 0 on the mutator |
| (f) incremental marking | Not planned; "on a latency goal" (§2:200; §19:1882) | `barrier_mode` 0x44 (§14:1232); `value_filter_bit: Option` (§14:1248); the darken rule (§13:1074-1075) | not priced; `proposal-bounded` priced 8–12 wk [E] (`design/proposal-bounded.md:923`) | SATB loses the value filter that removes 64% of heap stores (§5:335) [I] |
| (g) concurrent marking | **Excluded**, for throughput (§1:124; §3:273) | nothing beyond (d) and (f) | not priced; the sourced parts sum to 14–24 wk, and the marker thread, termination and deterministic stand-in are unpriced | (f)'s barrier plus the `threaded` tax with one mutator [I] |
| (h) concurrent relocation | **Excluded** (§1:88; §14:1288 load barriers; §14:1293 racing evacuation) | `can_move`, `BUSY`, the BFG hash, colorless frames | not priced | read barriers ≈5.4% on average [A] (§3:273), on top of (g) |

### 2.4 What to take from this

1. **The load-bearing sentence is C1 with R3**: collection only at a poll, through `&mut Heap`, with every value in VM
   memory (§9:528-532; §11.3:893-894).
   - Features (a) to (f) keep it: (d) per carrier after a handshake, (f) once per slice.
   - (g) and (h) cannot [I]. The PRD's stated reason for excluding them is throughput (decision 6, §1:124, §3:273). §14's
     exclusion list (§14:1288-1293) names load and read barriers and racing evacuation, not concurrent marking.
2. **The reservations sit where the PRD decided or budgeted.** (a), (c), (d) and (e) have reserved ABI words, accessors,
   event bits, lanes and a cargo feature. (f) has one byte, one `Option` and one rule. Several representation choices
   make (g) and (h) dearer on purpose, for throughput:
   - headerless pairs with forwarding in word 0 (§5:365-367);
   - low-bit tags folded into the load displacement (§5:318);
   - the value filter (§10:725);
   - no load-barrier field in `GcAttrs`.
3. **Order of dependencies.**
   - Mutator side: (a) → (c) → (d). The token needs stage 9 and SRFI 18. (d)'s first step is the threaded build with one
     carrier (§18.7:1809-1813).
   - Collector side: (f) → (g) → (h). (g) also needs (d)'s heap-word obligations with a single mutator [I].
   - (e) is independent, and (d) reuses it (carriers as workers). (b) is independent of everything.
4. **Determinism.**
   - Lanes stay byte-identical under (a) through Tick, under (e) through the `workers=4` lane, and under (f) if slices are
     paced by bytes [I].
   - (c) and (d) stay byte-identical only with one carrier (§18.7:1815-1817).
   - (g) and (h) need a deterministic stand-in mode [I].
5. **Limits that do not change with threads.**
   - A green thread cannot switch while Rust frames are on the one OS stack (`reentry_depth > 0`, §12:1042), and
     blocking inside a Rust re-entry raises (§18.1:1629). Precise rooting does not lift this.
   - Under the token, `read` must lex into Rust-owned tokens before it allocates (§12:1047-1048).
6. **Today's design instead.**
   - It can host (b), once stdin and exit flushing stop being thread-local (`core:port.rs:157-185`) and teardown exists
     (#604).
   - It can host (a) at 12–19 weeks [E] (§18.3:1703), with full-stack scans and switches only at outermost safe points.
   - It needs most of the redesign before (c), (d) or (e) pay: 79.5–145.5 weeks from today for (d), 41.5–68.5 of them
     work the redesign does anyway (§18.3:1701, 1706).
   - It cannot host (f) without first building a store funnel. It cannot host (g) or (h) at all: `Rc<RefCell<Heap>>`, a
     `GcVisitor` that bumps `Rc` counts, and no barrier.

---

## 3. A Cranelift JIT

**Status.** No JIT PRD exists. Until one does, §11.2, §13, §14 and stage 6 are the GC's whole contract with the JIT
(§1:170-172). Stage 6 is a 3–5-week spike on an unmerged branch beside stage 5. It freezes:
- the `Mutator` ABI;
- the helper classes;
- the frame contract;
- the entry trampoline.

It does not freeze object layouts, which `declare_layouts!` regenerates into the JIT offset table (§1:166-170). K6
(flonums), K7 (call and poll shape) and K11 (fragments against native call/ret) are decided there; K12 (tier 2) later.

### 3.1 One rule carries almost everything

The rule has three parts:
- **J1.** In every tier, at every suspension point (every non-`Leaf` call), the VM register-stack frames are the only
  home of Scheme values, and they hold only tagged values (§11.2:851-857).
- **J4.** Before a `Transfer` the fragment publishes its dirty values, its `pc` and `ap`; afterwards it reloads
  (§11.2:864).
- **J5.** `(code, pc)` is the resume truth; `ret` is only a cache, re-derived from `desc.resume[pc]` (§13:1077-1082).

Between suspension points a tier may do anything, and the collector never sees it. At a suspension point the stack is
exactly what the interpreter would have left. From that:
- the collector needs no Cranelift stack maps, frame walker or unwinder;
- deoptimization is "pick another resume entry";
- OSR is "enter at a suspension point";
- capture stays a `memcpy` (§13:1059-1064);
- moving objects (stage 8) never touches emitted code.

**The price** is that returns go through a trampoline (the fragment model), and that tier 2 must re-tag or box at every
call. K11 (more than 8% on fib/tak/nboyer) and K12 (more than 15% on call-heavy float code) are the hatches
(§20:1950-1951).

### 3.2 By tier and feature

| Tier / feature | The JIT relies on | The JIT must obey | Cranelift | Decided at |
|---|---|---|---|---|
| **Tier 1: baseline fragments** | C2: allocation never collects, so `ap`/`limit` stay in SSA and allocation sites publish nothing (§8:499-503, 508-511). C1: no collection sees a half-built frame (§12:1025-1027). V4: the bytecode compiler's maps at every suspension point, so the JIT emits none (§11.1:830). One 40 B header for every tier (§11.1:807-823) | J1, J4. J2: results go in the first `Tail` argument (`x2`, `rdi`). V3: window initialization. V8: the poll in the prologue after the frame is complete (§12:1020). J6: embed only four kinds of address (§11.2:866-872). J7: the `Mutator` in `x21`/`r15`, never embedded. J9 and K-9: inline sequences, every word written (§8:505-512; §10:720-735). J14: back-edge polls | `CallConv::Tail`, `return_call_indirect`, the pinned register, `set_cold_block`; no user stack maps | stage 6; K7, K11 |
| **Tier 2: inlining** | C2; J5; §9.7: a frame's `code` word keeps its unit alive | materialize inlined frames at each suspension point. Liveness there is bytecode liveness [I], since the same `(code, pc)` must mean the same thing in every tier (§11.2:856-857). Keep loop polls | none extra | K12 |
| **Tier 2: unboxed floats and ints** | self-tagged flonums (§5:340-348, subject to K6); immortal canonical boxes; boxing is a `Leaf` allocation that never collects | re-tag or box before every suspension point; invariant W (§6:375-380) | none | K6 before the freeze; K12 |
| **Tier 2: registers across calls** | J3: values stay in SSA across `Leaf` calls | nothing crosses a `Transfer` or a Scheme call (§14:1291-1292) | user stack maps would spill every reference around *every* non-tail call, including calls that never collect (`research/cranelift-gc.md` §2.1) [I] | K11, K12 |
| **Escape analysis, allocation sinking** | C2; unbarriered initializing stores (§8:501-503; §14:1254) | materialize once where a live slot reaches the object; use the barrier after any later suspension; never sink a user-sized `try_alloc` past a visible effect (C3) [I] | none | — |
| **Inline caches, `WATCHED` globals** | C8: the NMS never moves; C13 step 5 clears weak IC entries (§9.9:670); `CodeStore::escape` (§9.7:628-633) | an IC is one traced word, never patched code (§11.2:871; §14:1289-1290). J10: a store to a `WATCHED` cell takes the `Transfer` path that invalidates first (§10:762-765). Under variant R, `WATCHED` on record and cell, or 2 loads (§11.6:976) | ordinary loads; no `patchable` | 4b, C, 6; gap A11 |
| **Tier-up and OSR** | J5; pc 0 always has a map (§11.1:830) | tier-up is a `Transfer` that writes `desc.entry` (§13:1078). Every loop passes through a frame-entry poll at pc 0 (§12:1014-1016), so OSR at a loop head means "enter the new entry at pc 0" [I] | `return_call_indirect` | 6; gap A10 |
| **Deoptimization and invalidation** | J5; C12 frees bodies by epoch only when no JIT activation is on the native stack (§9.7:635-637) | re-derive `ret` in every affected frame of every green thread's stack (§13:1079-1081); no extra metadata | no debug tags | 6; gaps A6, A12 |
| **Continuations, green-thread switches** | V11: capture is a `memcpy` of value-only frames; J5 | capture, invoke, winds, abort and switches are `Transfer`s; reload `reg_top`/`thread` after one [I] | avoid `stack_switch` (x64-only, one-shot) | 4e, 9 |
| **Debugger hooks** | invalidation; `RootSet::register` | J13: attaching pins the interpreter tier (§11.2:879-883) | no DWARF | 3, 6 |
| **Code memory** | C12; F4; M3 (JIT bytes are external bytes, §15:1298); the cap (§17.3:1487) | W^X only in `install_code`; free per unit by epoch (decision 20, §2:220) | avoid `JITModule` (no `MAP_JIT`, whole-module frees, §3:272) | 6 (interface); first JIT stage |
| **Exceptions** | raise is a `Transfer`; `Mutator.status` (§14:1236) | `Leaf` errors return as a status; nothing unwinds native frames (§13:1082) | `try_call` not needed | — |
| **Multiple carriers** (only if decision 7 flips) | N1; reserved ABI words | rule 4(f): address-dependency loads with no value speculation (§18.2:1664); invalidation through a handshake | none | 2–4 wk, or 6–10 to retrofit (§18.3:1704) |
| **Native call/ret (S2)** | — | outside this contract: a separate design with native-stack abandonment at every `Transfer`, a depth cap, and the watermark checked on the driver's resume path (K11, §20:1950) | an SP-reset stub outside Cranelift | K11 |

### 3.3 What each GC stage changes in emitted code

| Stage | Emitted code |
|---|---|
| 3 | the `Mutator` ABI exists; interpreter polls move to frame entry (§12:1028) |
| 4b, then C | globals take 2 dependent loads under R and 1 under C (§11.6:976) |
| 4d, 4e | the frame header; capture zeroes `ret` (§11.1:807-823; §13:1059-1062) |
| 5 | raw tagged addresses and layouts; offsets regenerate (§5; §6:436-439) |
| 6 | the freeze |
| **7, generations** | **changes emitted code.** `GcAttrs.barrier` becomes `GranuleLog` (4 instructions for heap values, 1 for immediates), and `watermark` becomes `ReturnBarrier` (§10:720-735, 758-760; §14:1245-1250). A body compiled under `BarrierKind::None` is wrong on a generational heap (A1) |
| 8, evacuation | nothing: values are reloaded after every `Transfer`, no derived pointer crosses a suspension point, no movable address is embedded, and IC words are traced (J1, J4, J6) |
| P, parallel marking | nothing (§14:1283-1286) |
| Incremental marking | slow paths only, through `barrier_mode`, except SATB, which "would need `value_filter_bit = None` and a JIT recompile" (§14:1281-1282); the barrier must also exist on that heap (A7) |

### 3.4 Excluded, and what it would cost to want it

The PRD excludes these (§14:1288-1293):
- Cranelift user stack maps, frame walkers and `stack_switch`;
- raw slots in published frames;
- native frames holding values across a suspension point;
- code patching outside `install_code`;
- load barriers;
- collection inside allocation.

Wanting one reopens the contract:
- **Raw unboxed slots** need a per-tier frame layout. Capture or tier-down would then have to convert those frames, and
  a wrong raw bit becomes a missed root rather than retained garbage (§11.1:831) [I].
- **Native stack maps** need a walker and deoptimization at capture. Every `Leaf` call would also become a Cranelift
  safepoint (`research/cranelift-gc.md` §2.1). "Each of those costs months" (`research/threads-patina-cost.md` §2.8).
- **Allocation that may collect** would make 230–890 Rust functions unsound (§8:500). B2 and K16 forbid it (§4:288;
  §20:1955).

### 3.5 Today's code against the same JIT

A JIT on today's VM could only be a template compiler that calls Rust for every heap operation and reloads the register
file's base after every call. The reasons:
- `Rc<RefCell<Heap>>` (`mod.rs:51`);
- relocating `Vec` arenas (`mod.rs:304-316`);
- index-encoded values;
- a 72 B enum with `Rc` payloads;
- a `Vec` register file;
- `Rc<CodeObject>` frames;
- weak continuation tables under the one-dispatch rule (`gc_roots.rs:21-24`);
- code release through sweep reports.

Each obstacle is removed by a stage the plan runs anyway: 2, 3, 4b, 4d, 4e, 5, then C (`jit.md` §6). That is decision 21,
representation first (§2:221). A JIT built ad hoc on today's VM would need 6–10 weeks of retrofit to become
multi-carrier later (§18.3:1704).

---

## 4. Gaps worth an amendment or an issue

All were checked against the PRD text and, where cited, against DESIGN and the study. The review graded A1–A3 major
and the rest minor.

**Where each gap belongs.** These are rules of the design contract, so they belong in §3–§18 ("a stage that needs to
change them changes this text in its PR", §2:234-235), not in issues. Where a gap only needs settling at one stage, the
"Where" column says so. Then it can go in that stage's issue, filed when the stage starts (§22). Search the issues and
`PRD/ARCHIVE/` first.

| # | Severity | Gap | Failure if left | Where |
|---|---|---|---|---|
| A1 | major | Policy can flip at run time, but JIT code has `GcAttrs.barrier` compiled in | after a flip to generational, bodies compiled under `BarrierKind::None` skip logging old→young stores; a minor frees a live young object | §9 and §14 now; before stage 6 freezes `GcAttrs` |
| A2 | major | An embedder's `NoAlloc` declaration is unchecked in release builds | a safe-Rust host primitive that allocates while declared `NoAlloc` leaves JIT code with stale `ap`; two live objects share memory | §11.5 and §16 now; before stage 3 ships `register_primitive` |
| A3 | major | No process-wide memory budget | N isolates or heaps each default to 75%·B; the process is OOM-killed instead of raising `&heap-exhausted`; under `RLIMIT_AS` the first heap takes the address space | §17.3, §18.2 rule 7 and §11.5; by stage 5g |
| A4 | minor | FFI callbacks have no collecting home; foreign finalizers may block | a C event loop calling Scheme never collects (until K16 fires); `sqlite3_close` in a finalizer stalls every green thread | §11.3 or K16; §9.6 (F7); with the stage-2/3 `FFI_DESIGN.md` rewrite |
| A5 | minor | A boot image would sit in the never-scanned immortal space, yet holds mutable cells | a program's `set!` of a library cell stores a mortal value that no collection sees | §7:467 and M1; before the optional boot-image row |
| A6 | minor | Invalidation's `ret` re-derivation overwrites the watermark trampoline | the return barrier is silently lost; a frame below the watermark gains young references the next minor never scans | §13; stage 7 |
| A7 | minor | Incremental marking is said to need only slow-path changes, but whole-heap heaps emit no barrier | an incremental mode on a whole-heap heap misses edges | §14:1281-1282 |
| A8 | minor | `HostPayloadTable::dirty` is an unchecked convention | an FFI or host payload that stores a young value into an old payload misses an old→young edge | §14; stage 5a conformance suite |
| A9 | minor | Tick preemption depends on the tier | JIT-on and JIT-off SRFI 18 runs interleave differently | §12; stage 9 or the first JIT stage |
| A10 | minor | No ordering rule for a `desc.entry` write seen by another carrier (or for `isb`) | an arm64 carrier enters partly visible code | §18.2 rule 4; stage 6's issue |
| A11 | minor | Inline-cache words: no stated location or barrier rule | an IC store into an old descriptor that names a young object is unlogged | §11.2 / §10; stage 6 |
| A12 | minor | Whether a nested driver beneath a `Cx` may enter JIT code is unstated | a body is freed while an outer fragment is still suspended on the native stack | §11.3 / §9.7; stage 6 |
| A13 | minor | Symbol-keyed weak tables over fresh symbols are unclassified in §17 | a steady-state row fails with no stated reason | §17.1 |
| A14 | minor | Under the token, `InterruptCell`'s one `AtomicPtr<Mutator>` must follow the holder | Ctrl-C or a profiler posts to a parked carrier | §12; priced (`followup/parallelism/ANSWER.md:452`, R24/R28) but not in the contract |

### Proposed text

**A1 (major): fix policy once code exists.** The problem:
- Per-heap policy is "a runtime field read only by slow paths: VM heaps start `{false, false}` and measured gates flip
  the bits" (§9:530-532).
- A non-generational heap reports `BarrierKind::None`, and "JIT code emits nothing" (§10:758-760).
- `attrs()` is "per heap; the JIT reads it" (§14:1149), and "the JIT's emitters switch on these values at compile time"
  (§14:1260).
- K1 keeps generational "opt-in for one release" (§20:1940).
- The Rust funnel is safe either way, because it tests `LOG` at run time (§10:774-777). Only emitted code diverges, and
  AOT code (`other.md` §5) could not be recompiled at all.

Add to §9 after line 532:
> **Policy is fixed once code is installed.** A heap's policy bits and its `GcAttrs` are set from `HeapConfig` when the
> heap is created and may change only before its first `install_code`. A measured gate changes the default for new
> heaps, never a running heap's attributes. (If run-time adaptivity is ever wanted, a flip is a posted `POLICY` event
> serviced at a poll: it invalidates every installed body through §13's path, then runs a major that arms every
> pointerful granule.)

Add to §14 after line 1261:
> `install_code` records the `GcAttrs` a body was compiled against and refuses a mismatch; a contract test checks that a
> policy change with installed bodies is refused.

**A2 (major): a host `NoAlloc` must not be trusted.** The problem:
- `class` is "`Leaf`, optionally `NoAlloc`" (§11.5:955).
- "Only `NoAlloc` helpers keep `ap`/`limit` in SSA across a call, which a call-graph test and a debug assertion
  enforce" (§8:510-512). The call-graph test covers `&'static HelperTable`, the runtime's own helpers (§14:1256;
  §16:1347).
- Only debug builds assert that `Mutator.ap` is unchanged (`design/DESIGN.md:1993-1995`).
- A `Prim` takes `&mut Cx`, which can allocate (§11.3:901), and from 5e so can `&Cx` (§11.3:917-918).
- A wrong `Leaf` cannot collect or call Scheme: `Cx` has neither (§11.3:891; §11.5:954). It can only block, which
  stalls. A wrong `NoAlloc`, by contrast, is memory corruption reachable from safe Rust.

Replace the host-primitive row's class sentence in §11.5:955:
> `class` declares `Leaf` or `Transfer` (resumable means `Transfer`). **The JIT treats every registered host primitive
> as allocating:** it writes `ap` back before the call and reloads it after, whatever the declaration. Only the
> runtime's own helpers in `HelperTable`, checked by the call-graph test, may be `NoAlloc`. A host `Leaf` is trusted
> only not to block.

Add to §16's contract tests:
> a host `Leaf` primitive that allocates, called from JIT code, leaves the heap verifier clean.

(An unsafe `NoAlloc` declaration, or a separate context type with no allocation methods, would also work. Reloading
`ap` costs one store and one load around a call [I].)

**A3 (major): a process budget.** The problem:
- B, `max_heap`, the stack defaults and `RLIMIT_AS` fitting are all worked out per heap (§17.3:1471-1475, 1482, 1487).
- Rule 7 says "Limits are per heap" (§18.2:1667), and the memory hooks are per heap (§11.5:956).
- Isolates are the PRD's concurrency alternative (§18.1:1650-1655), many heaps per process are a decision (13, §2:212),
  and 318 heaps live in one test process (§7:449).
- Stage P spawns GC workers per heap (§14:1283-1284).
- A process-wide singleton is excluded (§14:1290), but a shared value passed in is not.

Add a paragraph to §17.3 after line 1475:
> **Process budget.** `HeapConfig` takes an optional embedder-owned `Arc<MemoryBudget>`. Every heap created with the
> same budget draws its `max_heap`, register-stack reservations, JIT code and GC worker count from it. The B-derived
> defaults (75%·B, 25%·B) apply to the budget's total, and `RLIMIT_AS` fitting is against the budget's remaining
> address space. Without one, each interpreter has its own budget, as now. It is not a singleton: an embedder that
> wants one budget per process passes the same `Arc` to each interpreter.

Further changes:
- Amend rule 7 to "Limits are per heap, or per shared budget".
- Add a several-heaps row to the 5g limits lane.
- Add the budget to §18.1's isolate cost.

**A4 (minor): FFI callbacks and blocking finalizers.** The problem:
- K16's list of windows that cannot collect (§20:1955) and §11.3's driver entries (§11.3:888-893) both omit
  synchronous C→Scheme callbacks.
- A C frame beneath a primitive holding a `Cx` cannot get `&mut self` for `Interpreter::call` (§11.5:954), so today the
  callback could only run under `NoGcScope`, without bound.
- §19 plans only "global handles for callbacks" (§19:1892-1893).

Add to §11.3:
> An FFI callback is a driver entry like `Interpreter::call`: the FFI primitive marshals its arguments, holds only
> `Owned` and `PinToken` across the C call, releases its `Cx`, and the callback enters behind a continuation barrier.
> Until that exists, FFI callbacks are a K16 site.

Add an F7 to §9.6:
> A `Foreign` finalizer must not block; one that may block is handed to the blocking-I/O helper pool (stage 9) or runs
> in a safe region.

**A5 (minor): the boot image is immutable only.** The immortal space is "never swept and never scanned: whatever it
references is immortal too" (§7:467; §10:753-754). A standard-library image holds library cells that a program can
`set!` (#406; §11.6:981). In the immortal row of §7:467 and in M1 (§17.2:1438), replace "a future boot image" with:
> a future boot image's immutable objects; its cells, binding records, parameters, promise boxes and tables are
> allocated old in the NMS (`alloc_old`, remember-whole)

**A6 (minor): invalidation keeps the watermark.**
- The collector swaps one frame's `ret` for a trampoline, saving the real target as `(code, pc)` (§11.1:836-838).
- Invalidation "re-derives the `ret` of every frame of the affected descriptors" (§13:1079-1081).
- The verifier's check would catch the clash only under zeal (§11.1:843-844).

Add to §13 after line 1081:
> Re-derivation skips a frame whose `ret` is the watermark trampoline and updates the thread's saved `(code, pc)` target
> instead.

**A7 (minor): incremental marking needs the barrier from creation.** §14:1281-1282 says incremental marking "uses
`barrier_mode` in slow paths". A whole-heap heap has no fast path to reach those slow paths (§10:758-760), and sticky
minors share the `STATE` epoch with majors (§9.2:549). Amend the sentence:
> Incremental marking needs `GcAttrs.barrier = GranuleLog` from creation on every heap that may run it, and a rule for
> sticky minors during an in-progress major, which share the `STATE` epoch; SATB would also need …

**A8 (minor): a store path for host payloads.** `dirty(id)` is the only way a mutated old payload rejoins the next
minor (§14:1191). No funnel slot covers payloads (§10:773-775), the verifier checks heap granules only (§16:1336-1337),
and the conformance suite never tests `dirty` (§14:1211-1215). Add to `HostPayloadTable` in §14:
> `store(id, …)` writes a value into a payload and calls `dirty(id)` when the payload is old.

Extend `VERIFY_ROOTS` to old payloads that reach young referents, and add the case to the conformance suite.

**A9 (minor): Tick counts bytecode events.** Poll sites decrement `ticks` (§12:1031-1034), but inlining removes
frame-entry polls (§12:1020; K12). Only cross-system replay pins the JIT off (§17.4:1551). Add to §12:
> Under `PollKind::Tick`, `ticks` counts bytecode-level frame entries and back-edges in every tier: an inlined callee
> decrements as its frame entry would. Otherwise JIT-on runs are outside the byte-identical scheduler lanes.

**A10 (minor): code publication.** Tier-up writes `desc.entry` as data (§11.2:872; §13:1078). Rule 4 covers publishing
objects, and `quiesce_epoch` covers reuse of code memory (§14:1238; `followup/parallelism/itemize.md:464-470`). Add a
clause (h) to rule 4 (§18.2:1664):
> `desc.entry` is written with release ordering after `install_code` completes; a carrier context-synchronizes (`isb`
> on arm64) before first entering code another carrier installed.

**A11 (minor): where inline caches live.**
- Descriptors are "written only by initializing stores" (§6:427-428), with "no barrier on … descriptors after creation"
  (§10:757-758).
- Yet IC words are traced and updated at run time (§9.7:632-633; §11.2:871; §18.2:1666).

Add to §11.2:
> Inline-cache words live in a per-body table owned by the `CodeStore` and traced as a root of its unit; they hold only
> NMS or immortal references and immediates, so no barrier is needed. Any other referent goes through the funnel.

**A12 (minor): nested drivers stay in the interpreter tier.**
- JIT entry derives its heap pointer from the driver's `&mut Heap` (§11.3:896-898).
- Nested entries beneath a `Cx` hold no such capability (§11.3:891-893).
- Bodies are freed "when the driver sees no JIT activation on the native stack" (§9.7:635-637).

Add to §11.3:
> A nested driver beneath a `Cx` runs only the interpreter tier, as attached hooks do (§11.2:879-883); only the outermost
> driver frees JIT bodies.

**A13 (minor): classify symbol-keyed weak tables.** "An ephemeron keeps a symbol key alive, so symbol-keyed ephemerons
never break" (§9.5:590) is deliberate. §17.1's list of what is outside the contract (§17.1:1430-1432) omits it. Add:
> weak tables keyed by fresh symbols (class D; decision 11, §9.5)

**A14 (minor): the token re-points the interrupt cell.**
- A carrier is a `Mutator` (the Terms table, line 75), and "N carriers share one mutator token" (§18.7:1801).
- `InterruptCell` holds one `AtomicPtr<Mutator>` (§12:1006).

Add to §18.7's token paragraph:
> At each hand-off, under the hand-off mutex, `InterruptCell.mutator` is re-pointed to the new holder's `Mutator`.

### Lower priority: noted, not separately reviewed

- **Rust panics crossing JIT fragments.** The PRD says nothing about them. Helpers should turn a panic into a fatal
  status or an abort, never an unwind through a fragment (`jit.md` §7 gap 4).
- **`NoGcScope` is described two ways.** It is "by type" (§11.3:893), yet runs as the counter `no_gc_depth`
  (§12:1037; §18.1:1614). The PRD does not say which check catches a missed scope (`prd-contract.md` §11 gap 1).
- **syntax-case.** The `ExpansionContext` is named but not specified (decision 17, §2:217). The tree-walker keeps
  `NoGcScope` for transformers for good (T2). §19's update list omits `PRD/macro/SYNTAX_CASE_DESIGN.md`, which still
  uses the deleted `Value` enum (`other.md` §2).
- **One-shot fibers or effect handlers.** These would need a root class for stacks reachable only from the heap; the
  scheduler rule roots every started thread (§18.1:1626; `other.md` §6).
- **Items with no stage.** §19 stages neither `forbid(unsafe_code)` nor the `return_into` choke point
  (`prd-contract.md` §11 gap 3).
- **Cranelift facts.** Four facts rest on the design review, not on the Cranelift study: the pinned register is not
  saved, `Tail` results arrive in `x2`, `JITModule` lacks `MAP_JIT`, and single-bit tests lower to `tbz`. BTI is checked
  nowhere. Stage 6 should confirm all five against the version it pins (`jit.md` §7 gap 9).

---

## 5. Corrections made to the working notes

The review's findings were checked against the PRD and the study, and applied in place. The pre-edit copies of the
notes were not retained.

- **The GcAttrs sentence was not PRD text.** "Truthful per heap, since the JIT bakes it into code" is not in
  `GC_PRD.md`. It is removed from `prd-contract.md`, `jit.md` and `other.md`. Those notes now cite §14:1149, :1260,
  §9:530-531 and §10:758-760, and label the conclusion [I].
- **`prd-contract.md` §10** now says stage 7 changes emitted code, and stage 8 does not.
- **`concurrency.md`:**
  - the DESIGN quote is attributed to DESIGN;
  - the brand argument is labelled [I], with the PRD's throughput reason cited;
  - the two gaps the PRD already answers are dropped (A14 is the real one);
  - the concurrent-marking cost now shows the 14–24 sourced weeks and marks the rest as unpriced;
  - §18.6's additions are written "addition C*n*".
- **`jit.md`:**
  - `readonly` + `can_move` corrected;
  - DRC "replaced as the default", not "dropped";
  - the trampoline's reason is now the pinned register;
  - the watermark under S2 follows K11;
  - the loop claim follows §12:1014-1016;
  - the nested-driver caveat is added.
- **`other.md`:** raw-word precedent (bignum, `CodeDesc`); the Ctrl-C wording; cross-cutting item 3 rewritten.
