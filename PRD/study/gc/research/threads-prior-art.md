# Threads in Scheme and dynamic-language runtimes: prior art for Patina

Research note, 2026-09-30. Scope: how comparable runtimes implement threads,
which SRFI 18 semantics they deliver, what their garbage collectors had to do
about it, and what a JIT has to emit as a result. Local sources are cited as
`repo:path:line` under `~/Project/reference/`; web sources are listed at the
end. Nothing in the Patina repository was modified.

## 0. Bottom line

Five models are in use:

1. **Green threads on one OS thread.** Used by chibi, Chicken, Gambit
   (default), Larceny's tasking library, and Racket's coroutine threads.
2. **1:1 OS threads over one shared heap.** Used by Chez, Gauche, Guile,
   CPython 3.13+ free-threaded builds, and Java platform threads.
3. **M:N: green threads multiplexed onto N OS threads that share a heap.**
   Used by Gambit `--enable-smp`, Racket 9 parallel threads, OCaml 5 domains
   with effect-based fibers, and Java virtual threads.
4. **Isolated heaps with message passing.** Used by Racket 3m places, JS
   workers, and Ruby Ractors (logically isolated).
5. **Futures.** Racket futures are parallel only until they reach an "unsafe"
   operation; this is a degenerate case of model 3.

SRFI 18 assumes one shared object space. Gambit's manual describes a thread as
"a virtual processor which shares object space with all other threads".
Model 4 therefore cannot implement SRFI 18. Its value is that the collector
stays simple.

No runtime that went parallel avoided the following machinery:

- an explicit per-thread **context** in a register (Chez `tc`, OCaml
  `Caml_state`, Gambit `___ps`, the HotSpot thread register);
- thread-local **bump allocation**;
- a **poll word** that serves preemption, GC rendezvous and signals together;
- a **deactivate/attach protocol** around blocking calls;
- **per-thread barrier logs** instead of atomics;
- **store fences for publication** on ARM;
- a **stop-the-world rendezvous**.

Systems that later added concurrent or incremental marking (OCaml 5, HotSpot
Loom) also had to treat thread stacks and continuations as heap objects that
must be darkened before they are resumed.

Measured single-thread costs of making a runtime parallel:

| Runtime | Cost |
|---|---|
| OCaml 5 | 3.5% (stop-the-world parallel minor GC) or 4.9% (concurrent minor GC with a read barrier) |
| Racket parallel threads | up to 6–8% |
| CPython free-threaded | about 10% on Linux and Windows (3% on macOS); memory up 15–20% |
| Ruby per-Ractor GC proposal | 2–4% |
| HotSpot thread-local handshakes | under 1% |

**Recommendation for Patina (§6).** Make the representation decisions for M:N
now: a per-mutator context, deep-bound parameterization, self-contained
continuations, a TLAB, per-mutator barrier logs, and one poll word. Ship SRFI 18
first as green threads on one OS thread; this requires no GC change beyond
having multiple stacks as roots. Keep the step to N carriers with a
stop-the-world parallel GC open. Do not convert `Rc` to `Arc`; move the runtime
graphs into the GC heap instead.

## 1. Chez Scheme: 1:1 native threads, one shared heap, parallel stop-the-world GC

### Model
- `fork-thread` creates a pthread (`ChezScheme:c/thread.c:332`).
- Mutexes are recursive pthread mutexes with an owner and a count
  (`thread.c:365-445`).
- Condition variables, `thread-join` and `get-initial-thread` are provided.
- There is **no thread termination primitive**, so `thread-terminate!` cannot
  be implemented natively.
- Most primitives are documented as thread-safe, including `set-car!` and
  `vector-set!`. Concurrent `putprop` on the same symbol, and buffered ports,
  are not (`csug/threads.stex:12-40`, `:668-676`).

### Per-thread state
The thread context `tc` is malloc'd and cloned from the parent
(`thread.c:58-190`). It holds:
- registers;
- `AP`/`EAP`, the allocation pointer and allocation limit;
- the Scheme stack;
- a `PARAMETERS` vector of *thread parameters*. These are copied by value from
  the parent; changes are not seen by other threads
  (`csug/threads.stex:631-665`);
- `WINDERS`, reset to `()` in the child;
- the handler stack;
- the timer ticks, `DISABLECOUNT` and `SOMETHINGPENDING`;
- a signal queue;
- guardian entries;
- a `thread_gc` holding per-generation and per-space bump areas, so even
  promotion during GC allocates thread-locally.

The child's dynamic-wind list is empty, and thread parameters are inherited by
value, which matches SRFI 18's "inherits the dynamic environment".
`with-mutex` releases the mutex on a non-local exit and reacquires it when a
continuation re-enters (`csug/threads.stex:220-236`). Continuations are heap
objects that can be resumed on another OS thread: Racket CS relies on this
(§2).

