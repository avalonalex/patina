# The GC contract in `PRD/GC_PRD.md`, by party and direction

Source: `PRD/GC_PRD.md` at branch `gc-prd`, HEAD `f82e8e8` (the PRD is dated 2026-10-01 against `28a94f8`; no Rust
source changed between the two, so source line numbers are the same). Citation form: **§n:L** is section n of the PRD,
line L of `PRD/GC_PRD.md`. "Today" lines cite the collector as built (`docs/GC_DESIGN.md`, `crates/...`).

**Clause ids.** C1–C19 below are this note's collector promises. The PRD's own §18.6 additions are also numbered
C1–C17; this directory writes those as "addition C*n*", as §19 does. K-1–K-10 are object-kind obligations; K*n* is a
kill criterion (§20).

**Status caveat.** Most of this contract is a *proposed default*, not a decision. Decided on 2026-10-01: decisions
1 (tree-walker kept, may lag), 2 (variant R then C), 6 (throughput first, stop-the-world), 7 (M:1 green threads, N-ready
interfaces), 12 (port finalization), 14 (no MMTk; `libc` only), 15 (limits in scope), 22 (one PRD) (§2:193-233). The
JIT contract (decisions 8, 9, 20), pluggability (24), embedding (13), identity hash (10), weakness (11) and "no
parallelism now" (7, second row) are all **Proposed**. The JIT-facing part freezes only after the stage-6 spike
(§1:161-172).

**Enforcement legend.** **T** type system or compiler (borrow checker, brands, trybuild, lint attributes, build-time
`const` assertions). **D** debug-build assertion. **L** a test lane, CI test or scoreboard (§16). **K** a measured gate
or kill criterion (§20). **R** review or process rule only; nothing mechanical checks it.

**Stage legend** (§19:1857-1882). now, 0, 1, 2, 3, 4a–4g, 5 (5a–5f), 5g, C (variant C), 6 (JIT ABI spike), 7
(generations), 8 (evacuation), 9 (threads readiness), P (parallel marking), JIT (the JIT track's first merged stage,
outside this plan).

---

## 0. One-page summary (diagram-ready)

```
 L0  EMBEDDER / HOST                                                          (§11.5)
     Owned handles · eval_* · call(&Owned,&[Owned]) · with(|cx| ..) · register_primitive(PrimSpec{f,arity,class})
     interrupt_handle() · external-bytes accounter · notify_idle · near-limit / pressure hooks · Drop = teardown
        │ driver entry: may collect                         ▲ Owned results; &heap-exhausted,
        ▼                                                   │ &stack-exhausted, &interrupt
 L1  DRIVERS: hold &mut Heap, the ONLY collect capability                       (§11.3)
     VM outermost loop · TW outermost trampoline · loader points A/B/D · eval_datum · run_forms · eval_*/call
        │ lends Cx<'gc> (cannot collect)                    │ poll slow path, no_gc_depth == 0:
        ▼                                                   ▼ requested(m)? → collect(kind, &mut MutatorSet,
 L2  MUTATORS (never collect)                                                     &RootSet, &mut WeakRegistry)
   ┌──────────────────────────────────────────┐        L3  COLLECTOR  patina-gc (libc only)        (§14)
   │ Rust code : Cx::store funnel · Fresh ·   │─alloc─►  alloc_slow · try_alloc · alloc_old   (never collect)
   │             RootScope · NoGcScope        │─log───►  log_slow · log_range                 (barrier slow path)
   │ VM interp : tagged frames + dense maps · │─post──►  Mutator.pending/event ; reg_limit := 0   (§12)
   │             frame-entry poll · return_into│─hash──►  identity_hash (BFG; never pins) · pin → PinToken
   │             · capture = T_CONT memcpy    │          is_live · forwarded · attrs() → GcAttrs (data)
   │ JIT tiers : x21/r15 = Mutator · Leaf |   │◄─data──  GcAttrs: alloc/barrier/poll/watermark/publication/
   │             Transfer · publish at every  │          can_move/can_pin/layout/helpers → emitters + Rust twins
   │             suspension · (code,pc) truth │
   │ Tree-walker: whole-heap, non-moving,     │          epilogue §9.9 (11 fixed steps) → finalizers run
   │             pinned roots · host ids      │          at the poll, Rust-only, before Scheme resumes →
   └──────────────────────────────────────────┘          post-pause slices (sweep, classify, decommit)
        ▲ Mutator ABI block, 128 B aligned (§14:1217)        │ trace objects            ▲ root slots
        │ ap·limit·meta_bias·remset·reg_top·reg_limit·        ▼                          │
        │ event·pending·status·thread·heap (+reserved)    L4 OBJECT MODEL  patina-core    │
        │                                                 declare_layouts! → CoreModel:  │
        │                                                 size/trace/copy/verify, JIT    │
        │                                                 offsets · no Drop · invariant W│
        │                                                                                │
        │                                                 ROOT PROVIDERS ────────────────┘
        └──── one MutatorSet: N carriers (M:1 today)      VM frames above watermark · TW StepResult (pinned) ·
              × green threads (ThreadGcState)             scheduler's live threads · namespaces · RootScope ·
                                                          Owned · host payloads (HostPayloadTable, in fixpoint)
```

**Arrows down (collector promises):** allocation never collects; collection only at a poll, through `&mut Heap`; a
user-sized `Oom` is retried once after a major; nothing moves before stage 8, and LOS/NMS/immortal/pinned/tree-walker
objects never move; hashes survive moves; ephemerons are linear and broken only when the key is dead; finalizers run
Rust-only before Scheme resumes; pauses have one budget form; SS1–SS5 and M1–M5 hold; limits raise catchable
conditions naming what grew.

**Arrows up (obligations):** every Scheme value tagged in VM memory at every suspension point; maps at every suspension
point, dead slots cleared; every heap store through the funnel or a barrier sequence generated from `GcAttrs`; no raw
movable address kept across a poll; no `Drop` payload; every object kind declared once; every Rust table bounded or
pruned; external bytes reported; no runtime state in `thread_local!`.

---

## 1. Collector → runtime: promises

