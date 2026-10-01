# Shared-memory parallelism in Patina: an itemized cost, today and after the GC redesign

Date 2026-10-01. Repository `main` at `28a94f8`, unmodified. Inputs: `design/DESIGN.md` (§9, §11, §12, §15,
Appendix D, E.7, H.4), `DIGEST.md`, `research/threads-{prior-art,patina-cost,recommendation}.md`,
`understand/*.md`. Intended home: the research corpus under `PRD/study/`, with decision 7's row in `PRD/GC_PRD.md`
linking here.

**Conventions.** **[S]** is a count or a line read in source at `28a94f8` (the `rg` pattern is given, over
`crates/*/src`, which includes `#[cfg(test)]` modules inside `src`). **[P]** is a measurement taken for this note
on the development machine (Apple M4 Pro, 8 performance + 4 efficiency cores, macOS 27.2 arm64, Rust 1.97.1,
release, LTO), with its program in `PRD/study/gc/probes/followup/`. **[I]** is judgement. Effort is in focused engineer-weeks for
one engineer who knows the codebase, as in the design's Appendix D. "(a)" is the cost from today's code; "(b)" is
the cost after stages 0–9 of `DESIGN.md` have landed as specified, with its N-ready rules followed. "Rnn" names a row
of §4.

**Errata after review (2026-10-01).** `ANSWER.md` in this directory supersedes this note where they differ:
- **Rows.** It adds R32 (pacing across carriers), R33 (native stack depth) and R34 (promise forcing and the source
  map), and widens R26. The totals become 79.5–145.5 weeks from today and 38–77 after the redesign.
- **Fences.** The "double digits on arm64" and 12–57 ns fence figures (§0, §3, §5) are an intermittent
  microbenchmark anomaly that in-situ code did not reproduce.
- **Additions.** C1 is valid only with a standalone store-store fence. C7 is restricted to stage P. C11 is staged by
  the stage at which each type qualifies. C13, C14 and C16 leave the additions (C16's scan was mostly internal
  defines).
- **The token** (§8 step 3) needs no `threaded` heap accessors.

---

## 0. Bottom line

| | (a) from today | (b) after stages 0–9 | (b) plus the cheap additions of §7 |
|---|---|---|---|
| Parallel-specific work (rows R1–R31) | **77–137 weeks (≈ 18–32 engineer-months)** | **36–69 weeks (≈ 8–16 engineer-months)** | **26–52 weeks (≈ 6–12 engineer-months)** |
| SRFI 18 on M:1, which parallelism presupposes (row S) | 12–19 weeks | 6–11 weeks | 6–11 weeks |
| Making a JIT multi-carrier (row J), only if a JIT exists | no JIT today; 6–10 weeks to retrofit one built without the §11 contract | 2–4 weeks | 1–3.5 weeks |
| Risk | high: touches every crate; repeats most of stages 2–5 under another name | medium-high: concentrated in the handshake, publication, scheduler and test rows | the same rows, with the protocol kernels tested from stage 3 |
| Single-thread tax of the `threaded` build | est. 5–15% VM, 15–40% tree-walker (ST, unmeasured) | est. 2–6% VM if fences are filtered (§5); double digits on arm64 if every pointer store fences [P, §3] | same; or 0 with a separate non-threaded build (Chez's split) |

- **The redesign roughly halves the cost.** About 41–68 of (a)'s weeks are work stages 1–9 do anyway (the block heap,
  the `Cx` codemod, precise rooting, heap continuations, cells, the `GreenThread` split), so doing parallelism first
  would mean building those pieces twice. Nothing in stages 0–9 has to be undone.
- **This itemization is higher than decision 7's figure** (4–9 engineer-months after stage 9, from the
  threading-model study, ST). The rows ST did not price add about 11–20 weeks: the M:N scheduler delta (R26), the
  per-heap Rust tables the redesign itself introduces (R9), library-loading and expander concurrency (R5, R6),
  time-to-safepoint (R17), the debugger (R27), embedding (R28), a memory-model statement (R29) and the bundled-library
  audit (R25). ST's own rows land within or near its ranges.
- **The five most expensive rows in (b)** are tests (R30, 4–7), performance recovery (R31, 3–6), the scheduler
  across carriers (R26, 3–5), the heap/capability split (R11, 2–4) and the NoGcScope windows (R16, 1.5–4).
  The riskiest are publication (R13), handshakes and safe regions (R15), the scheduler (R26) and continuations
  across carriers (R21), the last because transfers are where this codebase's defects have historically clustered.
- **The tree-walker stays M:1 and `!Send` in both (a) and (b)** (R23). Parallelizing it would cost 8–14 weeks and
  turn its per-call `Rc` clones into atomic ones, at +1 ns uncontended and 85–286 ns at 4–8 threads on a shared
  count [P].
- **Measured hazards that reshape (b)** (§3): a publication fence costs 12–57 ns when recent allocations are not yet
  in L1 (`dmb ish`, `dmb ishst` and `stlr` alike on M4) against 1.5 ns for a pair allocation; element-wise relaxed
  atomics make bulk scans 3.4–3.8× slower than plain vectorized code; a shared `Mutex` costs 4 ns alone and 44–94 ns
  at 4–8 threads. With today's GC pause share of 2.4–21.7% of run time, a serial stop-the-world collector caps 8
  carriers at 3.2–6.9× (Amdahl), so the carriers must do the GC work in parallel, minors included (R18).
- **Cheapest large lever.** Reserve a publication watermark (`fenced_ap`) and a quiescence epoch in the `Mutator`
  ABI block before stage 6 freezes it, and define the fence rule as "fence only when publishing an object allocated
  since this mutator's last fence" (C1, C10). That costs days now and is the difference between an ABI break plus
  a double-digit arm64 tax and a filtered few-percent one later.
- **Staging exists.** The cheapest first useful step after stage 9 is N carriers that share one "mutator token"
  (one runs Scheme, the others sit in safe regions doing blocking I/O): it exercises the handshake, safe regions and
  the scheduler without any data race, before the token is released for real parallelism (§8).

---

## 1. Target and assumptions