### Allocation
Allocation is an inline bump from `AP` towards `EAP`, one segment at a time.
When a thread runs out, `S_get_more_room` takes the global `alloc_mutex` for a
new segment (`c/alloc.c:330-355`, `:500`).

### Write barrier
The barrier is unusually cheap and lock-free. On a store of a non-fixnum,
compiled code writes the *address* into a log kept at the **top of the
thread's own allocation area**: `EAP` grows downward while `AP` grows upward.
When the two meet, `S_scan_remembered_set` drains the log into card dirty
bytes under the alloc mutex (`c/alloc.c:431-491`, `s/cpprim.ss:665-720`). As a
result the barrier needs no atomics and shares the TLAB's overflow check.

### Memory ordering
Threaded builds on `arm32/arm64/riscv64/loongarch64/ppc32` add a store–store
fence before a remembered store (`s/cpprim.ss:652-664`). On Apple M1 the store
is duplicated so that the fixnum test sits outside the fence (`:707-716`).

### Safepoints
Each procedure entry and loop header runs `trap-check`: decrement the `%trap`
register and branch to the `event` handler when it reaches zero
(`s/cpnanopass.ss:3997-4009`). The default is 1000 ticks
(`s/cmacros.ss:2119`).

This one counter serves engines (timer preemption), keyboard interrupts,
signals and GC requests. The event handler checks `something-pending`
(`s/library.ss:1200-1240`). An allocation that crosses `collect-trip-bytes`
calls `S_fire_collector`, which sets `collect-request-pending` and every
thread's `SOMETHINGPENDING` (`c/schsig.c:574-594`). Each thread notices within
one tick period.

### Rendezvous
`$collect-rendezvous` (`s/7.ss:1253-1297`) works as follows:
1. Each *active* thread waits on `$collect-cond` under `$tc-mutex`, which
   deactivates it.
2. When the active count reaches 1, the remaining thread runs
   `collect-request-handler`. It prefers the initial thread
   (`csug/smgmt.stex:210-226`).

"Deactivated" is the central idea of the protocol:
- `condition-wait` deactivates the caller (`c/thread.c:513-575`).
- `mutex-acquire` first tries an uncontended `trylock`, and deactivates only if
  it has to block (`c/prim5.c:1603-1621`).
- Foreign calls declared `__collect_safe` deactivate for their duration. They
  must not pass collectable memory unless it is locked
  (`csug/foreign.stex:264-275`).
- `Sactivate_thread`/`Sdeactivate_thread` are exposed to C.

A deactivated thread's stack is still a root, but it does not have to reach a
safepoint.

### Parallel GC (Chez 10)
- Collection becomes parallel automatically when several Scheme threads are
  active (`release_notes.stex:516-523`). It is "only parallel with itself, not
  the main program" (Racket blog).
- The collector is a hybrid mark/copy design.
- Parallel sweeping avoids atomics on object headers by **segment
  ownership**. A segment belongs to the thread that allocated it; a sweeper
  that meets a reference into a remote segment sends the referencing object to
  the owning sweeper to re-sweep (`c/gc.c:120-190`).
- `thread-preserve-ownership!` is a hint that a thread's allocations are worth
  tracking.
- GC parallelism therefore follows allocation locality.

### JIT and native code consequences
- `tc` lives in a register.
- Allocation, `trap-check` and the remember barrier are all inline.
- Frames carry live-pointer masks, so stack scanning is precise.
- The manual warns that code created by one thread should be run only by that
  thread and its descendants, because instruction and data caches may not be
  synchronized across processors (`csug/threads.stex:69-75`). A JIT has to
  solve this explicitly.

### Single-thread cost
Chez ships separate threaded and non-threaded builds. The threaded build pays
for:
- `tc` indirection;
- fences on ARM only;
- the tc-mutex fast path.

## 2. Racket CS: coroutine threads on Chez engines; futures, places, and (9.0) parallel threads

### Coroutine threads
A Racket thread is a coroutine within a place. It runs inside an *engine*: a
continuation run for a number of ticks and then interrupted through Chez's
timer, the same `trap` counter described above
(`racket/src/thread/README.txt:1-60`, `cs/rumble/engine.ss`).

### Synchronization and dynamic state
- Synchronization uses `start-atomic`/`end-atomic`, which prevent coroutine
  swaps.
- The dynamic state is **deep-bound**. The parameterization is a continuation
  mark, and the parameterization maps each parameter to a thread cell.
- Moving a continuation to another OS thread therefore carries its dynamic
  environment along with it.

### Futures
A future is a continuation run on another Chez OS thread. It blocks
permanently at any atomic-mode or continuation-sensitive operation, unless a
prompt shallower than the future's start delimits that operation
(`thread/README.txt:66-115`).

### Parallel threads (Racket 9.0, Nov 2025)
- Created with `(thread #:pool 'own)`.
- Each one is a coroutine thread paired with a future. When the future blocks,
  its continuation migrates to the coroutine thread; at `end-atomic` it moves
  back (`thread/README.txt:117-150`).
