# Steady state and limits: how production runtimes define and enforce them

Date 2026-10-01. Input to the GC PRD's limits and steady-state section (owner: "New limits, we need to have
this as part of effort. may be even worth have some concept as what is a steady state?"). Patina at `main`
`28a94f8`; nothing in the repository was modified.

**Tags.** **[S]** read in source at the pinned revision (path:line); **[D]** read in the project's own
documentation; **[P]** measured here (Apple M4 Pro, macOS 27.2 arm64, one run unless stated, peak RSS from
`/usr/bin/time -l`); **[I]** inference or proposal. Pinned revisions and fetched copies are listed in §11; the
fetched files were not retained, and the probes are under `PRD/study/gc/probes/followup/probes/`.

---

## 0. Findings in one page

1. **"Steady state" has two meanings in the prior art, and a contract needs both.** Go defines it as a property
   of the *workload*: a constant allocation rate and constant marginal GC cost (stable object-graph statistics)
   [D, go.dev/doc/gc-guide]. Every runtime then promises something about the *runtime* under that
   precondition: heap size is a fixed function f(L) of live size, so footprint stops growing, and GC cost per
   allocated byte stays constant. .NET's DATAS adds a portability clause: the same work on a different
   machine gets the same heap [D]. Nobody publishes the runtime half as a testable contract. Go's
   `TestMemoryLimit` and ZGC's `TestUncommit` come closest (§9).
2. **f(L) is the steady-state definition in practice, and production runtimes are moving from linear to
   square-root headroom.** Fixed multipliers: Go 2× by default (`GOGC=100`), Gambit 2× (live-ratio 50%), Lua
   5.4 2× and 5.5 2.5×, OCaml about 2.2× (`space_overhead` 120), Erlang a 25–75% occupancy band, JSC
   2×/1.5×/1.24× by heap size against RAM. Headroom that grows with √L: Racket CS `L + 8192·√L`, Whippet's
   growable sizer `L + √(L·T/2)`, .NET DATAS's gen0 budget `(20 − conserve)/√L_MB` of L, MemBalancer
   `L + √(L·g/(c·s))` (in Whippet's adaptive sizer; behind a flag, off by default, in V8). HotSpot G1 sizes by a
   GC-CPU target (4%) rather than by L [S]. Patina's whole-heap rule (next major after `max(8 MiB, 2·L)`, peak
   about 3·L) is at the generous end of the linear group: equivalent to `GOGC=200`.
3. **Limits come in seven kinds** (§4): hard heap (HotSpot `-Xmx`, .NET `GCHeapHardLimit`, V8
   `--max-old-space-size`, Gambit `max-heap`), soft (Go `GOMEMLIMIT`, HotSpot `SoftMaxHeapSize`), per heap or
   isolate (V8, .NET per object heap), per actor or subtree (Erlang `max_heap_size`, Racket custodians), per
   thread stack (JVM `-Xss`, Go `SetMaxStack`, OCaml `stack_limit`, Lua `LUAI_MAXSTACK`), external memory
   (V8 `ExternalMemoryAccounter`, .NET `AddMemoryPressure`, OCaml `custom_major_ratio`, Chez and Racket
   phantom bytes), and **progress limits**, the death-spiral guards (HotSpot `UseGCOverheadLimit`, V8's
   ineffective mark-compacts, Go's 50% CPU limiter, Netflix's jvmquake).
4. **Every runtime with a hard heap limit also has a progress guard, and Patina's design has none.** Near the
   ceiling a collector can run back to back, each collection reclaiming almost nothing. HotSpot throws
   `OutOfMemoryError` after 5 consecutive GCs with more than 98% of time in GC and less than 2% of the heap
   free (G1 since JDK 26) [S]. V8 aborts after 4 consecutive mark-compacts at 95% or more of the limit with
   mutator utilization below 0.4 [S]. Go lets memory overshoot rather than spend more than about 50% of CPU
   [S]. chibi, which has a heap cap and no guard, ran at 100% CPU at its cap until killed after 30 s [P].
   Patina's §13 rule ("two majors each freeing under 1% raise the target ×1.5") is Wingo's livelock fix, and
   at the ceiling the ×1.5 cannot apply. The design's only terminal condition there is an allocation that
   fails after a major (§5, step 7), and a program that reclaims a little at each major never reaches it: it
   collects back to back instead. A deterministic guard counted in majors and bytes is the gap (§10.3).
5. **Behaviour at the limit is a ladder, and its last rung differs.** G1 tries a young GC, then a full GC,
   then a full GC that clears soft references with maximal compaction, then OOME [S]. V8 tries a minor GC,
   then a full GC, then a last-resort GC (up to 7 cycles until the roots are stable), then the embedder's
   near-limit callback, then a fatal abort [S]. Lua runs an emergency full GC, then returns `LUA_ERRMEM`
   *without running the message handler* [S, D]. The final rung is a catchable condition (JVM, .NET, OCaml,
   Gambit, Lua, Racket), a fatal abort (V8 134, Gauche 1, Chez `out of memory`), or killing the culprit
   (Erlang, Racket custodians). Patina's design (one collect-and-retry, a 4 MiB emergency reserve, a catchable
   `&heap-exhausted`, a non-zero exit) is the JVM and Gambit shape.
6. **Stacks split into two camps.** "The stack is heap": Chez, Racket, Gambit, Guile, Erlang and Gauche
   bound recursion only by memory. Racket's guide says flatly that there is no such thing as stack overflow
   [D]. Gambit raises `stack-overflow-exception` when a frame would exceed the heap [D]. "Fixed per-thread
   stacks": JVM (1 MiB on Linux x64, 2 MiB on macOS arm64), V8 (984 KiB), JSC (5 MiB), Lua (1 M slots), chibi
   (about 8.2 M words), .NET (where overflow kills the process). Go and OCaml 5 sit between: growable stacks
   with a large cap (1 GB per goroutine, 1 GiB per fiber). Patina's 8 GiB main register stack is in the first
   camp in effect, and Guile's design (grow by doubling, return unused pages at GC without shrinking, a
   last-resort unwind-only exception) is its closest model.
7. **Handler room is universal.** HotSpot's yellow zone, JSC's 64 KiB reserved and 128 KiB soft-reserved
   zones, Lua's `ERRORSTACKSIZE` and Guile's unwind-only exception all exist so that reporting the failure
   does not itself fail. Patina's last stack MiB and 4 MiB heap emergency reserve
   follow this pattern. The prior art adds one rule: when even the handler cannot run, unwind without running
   pre-unwind handlers (Guile, Lua) instead of aborting.
8. **Memory return is hysteresis counted in collections or in time.** Boehm unmaps a block after 6
   consecutive collections free [S]. Chez keeps `heap-reserve-ratio` (1.0) empty segments per occupied one
   [S]. Patina's design waits 2 majors. ZGC waits 300 s, V8's memory reducer 8 s, and G1's periodic GC is off
   by default [S]. The count-based rules keep pacing deterministic, which matters for Patina's byte-identical
   lanes.
9. **Test methods exist and are cheap to copy** (§9): step tests against a limit (Go), return-to-baseline
   with both bounds (ZGC), overhead-limit tests that expect a non-zero exit (HotSpot), allocation-failure zeal
   (Lua `EMERGENCYGCTESTS`, `MEMLIMIT`), minimum-heap bisection and evaluation at 1–6× the minimum heap
   (DaCapo Chopin H1/H2), MMU over windows (Cheng and Blelloch), heap-growth detection (Cork), and slope tests
   over iteration counts. A four-implementation slope probe shows why the warm-up must be excluded: Chez rises
   102 → 124 MB and then stays flat for 16× more work (§9.2).
10. **Proposal** (§10): adopt a seven-clause steady-state contract (SS1–SS7) stated in bytes and majors, never
    in seconds. Add a byte-counted progress guard, an optional soft target in bytes, an embedder near-limit
    hook and external-memory accounting. State exactly what `max_heap` counts. Give each clause a named test.

