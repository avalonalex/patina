## Steady state, the memory contract, and limits

A program with bounded live data should never hit a limit; one that really grows should hit one and be told what
grew. Numbers come from the steady-state audit and limits survey in `PRD/study/`; figures re-measured in review say
so. Patina fails steady state in eight measured ways today, and the design as first written would have made several
permanent.

### Definition

- A **cycle** is one iteration of what the program repeats: a request, a REPL form, an `eval`. A **phase** is a
  run of cycles doing the same work while the program's own live data (what its stacks, handles, ports, threads and
  nameable bindings reach) stays bounded. Lane programs bound it by construction.
- **L** is `live-bytes` after a major: marked bytes, plus LOS bytes, plus external bytes (§13). `PATINA_GC_LOG`
  records it at every major; a window's L is its maximum.
- **Footprint** is `committed-bytes` (committed data blocks and LOS runs; arena capacity before 5e) plus
  `external-bytes`. Metadata and the block table, 1/16 of the data, are reported apart as `metadata-bytes`;
  `max_heap` counts all three.
- **Warm-up** is the first N cycles of a phase, excluded from every comparison. N is set per row so that the plateau
  has begun (Chez's slope probe rises from 102 MB to 124 MB before a plateau that holds for 16× more work) and, for
  rows that test SS2 or SS3, so that warm-up has seen 3 paced majors.

A phase is in **steady state** when these clauses hold over the window from cycle N to 4N. Values are read at the
window's two ends right after two forced `(gc)` calls, which collect at their call from stage 1, so the readings are
deterministic.

| # | Clause | Test | Failing today |
|---|---|---|---|
| SS1 | L follows the program, not its history | L(4N) − L(N) ≤ 3N·8 B + 64 KiB | symbol churn: 192 B/symbol (chibi 166, Chez 0) |
| SS2 | Footprint ≤ F(L) | at every paced major in the window, footprint ≤ F(L) = 1.1·(L + max(8 MiB, 2·L)) + free reserve | `environment` loop: 972 MiB (chibi 8.7) |
| SS3 | Pause work stationary | in-pause work counters (frames scanned, bytes marked, root-region words, blocks touched): the second half's maximum ≤ 2× the first half's | `(gc)` after a dropped peak sweeps 2 M slots: 9.3 ms, against 0.095 before |
| SS4 | Operation cost flat | mutator CPU time per cycle over [3N, 4N) ≤ 1.5× that over [N, 2N), each segment the minimum of 3 runs | macro-introduced definitions: 15× the time for 4× the cycles |
| SS5 | Side structures bounded | each per-owner count in `(gc-stats)` at 4N within max(16, 5%) of N; byte-valued keys under SS1's bound | `symbols` +3 per top-level `guard` |

SS1's bound catches a 32 B-per-cycle leak (one symbol in the new heap) from N ≈ 1,000, and today's smallest class-A
leak (192 B) with 24× margin. SS4's bound sits between flat cost (1.0) and cost proportional to history (2.33, the
shape of every class-E row; `hidden-define` fits 2.0 at N = 2,000 [I]); timing one process's loop excludes
start-up and library load. In F(L), 1.1 is K3's pre-stage-8 fragmentation allowance and the free reserve is
max(8 blocks, 5%). A row that declares SS2 or SS3 needs 2 paced majors in each half-window, or it fails as vacuous,
as `bytes-reclaimed` > 0 guards the reclamation proofs.

**Phase change.** Suppose live size falls from L to L′, because a peak was dropped or a deep recursion returned:
- The first major after the drop does in-pause work within SS3's budget for L′.
- **Empty memory returns.** Runs of blocks and LOS runs that the drop left empty are decommitted within 2 majors,
  or at one idle major (below). Empty blocks inside runs that still hold a survivor are decommitted one by one, data
  pages only; their metadata pages stay. Register stacks follow after 2 observations. Footprint is then
  ≤ F(L′) + `sparse-bytes`, the committed bytes of non-empty blocks under 25% live.
- **Partly occupied blocks are reused, not returned, until stage 8.** A non-moving heap cannot give back a block that
  one survivor holds. From stage 8 a footprint trigger evacuates the sparsest unpinned blocks whenever
  footprint − F(L) exceeds max(64 MiB, 25%·F(L)) at 2 consecutive majors. The bound becomes F(L′) plus that excess
  plus pinned blocks.