- Atomic mode in a parallel thread does not stop other parallel threads.
- The cost is "up to 6–8%" for programs that do not use parallel threads,
  mostly in mutable hash tables and ports.
- Scaling is limited by coarse I/O locks (Racket blog).

### Places
A place is "effectively a separate instance of the Racket virtual machine" in
the same process. Places communicate through place channels and may share
only `shared-bytes`, `shared-flvector` and `shared-fxvector`.
- In **CS**, places share the Chez heap and collect "in parallel".
- In **3m**, they collect "independently". A shared page table can become the
  bottleneck at about 8–16 places (`reference/places.scrbl:26-46`).

### GC handler
The GC handler is "called in an unspecified thread with all other threads
paused" (`cs/rumble/memory.ss:7-12`).

### Rules worth copying
- **Chez deactivates a thread blocked on a mutex.** If a lock also guards
  state used by a GC callback, interrupts must be disabled before taking it,
  to avoid deadlock (`cs/README.txt:415-420`).
- Racket keeps three distinct atomicity levels:
  - OS-thread mutex or spinlock;
  - Racket atomic mode;
  - engine-uninterrupted.

## 3. Gambit: green threads in Scheme; experimental SMP (M:N) with a parallel GC

### Model and SRFI 18 coverage
- Marc Feeley wrote SRFI 18, and Gambit's thread system is its reference
  semantics.
- Threads are "managed entirely by Gambit's runtime and are not related to the
  host operating system's threads" (`gambit:doc/gambit.txi:9839-9847`).
- They support priorities, priority inheritance, quanta, `thread-interrupt!`,
  timeouts and full SRFI 18 exceptions.
- A new thread "inherits the dynamic environment from the current thread"
  (`gambit.txi:10169`).
- Invoking another thread's continuation is "well defined" (`gambit.txi:11148-11151`).
- The scheduler is about 8.9k lines of Scheme over first-class continuations
  (`lib/_thread.scm`).

### SMP
`--enable-smp` (default off, `configure.ac:1290`) adds an SMP scheduler: one
run queue per *processor* (an OS thread). Short-held low-level spinlocks are
taken in a fixed order: mutex → condvar → thread → thread group → processor →
VM. Trylock-and-retry is used where that order cannot be followed
(`_thread.scm:13-45`).

In `mem.c`, each processor state `___ps` owns:
- `alloc_heap_ptr/limit`, a thread-local heap bump area in "msections";
- `alloc_stack_ptr/limit`, a per-processor stack area. Frames are copied to
  the heap lazily when a continuation is captured.

The GC is stop-the-world parallel. It runs in phases (setup, mark-strong, …)
separated by `BARRIER()` calls across processors
(`lib/mem.c:277-301`, `:6775-6830`).

### Polling
The interrupt poll is merged with the stack-overflow check: `___POLL` compares
`fp` with `stack_trip`, and an interrupt is requested by setting
`stack_trip = stack_start` (`include/gambit.h.in:8483-8516`). This is Feeley's
1993 "polling efficiently on stock hardware". OCaml 5 cites the same algorithm
to bound the distance between safepoints.

## 4. chibi-scheme, Chicken, Larceny: green threads, no GC impact

### chibi
- A thread is a `sexp_context` with its own stack object.
- The bytecode loop decrements `fuel` and, when it runs out and not
  `SEXP_G_ATOMIC_P`, saves `ip/fp/top` and calls the Scheme-level scheduler,
  which returns the next context (`chibi:vm.c:1084-1160`).
- Blocking I/O raises "I/O would block" and the thread waits on a pollfd set
  (`lib/srfi/18/threads.c`).
- The GC is unchanged: only one mutator runs at a time.
- Deviations from SRFI 18:
  - `make-thread` **resets** parameters to `()` and gives the thread a fresh
    `dk` (wind) vector, so parameterizations are not inherited
    (`threads.c:84-110`);
  - `thread-terminate!` zeroes the target's fuel and stores a "thread
    terminated" exception;
  - `abandoned-mutex-exception?`, `terminated-thread-exception?` and
    `uncaught-exception?` are stubs returning `#f`
    (`lib/srfi/18/interface.scm:79-83`).

### Chicken
- Green threads built on first-class continuations; "execution of Scheme code
  on multiple processor cores is not available".
- Blocking I/O blocks every thread except some socket operations.
- A new thread inherits the dynamic environment.
- On error termination, pending `dynamic-wind` thunks are *not* run.
- The manual says calling another thread's continuation "is generally not a
  good idea … if dynamic-wind is involved".
- Cheney-on-the-MTA uses the C stack as the nursery, so the collector is
  inherently single-threaded.

### Larceny
- Has only an experimental timer-interrupt tasking library: `spawn`, `yield`,
  `without-interrupts` (`larceny:lib/Experimental/tasking.sch`).
- The runtime's SSB write barrier states "The code in this file is *NOT*
  reentrant" (`src/Rts/Sys/barrier.c:1-30`).