---

## 1. What "steady state" means in the prior art

| Source | Definition | Kind |
|---|---|---|
| Go GC guide [D] | The application allocates at a constant rate in bytes per second, and the marginal GC costs are constant, meaning the object-graph statistics (object sizes, pointer counts, structure depth) stay the same from cycle to cycle | workload precondition |
| Go pacer [S, `mgcpacer.go:504`] | The trigger is computed "assuming the heap is in steady-state" | pacer assumption |
| Go soft-limit design [D, 48409 §"Alternative approaches"] | It rejected span-based accounting because it would make "the definition of the GC steady-state" depend on fragmentation | rejected definition |
| .NET DATAS [D] | Heap size roughly proportional to the long-lived data size: the same work on machines with different specs gets the same or a similar heap, and the heap follows the workload as it gets lighter or heavier | runtime guarantee, portable |
| MemBalancer (Kirisame, Shenoy, Panchekha) [D, arXiv 2204.10455] | The square-root limit "minimizes total memory usage for any amount of total garbage collection time" under steady allocation and collection rates, and is optimal across heaps without communication | optimality under the precondition |
| Cheng and Blelloch, PLDI 2001 [D] | MMU(w): the minimum mutator fraction over every window of length w, so the requirement holds for every window position (a "rolling" guarantee) | pause-side steady guarantee |
| DaCapo Chopin (Blackburn et al., ASPLOS 2025) [D] | The minimum heap reflects a workload's *peak* use, not its average; "area under the memory use curve" would better measure footprint | metric caveat |
| Cork (Jump and McKinley, POPL 2007) [D] | A leak is *systematic heap growth*; Cork detects it by differencing per-type volumes across consecutive full collections, damping fluctuations | negative definition (not steady) |

**Synthesis [I].** Steady state is the conjunction of (a) a workload precondition, a bounded live size and a
constant allocation rate per unit of work, and (b) a runtime response: footprint converges to f(L), memory
returns when L falls, and GC cost per allocated byte stays constant. No runtime publishes (b) as a contract.
Each publishes an f and leaves the guarantee implicit.

---

## 2. Heap size as a function of live size L