- **Idle trim.** A drop is invisible until something marks, and a program that idles after a peak allocates nothing
  that would pace a major. Entering a wait therefore runs one **idle major** when footprint exceeds 64 MiB and the
  back-off allows. The idle major:
  - completes its post-pause work (catch-up sweep, classification, decommit) before the wait, without the per-poll
    rate limit and with the 2-major hysteresis waived;
  - recomputes the target from L′, as any major does;
  - neither counts toward nor resets the livelock boost or the progress guard.

  **Back-off.** At most one idle major runs per paced cycle. One that returns less than max(64 MiB, 25% of footprint)
  doubles the number of paced majors before the next (1, 2, 4, …). The count resets to 1 when an idle major returns
  that much, or when a paced major's L exceeds 1.5× the L at which the count last doubled. A program whose L never
  drops pays at most log₂ P + 1 useless idle majors over P paced ones.

  **Waits.** From 5e:
  - the REPL prompt;
  - `notify_idle()`, and `(notify-idle)` in `(patina debug)`;
  - entry to a read primitive whose buffer is empty, on a port that is not a regular file, through
    `Step::CollectAndRetry` before anything is consumed.

  From stage 9, a thread wait with nothing runnable, and a read that blocks mid-datum once the reader lexes before
  it builds. Where collection is deferred, no trim runs. Under `PATINA_DETERMINISTIC=1` only the prompt and
  `notify_idle` qualify, because whether a pipe's buffer is empty depends on how the writer's bytes arrive. G1 used a
  timer for this case (JEP 346); Patina counts waits instead.
- The livelock boost (×1.5 after two majors that each free under 1%) decays. At the first major that frees more
  than 10%, the target is recomputed from 2·L.

**Outside the contract:**
- growing name or live sets (class D: a fresh top-level name costs 365 B, oracles 400–484; the lane fails a 20%
  regression);
- threads blocked for ever (decision 23);
- partly occupied blocks before stage 8 (above);
- `NoGcScope` windows, which may overshoot F(L) by K16's bound.

### The memory contract

Five rules for `docs/GC_DESIGN.md`:
- **M1.** The immortal space holds only objects bounded by the binary and the program text: primitives and their
  descriptors, canonical flonum boxes, core syntax, and a future boot image. Everything created at run time is
  mortal. Objects that must not move go to a non-moving space that majors sweep and that is never evacuated.
- **M2.** Every Rust table keyed by name, id or address is bounded by the program text, or loses entries when their
  heap object dies (finalization or epilogue step 6); the holder inventory (S0) names each owner.
- **M3.** Rust bytes that a heap object keeps alive count as external bytes while the object lives: in L, in the
  trigger, and in `max_heap`.
- **M4.** No lookup scans a table that grows with history.
- **M5.** Empty memory returns. Empty runs within 2 majors (one at an idle trim), register-stack pages after 2
  observations. Partly occupied blocks are reused for allocation and, from stage 8, evacuated by the footprint
  trigger. Boosts to the pacing target decay.