- Its regional GC research assumes a single mutator.

### Lesson
Green threads cost the GC nothing beyond treating every thread's stack (or
thread object) as a root. They deliver all of SRFI 18, including
cross-thread continuations, if the scheduler is built on the VM's own
continuations.

## 5. Gauche and Guile: pthreads plus Boehm–Demers–Weiser (BDW)

### Gauche
- Each thread is an `ScmVM` attached to a pthread.
- BDW is built with `GC_THREADS`, `PARALLEL_MARK` and `THREAD_LOCAL_ALLOC`
  (`Gauche:src/gauche/config_threads.h:2`, `gc/include/config.h:207,241`).
  Threads must be created through BDW's `pthread_create` wrapper "for it is
  the only way for GC to see the thread's stack" (`src/vm.c:233-256`).
- BDW stops the world by sending each thread a signal and parking it in the
  handler (`gc/doc/gcdescr.md:465-490`). Thread-local free lists remove the
  allocation lock (`:509-543`).
- A new VM inherits **only the parameterization**. Parameter cells that are
  not shared are copied, and exception and dynamic handlers are reset
  (`src/vm.c:312-319`, `:1377-1408`).
- Thread-locals come in two kinds, inheritable and non-inheritable
  (`src/threadlocal.c`).
- `thread-terminate!` works in escalating steps:
  1. set a `stopRequest` flag that the VM loop polls;
  2. send a signal to interrupt a blocking system call;
  3. optionally `pthread_cancel`, which is described as "not safe".

  `dynamic-wind` after-thunks are not run (`src/thread.c:373-400`;
  `src/vm.c:3937-3966`).
- A continuation captured on a C stack that is gone, or in another thread, is
  a **"ghost"** whose Scheme part runs on the current C stack
  (`src/vm.c:3592-3600`). Cross-thread invocation therefore works only for the
  Scheme portion.

### Guile
- Guile threads "are wrappers around the system's POSIX threads".
  `call-with-new-thread` runs its thunk "with a new dynamic state" that
  inherits fluid values; fluids act as thread-local storage.
- `cancel-thread` runs `dynamic-wind` post-thunks but not throw handlers,
  unlike SRFI 18.
- A continuation may be used "only from the thread in which it was created".
- Since Guile 2.0, threads blocked in "guile mode" no longer stop the GC.
  `scm_without_guile` is now only an optimization; while outside guile mode, a
  thread must not touch `SCM` values (manual, *Blocking*).
- Guile's JIT relies on BDW's conservative stack scanning, so it emits no
  stack maps.

### Whippet
Whippet is Andy Wingo's replacement collector for Guile:
- Every thread needs a mutator handle (`gc_init_for_thread`).
- Safepoints are **cooperative** (`gc_safepoint`), and blocking regions use
  `gc_call_without_gc`.
- Allocation and write-barrier fast paths are parameterized by "attributes" so
  that a JIT can inline them.
- Stacks can be scanned conservatively while the heap is traced precisely, and
  Immix-style opportunistic evacuation is kept.

Wingo's "on safepoints" explains why BDW's preemptive, signal-based stopping
requires stacks to be traversable at every instruction, while cooperative
safepoints can be lazy. He knows of no production collector that is fully
preemptive and precise.

## 6. OCaml 5: domains with per-domain minor heaps, concurrent major GC, effect fibers

### What the team chose and why
The ICFP 2020 paper set three requirements:
1. sequential programs must keep their performance and the C API;
2. parallel programs must first minimize pause times, then scale.

It compared two minor collectors:
- **ConcMinor**, with domain-private minor heaps collected independently. This
  needs a read barrier that promotes an object when another domain reads it,
  so reads become safepoints and every C stub breaks.
- **ParMinor**, a stop-the-world parallel minor collector. It allows pointers
  between minor heaps, so it needs no read barrier.

ParMinor "outperforms the concurrent minor collector in almost all
circumstances". On sequential benchmarks ConcMinor was 4.9% slower than stock
OCaml and ParMinor 3.5% slower. OCaml 5 shipped ParMinor.

### Minor heap
- Each domain has its own bump-allocated minor heap, carved from one
  reservation (128 domains × 16 MB of address space by default in the paper).
- Promotion inside the stop-the-world section is parallel. A domain claims an
  object with a CAS on its header to an "in-progress" state.
- A single-domain fast path elides the CAS.
- Domains do opportunistic major-GC work while waiting at the barrier.

### Major heap
- Shared, non-moving, mostly-concurrent mark-and-sweep in the style of VCGC.
- Allocation uses size-segregated per-domain pages, after Streamflow.
- Mutators and the collector synchronize only once per cycle, unless
  ephemerons are involved.
- The barrier is a **deletion barrier**: `caml_modify` darkens the *old* value
  while marking is active, and logs major→minor edges in the domain's
  `major_ref` table (`ocaml:runtime/memory.c:306-354`).