| # | Promise | PRD | Enforcement | Stage |
|---|---|---|---|---|
| C1 | **When collection can happen.** Only at a poll, with every Scheme value in VM-managed memory, and only from code holding the driver's `&mut Heap`. Polls: frame entry (non-tail call, tail call, `Apply`, `TailApply`), the self-tail-call back-edge, every `Transfer` return, return to the driver. No collection sees a half-built frame. Never inside allocation or barrier slow paths, never reachable from a `Cx` | §9:528-532; §12:1014-1029; §14:1288-1289 | T (`collect(&mut self)` reachable only through `&mut Heap`; `Cx` has no collecting method); L (zeal `entry` services at every poll); K16 counters | 3 (frame-entry polls; the per-instruction poll goes) |
| C2 | **Allocation never collects.** The slow path refills, overdrafts, posts `GC_MINOR`/`GC_MAJOR` and continues; it never runs Scheme; it fails only at hard limits. So allocation sites are not safepoints and initializing stores need no barrier | §8:499-503, step 5 :522; B2 :288 | T (`alloc_slow` contract, §14:1151); L (conformance suite); K16 (window high-water) | holds today; kept throughout |
| C3 | **User-sized requests are fallible and retried once.** `make-vector`, `make-string`, `make-bytevector`, string-port growth, `read-string` and capture call `try_alloc` before any visible change; on `Oom` the machine does `Step::CollectAndRetry` (a full major at the call's return pc, arguments still in the suspended frame), then one more call; a second `Oom` raises `&heap-exhausted`. A request larger than `max_heap` − reserve − the uncollectable floor raises at once. Where collection is deferred, the first `Oom` raises. Capture never draws on the emergency reserve | §8:523; §17.3:1494 | L (`large_request_after_garbage` on both backends; limits lane) | 5e (retry); 5g (whole-limit check) |
| C4 | **Hard ceiling behaviour.** A small infallible allocation over `max_heap` dips into a 4 MiB emergency reserve and posts `HEAP_EXHAUSTED`; the next poll runs a maximal major (drops caches; evacuates from stage 8), then raises a catchable `&heap-exhausted` whose payload names the limit, the managed bytes and the per-owner key that grew most; uncaught, a non-zero exit (SD9). If the reserve runs out before a poll: flush ports (F1), abort with a diagnostic | §8:524; §17.3:1490-1498 | L (limits lane, handler-room test, allocation-failure zeal, downward `--heap-max` sweep); K19 | 5e, 5g |
| C5 | **`(gc)` is a full major that collects at its call**, a `Transfer` primitive returning `Step::Collect(Major)` serviced before the caller's next instruction. Inside `NoGcScope` it posts and counts a deferred poll | §9.5:593 | L (`ephemerons.rs:94,109,127`; dead key of any age breaks after one `(gc)`) | 1 (collects at its call); 3 (gate) |
| C6 | **Determinism.** Stop-the-world, fixed trace order; under stage P, marks, broken ephemerons and addresses do not depend on the worker schedule. Triggers are bytes and counts, never time; no timer runs under `PATINA_DETERMINISTIC=1` | §9:528-529; §12:1031-1034; §15:1297-1301 | L (every mode byte-identical to `off` on the chibi suite; determinism replay; `workers=4` matches 1) | 1 (byte trigger); 5e (pacing); P |
| C7 | **What moves, and when.** Nothing moves through stages 5–7. From stage 8, opportunistic evacuation of SOS blocks under ¾ live with no pin, starting above 10% fragmentation (stopping below 5%) or on the footprint trigger. Forwarding state lives in the metadata byte (`FORWARDED`), the new address in word 0, and a visitor rewrites a slot as `new \| (old & 15)` | §9.4:574-582; §5:365-367; §7:464 | L (move-all lane green 4 weeks; zeal `move-all` poisons from-space; verifier finds no reachable `FORWARDED`); K3 can pull 8 ahead of 7 | 8 |
| C8 | **What is never moved.** LOS, NMS (code descriptors, record types, symbols, global cells, binding records), the immortal space, pinned blocks (per-block pin count, `PinToken`), tree-walker heaps. `SlotVisitor::pinned` roots are never updated. Identity hashing never pins | §9.4:580-581; §7:460-467, 484-486; §14:1125 | T (`pinned` takes a `Word`, not a slot); L (move-all) | 5a (spaces); 8 (when it matters) |
| C9 | **Identity hash.** Immediate → `mix64(bits)`; symbol → stored fixed-seed hash of its UTF-8 name; first hash sets `HASHED`; the hash is heap-base-relative, so identical across runs despite ASLR; an evacuated hashed object carries the original hash in a trailing granule (`HASH_MOVED`, Bacon–Fink–Grove). SRFI 69 stored hashes stay valid | §9.8:641-659; decision 10 | L (verifier checks `HASH_MOVED` extensions; poison lanes compare output, never hash values); `DIVERGENCES.tsv` row for symbol-keyed `eq?` order | 5c (symbol name hash, own PR) |
| C10 | **Weak references and ephemerons.** Resolved by key in one fixpoint (`pending[K]`, `KEYHINT`), linear work; an ephemeron is never older than its key or value (allocated young only, promotion age-monotone, no remembered-set edge for ephemerons); `is_live(K)` holds for immediates, immortal, marked, or (in a minor) old keys; a symbol key keeps its symbol alive; breaking writes `BWP` into key and value, which never reaches Scheme (`ephemeron-key`/`-datum` answer `#f`). One fixpoint covers host payloads, ephemerons and later guardians (separate sequencing was a use-after-free, commit `1d18c49`). The symbol table is weak, pruned at majors | §9.5:584-594; §5:356; §17.2:1446 | L (conformance: 16 K chain resolved linearly, counted; live key never broken; dead key broken after a `complete` collection; `ephemerons.rs`) | 5a (weak contract); 5 ([#609]); 5c (weak symbol table) |
| C11 | **Finalization.** `FinalRegistry { young, old }` of `{obj, kind: Port \| CodeUnit \| HostPayload \| Thread \| Foreign, id}`, registered only by slow-path constructors. A young entry neither marked nor forwarded is queued after a minor; unmarked old entries after a major; a runtime may queue a finished object itself (a terminated thread). Queued entries run **in id order, after the pause, before Scheme resumes**, in the poll slow path, Rust-only (no allocation, no Scheme). F1 exit flushes every heap's file ports; F2 a dead file port is flushed and closed before Scheme resumes; F3 descriptor pressure and `EMFILE` collect-and-retry; F4 no Scheme, no allocation; F5 heap drop finalizes everything; F6 one canonical port object. GC timing is declared observable | §9.6:598-616; decision 12 | L (conformance: `drain` in id order, `drain_all` at teardown, nothing finalized inside `collect`; port tests outside the byte-identical lane) | 1 (`EMFILE` retry, descriptor pressure); 2 (F5, [#604]); 4a (F2, F6, `PortTable` registry); 5a (registry type) |
| C12 | **Code liveness and the eager-release contract.** Units are released per unit; a unit is live iff one of its descriptors is marked (procedure word 0, frame `code` words, continuation frames, nested constants). A unit whose `escaped` bit was never set is released as soon as its form finishes, with no collection; an escaped unit waits for a complete major. JIT bodies go with their unit, freed by epoch only when no JIT activation is on the native stack | §9.7:618-639 | L (`finished_forms_release_code.rs:72-99`: 2,000 forms, no collection); D (released descriptors' raw fields poisoned, nothing may reference them); zeal `jit-invalidate` | 4e (traced code liveness); JIT (bodies) |
| C13 | **Epilogue, in a fixed order** (11 steps): trace; one fixpoint (host payloads → ephemerons by key → later guardians, resurrecting before any breaking); break pending ephemerons; (later) transport cells; weak-key tables and weak inline-cache entries; prune weak-id and host-payload tables, and at majors the interner and namespace weak entries; classify the finalization registry; code release (majors); queue post-pause work; statistics and pacing; release the world. Minors restrict every step to young entries | §9.9:661-682 | L (conformance suite checks the order) | 5a |
| C14 | **Pause budgets, one form.** Major: c + a·(frames in all rooted stacks) + b·(live MiB). Minor: c_min + a·(frames above watermarks) + d·(logged granules) + b·(survivor MiB). c ≤ 1 ms, a ≤ 15 ms per million frames, b ≤ 0.35 ms per live MiB, c_min ≤ 0.25 ms, d ≤ 10 ns per granule. **No term in dead objects, committed heap size or the arena high-water mark.** The frames term is deliberately unbounded (a million-frame descent costs about 13 ms in one minor); the logged-granules term is bounded by the store buffer's soft limit | §9.10:684-711; decision 6 | K (per-workload max pause and MMU(10 ms) on GBS, `deep-descent`, `deep-unwind`, `large-live`); K9 (queue3 < 15 ms) | 5a (a, b, c measured); 7b (c_min, d); P (if `large-live` > 100 ms) |
| C15 | **Steady state** on deterministic readings (two forced `(gc)` at N and 4N): SS1 L follows the program; SS2 footprint ≤ F(L) = 1.1·(L + max(8 MiB, 2·L)) + free reserve; SS3 in-pause work stationary; SS4 operation cost flat; SS5 side structures bounded. Phase change: empty runs return within 2 majors (or one idle major); partly occupied blocks are reused, not returned, until stage 8; idle trim at waits, with back-off; the livelock boost decays. Outside the contract: growing name sets (class D), threads blocked for ever, `NoGcScope` windows | §17.1:1387-1432 | L (steady-state lane: rows < 2 s per PR, the rest nightly, both backends); K17 (a stage that turns a green row red is blocked) | 0 (lane); 1 (SS1, SS2, SS5); 5e (SS3, phase change, idle trim); 9 (thread rows) |
| C16 | **Memory contract.** M1 only binary- or text-bounded objects are immortal; everything made at run time is mortal. M2 every Rust table keyed by name, id or address is bounded by program text or pruned when its heap object dies. M3 Rust bytes a heap object keeps alive count as external bytes (in L, the trigger and `max_heap`). M4 no lookup scans a table that grows with history. M5 empty memory returns. The per-source table names each owner and stage | §17.2:1434-1464 | L (steady-state lane, per-owner `(gc-stats)` keys); K18 (M1 cost) | per row of §17.2:1444-1456 (1, 2, 4a, 4b, 4c, 5c, 5e, 6, 8, 9) |
| C17 | **Limits.** Hard heap `max_heap` = min(16 GiB, 75%·B), B = min(RAM, cgroup limit), counting external bytes (SD4); heap ceiling; opt-in soft target (SD8); main register stack min(8 GiB, 25%·B), green threads 256 MiB, `&stack-exhausted` within 1 MiB of the cap; a native-stack depth guard; descriptors; JIT code cap (stop tiering, no error); address space fitted under `RLIMIT_AS`. Progress guard (SD6): 5 consecutive counted majors each freeing < 2% raise | §17.3:1466-1504; §11.1:800-805 | L (limits lane, outside byte-identical lanes); K19 | now ([#617] guard); 4d (stack, B); 5e (heap, ceiling); 5g (rest); 9 (threads) |
| C18 | **Observability.** `GcStats` per collection (kind, reason, `complete`, phase times, bytes, blocks, store-buffer entries, frames scanned, deferred polls, K16 high-water marks with sites, time-to-safepoint); MMU ring buffer; representation-independent `(gc-stats)` keys | §15:1311-1323 | L (reclamation proofs use these keys, `bytes-reclaimed` > 0) | 0, 1 |
| C19 | **Barrier honesty.** A heap whose policy is not generational reports `BarrierKind::None`: JIT code emits nothing and the funnel's branch is never taken. A store buffer past its soft limit while collection is deferred is discarded and the next collection forced major | §10:741-743, 757-760 | L (barrier-tax experiment, K2) | 7 |

**Today.** `Collector::collect(&mut self, heap, roots)` is called from the outermost driver only
(`crates/patina-core/src/heap/gc.rs:208-212`); the decision is a flag raised by `note_alloc`
(`crates/patina-core/src/heap/mod.rs:575-584`), serviced at a per-instruction safe point
(`crates/patina-vm/src/runtime/vm_state.rs:1286-1309`, `crates/patina-tree-walker/src/eval/cps_eval/mod.rs:112-133`).
The collector must be non-moving because raw arena indices escape (`gc.rs:199-202`). Symbols are strong roots
(`gc.rs:449-465`).

---

## 2. Obligations on the VM interpreter

| # | Obligation | PRD | Enforcement | Stage |
|---|---|---|---|---|
| V1 | **The register stack never relocates**: one fixed reservation per green thread, committed on demand, capped (main min(8 GiB, 25%·B), others 256 MiB); within 1 MiB of the cap the frame-entry check raises `&stack-exhausted` | §11.1:800-805 | L (10 M-deep recursion; limits lane) | 4d |
| V2 | **One frame header** for interpreter, JIT and captured copies, 40 B, interleaved and position-independent: `ret` (raw cache; 0 in interpreter frames and captured copies), `link` (fixnum: caller distance, nregs, flags incl. `WM`), `code` (value), `meta` (fixnum: pc of last suspension point, `return_reg`), `closure` (a traced value), then tagged registers. Prompts, handlers, winds record byte offsets | §11.1:807-823 | L (matrix 64/64; raise-site tests) | 2 (`closure` as a value); 4d (format) |
| V3 | **Frame invariant 1, window initialization**: every frame push, stubs included, writes an immediate into every non-parameter slot | §11.1:829 | L (verifier; [#423] tests) | 4d |
| V4 | **Invariant 2, dense maps at every suspension point**: the return pc of every call (including `CallPrimitive` and every inline primitive opcode, whose slow path or shadow deoptimization calls from that pc); the pc after every instruction that can raise; pc 0; every `Transfer` helper site. `pc → u16` map index per body, `0xFFFF` = never a suspension point | §11.1:830 | D (a frame at a `0xFFFF` pc panics); L (verifier: every frame pc reached has a map) | 4d |
| V5 | **Invariant 3, clearing, not skipping**: the root provider visits each live slot through the map plus `code` and `closure`, and overwrites each dead slot with `UNSPECIFIED` (`DEAD_SLOT` in poison and zeal lanes). Stub frames have all-live maps. Capture clears dead slots in the copy too | §11.1:831 | D (`reg_at` asserts no instruction reads `DEAD_SLOT`); L (poison, zeal; [#423] tests catch retained garbage) | today (`retire_registers`, `vm_state/gc_roots.rs:47-68`); generalized 4d; "stays for good" (§19:1846-1848) |
| V6 | **Invariant 4, other raw readers** (`StepTracer`, watchpoints, debugger hooks, `--dump`) read registers only at suspension points; attached hooks pin the interpreter tier | §11.1:832; §11.2:879-883 | R; L (tracer tests) | 3 (tier-policy flag); 4d |
| V7 | **Invariant 5, verifier**: every visited register decodes to an immediate or a start granule | §11.1:833; §16:1334-1339 | L (`PATINA_GC_VERIFY=1` under zeal) | 5a |
| V8 | **Polls at frame entry**, after the header and arguments are stored and the window initialized: `top > reg_limit`; an event is serviced at the callee's pc 0. A self tail call re-enters at pc 0 through the same check. `Transfer` helpers service pending events before answering the next `(code, pc)`. The per-instruction `gc_pending` load (+1.1–1.4%) goes. Allocation and barrier slow paths only post | §12:1014-1029 | L (zeal `entry`; `ephemerons.rs:94,109,127`) | 3 |
| V9 | **Barrier at every heap store**: the interpreter's `#[inline(always)]` Rust versions of alloc/store/poll are generated from the same `GcAttrs` table as the JIT emitters; no barrier on register-stack or frame stores (roots, covered by the watermark), on continuation objects (immutable) or on descriptors after creation | §10:757-760; §14:1260-1261 | L (conformance: emitters against Rust twins; verifier remembered-set completeness) | 3 (funnel); 7 (barrier armed) |
| V10 | **`return_into(frame)` is the one choke point** for every write into, or activation of, a suspended frame (value delivery, `ResumeWindJump`, raise stubs, abort landing, delimited append, scheduler deliveries, debugger writes): it lowers the watermark and sets the thread's `ran_since_gc`. A tail call copies `link` and `ret`; reinstating a continuation resets the watermark to the base | §11.1:835-845 | L (verifier: no young reference below a watermark; matrix under zeal-minor); K8 (can switch the watermark off) | 7 (with the watermark; §19 does not stage the choke point separately) |
| V11 | **Continuation capture (design A)**: `memcpy` of `[base, top]` into a `T_CONT` allocated through `try_alloc`; clear dead slots through the maps plus the `call/cc` `dst` hole; zero every `ret`; clear `WM` flags. The copy is value-only, so the core traces it word by word without parsing frames. Captures over 8 KiB go to the LOS. Reinstatement copies frames out; multi-shot copies out on every invoke; delimited capture relocates by byte offsets | §13:1054-1086 | L (matrix and `escape_from_primitive.rs` on both backends gate 4e; capture targets) | 4e |
| V12 | **Root provider**: the VM implements `RootProvider`, walking frames through the maps, reading each thread's watermark from `MutatorSet` and re-installing it as it finishes; every rooted thread's dynamic-state slots are roots in every collection | §14:1131-1145; §9.1:536-541; §9.2:549-551 | L (`VERIFY_ROOTS`) | 2 (slot visitor); 7 (watermarks) |
| V13 | **Nested loops**: until 4e every nested VM loop defers (the weak continuation tables' soundness rests on it); from 4e loops the VM enters from its own driver level may collect, loops beneath a `Cx` stay deferred for good. Green-thread preemption is deferred while `reentry_depth > 0` | §11.3:930-933; §12:1036-1042 | K16; L (4e's nested churn probe collects with a bounded heap) | 4e |
| V14 | **Globals under variant R**: `LoadGlobal`, `StoreGlobal`, `Define` take a link index; a read is `link[k] → record → cell → value`; the shadow bitsets and `mark_if_*` stay and drive per-site deoptimization. Under variant C, per-binding guards on `cell.value == expected` | §11.6:960-984 | L (rebinding tests; `vm_callprimitive.rs` set-after-use tests; `run_suite_oracles.sh`) | 4b (R); C (after 5) |

**Today.** `VmState` is one `GcRoots` that traces the whole register file after `retire_registers`
(`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:70-76`); `CallFrame::closure` is a bare `HeapIndex`
(`gc_roots.rs:17-19`); continuations are weak side tables whose soundness rests on "every store touch confined to one
instruction dispatch and nested loops defer" (`gc_roots.rs:8-24`). `TailCall` stages arguments in a Rust buffer
(`vm_state.rs:1601-1630`). The informal rule "publish all live Scheme values before servicing a safe point" is in
`crates/patina-vm/src/runtime/control.rs:23-30`.

---

## 3. Obligations on JIT code, by tier (§11.2)

| # | Obligation | PRD | Enforcement | Stage |
|---|---|---|---|---|
| J1 | **VM register-stack frames are the only home of Scheme values at every suspension point (every non-`Leaf` call), and hold only tagged values, in every tier.** No Cranelift user stack maps, native frame walker, unwinder or `stack_switch`; no derived pointer live across a suspension point. Tier 1 caches registers in SSA between suspension points and stores dirty values before each. A later tier 2 may unbox and derive within a block but publishes tagged values only, so one `(code, pc)` means the same in every tier and capture stays a `memcpy` | §11.2:851-857; decision 8 | L (verifier on JIT frames; zeal); K12 (tier-2 cost > 15% → more `Leaf` helpers first; native maps only through a separately approved design) | 6 (frozen) |
| J2 | **Fragments are the only baseline**: `CallConv::Tail` fragments with `return_call_indirect`; a return goes through a trampoline, not a native `ret`. The JIT delivers a result as the first `Tail` argument of the caller's resume entry (`x2` aarch64, `rdi` x64). Native call/ret is outside this contract | §11.2:853-855; §11.1:821-823 | K11 (fragments > 8% slower than native call/ret on fib/tak/nboyer → a separate native design) | 6 |
| J3 | **Helper classes.** `Leaf`: never collects, runs Scheme, transfers control, blocks or enters a safe region; values stay in SSA across the call; an error comes back as a status (`Mutator.status`), after which the fragment takes a `Transfer` path to raise it, or for `Oom` to collect and retry. `NoAlloc` marks `Leaf`s that do not touch the allocation buffer; only they may keep `ap`/`limit` in SSA across a call. `Transfer`: everything else (poll slow path, calls of closures, resumable and higher-order primitives, `apply`, shadow deoptimization, `call/cc`, continuation invocation, raise, winds, abort, `(gc)`, blocking I/O, tier-up and invalidation, `WATCHED` stores) | §11.2:859-864; §8:510-512 | T (`#[helper(class = …, noalloc)]` attribute; `GcAttrs.helpers` table); L (call-graph test: no `Leaf` reaches a `Transfer` function, no `NoAlloc` reaches an allocation); D (`NoAlloc` assertion) | 3 (classes on host primitives); 6 (frozen) |
| J4 | **Publish before a `Transfer`, reload after.** The fragment publishes dirty values, `pc` and `ap`; the helper services pending events and answers `(Continue \| Target(entry), value)`; the fragment reloads and continues or tail-calls the target. A thread switch, signal, debugger stop or deoptimization therefore resumes elsewhere, never in a stale native frame. Allocation sites publish nothing | §11.2:864; §8:501-502 | L (emitter conformance; matrix under zeal) | 6 |
| J5 | **`(code, pc)` is the resume truth; `ret` is a cache**, re-derived on reinstatement from `desc.resume[pc]` or the interpreter trampoline. Captured copies hold no `ret`, so a multi-shot continuation never jumps into discarded code. The watermark saves the real return target as `(code, pc)`, never a raw address. Invalidation (a `WATCHED` store, tier-down, attached hook) marks dependent bodies invalid, deoptimizes the executing fragment and re-derives the `ret` of every frame of the affected descriptors in every green thread's stack | §13:1077-1082; §11.1:837-838 | L (matrix; zeal `jit-invalidate`) | 4e (capture zeroes `ret`); 6 |
| J6 | **No embedded movable addresses.** Machine code embeds an address only if it is (1) in the immortal space; (2) a descriptor of the body's own unit; (3) guarded by a `WATCHED` dependency; (4) an NMS object the unit's constants or link table reference. Otherwise one load from the descriptor's traced constants. Inline-cache words are traced. The `Mutator` is never embedded. Code is never patched at GC; tier-up writes `desc.entry` (data) | §11.2:866-872; §17.2:1453 | L (contract test: embedded addresses limited to the four cases, §16:1347-1348) | 5c, 6 |
| J7 | **The `Mutator` ABI**: `#[repr(C)]`, 128-byte aligned, pinned in `x21` (aarch64) / `r15` (x64), set by a generated entry trampoline that saves and restores the caller's pinned register (Cranelift never saves it, yet it is callee-saved for Rust), so nested drivers and many heaps per process never corrupt it. Offsets: `ap` 0x00, `alloc_limit` 0x08, `meta_bias` 0x10, `remset_cur` 0x18, `remset_soft` 0x20, `reg_top` 0x28, `reg_limit` 0x30, `event` 0x38, `pending` 0x3C, `ticks` 0x40, `barrier_mode` 0x44, `safepoint_state` 0x45, `thread` 0x48, `heap` 0x50, `status` 0x58; reserved `fenced_ap` 0x60, `quiesce_epoch` 0x68, `handshake` 0x70 | §14:1217-1239; §11.2:874-877 | L (offsets, alignment and `GcAttrs` asserted in CI) | 3 (exists; reserved words); 6 (frozen) |
| J8 | **`GcAttrs` is read as data** by the emitters at compile time: `alloc` (`BumpPointer`), `barrier` (`None \| GranuleLog \| Card \| Call`), `value_filter_bit` (`Some(2)`), `poll` (`LimitFold \| Tick`), `watermark` (`None \| ReturnBarrier`), `publication` (read only under `threaded`), `can_move`, `can_pin`, `initializing_stores_need_barrier` (false), `layout`, `helpers`. Nothing on a fast path is `dyn`; no per-collector codegen hooks | §14:1096-1104, 1241-1261 | L (emitters against Rust twins) | 3 (fields incl. `publication`); 6 (frozen) |
| J9 | **Inline sequences.** Allocation: `ap`/`limit` in SSA, one `add; cmp; b.hi` per basic block's summed allocation, tags applied with `add`, every word written (padding fixnum 0), the refill a `Leaf` helper. Barrier (aarch64): `add` slot; `tbz` value bit 2; `lsr`; `ldrb` metadata; `tbnz` bit 6 → cold block, call-free, which disarms the granule (`ldclrb` under `threaded`), appends to `remset_cur`, owner-posts `GC_MINOR` at the soft limit. 4 instructions for heap values, 1 for immediates. Polls: free at frame entry (folded into the stack-limit check), 2 instructions at back-edges. Globals: 2 dependent loads under R, 1 under C | §8:505-512; §10:720-735; §12:1018-1023; §1:161-165 | L (emitter conformance: every word of every inline-allocated kind); K2, K7 (spike) | 6 |
| J10 | **`WATCHED` cells.** A cell's flags carry `WATCHED` once JIT code depends on its value; a store into it takes a `Transfer` cold path that invalidates dependent bodies and deoptimizes the executing fragment first. Under R, a cell behind a re-pointable record needs `WATCHED` on record and cell, or two loads | §10:762-765; §11.6:976 | L (`vm_callprimitive.rs` set-after-use tests) | 6; C |
| J11 | **Code memory** (decision 20): Patina's own `MAP_JIT` reservation; W^X toggled only in `install_code`; replaced bodies freed by epoch, batched; size-segregated slabs with a coalescing list under a cap (`PATINA_JIT_CODE_MAX`: stop tiering, no error); BTI landing pads if enabled. Any holder of a descriptor reference (inline cache, profiler, debugger) obtains it through `CodeStore::escape`. Under OS-thread carriers, a dual-mapped (`memfd`) code reservation, since Linux `mprotect` is process-wide | §9.7:627-639; §17.2:1452; §17.3:1487; §18.1:1644-1645 | L (`eval-lambda`, `eval-redefine` steady-state rows) | 6 (interface: `install_code`, per-unit free); JIT (allocator, cap) |
| J12 | **Heap access from JIT entry**: the heap is passed as a raw pointer derived from the driver's `&mut Heap`, reborrowed by each helper | §11.3:896-898 | L (Miri on the Rust twins, strict provenance) | 6 |
| J13 | **Debugging pins the interpreter tier**: attaching a hook at a form boundary sets a per-heap tier-policy flag; running fragments leave through the `Transfer` invalidation path | §11.2:879-883 | R; L (tracer tests) | 3 (flag); 6 |
| J14 | **Interrupts**: posted through `InterruptHandle`; free at calls, 2 instructions at back-edges, so Ctrl-C stops a tight loop | §12:1005-1012; decision 9 | L (`InterruptHandle` tests) | 3; 6 |
| J15 | **Under `threaded`**: JIT code may rely on address dependencies instead of acquire loads, under a documented exception forbidding value speculation and equality substitution of loaded references; publication per `GcAttrs.publication` | §18.2:1664 rule 4(f); §18.1:1641-1642 | L (arm64 lanes, loom, when they exist) | 6 (publication spike); after decision 7 |
| J16 | **Explicitly excluded**: load and read barriers; conservative scanning; collection inside allocation or barrier slow paths; patching machine code at GC or outside `install_code`; Cranelift user stack maps, frame walkers, `stack_switch`; raw slots in published frames; native frames holding Scheme values across a suspension point; embedding the `Mutator` or an address outside the four cases | §14:1288-1293 | R; the contract tests above | — |

**No JIT PRD exists.** Until one does, §11.2, §13, §14 and stage 6 are the GC's whole contract with it (§1:170-172).
Stage 6 is 3–5 weeks on an unmerged branch beside stage 5; it freezes the `Mutator` ABI, the helper classes, the frame
contract and the entry trampoline, not object layouts, which `declare_layouts!` regenerates into the JIT offset table
(§1:167-170). K6 (self-tagged flonums), K7 (2-load call, limit-fold poll) and K11 (fragments) are decided there.

---

## 4. Obligations on Rust runtime code (primitives, frontend, macros, runtime, loader)

| # | Obligation | PRD | Enforcement | Stage |
|---|---|---|---|---|
| R1 | **`&mut Heap` is the only capability that can collect.** Held by the VM's and tree-walker's outermost driver loops, the loader at points A and B, `eval_datum`, `run_forms`, and the embedding API's `eval_*` and `call`. Until 5e it is a `GcDriver` token over the `RefCell`-wrapped arena heap | §11.3:887-889, 898 | T (borrow checker refuses `&mut Heap` while any `Cx<'gc>` borrows the heap) | 3 |
| R2 | **`Cx<'gc>` is the mutation and allocation context**, created by `heap.mutate(\|cx\| …)` (HRTB-branded) or by a driver for one primitive call. No method on `Cx` can collect. Reads take `&Cx`; mutation and allocation `&mut Cx` until 5e, `&Cx` after. Bulk reads return `&[HeapSlot<'gc>]` tied to a `&Cx` borrow. No public API returns `&BigInt`, `&mut Vec<char>` or `&mut [Word]` into the heap | §11.3:890-893, 917-919; §8:506-507 | T | 3; 5e (receiver change) |
| R3 | **The `'gc` brand**: `Value<'gc>` is `#[repr(transparent)]` word + invariant `PhantomData`; so a `Value<'gc>` cannot be alive while a collection runs; roots carry values across | §11.3:893-894, 900-904 | T (trybuild compile-fail tests: a value used after a may-collect call, a slice held across `load_library`, a value in a `static`, a value escaping `interp.with`; in primitives, frontend, macros, embedding API, `patina-compat`) | 3 |
| R4 | **Primitive shape**: `for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>]) -> Result<Value<'gc>, EvalError>`; resumable primitives return `Step<'gc>`: `Done \| Call \| Eval \| Collect(kind) \| CollectAndRetry`. A primitive never calls a procedure from Rust; it returns `Step::Call` (AGENTS.md rule, kept) | §11.3:900-904; §11.5:954 | T (`Cx` has no `call`) | 3 |
| R5 | **`NoGcScope`** for any nested driver entry reachable from a `Cx`: point C (an import met mid-form), the residual `apply_proc` fallbacks, a paused debugger's evaluation, `across_reentry` until 4e, nested tree-walker trampolines. Polls run deferred (the poll slow path collects only with `&mut Heap` **and** `no_gc_depth == 0`); `(gc)` posts and counts; a deferred event stays posted while allocation overdrafts. Windows are confined, counted and bounded: `GcStats` keeps the high-water bytes allocated since the last serviced poll and under one `NoGcScope`, with the opening site; a driver about to enter a known `NoGcScope` site first runs a major when headroom is below that site's high-water | §11.3:891-893; §12:1036-1042; §17.3:1497 | T (entry type); runtime counter `no_gc_depth` (on the `Mutator`, §18.1:1614); K16 (window > max(64 MiB, 25% of target) → add a collection point at that site; allocation never becomes one) | 3 (replaces `GcDeferGuard`); 5g (pre-entry major) |
| R6 | **The store funnel is the only way to write a value into a heap object**: `Cx::store(slot: impl SlotOf<'gc>, v)` with typed slots (`PairCar`, `PairCdr`, `VectorElem` bounds-checked, `CellValue`, `RecordField`, `ClosureFree` fix-up only, `GlobalCellValue`, `PromiseBox`, `ParamValue`); bulk stores through `store_range`/`fill_range` (one range entry); `HeapSlot::at` derives its pointer from the per-heap base (provenance kept). `Heap::vector_slice_mut`, `get_string_chars_mut`, `get_bytevector_mut` are deleted; records, parameters and promises are written through the funnel. Under `threaded`, the funnel is the one Rust place publication is enforced | §10:767-794; §18.1:1634-1635 | T (no `&mut` into heap words); L (Miri, strict provenance; verifier remembered-set completeness; barrier-tax experiment) | 3 |
| R7 | **`Fresh` tokens for initializing stores**: only through `Fresh<'m, 'gc>`, which holds the mutator borrow, so no safepoint can intervene; only values of the same heap; constructors write every word, padding as fixnum 0; hole data is never zeroed | §10:784-791; §8:507-508 | T (`PhantomData<&'m mut Mutator>`); D (`Fresh::init` asserts the holder is young or pre-logged) | 3; 5 (layouts) |
| R8 | **What may hold a value across a collection, and nothing else**: resumable-primitive state (VM `resume_stub` frame, tree-walker `ResumePrimitive` continuation, both traced); `RootScope<'h>` (wraps `&'h mut Heap` so `mem::swap` cannot break LIFO; `Rooted` carries `{heap_id, generation}`, checked on `get`); `Owned` (`{heap_id, index, generation}` + `Weak<HandleTable>`); `RootToken`, `PinToken` (each holds a `Weak` to its table); a registered `RootProvider` (`RootSet::register(Box<dyn RootProvider>) → RootToken`) | §11.3:921-925; §14:1131-1133 | T (lifetimes; generation checks); L (`PATINA_GC_VERIFY_ROOTS=1`) | 2 (`Owned`, slot visitor); 3 (`RootScope`) |
| R9 | **Long-lived values outside the core crates** (`CoreExpr`/`CpsExpr` literals, macro literals, `Step` state) use a `pub unsafe` raw-word API under `NoGcScope` or a traced root, or the `CoreExpr` literal pool, a traced table shared by both backends and scoped to one compilation (M2) | §11.3:908-910; §17.2:1450 | R (unsafe API); L (steady-state lane) | 3 |
| R10 | **The unsafe boundary**: `#![forbid(unsafe_code)]` in `patina-primitives`, `-frontend`, `-macros`, `-runtime`, `-ir`, `-pipeline`, `-interpreter`, `-compat`; `deny` in `patina-repl` with an audited allow; unsafe heap access confined to `patina-core`, `patina-gc`, `patina-vm`, `patina-tree-walker` and the JIT crate; `Word → Value` and raw slot access are `pub unsafe fn`; release-mode reservation range checks at the trust boundaries; `PENDING_ESCAPE` leaves `thread_local!` | §11.3:912-917 | T (lint attributes) | not staged in §19 (implied by 3) |
| R11 | **Loading**: a bare top-level `(import …)` and `define-library` are recognized by the binding of `import`, never its spelling, and processed outside the desugarer, so library bodies collect between forms (A, B) and inside them (D, outermost loop). `%parameterize-swap!` stops calling back at 4a | §11.3:927-934 | L (stage 2 gate: library bodies loaded through `import` collect) | 2 ([#610], [#614]); 4a |
| R12 | **Rust tables obey M2/M3**: every table keyed by name, id or address is bounded by program text or loses entries when its heap object dies (finalization or epilogue step 6); Rust bytes kept alive by a heap object are charged as external bytes. The off-heap holder inventory (DESIGN E.1) names each owner | §17.2:1436-1442 | L (steady-state lane, per-owner keys); R (inventory on stage 0's tracking issue) | per §17.2 row |
| R13 | **Posting protocol**: owner posts `pending \|= bit; reg_limit.store(0, Relaxed)`; remote posts (signal handlers, timers, profilers, later other carriers) `event.fetch_or(bit, SeqCst); reg_limit.store(0, SeqCst)`; `set_limit(new)` stores, fences, and re-zeroes if `event` or `pending` is non-zero (OCaml's Dekker pair: no request lost) | §12:990-1003 | L (`InterruptHandle` tests; two-mutator lane) | 3 |
| R14 | **Safe regions** wrap blocking I/O and FFI: entered only with no unrooted heap value on the Rust stack or in JIT SSA; a `Leaf` never enters one; blocking primitives are `Transfer`; the blocking part works on Rust buffers (output formatted first; input read before anything is allocated). No-ops under one mutator | §12:1044-1048 | R (restructuring required only before N > 1) | 9 |
| R15 | **No runtime state in `thread_local!`**; no new `static mut`; no heap-side `Rc`/`RefCell`/`Cell` | §18.1:1618-1619; §16:1349-1350 | L (CI check, today's 19 `thread_local!`s allowlisted) | 3 |

**Today.** `GcDeferGuard` is the only mechanism for Rust-held values, an RAII counter on `SharedHeap`
(`crates/patina-core/src/heap/gc.rs:218-269`), taken at five sites: `crates/patina-vm/src/runtime/vm_state.rs:343,
1151`, `crates/patina-tree-walker/src/eval/cps_eval/mod.rs:219`, `crates/patina-runtime/src/library_loader.rs:208`,
`crates/patina-frontend/src/desugarer/mod.rs:1841`. Records, parameters and promises are written through cloned `Rc`
handles that bypass the heap (`crates/patina-primitives/src/primitives/records.rs:262`, `parameters.rs:168,267`,
`lazy.rs:134`, cited at §10:792-794).

---

## 5. Obligations on the tree-walker

| # | Obligation | PRD | Enforcement | Stage |
|---|---|---|---|---|
| T1 | Its heaps are `HeapPolicy { generational: false, evacuation: false }` **for good**: whole-heap, non-moving, sharing the object model, primitives, allocator and funnel | §11.4:938-940; decision 1 | R; L (tree-walker lanes) | 5 |
| T2 | Collects only at its outermost trampoline safepoint; nested trampolines keep `NoGcScope`; where collection is deferred, the first `Oom` raises | §11.4:940-941; §8:523 | K16 (evaluator-owned `StepRoots` if a window grows) | 3 |
| T3 | `Rc<Environment>` frames, `ContValue` chains, `StepResult` fields, `CpsExpr` literals and the pending escape are reported **by value through `SlotVisitor::pinned`**, sound only because nothing in its heaps moves | §11.4:941-943 | L (`VERIFY_ROOTS`; tree-walker zeal lanes) | 2 |
| T4 | `CpsLambda`/`CpsContinuation` become host-payload ids; an `Environment` is charged as external bytes only while a registered host payload holds it | §11.4:943-945; §6:432-433 | L (no environment-driven collection on `(fib 25)`) | 4f |
| T5 | Globals go through the same binding records and cells as the VM | §11.4:943-944; §11.6:976 | L (rebinding tests on both backends) | 4b |
| T6 | Stays M:1 and `!Send`; a green thread is one suspended `StepResult` in the thread table, reported through `pinned`, switching only at the outermost trampoline safepoint. Its host payloads are `!Send` | §18.1:1629; §18.2:1668 rule 8; §14:1183 | T (`HostPayload` not `Send`) | 9 |
| T7 | **A red tree-walker lane blocks the stage that turned it red.** It may lag only in steady-state rows K17 allows | §11.4:945; §20:1956 | L; K17 | every stage |

**Today.** Roots are `Evaluator`, `LibraryRegistry`, `StepRoots` built at each safe point and `EscapeRoots` for the
`PENDING_ESCAPE` thread-local (`crates/patina-tree-walker/src/eval/cps_eval/gc_roots.rs:29-60`); every trampoline entry
takes a `GcDeferGuard` (`cps_eval/mod.rs:219`).

---

## 6. Obligations on (and promises to) embedders

| # | Clause | PRD | Enforcement | Stage |
|---|---|---|---|---|
| E1 | `eval_*` returns `Owned` (`{heap_id, index, generation}` + `Weak<HandleTable>`; a `Drop` after teardown is a no-op), fixing the use-after-free of an `eval_str` result read after a later collecting `eval_program` | §11.5:953; §11.3:924 | T; L (embedding tests on both backends) | 2 ([#605]) |
| E2 | `interp.with(\|cx: &mut Cx<'gc>\| …)` is branded and takes `&mut self`, so nothing that may collect runs inside it | §11.5:953 | T (trybuild: a value escaping `interp.with`) | 3 |
| E3 | `Interpreter::call(&mut self, f: &Owned, args: &[Owned]) -> Result<Owned, …>` is a driver entry: it may collect while the callee runs, arguments rooted by their handles; a continuation captured inside behaves as one captured inside `eval_*` | §11.5:954 | L (embedding tests) | 3 |
| E4 | **Host primitives** via `register_primitive(lib, name, PrimSpec { f: Prim, arity, class })` and `register_resumable(lib, name, arity, f)`. `class` declares the helper class the JIT reads (`Leaf`, optionally `NoAlloc`; `Transfer` for one that blocks; resumable means `Transfer`); without a declared class the JIT must treat it as `Transfer`. A procedure argument is called only through `Step::Call`. Registration exports from library `lib`, so fast paths key on the binding, never the spelling | §11.5:955; decision 13 (§2:212) | T (`Prim` signature takes `&mut Cx`, which cannot collect or call); R (the declared class itself: the call-graph test covers the runtime's helper table, and the PRD names no check of an embedder's declaration) | 3 |
| E5 | **Memory hooks**: an external-bytes accounter (V8's `ExternalMemoryAccounter`); `notify_idle()`; a near-limit callback that may raise `max_heap` up to the heap ceiling; a memory-pressure notification | §11.5:956; §17.3:1502-1503 | L (limits lane) | 3 (accounter); 5e (`notify_idle`); 5g (hooks) |
| E6 | **`InterruptHandle`** (`Arc<InterruptCell { mutator: AtomicPtr<Mutator>, posting: AtomicUsize }>`, `Send + Sync + Clone`): `post(ev) -> bool` is async-signal-safe (lock-free atomics, no allocation, no panic path); teardown nulls `mutator`, waits for `posting == 0`, then frees. Servicing `SIGNAL` raises a non-continuable `&interrupt` at the poll, also under `NoGcScope`. No process-wide GC singleton | §12:1005-1012 | L (`InterruptHandle` tests); T (`Send`/`Sync` assertion) | 3 |
| E7 | **Teardown is mandatory**: dropping the interpreter runs finalizers, flushes ports and unmaps the reservation. Current ports belong to the interpreter, never the OS thread. `patina-compat`'s reader uses `Heap::new_standalone()` (collection off, no driver). Plugins never see `HeapIndex` or the encoding | §11.5:958; F5 §9.6:613 | L (`dropped_interpreter_flushes_ports`, `exit_flushes_every_interpreter`, [#618]'s repro) | 2 ([#604]); 4a ([#618]) |
| E8 | The public `Backend` trait migrates one step per stage under [#601]'s deprecation convention | §11.5:957; §19:1849 | L (`scripts/check_embedding_features.sh` green at each step) | 2, 3 |
| E9 | **Many heaps per process**: one VA reservation per heap (318 live heaps in one test process today); the entry trampoline preserves the caller's pinned register across nested drivers and heaps | §7:443-450; §11.2:874-877 | L (`many-heaps` probe); K13 | 5a, 6 |
| E10 | **Behaviour changes they will see**: new default limits (`&heap-exhausted` past min(16 GiB, 75%·B), main stack capped), GC-time port flushing, and the semantic changes listed with their stages, each with `DIVERGENCES.tsv` rows | §1:152-159 | L (oracle-scored rows) | 4a, 4d, 5c, 5e, 5f, 9, C |
| E11 | **Isolates** (optional): one interpreter per OS thread; moving one needs an audited `unsafe impl Send` wrapper, sound only if no `Rc` reachable from it is shared, no `Owned` stays behind, and host primitives and payloads are `Send`. Under shared-memory threads later, host primitives and payloads become `Send + Sync` and host threads attach as carriers | §18.1:1650-1655; §18.7:1824-1826 | R | optional, after 5e |

---

## 7. Obligations on anyone adding an object kind

| # | Obligation | PRD | Enforcement | Stage |
|---|---|---|---|---|
| K-1 | **Declare it once in `declare_layouts!`** (defined in `patina-gc`, invoked in `patina-core`). It generates the `ObjectModel` (`size`, `trace`, `copy`, `verify`), the debug printer, the datum writer's kind view and the JIT offset table, so misfiling a value-bearing variant as a leaf is a compile error | §6:436-439 | T | 5a; AGENTS.md's "New heap object type" becomes this recipe at 5 (§19:1911-1912) |
| K-2 | **Encoding**: headerless (pair, procedure with descriptor first, record with type first) or one immutable header `len \| type \| 0x81` written once by the constructor; no GC state and no mutable flags in a header (mark, end, log, hash, forwarding live in side metadata); immutability variants use distinct type codes; size rounds up to 16 B | §5:311-330; §6:371-373, 384 | T (generator); L (verifier: `END` bits against sizes) | 5 |
| K-3 | **Invariant W**: every word of every object the barrier can log, and every word of a captured continuation, is a value or has low bits `000`. Constructors write padding as fixnum 0; raw fields (`CodeDesc`) are declared raw, never scanned, never a barrier target | §6:375-380 | L (verifier checks invariant W; emitter conformance writes every word) | 5 |
| K-4 | **No Rust `Drop` payload.** Rust-owned resources live in per-heap side tables reached by a `u32` id (`PortTable`, `CodeStore`, the thread table, `HostPayloadTable`), each entry registered in the finalization registry | §6:430-434 | T (generated `const` assertion fails the build if a laid-out type needs `Drop`); K9 (census: `Drop`-carrying allocations ≤ 1%) | 4g (inline payloads); 5 |
| K-5 | **Host payloads** implement `HostPayload { trace(&self, v), external_bytes(&self) }` (traced only on the collecting thread, `slot`s or, in tree-walker heaps, `pinned`); `register` → `HostId` (joins `young`); `dirty(id)` when an old payload is mutated, so the next minor rescans it; `take(id)` only by its finalizer; edges reported through `SlotVisitor::host(id)` | §14:1184-1194 | L (conformance: host-payload edges traced, an unreached payload queued) | 4f (tree-walker), 4e (`CodeStore`), 4a (`PortTable`), 5a (trait) |
| K-6 | **Weak kinds** report through `SlotVisitor::ephemeron`, never as two strong slots; the link word is collector-private and is fixnum 0 outside a collection | §6:403; §14:1126-1129, 1208 | L (conformance) | 5a |
| K-7 | **Placement**: objects that must not move go to the NMS through `alloc_old` (born old, unarmed, pointerful ones pushed on the remember-whole list); the immortal space only for what the binary and program text bound (M1); over 8 KiB to the LOS; ephemerons young only | §7:460-467; §10:750-755; §9.5:592; §17.2:1438 | L (verifier: LOS young list, remembered-set completeness); K18 | 5a, 5c |
| K-8 | **Pointer-free kinds** need no barrier; under `threaded`, sub-word stores (`string-set!`, `bytevector-u8-set!`) become atomic `u32`/`u8` stores | §10:757; §18.1:1640 | R | 5d; 9+ |
| K-9 | **A kind the JIT allocates inline** must have every word written by the emitter | §8:512 | L (conformance test) | 6 |
| K-10 | **Semantic changes** a representation causes (flonum `eq?`, symbol hash, port identity) land in their own PR with oracle-scored `DIVERGENCES.tsv` rows | §19:1842-1845 | R; L (`run_suite_oracles.sh`) | per change |

**Today.** `HeapObjectData` is a 28-variant enum, 19 of which carry `Drop`; adding one needs a hand-placed arm in
`trace_object_children`, and a value-bearing variant misfiled as a leaf is a use-after-free, not a compile error
(`crates/patina-core/src/heap/mod.rs:132-141`).

---

## 8. The pluggability seam (§14): what a collector implements and may assume

**Shape.** `patina-gc` depends on `libc` only, has no VM or core dependency, and is Miri-testable (`VirtualMemory` is a
trait so Miri can run on a `Vec`). `patina-core` generates `CoreModel` and defines `ActiveGc = MarkRegion<CoreModel>`;
downstream crates name `Heap`, never `Heap<C>`. `NullGc` exists only in the conformance suite and is never linked into
the binary. Modes are runtime knobs of one binary; no mutually exclusive cargo features (§14:1106-1171, 1263-1277).
Against HotSpot's JEP 304: `Collector<M>` plays `CollectedHeap`, `GcAttrs.barrier` the `BarrierSet`, and
`emit_alloc`/`emit_store`/`emit_poll` the per-tier halves (§14:1099-1100).

**What a collector must implement**

| Item | Contract | PRD |
|---|---|---|
| `ObjectModel` (unsafe trait; implemented by `declare_layouts!` and `TestModel`) | `LAYOUT`; `size` (+16 if `HASH_MOVED`); `trace` (writable slots; `T_CONT` word by word; `T_EPHEMERON` only through `ephemeron`); `copy_to`; `verify`. Every method requires `obj` to start a live object of this heap | §14:1109-1118 |
| `SlotVisitor` | `slot` (precise, updatable), `slots`, `pinned` (precise, never updated, pins the block this cycle), `host` (edge to a host payload, traced in the fixpoint), `ephemeron`. Frames are walked by the VM's `RootProvider`, not here | §14:1120-1130 |
| `RootProvider`, `RootSet` | `trace(&self, pass: Minor \| Major, v)`; open registration returning a `RootToken` that holds a `Weak` | §14:1131-1133 |
| `MutatorSet`, `ThreadGcState` | owned by the heap, lent to `collect`: every carrier (retire `ap`/`limit`; drain on minors or drop on majors the store buffer, range entries, remember-whole list; deferral high-water) and every rooted green thread (`ran_since_gc`, `watermark`, `stack`) | §14:1135-1145 |
| `Collector<M>` | `new`, `attrs`, `bind_mutator`, `alloc_slow` (never collects), `try_alloc` (user-sized, §8), `alloc_old` (NMS; remember-whole), `log_slow`, `log_range`, `identity_hash`, `pin`, `is_live` (true for the immortal space), `forwarded`, `requested`, `collect(&mut self, kind, &mut MutatorSet, &RootSet, &mut WeakRegistry) -> GcStats` | §14:1147-1164 |
| `WeakRegistry` | `ephemerons`, `host` (`HostPayloadTable`), `finals` (`FinalRegistry`), later `guardians`; per heap, lent to `collect`; **no collector keeps a copy** | §14:1177-1202 |
| `GcAttrs` | per heap, and the JIT reads it (§14:1149); emitters switch on it at compile time (§14:1260): alloc, barrier, value filter bit, poll, watermark, publication, `can_move`, `can_pin`, initializing-store rule, layout, helpers. [I] Because policy is a runtime field that gates flip (§9:530-531) and `BarrierKind::None` emits nothing (§10:758-760), compiled bodies depend on the attributes at compile time; the PRD does not say what happens to them when policy flips (`followup/contract/ANSWER.md` §4, A1) | §14:1241-1261 |

**Obligations of every collector** (§14:1204-1209)

| Obligation | Content |
|---|---|
| Order | §9.9's eleven steps in every collection; a step may be empty (`NullGc` leaves all but statistics and release empty), never reordered |
| Tracing | host edges reported through `host(id)` are traced through `trace_payload`, on the collecting thread, inside step 2's one fixpoint, with ephemerons resolved by key and, later, guardians resurrecting before breaking |
| Safety, always | never break an ephemeron whose key is live; never queue a reachable registered object or payload; never finalize or decommit inside `collect`; never call a `HostPayload` off the collecting thread; leave every ephemeron link word at fixnum 0; retire every mutator's buffer and drain or drop every mutator's logs |
| Completeness, in collections reported `complete` (every `MarkRegion` major, `(gc)` included; never `NullGc`) | every ephemeron with an unreachable key is broken; every unreachable registered object and payload is queued. Minors restrict steps 2–7 to young entries and treat old, immortal and immediate keys as live |

**What a collector may assume** (gathered from the rest of the PRD; the PRD does not list these in one place)

| Assumption | Guaranteed by |
|---|---|
| `collect` runs only at a poll, holding `&mut Heap`, with every Scheme value in VM-managed memory; no `Value<'gc>` is alive on the Rust stack | §9:528-532; §11.3:893-894 (types) |
| Every root slot is a tagged value or immediate; dead frame slots are already cleared; stub frames are all-live; JIT frames hold no raw slots | §11.1:829-833; §11.2:851-857 |
| Invariant W: every loggable word and every `T_CONT` word is a value or reads as a fixnum | §6:375-380 |
| Objects are born young except through `alloc_old`; descriptors and continuations are written only by initializing stores; immortal objects reference nothing mortal | §10:750-756; §6:426-428; §13:1072-1073 |
| Ephemerons are allocated young only, never in LOS/NMS/immortal, and promotion is age-monotone | §9.5:592 |
| Every Rust store into an old object went through the funnel or a generated barrier sequence; bulk stores logged a range entry | §10:767-783 |
| No object owns a Rust `Drop` value; Rust resources are reached only through host ids and the finalization registry | §6:430-434 |
| Inputs to pacing are bytes and counts, never time | §15:1297-1301 |
| Tree-walker roots arrive through `pinned`, and tree-walker heaps never ask to move | §11.4:941-943 |

**Conformance suite** (§14:1211-1215), run on `MarkRegion<TestModel>`, `NullGc<TestModel>` and `ActiveGc`: allocation
and tracing; live-key ephemeron never broken, dead-key broken after a `complete` collection; a 16 K ephemeron chain
resolved with counted linear work; host-payload edges traced, an unreached payload queued; `drain` in id order,
`drain_all` at teardown, nothing finalized inside `collect`; epilogue order; with two `Mutator`s, both buffers retired
and both logs drained. **Stage 5a.**

**How future collectors plug in** (§14:1279-1286): a copying nursery (K4) is a `NurserySpace` composed into
`MarkRegion`, never armed, promoting in place; incremental marking (only on a latency goal) uses `barrier_mode` in slow
paths, and SATB would need `value_filter_bit = None` plus a JIT recompile; parallel stop-the-world marking (stage P) uses
per-heap GC workers, CAS marking on the metadata byte during GC only, work stealing and a sharded ephemeron fixpoint,
with roots on the collecting thread, sequential evacuation destinations and a `workers=4` lane byte-identical to one
worker. MMTk is not adopted (decision 14).

**Explicitly excluded** (§14:1288-1293): load/read barriers; conservative scanning; collection inside allocation or
barrier slow paths or reachable from a `Cx`; `dyn` on fast paths; code patching; process-wide GC singletons;
Java-style finalizers; Cranelift stack maps, frame walkers, `stack_switch`; raw slots in published frames; racing
parallel evacuation; more than one production collector.

**Today.** `Collector` is a one-method trait (`collect(&mut self, heap, roots)`) that must be non-moving
(`crates/patina-core/src/heap/gc.rs:199-213`); `GcRoots` carries a continuation-specific weak fixpoint
(`trace_weak_ids`, `sweep_weak`, `gc.rs:177-197`); the policy, mode table and safe-point rule live in `GcController`
(`gc.rs:322-410`).

---

## 9. The N-mutator rules (§18)

| # | Rule | PRD | Enforcement | Stage |
|---|---|---|---|---|
| N1 | **Carriers and green threads are separate.** The `Mutator` (carrier) owns the ABI block, allocation buffer, store buffer and remember-whole list, root-scope stack and `no_gc_depth`, handle table, safepoint state, `current_thread`; under M:1 there is one. A `GreenThread` owns its register stack and `ThreadGcState`, frames, re-entry fields, dynamic environment (as heap data) and scheduler links. A switch saves `reg_top` and calls `set_limit`. `allocs_since_gc`, `gc_threshold`, `gc_pending`, `gc_defer_depth` leave `Heap` | §18.1:1613-1619; rule 1 :1661 | L (two-mutator lane); R | 3 (`Mutator`); 9 (split) |
| N2 | **Heap words have one way in**: `HeapSlot`/`MetaByte` for every access, the funnel for every store; no `&`/`&mut` into heap words outlives one operation or crosses the embedding API | rule 2 :1662 | T | 3 |
| N3 | **No shared read-modify-write on a hot path**: no `Rc`, `RefCell` or `Drop` payloads in heap objects; each carrier's mutable data on its own 128-byte line | rule 3 :1663 | T (`Mutator` alignment asserted); L (CI check) | 3, 5 |
| N4 | **Publication** (under `threaded`): (a) a store that may make an object reachable by another carrier is ordered after that object's initializing stores, whatever the `BarrierKind`; (b) the mechanism is `GcAttrs.publication` (`FenceAtAllocation \| ReleaseOnHeapStore \| FenceOnHeapStore`), separate from the generational barrier; (c) skip only when the holder has not escaped since allocation; (d) a bulk range copy publishes with one store-store fence; (e) Rust loads heap references with Acquire; (f) only JIT code may rely on address dependencies; (g) prefer release/acquire, and list every standalone fence for loom and arm64 lanes | rule 4 :1664 | L (loom, arm64, conformance for `BarrierKind::None` under `threaded`) | 6 (mechanism chosen by the publication spike); after decision 7 |
| N5 | **Protocols are written for N**: request, acknowledge (at a poll or in a safe region), collect, release; posters write only atomics; safe regions entered with no unrooted value; a mutex that must block tries first, then deactivates | rule 5 :1665; §12:997-1003 | L (two-mutator, then seeded simulation lane) | 3 (protocol); 9 (lanes) |
| N6 | **One entry point per shared structure**: interning, namespaces, code installation; inline caches are one traced word; continuations refer to no carrier | rule 6 :1666 | R | 4b (namespace API), 4e, 6 |
| N7 | **Limits are per heap**: pacing sums every carrier's allocation; the store-buffer soft limit is divided among carriers; green-thread stacks share a per-heap address-space bound; carriers' native stacks have an explicit size; Rust recursion is depth-guarded | rule 7 :1667 | L (limits lane) | now ([#617]); 9 |
| N8 | **The tree-walker stays M:1 and `!Send`** | rule 8 :1668 | T | — |
| N9 | **Concurrency policy**: under `threaded`, Rust-implemented state synchronizes internally (ports, tables, registries, promise forcing, the source map); in the default build it is plain, behind one wrapper (addition C3). Scheme-built structures do not synchronize; users lock, as in Chez | rule 9 :1669 | R | 2 (addition C3's wrapper) |
| N10 | **The two-mutator lane stays green from stage 9** (`PATINA_GC_MUTATORS=2`, two `Mutator`s alternating deterministically on one OS thread), generalized into a seeded multi-carrier simulation | rule 10 :1670; §14:1275 | L | 9 |
| N11 | **Memory model** (stated at stage 3): no crash, tearing or uninitialized object; every read returns some written value; happens-before only through mutexes, condition variables, thread start and join | §18.2:1672-1673 | R | 3 |
| N12 | **Obligations of the `threaded` build before N > 1**: atomic sub-word stores; acquire loads in Rust; publication per rule 4; safe regions only with no unrooted value, `read` lexing into Rust-owned tokens first; a dual-mapped code reservation before OS-thread carriers. The `threaded` feature exists from stage 3 with accessor bodies only (linted by `--all-features` clippy); no lane runs it until 9; enabling it needs decision 7 | §18.1:1640-1648 | L (clippy `--all-features`) | 3 (feature); 9 (lane) |
| N13 | **SRFI 18 root rule** (decision 23, proposed): the scheduler is a root provider reporting every started, non-terminated thread; a thread blocked for ever on objects nothing else reaches stays rooted until it terminates. Every `GreenThread` is registered `FinalKind::Thread`; termination queues it at once, so its stack returns at the next poll without a collection | §18.1:1626-1627 | L (`blocked-threads` probe scored against Gambit and chibi; thread-lifetime tests on both backends) | 9 |
| N14 | **Reserved ABI words**: `fenced_ap` (addition C1's standalone-fence variant), `quiesce_epoch` (deferred code-memory reuse until every carrier passes a quiescent point), `handshake` (stop-the-world handshake); `safepoint_state` (`Running`/`AtSafepoint`/`InSafeRegion`) | §14:1233-1239; §18.5:1753 | L (offsets asserted) | 3 (reserved); 6 (frozen) |
| N15 | **`Send`/`Sync` timeline**: `InterruptHandle` and `PrimitiveRegistry` at 3, `CodeBody` at 4e, `GreenThread` at 9; `HeapShared: Sync` waits for decision 7; a threaded VM heap needs its own `Send + Sync` payload trait-object type, because tree-walker payloads stay `!Send` | §18.6:1770; §14:1183 | T (assertions as each type first satisfies them) | 3, 4e, 9 |
| N16 | **Start conditions for shared-memory parallelism** (decision 7, proposed "not now"): a named workload needs it and isolates cannot serve; stages 0–9 and SRFI 18 on M:1 shipped with switch, cross-thread continuation and terminate matrix rows green on both backends and the simulation lane green; the stage-5 tax re-measurement within 5% geomean; a re-estimated planning figure. First-step gates: threaded build with one carrier byte-identical, ≤ 5% geomean time, ≤ 5% committed bytes | §18.7:1789-1799 | K (stop rules :1812-1813) | after 9 |

---

## 10. What the JIT and concurrency lean on, and what they change

**A Cranelift JIT** consumes the contract rather than reshaping it. It relies on: allocation never collecting (C2),
so allocation sites need no publish; values living only in VM frames at suspension points (J1), so no stack maps, frame
walker or unwinder; `(code, pc)` as resume truth (J5), so continuations, preemption and deoptimization never resume a
native frame; `GcAttrs` and the `Mutator` block as data (J7, J8), so evacuation (stage 8) is invisible to emitted code, while
generations (stage 7) need bodies compiled with the `GranuleLog` barrier (§10:758-760; see `followup/contract/ANSWER.md` §4, A1); the four embedded-address cases (J6), so code is never patched
at GC. What it changes in today's code is mostly prerequisite representation work the redesign does anyway: a
non-relocating register stack and frame header (4d), heap continuations (4e), global cells (4b, then C for one-load
globals), raw tagged addresses (5), and removing the per-instruction poll (3). Its costs and escape hatches are K6
(flonums), K7 (call and poll shape), K11 (fragments versus native call/ret, > 8%) and K12 (tier 2's tagged-only rule,
> 15%), all decided at or after the stage-6 spike. Code memory (J11) is its own allocator under a cap.

**Concurrency** splits into three things the PRD treats differently. (1) **SRFI 18 green threads (M:1, decided)** need
the carrier/thread split (N1), the scheduler root rule (N13), per-thread watermarks in `MutatorSet`, and
`FinalKind::Thread`; stage 9 builds them, and the collector still sees one mutator. (2) **Parallel stop-the-world
marking (stage P)** needs GC worker threads, not mutator threads, and lives entirely inside `collect`: CAS marking on
the metadata byte during GC only, roots on the collecting thread, sequential evacuation destinations, byte-identical
output. It does not depend on decision 7. (3) **Shared-memory mutator parallelism (proposed "not now")** is where the
contract's N-ready interfaces pay: per-mutator buffers, `MetaByte`/`HeapSlot` accessors that become atomics under
`threaded`, the Dekker-pair poll, safe regions, publication (rule 4) and the reserved ABI words. What remains open is
listed in §18.3 (38–77 engineer-weeks after the redesign, about 2–9 engineer-years with overrun and recovery), decisions
7a and 7b, the `Send + Sync` payload type, and Rust-side tables (rule 9).

---

## 11. Gaps and tensions found while extracting

1. **`NoGcScope` is described as a type and runs as a counter.** §11.3:891-893 says nested entries beneath a `Cx` run
   "`NoGcScope` by type"; §12:1037 and §18.1:1614 gate the poll slow path on `no_gc_depth == 0`, a runtime counter on
   the `Mutator`. Both exist; the PRD does not say which check catches a missed scope.
2. **A host primitive's declared helper class is trusted.** The call-graph test (§16:1347) covers the runtime's helper
   table; `PrimSpec.class` (§11.5:955) is an embedder declaration with no stated check. Under M:1 a `Leaf` host
   primitive that blocks only stalls; under N carriers it would stall stop-the-world (rule 5).
3. **Stages that §19 does not name:** `#![forbid(unsafe_code)]` in the safe crates (§11.3:912), the `return_into`
   choke point (§11.1:839-843, needed by the watermark at 7 but describing writes that exist from 4d), and the
   frame-invariant verifier's JIT-frame coverage.
4. **Cross-carrier visibility of `desc.entry`.** Tier-up writes `desc.entry` as data (§11.2:872; §13:1078); rule 4
   covers heap stores that publish objects, and `quiesce_epoch` covers code reuse, but the ordering of a code-pointer
   write seen by another carrier is not stated.
5. **The "what a collector may assume" list is scattered.** §14 states obligations of a collector in one table but its
   assumptions only implicitly; section 8 above gathers them. A second collector (`NurserySpace`, an MMTk adapter) would
   need that list.
6. **Decisions 3, 7a, 7b and 23 shape parts of this contract and are open or proposed.** Variant C's rebinding answers
   (decision 3) change which JIT globals are one load; 7a/7b are open until the parallelism trigger.

[#423]: https://github.com/avalonalex/patina/issues/423
[#601]: https://github.com/avalonalex/patina/pull/601
[#604]: https://github.com/avalonalex/patina/issues/604
[#605]: https://github.com/avalonalex/patina/issues/605
[#609]: https://github.com/avalonalex/patina/issues/609
[#610]: https://github.com/avalonalex/patina/issues/610
[#614]: https://github.com/avalonalex/patina/issues/614
[#617]: https://github.com/avalonalex/patina/issues/617
[#618]: https://github.com/avalonalex/patina/issues/618