| Source | Guarantee; design change | Today | Stage |
|---|---|---|---|
| Symbols | Reclaimed when unreferenced (SD2). Allocated old in the non-moving space, so only majors reclaim them and the interner is pruned only in the major epilogue (step 6). The stored hash is a fixed-seed hash of the UTF-8 name, so a re-interned symbol hashes the same. `identity-hash` of a symbol changes from the heap index to that hash, in its own PR, because it reorders symbol-keyed `eq?` tables. An ephemeron keeps a symbol key alive, so symbol-keyed weak tables answer as today (§6.6) | 192 B/symbol | 5c |
| Global cells and binding records | Owned by namespaces; global and library namespaces are roots. Placeholders are weak entries while their cell is still `UNBOUND` and no live link table holds their record. A variable alias stops being a name at 4b: the reference links the definition's own record. Keyword aliases are deduplicated by target (I1), which bounds them by program text. **Macro-introduced definitions stay strong at 4b**, with an indexed lookup (I3): a template can be their only reference (the `def-getter` shape below), and 4b cannot see templates. From 5c an entry is reclaimed only when no live link table references its record **and** no live identifier carries the expansion scope that introduced it; 4c's weak scope-set table answers the second test. Cells stop being a root region (§6.1, §6.10), so pauses stop growing with cell history. Variant C keeps this | 1.8–2.3 KiB per top-level `guard`; quadratic introduced definitions | I1 now; 4b own; 5c sweep |
| Environment specifiers; redefined libraries | A namespace is a charged host payload that dies with its specifier or last unit (specifiers are immutable, so nothing defines into them); `define-library` forms get a collection point | 972 MiB plateau; 49 KiB per redefinition | 1 charge, 4b |
| Provenance; scope sets | A document owns its location table and compacts it while streaming. Expansion chains are interned per document. The scope-set table is weak (ids recycled) or scoped to one form, and it reports which expansion scopes live identifiers still carry | ≈130 KiB per `eval` of a quoted `case` | 1 chains, 4c |
| `CoreExpr` literal pool | Scoped to one compilation (§8.3) | — | 3 |
| Descriptors, record types | The descriptor space becomes mark-region with hole reuse, inside the non-moving space; K3 reports it | steady | 5c |
| JIT code | Freed per unit, through size-segregated slabs and a coalescing list; fragmentation shown in `GcStats`; capped; gated by `eval-lambda` and `eval-redefine`. Stage 6 is a spike on an unmerged branch, so it freezes only the interface: `install_code` and per-unit free | — | 6 interface; the JIT track's first merged stage (decision 20) |
| JIT embedded addresses (§8.2) | A fourth case is added. JIT code may embed an object that lives in a non-moving space (symbols, cells, binding records, the descriptor space) and that the unit's constants or link table reference. Such an object is never rewritten while the body lives. Everything else stays one load from the descriptor. Under variant R, a cell reached through a re-pointable record stays under the `WATCHED` rule | — | 5c, 6 |
| Heap blocks, LOS cache, mark segments | Empty runs returned within 2 majors (one at an idle trim); empty blocks in partly occupied runs decommitted one by one; partly occupied blocks reused, then from stage 8 evacuated by the footprint trigger (M5) | 210 MiB kept after a peak | 5e, 8 |
| Green-thread stacks | Fixed `--stack-max` slots, carved from chunked `MAP_NORESERVE` reservations of 256 slots each (one VMA per 256 threads). Slots are lazily committed, trimmed by page and charged. No guard page: every push passes the frame-entry check. Stacks never relocate (§8.1). This replaces one reservation per thread, which Linux's `vm.max_map_count` (65,530) caps at 32–65 K threads | — | 9 |
| Ports | `PortTable` registry prunes dropped heaps | steady | 4a |

The design already fixes these:
- continuations: 563 MiB plateau today; fixed at 4e;
- tree-walker environments: fixed at 4f;
- register stacks: 143 MiB kept on the VM and 984 MiB on the tree-walker; fixed at 4d;
- teardown: 3.6 MiB per interpreter; fixed at 2.

The MMU uses a ring buffer per window (stage 0). Slot tables keep their peak length (4–16 B per slot).

Each per-owner key arrives with its table: `symbols`, `namespaces`, `cells`, `binding-records`, `code-units`,
`descriptor-bytes`, `jit-code-bytes`, `external-bytes` by kind, `scope-sets`, `documents`, `ports` and `threads`.
Alongside them, `(gc-stats)` carries:
- `metadata-bytes` and `sparse-bytes`;
- the in-pause work counters;
- from stage 0, `resident-bytes` and `cpu-us`;
- from 5e, `memory-budget`.

### Limits

A steady program stays under `max_heap` while F(L) plus metadata fits under `max_heap` − 4 MiB reserve. That
allows L up to about 27% of `max_heap`, or 4.3 GiB at 16 GiB. Beyond that the target clamps and majors come more
often.

**Memory budget.** Defaults derive from B = min(physical RAM, the cgroup limit: v2 `memory.max`, v1
`memory.limit_in_bytes`), never from host RAM alone. .NET does the same with 75% of the container limit. When
`RLIMIT_AS` is set:
- the main stack, the store buffers and the JIT reservation are sized first, and the heap reservation gets what
  remains;
- `max_heap` is clamped to that reservation;
- startup falls back to smaller sizes rather than failing.