- The store itself is an acquire fence followed by a release store. This is
  free on x86 and costs fences on ARMv8 (`memory.c:100-200`).

### Safepoints and blocking
- Polls are inserted with Feeley's algorithm.
- An interrupt is requested by setting `young_limit` to `UINTNAT_MAX`, so the
  next allocation check traps. The allocation-limit check doubles as the poll
  (`runtime/domain.c:886`, `:2022-2040`).
- A domain inside a blocking section hands its stop-the-world duties to a
  **backup thread**. This is needed because every domain must take part in a
  ParMinor collection (`domain.c:84-150`).
- Systhreads inside one domain share a domain (master) lock
  (`otherlibs/systhreads/st_stubs.c:182`).

### Fibers
- Effect handlers use one-shot continuations
  (`Continuation_already_resumed`, `stdlib/effect.mli:32`) over heap-allocated,
  growable stack segments.
- Because a deletion barrier requires stacks to be scanned at the start of a
  cycle, **a fiber must be fully marked before control switches to it**. The
  fiber is locked during marking; a mutator that wants to switch to it spins,
  and a marker that finds it locked skips it (paper §5.4).
- Libraries such as domainslib suspend a task on one domain and resume it on
  another.

## 7. CPython free-threading (PEP 703; officially supported in 3.14 per PEP 779)

### Reference counting
- **Biased reference counting:** the owning thread updates a local count
  non-atomically, and other threads update a shared count atomically.
- **Immortal objects** turn INCREF and DECREF into no-ops.
- **Deferred reference counting** for functions, modules and similar objects:
  they are freed only by the GC.

### Cyclic GC
- Became **stop-the-world and non-generational**, with two pauses.
- Finds GC objects by walking mimalloc heaps instead of linked lists.
- Objects are segregated into three mimalloc heaps: non-GC, GC with a managed
  dict, and other GC objects.

### Thread states and locking
- Threads are ATTACHED, DETACHED or GC. The "eval breaker" asks attached
  threads to pause, and detached threads are moved to GC by CAS.
- Per-object mutexes and `Py_BEGIN_CRITICAL_SECTION` replace the GIL. A blocked
  critical section suspends the outer ones, which prevents deadlock.
- Reads of lists and dicts are optimistic and lock-free. They are made safe by
  delaying reuse of mimalloc pages until every thread has passed a sequence
  number (QSBR-like).

### Cost
- The PEP's estimate was 5–6% for one thread.
- PEP 779 reports about 10% on Linux and Windows and 3% on macOS, with a 15%
  hard target, and 15–20% more memory against a 20% target.

### Lesson
Atomic reference counting dominates the cost. A tracing GC with stop-the-world
pauses and thread-attached states is the simpler half of the design.

## 8. Ruby Ractors, and JS workers and isolates: isolation instead of sharing

### Ruby
- Ractors may share only *shareable* objects, such as deeply frozen values and
  classes.
