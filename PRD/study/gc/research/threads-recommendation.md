# Threading model for the GC redesign: judge's recommendation

Date: 2026-09-30. Repo state: `main` at `28a94f8`. Inputs: `threads-prior-art.md`,
`threads-patina-cost.md`, the three advocate briefs (green threads M:1;
shared-heap OS threads; isolates with green threads inside), and
`../understand/jit-readiness.md` for the GC/JIT contract. Effort and overhead
figures come from those reports. They are estimates, not measurements, except
the isolate spawn proxy (11 ms, 12 MB) in the cost report.

## 0. Verdict

**Build one mutator now, and design the GC's interfaces for N mutators sharing
one heap.**

- **Ship SRFI 18 as green threads on one OS thread (M:1), VM first.** This is
  advocate 0's model, and the first stage of advocate 2's.
- **The GC's design target is N mutators over one shared heap with
  stop-the-world parallel collection.** This is the M:N shape used by Gambit
  SMP, OCaml 5, Racket 9 parallel threads and Java Loom. It is advocate 1's
  destination, reached through M:N rather than 1:1, and not built now.
- **Isolates are optional and GC-neutral.** They can ship in 3–5 weeks
  whenever a share-nothing parallel workload appears. They are not a reason to
  build single-mutator assumptions into the collector or the JIT.

The rule for the redesign: **every per-thread concept gets an explicit owner
object, and every protocol is written for "all mutators", even though only one
exists.** In the single-mutator build, the multi-mutator paths compile to
plain code: no atomics on the hot path, no fences, no handshake latency.

| | Ships now | GC must support now | Kept open cheaply | Not planned |
|---|---|---|---|---|
| Green threads (M:1) | yes | many stacks as roots; deep-bound dynamic state | — | — |
| Shared heap, N carriers (M:N, or 1:1 as its degenerate case) | no | interfaces shaped for N (§2) | yes (§3.1) | — |
| Isolates per OS thread | when needed | nothing | yes (§3.2) | a GC that relies on one mutator per heap forever |
| Single-mutator GC shortcuts | — | — | — | allocation state on `Heap`, shared barrier logs, bit-packed RMW cards, relocating arenas, new `Rc<RefCell>` payloads |

## 1. Reasoning

### 1.1 What SRFI 18 requires

Checked against the SRFI 18 text:

