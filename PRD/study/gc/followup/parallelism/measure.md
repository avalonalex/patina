# Shared-memory parallelism: the single-thread tax, measured

Task key: `par-measure`. Machine: Apple M4 Pro (8 P-cores + 4 E-cores; P-core L1D 128 KiB, L2 16 MiB per cluster,
128 B cache lines, 16 KiB pages), macOS 27.2, arm64. Patina `main` at `28a94f8`. Rust 1.97.1 (the repo's
`rust-toolchain.toml`), target `aarch64-apple-darwin`, whose default CPU has `lse`, `rcpc` and `rcpc2`. Nothing in the
repository was modified; the programs are retained under `PRD/study/gc/probes/followup/`, and the binaries and raw results were not (§8).

Tags: **[P]** measured here; **[S]** read in source; **[C]** cited from a primary source; **[I]** inference or model.

**Errata after review (2026-10-01).** `ANSWER.md` in this directory supersedes this note where they differ:
- **The fence tail.** The 15–54 ns barrier figure (§0, §2.5, §6.4) is an intermittent, unexplained microbenchmark
  anomaly, and its stated cause ("memory not recently written") does not fit the raw sweep: a 64 KiB window gave
  46.7 ns in one run and 16.0 ns in the other. Chez's `setcar` loop, which has the JIT-like shape, ran at 1.84 ns
  per iteration. The "10–100× on a tight `cons` loop" claim is withdrawn, and fence placement is left to the S6
  spike.
- **The `order` result** covers 11 workloads (namedlet excluded; with it, +2.0%). The post-redesign estimate is
  ≈2–5%, up to ~7% if load latency is exposed.
- **The `a_all` +4.1%** is a lower bound for representation (a).
- **Acquire loads.** The Rust interpreter must use acquire loads (§0 item 4 and §7 item 3 said "may").

**Noise.** The machine was shared with other agents' benchmark jobs throughout (load average 2–7). Every comparison
below is therefore *interleaved* (variants alternate within each round) and reported as a median with its spread;
microbenchmarks run at `USER_INTERACTIVE` QoS so they land on P-cores. The clock under this load measured 3.92–3.93
GHz (from an 8-deep dependent `add` chain), and cycle counts below use 3.93 GHz.

---

## 0. Answer

**What parallelism would cost a single-threaded Patina, by mechanism and by representation:**

| Mechanism | Per operation [P] | On today's interpreter, measured in a real build [P] | After the redesign, interpreter [I] | After the redesign, JIT [I] |
|---|---|---|---|---|
| Heap words as relaxed `AtomicU64`, sub-word atomics, the poll word | 0: identical machine code (`ldr`/`str`/`strb`), identical time | — | **0** | **0** |
| Per-mutator TLAB refilled from a block pool under a `Mutex` | +0.00 ns per allocation (one lock per 32 KiB block) | — | **0** | **0** |
| Acquire loads of heap references (`ldapr`) + release stores of heap values (`stlr`) | load +0.05 ns throughput, +0.54 ns when it is on a dependent chain; store +0.06 ns | **+1.9% geomean**, +0.00–0.22 ns per VM instruction (12 workloads) | **+2–5%** (same absolute ns on a faster interpreter) | loads must stay plain `ldr` under a documented dependency-ordering exception: acquire loads would cost **+3% to +60%** |
| One publication fence per allocation (`dmb ishst`) | 0.1–0.4 ns in mixed code; **15–54 ns** right after streaming stores to memory not recently written | **+0.4% geomean**, −0.12…+0.08 ns per instruction | not needed (the funnel's release store publishes) | **0–4% in realistic code; a 10–100× tail on tight allocation loops** → put the fence on publishing stores, as Chez does |
| Fence before every heap-valued store (Chez's placement) | as above | (subsumed by the release-store row) | ≈ 0 | **≈ 0–4%**: Chez does exactly this on arm64 and its `set-car!` and `vector-set!` loops of fresh pairs run −1.4% / +4.1% |
| `RefCell<Heap>` → an uncontended `RwLock` per heap access | +2.4 ns per borrow in a serial chain; ~0.25 ns per borrow in the interpreter | **+2.6% geomean** (lock alone) | — (no heap lock) | — |
| `Rc` → `Arc` on code objects and environments | +0.6 ns per clone+drop pair; +1.9 ns per call+return | **+2.2% geomean** (Arc alone); lock+Arc together **+4.1%**, +0.30 ns per instruction | — (code refs are words; global cells) | — |
| `thread_local!` instead of the `Cx` register | +0.51 ns per access (a call through the TLV descriptor) | — | 0 (the design passes `Cx`) | 0 (`x21`) |
| Interner behind a lock | +0.7 ns (`parking_lot`) to +5.1 ns (`std`) on a 17 ns lookup | — | rare operation; ≈ 0 | ≈ 0 |
| Bulk copy/fill over relaxed atomics | **3.2–3.3× slower per word** (no `memcpy`, no SIMD) | — | 0 if bulk ops use one word-atomic copy routine; otherwise 3× on `vector-copy!`, `vector-fill!`, string ops | same |
| Parallel marking with `ldsetb`/CAS instead of a plain `orr` | +0.41 ns (1 MiB) to +0.87 ns (64 MiB) per unconditional mark; 0 for test-then-CAS | — | GC time only, and only with GC workers (stage P already) | same |

**Six conclusions.**

1. **Representation (a) is cheap single-threaded and useless in parallel.** Making today's interpreter thread-safe by
   brute force (an `RwLock` around every heap access, `Arc` for code objects and environments) cost only **+4.1%
   geomean** in a real build (+0.17–0.42 ns per VM instruction). But it does not scale: one shared reference count
   costs **3.6 ns at 1 thread, 20 ns at 2, 97 ns at 4 and 370–550 ns at 8** per operation, and a shared counter
   `fetch_add` 1.8 → 10 → 54 → 276–292 ns. At ~0.6 heap borrows per instruction, four carriers would spend ~65 ns per
   instruction on the lock's cache line against today's ~5.7 ns of useful work: **negative scaling**. The tax is not
   the problem with (a); the ceiling is. The earlier estimate of 5–15% (VM, unmeasured) was pessimistic for the
   single-thread case and silent about this.
2. **Representation (b) pays only for memory ordering, and it is small.** The `threaded` obligations of DESIGN §12
   (acquire loads, release stores of heap values) cost **+1.9% geomean** on today's interpreter (range −0.1…+3.6%,
   at the noise floor of code-layout effects, ±4%). On a faster redesigned interpreter the same ~0.13 ns per
   instruction is roughly +2–5%.
3. **The publication fence is cheap where it is usually placed, and catastrophic where a JIT might place it.** On the
   M4 a barrier (`dmb ish`, `dmb ishst` or `stlr` alike) costs 0.1–0.5 ns in mixed code but **15–54 ns** when it
   follows streaming stores to memory not recently written: exactly the shape of a tight JIT allocation loop with
   `ap` in a register and a fence per allocation group. Chez, which fences on arm64 *before each heap-valued store*
   and never after allocation, pays ≈ 0 for it. Recommendation for the PRD: specify the `threaded` JIT fence at the
   funnel/barrier's heap-value branch (after the `tbz x1,#2` immediate filter), not at allocation groups, and make
   the S6 spike measure a tight `cons` loop both ways.
4. **The JIT must not use acquire loads.** A dependent `ldapr` adds 2.1 cycles per load; at a JIT's ~0.5–0.9 ns per
   bytecode and ~0.5 heap-reference loads per bytecode (nboyer), that is +3% to +60%. Cranelift lowers
   `atomic_load` to a load-acquire (`aarch64/lower.isle`: `(load_acquire ty flags addr)`), so the JIT must emit plain loads and rely on address dependencies, as DESIGN §12 already
   allows; the Rust interpreter may use `ldapr` (`ldapur` with an offset), which is what LLVM emits for `Acquire`.
5. **Chez on this machine: +0.8% geomean** for a threaded build over a non-threaded build of the same source, over
   14 benchmarks; **+3.5% including interning**, which takes a global pthread mutex in the threaded build and runs
   +50%. Allocation/GC-heavy benchmarks pay +6–8% (cons-churn, gcbench), the write-barrier loops with an `dmb ishst`
   per store −1.4% and +4.1%, calls and flonums 0–2%.
6. **Published single-thread costs agree:** OCaml 5 is 3.5% (stop-the-world parallel minors) or 4.9% (concurrent
   minors, read barrier) slower than stock OCaml [C]; OCaml's memory model on AArch64 costs 0.6% (`dmb ld` before
   stores) or 2.5% (branch after loads), but **85.3%** when every mutable access is `ldar`/`stlr` [C, ThunderX];
   CPython's free-threaded build was estimated at 5–6% and lands at ~10% on Linux, ~3% on macOS [C]. Go executes
   `DMB ST` in every `mallocgc` on arm64 [C, S]; HotSpot emits a StoreStore barrier (`dmb ishst`) after each
   allocation's header stores [C].