- Threads inside a Ractor are serialized by that Ractor's lock.
- Today, after the first Ractor starts, `ObjectSpace.each_object` yields only
  shareable objects (bug #21401), and GC is global.
- ko1's open **Feature #22227** gives each Ractor its own objspace:
  - a **local GC** runs on the owner thread "with no VM lock and no barrier";
  - a stop-the-world **global GC** runs only for shareable objects,
    cross-Ractor edges and dead Ractors;
  - the invariant is that an unshareable object is reachable only from its
    owner, except through edges the GC has been told about. Shareable objects
    that point to unshareable ones are recorded by a write barrier ("shrefs"),
    and in-flight messages are pinned.
- Results:
  - single-Ractor overhead of 2–4%;
  - multi-Ractor JSON parsing at 3.32×, against 10.17× on master and 3.33× for
    fork.

### JS workers
- Each worker is a separate isolate with its own heap and independent GC.
- Values cross by structured clone or transfer.
- `SharedArrayBuffer` shares raw bytes only, and requires cross-origin
  isolation.
- The TC39 shared-structs proposal (Stage 2) adds shared objects under the rule
  "there are no references from shared objects to non-shared objects", plus
  `Atomics.Mutex` and `Atomics.Condition`.
- V8 prototyped a separate shared space for these objects. Fields that may be
  read atomically must be 64-bit aligned to avoid tearing (Wingo, 2025).

Neither model provides SRFI 18's shared mutable object space. Both show how
much simpler the collector becomes when the heap partitioning is
*semantically* guaranteed.

## 9. Java (HotSpot, Loom): the reference design for M:N over a shared heap

### Platform threads
- Platform threads map 1:1 to OS threads. Each has a TLAB and a thread
  register.
- **JEP 312 thread-local handshakes** replaced the global polling page with a
  per-thread poll pointer. This lets the VM stop *one* thread (to revoke a
  biased lock, take a stack sample, or run an asymmetric Dekker protocol for
  barrier elision) for under 1% overhead.

### Virtual threads (JEP 444)
- Virtual threads are M:N over carrier threads.
- Their stacks are **heap `StackChunk` objects** and "are not GC roots".
- Frames are frozen (copied into a chunk) on unmount and thawed lazily on
  mount.
- A chunk may receive frames without GC barriers only while
  `requires_barriers()` is false: young in G1, or in an allocating region in
  ZGC. After that, frames can only be thawed out of it, and the next freeze
  allocates a new young chunk (loom-dev, 2021).
- Pinning by `synchronized` was removed in JEP 491 (JDK 24). Pinning by native
  frames remains.

### Thread locals
Thread-locals are discouraged for very large numbers of threads. **Scoped
values** (JEP 506) are immutable, inheritable dynamic bindings, which is the
Java counterpart of a deep-bound `parameterize`.

## 10. Semantics matrix

| | New thread inherits | Terminate runs winders? | Abandoned mutex | Cross-thread `k` | Thread-locals |
|---|---|---|---|---|---|
| SRFI 18 | dynamic env; handler set to initial; empty wind list | no ("impossible … to perform any kind of cleanup") | yes, exception | well-defined: winders run, dynamic env reinstated | `thread-specific` |
| SRFI 226 | parameterization of the `make-thread` call | unspecified; adds `thread-interrupt!`, `thread-schedule-terminate!` | — | not stated; threads start with a prompt | `make-thread-parameter` / inheritable thread locals; plain parameter mutation is shared |
| Chez | thread parameters by value; winders reset | no terminate | no | yes (heap continuations) | thread parameters |
| Racket | parameterization (continuation mark) + preserved thread cells | `kill-thread`: no | custodians | yes, continuations migrate between OS threads | thread cells |
| Gambit | dynamic env | per SRFI 18 | yes | yes (reference) | specific field |
| chibi | **nothing** (params reset) | no | stub `#f` | probably works (single heap) | specific field |
| Chicken | dynamic env | no | yes | discouraged | specific field |
| Gauche | parameterization only | no (flag → signal → cancel) | yes | Scheme part only ("ghost") | inheritable/non-inheritable |
| Guile | copy of dynamic state (fluids) | **yes**, post-thunks run | — | **forbidden** | fluids, thread-local fluids |
| OCaml 5 | — | — | — | one-shot, resumable on another domain | `Domain.DLS` |

## 11. Cross-cutting GC mechanisms

### Thread-local allocation
Universal:
- bump-pointer TLAB or segment (Chez, Gambit, OCaml minor heap, HotSpot);
- thread-local free lists (BDW);
- per-thread size-class pages (OCaml major heap, mimalloc).

Refill goes through a global lock or CAS. Chez also uses the TLAB's free top
end as the write-barrier log.

### One poll word for everything
A single poll word serves preemption, signals, GC rendezvous and handshakes:
- Chez: `%trap` counter, 1000 ticks;
- Gambit: `stack_trip`, merged with the stack-overflow check;
- OCaml: `young_limit`, merged with the allocation check;
- HotSpot: per-thread poll word;
- chibi: `fuel`.

Merging the poll into a check that already exists (stack overflow,
allocation limit) makes it nearly free.

### Stop-the-world rendezvous
The pattern is the same everywhere:
1. a requester sets the flag in every thread;
2. active threads park at their next poll;
3. *inactive* threads (blocked, in FFI, detached) are already counted as
   stopped;
4. the last thread to arrive, or a designated leader, collects;
5. the others either wait (Chez) or help (OCaml ParMinor, Gambit SMP,
   Chez sweepers).

### Deactivate protocol
Every system has one: Chez deactivate/`__collect_safe`, OCaml blocking
sections with a backup thread, CPython DETACHED, Guile `scm_without_guile`,
Whippet `gc_call_without_gc`, and HotSpot's `_thread_in_native` state. Without
it, one thread blocked in `read(2)` stalls every collection.

Chez's trylock-then-deactivate on mutexes keeps the uncontended path cheap.
The deadlock it avoids: thread A holds mutex M and parks for GC, while thread B
blocks on M and never parks.

### Barriers without atomics
- Per-thread logs: Chez's TLAB-top log, OCaml's per-domain `major_ref`,
  Larceny's SSB.
- Byte-store card marking, which is idempotent and so harmless under races.

Atomics appear only where two threads may install the same object at once:
- CAS on a header during parallel promotion or forwarding (OCaml);
- `fetch_or` on side mark bitmaps during parallel marking;
- Chez avoids even these through segment ownership and forwarding of remote
  work.

### Publication safety
On ARM, a store that publishes a new object must be ordered after the stores
that initialized it. Chez uses a store–store fence in threaded builds; OCaml
uses a release store in `caml_modify`. On x86 this is free.

### Per-thread nursery versus shared old space
OCaml's experience shows that independent per-thread nurseries need a read
barrier or eager promotion (Doligez–Leroy). A stop-the-world *parallel*
nursery collection is simpler, faster, and keeps the FFI unchanged.

### Stacks and continuations as heap objects
These keep the root set bounded (Loom: "not GC roots"), and they make M:N
migration and cross-thread `k` trivial. The price comes once marking becomes
incremental or concurrent:
- a stack or continuation snapshot must be darkened before it is resumed
  (OCaml fibers);
- barrier-free frame copying must target young or allocating chunks only
  (Loom `requires_barriers`).

## 12. JIT consequences

1. **Pinned context register** holding the TLAB pointer and limit, the poll
   word, the barrier log, and the dynamic state (Chez `tc`, `Caml_state`,
   `___ps`, the HotSpot thread register). The JIT reaches everything
   per-thread through it and never through OS TLS.
2. **Inline fast paths with out-of-line slow paths:** allocation, the poll
   (decrement-and-branch or load-and-branch at back-edges and entries), and the
   barrier. The poll is folded into the allocation-limit or stack check where
   possible. Whippet exposes "attributes" for exactly this.
3. **Precise stack maps at every safepoint and call** (Chez live masks, OCaml
   frame descriptors, HotSpot oop maps), or conservative stack scanning with
   pinning (Guile/BDW, Whippet, V8's recent work). Conservative scanning frees
   the JIT from stack maps but forbids moving objects referenced from the
   stack.
4. **Fences** for publication on weak memory models, emitted only in threaded
   builds, as Chez does.
5. **Code installation across threads.** Chez restricts which threads may run
   new code because of instruction-cache coherence. On macOS/arm64 a
   `MAP_JIT` W^X toggle (`pthread_jit_write_protect_np`) is *per thread*.
   Patching or deoptimizing code that other threads may be running needs a
   handshake (JEP 312).
6. **One poll word for preemption and GC.** If green-thread preemption, GC and
   signals share it, the JIT emits a single poll.

## 13. Implications for Patina

Relevant current state:
- `parameterize` is **shallow-bound**: `dynamic-wind` around
  `%parameterize-swap!` mutates the parameter's global cell. The file itself
  carries a TODO for threads (`lib/scheme/base/parameters.scm:1-6`,
  `docs/VM_RUNTIME.md:792-796`).
- The standard ports are "procedures over a thread-local".
- Continuations are VM stack snapshots kept in side tables (`docs/VM_DECISIONS.md` §4).
- GC Stage 5 lists concurrency as a non-goal (`PRD/ARCHIVE/GC_STAGE5_PRD.md:121`).

Prior art suggests the following.

1. **Threading model.** SRFI 18 requires a shared heap, so isolated heaps are
   out. Ship **green threads on one OS thread** first, as chibi, Gambit and
   Racket do:
   - the VM already has a safe-point flag check (about 1% cost in Stage 5) and
     machine-run stub frames, so a scheduler can swap per-thread VM state on
     the poll;
   - use non-blocking I/O with a poller, or offload blocking calls to helper OS
     threads that never touch the heap.

   Design the representation for **M:N**, as Gambit SMP, OCaml 5, Loom and
   Racket parallel threads do: N carrier OS threads, a shared heap, and a
   stop-the-world parallel GC. Do not design for 1:1 OS threads, which give
   the same GC burden with worse continuation semantics.

   The tree-walker can stay on green threads only.
2. **Make dynamic state deep-bound and per-thread now.** Each thread, and each
   continuation, should carry a parameterization: a parameter → cell map or a
   persistent structure (Racket, Gauche, Guile, Java scoped values). Shallow
   swapping cannot meet SRFI 18's inherited dynamic environment, nor
   cross-thread continuations, nor any parallelism. SRFI 226 then fixes the
   remaining choices:
   - mutating a plain parameter is shared;
   - `make-thread-parameter` creates an inheritable thread-local.
3. **Introduce an explicit `Mutator` context** (Chez `tc`). It should own:
   - the register stack and frames;
   - the TLAB;
   - the barrier log;
   - the poll word;
   - the wind list;
   - the handler stack;
   - the parameterization.

   Keep the heap separate. Avoid Rust `thread_local!` for runtime state: a
   green thread is not an OS thread, and the JIT needs one base pointer.
4. **Continuations should be self-contained heap data** that refer to no
   OS-thread state, so that invoking one in another thread is just "install
   frames, rewind winders against this thread's wind list, reinstate its
   parameterization", as SRFI 18 and Gambit do. Patina's rule that Rust frames
   never become part of a continuation already avoids Gauche's "ghost"
   problem.
5. **Do not convert `Rc` to `Arc`.** PEP 703 shows atomic reference counting
   is the dominant cost of free-threading. Move environments, closures and
   continuations into the traced heap, and keep `Rc`/`RefCell` only for
   mutator-confined or compiler data. `Heap` would become shared through
   `Sync`-audited raw access under the safepoint discipline, not through
   `RefCell`.
6. **GC readiness at little single-thread cost:**
   - per-mutator bump TLABs, refilled under a lock;
   - per-mutator remembered-set logs, no atomics;
   - a mark bitmap that is plain while there is one mutator and becomes
     `fetch_or` or ownership-partitioned (Chez segments) only for parallel
     marking;
   - fences behind a threaded-build flag;
   - a deactivate API around FFI and blocking I/O, with Chez's
     trylock-then-deactivate for mutexes.

   If incremental marking arrives later, plan for darkening a continuation or
   thread stack on resume (OCaml), and for copying frames into continuation
   objects only while they are young (Loom).
7. **Moving collection.** GC Stage 5 rules out moving permanently because raw
   arena indices escape. The user has since lifted that constraint ("free to
   change internal representations"). Every high-throughput parallel design
   above relies on a moving or bump nursery (Chez, OCaml minor heap, Gambit,
   HotSpot) or on conservative pinning plus opportunistic evacuation
   (Whippet). If Patina stays non-moving, the precedents are OCaml's major
   heap and BDW/mimalloc: per-thread size-segregated pages.

### Open questions
- Is cross-thread continuation invocation required (Gambit) or allowed to be
  an error (Guile)?
- Must `thread-terminate!` run winders (SRFI 18 says no; Guile says yes)?
- Is N > 1 a real goal, or is green-only acceptable for SRFI 18 compliance?
- How should blocking I/O work under green threads: a poller or helper
  threads?

## Sources

Local (`~/Project/reference/…`):
- ChezScheme: `c/thread.c`, `c/types.h:371-396`, `c/alloc.c`, `c/gc.c:120-190`,
  `c/schsig.c:574-594`, `c/prim5.c:1603-1621`, `s/7.ss:1253-1297`,
  `s/library.ss:1200-1240`, `s/cpprim.ss:652-720`, `s/cpnanopass.ss:3997`,
  `csug/threads.stex`, `csug/smgmt.stex`, `csug/foreign.stex`,
  `release_notes/release_notes.stex:516-523`.
- racket: `racket/src/thread/README.txt`, `racket/src/cs/README.txt:393-460`,
  `cs/rumble/{engine,memory,place}.ss`, `cs/place-register.ss`,
  `pkgs/racket-doc/scribblings/reference/places.scrbl`.
- gambit: `lib/_thread.scm`, `lib/mem.c`, `include/gambit.h.in`,
  `doc/gambit.txi`, `configure.ac`.
- chibi-scheme: `vm.c:1084-1160`, `lib/srfi/18/{threads.c,interface.scm}`.
- Gauche: `src/thread.c`, `src/threadlocal.c`, `src/vm.c`, `gc/doc/gcdescr.md`,
  `gc/include/config.h`.
- larceny: `lib/Experimental/tasking.sch`, `src/Rts/Sys/barrier.c`.
- ocaml: `runtime/domain.c`, `runtime/memory.c`, `runtime/major_gc.c`,
  `stdlib/effect.mli`, `otherlibs/systhreads/st_stubs.c`.

Web:
- [SRFI 18](https://srfi.schemers.org/srfi-18/srfi-18.html)
- [SRFI 226](https://srfi.schemers.org/srfi-226/srfi-226.html)
- [Guile: Threads](https://www.gnu.org/software/guile/manual/html_node/Threads.html)
- [Guile: Continuations](https://www.gnu.org/software/guile/manual/html_node/Continuations.html)
- [Guile: Blocking](https://www.gnu.org/software/guile/manual/html_node/Blocking.html)
- [Guile: Fluids and Dynamic States](https://www.gnu.org/software/guile/manual/html_node/Fluids-and-Dynamic-States.html)
- [Wingo, "on safepoints"](https://wingolog.org/archives/2023/10/16/on-safepoints)
- [Whippet manual](https://github.com/wingo/whippet/blob/main/doc/manual.md)
- [Wingo, "the last couple years in V8's garbage collector"](https://wingolog.org/archives/2025/11/13/the-last-couple-years-in-v8s-garbage-collector)
- [Chicken srfi-18](https://wiki.call-cc.org/eggref/5/srfi-18)
- [Sivaramakrishnan et al., *Retrofitting Parallelism onto OCaml*, ICFP 2020](https://arxiv.org/pdf/2004.11663)
- [PEP 703](https://peps.python.org/pep-0703/)
- [PEP 779](https://peps.python.org/pep-0779/)
- [Racket blog, "Parallel Threads" (2025-11)](https://blog.racket-lang.org/2025/11/parallel-threads.html)
- [Ruby Feature #22227](https://redmine.ruby-lang.org/issues/22227)
- [Ruby Bug #21401](https://bugs.ruby-lang.org/issues/21401)
- [TC39 proposal-structs](https://github.com/tc39/proposal-structs)
- [MDN SharedArrayBuffer](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer)
- [JEP 312](https://openjdk.org/jeps/312)
- [JEP 444](https://openjdk.org/jeps/444)
- [loom-dev: `requires_barriers` semantics](https://mail.openjdk.org/pipermail/loom-dev/2021-February/002116.html)