| Runtime | Rule (default) | Heap / L at L = 100 MiB | at 1 GiB | Source |
|---|---|---|---|---|
| Go | goal = L + (L + stacks + globals)·GOGC/100, GOGC 100, minimum 4 MiB; memory-limit goal = limit − non-heap − overage, less 3% (at least 1 MiB) | ≈ 2 | ≈ 2 | [S] `mgcpacer.go:59-75,1293-1302,1045-1158` |
| HotSpot G1 | not a function of L: grows or shrinks at young GCs toward a GC-CPU target from `GCTimeRatio` = 24 (4%); G1 resets `Min/MaxHeapFreeRatio` to 0/100 unless set; pause goal 200 ms | — | — | [S] `g1Arguments.cpp:206-235`, `g1HeapSizingPolicy.cpp:143-172` |
| HotSpot Parallel | priority order: pause goal, then throughput (`GCTimeRatio` 99 = 1%), then footprint (shrink while both goals hold) | — | — | [S] `gc_globals.hpp:322,332`; [D] Oracle GC tuning guide, "Ergonomics" |
| V8 | growing factor from a target mutator utilization of 0.97 given GC and mutator speeds, clamped to [1.1, 4.0] (max 1.3–2.0 on small devices); MemBalancer `L + f·√(g·L)` clamped to [1.1, 4.0]·L behind `--sqrt-allocation-limits` (default **false**) | 1.1–4 | 1.1–4 | [S] `heap-controller.h:20-23`, `heap-controller.cc:40-62,97-121,165-182`, `flag-definitions.h:2703-2722` |
| JSC | ×2 below 25% of RAM, ×1.5 below 50%, ×1.24 above; on machines with ≥ 16 GB, `3·e^(−2x) + 1` with x = heap/RAM | 2 (typical) | 2 (typical) | [S] `Heap.cpp:171-204`, `OptionsList.h:216-226` |
| .NET DATAS | gen0 budget ≤ L·clamp((20 − conserve)/√L_MB, 0.1, 10), conserve 5 by default; 2% throughput-cost target | 2.5 | 1.47 | [D] runtime-config GC page |
| Chez | gen-0 collection every `collect-trip-bytes` (8 MiB on 64-bit); radix 4; max generation collected when bytes allocated reach 2× the bytes after the last max-generation collection | ≈ 2 (old) + 8 MiB | ≈ 2 | [S] `s/7.ss:609-623,872-893`, `s/cmacros.ss:2119-2122` |
| Racket CS | major when allocated ≥ L + 8192·√L (also on allocated+overhead); first majors at 32 / 64 MiB; nursery 8 MiB per allocating place | 1.80 | 1.25 | [S] `rumble/memory.ss:39,55-57,156-160` |
| Gambit | resize after GC so that live = `live-ratio` (50%) of heap; min heap max(2 MiB, cache/2); max heap unset by default | 2 | 2 | [S] `lib/mem.c:4506-4521`, `lib/mem.h:74-75`; [D] `gambit.txi:2136-2156` |
| Whippet growable (Guile) | target = L + √(L·T/2), T = `heap-double-threshold` 60 MiB (2× at L = 30 MiB); never shrinks | 1.55 | 1.17 | [S] `growable-heap-sizer.h:12-38`, upstream `gc-options.c:18-29` |
| Whippet adaptive | MemBalancer: L·(1 + e·√(L·g/s)), e = `heap-expansiveness` 1.0, with min/max multipliers and minimum free space; background heartbeat | — | — | [S] `adaptive-heap-sizer.h` |
| OCaml 5 | dead + free ≈ `space_overhead`% of live (120); below `small_heap_limit` an idle phase is added | ≈ 2.2 | ≈ 2.2 | [S] `config.h:213-216`, `gc.mli:138-151` |
| Lua | new cycle when bytes in use reach `pause`% of the bytes in use after the previous collection: 200 in 5.4.7 (the 5.4 manual's default), 250 in `lua/lua` master (the 5.5 manual states no default) | 2 / 2.5 | same | [S] `lgc.h` (v5.4.7:129, master:191); [D] 5.4 and 5.5 manuals §2.5.1 |
| Erlang (per process) | heap grows if live > 75% after a fullsweep, shrinks if < 25%; Fibonacci sizes from 233 words, then +20% steps | 1.33–4 | 1.33–4 | [D] `internal_doc/GarbageCollection.md:118-132` |
| chibi | grow ×2 when more than 75% is still in use after GC; never shrinks; no maximum by default | 1.33–2.67 | same | [S] `features.h:313-330`, `gc.c:715-735` |
| Boehm (Gauche) | `GC_free_space_divisor` 3: about heap/3 allocated between collections | ≈ 1.5 | ≈ 1.5 | [S] Gauche `gc/alloc.c:181-183`; [D] `README.environment:146-148` |
| **Patina (design §13)** | next major after max(8 MiB, 2·L) of allocation, where L counts LOS and external bytes; generational heaps (stage 7) `L + max(8 MiB, min(8192·√L, 2·L))` after an A/B; MemBalancer opt-in | **3 (peak)** | **3 (peak)** | DESIGN §13, decision 15 |

**Observations [I].**
- The linear rules cluster at about 2× (Go, Gambit, Lua 5.4, OCaml, Chez's old generation). Patina's 3× peak
  is a throughput-first choice, which the owner has made. Hertz and Berger (OOPSLA 2005) measured why the
  space-time curve is steep there: their collector matched explicit management at 5× memory, ran 17% slower
  at 3× and about 70% slower at 2× [D].
- Three independent production lines arrived at √L headroom: Racket CS, .NET DATAS (.NET 9's default) and
  Whippet's *growable* sizer. The last is notable because it is not the adaptive one: Wingo
  replaced a plain multiplier with √L even where nothing is measured. Whippet's own manual still describes
  `heap-size-multiplier` (stale; the code and option table use `heap-double-threshold`) [S, D].
- V8 implemented MemBalancer (`ComputeSqrtLimit` cites the paper) and still ships it off by default [S].
  Together with Wingo's "hyperactive squirrel" caveat, this supports the design's choice to keep adaptive
  pacing opt-in.
- Only JSC and V8 scale f by physical RAM; .NET DATAS and Go deliberately do not (the DATAS portability
  clause). Patina's inputs are bytes, never time or RAM, so f is portable by construction. That is worth
  stating as a contract clause (SS5).

---

## 3. Returning memory

| Runtime | Mechanism | Hysteresis unit | Source |
|---|---|---|---|
| Go | background scavenger capped at 1% of mutator time (`scavengePercent`); goal without a limit: retain (heapGoal/lastHeapGoal)·lastHeapInUse + 10%; with a limit: 95% of the limit; synchronous scavenging when an allocation would pass the limit or the heap grows; only chunks not densely allocated for a full GC cycle; `debug.FreeOSMemory` forces a GC and a full release | GC cycles plus a CPU cap | [S] `mgcscavenge.go:5-90,103-126,174-188` |
| HotSpot G1 | uncommit at shrink; JEP 346 periodic GC when idle (`G1PeriodicGCInterval`, default 0 = off; `G1PeriodicGCSystemLoadThreshold`), "85%" less committed memory reported [D] | wall time, opt-in | [S] `g1_globals.hpp:338-352`; [D] JEP 346 |
| ZGC | `ZUncommit` on, `ZUncommitDelay` 300 s | wall time | [S] `z_globals.hpp:52-57` |
| V8 | memory reducer: on the transition from high to low allocation rate, up to `memory_reducer_gc_count` (2) incremental GCs, first after 8 s, watchdog 100 s; `LowMemoryNotification` and `MemoryPressureNotification` from the embedder | wall time and allocation rate | [S] `memory-reducer.h` (automaton comment), `memory-reducer.cc:18-21`, `flag-definitions.h:2685-2692`, `v8-isolate.h:864-870,1510-1514` |
| .NET | `GCRetainVM` (default: release); `GCConserveMemory` 1–9 compacts the LOH when it fragments; full compacting GCs above `GCHighMemPercent` (90%, up to 97% on large machines) | collections plus machine load | [D] runtime-config GC page |
| Chez | after a collection targeting `release-minimum-generation` (default: the max generation), keep `heap-reserve-ratio` (1.0) empty segments per occupied non-static segment and free the rest | major collections | [S] `c/segment.c:460-484`; [D] `csug/smgmt.stex:365-389,417-431` |
| Boehm (Gauche) | unmap a block after `GC_UNMAP_THRESHOLD` consecutive collections free (6 by default) | collections | [S] `gc/allchblk.c:404`; [D] `README.environment:150-153` |
| Guile (stack) | at GC, return the unused part of the stack to the OS without shrinking the stack | collections | [D] `api-debug.texi:690-697` |
| Go (stacks) | at GC, halve a goroutine stack using less than ¼ of its size; free empty stack spans | GC cycles | [S] `stack.go:1286-1330` |
| **Patina (design H.3)** | blocks empty for 2 consecutive majors, decommitted in 4 MiB runs after the pause, rate-limited to 64 MiB per poll; LOS recycle cache 32 MiB; register stacks and store buffers on the same rule, where an "observation" is a major or a return to the top level | majors and observations | DESIGN H.3 |

**[I]** Patina's choice to count majors, not seconds, keeps decommit deterministic. Boehm and Chez do the
same; Go, ZGC and V8 do not. The "observation = return to top level" clause is Patina's answer to the
idle-process problem that JEP 346 and V8's memory reducer solve with timers.

---

## 4. Kinds of limit

| Kind | What counts | Examples | At the limit |
|---|---|---|---|
| **Hard heap** | heap reservation or commit | HotSpot `-Xmx` (default 25% of RAM, `MaxRAMPercentage`) [S `gc_globals.hpp:271`]; .NET `GCHeapHardLimit` = commit for heap *and GC bookkeeping*; in a container the default is max(20 MB, 75% of the container limit) [D]; V8 old generation = RAM/2 clamped to [256 MiB, 4 GiB] on 64-bit [S `heap.cc:357-370`, `heap.h:324-329`]; Gambit `max-heap` [D]; Whippet `maximum-heap-size` [S]; Boehm `GC_MAXIMUM_HEAP_SIZE` [D]; chibi `-h size/max` [S `main.c:442-453`] | OOME, fatal abort, or a callback (§5) |
| **Soft target** | a target the GC aims under but may exceed | Go `GOMEMLIMIT`/`SetMemoryLimit`, counting everything the runtime maps less what it released (`Sys − HeapReleased`), honoured even with `GOGC=off` [S `debug/garbage.go:180-234`]; HotSpot `SoftMaxHeapSize` (manageable, ≤ `-Xmx`, JDK 13, used by ZGC) [D JDK-8222145]; JSC `criticalGCMemoryThreshold` 0.80 of machine memory in use [S]; .NET `GCHighMemPercent` 90% [D] | collect more often; never fail |
| **Per heap or isolate** | one GC heap of many in a process | V8 per isolate (with an optional global limit including external memory, `enforce_global_heap_limit` default false) [S]; .NET per object heap (`GCHeapHardLimitSOH/LOH/POH`) [D]; Racket CS nursery per place [S] | as for hard |
| **Per actor or ownership subtree** | memory reachable from, or charged to, a unit | Erlang `max_heap_size`: words, all generations, *the stack*, on-heap messages and GC scratch, checked only when a GC is triggered; `kill` (untrappable, default true), `error_logger` (default true), `include_shared_binaries` (default false); 0 disables it (default) [D `erlang.erl:8170-8230`]. Racket `custodian-limit-memory`: checked after a GC (and on single allocations larger than the limit); objects reachable from two unrelated custodians are charged arbitrarily [D `custodians.scrbl:108-137`, `eval-model.scrbl:1121-1136`] | kill the process or shut down the custodian |
| **Per-thread stack** | one thread or fiber | JVM `-Xss`/`ThreadStackSize` 1024 KiB on linux-x64, 2048 KiB on bsd-aarch64 [S]; Go `SetMaxStack` 1 GB (250 MB on 32-bit), with a hard ceiling of 2× [S `proc.go:163-172`]; OCaml `stack_limit` 128 M words = 1 GiB [S `config.h:187-189`]; Lua `LUAI_MAXSTACK` 1,000,000 slots [S `ldo.c:186-207`]; V8 `--stack-size` 984 KiB [S `globals.h`]; JSC `maxPerThreadStackUsage` 5 MiB [S `OptionsList.h:93`] | §6 |
| **External memory** | non-heap bytes kept alive by heap objects | V8 `ExternalMemoryAccounter` (replacing `AdjustAmountOfExternalAllocatedMemory`) [S]; .NET `GC.AddMemoryPressure` [D]; OCaml `caml_alloc_custom_mem` with `custom_major_ratio` 44% [S `gc.mli:212-223`]; Chez `make-phantom-bytevector` [D `smgmt.stex:1147-1157`]; Racket `make-phantom-bytes` (raises out-of-memory if implausible or over a custodian limit) [D]; Erlang `include_shared_binaries` [D]. Go excludes non-Go memory as an explicit non-goal [D 48409] | moves the trigger; rarely counted in the hard limit |
| **Progress (death-spiral guard)** | GC time and yield over consecutive collections | HotSpot `UseGCOverheadLimit` (default on in product builds): `GCTimeLimit` 98%, `GCHeapFreeLimit` 2%, 5 consecutive GCs, Parallel and now G1 (JDK-8212084, JDK 26) [S `gc_globals.hpp:348-360`, `g1CollectedHeap.cpp:989-1014`]; V8: heap ≥ 0.95 of max and mutator utilization < 0.4 for 4 consecutive mark-compacts [S `heap.cc:3570-3611`, `flag-definitions.h:2600-2614`]; Go CPU limiter, a leaky bucket of GC time drained by mutator time, about 50% over a 1 CPU-second-per-P bucket [S `mgclimit.go:9-28,313`]; Netflix jvmquake (an external agent: GC-over-runtime deficit, then OOM, a core dump or kill) [D] | OOME, fatal, or *give up memory* (Go) |

---

## 5. Behaviour at the limit

### 5.1 The collect-and-retry ladder

| Runtime | Rungs before failure | Final rung | Exit status if uncaught |
|---|---|---|---|
| HotSpot G1 | young GC → full GC → full GC clearing all soft references with maximal compaction → allocate without GC; then OOME, and if the overhead counter reached 5, log "GC Overhead Limit exceeded too often" [S `g1CollectedHeap.cpp:1067-1115`] | `OutOfMemoryError` (catchable); `-XX:+ExitOnOutOfMemoryError` exits 3, `CrashOnOutOfMemoryError` aborts with a core and error log, `HeapDumpOnOutOfMemoryError` [S `debug.cpp:288-291`, `globals.hpp:557,869-875`] | 1 (uncaught exception in main), 3 with `ExitOnOutOfMemoryError` |
| V8 | collect and retry; young allocations get a "light" full GC; then `CollectAllAvailableGarbage` (2–7 GCs until the root set is stable); `CheckHeapLimitReached` calls the near-limit callback, which may *raise* the limit [S `heap-allocator.cc:510-538`, `heap.cc:1295-1300,1669-1681,4139-4157`] | `FatalProcessOutOfMemory` ("Reached heap limit", "Ineffective mark-compacts near heap limit"); embedders get `SetOOMErrorHandler` | Node: **134** (SIGABRT), "FATAL ERROR: Reached heap limit Allocation failed - JavaScript heap out of memory" at `--max-old-space-size=64` [P] |
| Go (soft limit) | the GC runs more often down to the live heap; the CPU limiter caps GC at about 50%; the program keeps running over the limit [D gc-guide] | real exhaustion: `fatal error: runtime: out of memory` | **2** (`panic.go:1517-1529`) [S] |
| .NET | at the hard limit the commit fails; full compacting GCs are favoured near `GCHighMemPercent` [D]; the exact retry ladder was not read in source | `OutOfMemoryException` (catchable) | non-zero |
| Lua | `tryagain`: one emergency full GC (`luaC_fullgc(L, 1)`) unless the collector is not in a complete state, then the allocation is retried [S `lmem.c:56-58,158-170,201-215`] | `LUA_ERRMEM`, with the message handler *not called* [D 5.5 manual §4.4.1] | host-defined (`lua` CLI: 1) |
| OCaml | — | `Out_of_memory` exception, documented as "not reliable for allocations on the minor heap" [S `stdlib.mli:80-83`] | 2 (uncaught exception) |
| Gambit | GC; grow up to `max-heap` | `heap-overflow-exception` (catchable); "Heap overflow" fatal when no room remains for it [D `gambit.txi:11576-11598`; S `mem.c:3040-3060`] | non-zero |
| Racket CS | a custodian over its limit is shut down after a GC; single huge requests raise first (`guard-large-allocation` checks the request before allocating) [S `memory.ss:223-240`; D] | `exn:fail:out-of-memory` or custodian shutdown | 1 |
| Erlang | per process: the GC that detects the excess is abandoned and the process exits with `kill` [D] | process death plus a logger event; VM-wide exhaustion: crash dump with slogan "Cannot allocate N bytes of memory (of type "heap")" [D `crash_dump.md:83-87`] | VM exits on a crash dump |
| Chez | no heap limit; the allocator requests segments until the OS refuses | prints `out of memory` and calls `S_abnormal_exit` [S `c/segment.c:83-86`] | non-zero |
| Whippet | collect; grow if the policy allows; "progress" means allocation since the last GC exceeded fragmentation; a fixed heap without progress fails [S `mmc.c:605-643`] | embedder's `gc_heap_set_allocation_failure_handler` [S `api/gc-api.h:46-48`, `mmc.c:1062,1096`] | embedder-defined |
| chibi | the allocator itself collects, grows if the max allows, retries; on failure returns a preallocated OOM exception [S `gc.c:715-735`] | at `-h 16M/128M` (or 32M/128M) with a growing live set: **no termination**, 100% CPU, RSS flat at 256 MB, killed at 30 s [P] | 137 (killed) |
| Gauche (Boehm) | Boehm GC prints "Out of Memory! Trying to continue..." 6 times, then "Returning NULL!" | `out of memory (528).  aborting...` [P, `GC_MAXIMUM_HEAP_SIZE=128M`] | **1** [P] |
| **Patina (design §5)** | user-sized requests: `try_alloc` → `Step::CollectAndRetry` (one full major) → retry; small allocations dip into a 4 MiB emergency reserve and post `HEAP_EXHAUSTED`; the next poll runs a major | catchable `&heap-exhausted`; abort after flushing ports if the reserve runs out before a poll | non-zero |

### 5.2 Patterns [I]

- **A second, more aggressive rung.** G1 and V8 both retry with a *stronger* collection (clear soft
  references, maximal compaction, drop caches, iterate until the roots are stable) before failing. Patina's
  equivalent would be a major that also flushes droppable caches (expansion memos, the code cache of
  unreachable units, string-port slack) and runs compaction once evacuation exists (stage 8).
- **The handler must not need what ran out.** Lua does not run the message handler on `LUA_ERRMEM`. Guile
  raises an unwind-only exception when the stack cannot grow. Racket raises before allocating. Patina's
  reserve-based design is sound if raising `&heap-exhausted` and running a handler fit in the reserve; a test
  should prove it (§9).
- **Embedders can move the limit.** V8's `AddNearHeapLimitCallback` returns a new limit, and
  `AutomaticallyRestoreInitialHeapLimit(0.5)` restores the original once usage falls below half of it.
  Whippet hands failure to the embedder. Go's limit can be changed at run time. Patina's `HeapConfig::max_heap`
  is fixed at creation.

---

## 6. Stack limits

| Runtime | Model | Default limit | Overflow behaviour | Source |
|---|---|---|---|---|
| Chez | segmented: on overflow, the stack is split at a frame boundary into a continuation object and execution continues on a fresh stack segment (default size 4 heap segments less two words) | memory only | heap exhaustion: `out of memory` abort | [S `c/schsig.c:231-260`, `s/cmacros.ss:2163-2178`]; 100 M-deep non-tail recursion: 1.1 GB, 0.9 s [P] |
| Racket CS | inherits Chez | memory only; the guide says there is no such thing as stack overflow | out of memory | [D `guide/lists.scrbl:299-303`] |
| Gambit | frames live in heap sections; overflow moves them to the heap | the heap limit | `stack-overflow-exception` when a frame allocation would exceed the heap (catchable) | [D `gambit.txi:11602-11606`] |
| Guile 3 | one page at first, doubled on demand; at GC the unused part is returned to the OS without shrinking the stack | memory only; `call-with-stack-overflow-handler` imposes an artificial limit in words for a dynamic extent, and nested handlers can only credit what was available to them | if the stack cannot grow: an unwind-only exception (pre-unwind handlers not run) with a console message; C stack: an error at 80% of the rlimit or 160 K words | [D `api-debug.texi:629-800`] |
| Erlang | stack and heap share one block per process | `max_heap_size` (counts the stack) | process killed | [D] |
| Gauche | VM stack spills to heap continuations | memory only | heap exhaustion | 10 M-deep recursion: 1.07 GB, exit 0 [P] |
| Go | contiguous, copied when doubled from 2 KiB (`stackMin`); halved at GC when less than ¼ is used | 1 GB per goroutine on 64-bit (`SetMaxStack`) | `throw("stack overflow")`: fatal, exit 2, not recoverable | [S `stack.go:78,1200-1208,1286-1330`, `debug/garbage.go:105-119`] |
| OCaml 5 | fibers start small (`Stack_init_bsize`) and double on demand | `stack_limit` 128 M words (1 GiB) | `Stack_overflow` exception | [S `config.h:167-177,187-189`, `fiber.c:546-552`, `gc.mli:183-185`] |
| HotSpot | fixed OS thread stacks with guard zones: yellow (recoverable), red (fatal), reserved (JEP 270, for critical sections), shadow | `-Xss` 1 MiB (linux-x64), 2 MiB (bsd-aarch64) | `StackOverflowError` (catchable) | [S `globals.hpp:1462-1489`, `globals_linux_x86.hpp:32`, `globals_bsd_aarch64.hpp`] |
| V8 | native stack, limit checked at function entry (the same check carries interrupts) | `--stack-size` 984 KiB (64-bit), so about 7,800 frames of a 1-argument JS function [P] | `RangeError: Maximum call stack size exceeded` (catchable) [P]; `--stack-size` beyond the OS stack crashed with **SIGSEGV, exit 139** [P] | [S `globals.h`, `flag-definitions.h:3189`] |
| JSC | native stack | `maxPerThreadStackUsage` 5 MiB; `reservedZoneSize` 64 KiB guaranteed to clients; `softReservedZoneSize` 128 KiB to stringify the exception | `RangeError` | [S `OptionsList.h:93-95`] |
| .NET | native stack | OS default | **process terminated**; `StackOverflowException` cannot be caught; only a CLR host can choose to unload the app domain instead | [D API docs] |
| Lua | heap-allocated value stack, grown 1.5× | `LUAI_MAXSTACK` 1,000,000 slots; C calls `LUAI_MAXCCALLS` | "stack overflow" error (catchable), raised after growing to `ERRORSTACKSIZE` so the handler has room; `LUA_ERRERR` if the handler overflows too | [S `ldo.c:186-211,361-387`] |
| chibi | heap-allocated VM stack | `SEXP_MAX_STACK_SIZE` = 8192·1000 words | "out of stack space" (a preallocated exception); 10⁵ deep succeeds, 10⁶ fails; **not caught** by `with-exception-handler` in our probe; exit 70 [P] | [S `features.h:874-880`, `sexp.c:566`] |
| **Patina today** | VM frames in `Vec`s; no limit (`VmError::StackOverflow` exists but nothing raises it) | memory only | 10 M-deep recursion: 1.37 GB, exit 0 [P] | understand/vm-runtime.md §0 |
| **Patina (design §8.1)** | one non-relocating reservation per green thread, committed on demand; pages above `top` decommitted after 2 observations | 8 GiB main (about 60 M frames), 256 MiB others; `--stack-max` | catchable `&stack-exhausted` within 1 MiB of the cap; uncaught → non-zero exit | DESIGN §8.1, H.3 |

**[I]** Patina's design belongs to the "stack is memory" camp in effect (an 8 GiB cap is about 60 M frames,
beyond any oracle's practical depth) while keeping a VA cap. Two prior-art points are not yet in the design:
Guile's per-extent artificial limit (useful for a REPL), and Gambit's choice to count stack bytes against the
heap limit, as Erlang and Go's memory limit also do. Separate budgets are simpler and match the JVM, V8, .NET
and OCaml. The PRD should state which one Patina chose (§10.4).

---

## 7. Knobs and embedding APIs

| Runtime | Process knobs | Programmatic or embedding API |
|---|---|---|
| Go | `GOGC`, `GOMEMLIMIT`, `GODEBUG=gctrace=1,gcpacertrace=1` | `debug.SetGCPercent`, `SetMemoryLimit`, `SetMaxStack`, `SetMaxThreads`, `FreeOSMemory`; `runtime/metrics` (`/gc/throttles:events` was proposed as the death-spiral indicator [D 48409 §Telemetry]) |
| HotSpot | `-Xms/-Xmx`, `MaxRAMPercentage`, `SoftMaxHeapSize` (manageable), `GCTimeRatio`, `MaxGCPauseMillis`, `Min/MaxHeapFreeRatio` (manageable), `G1PeriodicGCInterval` (manageable), `ZUncommitDelay`, `UseGCOverheadLimit`/`GCTimeLimit`/`GCHeapFreeLimit`, `-Xss`, `Exit/CrashOn/HeapDumpOnOutOfMemoryError`, `-Xlog:gc*` | JMX memory-pool usage thresholds and notifications; manageable flags settable at run time |
| V8 | `--max-old-space-size`, `--max-semi-space-size`, `--stack-size`, `--heap-growing-percent`, `--sqrt-allocation-limits`, `--trace-gc`, `--heap-snapshot-on-oom` | `ResourceConstraints`, `AddNearHeapLimitCallback`, `AutomaticallyRestoreInitialHeapLimit`, `SetOOMErrorHandler`, `ExternalMemoryAccounter`, `MemoryPressureNotification`, `LowMemoryNotification`, `RetryCustomAllocate`, `MeasureMemory` |
| .NET | `GCHeapHardLimit(Percent)`, per-object-heap limits, `GCConserveMemory`, `GCHighMemPercent`, `GCRetainVM`, `GCDynamicAdaptationMode`, `GCDTargetTCP` (2%), `GCDGen0Growth*` | `GC.AddMemoryPressure/RemoveMemoryPressure`, `GC.GetGCMemoryInfo`, `GC.RegisterForFullGCNotification`; settings are read only at GC initialization [D] |
| Chez | — | `collect-request-handler` (the whole policy is replaceable Scheme; `void` disables collection), `collect-trip-bytes`, `collect-generation-radix`, `collect-maximum-generation-threshold-factor`, `heap-reserve-ratio`, `release-minimum-generation`, `in-place-minimum-generation`, `bytes-allocated`, `current-memory-bytes`, `maximum-memory-bytes`, `sstats`, phantom bytevectors |
| Racket | `PLT_INCREMENTAL_GC` | `custodian-limit-memory`, `custodian-require-memory`, `current-memory-use` (`'peak`, `'cumulative`, per custodian), `make-phantom-bytes`, `collect-garbage` (`'minor`, `'major`, `'incremental`), GC logging |
| Gambit | `-:min-heap=`, `-:max-heap=`, `-:live-ratio=` | `adjust_heap_hook` (a C hook that replaces the sizing rule [S `mem.c:4513-4514`]), `heap-overflow-exception?`, `stack-overflow-exception?` |
| Whippet/Guile | `GC_OPTIONS` / `GUILE_GC_OPTIONS`: `heap-size-policy` (fixed, growable, adaptive; fixed by default), `heap-size` (6 MiB), `maximum-heap-size`, `heap-double-threshold` (60 MiB), `heap-expansiveness` (1.0), `parallelism` | `gc_options_*`, `gc_heap_set_allocation_failure_handler`, event listeners and tracepoints; Guile `call-with-stack-overflow-handler`, `(debug-set! stack N)` for the C stack |
| OCaml | `OCAMLRUNPARAM` `o` (space_overhead), `O` (max_overhead), `l` (stack_limit), `M`/`m` (custom ratios) | `Gc.set`, `Gc.compact`, `Gc.quick_stat`, `caml_alloc_custom_mem` |
| Lua | — | `lua_newstate(lua_Alloc)` (the allocator *is* the limit: return NULL), `collectgarbage("incremental", pause, stepmul, stepsize)`, `"generational"`, `"count"`, `"step"`; test builds: `MEMLIMIT`, `EMERGENCYGCTESTS` [S `ltests.c:215-241`, `lmem.c:63-75`] |
| Erlang | `+hmax`, `+hmaxk`, `+hmaxel`, `+hmaxib`, `+hms` (min heap), `fullsweep_after` | `process_flag(max_heap_size, …)`, `spawn_opt`, `erlang:system_flag`, `process_info(total_heap_size)` |
| **Patina (design)** | `PATINA_HEAP_MAX`/`--heap-max`, `PATINA_STACK_MAX`/`--stack-max`, `PATINA_GC_PACING=adaptive`, `PATINA_GC_LOG`, `PATINA_GC_TRACE` | `HeapConfig::max_heap`, `(gc-stats)` keys, `interrupt_handle()` |

---

## 8. The literature on steady state

- **Time-space tradeoff.** Appel (IPL 1987) showed that a copying collector's cost per allocated word scales
  with L/(H − L), so large heaps make collection cheap. Hertz and Berger (OOPSLA 2005) measured the curve
  against explicit management: equal at 5×, 17% slower at 3×, about 70% slower at 2× [D]. DaCapo Chopin
  (ASPLOS 2025) renews the advice. **H1:** evaluate collectors across a range of heap sizes. **H2:** express
  heap sizes as multiples of the minimum heap in which a baseline collector runs the workload (the paper's
  figures use 1–6×). It also notes that the minimum heap measures *peak* use, so footprint is better measured
  as area under the memory curve [D].
- **Lower-bound overhead (LBO)** (Cai et al., ISPASS 2022; used in Chopin §6.2): distil a baseline by
  subtracting attributable stop-the-world time and taking the cheapest application cost over all collectors
  and heap sizes; report each configuration over that baseline. It captures barrier and allocation costs that
  pause logs miss [D]. Patina's `PATINA_GC=null` lower bound is the same idea for one collector.
- **MMU** (Cheng and Blelloch, PLDI 2001): the minimum mutator fraction over any window of width w, a rolling
  guarantee. Chopin adds *metered latency* (request start times smoothed over a 100 ms window) and
  recommends user-experienced latency over GC pauses as a proxy (L1) [D]. Patina's design already uses
  MMU(1–100 ms) from Larceny's `gc_mmu_log.c`.
- **The square-root rule** (Kirisame, Shenoy, Panchekha, OOPSLA 2022, arXiv 2204.10455): extra memory
  proportional to √(L·g/s) minimizes total memory for a given total GC time and composes across heaps
  without communication; about 16% less memory at constant GC time, up to 30% less GC time on
  memory-intensive benchmarks [D]. Implemented in Whippet adaptive, V8 (flag-gated) and, as a fixed-constant
  special case, Racket CS and Whippet growable [S].
- **Livelock under a multiplier** (Wingo, 2025-05-22): a 10 MB heap with a 2× multiplier, holding alternating
  16-byte objects and 16-byte holes, cannot place a 32-byte object; the sizing rule refuses to grow, because
  the heap is already 2× live, and every collection sweeps in vain. Fixes: keep empty blocks after a
  collection whatever the multiplier says; allow overflow allocation; grow on lack of progress (Whippet
  defines progress as allocation since the last GC exceeding fragmentation [S `mmc.c:605-609`]). Wingo also
  notes that growth in response to fragmentation is not deterministic once threads are involved [D].
- **Three heap-sizing policies** (Wingo, 2023-01-27): fixed (for benchmarking; bisect the minimum, then use
  1.5–5×), growable (never shrink), adaptive (MemBalancer). Shrinking is never necessary and is sometimes
  expensive; growing is necessary and cheap [D].
- **Leak detection as steady-state violation.** Cork (Jump and McKinley, POPL 2007) detects systematic heap
  growth per type by differencing type points-from graphs across full collections, with a decaying ratio rank
  to damp fluctuations; under 1% space and 2.3% time [D].

---

## 9. Test methodologies

### 9.1 Catalogue

| Method | Prior art | What it asserts | Patina analogue |
|---|---|---|---|
| **Step test against a limit** | Go `TestMemoryLimit` (`testdata/testprog/gc.go:325-410`): set a 256 MiB limit, step the live target 10%, 20%, …, 80% of it, sample `total − released` for 200 ms per step, fail above limit + 16 MiB (+48 MiB on Darwin); also with `GOGC=off`; skipped below 4 CPUs and in `-short` [S] | footprint stays under the limit across live sizes | step L to 10–80% of `--heap-max`; assert `committed-bytes` ≤ `max_heap`, and no `&heap-exhausted` until L passes the documented threshold |
| **Return to baseline, both bounds** | ZGC `TestUncommit`: `-Xms128M -Xmx512M -XX:ZUncommitDelay=5`; allocate 200 MiB as 4 KiB, 2 MiB and 200 MiB objects; drop them, `System.gc()`; assert uncommit started no earlier than the delay, warn above 3× and fail above 5× it, and committed memory returns to *exactly* the pre-allocation value (fails on too much or too little); two iterations [S] | memory comes back; hysteresis honoured | count majors, not seconds: after a spike, committed returns to the baseline within 2 majors + 1 observation, not before 2 majors, for small, medium and LOS sizes; twice |
| **Overhead-limit test** | HotSpot `TestUseGCOverheadLimit`: `-Xmx128m`, 1 GC thread, `GCTimeLimit=80`, a live-heavy cache churn; assert a **non-zero exit** and the log line "GC Overhead Limit exceeded too often (5)." for Parallel and G1 [S] | a death spiral terminates, with a diagnosable message | near-ceiling churn under `--heap-max`; assert `&heap-exhausted` (or a non-zero exit) within N majors, never a hang (chibi hangs, §5.1) |
| **Allocation-failure zeal** | Lua `EMERGENCYGCTESTS`: every first allocation attempt fails, so every allocation runs the emergency full GC; `MEMLIMIT` caps the test allocator so tests can probe each out-of-memory point [S `lmem.c:63-75`, `ltests.c:215-241`] | every OOM point is recoverable and leaves consistent state | a zeal mode in which `try_alloc` fails first, exercising `Step::CollectAndRetry` at every user-sized primitive; sweep `--heap-max` downwards over the chibi suite, expecting either the normal output or a clean `&heap-exhausted` |
| **Minimum-heap bisection, evaluation at multiples** | DaCapo Chopin H1/H2 (minimum heap per benchmark; 1–6× in its figures); Wingo's fixed-heap bisection [D] | the time-space curve, not one point | bisect the smallest `--heap-max` at which each GBS workload completes; report cycles and RSS at 1.5×, 2×, 3× and 6× that minimum |
| **MMU and metered latency** | Cheng and Blelloch; Larceny `gc_mmu_log.c`; Chopin L1/L2 [D] | rolling pause guarantee | already in the design (§13, K1) |
| **Slope test** | Cork's heap-growth ranking [D]; Racket and Chez report `bytes-allocated`/`current-memory-use` for the same purpose | zero growth after warm-up | sample `(gc-stats)` after each major; regress `committed-bytes` and `live-bytes` on iteration count, excluding the first k majors; assert a slope CI including 0 or ≤ ε bytes per iteration |
| **Determinism replay** | — (Patina-specific; DATAS's "same work, same heap" is the cross-machine version) | pacing depends on bytes, never time or RAM | two runs (and two machines in CI) produce the same sequence of collection kinds and the same `live-bytes` per major |
| **Soak** | jvmquake's motivating incidents; Go's 48409 motivation (OOMs from transient spikes in long-running services) [D] | no drift over hours | a nightly REPL-like loop (load libraries, evaluate, discard environments) run for hours; slope test on its samples |

### 9.2 A cross-implementation slope probe [P]

`probes/churn.scm`: a 100,000-slot vector holds 10-element lists (about 1 M live pairs); each iteration builds a
fresh list into slot `i mod 100000`, so the allocation rate per iteration and the live size are constant.
Peak RSS by iteration count, one run each:

| Implementation | 1 M | 4 M | 16 M | 64 M | 256 M |
|---|---|---|---|---|---|
| Patina (release binary in `target/`, built 2026-09-30) | 105 MB | 104 MB | 105 MB | 109 MB | — |
| chibi 0.12 | 92 MB | 92 MB | 92 MB | — | — |
| Gauche 0.9.15 | 71 MB | 71 MB | 71 MB | — | — |
| Chez 10.3 | 102 MB | 110 MB | 124 MB | 124 MB | 124 MB |

- Chez's rise is warm-up, not a leak: its max generation is collected only once the bytes allocated have
  doubled since the last max-generation collection (§2), so the plateau arrives after about 16 M iterations
  and then holds for 16× more work. A slope test that started measuring at 1 M iterations would report a
  false leak. **The warm-up must be defined in majors, and excluded.**
- Patina's +4 MB between 16 M and 64 M iterations is within what a single peak-RSS sample can show; an
  in-process slope over `(gc-stats)` would settle it. Peak RSS is the wrong instrument for a slope test; the
  contract should be stated in the runtime's own counters, as Go's test does (`total − released`).
- Live data here is about 16 MB in a 16-byte-pair layout. The 4–7× ratios measure each implementation's
  representation, not its pacing.

### 9.3 Oracle probes at the limit [P]

| Probe | chibi 0.12 | Gauche 0.9.15 | Chez 10.3 | Node 22.17 (V8) | Patina today |
|---|---|---|---|---|---|
| non-tail recursion, depth 10⁶ / 10⁷ (`deep.scm`, handler installed) | "out of stack space", **not caught**, exit 70 / same | 132 MB, exit 0 / 1.07 GB, exit 0 | (10⁷ and 10⁸) 129 MB / 1.1 GB, exit 0 | `RangeError` caught at about 7,800 frames | 151 MB / 1.37 GB, exit 0 |
| growing live set under a heap cap (`heap2.scm`) | `-h 16M/128M`: no termination in 30 s at 100% CPU, RSS flat at 256 MB, exit 137 | `GC_MAXIMUM_HEAP_SIZE=128M`: Boehm warnings, then `out of memory (528). aborting...`, exit 1, not catchable | no cap knob | `--max-old-space-size=64`: fatal "Reached heap limit", exit 134, not catchable | no cap (design: `&heap-exhausted`) |

**[I]** Neither Scheme oracle makes heap exhaustion catchable: chibi thrashes and Gauche aborts. A catchable
`&heap-exhausted` is therefore Patina's own semantics, followed by Gambit, Racket, the JVM and OCaml but not
by chibi or Gauche. The test suite should treat it like port finalization: outside the byte-identical lanes,
with a `DIVERGENCES.tsv` row if a suite file exercises it.

---

## 10. What this means for Patina

### 10.1 A steady-state contract (proposal [I])

**Precondition (after Go).** A program phase is *steady* when, over a window of W allocated bytes after
warm-up, (a) live bytes after each major stay within [L_lo, L_hi] with L_hi ≤ (1 + δ)·L_lo, and (b)
allocation per unit of work is constant. Warm-up is the first k majors of the phase (k = 3 proposed; Chez's
probe shows why it is needed). Units are bytes, majors and work units, never seconds, because Patina's pacing
is time-free (§13).

**Guarantees.** Under the precondition, with L = L_hi:

| # | Clause | Statement | Test (§9) |
|---|---|---|---|
| SS1 | Bounded footprint | at every major, committed heap ≤ F(L) = L + max(8 MiB, 2·L) + free reserve + fragmentation allowance (K3's 10% before stage 8); resident ≤ committed | step test; the GBS with `GcStats` |
| SS2 | Zero slope | the regression of `committed-bytes` and of `live-bytes` on iteration count has a 95% CI containing 0, or a slope ≤ ε (ε = 1 KiB per iteration proposed) | slope test; soak |
| SS3 | Constant GC cost | GC CPU per allocated MiB within ±10% across the phase; MMU(10 ms) ≥ the per-workload floor recorded at stage 0 | `GcStats`; MMU log |
| SS4 | Return to baseline | when a phase ends with live size L' < L, committed returns to ≤ F(L') + reserve after 2 majors and one decommit pass, and not before 2 majors (the hysteresis); the same holds for register stacks after 2 observations | return-to-baseline test, small, medium and LOS sizes, twice |
| SS5 | Portability and determinism | the sequence of collection kinds and `live-bytes` per major is identical across runs and machines for the same program and inputs (DATAS's "same work, same heap") | determinism replay in CI on Linux and macOS |
| SS6 | Ceiling behaviour | when F(L) exceeds `max_heap` − reserve, collections become more frequent (the target clamps), then the progress guard fires: a catchable `&heap-exhausted`, a non-zero exit if uncaught; never a livelock, never a silent hang | overhead-limit test; allocation-failure zeal |
| SS7 | Stack | committed stack ≤ deepest depth × frame size, returned after 2 observations; `&stack-exhausted` within 1 MiB of `--stack-max`, with room for the handler | deep-descent and deep-unwind probes; a handler-room test |

**Exclusions** (stated, not hidden): a phase holding an unbounded number of continuations, ports or
blocked threads is not steady by definition, since its L grows; `NoGcScope` windows (K16) may overshoot
F(L) by at most K16's bound; tree-walker heaps meet SS1–SS4 with whole-heap rules only.

### 10.2 What `max_heap` counts (gap)

The prior art differs: HotSpot counts the Java heap only; .NET counts heap commit *plus GC bookkeeping*; Go
counts everything the runtime maps less what it has released, stacks included; V8 counts old-generation
object bytes, with an optional global limit that includes external memory; Gambit and Erlang count stacks in
the heap. Design §4 sizes one VA reservation per heap from `max_heap`. **Proposal [I]:** `max_heap` bounds
committed heap blocks, the LOS and their side metadata (the .NET definition). Register stacks (`--stack-max`),
store buffers (a 256 MiB VA cap), JIT code and external bytes are bounded separately; external bytes move the
pacing trigger (§13) but not the hard limit, as in V8's default. The PRD should say this in one line, because
it determines what SS1 measures.

### 10.3 Limits the design lacks (gaps, each a candidate issue)

1. **A progress guard (death-spiral rule).** Deterministic, in majors and bytes (HotSpot's form without the
   time term): when the pacing target is clamped at `max_heap` − reserve, count consecutive majors whose freed
   bytes are under 2% of `max_heap`; at 5, raise `&heap-exhausted` at the next poll instead of collecting
   again. It complements Wingo's livelock fix, which applies below the ceiling. Time-based variants (V8's
   mutator utilization, Go's CPU bucket, HotSpot's 98%) would break SS5 and belong only in an opt-in mode.
2. **A stronger last rung.** Before raising, one "maximal" major that also drops droppable caches (expansion
   memos, unreachable code units, string-port slack) and, from stage 8, evacuates (G1's soft-reference and
   maximal-compaction rung; V8's last resort).
3. **Reject implausible requests first.** A user-sized request larger than `max_heap` − L should raise at
   once, without a futile major (Racket's `guard-large-allocation`, V8's `RetryCustomAllocate` contract).
4. **An optional soft target in bytes** (`--heap-soft-max`, HotSpot's `SoftMaxHeapSize`): it clamps the pacing
   target, so collections become more frequent, but never fails. Deterministic, unlike `GOMEMLIMIT`, whose
   CPU limiter depends on time.
5. **Embedder hooks** (decision 13): a near-limit callback that may raise `max_heap` (V8
   `AddNearHeapLimitCallback`, restoring the initial limit below 50%); an external-memory accounter for host
   payloads (V8 `ExternalMemoryAccounter`, .NET `AddMemoryPressure`, Chez phantom bytevectors); a
   memory-pressure notification that requests a major plus decommit at the next poll (V8
   `MemoryPressureNotification`). A many-heaps process (318 heaps in the K13 probe) is exactly where V8
   needed these.
6. **Handler room proven by a test.** A test that a handler for `&heap-exhausted` (and for
   `&stack-exhausted`) can allocate a condition, format a message and unwind using only the reserve. If it
   cannot, adopt Lua's and Guile's fallback of unwinding without running the handler instead of aborting.
7. **A per-extent stack limit for the REPL** (Guile's `call-with-stack-overflow-handler`): optional, after
   stage 4d; it turns runaway recursion into an error without lowering the 8 GiB default.
8. **A stated exit status for resource exhaustion.** Prior art: Go 2, HotSpot 3 (`ExitOnOutOfMemoryError`),
   chibi 70 (EX_SOFTWARE), V8 134 (abort), Gauche 1. Patina's rule is "non-zero"; a distinct code would let
   CI and embedders tell exhaustion from ordinary errors. The owner's exit-status rule allows either.

### 10.4 Decisions for the PRD (owner)

| Question | Options from the prior art | Lean [I] |
|---|---|---|
| Is the steady-state contract SS1–SS7 part of the PRD's acceptance criteria? | none of the surveyed runtimes publishes one; Go and ZGC test fragments of it | yes, as stage-gated clauses: SS1, SS2 and SS5 from stage 1 (byte pacing), SS4 and SS7 from stage 5 (decommit), SS6 from stage 5 (`max_heap`) |
| Does the stack count against `max_heap`? | one budget: Gambit, Erlang, Go's limit; separate: JVM, V8, .NET, OCaml | separate (the design's choice); document it |
| Progress guard: time-based or byte-based? | HotSpot (time and free %), V8 (utilization), Go (CPU) | byte- and major-based, to keep SS5 |
| Soft limit? | Go `GOMEMLIMIT` (a total-memory target with a CPU limiter), HotSpot `SoftMaxHeapSize` (a heap target) | a byte target only, opt-in |
| Catchable exhaustion, given that chibi and Gauche do not offer it? | JVM, .NET, OCaml, Gambit, Racket, Lua: catchable; V8, Gauche, Chez: fatal | catchable, outside the byte-identical lanes |

---

## 11. Sources and pinned revisions

Fetched copies were not retained; local checkouts are in `~/Project/reference/`.

- **Go** master `e873c5e` (2026-09-17): `src/runtime/{mgclimit.go, mgcscavenge.go, stack.go, proc.go,
  panic.go, gc_test.go, testdata/testprog/gc.go}`, `src/runtime/debug/garbage.go`; `mgcpacer.go` from the
  research's fetched copy (2026-09-30, not retained). https://go.dev/doc/gc-guide;
  https://github.com/golang/proposal/blob/master/design/48409-soft-memory-limit.md
- **OpenJDK** master `46fbea9` (2026-10-01): `src/hotspot/share/gc/shared/gc_globals.hpp`,
  `gc/g1/{g1_globals.hpp, g1Arguments.cpp, g1HeapSizingPolicy.cpp, g1CollectedHeap.cpp}`,
  `gc/z/z_globals.hpp`, `runtime/globals.hpp`, `utilities/debug.cpp`,
  `os_cpu/{linux_x86,bsd_aarch64}/globals_*.hpp`; tests `test/hotspot/jtreg/gc/{TestUseGCOverheadLimit.java,
  z/TestUncommit.java, g1/TestPeriodicCollection.java}`. https://openjdk.org/jeps/346; JDK-8222145
  (SoftMaxHeapSize); JDK-8212084 and release note JDK-8370491 (G1 overhead limit, JDK 26); JEP draft 8359211
  (G1 automatic heap sizing, submitted 2025-06-11, updated 2026-04-06);
  https://docs.oracle.com/en/java/javase/25/gctuning/ergonomics.html;
  https://github.com/Netflix-Skunkworks/jvmquake
- **V8** main `327c4ad` (2026-09-28): `include/{v8-isolate.h, v8-callbacks.h,
  v8-external-memory-accounter.h}`, `src/heap/{heap.cc, heap.h, heap-controller.{h,cc}, memory-reducer.{h,cc},
  heap-allocator.cc}`, `src/flags/flag-definitions.h`, `src/common/globals.h`
- **JavaScriptCore**: WebKit main as fetched 2026-10-01: `Source/JavaScriptCore/heap/Heap.cpp`,
  `runtime/OptionsList.h`
- **.NET**: https://learn.microsoft.com/en-us/dotnet/core/runtime-config/garbage-collector (updated
  2026-02-09); https://learn.microsoft.com/en-us/dotnet/standard/garbage-collection/datas;
  https://learn.microsoft.com/en-us/dotnet/api/system.stackoverflowexception
- **Chez Scheme** `7d82bd86` (2026-09-05): `s/7.ss`, `s/back.ss`, `s/cmacros.ss`, `c/segment.c`, `c/schsig.c`,
  `csug/smgmt.stex`
- **Racket** `50f1f60628` (2026-08-29): `racket/src/cs/rumble/memory.ss`;
  `pkgs/racket-doc/scribblings/{reference/custodians.scrbl, reference/eval-model.scrbl, reference/memory.scrbl,
  guide/lists.scrbl}`
- **Gambit** `ab6f255c` (2026-08-27): `lib/mem.c`, `lib/mem.h`, `lib/_kernel.scm`, `doc/gambit.txi`
- **Whippet** `f9b54bb` (2026-03-18; the research's fetched copy of `growable-heap-sizer.h` is
  identical): `src/{growable-heap-sizer.h, adaptive-heap-sizer.h, heap-sizer.h, mmc.c, gc-options.c}`,
  `api/gc-api.h`, `doc/manual.md`
- **Guile** manual source (Savannah, fetched 2026-10-01): `doc/ref/api-debug.texi` "Stack Overflow"
- **Wingo**: https://wingolog.org/archives/2023/01/27/three-approaches-to-heap-sizing;
  https://wingolog.org/archives/2025/05/22/whippet-lab-notebook-guile-heuristics-and-heap-growth;
  https://wingolog.org/archives/2024/09/18/whippet-progress-update-feature-complete
- **OCaml** `70165ff23e` (5.6.0+dev, 2026-09-04): `runtime/caml/config.h`, `runtime/fiber.c`,
  `stdlib/gc.mli`, `stdlib/stdlib.mli`
- **Lua** master (github.com/lua/lua, fetched 2026-10-01): `lmem.c`, `ldo.c`, `ltests.c`, `lgc.h`, plus
  `lgc.h` at v5.4.7; manuals https://www.lua.org/manual/5.4/manual.html and
  https://www.lua.org/manual/5.5/manual.html
- **Erlang/OTP** master (fetched 2026-10-01): `erts/preloaded/src/erlang.erl` (`process_flag/2` docs),
  `erts/emulator/internal_doc/GarbageCollection.md`, `erts/doc/guides/crash_dump.md`
- **chibi-scheme** `bb9b3215` (2026-09-09): `include/chibi/features.h`, `gc.c`, `sexp.c`, `main.c`
- **Gauche** `f582cf69e` (2026-09-08): bundled Boehm GC `gc/{alloc.c, allchblk.c, doc/README.environment}`
- **Papers**: Kirisame, Shenoy, Panchekha, "Optimal Heap Limits for Reducing Browser Memory Use",
  arXiv 2204.10455 (OOPSLA 2022); Cheng and Blelloch, "A Parallel, Real-Time Garbage Collector", PLDI 2001;
  Hertz and Berger, "Quantifying the Performance of Garbage Collection vs. Explicit Memory Management",
  OOPSLA 2005 (https://people.cs.umass.edu/~emery/pubs/gcvsmalloc.pdf); Blackburn et al., "Rethinking Java
  Performance Analysis", ASPLOS 2025 (https://www.dacapobench.org/assets/pdf/dacapo-asplos-2025-with-appendix.pdf);
  Cai et al., "Distilling the Real Cost of Production Garbage Collectors", ISPASS 2022; Jump and McKinley,
  "Cork: Dynamic Memory Leak Detection for Garbage-Collected Languages", POPL 2007; Appel, "Garbage
  Collection Can Be Faster Than Stack Allocation", IPL 25(4), 1987
- **Probes** (`PRD/study/gc/probes/followup/probes/`): `deep.scm`, `deep-chez.ss`, `heap.scm`, `heap2.scm`, `churn.scm`,
  `churn-chez.ss`; oracles chibi-scheme 0.12.0, Gauche 0.9.15, Chez 10.3.0, Node v22.17.0; Patina release
  binary `target/release/patina` (built 2026-09-30 21:56, not rebuilt for this note).