**So the cost of adding shared-memory parallelism later is overwhelmingly engineering, not runtime.** For the
redesign path (b), the measured single-thread tax of the mechanisms is ~2–5% for the interpreter and ~0–4% for a JIT
that follows the two placement rules above, against 8–16 engineer-months of work (itemize.md). Representation (a)
would pay a similar tax and get no speedup. The tax can be driven to 0 with a separate non-threaded build, as Chez
ships, at the price of a second CI configuration.

---

## 1. Method

Three instruments, each checked against the others:

1. **Mechanism microbenchmarks** (`par-measure`, §2): one mechanism per kernel, `#[inline(never)]` and
   `#[unsafe(no_mangle)]` so the machine code can be read by name in the emitted assembly; each kernel runs `n`
   operations; the harness calibrates `n` to 20 ms, runs 15 rounds with a rotating start, and reports the median
   ns per operation; three separate processes; the table reports the median of the three process medians, their
   range, and the largest within-run p10–p90 spread. `black_box` is used to stop LLVM folding refcount pairs and
   merging loads; its own cost (~0.3 ns) is in every row of a group and cancels in the deltas. A hand-written
   assembly sweep (`barrier`, §2.5) isolates the barrier anomaly.
2. **Perturbing the real interpreter** (§3): a patched copy of the repository, with each mechanism behind a `--cfg`,
   built five ways and timed on 12 workloads, interleaved, 5 rounds.
3. **Chez threaded vs non-threaded** (§4): Chez Scheme 10.5.0 built twice from one source tree on this machine,
   timed on 15 benchmarks, interleaved, 14 rounds.

Microbenchmark limits: an isolated kernel shows *latency* when the mechanism sits on a dependence chain and
*throughput* otherwise, and real code is in between. §3 shows by how much: the heap lock's +2.4 ns per borrow in a
serial chain became ~0.25 ns per borrow inside the interpreter. Use §2 for mechanism and asm, §3 and §4 for
percentages.

---

## 2. Mechanism microbenchmarks [P]

Median ns per operation, three processes × 15 rounds (`results/micro_run{1,2,3}.tsv`, aggregated in
`results/micro_agg.md`).

### 2.1 The codegen check: what each Rust form compiles to

From `par_measure-*.s` (release, `codegen-units = 1`), one operation per probe:

| Rust | aarch64 | Note |
|---|---|---|
| `Cell<u64>::get` / `set` | `ldr x0,[x0]` / `str x1,[x0]` | |
| `AtomicU64::load(Relaxed)` / `store(Relaxed)` | `ldr x0,[x0]` / `str x1,[x0]` | **identical to `Cell`** |
| `AtomicU32` / `AtomicU8::store(Relaxed)` | `str w1,[x0]` / `strb w1,[x0]` | `string-set!`, `bytevector-u8-set!` under `threaded`: free |
| `load(Acquire)` | `ldapr x0,[x0]`; with an offset `ldapur x0,[x8,#-4]` | RCpc (`rcpc`, `rcpc2`) |
| `load(SeqCst)` | `ldar x0,[x0]` | RCsc |
| `store(Release)` / `store(SeqCst)` | `stlr x1,[x0]` (register base only, so an extra `add` for `base+offset`) | |
| `fetch_add(1, Relaxed)` / `AcqRel` | `ldadd` / `ldaddal` | LSE, no LL/SC loop |
| `AtomicU8::fetch_or` / `fetch_and` (Relaxed) | `ldsetb` / `ldclrb` | the design's `MetaByte` under `threaded` |
| `compare_exchange` (u8, Relaxed) | `casb` | |
| `swap(0, SeqCst)` | `swpal` | the poll slow path's `event.swap` |
| `fence(Acquire)` / `fence(Release)` / `fence(SeqCst)` | `dmb ishld` / **`dmb ish`** / `dmb ish` | Rust cannot emit `dmb ishst`; it needs `asm!` |
| `Rc::clone` | `ldr; adds #1; str; b.hs trap` | |
| `Arc::clone` / `Arc` drop | `ldadd` (relaxed) + sign test / `ldaddl` (release), `dmb ishld` before freeing | |
| `RefCell::borrow` | `ldr; cmp #i64::MAX; b.hs panic` + inc/dec around the use | |
| `thread_local!` (const init, no `Drop`) | `adrp/ldr` of the TLV descriptor, **`blr`** to its thunk, then the access | a call per access on macOS |
| `&mut Ctx` field | `ldr x8,[x0]` | |

The design's threaded funnel compiles to a test and two stores:

```
_probe_funnel_store_threaded:          ; HeapSlot store: immediates relaxed, heap values Release
    tbnz  w2, #2, LBB368_2             ; the value filter bit doubles as the "needs release" test
    str   x2, [x0, x1]
    ret
LBB368_2:
    add   x8, x0, x1
    stlr  x2, [x8]
    ret
_probe_car_acquire:                    ; car of a pair at w-4 under `threaded`
    add   x8, x0, x1
    ldapur x0, [x8, #-4]
```

and the relaxed-RMW and `Cell` kernels are byte-identical loops (`k_cell_rmw` vs `k_relaxed_rmw`), as are the
pointer chases `k_chase_plain` and `k_chase_relaxed`; the acquire chase differs in one instruction:

```
k_chase_plain:    and x8,x8,x2 ; cmp x8,x1 ; b.hs .. ; ldr   x8,[x0,x8,lsl #3]           ; subs ; b.ne
k_chase_acquire:  and x8,x8,x2 ; cmp x8,x1 ; b.hs .. ; add x8,x0,x8,lsl #3 ; ldapr x8,[x8] ; subs ; b.ne
```

### 2.2 Reference counts

| Case | ns | cycles | Δ vs `Rc` |
|---|---|---|---|
| `Rc` clone+drop, same object (store→load chain through the count) | 3.01 | 11.8 | |
| `Arc` clone+drop, same object | 3.58 | 14.1 | **+0.58** |
| `Rc` clone+drop, rotating over 64 objects | 1.00 | 3.9 | |
| `Arc` clone+drop, rotating over 64 objects | 1.65 | 6.5 | **+0.65** |
| Patina's call+return shape with `Rc<CodeObject>` (3 clone/drop pairs: `code_object`, `dispatch_frame` ×2; frame push/pop) | 5.88 | 23.1 | |
| same with `Arc<CodeObject>` | 7.75 | 30.5 | **+1.87 per call** |
| same with a `Copy` code word (the redesign) | 2.82 | 11.1 | **−3.06 per call vs today** |

The last row is a gain the redesign gets single-threaded: today's three `Rc` pairs per call cost about 3 ns of a ~38
ns call (`vm-runtime.md` §9). Sources of those pairs: `VmState::code_object` clones (`vm_state.rs:575-579`), called
from `call_closure_from_regs` (`control.rs:177`); `ExecutionState::dispatch_frame` clones when the frame's code
changes (`execution_state.rs:134-142`); `pop_frame` drops (`execution_state.rs:78-82`).

### 2.3 Loads, stores and ordering

| Case | ns | cycles | Δ |
|---|---|---|---|
| `Cell` load+store, 1024 slots | 0.354 | 1.4 | |
| relaxed load+store | 0.359 | 1.4 | +0.00 (same code) |
| `ldapr` + `stlr` | 0.393 | 1.5 | +0.04 |
| `ldar` + `stlr` | 0.378 | 1.5 | +0.02 |
| same slot (counter chain): `Cell` / relaxed / acq-rel / SeqCst | 1.577 / 1.578 / 1.781 / 1.786 | 6.2 / 6.2 / 7.0 / 7.0 | +0.20 for ordered |
| independent loads: `ldr` / `ldapr` / `ldar` | 0.278 / 0.326 / 0.325 | 1.1 / 1.3 / 1.3 | **+0.05** |
| independent stores: `str` / `stlr` | 0.255 / 0.315 | 1.0 / 1.2 | **+0.06** |
| store then load elsewhere: relaxed / `stlr`+`ldapr` / `stlr`+`ldar` | 0.369 / 0.447 / 0.444 | 1.4 / 1.8 / 1.7 | +0.08 |
| **pointer chase, L1 (32 KiB)**: `ldr` / relaxed / `ldapr` / `ldar` | 1.280 / 1.278 / 1.820 / 1.819 | 5.0 / 5.0 / 7.1 / 7.1 | **+0.54 per dependent load** |
| pointer chase, 2 MiB: `ldr` / `ldapr` | 7.52 / 8.09 | 29.5 / 31.8 | +0.57 |
| pair walk (`car`+`cdr` per node, L1): relaxed / acquire | 1.901 / 2.407 | 7.5 / 9.5 | **+0.51 per node** |
| `fetch_add` relaxed / AcqRel, independent slots | 0.838 / 0.875 | 3.3 / 3.4 | +0.48 vs plain RMW |

On this core, `ldapr` and `ldar` cost the same when no `stlr` precedes them; the RCpc form matters on cores where
`ldar` must wait for an earlier `stlr`. The acquire tax is a latency tax: ~2 cycles on each load of a dependent chain
(`car`/`cdr` walks, closure → cell → value), ~0 otherwise.

### 2.4 `RefCell` versus locks

| Case (lock, touch the value, unlock) | ns | cycles |
|---|---|---|
| no lock (`black_box` read) | 0.30 | 1.2 |
| `RefCell::borrow` | 1.13 | 4.5 |
| `RefCell::borrow_mut` | 1.95 | 7.6 |
| a `RefCell`-shaped borrow from atomics (`ldadda` in, `ldaddl` out) | 3.56 | 14.0 |
| spin lock (`casab`, `stlrb`) | 1.39 | 5.5 |
| `parking_lot::Mutex` | 1.98 | 7.8 |
| `std::sync::Mutex` (a boxed pthread mutex on macOS; two calls) | 4.24 | 16.7 |
| `parking_lot::RwLock::read` | 3.63 | 14.2 |
| `std::sync::RwLock::read` | 4.86 | 19.1 |

These are serial chains on one lock word; §3 shows the in-situ cost is ~10× lower.

### 2.5 Allocation and publication fences: the M4's barrier anomaly

Bump allocation of a 16 B pair with the bump pointer in memory, as the Rust allocator of DESIGN §5 has it (4 MiB of
32 KiB blocks):

| Case | ns per pair | cycles |
|---|---|---|
| bump + 2 initializing stores + publish into a register-file slot | 1.92 | 7.5 |
| + `fence(Release)` (`dmb ish`) | 1.91 | 7.5 |
| + `dmb ishst` (`asm!`) | 1.91 | 7.5 |
| + `fence(SeqCst)` | 1.91 | 7.5 |
| block refill through a `Mutex`-protected block pool (every 2,048 pairs) | 1.92 | 7.5 |
| list built by `set-cdr!`-style publication: `str` / `stlr` | 1.886 / 1.888 | 7.4 |
| 4 pairs per group, no fence / one `dmb ishst` per group | 1.034 / 1.041 | 4.1 |

So fences after allocation are free *here*. But a store-only kernel tells another story (`stores_dmb_ishst`: 18 ns
per store+barrier; `str + fence(SeqCst)`: 15 ns). The hand-written sweep (`src/bin/barrier.rs`; every loop is one
`asm!` block; 16 B stores walk a window of the given size; two runs, `results/barrier_sweep_run{1,2}.txt`) isolates
the cause:

| ns per iteration | 16 B–1 KiB | 8 KiB | 64 KiB | 512 KiB | 4 MiB | 64 MiB |
|---|---|---|---|---|---|---|
| `stp` | 0.30 | 0.30 | 0.30 | 0.30 | 0.30 | 0.30 |
| `stp; dmb ishst` | 0.50 | 0.53–0.66 | 16–47 | 53–57 | 54 | 53–54 |
| `stp; dmb ish` | 0.50 | 2.0–5.7 | 14–47 | 48–53 | 52–54 | 53–54 |
| `str; stlr` (same line) | 0.28 | 0.28 | 0.28 | 0.41 | 0.40 | 0.40 |
| `stp; stlr` to another, hot line | 0.27 | 5–10 | 19–44 | 47–52 | 52–53 | 53 |
| `stp; dmb ishst; str` hot (JIT-like: `ap` in a register) | 0.63 | 0.63 | 0.70–0.74 | 1.5–4.4 | 14–15 | 19–23 |
| bump pointer loaded/stored in memory; `stp; dmb ishst; str` hot | 0.75 | 0.75 | 0.75 | 0.9–1.0 | 0.9–1.2 | 0.9 |

Reading: on the M4 any ordering point (`dmb ish`, `dmb ishst`, or an `stlr` to a different line) waits for earlier
stores to lines that are not yet writable in L1 to complete, about **50 ns** (a memory round trip); it is cheap when
the earlier stores hit recently written lines, and cheap when the loop is slowed by a dependence through memory (the
interpreter-like last row). `stlr` to the *same* line as the store before it is free. Chez's arm64 back end carries
a comment to the same effect (`s/cpprim.ss:708-711` [S]: duplicating the store "appears to be worthwhile on the
Apple M1 to avoid tightly interleaved writes and fences").

### 2.6 Mark bits (GC workers only)

| Case | 1 MiB metadata (L2) | 64 MiB (DRAM) |
|---|---|---|
| plain `orr` byte, random index | 0.70 | 0.90 |
| `ldsetb` (`fetch_or` Relaxed) | 1.11 (+0.41) | 1.77 (+0.87) |
| `ldsetalb` (AcqRel) | 1.39 | 2.08 |
| test, then plain store if unmarked (the usual mark loop) | 0.57 | 0.89 |
| test, then `casb` if unmarked | 0.57 | 0.90 |

With the test first (a pass marks each object once), the CAS costs nothing measurable; stage P's CAS marking is free
at this granularity.

### 2.7 Context in a register versus `thread_local!`

| Per call of a non-inlined accessor | ns | cycles |
|---|---|---|
| `&mut Ctx` argument | 0.77 | 3.0 |
| static `AtomicU64` (relaxed) | 0.76 | 3.0 |
| `thread_local!` const `Cell` | 1.28 | 5.0 |
| `thread_local!` lazy `Cell` | 1.27 | 5.0 |
| `thread_local!` with `Drop` (`RefCell<Vec>`) | 1.53 | 6.0 |
| `thread_local!` holding `*mut Ctx`, then the field | 1.28 | 5.0 |

**+0.51 ns per access** for TLS on macOS (the `blr` through the TLV descriptor). The design's rule "no runtime
state in `thread_local!`; the `Mutator` in `x21`" avoids it; today Patina has 19 `thread_local!` statics
(threads-patina-cost §1), none on the per-instruction path.

### 2.8 Interning

Lookup of 10,000 identifier-like strings in random order (`symbol_table` is a `std` `HashMap<String, HeapIndex>`,
`heap/mod.rs:319`):

| Map | ns per lookup |
|---|---|
| `std::HashMap` (SipHash) | 17.0 |
| + `std::Mutex` | 22.1 (+5.1) |
| + `std::RwLock` read | 18.8 (+1.8) |
| + `parking_lot::Mutex` | 17.7 (+0.7) |
| `FxHashMap`, no lock | 7.4 |

The hasher costs more than the lock. Chez's threaded build runs `string->symbol` 50% slower (§4) because its intern
path takes a recursive pthread mutex (`c/intern.c:162-190`, `c/thread.c:388-404` [S]); a `parking_lot` lock (or a
sharded one) avoids most of that.

### 2.9 Fences and the poll protocol

| Case | ns | cycles |
|---|---|---|
| `dmb ish` with nothing pending | 0.38–0.51 | 1.5–2.0 |
| `swap(SeqCst)` (`swpal`; poll slow path) | 0.64 | 2.5 |
| `fetch_or(SeqCst)` (`ldsetal`; a remote post) | 1.78 | 7.0 |
| poll: today's `Cell<bool>` load / the design's relaxed `AtomicUsize` limit compare | 0.512 / 0.521 | 2.0 / 2.0 |
| `set_limit` (store, `dmb ish`, load `event`) | 0.51 | 2.0 |

The poll protocol of DESIGN §9 costs nothing more than today's poll.

### 2.10 Bulk copies (what atomics forbid)

| Per 8 B word, 1,024-word vector | plain | element-wise relaxed atomics |
|---|---|---|
| copy (`copy_from_slice` → `memcpy`) | 0.082 | 0.262 (**3.2×**) |
| fill | 0.080 | 0.262 (**3.3×**) |

Rust has no relaxed-atomic `memcpy` (RFC 3301 is not stable); a `threaded` build needs one word-atomic bulk routine
(inline `ldp`/`stp` in `asm!` is outside the language's model, so it is allowed), or `vector-copy!`, `vector-fill!`,
`string-copy` and the GC's own copying of mutator-visible memory lose SIMD.

### 2.11 Contention (supplement): why sharing a counter is not an option

Wall ns per operation per thread, median of 5, two runs (`results/contention.tsv`):

| Threads | one shared `Arc` | private `Arc`s allocated adjacently | private `Arc`s, 256 B apart | one shared `AtomicU64::fetch_add` | private counters, padded |
|---|---|---|---|---|---|
| 1 | 3.6 | 3.6 | 3.6 | 1.8 | 1.8 |
| 2 | 19.7–20.0 | 3.6 | 3.6 | 10.5–10.8 | 1.8 |
| 4 | 97 | 55–65 (false sharing) | 3.6 | 54–57 | 1.8 |
| 8 | 370–551 | 78–96 (false sharing) | 4.2–4.3 | 276–292 | 2.2 |

Two consequences for the PRD: nothing touched per instruction may be a shared read-modify-write (a reader count, a
refcount on a shared code object), and per-carrier mutable data (`Mutator`, store-buffer cursors, block-pool
headers, per-carrier statistics) must sit on separate **128 B** lines on Apple silicon; adjacent 24 B allocations
already false-share at 4 threads.

---

## 3. Perturbing the real interpreter [P]

**What was built.** A copy of `crates/` and `lib/` at `28a94f8`, patched by `PRD/study/gc/probes/followup/perturb_patch.py`, with every
perturbation behind a `--cfg`, built six ways (`target-pb-{base,lock,arc,a_all,order,allocfence}`):

| Variant | What it adds | Sites |
|---|---|---|
| `lock` | every heap borrow in the VM and in most primitives also does an uncontended `RwLock`'s atomics: `ldadda`/`ldaddl` per shared borrow, `casa`/`stlr` per mutable borrow | `heap.borrow()`/`borrow_mut()` routed through a `SimBorrow` trait in 47 files of `patina-vm` and `patina-primitives` (212 `ldadda` sites in the binary) |
| `arc` | `Rc<CodeObject>` → `Arc<CodeObject>` (a real type change in `patina-vm`); the `Rc<Environment>` clone+drop in `LoadGlobal`/`StoreGlobal` (`frame_globals`, `vm_state.rs:1357-1364`) also pays an `Arc`'s atomics | |
| `a_all` | `lock` + `arc`: representation (a), single-threaded | |
| `order` | DESIGN §12's obligations on today's representation: `Acquire` loads in `car`, `cdr`, `vector_ref`, `read_mutable_cell`, `get_vm_closure_free_var`, `get_vm_closure_code_id`; `Release` stores of heap values (immediates stay plain) in `set_car`, `set_cdr`, `vector_set`, `write_mutable_cell` and the VM's `VectorSet` (`vm_state.rs:2380`) | 207 `ldapr` + 133 `ldapur` vs 108 + 33 in base |
| `allocfence` | `dmb ishst` after the initializing store of every pair and every object (`alloc_pair`, `alloc_object`; `heap/mod.rs:703-714`, `1469-1480`) | 48 `dmb ishst` sites |

The instruction counts were checked in each binary with `otool -tv`. Outputs were compared across variants (digits
masked, since the Larceny harness prints its timing) and checked for "wrong"/"error".

**Result** (5 interleaved rounds; % is the median of the per-round paired ratios, with their range; ns per
instruction is the median time difference divided by the workload's dispatched VM instructions, counted with the
instrumented binary of `workload-demographics.md`):

| Workload | base s | VM instr (M) | base ns/instr | lock | arc | **a_all** | **order** | **allocfence** | a_all ns/instr | order ns/instr | allocfence ns/instr |
|---|---|---|---|---|---|---|---|---|---|---|---|
| fib32 | 0.270 | 45.8 | 5.9 | +3.5% | +2.3% | **+5.6%** (+5.4…+7.8) | +1.4% (+0.7…+1.9) | +1.3% (+0.5…+1.5) | +0.34 | +0.08 | +0.08 |
| namedlet | 0.648 | 140.0 | 4.6 | −4.5% | −3.1% | −5.2% (−5.9…−2.6) | +3.6% (+2.4…+5.0) | −2.6% (−7.2…−0.8) | −0.16 | +0.17 | −0.12 |
| consloop | 1.210 | 220.0 | 5.5 | +3.3% | +2.1% | +4.1% (+2.3…+4.5) | +3.3% (+0.9…+3.7) | +1.2% (−0.9…+2.4) | +0.17 | +0.12 | −0.01 |
| closure | 0.545 | 85.0 | 6.4 | +3.6% | +4.1% | +4.3% (+1.6…+7.8) | +1.2% (−0.3…+3.7) | −1.0% (−3.1…+0.8) | +0.32 | +0.15 | −0.07 |
| nboyer | 2.284 | 402.7 | 5.7 | +2.7% | +3.0% | +4.3% (+2.3…+5.3) | +2.9% (+2.1…+3.7) | +1.1% (−0.7…+1.5) | +0.23 | +0.14 | +0.02 |
| deriv | 0.922 | 126.0 | 7.3 | +2.0% | +1.4% | +4.7% (+1.4…+6.5) | +1.1% (−2.2…+2.5) | +0.2% (−3.4…+2.3) | +0.19 | +0.03 | −0.11 |
| destruc | 2.788 | 303.1 | 9.2 | +2.9% | +1.5% | +3.6% (+2.1…+4.3) | +1.4% (−0.7…+2.3) | −0.1% (−2.1…+1.0) | +0.37 | +0.13 | −0.01 |
| quicksort | 2.276 | 308.8 | 7.4 | +2.3% | +2.6% | +3.7% (+3.5…+4.1) | +2.6% (+2.1…+3.4) | +0.2% (−0.3…+1.5) | +0.30 | +0.20 | +0.01 |
| fibfp | 1.499 | 114.4 | 13.1 | +2.4% | +0.2% | +2.7% (+2.4…+4.5) | −0.1% (−1.2…+0.2) | −0.3% (−1.4…+0.1) | +0.32 | −0.00 | −0.04 |
| generator | 3.047 | 482.0 | 6.3 | +1.9% | +2.0% | +3.6% (+1.4…+4.0) | +3.6% (+1.4…+4.8) | +1.1% (−1.8…+6.5) | +0.25 | +0.22 | +0.07 |
| hashtable0 | 2.484 | 239.6 | 10.4 | +2.4% | +2.2% | +4.1% (+3.0…+5.6) | +1.2% (−0.6…+2.2) | +0.3% (−1.4…+0.5) | +0.42 | +0.04 | −0.12 |
| mperm | 1.697 | 250.2 | 6.8 | +1.9% | +2.5% | +4.4% (−0.7…+5.5) | +2.4% (−2.0…+4.4) | +0.6% (−4.3…+14.9) | +0.29 | +0.19 | +0.03 |
| **geomean** (excluding namedlet) | | | | **+2.6%** | **+2.2%** | **+4.1%** | **+1.9%** | **+0.4%** | median +0.30 | median +0.13 | median −0.01 |

**Reading.**
- **The noise floor is code layout, ±4%.** `namedlet` (a self-tail-call loop that touches the heap once per seven
  instructions) runs 4.5% *faster* with the lock added: a layout effect in the dispatch loop, not a mechanism. Single
  results under ~4% are not individually meaningful; the consistent sign across 11 workloads is.
- **In situ, the mechanisms cost ~10× less than their serial microbenchmarks.** nboyer borrows the heap on ~0.6
  instructions in 1 (LoadClosure, ReadCell, Car, Cdr, Call, TailCall, Cons, WriteCell; jit-readiness §2.3); the lock
  added 0.15 ns per instruction, ~0.25 ns per borrow, against 2.4 ns in §2.4. fib32 makes 0.154 calls per
  instruction; `Arc` added 0.13 ns per instruction, ~0.85 ns per call, against 1.87 ns in §2.2. The out-of-order core
  hides uncontended atomics that are off the critical path.
- **Allocation fences are free in the interpreter** (one `dmb ishst` per pair, closure, cell and flonum: −0.12…+0.08
  ns per instruction), because the bump pointer and the slot index travel through memory (§2.5's last row).
- **`order` is the closest measurement of the redesign's interpreter tax**, and +1.9% is an upper-ish bound for it:
  today's slots are reached through `Vec` indexing and a `RefCell` flag, so the extra `add` per `stlr` and the
  `ldapr` latency sit beside more work than they will after stage 5.

---

## 4. Chez Scheme: threaded versus non-threaded on this machine [P]

**Builds.** The installed Homebrew `chez` (10.3.0) is threaded: `(threaded?)` → `#t`, machine type `tarm64osx`.
For a like-for-like comparison, Chez Scheme 10.5.0 was built twice from one source tree
(`~/Project/reference/ChezScheme` at `7d82bd86`, with its `nanopass`, `lz4`, `zlib`, `zuo` and `stex` submodules
copied from the local Racket checkout at `50f1f60628`, both v10.5.x): `./configure --threads -m=tarm64osx` and
`./configure --nothreads -m=arm64osx`, each bootstrapped from the portable boot files with the same Apple clang.

**What the threaded arm64 build changes in generated code** [S]: `need-store-fence?` is true for arm64 when
`pthreads` is set, and `build-dirty-store` emits a `store-store-fence` (`dmb ishst`) before every store whose value
is not a fixnum (`s/cpprim.ss:652-723`, comment at :708-711; `s/arm64.ss:2012`). Initializing stores are never fenced. Observed with
`$assembly-output` for `(lambda (p x) (set-car! p x))`:

```
tarm64osx (threaded)                         arm64osx (non-threaded)
 tsti  %sp, %r1, 7                            stri  %r1, %r15, 0      ; store first
 bne   lf.2                                   tsti  %sp, %r1, 7
 stri  %r1, %r15, 0   ; fixnum: plain store   bne   lf.2
 ...                                          ...
lf.2:                                        lf.2:
 dmbishst             ; pointer: fence,       ldri  %td, %tc, 160      ; remember the slot
 stri  %r1, %r15, 0   ;   then store          ...
 ldri  %td, %tc, 160  ; remember the slot
```

Other threaded-only costs: the thread context through `%tc` for thread-local state, `S_tc_mutex` around interning
and symbol tables, `S_alloc_mutex` for segment allocation, `memory-order-acquire/release` fences, and a collector
that can run with helper threads (`gc-par.inc`).

**Result** (`chez/bench/bench.ss`, run with `--script`; time is `real-time` around the benchmark only; 2 batches ×
7 interleaved rounds; % is the median of the 14 paired ratios):

| Benchmark | What it stresses | non-threaded ms | threaded ms | threaded vs non-threaded | middle 50% |
|---|---|---|---|---|---|
| fib (41) | calls | 596.5 | 605.0 | +1.8% | −2.1…+4.4% |
| tak (24 16 8, ×300) | calls | 720.0 | 713.5 | −1.0% | −1.3…−0.6% |
| fibfp (39.0) | `fl+` | 356.5 | 357.0 | +0.1% | +0.0…+0.3% |
| generic-fp (37.0) | generic flonum arithmetic, boxing | 321.0 | 322.0 | +0.6% | +0.3…+0.6% |
| cons-churn (300 K × 1,000-element lists) | allocation, minor GC | 441.0 | 482.0 | **+8.1%** | +0.9…+11.4% |
| deriv (×10 M) | allocation | 442.5 | 439.5 | −0.4% | −1.6…+0.2% |
| setcar (fresh pair into an old 10 K list, ×30 K) | `dmb ishst` + store + remember per iteration | 563.5 | 551.0 | **−1.4%** | −4.0…+1.5% |
| vecset (fresh list into an old 10 K vector, ×30 K) | same, `vector-set!` | 538.5 | 557.5 | **+4.1%** | +2.3…+4.9% |
| intern (`string->symbol`, 3 M) | intern mutex | 259.0 | 389.5 | **+50.2%** | +49.2…+51.4% |
| eqtable (5 K pair keys × 20 K passes) | `eq` hashtable, address hashing | 634.5 | 561.5 | −9.5% | −16.3…−1.6% |
| ctak (×1,000) | `call/cc` | 459.0 | 456.0 | −0.5% | −1.1…+0.0% |
| strport (`with-output-to-string`, 400 M chars) | ports | 440.5 | 457.0 | +3.4% | −1.3…+8.6% |
| closure (600 M) | closure creation | 437.5 | 430.5 | +0.0% | −4.8…+3.2% |
| param (`parameterize`, 40 M) | dynamic binding | 440.0 | 438.5 | +0.6% | −2.0…+1.8% |
| gcbench (depth 14, ×9,000) | allocation, GC | 364.5 | 386.0 | **+5.9%** | +5.2…+6.6% |
| **geometric mean, 15** | | | | **+3.5%** | |
| **geometric mean without intern** | | | | **+0.8%** | |

**Reading.** setcar runs 3×10⁸ fenced pointer stores, each storing a pair allocated in the same iteration, in 551
ms (1.84 ns per iteration): a fence that cost even 1 ns would show as +50%. In compiled code on the M4 the
publication fence before each heap-valued store is free or nearly so (vecset +4.1%, ~0.07 ns per fence). The
threaded build's real costs are elsewhere: a global lock on a hot runtime path (intern, +50%) and allocation/GC
(+6–8%). Racket CS, built on this Chez, reports up to 6–8% for its parallel-threads work (`research/threads-prior-art.md`).

---

## 5. Published numbers [C]

| System | Single-thread cost of being parallel-capable | Source |
|---|---|---|
| OCaml 5 vs OCaml 4 (sequential suite, geomean) | **3.5%** (ParMinor: stop-the-world parallel minors), **4.9%** (ConcMinor: concurrent minors with a read barrier); 54–61% less memory | Sivaramakrishnan et al., "Retrofitting Parallelism onto OCaml", ICFP 2020, §6 "Evaluation" (https://arxiv.org/abs/2004.11663) |
| OCaml's memory model on AArch64 (Cavium ThunderX), vs no decoration | **0.6%** (`dmb ld` before each mutable store, "FBS"), **2.5%** (branch after each mutable load, "BAL"), **85.3%** (every mutable load `ldar`, store `stlr`, "SRA"; floats via `dmb`); POWER 2.9% / 26.0% / 40.8% | Dolan, Sivaramakrishnan, Madhavapeddy, "Bounding Data Races in Space and Time", PLDI 2018, §8.3 (https://kcsrk.info/papers/pldi18-memory.pdf). It also issues a full `dmb ish` at the end of every promotion/minor GC so initializing stores are visible |
| CPython free-threading (PEP 703) | estimated 5–6%; PEP 779 reports ~10% Linux/Windows, ~3% macOS; +15–20% memory | https://peps.python.org/pep-0703/, PEP 779 (threads-prior-art.md §7) |
| Go | `publicationBarrier()` (`DMB ST` on arm64) in every `mallocgc` path, so the collector never sees uninitialized memory | `src/runtime/malloc.go` (`publicationBarrier` after initialization, e.g. :1301-1308 in the cached copy), `src/runtime/atomic_arm64.s` (https://github.com/golang/go/blob/master/src/runtime/atomic_arm64.s) |
| HotSpot C2 on AArch64 | a StoreStore barrier (`dmb ishst`) after each allocation's header stores, plus a release barrier at constructor exit when final fields are written; JDK-8300148 replaces the latter by StoreStore when the object does not escape | https://bugs.openjdk.org/browse/JDK-8300148 |
| Chez Scheme | "Nonthreaded versions are also available and are faster for single-threaded applications" (no number given) | `csug/preface.stex:39-40` [S]; measured +0.8% / +3.5% in §4 |

---

## 6. The model: Patina's interpreter and JIT

### 6.1 Inputs

- **Interpreter speed.** 4.4 ns per instruction on the named-let loop and ~38 ns per `fib` call (`vm-runtime.md` §9);
  4.6–13.1 ns per instruction across the 12 workloads of §3, 5.7 on nboyer.
- **Mix** (jit-readiness §2.3, nboyer, 9.69 M dispatches): heap-reference loads ≈ 0.53 per instruction (LoadClosure
  0.160, ReadCell 0.117, Car 0.090, Cdr 0.056, the callee's code in Call 0.060 and TailCall 0.045); allocations
  0.032; heap stores 0.022. Over the 20-workload census (workload-demographics §4.1, §6): allocations 36–258 per
  1,000 instructions (libload 409); heap stores 0.1–70 per 1,000 (median ~13; libload's parser 249); 64% of heap
  stores carry immediates, so heap-valued stores are ~5 per 1,000 at the median, 11–42 on destruc, gcbench and
  gcold, ~200 in libload's parser.
- **JIT speed** [I]: 5–10× the interpreter, 0.45–0.9 ns per bytecode (workload-demographics §4.1's assumption).

### 6.2 (a) Today's representation made thread-safe

Requires an `RwLock` (or equivalent) around the heap, because the `Vec` arenas relocate on push
(`heap/mod.rs:703-714`) and `HeapObjectData` payloads are `Rc`/`RefCell`; `Arc` for code objects, environments and
the ~14 `Rc` payload kinds; `Mutex`es for ports and tables. Measured single-threaded (§3, `a_all`): **+4.1% geomean,
+0.17–0.42 ns per instruction**. The memory-ordering obligations are then subsumed by the lock's acquire/release.
The tree-walker would add an `Arc` clone and drop per call for `Rc<Environment>` and the heap handle (per-call
environments, `cps_eval/application.rs:79`), +0.6 ns per pair (§2.2): small against its per-call cost, and
irrelevant because it stays M:1 (DESIGN §8.4). **But every reader takes the lock's cache line**: at 0.6 borrows per
instruction and 54 ns per contended RMW at 4 threads (§2.11), four carriers pay ~65 ns per instruction, ~11× a
single thread's whole instruction. Allocation needs the write lock and serializes outright. (a) has a tolerable tax
and no parallel speedup.

### 6.3 (b) The redesign: interpreter

What remains under `threaded` (DESIGN §12): relaxed `HeapSlot` and `MetaByte` accessors (free, §2.1); acquire loads of
heap references and release stores of heap values in the funnel (§3 `order`: +0.00–0.22 ns per instruction, +1.9%
geomean today); per-mutator TLABs refilled under a pool lock (free, §2.5); sub-word atomic stores (free); the poll
protocol (free, §2.9); `MetaByte` RMWs in barrier slow paths (+0.4 ns per slow path, 5–131 K slow paths per run in
the barrier simulation: < 0.1 ms); a locked interner (+0.7 ns with `parking_lot` on a rare operation).

The redesign also *removes* single-thread costs that (a) would have made atomic: ~3 ns of `Rc` traffic per call
(§2.2), the `RefCell<Heap>` flag on ~60% of instructions, and the `Rc<Environment>` clone in every `LoadGlobal`
(`vm_state.rs:1357-1364`). If those removals make the interpreter ~30% faster per instruction, the same ~0.13 ns
becomes **+2–5%**. Two conditions keep it there: bulk operations get one word-atomic routine (otherwise 3.2–3.3× per
word, §2.10), and no per-carrier data shares a 128 B line (§2.11).

### 6.4 (b) The redesign: JIT

| Choice | Cost model | Estimate at 0.45–0.9 ns per bytecode |
|---|---|---|
| Heap loads as plain `ldr` (address-dependency ordering, documented exception) | 0 | **0** |
| Heap loads as `ldapr` (or Cranelift `atomic_load` → load-acquire) | 0.53 loads per bytecode × (0.05 throughput … 0.54 dependent) ns | **+3% … +60%** |
| Fence before each heap-valued store (Chez's placement; in the barrier's heap branch, after `tbz x1,#2`) | 0.005–0.04 heap-valued stores per bytecode (0.2 in the parser) × 0–0.07 ns (Chez in situ, §4) | **≈ 0–1%** typical; ≤ 3% on the parser; worst-case microbenchmark 15–54 ns per fence would give 8–100%+ on a store-dense loop |
| `stlr` for heap-valued stores instead | same count × 0.06 ns typical; 19–53 ns when the store follows fresh streaming stores to another line (§2.5) | ≈ 0–1% typical, same tail |
| Fence per allocation group (DESIGN §12, §5's JIT fast path) | 0.04–0.26 allocations per bytecode; ~0 ns in mixed code, **15–54 ns** in a tight loop that keeps `ap` in a register (§2.5 rows 2 and 6) | ≈ 0–4% typical; **10–100× on a tight `cons`/`make-list`/`list-copy` loop** |
| Poll, TLAB, `x21` context | 0 | **0** |
| **Total, following the two placement rules** | | **≈ 0–4%**, matching Chez's +0.8% geomean |

### 6.5 Not measured here

- Contended behaviour of the redesign itself (handshake latency, time-to-safepoint with N carriers, parallel
  minors): that is stage-9-and-later work; §2.11 bounds what sharing a line costs.
- Frontend, macro expander and library-loading `Rc` traffic (588 `Rc<` sites, threads-patina-cost §1) if those
  structures became shared between carriers: load-time cost, not per-instruction; unmeasured.
- Port operations under per-port locks: §2.4 gives +0.7–3 ns per lock; Chez's string ports ran +3.4% (±5%).
- x86-64: relaxed atomics and acquire loads are plain `mov` there and store-store order is free (TSO), so every
  ordering row above is 0 on x86-64 except RMWs and `SeqCst` stores; arm64 is the platform that pays.

---

## 7. Implications for the PRD

1. **Answer to "what does adding shared-memory parallelism later cost at run time":** for (b), ~2–5% for the
   interpreter and ~0–4% for a JIT on arm64 (0 on x86-64), or 0 with a separate non-threaded build. The cost is in
   engineering (itemize.md: 8–16 engineer-months after stage 9) and in getting two placement rules right.
2. **Fence placement (amend DESIGN §5 and §12).** Put the `threaded` publication fence on the store path, before a
   heap-valued store (the barrier's heap branch in JIT code, the funnel's heap branch in Rust), and not after JIT
   allocation groups. Every publication of an object to another carrier goes through such a store, a global-cell
   store, or a synchronizing operation (thread start, mutex, channel) that has its own release; the register stack is
   carrier-private. Evidence: Chez does this and pays ≈ 0 (§4); a per-allocation fence in a tight JIT loop costs 15–54
   ns per allocation on the M4 (§2.5). itemize.md's C1 (fence only when publishing an object allocated since the last
   fence) is compatible and further reduces the count. This revises itemize.md's "double digits on arm64 if every
   pointer store fences": its 12–57 ns per fence is the microbenchmark tail of §2.5, while in situ Chez fences every
   pointer store for ≈ 0–4% (§4) and Patina's interpreter fences every allocation for ≈ 0 (§3); C1 remains useful as
   insurance against the tail, not as the difference between a few percent and double digits.
3. **No acquire loads in JIT code** (DESIGN §12's "documented dependency-ordering exception" should become the rule
   for the JIT, with the Rust interpreter allowed `ldapr`): +3% to +60% otherwise.
4. **One word-atomic bulk routine** (`copy_range`/`fill_range`), or the bulk primitives lose 3.2–3.3× under
   `threaded`.
5. **128 B alignment for per-carrier mutable state** on Apple silicon, enforced by a static assertion on `Mutator`.
6. **Interning:** `FxHash` buys more than any lock costs; use `parking_lot` or sharding, never a recursive pthread
   mutex (Chez's +50%).
7. **A kill-criterion candidate** for the `threaded` feature, once it has bodies: the `threaded` build at N = 1 stays
   within 5% geomean of the default build on the GC benchmark set, with the per-mechanism split of §3 re-measured on
   the stage-5 representation; and the S6 spike reports a tight `cons` loop with and without its chosen fence.
8. **Issue candidates (to file, not fixed here):** (i) the S6 spike's checklist should include the fence-placement
   measurement of item 2; (ii) the interpreter's three `Rc<CodeObject>` clone/drop pairs per call cost ~3 ns of a
   ~38 ns call today (§2.2) and are removable before stage 5 by caching the code pointer in `CallFrame` without a
   refcount; (iii) `symbol_table` hashes with SipHash (`heap/mod.rs:319`), 2.3× slower than `FxHash` per lookup.
   Search existing issues first.

---

## 8. Reproduction and files

Paths are relative to the study's working directory. The programs are retained under `PRD/study/gc/probes/followup/`
(`par-measure/`, `agg.py`, `perturb_patch.py`, `perturb-wl/`, `chez/asm.ss`); build outputs, raw results, the
perturbed copy, the Chez builds and the papers were not (`PRD/study/gc/probes/README.md`):

| What | Where |
|---|---|
| Microbenchmark crate (pinned toolchain copied from the repo) | `par-measure/` (`src/main.rs`, `src/bin/barrier.rs`, `extract_asm.py`) |
| Build, run, assembly | `CARGO_TARGET_DIR=<target dir> cargo build --release --offline`; `target/release/par-measure --rounds 15 --target-ms 20`; `… --contention`; `target/release/barrier`; `cargo rustc --release --offline -- --emit asm` then `python3 extract_asm.py <.s> <symbols…>` |
| Raw results | `results/micro_run{1,2,3}.tsv`, `results/micro_agg.md` (via `agg.py`), `results/barrier_sweep_run{1,2}.txt`, `results/contention.tsv` |
| Perturbed Patina | `patina-perturb/` (patched by `perturb_patch.py` from a `cp -R` of `crates`, `lib`, `Cargo.*`, `rust-toolchain.toml`); binaries `target-pb-<variant>/release/patina`, built with `RUSTFLAGS="--cfg perturb_<variant>"` (`a_all` = `--cfg perturb_lock --cfg perturb_arc`) |
| Perturbation timing | `perturb-wl/drive.py 5` → `perturb-wl/results.json`, `results_5reps.tsv`; `perturb-wl/table.py results.json instrs.json` → `perturb_table.md`; workloads are `perturb-wl/*.scm` and the Larceny-derived set of `PRD/study/gc/probes/workload-demographics/instrumented/workloads/` with its reduced inputs (the Larceny sources are not retained) |
| Chez builds | `chez/src-t` (`--threads -m=tarm64osx`), `chez/src-nt` (`--nothreads -m=arm64osx`); logs `chez/make-{t,nt}.log` |
| Chez benchmarks | `chez/bench/bench.ss`, `chez/bench/drive.py 7` (run twice; `results_batch{1,2}.{md,json}`); asm probe `chez/asm.ss` |
| Papers fetched | `refs/retrofitting-parallelism-ocaml.pdf`, `refs/pldi18-memory.pdf` (text extracted locally; not retained) |

## 9. Patina code locations used

| Mechanism | Location (`28a94f8`) |
|---|---|
| `SharedHeap = Rc<RefCell<Heap>>` | `crates/patina-core/src/heap/mod.rs:51` |
| Per-instruction poll (`Cell<bool>`) | `crates/patina-vm/src/runtime/vm_state.rs:1204-1207` |
| Heap borrow per closure call / tail call | `vm_state.rs:1591`, `:1606` |
| `LoadClosure`, `Cons`, `Car`, `ReadCell`, `WriteCell` arms | `vm_state.rs:1451`, `:2106`, `:2119`, `:2395`, `:2407` |
| `VectorSet` through `vector_slice_mut` | `vm_state.rs:2380` |
| `frame_globals` (`Rc<Environment>` clone per `LoadGlobal`/`StoreGlobal`) | `vm_state.rs:1357-1364` |
| Code-object `Rc` clones | `vm_state.rs:575-579` (`code_object`), `control.rs:177`, `execution_state.rs:134-142` (`dispatch_frame`), `:55-82` (`push_frame`, `pop_frame`) |
| Allocation | `heap/mod.rs:703-714` (`alloc_pair`), `:1469-1480` (`alloc_object`), `:581-584` (`note_alloc`) |
| Slot access | `heap/mod.rs:731-739` (`car`, `cdr`), `:743-760` (`set_car`, `set_cdr`), `:797-807` (`vector_ref`, `vector_set`), `:1260-1287` (`read_mutable_cell`, `write_mutable_cell`), `:1331-1339` (`get_vm_closure_code_id`), `:1380-1389` (`get_vm_closure_free_var`) |
| Interning | `heap/mod.rs:319` (`symbol_table`, SipHash), `:981` (`intern_symbol`) |
| Chez's threaded store fence | Chez `s/cpprim.ss:652-723`, `s/arm64.ss:924-927, 2012`; intern mutex `c/intern.c:162-190`, `c/thread.c:388-404`, `c/types.h:402-410` |