| Limit | Counts | Default | Knobs | At the limit | Stage |
|---|---|---|---|---|---|
| Hard heap | committed data, LOS, metadata, external bytes (SD4) | min(16 GiB, 75%·B) | `PATINA_HEAP_MAX`, `--heap-max`, `HeapConfig::max_heap` | the steps below | 5e, 5g |
| Heap ceiling | the heap's reservation | max(`max_heap`, the default `max_heap`), within `RLIMIT_AS` | `HeapConfig::heap_ceiling` | `max_heap`, and the embedder's raise of it, stop here | 5e |
| Soft heap | as the hard heap | off | `PATINA_HEAP_SOFT_MAX`, `--heap-soft-max`, `HeapConfig::soft_max_heap` | clamps the target and never fails; lifts after 5 consecutive majors it clamped that each free < 2% | 5g |
| Register stack | one thread's frames | main: min(8 GiB, 25%·B), which is 1.75 GiB on a 7 GB runner and still holds the VM's 10 M frames (1.37 GB); 256 MiB per green thread | `PATINA_STACK_MAX`, `--stack-max`, `HeapConfig` | `&stack-exhausted` within 1 MiB of the cap | 4d |
| Threads | started, unterminated | charged stacks under `max_heap`; one `--stack-max` slot of address space each | optional `PATINA_MAX_THREADS` | `thread-start!` raises `&heap-exhausted` | 9 |
| Descriptors | open file ports | `RLIMIT_NOFILE` | OS; lanes pin it | post a major when opens − closes since the last major reach min(128, limit/4); on `EMFILE`, collect and retry once, then a file error | 1, 4a |
| JIT code | installed bodies | set by the JIT track's first merged stage | `PATINA_JIT_CODE_MAX` | stop tiering up; no error | first merged JIT stage |
| Address space | heap reservation (from the ceiling); main stack; 256 MiB store buffer per mutator; JIT reservation; green-thread chunks | fitted under `RLIMIT_AS` | — | smaller reservations, then K13's fallback; the store buffer's soft limit collects first | 5a, 5e, 7, 9 |

**At the heap limit:**
1. The target never exceeds `max_heap` − reserve. A user-sized request raises at once only if it exceeds
   `max_heap` − reserve − the uncollectable floor: the immortal space, metadata and block table, and the stacks of
   started threads. Racket likewise raises at once only for a request at least as large as the whole custodian
   limit, or one whose size is not a fixnum (`racket/src/thread/custodian.rkt:686-700`,
   `racket/src/cs/rumble/memory.ss:223-232`).
   Every other failed request takes the single collect-and-retry of §5 step 6.
2. Before raising, one maximal major also drops caches (expansion memos, unreachable units, string-port slack) and,
   from stage 8, evacuates, as G1 and V8 do.
3. **Progress guard:** 5 consecutive *counted* majors that each free under 2% of `max_heap` raise at the next poll.
   - A major is counted when allocation reached a target clamped by `max_heap`, not by `soft_max_heap`.
   - Forced `(gc)`, idle, descriptor-pressure, retry and maximal majors neither count nor reset the run.
   - Any major that frees at least 2% resets it.

   These are HotSpot's overhead-limit counts without the time term. chibi has no guard: at its cap it ran at 100%
   CPU until killed after 30 s.
4. `&heap-exhausted` is catchable. Its payload holds the limit, the managed bytes and the per-owner key that grew
   most since the previous major. Uncaught, it gives a non-zero exit (SD9). If the reserve runs out before a poll,
   the runtime flushes ports and aborts. To keep that rare, a driver about to enter a known `NoGcScope` site (an
   import met mid-form, a tree-walker library body) first runs a major when headroom is below that site's K16
   high-water mark.
5. A test proves that a handler for either condition fits in the reserve. If one does not, the runtime unwinds
   without running handlers, as Lua and Guile do.

chibi thrashes and Gauche aborts (status 1), so these tests stay outside the byte-identical lanes.

**Embedding** (decision 13):
- at stage 3, an external-bytes accounter (V8's `ExternalMemoryAccounter`, Chez's phantom bytevectors);
- at 5g, a near-limit callback that may raise `max_heap` up to the heap ceiling (V8 raises only inside its
  pre-reserved cage), `notify_idle()` and a memory-pressure notification;
- from stage 1, `(gc-stats)` shows each limit beside its use.

**Placement.** No new top-level stage.

New sub-stage **5g, Limits** (after 5e, independent of 5f, 2–3 engineer-weeks [I]):
- external bytes in `max_heap`;
- the soft target;
- the whole-limit request check;
- the maximal major;
- the progress guard;
- the condition payload;
- the `NoGcScope` pre-entry check;
- the embedder hooks;
- the handler-room test;
- the limits lane.

The rest joins named stages, about 2–3 engineer-weeks in all [I]:
- **Stage 0:** the lane and its runner, the `resident-bytes` and `cpu-us` keys, the hygiene-matrix maker.
- **4d:** the main stack's default from B.
- **5c:** symbols' name hash and old allocation.
- **5e:**
  - B, the heap ceiling and `RLIMIT_AS` fitting;
  - the idle trim at prompts, `notify_idle` and reads;
  - per-block decommit;
  - the boost decay.