- It allows parallelism but does not require it ("more than one running thread
  on a multiprocessor machine").
- Invoking a continuation captured by another thread is "well defined".
- A new thread inherits the dynamic environment, gets the initial exception
  handler, and starts with an empty dynamic-wind stack.
- Neither a scheduler switch nor `thread-terminate!` runs dynamic-wind thunks.

Every one of these is met by green threads over one shared heap, as Gambit
(the reference implementation) shows. Isolates cannot meet them, because
SRFI 18 assumes shared mutable state. So the compliance requirement fixes a
**shared heap**, and does not fix **parallelism**.

### 1.2 Why not build shared-heap OS threads now (against advocate 1 as a build plan)

1. **Cost and risk against benefit nobody has asked for.**
   - Cost: 9–18 engineer-months by the cost report, or 5–9 months by advocate
     1's estimate once the §2 items exist. Green threads cost 3–4 months in
     full.
   - Risk: it touches every crate (588 `Rc<`, 115 `RefCell<`, 787 heap-borrow
     sites).
   - Benefit: the request asks for SRFI 18, and it does not ask for multicore
     speedup over shared mutable data.
2. **Throughput comes first.** A threaded build pays per operation:
   - an estimated 5–15% on the VM (unmeasured; OCaml 5 pays 3.5%, CPython
     about 10%);
   - publication fences on arm64, which is the development platform (this
     machine is `arm64`).

   A single-mutator build with N-ready interfaces pays none of this.
3. **This codebase depends on determinism.** It relies on byte-identical GC
   differential lanes and on oracle-scored matrices for dynamic-state
   transfers, where its defects historically cluster (#157–#163). Real
   parallelism brings non-deterministic failures into exactly that area.
4. **Waiting costs little.** Advocate 1's strongest point is that A followed
   by B costs more than B now. That holds only if A builds in single-mutator
   assumptions. Under the rules in §2, all of A's work is reused by M:N:
   - the scheduler;
   - the thread, mutex and condition-variable objects;
   - deep-bound parameters;
   - per-thread root providers;
   - the poll word.

   Most of B's remaining premium is moving `Rc` graphs into the traced heap,
   which the JIT contract already requires for its own reasons:
   - heap environments and global cells (R5);
   - one-field boxes (R3);
   - closures with inline free variables (R4);
   - continuations as heap objects (R30).

   That premium therefore shrinks as the GC and JIT work lands, whether or not
   B is ever chosen.

### 1.3 Why the target is a shared heap, not permanent isolation (against advocate 2's GC premise)

Advocate 2's GC argument is that one mutator per heap, forever, allows plain
mark bits, no fences and no handshakes. Under M:1, Patina gets all of those
simplifications today. The N-ready choices in §2 cost essentially nothing with
one mutator:

- a mutator context;
- per-mutator logs;
- byte cards;
- stable blocks;
- a protocol written for N.

Ruling them out would turn any later move to a shared heap into a rewrite. The
precedents also point toward shared memory:

- Racket CS places share Chez's heap.
- Racket 9 added shared-heap parallel threads.
- OCaml chose shared-heap domains.
- Ruby Ractors still share one GC.

Isolates stay attractive as a *feature*: zero GC work and independent pauses.
They do not justify narrowing the GC.

### 1.4 Why M:N and not 1:1 as the eventual scheduler, and why the GC hardly cares

To the collector, a 1:1 thread is just M:N with one green thread per carrier.
The GC sees N **carriers** (mutators) in both cases. The choice of scheduler
can therefore wait. M:N is the better default when the time comes:

- it reuses the green scheduler unchanged;
- thread count is not tied to OS threads;
- `thread-terminate!` is a bit in the poll word;
- a carrier blocked in a system call deactivates while the others keep
  running.

The prior-art report reaches the same conclusion (§13.1).

### 1.5 Corrections to the advocate briefs

- **The mutator is the carrier, not the green thread.** In Go (P/mcache), Loom
  (carrier TLAB) and OCaml (domain minor heap), the allocation buffer, barrier
  log, poll word and safepoint state belong to the OS-level carrier, and green
  threads are execution states it runs. Under M:1 that means exactly **one**
  `Mutator` and many `GreenThread`s. The JIT's context register points at the
  `Mutator`, which points at the current thread. Getting this split wrong
  would put a TLAB in each green thread, which wastes memory with thousands of
  threads and is wrong for M:N.
- **Stage 5 Priority 2 does not remove the "no switch across a Rust
  re-entry" limit.** This corrects advocate 0, objection 4. Precise rooting
  lets the GC run *inside* a nested Rust loop. A green thread still cannot be
  *suspended* while its state is held in Rust frames on the native stack,
  because those frames are LIFO on the one OS stack. Only moving the residual
  re-entry paths into machine frames removes the limit, continuing the
  #471–#478 direction. Those paths are library bodies, `Step::Eval` import
  loading, and the remaining `apply_proc` fallbacks. Until then, a blocking
  SRFI 18 operation inside one of them must raise an error. Precise rooting is
  still required before N > 1, and still valuable for nested-loop memory
  bounds and for JIT helpers that allocate.
- **A timer-driven preemption flag breaks the deterministic lanes.**
  Preemption must have a deterministic mode. §2 item 7 uses a tick counter
  for this, as Chez's `%trap` does.

## 2. What the GC must include now

These items are required by the chosen path: green threads now, a shared heap
with N carriers later, and the Cranelift JIT. Several are also items in the
GC/JIT contract (`jit-readiness.md` §5, items R1–R35).

1. **A `Mutator` (carrier) context, `#[repr(C)]`, which is also the JIT's single
   ABI object (R35).** It owns:
   - the allocation cursor and limit;
   - the allocation counter and threshold;
   - the poll counter and the atomic request word;
   - the barrier log or card-table base;
   - the root stack (item 6);
   - the safepoint state (Running, AtSafepoint or InSafeRegion);
   - a pointer to the current green thread.

   `allocs_since_gc`, `gc_threshold`, `gc_pending` and `gc_defer_depth` move
   off `Heap` (`heap/mod.rs:379–417`). Allocation takes the mutator. Runtime
   state does not go in `thread_local!`.
2. **A green-thread context separate from the carrier.**
   - It contains `ExecutionState`, the re-entry and escape fields
     (`vm_state.rs:54–106,188`) and the dynamic environment: parameterization,
     the three current ports, and the handler stack.
   - Split `impl GcRoots for VmState` into machine-wide roots (code store,
     globals, continuation tables) and one root provider per thread.
   - Each thread provider carries a "ran since last GC" bit, so a future minor
     GC can skip stacks that have not run.
   - Thread, Mutex, ConditionVariable and Time are heap objects. The thread
     table is a root.
3. **A block-structured, stable-address heap.**
   - Fixed-size blocks come from a global block pool, refilled under a lock
     that is uncontended today.
   - Allocation buffers are bump runs inside blocks.
   - Sweep (lazy, per block) produces free runs, or a nursery evacuates.
   - Addresses never move because an arena grew. This is also R2/R24 for
     the JIT.
   - Mark metadata lives per block, behind one `mark(tv) -> bool`.
4. **One complete store funnel with a barrier hook (R11/R12).**
   - Every Rust mutation path goes through it: pairs, vectors, cells, records,
     parameters, promises, ephemerons, hashtables and environment slots.
   - Remove `vector_slice_mut` or make it barrier-aware.
   - Barrier state must tolerate races by construction. Use **byte** card
     marks, which are idempotent, or **per-mutator** logs (an SSB, or Chez's
     log at the top of the TLAB). Never use bit-packed read-modify-write
     cards or a log shared between mutators.
   - Initializing stores need no barrier.
   - An object becomes visible to the rest of the heap only through the
     funnel. That is the single place where a threaded build would add a
     release fence.
5. **A safepoint protocol written for N mutators.**
   - The sequence is: request; each mutator acknowledges, either at a poll or
     by being in a safe region; collect; release. With one mutator the
     acknowledgement is immediate.
   - Provide `enter_safe_region`/`leave_safe_region` now and wrap blocking I/O
     (and the future FFI) in them, even though they do nothing today. This is
     Chez's `Sdeactivate_thread` and OCaml's blocking section.
   - A mutex acquire that has to block uses Chez's order: trylock first, then
     deactivate.
6. **A precise-rooting API on the `Mutator`.**
   - A root stack, or handle scopes, that all new runtime code uses.
   - Stage 5 Priority 2 converts the residual re-entry paths.
   - After that, `GcDeferGuard` deferral stays only for the tree-walker.
   - Mandatory before N > 1; also needed for nested-loop collection and for
     JIT helpers that may allocate.
7. **One poll for everything, deterministic in test mode.**
   - Fast path: a private tick counter, decremented at loop back-edges and
     function entries. Interpreted code keeps today's cheap safe point. This
     follows Chez's `%trap`.
   - Slow path: read an `AtomicU32` request word with bits for GC, preempt,
     signal, debugger, terminate and handshake.
   - The quantum is deterministic, so the GC differential lanes stay
     byte-identical. Cross-thread requests are seen within one quantum.
   - The JIT emits exactly one poll sequence.
8. **Continuations are heap objects that refer to no carrier state (R30).**
   - A continuation captures the thread's dynamic environment, so invoking it
     from another thread works as follows: install the frames, rewind winders
     against *this* thread's wind list, then reinstate the captured
     parameterization.
   - Register stacks are per thread and do not relocate (R24).
   - Frame headers carry code, pc and size, so a stack segment can be walked
     without the `frames` side vector (R31). That keeps open Chez-style
     segments and Loom-style "suspended stacks are heap objects", which
     matter once there are thousands of threads.
9. **The dynamic state is deep-bound, and held as heap data.**
   - Each thread holds a parameterization, which maps each parameter to a
     cell. This replaces the shallow `%parameterize-swap!` and the process-wide
     `Parameter{values}` stack.
   - The current ports move out of `thread_local!` (`io/ports.rs:41–44`) into
     the dynamic environment, with a cache in the thread context. That also
     fixes today's latent sharing between two interpreters on one OS thread.
   - SRFI 226 settles the remaining choices: mutating a plain parameter is
     shared, and `make-thread-parameter` creates an inheritable thread-local
     value.
10. **New runtime data goes in the traced heap.** That means:
    - records, parameters, promises, and the new thread, mutex and
      condition-variable objects;
    - environments and global cells, as the JIT needs them.

    No new `Rc`/`RefCell` payloads go into `HeapObjectData`.
11. **Code is split from its heap constants (R22/R23).**
    - Instructions and register maps are immutable.
    - Constants are per-heap and traced.
    - Machine code never embeds heap references as immediates.
    - Code liveness is traced, not counted through sweep.
12. **Object headers keep GC bits apart from bits the mutator writes.**
    - The mutator never does a read-modify-write on GC-owned header bits.
    - A forwarding state can be installed by CAS, which later parallel
      evacuation needs. The threading model does not forbid a moving nursery.

## 3. Cheap choices that keep the other options open

### 3.1 Toward N carriers over the shared heap

- **Slot access goes through one accessor type** (`Slot::load`/`store`) and an
  audited unsafe heap API in place of `RefCell`. The accessor is plain today.
  A `threaded` feature turns it into relaxed `AtomicU64`, which is still a
  plain `ldr`/`mov`. Rust treats a racy plain access as undefined behaviour.
- **Reserve a `threaded` cargo feature** for the publication fences. They sit
  in the funnel and in the JIT's allocation sequence, and compile to nothing
  otherwise, as in Chez's split builds.
- **`mark(tv)` is plain now** and becomes `fetch_or` for parallel marking. Mark
  work lists are plain `Vec`s, so work stealing can be added later.
- **Interning goes through one function**, so it can become a sharded
  concurrent interner. Symbol names live in the heap, or in `Arc<str>` held
  only by the table.
- **Inline caches fit in one word** (32-bit environment ids, `code_object.rs:196–201`),
  so they can be updated atomically.
- **The code store and continuation tables stay machine-wide.** Under M:1 they
  already are, since every thread shares one `VmState`. They must never move
  into the per-thread struct.
- **A two-mutator test mode on one OS thread.** Two `Mutator`s alternate
  deterministically, exercising buffer retire and refill, per-mutator log
  flushes and acknowledgements from N mutators. That keeps the N paths alive
  before any OS threads exist.
- **Incremental marking later:** add per-mutator SATB buffers behind the
  funnel hook, and darken a stack before resuming it, as OCaml does for
  fibers.

### 3.2 Toward isolates (no GC work)

- Make the leftovers process-global, or define their semantics, when isolates
  arrive:
  - stdin lookahead and `OUTPUT_FILES` (`port.rs:157–179`);
  - the exit status;
  - `exit` called inside an isolate.
- Item 11 (code split from constants) is the prerequisite for a code cache
  shared between isolates.
- Share bytevectors and flvectors through pointer-free `Arc<[u8]>` payloads.

### 3.3 For the JIT under any model

- Use a pinned context register, never OS TLS.
- Scheme frames live in the per-thread register stack, and native frames are
  transient. Calls trampoline through the driver, as `jit-readiness.md` §3
  option A describes. That makes a green-thread switch O(1) and keeps
  continuations as snapshots.
- No in-place code patching: inline caches are indirect through data. Install
  code through one function, which is where an instruction-cache flush and a
  cross-thread handshake would go. Confine macOS `MAP_JIT` write toggling to
  that function.

## 4. Risks

| Risk | Why it matters | Mitigation |
|---|---|---|
| **Single-mutator shortcuts creep in** (the main risk) | Each shortcut makes the step to N a rewrite, and no test catches it while N = 1 | Record §2 as rules in `docs/GC_DESIGN.md`; run the two-mutator test mode in CI; review new `HeapObjectData` payloads and `thread_local!` |
| N-ready rules cost single-thread throughput | Throughput comes first | Measure each rule with the project's interleaved A/B method; put any rule costing more than about 1% behind the `threaded` feature |
| Bugs in dynamic-state transfers (switch, start, cross-thread `k`) | This is where defects have historically arrived (#157–#163); deep-bound `parameterize` sits on a hot path | New rows in `control_flow_matrix.rs` and VM_RUNTIME §5.6, scored against Gambit (the reference), chibi and Gauche. Do not use chibi as the oracle for parameter inheritance, because it resets parameters in new threads |
| No switch across Rust re-entry | A blocking SRFI 18 call inside a library body, an import or a callback must raise | Document it; continue moving re-entry into machine frames. Precise rooting does not fix this (§1.5) |
| Blocking I/O stalls every green thread | One `read-line` freezes the program | Document it in the first release; then a helper pool that posts `Vec<u8>` back through the VFS (2–4 weeks) or a scheduler that polls file descriptors |
| Demand for shared-memory parallelism arrives | M:1 never uses more than one core | The M:N program becomes the next project. Its cost after §2 is mainly the slot-access switch, the remaining `Rc` graphs, fences, parallel marking, and Miri/loom/TSan test lanes; estimated 4–9 months, unmeasured |
| Pause times grow with many threads | Bounded pauses are a goal, and a stop-the-world mark is O(sum of stacks) | "Ran since last GC" bits or stack watermarks for minor GCs; suspended stacks as heap objects later |
| The tree-walker falls behind | It keeps values in Rust locals and `Rc` continuations | Green threads only, with a thread being a `StepResult` (+2–3 weeks); deferral stays its rooting mechanism; it never runs as one of N carriers |
| Effort figures are wrong | All of them are estimates by analogy | Treat the §2 items as the GC redesign's scope, and re-estimate SRFI 18 after the `Mutator`/thread split lands |

## 5. Questions for the owner

1. **Is shared-memory parallelism under SRFI 18 a goal?** This is the
   deciding question. If yes, M:N is the scheduled destination and §3.1 moves
   from "keep open" to "plan". If no, M:1 plus optional isolates is enough. §2
   is the same either way.
2. May invoking another thread's continuation be an error, as in Guile? The
   recommendation assumes it is supported, as SRFI 18 and Gambit require,
   because under M:1 it costs nothing.
3. Is it acceptable that blocking I/O stalls all threads in the first SRFI 18
   release?

## 6. Sequencing

1. GC redesign: §2 items 1–5 and 7 as structure; item 6 as Stage 5
   Priority 2; items 10–12 with the JIT-driven representation work.
2. Deep-bound parameterization and ports (item 9), before any thread exists.
3. SRFI 18 on the VM (8–12 weeks), then the tree-walker (+2–3), then
   non-blocking I/O (+2–4).
4. Isolates when a parallel workload appears (3–5 weeks).
5. N carriers only as a separately approved program, after question 1.

## Sources

- `threads-prior-art.md`; `threads-patina-cost.md`; `../understand/jit-readiness.md`
  (all in `PRD/study/gc/`).
- SRFI 18: https://srfi.schemers.org/srfi-18/srfi-18.html. Checked for:
  multiprocessor wording, continuations from another thread, dynamic
  environment inheritance, and whether switches or termination run winders.
- Repo, spot-checked at `28a94f8`:
  - `lib/scheme/base/parameters.scm:1–8,36–49` (shallow binding, threading TODO);
  - `crates/patina-vm/src/runtime/vm_state.rs:218,1151–1154,1204–1208,1287–1296`
    (`gc_pending`, `is_outermost`, `maybe_collect`);
  - `crates/patina-core/src/heap/mod.rs:51,304–316,379–417` (`SharedHeap`, `Vec`
    arenas, allocation and GC state on `Heap`);
  - `crates/patina-vm/src/runtime/execution_state.rs:17–23,239–261`
    (`ExecutionState`, clone-based capture);
  - `crates/patina-vm/src/runtime/control.rs:23–30` (rooting contract for a
    compiled driver);
  - `PRD/ARCHIVE/GC_STAGE5_PRD.md:66–76,117–122` (Priority 2; concurrency
    listed as a non-goal).