**Target.** N OS-thread *carriers* (mutators, §11's `Mutator`) over one shared heap; SRFI 18 threads are green
threads scheduled M:N over the carriers (1:1 is the degenerate case); collection stays stop-the-world (all carriers
parked or in safe regions), with parallel marking. This is the shape of Gambit SMP, OCaml 5, Racket 9's parallel
threads and Loom (`research/threads-prior-art.md` §0). Production quality means: `threaded` build lanes,
sanitizer and model-checking lanes, every existing scoreboard green on the threaded build with one carrier, and a
declared single-thread tax.

**What (b) assumes is in place** (from `DESIGN.md` §12, §15, Appendix D):
- stage 2: slot visitor, open `RootSet`, `Owned` handles, teardown, rooted loading at points A and B;
- stage 3: `#[repr(C)] Mutator`, the collect capability, `Cx<'gc>` over the ~1,300 heap call sites, the store funnel
  (`HeapSlot`, `MetaByte`), the `threaded` cargo feature with accessor bodies only (linted, never run), polls at
  frame entry, the `InterruptHandle` with remote-post protocol;
- 4a ports in a `PortTable` with a process-wide registry; 4b global cells and binding records (variant R; C after
  stage 5 does not change any row here); 4c identifiers as ids; 4d non-relocating register stacks; 4e continuations
  as immutable heap objects and traced code liveness; 4f/4g host payloads and inline payloads;
- stage 5: block heap with per-mutator bump buffers and a block pool behind an uncontended mutex, no `RefCell`,
  no `Drop` payloads, immortal and descriptor spaces; stage 7 per-mutator store buffers; stage 8 evacuation;
- stage 9: `GreenThread` split, thread/mutex/condvar objects, the root rule, `FinalKind::Thread`, a minimal internal
  scheduler, deep-bound `parameterize`, current ports in the dynamic environment, safe regions with their entry rule,
  blocking primitives as `Transfer` with Rust-buffer restructuring, the two-mutator lane;
- stage P: parallel stop-the-world marking on per-heap GC workers.

**What (b) does not assume:** a production JIT (stage 6 is a throwaway spike that freezes the ABI; row J prices the
JIT delta separately); the SRFI 18 library on top of stage 9 (row S); isolates.

**Basis of the estimates.** Site counts below, times a per-site rate where the work is mechanical (as the design's
codemod estimates do), and analogy with the systems that did this: OCaml 5 (ParMinor: 3.5% sequential cost),
Racket 9 parallel threads ("up to 6–8%" for programs that do not use them, from locks "taken whether or not any
parallel threads are active"; Racket blog 2025-11), CPython free-threading (about 10% on Linux and Windows, about 3%
on macOS, 15–20% more memory; PEP 779), HotSpot handshakes (<1%, JEP 312). All weeks are [I].

---

## 2. Ground truth today [S]

| Quantity | Count | Pattern / place |
|---|---|---|
| `Rc<` mentions by crate | core 233, frontend 78, vm 85, tree-walker 62, runtime 40, macros 38, primitives 22, interpreter 17, repl 5, ir 2 (**582**) | `Rc<` |
| `RefCell<` / `Cell<` (not `RefCell`) | 113 / 29 | `RefCell<`, `(^\|[^f])Cell<` |
| Heap borrow sites | primitives 406, frontend 185, macros 61, vm 58, tree-walker 30, core 15, runtime 6, compat 3, repl 2, interpreter 1 (**767**; ST's broader pattern found 787) | `heap(\(\))?\.borrow(_mut)?\(\)` |
| `SharedHeap = Rc<RefCell<Heap>>` mentions | 590 (primitives 321, frontend 126, macros 54, compat 32) | `crates/patina-core/src/heap/mod.rs:51` |
| Allocation call sites | 434 in 62 files | `\.alloc_[a-z_]+\(` |
| Store paths that bypass or are the funnel | 38 calls in 12 files (`set_car`, `set_cdr`, `vector_set`, `write_mutable_cell`, `vector_slice_mut`, `get_string_chars_mut`, `get_bytevector_mut`) plus record fields, parameters and promises through cloned `Rc` handles | design §7 |
| `HeapObjectData` variants with `Rc` payloads / interior mutability | 14 / 5 of 28 | `heap/mod.rs:143-229` |
| Types that directly hold `Rc`, `RefCell`, `Cell`, `Weak` or `SharedHeap` | **66** (core 30, frontend 11, vm 7, macros 6, tree-walker 4, runtime 3, primitives 2, repl 2, ir 1); more are `!Send` transitively | `PRD/study/gc/probes/followup/nonsend_types.py` |
| `thread_local!` | 11 blocks, 19 statics: `CURRENT_{INPUT,OUTPUT,ERROR}_PORT` (`primitives/io/ports.rs:41-44`); `STDIN_{UNREAD,POSITION,FOLD_CASE,CARRY}`, `OUTPUT_FILES` (`core/port.rs:157-179`); `EMPTY_CONT_ENV`, `SCOPE_ORIGINS`, `PHASE`; `PENDING_ESCAPE`, `ACTIVE_TRAMPOLINES`, `NEXT_TRAMPOLINE`, `UNHANDLED_IN_CALLBACK` (`tree-walker cps_eval/types.rs:20,66-69`); `EMPTY_REENTRY`, `EMPTY_HANDLERS` (`vm control.rs:1657,2114`); macro `TRACER`; diagnostics `OUTPUT` | `thread_local!` |
| Process-global statics | 16, all already `Sync` (atomic id counters, `ERROR_REPORTED`/`INTERRUPTED_EXIT`, `MACRO_DEBUG_ENABLED`, two `OnceLock<Mutex<Sink>>`, `PROGRAM_START`, the char-class `CACHE`) | `^\s*static` outside `thread_local!` |
| `unsafe` | 8 blocks (vm 6, repl 2), 0 `unsafe fn`, 0 `unsafe impl Send/Sync` | |
| Interning | `intern_symbol(` 40 calls in 24 files; `symbol_table: HashMap<String, HeapIndex>` (`heap/mod.rs:319`); `core_syntax_table` (:332); `Symbol(Rc<str>)`; `Rc<str>` 158 mentions | |
| Deferral and Rust callbacks | `GcDeferGuard` 21 mentions in 13 files; 20 `apply_proc`/`eval_expr`/`run_synchronously` call sites in 11 files | |
| Production root providers | 6 (`VmState`, `LibraryRegistry`, `StepTracer`, tree-walker `Evaluator`, `EscapeRoots`, `StepRoots`) | `impl.*GcRoots for` |
| VM machine state | `VmState` mixes per-thread (`execution`, `pending_escape`, `reentry`, `scratch_args`), per-heap (`code_store: Vec<Rc<CodeObject>>` :110, `globals: Rc<Environment>` :164, `primitive_registry: Rc<…>` :168, `shadowed_primitives` :174, continuation tables `RefCell<FxHashMap<u64, Rc<…>>>` :196-199, `library_registry: Rc<RefCell<…>>` :204) and GC state (`gc: RefCell<GcController>` :214, `gc_pending: Rc<Cell<bool>>` :218) | `vm_state.rs` |
| Caches with interior mutability | `CodeObject.global_cache: Vec<Cell<GlobalCacheEntry>>` (12 B, `code_object.rs:165`), `live_closures: Cell<u32>` (:173), `Procedure::Primitive.registry_index: Cell<Option<usize>>` (`procedure.rs:49`) | |
| Libraries | `LibraryRegistry` behind `Rc<RefCell<…>>` (5 mentions; `library_registry` 58), one `loading_stack: Vec<Vec<String>>` for cycle detection (`library_registry.rs:302`), `Library { exports: HashMap<String, TaggedValue>, env: Rc<Environment> }` | |
| Ports | `Port { data: Rc<RefCell<PortData>>, pushback, fold_case: Rc<Cell<bool>>, position }` (`port.rs:20-40`); `ReadPort`/`WritePort: Send` already (`vfs.rs:86,90`) | |
| Blocking primitives | about 35 entry points: 11 input (`read-char`, `peek-char`, `char-ready?`, `read-line`, `read-string`, `read-u8`, `peek-u8`, `u8-ready?`, `read-bytevector`, `read-bytevector!`, `read`), about 14 output/flush/close, 6 file, the directory primitives, `load`/`include`, the loader's file reads | `primitives/io/mod.rs` registrations |
| Primitive registrations | 306 `PrimitiveFn::new_*` (282 heap, 19 higher-order, 5 resumable) | |
| Mutable top-level state in bundled Scheme | 16 variables in 6 files: `srfi/115.scm` (4: `current-match`, …), `srfi/115/boundary.scm` (3), `srfi/158/srfi-158-impl.scm` (3), `srfi/128/128.body2.scm` (2), `srfi/27.scm` (1), `r6rs/hashtables.atop69.scm` (1); plus shallow-bound `parameterize` (`lib/scheme/base/parameters.scm`) | `PRD/study/gc/probes/followup/scm_globals.py` (heuristic) |
| Concurrency tooling | none: no Miri, loom, TSan or arm64 Linux lane; CI is x86-64 Ubuntu plus macOS, stable 1.97.1 pinned | `.github/workflows/ci.yml` |

**`Send`/`Sync` today, compiled** [P, `PRD/study/gc/probes/followup/sendcheck`, an auto-trait probe over 28 public types]: only
`TaggedValue`, `GcController`, `SourceLocation`, `PrimitiveRegistry`, `SourceMap` and `VmBackendError` are
`Send + Sync`. `Heap`, `SharedHeap`, `HeapObjectData`, `Environment`, `Port`, `CompiledMacro`, `Library`,
`Procedure`, `CpsExpr`, `CoreExpr`, `LibraryRegistry`, `LibraryLoaderRegistry`, `Step`, `Parser`, `Desugarer`,
`CodeObject`, `VmState`, `VmBackend`, `Evaluator`, `TreeWalker` and both `Interpreter<…>` instantiations are
neither. `PrimitiveRegistry` is already shareable: it holds `fn` pointers.

---

## 3. Measurements taken for this note [P]

Programs: `PRD/study/gc/probes/followup/{atomtax,pubfence,contend}`; best of 5–20 repetitions, three runs; numbers
are ranges over runs.

| Operation | Plain / single-threaded | Threaded form | Consequence |
|---|---|---|---|
| Scan 1 M words (sum) | 0.07–0.08 ms (vectorized) | relaxed `AtomicU64`: 0.24–0.27 ms (**3.4–3.8×**); acquire: 3.6–4.1× | bulk primitives over atomic slots lose SIMD: keep one bulk-copy implementation (C4) |
| Random pointer chase, 1 M steps | 7.8–9.6 ns/step | relaxed: same; acquire: −1% to +8% | scalar heap reads are free as relaxed; acquire is cheap but not free |
| Fill 1 M words | 0.39–0.45 ms | relaxed or release stores: within ±6% | scalar stores are free |
| Pair allocation + init, streaming 64 MiB | 1.5 ns/pair | relaxed stores: 1.5 ns | initializing stores need no atomics cost |
| Publication fence after k ≥ 4 fresh pairs, allocations streaming into cold memory | — | `dmb ish` +47–53 ns, `dmb ishst` +50–52 ns, `stlr` +49–57 ns per fence | the fence waits for the store buffer to drain; on M4 the store-store form is no cheaper |
| Same, allocations L2-resident (256 KiB) | — | +12–18 ns per fence (any form) | |
| Fence after a store that hits L1 | 0.22–0.24 ns/store | +0.22–0.25 ns | fences are cheap only when nothing is in flight |
| Metadata byte RMW | 0.39–0.41 ns | `fetch_or`: 0.72–0.76 ns | `MetaByte` under `threaded` costs ~0.35 ns per barrier slow path or hash |
| `Rc` vs `Arc` clone+drop | 2.2–2.3 ns | `Arc`: 3.1–3.4 ns; one `Arc` shared by 2/4/8 threads: 15.5 / 84.6 / 285.8 ns | never put a per-call refcount on the shared path (tree-walker, environments) |
| `RefCell::borrow_mut` vs `Mutex` | 1.25–1.33 ns | `Mutex` 4.0–4.1 ns; one `Mutex` shared by 2/4/8 threads: 11.4 / 43.6 / 94.0 ns | per-port and per-table locks cost ~3 ns uncontended; contention needs sharding or ownership |

**Reading the fence numbers against the census.** The design's store census (DIGEST "Store mix") gives a median of
about 13 heap stores per 1,000 instructions, 36% of them heap values, so about 4.7 pointer stores per 1,000
instructions. At 0.2–57 ns per fence that is 1–270 ns per 1,000 instructions: from noise to a multiple of the run
time on allocation-heavy code. The real figure depends on how many stores are in flight at each publication, which no
census measures yet. Hence C1: fence only when the stored value was allocated since the mutator's last fence, and
count that event in the stage-0 census.

---

## 4. The itemized cost

### 4.1 Summary table

"Removed by" names the stages whose delivery is the difference between (a) and (b). "Stage?" says whether the row
can ship in pieces behind the `threaded` feature or a carrier count of 1.

| # | Item | (a) wk | (b) wk | Removed by (b) | Risk | Stage? |
|---|---|---|---|---|---|---|
| R1 | Heap: stable addresses, no `RefCell` around the heap | 6–10 | 0 | 5 (5e) | high / — | yes |
| R2 | `Rc`/`RefCell` payloads inside heap objects | 3–5 | 0 | 4g, 5c–5e | med / — | yes |
| R3 | Environments and namespaces | 3–5 | 1.5–3 | 3 (`Environment.heap`), 4b | med | yes |
| R4 | Code store, code-object caches, code liveness | 2–4 | 0.5–1 | 4b, 4d, 4e | low–med | yes |
| R5 | Libraries and concurrent loading | 2–3 | 1.5–2.5 | 2, 4b | med (deadlock) | yes |
| R6 | Macros, expander and syntax data | 2–4 | 1–2 | 3, 4c, 4g | low | yes |
| R7 | Ports, stdin, the output-file registry | 2–3 | 1–2 | 4a, 9 | med (perf) | yes |
| R8 | Interning: symbols, core syntax, scope sets | 1–2 | 0.5–1 | 5c (one interner) | low | yes |
| R9 | Per-heap Rust tables introduced by the redesign | — | 1.5–3 | (new cost) | med | yes |
| R10 | `Send`/`Sync` bounds and the audit | 2–4 | 1–2 | 3, 5e | low | yes |
| R11 | Heap/`Cx` API, the collect capability, the VM machine split | 8–13 | 2–4 | 3 | high / med | partly |
| R12 | Slot-access atomics switch, sub-word stores, bulk operations | 2–4 | 1–3 | 3, 5 | med (perf) | yes |
| R13 | Publication fences (funnel, JIT allocation groups) | 1–2 | 1–2 | 3 (one funnel) | **high** | yes |
| R14 | TLABs, block pool, LOS and immortal allocation, pacing across mutators | 2–3 | 0.5–1 | 5 | low | yes |
| R15 | Safepoint handshake, safe regions, blocking I/O | 4–7 | 2–3 | 3 (§9 protocol), 9 | **high** | yes |
| R16 | Precise rooting, the remaining `NoGcScope` windows, carrier pinning | 4–8 | 1.5–4 | 2, 3, 4e | high / med | yes |
| R17 | Time-to-safepoint in long Rust primitives | 1–3 | 1–2 | 1, 3 (`Step::Collect`) | med | yes |
| R18 | Parallel GC with N carriers: marking incl. minors, root scan, sweep claiming | 3–6 | 1.5–3 | P | med | yes |
| R19 | Store buffers and barrier logs | 0 | 0.5–1 | (new cost, stage 7) | low | yes |
| R20 | Ephemerons, weak tables, finalization | 2–3 | 0.5–1 | 4a, 4e, 5a | low | yes |
| R21 | Continuations and `dynamic-wind` across carriers | 3–5 | 1–2 | 4e, 9 | med–high | yes |
| R22 | Inline caches, shadow bitsets, per-site deoptimization | 1–2 | 0.5–1 | 4b | low | yes |
| R23 | Tree-walker kept M:1 (enforced) | 0.5–1 | 0.5 | 9 (§8.4 rule) | low | — |
| R24 | Global state in primitives and runtime | 1–2 | 0.5–1 | 2, 4a, 9 | low | yes |
| R25 | Bundled Scheme libraries with global mutable state | 1–2 | 0.5–1 | 9 (`parameterize`) | low | yes |
| R26 | M:N scheduler; SRFI 18 blocking operations across carriers | 4–7 | 3–5 | 9 | **high** | yes |
| R27 | Debugger, tracer and hooks across carriers | 2–3 | 1–2 | 2, 3 | med | yes |
| R28 | Embedding API, host threads, signals | 2–3 | 1–1.5 | 2, 3 (`Owned`, `InterruptHandle`) | med | yes |
| R29 | Memory-model statement for racy Scheme programs | 0.5–1 | 0.5–1 | — | low | — |
| R30 | Test infrastructure | 6–10 | 4–7 | 5a (Miri), 9 (two-mutator lane) | med | yes |
| R31 | Performance recovery and scaling | 6–12 | 3–6 | 3, 5 | **high** | yes |
| | **Total, parallel-specific** | **77–137** | **36–69** | | | |
| S | SRFI 18 on M:1 (prerequisite) | 12–19 | 6–11 | 9 | med | yes |
| J | A JIT made multi-carrier (only if a JIT exists) | n/a (6–10 to retrofit) | 2–4 | 6 (frozen contract) | high | yes |

### 4.2 Detail per row

Each entry: **what and why**; **sites today** [S]; **(a)**; **(b)** and what the redesign removed; **risk**;
**staging**.

**R1. Heap with stable addresses.** A concurrent `car` must never read an arena that another carrier's `push` is
reallocating, and Rust treats any racy plain access as undefined behaviour even when the Scheme program is the one
racing. *Today:* four growable `Vec` arenas (`heap/mod.rs:307-316`; `alloc_pair` pushes, :710), global free lists
(:367-376) and the whole heap behind `Rc<RefCell<Heap>>` (:51). *(a)* 6–10: a chunked or reserved-VA arena with
index runs as TLABs (ST's estimate), leaving `HeapObjectData` in place. *(b)* 0: stage 5's reservation, blocks,
side metadata and no `RefCell` (5e). *Risk* high in (a): it is half of stage 5 done in a form stage 5 then replaces.

**R2. `Rc`/`RefCell` payloads inside heap objects.** Shared objects with non-atomic counts or `RefCell` flags are
data races. *Today:* 14 of 28 variants carry `Rc` payloads, 5 use interior mutability (`Record.fields`,
`Parameter.values`, `Promise`, `MutableCell`, `Ephemeron`; `heap/mod.rs:143-229`), and 47% of allocations carry a
`Drop` payload (DESIGN §3). *(a)* 3–5: `Arc` plus locks or atomics per variant, paying `Arc` traffic (§3). *(b)* 0:
4g and 5c–5e make every layout plain words; Rust-owned resources sit in per-heap tables (counted in R9).

**R3. Environments and namespaces.** Top-level `define`, `import`, `eval` and `environment` mutate name tables that
every carrier reads. *Today:* `Rc<Environment>` 162 mentions; `Environment` holds `SharedHeap`, three `RefCell`
tables, two `Cell` flags and a `OnceCell` (`environment.rs:487-547`), mutated through `&self` (`define`, :712;
`set`, :920; `define_introduced_global`, :1182); `VmClosure.globals: Rc<Environment>` on every closure. *(a)* 3–5:
`Arc` plus `RwLock` or per-slot atomics with re-entrancy audits (a `RefCell` borrow held across a call becomes a
`RwLock` deadlock). *(b)* 1.5–3: 4b moves the VM's global path to immortal cells and binding records reached through
link tables and deletes `VmClosure.globals`, `GlobalCacheEntry` and `FORWARDED`; stage 3 deletes `Environment.heap`.
What remains is the namespace map itself (name → binding record, plus the scoped and alias tables the expander
reads): it needs a read-mostly concurrent structure (copy-on-write snapshot with a lock for inserts), and
re-pointing `record.cell` becomes a release store. *Risk* medium. *Staging:* per namespace kind.

**R4. Code store and code objects.** Every carrier runs code that any carrier compiled. *Today:*
`code_store: Vec<Rc<CodeObject>>` per `VmState` (:110), `Rc<CodeObject>` 27 mentions (cloned per frame),
12-byte `Cell<GlobalCacheEntry>` caches that do not fit one atomic word, `live_closures: Cell<u32>`, code ids
meaningful only inside one `VmState`. *(a)* 2–4 (ST): a machine-wide lock-protected or append-only store, packed
caches. *(b)* 0.5–1: 4b deletes the global caches, 4e makes descriptors heap objects with traced liveness and deletes
the `Rc` side vector and `live_closures`. Left: a lock on `CodeStore` insert and release (rare), a static
`CodeBody: Sync` assertion, and the unit `escaped` bit set with `fetch_or` (a carrier other than the creator can
only reach an escaped unit, so eager release stays sound).

**R5. Libraries and concurrent loading.** Two threads importing the same library must load it once, the second
waiting; cycle detection must be per thread. *Today:* `LibraryRegistry` behind `Rc<RefCell<…>>`, one shared
`loading_stack` (`library_registry.rs:302`) that would mix two threads' load chains, `exports` as copied values.
*(a)* 2–3. *(b)* 1.5–2.5: stage 2's `Loading` entry and rooted loading, 4b's `exports → CellRef`. Left: a
per-library once-protocol (Java class-initialization locks are the model), cross-thread cycle detection (A loads X
which needs Y while B loads Y which needs X: one must fail, not deadlock), and waiting for another thread's load as a
blocking operation in a safe region. *Risk* medium: a deadlock class the scoreboards cannot see.

**R6. Macros, expander and syntax data.** Macro bodies and syntax objects created on one carrier are expanded on
another. *Today:* 18 `!Send` frontend, macro and IR types (`Parser`, `Desugarer`, `Matcher`, `Renames`, …);
`CompiledMacro.heap: SharedHeap` (`compiled_macro.rs:483`); `Rc<str>` 158 mentions (identifiers, macro names,
patterns). *(a)* 2–4: `Arc<str>` or interning, `Sync` macro bodies. *(b)* 1–2: stage 3 deletes `CompiledMacro.heap`,
4c makes identifiers ids (no `Rc<str>` in syntax), 4g puts macro bodies in host payloads. Left: `HostPayload`
implementations shared read-only across carriers (`Send + Sync`), the remaining `Rc<str>` in compiled rules.
Expansion-local types stay thread-confined: a green thread mid-expansion has Rust frames on its carrier and cannot
migrate (R16).

**R7. Ports.** Ports are shared objects with buffers; SRFI 18 programs write to `current-output-port` from many
threads. *Today:* `Rc<Port>` 29 mentions with four `Rc`-shared inner cells (`port.rs:20-40`); current ports and
stdin state are `thread_local!` (`ports.rs:41-44`, `port.rs:157-179`), so a second OS thread would see another
stdin. *(a)* 2–3: `Arc<Mutex<PortData>>`, process-global stdin lookahead, the exit flush registry. *(b)* 1–2: 4a's
`PortTable`, canonical ports and process-wide registry (R1 of §6.7); stage 9's current ports in the dynamic
environment. Left: a lock per table entry (or an owner-biased lock with revocation through a handshake), stdin
lookahead as one process-wide object, blocking reads in safe regions. *Cost:* +2.8 ns per operation uncontended,
11–94 ns contended [P]; Racket names ports and mutable hash tables as most of its 6–8%. *Risk* medium (throughput).

**R8. Interning.** `string->symbol` racing on two carriers must answer one symbol. *Today:* `intern_symbol(` 40
calls in 24 files over a `HashMap<String, HeapIndex>` on the heap; `core_syntax_table`; scope sets interned nowhere
yet. *(a)* 1–2: a sharded concurrent interner, symbol names off `Rc<str>`. *(b)* 0.5–1: 5c's one interning function
over immortal symbols. Left: shards (or a lock-free table), immortal-space allocation from any carrier (R14), and
4c's scope-set interning table, which the redesign adds.

**R9. Per-heap Rust tables introduced by the redesign.** Stages 2–5 create Rust-owned tables that today's design
mutates through the single `Cx`: `PortTable`, `CodeStore`, `HostPayloadTable`, `FinalRegistry` (young and old),
`EphemeronTable` (GC only), the handle table, `RootSet`, the thread table, the external-bytes and pacing counters.
Under N carriers each needs a lock, an atomic, or a per-mutator young list merged at collection. *(a)* — (their
equivalents are inside R1 and R2). *(b)* 1.5–3. With C3 (per-mutator registration lists and one wrapper type from
the start), 0.5–1.

**R10. `Send`/`Sync` bounds and the audit.** The compiler is the cheapest race detector. *Today:* 22 of 28 key
types are neither `Send` nor `Sync` [P]; 66 types hold `Rc`/`RefCell`/`Cell` directly [S]; the only existing bounds
are `FileSystem: Send + Sync`, `ReadPort`/`WritePort: Send`, `Backend::Error: Send + Sync`. *(a)* 2–4 beyond what
R2–R8 force. *(b)* 1–2: the heap and VM types become plain words and ids by 5e. Left: `HeapShared: Sync`,
`GreenThread: Send` (it migrates between carriers), `CodeBody: Send + Sync`, `HostPayload`/`RootProvider:
Send + Sync` for VM heaps (the collecting carrier traces providers registered by others), `Owned` and `RootToken`
over `Arc`-weak tables, each justified `unsafe impl` with a test.

**R11. Heap/`Cx` API, the collect capability and the VM machine split.** Each carrier needs its own allocation and
mutation context at the same time, and collection needs "every carrier stopped", not "`&mut Heap`". *Today:* 767
heap borrow sites, 590 `SharedHeap` mentions, 434 allocation sites; `VmState` mixes per-thread, per-heap and GC state
(§2). *(a)* 8–13: the stage-3 codemod (the design prices it at 10–13 weeks with the `Mutator` and funnel) plus the
`VmState` split. *(b)* 2–4: the codemod is done and primitives already take `&mut Cx<'gc>`. Left: split `Heap` into
`HeapShared` (the ABI block already points at one, `Mutator` offset 0x50) and per-carrier `Mutator`s; replace the
`&mut Heap` capability by a per-carrier driver token whose `safepoint()` either collects (N = 1) or joins the
handshake (C2 does this at stage 3 for days, not weeks later); split `VmState` into `VmShared: Sync` (code store,
namespaces, registries, `Arc<PrimitiveRegistry>`, `Arc<dyn FileSystem>`) and per-carrier state; one driver loop per
carrier. *Risk* medium: the brand stays sound per carrier only if a carrier acknowledges a handshake with no `Cx`
window open, which the per-carrier token makes a type fact.

**R12. Slot-access atomics, sub-word stores and bulk operations.** Racing Scheme stores must not be undefined
behaviour in Rust. *Today:* no accessor layer; 38 store calls plus `Rc`-bypassing record, parameter and promise
writes. *(a)* 2–4 to build the layer. *(b)* 1–3: `HeapSlot` and `MetaByte` exist with `threaded` bodies (relaxed
`AtomicU64`, `fetch_or`). Left: run the feature and fix what breaks; atomic `u32` stores for `string-set!` and `u8`
for `bytevector-u8-set!` (a design obligation); about 23 bulk-copy sites in `vectors.rs`, `strings.rs`,
`bytevectors.rs`, `lists.rs` and `io/text_output.rs` (`copy_from_slice`, `extend_from_slice`, `to_vec`, …), whose element-wise
atomic rewrite would be 3.4–3.8× slower on scans [P] unless a word-atomic copy routine replaces `memcpy` in one place
(C4); GC access under stop-the-world through raw pointers.

**R13. Publication fences.** Holes are never zeroed (DESIGN §5), so a carrier that sees a reference before the
initializing stores would read a dead object's words as references: a memory-safety bug, not just a Scheme-level
race. arm64 (the development platform) needs store ordering before publication; x86-64 does not. *Today:* nothing
to publish from (arenas). *(a)* 1–2 after R1. *(b)* 1–2: stage 3 makes the funnel the one Rust publication point and
the JIT's allocation groups the other. Left: implement and measure. A fence costs 12–57 ns whenever recent
allocations are still in flight, the same for `dmb ish`, `dmb ishst` and `stlr` [P]; so the rule must be filtered
(C1: fence only when the value was allocated since the mutator's last fence; retire a TLAB with one fence; LOS and
immortal allocations fence once). Rust readers (the interpreter, primitives and frontend) must load heap references
with Acquire: Rust has no consume ordering, so a relaxed load does not order the loads made through its result, and a
stale hole word used as a reference is a memory-safety bug even though the load itself is not undefined behaviour.
Only JIT code may rely on address dependencies, under a documented exception that forbids value speculation and
equality substitution of loaded references. Acquire costs −1% to +8% on a chase [P]. (Corrected after review; see
`ANSWER.md`, rule 4.)
*Risk* high: correctness bugs here appear only on weak-memory hardware, and CI's Linux lane is x86-64.

**R14. TLABs, block pool, LOS and immortal allocation, pacing.** *Today:* allocation state on `Heap`
(`allocs_since_gc` :379, `gc_defer_depth` :417), global free lists. *(a)* 2–3. *(b)* 0.5–1: per-mutator `ap`/`limit`,
a block pool behind an uncontended mutex, per-mutator accounting. Left: measure pool contention (add per-carrier
block caches, Go's `mcache`/`mcentral` split, if the mutex shows), locks on LOS, descriptor and immortal appends, the
emergency reserve and `bytes_since_gc` summed across carriers (per-mutator counters folded at refill), TLAB waste of
up to one 32 KiB block per carrier.

**R15. Safepoint handshake, safe regions, blocking I/O.** Without a deactivate protocol one thread in `read(2)`
stalls every collection (Chez `Sdeactivate_thread`, OCaml blocking sections, CPython DETACHED). *Today:*
`gc_pending: Rc<Cell<bool>>` polled per instruction, `GcDeferGuard` deferral, no remote posting, no safe regions,
about 35 blocking primitives, and `read` lexes while the parser holds a half-built datum. *(a)* 4–7. *(b)* 2–3: §9's
poll word with remote posts and the `set_limit` Dekker fence, `safepoint_state`, the reserved `HANDSHAKE` bit,
`MutatorSet` lent to `collect`, and stage 9's safe-region API with its entry rule and restructured blocking
primitives. Left: the rendezvous itself (a leader, counted acknowledgements, safe-region carriers counted as stopped,
`leave_safe_region` waiting while a collection runs), Chez's trylock-then-deactivate for SRFI 18 mutexes,
per-carrier handshakes (JEP 312) for debugger stops, port-lock revocation and JIT invalidation. *Risk* high:
deadlocks between the GC, mutexes and I/O locks.

**R16. Precise rooting, the remaining `NoGcScope` windows, carrier pinning.** A carrier holding raw values in Rust
frames cannot park, so every other carrier waits for it; if it waits for a lock a parked carrier holds, the system
deadlocks. *Today:* deferral is the rooting mechanism (21 `GcDeferGuard` mentions, 20 Rust callback sites).
*(a)* 4–8 (ST): it is decisive. *(b)* 1.5–4: stages 2, 3 and 4e give precise roots and `NoGcScope` by type. Left:
the windows that stay deferred by design (point C imports, residual `apply_proc` fallbacks, a paused debugger's
evaluation; nested tree-walker trampolines do not matter, R23) become stalls for all carriers, so point C's
`ExpansionContext` root provider (decision 17, if not yet done) and resumable versions of the fallbacks move from
"if K16 fires" to required; a green thread inside a Rust re-entry pins its carrier (as Loom's native frames do), so
blocking there blocks the OS thread inside a safe region and the scheduler adds a compensating carrier.

**R17. Time-to-safepoint in long Rust primitives.** Allocation never collects, so a `Leaf` primitive that runs for
milliseconds (`make-vector` of 10⁸, `list->vector` or `equal?` on huge data, `string-append`, `write` formatting a
large datum, `read` of a large datum) delays every carrier's collection. Under M:1 K16 counts bytes in such windows;
under N they are stall time times N. *(a)* 1–3. *(b)* 1–2: `Step::Collect`/`CollectAndRetry` and resumable primitives
exist. Left: a time-to-safepoint metric (C12) and chunking of the worst offenders as resumable primitives.

**R18. Parallel GC with N carriers.** Collection is stop-the-world, so serial GC caps speed-up: with today's pause
share of 2.4–21.7% (DIGEST), 8 carriers reach at most 3.2–6.9× and 4 carriers 2.4–3.7× [Amdahl]. *(a)* 3–6: atomic
marking, replacing the `FxHashSet` dedup over `Rc` graphs, per-carrier root scans. *(b)* 1.5–3: stage P's CAS marking
on the metadata byte, work stealing and sharded ephemeron fixpoint. Left: let parked carriers be the workers
(OCaml's ParMinor, Gambit SMP, Chez sweepers) instead of idling beside separate worker threads; use the parallel
marker for sticky minors, whose survivor and logged-granule terms grow with the number of allocating carriers; scan
thread stacks as work packets (the design keeps roots on the collecting thread, and the frames term sums over every
rooted stack); claim post-pause slices (catch-up sweep, block classification, decommit) with an atomic cursor,
since every carrier's poll slow path may run them.

**R19. Store buffers.** *(a)* 0 if parallelism lands on a non-generational collector. *(b)* 0.5–1: the buffers are
per mutator already. Left: `ldclrb` disarm under `threaded` (duplicate entries from two carriers are harmless), soft
limits divided among carriers so the minor's d·(logged granules) term keeps its budget, and draining in parallel.

**R20. Ephemerons, weak tables, finalization.** *Today:* ephemerons are `RefCell<Option<(TV, TV)>>`; finalization is
`Drop` at sweep and a `thread_local!` exit flush. *(a)* 2–3. *(b)* 0.5–1: the fixpoint runs stop-the-world
(sharded under P); finalizers run no Scheme and allocate nothing (R4 of §6.7). Left: which carrier drains the queue
(the leader, before releasing the world), locks on `PortTable` during finalization, registration from any carrier
(R9).

**R21. Continuations and `dynamic-wind` across carriers.** SRFI 18 makes invoking another thread's continuation
well defined. *Today:* continuations live in per-`VmState` weak tables of `Rc<VmContinuation>` (`continuation_store`
26 mentions), capture clones five `Vec`s and `Rc<CodeObject>`s, `EMPTY_HANDLERS`/`EMPTY_REENTRY` are
`thread_local!` `Rc`s. *(a)* 3–5. *(b)* 1–2: 4e makes a continuation an immutable, value-only heap object that refers
to no carrier state; stage 9 makes the dynamic state per-thread heap data. Left: publication of the captured object
(R13), scheduler deliveries through `return_into` under the target thread's lock, `thread-terminate!` of a thread
running on another carrier (post `TERMINATE`; it acts at its next poll, without running winds, as SRFI 18 says),
and matrix rows for cross-carrier invocation, abort across a switch and terminate inside a wind, scored against
Gambit. *Risk* medium-high: this is where the codebase's defects have arrived (#157–#163).

**R22. Inline caches and per-site deoptimization.** *Today:* 12-byte global caches, `registry_index: Cell`,
`shadowed_primitives: Vec<u64>` per `VmState`. *(a)* 1–2. *(b)* 0.5–1: 4b deletes the caches; the shadow bitsets
latch monotonically, so they become `fetch_or` plus relaxed loads; a racing site may take the fast path once more
before seeing a rebinding, which is a race the program wrote.

**R23. The tree-walker stays M:1.** Its machine state is `Rc` all the way down: `StepResult` carries `Rc<CpsExpr>`
and `Rc<Environment>` in every variant (`cps_eval/types.rs:181-212`), an environment is created per call and per
`let` (`application.rs:79`, `step.rs:154`), and four `thread_local!`s describe the running trampoline.
Parallelizing it is 8–14 weeks plus the measured `Arc` costs (§3); ST estimated a 15–40% single-thread tax. Both (a)
and (b): keep it `!Send`, refuse a second carrier on a tree-walker heap, give its programs SRFI 18 as green threads
(stage 9 does), and offer isolates for parallelism. *(a)* 0.5–1, *(b)* 0.5 for the enforcement and its tests.

**R24. Global state in primitives and runtime.** *Today:* 19 `thread_local!` statics (§2), process-wide
`set_current_dir` behind `change-directory`, `exit`/`emergency-exit` through `process::exit` after flushing only this
thread's `OUTPUT_FILES`. The 16 process-global statics are already `Sync`. *(a)* 1–2. *(b)* 0.5–1: "no runtime state
in `thread_local!`" is a stage-2/9 rule and 4a's registry is process-wide. Left: `exit` from a non-main carrier (flush
every carrier's ports, then end the process), documenting the current directory as process-global, the debug-only
thread-locals (`TRACER`, `PHASE`, `SCOPE_ORIGINS`, diagnostics `OUTPUT`) made per carrier.

**R25. Bundled Scheme libraries.** Sixteen top-level variables mutated with `set!` in six bundled library files
(`srfi/115` keeps its match state in four globals, `srfi/158` keeps generator `return`/`resume` in globals). They are
wrong under preemptive green threads already, M:1 included. *(a)* 1–2 (with shallow `parameterize`). *(b)* 0.5–1:
stage 9 fixes `parameterize`; the six files remain.

**R26. The M:N scheduler and SRFI 18 across carriers.** *Today:* no SRFI 18. *(a)* 4–7 for the parallel delta on top
of row S. *(b)* 3–5: stage 9's scheduler interface, thread objects and root rule exist. Left: per-carrier run queues
with work stealing, carrier park and unpark, a timer queue shared by carriers, mutex fast paths by CAS on the owner
slot (C5) with waiters parked on heap queues, condition variables with spurious wake-ups, abandoned mutexes on
termination, a carrier-count policy (default to the core count, extra carriers to compensate pinned ones). *Risk*
high: lost wake-ups and priority-free starvation are found only by schedule exploration (R30).

**R27. Debugger, tracer and hooks.** `StepTracer`, breakpoints, watchpoints and the hook system's all-stop
(`TREE_WALKER_HOOK_SYSTEM.md` §9.1) assume one machine. *(a)* 2–3. *(b)* 1–2: root providers and the tier-policy flag
exist. Left: all-stop as a handshake, per-thread hook state, a tracer per carrier.

**R28. Embedding API, host threads, signals.** A host thread calling `Interpreter::call` while carriers run must
become a carrier (JNI's `AttachCurrentThread`); `SIGINT` lands on an arbitrary OS thread. *(a)* 2–3. *(b)* 1–1.5:
`Owned` handles, branded `with`, `call` as a driver entry, `InterruptHandle` (`Send + Sync`) exist. Left: attach and
detach, `Owned: Send` (C13), the handle posting to every carrier or to a designated one.

**R29. Memory model for racy programs.** SRFI 18 does not say what an unsynchronized race observes. The statement to
make: word-sized slots never tear, sub-word string and bytevector elements never tear, a racing reader sees some
previously stored value, never an uninitialized object (R13), and no Scheme operation is atomic beyond one slot;
with rows scored against Gambit, Chez and Gauche. 0.5–1 in both.

**R30. Test infrastructure.** The existing lanes are deterministic and byte-identical, and that cannot hold with
N real carriers. *Today:* no Miri, loom, TSan or arm64 Linux lane; stable toolchain pinned. *(a)* 6–10. *(b)* 4–7:
the two-mutator lane, Miri on `patina-gc` and a core subset, the verifier and zeal modes exist. Left: every existing
lane on the `threaded` build with one carrier (must stay byte-identical); an N-carrier deterministic simulation lane
with seeded switch points at polls, safe-region entry and lock acquisition (C8); loom models of the poll word,
handshake, block pool, `InterruptHandle` and port lock; ThreadSanitizer, which needs nightly and `-Zbuild-std` and
does not model `std::sync::atomic::fence` (Rust unstable book), so threaded code should synchronize with
release/acquire operations it can see (C9); Miri with many seeds for weak-memory effects (Miri's README: emulation
is incomplete, and mmap-style FFI is unsupported, which the design's `VirtualMemory` trait already works around); an
arm64 Linux runner, because x86-64 is TSO and hides publication bugs; stress lanes over SRFI 18 programs with oracle
results from Gambit.

**R31. Performance recovery and scaling.** Single-thread tax: relaxed scalar accesses are free; bulk operations,
`MetaByte` RMWs, port and table locks, and fences are not (§3). Multi-thread scaling: shared counters, global-cell
cache-line ping-pong, the block-pool mutex, false sharing of `Mutator` blocks (pad to 128 B), GC parallel efficiency
(R18). *(a)* 6–12. *(b)* 3–6. A Chez-style split (a non-threaded build that compiles every `threaded` accessor to
plain code) removes the single-thread tax at the cost of a second CI build matrix; Racket and CPython chose one
build and pay 3–10%.

**S. SRFI 18 on M:1 (prerequisite).** *(a)* 12–19: VM 8–12, tree-walker 2–3, non-blocking I/O 2–4 (ST). *(b)* 6–11:
stage 9 delivered the `GreenThread` split, objects, root rule, lifetime, minimal scheduler, deep-bound
`parameterize` and current ports; left are the SRFI 18 library proper (exceptions, timeouts, `thread-join!` results),
preemption ticks, its matrix rows scored against Gambit, and a helper pool for blocking I/O.

**J. A multi-carrier JIT.** Only if a JIT exists. *(b)* 2–4 under §11's contract: the `Mutator` stays in `x21`/`r15`
per carrier through the entry trampoline; macOS `MAP_JIT` toggling is per thread (Apple's
`pthread_jit_write_protect_np(3)`), so an installing carrier never blocks the others; Linux needs the dual mapping
the design lists as an obligation, because `mprotect` is process-wide; reuse of freed code memory needs every carrier
past a quiescent point that executes a context-synchronizing `isb` (C10), since arm64 cores may hold stale
prefetched instructions; `WATCHED` invalidation deoptimizes fragments on other carriers through a per-carrier
handshake; allocation groups fence under C1's rule; inline-cache words are single traced words updated with relaxed
stores. *(a)* no JIT exists; one built ad hoc for today's VM (raw `Rc<CodeObject>`s, patched caches, a single
context) would need 6–10 weeks of retrofit [I].

---

## 5. Single-thread tax, expected [I]

| Source | Under `threaded`, N = 1 | Basis |
|---|---|---|
| Relaxed scalar slot loads and stores | ≈ 0 | [P] chase and fill |
| Bulk operations (copy, fill, compare, `equal?` on vectors and strings) | 0 with a word-atomic copy routine (C4); up to 3.4–3.8× on those primitives otherwise | [P] scan |
| Publication fences | 0.1–1% with C1's filter if fresh-value publications are rare (to be counted); 10%+ on arm64 allocation-heavy code if every pointer store fences | [P] 12–57 ns per fence, census 4.7 pointer stores per 1,000 instructions |
| `MetaByte` `fetch_or` | ≈ 0 (barrier slow paths and hashing only) | [P] +0.35 ns |
| Port and table locks | 1–3% on I/O-bound loops | [P] +2.8 ns per uncontended lock; Racket's 6–8% |
| Interner, namespace and loader locks | ≈ 0 on run time; small on load time | rare operations |
| Poll and handshake | ≈ 0: the poll is the stack-limit check, remote posts are atomics already | DESIGN §9; JEP 312 (<1%) |
| **Total, VM** | **≈ 2–6%** with C1 and C4; more without | compare OCaml 5's 3.5%, Racket's ≤ 6–8%, CPython's 3–10% |

With a separate non-threaded build the tax is 0 and the cost moves to CI (a second build of every lane that matters).

---

## 6. Which redesign choices reduce (b) the most

Ordered by the weeks they remove from (a), from the row deltas of §4.1; rank 3 sits above its weeks because it
removes a deadlock class, not just effort.

| Rank | Redesign choice | Rows | Weeks removed | Why it matters for N carriers |
|---|---|---|---|---|
| 1 | **Stage 5:** block heap with stable addresses, no `RefCell`, no `Drop`/`Rc` payloads, side metadata so headers are immutable, per-mutator bump buffers and a block pool | R1, R2, R14 | 10.5–17 | no relocation, no refcounts, no mutator writes to words the collector writes; TLABs exist |
| 2 | **Stage 3:** `Mutator` ABI block, the collect capability, the `Cx` codemod, one store funnel through `HeapSlot`/`MetaByte`, the `threaded` feature with accessor bodies | R10, R11, R12, R13 | 8–12 | every heap access goes through one accessor that can turn atomic; one place for the fence |
| 3 | **Stages 2, 3, 4e:** precise roots, open root providers, `Owned` handles, `NoGcScope` by type, collectable nested loops | R16 | 2.5–4 | removes the deadlock class "a carrier cannot park because Rust holds raw values" |
| 4 | **Stage 4e:** immutable value-only continuations, traced code liveness, no weak side tables | R4, R21 | 3.5–6 | cross-carrier `k` is "copy frames into my stack"; no shared mutable continuation store |
| 5 | **Stage 9:** `GreenThread`/`Mutator` split, deep-bound `parameterize`, current ports in the dynamic environment, safe regions with an entry rule, blocking primitives as `Transfer` | R15, R26, S | 3–6, plus 6–8 of S | the scheduler and the dynamic state are already per thread; blocking already leaves the mutator |
| 6 | **Stage 4b:** global cells and binding records | R3, R22 | 2–3 | no `Rc<Environment>` or two-word caches on the hot path |
| 7 | **Stage P:** parallel stop-the-world marking | R18 | 1.5–3 | CAS marking and work stealing exist; carriers can join |
| 8 | **Stages 1, 4a, 4c:** byte pacing, `PortTable`, identifiers as ids | R6, R7, R17 | 2–4 | |
| — | **§9's poll protocol** (remote posts, `set_limit` fence, `InterruptHandle`) | R15, R28 | inside R15 | a handshake request is a remote post |
| — | **§11's JIT contract** (`Mutator` in a pinned register, never embedded; no code patching; one `install_code`; epoch freeing) | J | 4–6 of a retrofit | a JIT built to it needs only the J delta |

The redesign also adds work under N (R9's Rust tables, R19's store buffers, the per-thread stack reservations of
§9, the per-heap stage-P workers that idle beside parked carriers); the additions below target exactly those.

---

## 7. Cheap additions to the redesign that reduce (b) further

| # | Addition | Stage | Cost now | Saves in (b) |
|---|---|---|---|---|
| C1 | **Publication watermark.** Reserve `fenced_ap` in the `Mutator` ABI block; specify the `threaded` fence rule as "fence only when the stored value lies in `[fenced_ap, ap)`, then set `fenced_ap = ap`; fence once when retiring a TLAB and after LOS or immortal allocation"; add a census counter for fresh-value stores into non-fresh holders | 0 (census), 3 (field), 6 (freeze) | 1–2 days | an ABI break after the freeze, and most of a 12–57 ns-per-store tax [P]; 0.5–1 wk of R13 |
| C2 | **Per-carrier collect capability.** Make the capability a per-carrier driver token over `HeapShared` + `Mutator` (the ABI already names `HeapShared`), with `safepoint()` collecting when N = 1 | 3 | 2–3 days | 1–2 wk of R11, and keeps "no `Cx` across a collection" a type fact per carrier |
| C3 | **One wrapper for per-heap Rust tables** and per-mutator young registration lists (`FinalRegistry`, `HostPayloadTable`, handles, `RootSet`), plain under M:1 | 2, 4a, 4e, 5a | 1–2 days | 1–2 wk of R9 |
| C4 | **Bulk-range accessors** (`copy_range`, `read_range`, `compare_range` beside `store_range`/`fill_range`) for vectors, strings and bytevectors, so one routine becomes word-atomic | 3, 5d | 2–3 days | 0.5–1 wk of R12, and a 3.4–3.8× loss on bulk primitives [P] |
| C5 | **`HeapSlot::compare_exchange`** in the accessor set, used by stage 9's mutex objects (plain under M:1) | 3, 9 | 1 day | 0.5–1 wk of R26 |
| C6 | **Namespace access through one read-mostly API** (lookup, insert, re-point) | 4b | 1–2 days | 1–1.5 wk of R3 |
| C7 | **Stage P's marker as work packets any thread can run**, usable for minors, so parked carriers join as workers | P, 7 | about 1 wk inside P | 1–2 wk of R18, and the Amdahl ceiling |
| C8 | **N-carrier deterministic simulation lane** (generalize the two-mutator lane: seeded switch points at polls, safe-region entry and lock acquisition) and **loom models** of the poll word, `set_limit`, `InterruptHandle` and block pool from the stage that creates each | 3, 5a, 9 | 1–2 wk | 2–3 wk of R30; catches protocol bugs while they are cheap |
| C9 | **TSan-visible synchronization:** in `threaded` code prefer release stores and acquire loads (or `SeqCst` RMWs) to standalone `fence`, since TSan does not model `fence` | 3 | 0 | false positives and blind spots later |
| C10 | **Reserve a quiescence epoch and a per-carrier handshake word** in the `Mutator` ABI block (code-memory reuse, `WATCHED` deopt on other carriers, port-lock revocation) | 3, 6 | 0 | an ABI break; 0.5–1 wk of J |
| C11 | **Compile-time `Send`/`Sync` assertions** under `threaded` (`HeapShared: Sync`, `CodeBody: Send + Sync`, `GreenThread: Send`, VM-heap `HostPayload`/`RootProvider: Send + Sync`) and a CI check against new `thread_local!`, `static mut`, and `Rc`/`RefCell`/`Cell` in heap-side tables | 3, 5e | 1 day | 0.5–1 wk of R10; turns "single-mutator shortcut creep", the design's top threading risk, into compile errors |
| C12 | **Time-to-safepoint metric** (max time from a post to its servicing poll) beside K16's byte counters | 0 or 1 | 1 day | makes R17 measurable; 0.5 wk |
| C13 | **`Arc`-weak handle tables** so `Owned`, `RootToken` and `PinToken` are `Send` | 2 | 0 | 0.5 wk of R28 |
| C14 | **Green-thread register stacks carved from one per-heap reservation**, with the frame-entry check as the only guard (no per-thread guard pages) | 4d, 9 | 2–3 days | avoids Linux's `vm.max_map_count` (about 65 K by default) at about 65 K started threads; the design's own `blocked-threads` probe starts 100 K; useful under M:1 too |
| C15 | **The registry's `Loading` entry records the loading thread** | 2 | 0 | 0.5 wk of R5 |
| C16 | **Issue for the six bundled libraries with top-level mutable state** | 0 | 2–3 days to fix | 0.5–1 wk of R25; needed for M:1 preemption anyway |
| C17 | **An arm64 Linux CI runner** for the threaded lanes once they exist | when the threaded lane starts | 0 now | weak-memory bugs that x86-64 hides |

Adopting all of them costs about 5–7 engineer-weeks spread over stages 0–9, of which C8, C14 and C16 pay for
themselves under M:1, and removes about 10–17 weeks from (b): **36–69 → 26–52 weeks**.

---

## 8. A staged path after stage 9

Each step ships with value and leaves every scoreboard green.

1. **`threaded` build, one carrier** (R10–R14, R24, R29, C11): every existing lane, byte-identical, on the threaded
   build; the single-thread tax measured with confidence intervals. Decides "one build or two" (R31).
2. **N simulated carriers on one OS thread** (C8): the handshake, safe regions, scheduler and finalization
   protocols under seeded interleavings; loom on their kernels.
3. **N carriers with one mutator token** (R15, R26 without parallel mutation): carriers in safe regions run blocking
   I/O, timers and FFI while one runs Scheme; Python's and Ruby's GIL shape, used here as a stage. Gives SRFI 18
   programs I/O concurrency with no data race possible.
4. **Token released for VM heaps** (R3–R9, R12, R13, R16, R17, R21, R22): real parallelism; TSan and arm64 lanes;
   the memory-model rows (R29).
5. **Scaling** (R18, R31): carriers as GC workers, parallel minors, contention fixes.

Isolates remain the alternative: one interpreter per OS thread, sharing nothing, 3–5 weeks after stage 5e (DESIGN
§12), parallel for share-nothing work only, not for SRFI 18 threads.

---

## 9. Steady state and limits under N carriers

The owner asked that limits be organized around a steady state. With N carriers and M green threads, the
steady-state footprint is the design's heap term plus per-carrier and per-thread terms:

| Term | Per | Size | Note |
|---|---|---|---|
| Heap | heap | about 3× live at the whole-heap interval `max(8 MiB, 2·L)` | unchanged by N; pacing sums allocation over carriers |
| Allocation buffer | carrier | up to one 32 KiB block of waste | |
| Store buffer | carrier | 256 MiB VA, committed up to the soft limit (about 1 MiB) | the soft limit is divided by N to keep the minor budget (R19) |
| Register stack | green thread | 256 MiB VA (8 GiB main), committed to its depth, decommitted with hysteresis | per-thread reservations meet `vm.max_map_count` near 65 K threads on Linux unless carved from one reservation (C14) |
| Mark-stack segments | GC worker or carrier | 4 KiB segments | |

Pause budgets keep their form: the frames term sums over every rooted stack whatever N is; the survivor and
logged-granule terms grow with the number of carriers allocating between minors, which is why the soft limit and the
nursery budget are per heap, not per carrier, and why minors must be parallel (R18).

---

## 10. Candidate issues observed while itemizing

- **Per-thread stack reservations versus `vm.max_map_count`.** Each started green thread reserves its own register
  stack (DESIGN §4, §8.1); the `blocked-threads` probe starts 100 K threads; Linux's default map count is about 65 K
  (kernel `Documentation/sysctl/vm.txt`), so the probe would fail on Linux CI under M:1 already. Fix: C14.
- **Publication-fence cost on arm64.** 12–57 ns per fence with allocations in flight [P]; the `threaded`
  obligations in DESIGN §12 place a fence in the funnel and in JIT allocation groups without a filter. Fix: C1,
  before the stage-6 ABI freeze.
- **TSan cannot check fence-based synchronization.** `set_limit` (§9) and the funnel fence use `fence`; TSan does not
  model it. Fix: C9.
- **Bundled libraries with top-level mutable state** (16 variables, 6 files) break under preemptive green threads,
  M:1 included. Fix: C16.
- **CI has no weak-memory hardware lane.** Fix: C17, when a threaded lane exists.
- **Stage P's per-heap workers idle beside parked carriers** under N; serial GC at today's 2.4–21.7% pause share caps
  8 carriers at 3.2–6.9×. Fix: C7.

---

## 11. Open questions for the owner

1. One build or two? A non-threaded build keeps today's speed exactly and doubles part of CI; one threaded build
   costs an estimated 2–6% (§5).
2. Is cross-carrier `thread-terminate!` of a thread blocked in a system call allowed to wait until the call returns
   (no signal-based interruption, unlike Gauche)?
3. Is the "one mutator token" stage (§8 step 3) worth shipping on its own, for SRFI 18 I/O concurrency?
4. Should the cheap additions C1, C2, C10 and C14, which touch the ABI or stage 3's types, be folded into the
   design now? They are the ones that are expensive to add after stage 6.

---

## Sources

Local: `design/DESIGN.md` (§4, §5, §7, §8.1–8.6, §9, §11, §12, §15, Appendix D, E.1, E.7, H.4);
`DIGEST.md` (store mix, pause share, process facts); `research/threads-patina-cost.md` (ST's rows and totals);
`research/threads-prior-art.md`; `research/threads-recommendation.md`; repository at `28a94f8` as cited.
Probes in `PRD/study/gc/probes/followup/`: `nonsend_types.py`, `sendcheck/` (auto-trait probe), `atomtax/`, `pubfence/`,
`contend/`, `scm_globals.py`.

Web:
- Racket, "Parallel Threads in Racket v9.0" (2025-11): https://blog.racket-lang.org/2025/11/parallel-threads.html
- PEP 779 (free-threaded CPython overheads): https://peps.python.org/pep-0779/
- Rust unstable book, sanitizers (TSan needs nightly, `-Zbuild-std`, no `fence` support):
  https://doc.rust-lang.org/beta/unstable-book/compiler-flags/sanitizer.html
- Miri README (data races, incomplete weak-memory emulation, `-Zmiri-many-seeds`, no platform FFI):
  https://github.com/rust-lang/miri
- Apple `pthread_jit_write_protect_np(3)` (per-thread `MAP_JIT` write protection):
  https://opensource.apple.com/source/libpthread/libpthread-454.40.3/man/pthread_jit_write_protect_np.3.auto.html
- Linux `vm.max_map_count`: https://www.kernel.org/doc/Documentation/sysctl/vm.txt
- Sivaramakrishnan et al., "Retrofitting Parallelism onto OCaml", ICFP 2020: https://arxiv.org/abs/2004.11663
- JEP 312 (thread-local handshakes): https://openjdk.org/jeps/312; JEP 444 (virtual threads):
  https://openjdk.org/jeps/444
- SRFI 18: https://srfi.schemers.org/srfi-18/srfi-18.html