- **Stage 8:** the footprint trigger.
- **Stage 9:**
  - slot-carved thread stacks;
  - idle trims at thread waits and mid-datum reads.
- **The JIT track's first merged stage:** its code allocator and cap.

I1's fix lands now, ahead of the stages.

### Verification

**The steady-state lane** (stage 0, `crates/patina-tests/bench_programs/gc/steady/`; Chez variants for scoring):
- **REPL-style:** redefinition streams; the `guard` stream run from a file, from stdin and in REPL `-i`;
  `eval-redefine`; `eval-lambda`; `macro-eval` (`case`, `guard`, the standard macros); `hidden-define`;
  `unbound-ref`; `load-repeat`; `reimport`; library redefinition.
- **Server-style:**
  - `steady-alloc` (a 20 K-slot ring);
  - `peak-then-drop` with `pause-after-peak`;
  - `peak-then-sparse` (keeps every 4,096th vector of the peak);
  - `peak-then-idle` (one `(notify-idle)` after a drop);
  - `deep-then-steady`;
  - `port-churn`;
  - `file-port-churn`.
- **Churn:** `sym-churn`; `eval-fresh-names` (class D); `env-churn`; `env-lambda`; `form-eval-fresh`;
  `record-redefine`; `cont-churn` at depths 10, 100 and 1,000; `interp-churn` (Rust).
- **Threads,** from stage 9: `thread-churn`, `blocked-threads`, and a 100 K-thread probe on Linux.

**Protocol.** Each run is one process to 4N cycles.
- At N and at 4N the probe calls `(gc)` twice, then reads `live-bytes`, footprint, the per-owner keys,
  `resident-bytes` and `cpu-us`.
- The segments [N, 2N) and [3N, 4N) are timed by `cpu-us` less in-pause time.
- Every row runs 3 times, both backends.
- N is set per row from a time budget. Each row prints the smallest leak it can detect, 64 KiB/3N + 8 B per cycle.

**Before stage 1** (no `live-bytes`, and `(gc)` does not yet collect at its call):
- the lane reads `resident-bytes` (from `/proc/self/statm`, or `task_info`'s footprint) in the top-level form
  after a `(gc)`;
- it does so at N, 4N and 16N, takes the median of 3 runs and fits a slope;
- the floor is max(4 MiB, 10%).

Max RSS (`ru_maxrss`) is a lifetime high-water mark that varies by 3 MiB for identical work, so it never gates.

**Runner.**
- It is portable: per-child rusage from `os.wait4`, with units normalised (KiB on Linux, bytes on macOS).
- A missing measurement fails the row.
- Every probe runs under a timeout and a memory cap: `--heap-max` from 5e, a resident-size watchdog before it. A cap
  hit is red.
- Rows red today run capped, at a reduced N, until their fixing stage.

**Cost.** Per-cycle times measured here are 0.6–380 µs on the VM, and 7.5 ms per `interp-churn` cycle. At
N = 2,000 (100 for `interp-churn`), the lane takes about 3–4 minutes on the VM and 6–8 on the tree-walker [I], so
it runs nightly. Per PR it runs the rows under 2 s each, about a minute on both backends.

**Not gates.** Wall-clock maximum pause, MMU(10 ms) and RSS series are nightly signals on a quiet machine: the
median of 5 runs, with differences under 1 ms ignored.

**Oracles.** Oracle numbers are context. The target is to be no worse than the best oracle that is steady on the
row.

**Hygiene.** Stage 0 adds a hygiene-matrix maker whose introduced definition only macro templates reach. It is the
`def-getter` shape: `(define secret 42)` beside a getter macro and a setter macro, with no getter procedure.
- It prints 7 today on both backends, on chibi and on Gauche.
- From 5a it also runs under zeal-major, so reclaiming introduced definitions at 5c cannot drop a binding that a
  template still reaches.

**The limits lane** (5g, outside the byte-identical lanes) follows prior art:
- **step test** (Go `TestMemoryLimit`): L at 10–80% of `--heap-max`; committed ≤ `max_heap`; nothing raised while
  F(L) fits.
- **Return to baseline** (ZGC `TestUncommit`, in majors): small, medium and LOS spikes, twice. A sparse variant keeps
  one object per MiB of each spike and is judged against F(L′) + `sparse-bytes` until stage 8, and against stage 8's
  bound after.
- **Idle in a real blocking read:** the harness holds a pipe open and samples footprint and resident size from
  outside while the probe is blocked. The pass bound is F(L′) + `sparse-bytes`.
- **Overhead limit** (HotSpot): exhaustion within 6 counted majors of the clamp, never a hang. L at 40% of
  `--heap-max` with 10 `(gc)` calls raises nothing.
- **`large_request_after_garbage`,** with a drop-then-allocate case under `--heap-max 1G`: a 700 MiB table live at
  a major, then dropped, then `(make-vector 60000000)` succeeds.
- **Allocation-failure zeal** (Lua `EMERGENCYGCTESTS`), and a downward `--heap-max` sweep over the chibi suite. The
  sweep goes down to a documented floor per backend: the startup peak plus the largest K16 window. Above the floor
  each run gives normal output or a clean `&heap-exhausted`. Below it, the only accepted failure is the abort
  diagnostic naming a `NoGcScope` site.
- **Handler room.**
- **10 M-deep recursion** at the default cap, on the VM. The tree-walker runs 1 M frames, since 10 M would need about
  9.6 GiB.
- **`descriptor_exhaustion_retries`.**
- **Budget detection:** under a cgroup `memory.max` and under `ulimit -v`, startup succeeds with smaller
  reservations, and `memory-budget` reports the limit.
- **Determinism replay:** two runs on the same machine, on Linux and on macOS, give the same collection kinds and
  `live-bytes` per major.
  - Across the two systems the replay runs only with the JIT off, from the same working directory, with
    `--heap-max` pinned, `ulimit -n 1024` and `PATINA_DETERMINISTIC=1`.
  - Every lane that compares collection sequences pins the same inputs.

**Soak:** from stage 5, a nightly hour of a REPL-like loop, slope-tested per major.

**Kill criteria:**
- **K17, steady state.** A stage that turns a green row red is blocked until it is green. Stage 5's exit (K9) also
  needs every class A, B and C row green on the VM, and on the tree-walker except rows allowed to lag (decision 1).
  Under SD2's alternative, `sym-churn` and the symbol part of `unbound-ref` are class D.
- **K18, contract cost.** M1 costing over 1% geomean in instructions, or 10% on any major pause → sweep the
  non-moving space every k-th major (reclamation delayed by up to k majors).
- **K19, limits.** Handler room fails → unwind without handlers. A GBS workload reaching the guard under default
  limits → a collection point (K16) or a corrected charge, never a larger default.

### Observed defects

Each is filed as a GitHub issue after searching issues and `PRD/ARCHIVE`, with symptom and minimal repro only.

| # | Symptom | Fixed at |
|---|---|---|
| I1 | a library macro with a private helper leaks interned aliases per top-level expansion (`%guard-aux.N`): `symbols` +3 and 1.8–2.3 KiB per `guard` | now: aliases deduplicated by target (home environment, name, identity scopes), as `import_alias` already is |
| I2 | re-`eval` of a datum with source locations grows without bound: 2,500 evals 313–320 MiB in 2.9 s, 5,000 evals 634 MiB in 10.5 s, about 130 KiB per eval (with `(patina debug)` also imported the audit measured 95–118 KiB, and 2.2 GiB in 168 s at 20 K) | 1 |
| I3 | references to macro-introduced definitions slow with each expansion: 25 K, 5 s; 100 K, 76 s | ≤ 4b (indexed lookup) |
| I4 | a redefined library stays alive (49 KiB each); `define-library` forms reach no safe point | 2, 4b |
| I5 | an `environment` loop plateaus at 972 MiB | 1, 4b |
| I6 | after a dropped peak, every pause keeps the peak's sweep cost | 5e |

### Owner decisions

| # | Question | Default | Alternative |
|---|---|---|---|
| SD1 | SS1–SS5 as acceptance criteria | yes, judged on deterministic readings. From stage 0, the lane's resident-size form and SS4 gate (K17). From stage 1, SS1, SS2 and SS5 at forced majors. From 5e, SS3's work counters and the phase-change clause. Wall-clock pauses, MMU and RSS stay nightly signals | guideline only |
| SD2 | Symbol table (amends decision 11) | weak, as in Chez (flat at 49.4 MiB over 1 M symbols), hashed by name | strong, as in chibi and Gauche: 32–48 B kept for ever per symbol, and the symbol rows become class D |
| SD3 | Run-time cells, records, symbols | mortal (M1) | immortal: about 8 KiB leaked per `environment` call, and pauses grow |
| SD4 | What `max_heap` counts | external bytes included | heap only (HotSpot, .NET, V8): Rust-table leaks get past the limit |
| SD5 | Stacks | main stack separate (JVM, V8, OCaml), default min(8 GiB, 25%·B); green-thread pages charged | one budget for all (Gambit, Erlang, Go) |
| SD6 | Progress guard | counts majors paced at a `max_heap` clamp, and bytes | time-based (HotSpot, V8, Go): not deterministic |
| SD7 | Idle trim | on, as defined above: at most one idle major per paced cycle, with back-off; prompts and `notify_idle` only in deterministic runs | off (G1 before JEP 346) or a timer (ZGC 300 s, V8 8 s) |
| SD8 | Soft target | opt-in, in bytes | `GOMEMLIMIT` with a CPU limiter |
| SD9 | Exit status on exhaustion | the status of any uncaught error | a distinct status (Go 2, HotSpot 3, V8 134) |

### Review dispositions

Two reviews of the first draft, labelled A and B; each finding was checked against the source or the reproduction
it cites.

**Checks run for this revision:**
- `review-ss/intro-template.scm` on both Patina backends, chibi and Gauche;
- `g1.scm` (`symbols` 2 → 17 → 20 → 23);
- the alias names in `review-ss/trace.txt`;
- `envdef.scm` (specifiers are immutable on Patina and chibi);
- the cited lines of `exceptions.scm`, `desugarer/mod.rs`, `environment.rs`, `srfi-69-impl.scm`,
  `hygiene_matrix.rs`, `ci.yml` and `run_suite_oracles.sh`;
- Racket's `custodian.rkt`, `thread.rkt` and `rumble/memory.ss`;
- the steady harness's `run.py` and `slopes.py`;
- the results files.

The contention timings of B1 and B4 were not re-run.

| Finding | Verified | Disposition |
|---|---|---|
| A1 (blocker): weak introduced definitions break hygiene | yes: prints 7 everywhere today; the matrix's Introducing maker always links X through a getter procedure | **Fixed.** Introduced definitions are strong at 4b, with only the indexed lookup. From 5c they are reclaimed when the record is unlinked and no live identifier carries the introducing scope. A new matrix maker is added at stage 0 and runs under zeal-major from 5a. The audit's "better than every oracle" (§3.4) is withdrawn |
| A2, B10: M5 and the phase change fail for scattered survivors | yes: H.3 decommits whole 4 MiB runs; §6.5 does not count large holes as fragmentation | **Fixed.** M5 and the phase change are restated for empty memory, with a `sparse-bytes` term. Per-block data decommit is added inside partly occupied runs (B10's first fix). Stage 8 gets a footprint trigger. `peak-then-sparse` and a sparse return-to-baseline variant are added |
| A3(a), B4: the idle trim returns nothing while blocked | yes: decommit and classification run from poll slices, with 2-major hysteresis | **Fixed.** The idle major completes its post-pause work before the wait, without the rate limit, and with the hysteresis waived. One major then suffices, so B4's two back-to-back majors are not needed: the idle major still keeps (target′ − L′) + reserve, which is what the hysteresis protects. The limits lane tests a real blocking read |
| A3(b), B5: trigger has no L term | yes | **Partly.** The cost is bounded by the back-off: at most log₂ P + 1 useless idle majors over P paced ones. The proposed trigger, committed > F(L_last) + max(64 MiB, 25%), is **rejected**: L_last is stale in the very case the trim exists for. A peak still live at the last paced major and dropped after it leaves footprint below F(L_last), so that trigger never fires. The idle major does recompute the target from L′; otherwise decommit would keep the peak's headroom. It neither counts toward nor resets the boost or the guard |
| A3(c): hooks before stage 9 | yes: blocking primitives become `Transfer` only at 9 | **Fixed.** At 5e the hooks are the prompt, `notify_idle`, and read entry with an empty buffer through `Step::CollectAndRetry`. Thread waits and mid-datum reads come at 9. `(notify-idle)` is exposed to Scheme |
| A4, B7: immediate raise uses stale L; Racket misread | yes: Racket compares the request with the whole limit | **Fixed.** A request raises at once only past `max_heap` − reserve − the uncollectable floor; the citation is corrected; the drop-then-allocate case is added |
| A5: broadened JIT embedding unsound under stage 8 | yes (§8.2, §6.5) | **Fixed** with the reviewer's wording. The non-moving space joins the never-evacuated list |
| A6: I1's 4b fix misses keyword aliases | yes: `%guard-aux` is `define-syntax`; `%guard-aux.0`, `.8`, `.11` in the trace | **Fixed.** Aliases are deduplicated by target now. Removing keyword aliases at 4c is **not adopted** as a commitment: 4c's scope does not include resolving keywords in their definition environment, and once deduplicated the aliases are bounded by program text, which M2 accepts |
| A7, B2: lane N, tolerance and budget inconsistent; vacuous windows; SS5 timing | yes: N ≥ 21,846 under the old floor; `reimport` collects once at every N | **Fixed.** Values are read after forced `(gc)` calls at the window ends. SS1 uses a per-cycle bound, so N ≈ 1,000 detects 32 B per cycle. N is set per row, with a printed detection limit. SS2 and SS3 need paced majors or fail as vacuous. SS5 is read after forced majors. Red rows are capped. The cost is recomputed |
| A8, B14: near-limit callback cannot raise `max_heap` | yes (§4) | **Fixed.** `HeapConfig::heap_ceiling` sizes the reservation; by default it is max(`max_heap`, the default `max_heap`), within `RLIMIT_AS` |
| A9: weak-symbol hash and young symbols | yes: `identity-hash` is the heap index; symbols are not born old | **Fixed.** The hash is a fixed-seed hash of the name, in its own PR; symbols are allocated with `alloc_old`, and the interner is pruned at majors only |
| A10: slot growth by relocation contradicts §8.1 | yes | **Fixed.** Fixed-size slots are carved from chunked reservations, with no guard pages; §8.1 is unchanged. The audit's relocation (§3.13) is withdrawn |
| A11: JIT allocator at stage 6, an unmerged spike | yes | **Fixed.** The allocator and cap move to the JIT track's first merged stage; stage 6 freezes only the interface |
| A12, B13: replay depends on platform inputs | yes: the descriptor threshold and default `max_heap` vary by machine; external bytes vary by OS | **Fixed.** Lanes pin `--heap-max` and `ulimit -n 1024`. Cross-OS replay runs only with the JIT off, from a fixed directory, under `PATINA_DETERMINISTIC=1`, where only deterministic idle triggers apply |
| A13: I2 quotes the stats variant | yes: plain 313 and 634 MiB at 2,500 and 5,000 evals | **Fixed.** I2 quotes the plain repro and names the other variant |
| B1 (blocker): SS4 on wall time flaps | not re-run; §14 already rules out wall time for small effects | **Fixed differently.** SS4 compares mutator CPU time per cycle between two segments of one process, each the minimum of 3 runs, against a bound of 1.5. Start-up is excluded by construction, and the bound sits between flat cost (1.0) and history-proportional cost (2.33). The suggested whole-run bound of 8 separates the same shapes (4 against 16) but needs one process per size |
| B3: SS2's RSS form is noise-level and macOS-only | yes: `run.py` parses `/usr/bin/time -l`; `slopes.py` skips rows with no RSS; `reimport` varies by 3 MiB | **Fixed.** In-process `resident-bytes` at forced `(gc)` boundaries, with 3 sizes, a median and a 4 MiB floor before stage 1. The runner is portable and fails on a missing value. Max RSS never gates |
| B6: progress guard counts unpaced majors | yes | **Fixed.** Only majors paced at a `max_heap` clamp count; others neither count nor reset. A limits-lane test runs L at 40% with 10 `(gc)` calls |
| B8: downward sweep below K16's windows | yes: 4 MiB reserve against windows of up to max(64 MiB, 25%) | **Fixed.** The sweep has a documented floor, and drivers run a major before entering a known `NoGcScope` site. The reserve stays 4 MiB for handler room: sizing it from K16 would cut up to 64 MiB from every heap |
| B9: defaults ignore cgroups, `RLIMIT_AS` and runner RAM | yes: the CI `ulimit -v` covers only the oracle step today, so nothing breaks yet | **Fixed.** Defaults come from B. Reservations are fitted under `RLIMIT_AS`. The main stack defaults to min(8 GiB, 25%·B). The 10 M-frame test runs on the VM only. `memory-budget` is reported |
| B11: `committed-bytes` undefined against the 1.1 factor | yes | **Fixed.** Footprint is committed data plus LOS plus external bytes; metadata is reported separately |
| B12: descriptor pressure lost "since the last major" | yes | **Fixed** |
| B15: SD2's alternative keeps K17 red for ever | yes | **Fixed.** Under it the symbol rows become class D |
