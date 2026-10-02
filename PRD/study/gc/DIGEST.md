# Patina GC redesign: research digest for the architecture panel

Date 2026-10-01. Repo `main` at `28a94f8` (clean). Built from the 23 reports listed in Appendix B, plus the three
`research/threads-*.md` reports, plus my own source checks. Nothing in the repo was modified.

**Conventions.**
- **[V]** = verified in source, by measurement, or in a primary source (by the cited report, or re-checked by me
  where marked **[V*]**). **[I]** = inference or judgement. **[D]** = quoted from a repo doc, not re-run.
- Report citations: `[gc-impl §5]`. Keys: heap-repr, gc-impl, vm-runtime, offheap, tree-walker, prim-embed,
  jit-ready (understand/); chez, racket-larceny, ocaml-gambit, hotspot, immix-mmtk, js-engines, whippet, rust-gcs,
  cranelift-gc, threads-rec (research/); demographics, global-cells, cont-repr, barrier-res, libload, finalization
  (gaps/).
- Repo paths are crate-relative: `core:` = `crates/patina-core/src/`, `vm:` = `crates/patina-vm/src/`,
  `prim:` = `crates/patina-primitives/src/`, `tw:` = `crates/patina-tree-walker/src/`, `fe:` =
  `crates/patina-frontend/src/`, `rt:` = `crates/patina-runtime/src/`.
- "Hypothetical layout" (demographics §1.1): pair 16 B, vector 8+8n, string 8+round8(4n), flonum 16, closure
  16+8·fv, record 8+8·fields, cell 16. Used for byte estimates of a headered design.

**Ten facts that drive everything** (details below):
1. Allocation never collects; collection happens only at the outermost driver-loop safe point. ~230–890 Rust
   functions hold unrooted `TaggedValue`s across allocation and are correct only because of this
   (`core:heap/mod.rs:581-584`, verified [V*]). Chez runs a moving generational GC under the same contract.
2. The representation, not the algorithm, is the bottleneck: 72 B enum slots, `Rc` payloads, typed `Vec` arenas
   whose bases move, `Rc<RefCell<Heap>>` on every access, index-based identity. Same allocations cost ~2.05× the
   bytes of a headered layout (demographics §4.4).
3. 19 of 28 object variants own Rust `Drop` payloads; 47% of allocations in a mixed workload carry `Drop`.
   Sweep-time tombstoning is load-bearing (cycle breaking, port flush, code release, provenance pruning).
4. Off-heap `Rc` graphs hold `TaggedValue`s that the visitor sees by value (`GcVisitor::visit(tv)`,
   `core:heap/gc.rs:485`): environments, code constants, macro literals, library exports, continuation side
   tables, all tree-walker state. None is updatable today.
5. The VM keeps all Scheme state in VM-managed memory (register file + `CallFrame` vector), already with per-pc
   liveness maps. A baseline JIT that keeps that layout needs no Cranelift stack maps.
6. Cranelift: every non-tail call is a safepoint; no exemption for calls known not to GC; one pinned register is
   available (x21/r15). Multi-shot `call/cc` across native Cranelift frames has no support.
7. Measured survival is bimodal: 10/20 GC workloads ≤1.5% at a 64 K-object nursery, others 13–100%; closures and
   cells (40% of allocations) almost never survive. Generational is a likely but not universal win.
8. Measured store mix: immediate-value filter removes 64% of heap stores; median workload sends 99.4% of stores
   into young holders; old→young edges hit very few targets. A field-logging barrier fits; a Chez SSB does not.
9. Library loading is the memory pathology (627 MiB RSS for 26 libraries, 765 MiB malloc peak), caused by deferral
   (no safe points during loads), syntax provenance side tables (316 MiB) and eager scope copying, not by the
   collector algorithm.
10. Continuation capture copies the whole stack into byte-blind weak side tables: 24 µs and 171 KB per capture at
    depth 1000; 5.7 GB RSS in 80 k captures.

---

## 1. Patina today

### 1.1 Value encoding [V]

| Fact | Evidence |
|---|---|
| `TaggedValue(u64)`, `#[repr(transparent)]`, `Copy, Eq, Hash` on raw bits. Low 3-bit tag: `000` fixnum (61-bit), `001` special, `010` char, `011` pair, `100` vector, `101` string, `110` closure, `111` object | `core:tagged_value.rs:55-57,76-84` [V*] |
| Heap refs = `u32` arena index `<< 3 \| tag`, **not addresses**; bits 35–63 of a heap ref are always zero | `tagged_value.rs:28,376-413`; heap-repr §1 |
| `TAG_CLOSURE` (110) is never minted in production; VM closures and CPS lambdas are tag-111 objects; `is_closure()` checks are dead branches | heap-repr §1, jit-ready §2.7 |
| 25 of 32 special codes free; `FORWARDED=0xF1` (env import marker, not GC forwarding), `GC_POISON=0xF9` (debug pair tombstone) | `tagged_value.rs:91-110` |
| Flonums always boxed (`HeapObjectData::Real`, 72 B slot); complex = 3 objects; bignum/rational via `num-bigint`, cloned on every arithmetic dispatch | heap-repr §4; `core:numeric.rs:418-435` |
| AGENTS.md calls the encoding "NaN-boxed"; it is not | heap-repr §1 |

### 1.2 Heap layout and allocation [V]

- `SharedHeap = Rc<RefCell<Heap>>` (`core:heap/mod.rs:51`); `Heap` 528 B, also carries per-instance metadata
  (features, command line, library availability, `:334-364`).
- Four typed arenas (`:304-316`): `pairs: Vec<(TV,TV)>` 16 B/slot; `vectors: Vec<Vec<TV>>` and
  `strings: Vec<Vec<char>>` 24 B slot + separate malloc (UTF-32, 4 B/char); `objects: Vec<HeapObjectData>` 72 B/slot.
- Allocation: `note_alloc()` then pop a LIFO `Vec<HeapIndex>` free list or `Vec::push` (`alloc_pair :703-714`).
  **Arena growth relocates the base**; arenas never shrink (no truncate anywhere in `heap/`).
- Interning: `symbol_table: HashMap<String, HeapIndex>` and `core_syntax_table` are immortal, re-marked on every
  GC (`gc.rs:449-465`). Parsers do not intern written identifiers: each occurrence is an `Identifier` object.
- `syntax_sources: HashMap<u64 raw bits, Rc<SyntaxSource>>` (`heap/mod.rs:305`), pruned at sweep.

### 1.3 Object model [V]

`HeapObjectData` (`core:heap/mod.rs:143-229`) is a 72 B `#[derive(Clone)]` enum of 28 variants. Size driven by
`Exception` (72), `Rational` (64), `Identifier` (64: `Rc<str>` + 40 B `SmallVec` scope set + bool); boxing those
three gives 40 B (heap-repr §3).

- **Drop-carrying variants: 19** [V*, enumerated]: BigInt, Rational, Symbol, Bytevector, Exception, Procedure,
  Port, Macro, RecordType, Record, Identifier, Continuation, Parameter, Promise, Library, Values,
  EnvironmentSpecifier, PromptTag, VmClosure. Of these, 14 hold an `Rc`. No-Drop: Real, Complex,
  LabelPlaceholder, MutableCell, Ephemeron, VmContinuationRef, VmDelimitedContinuationRef, CoreSyntax, Free.
- Per-object extra allocations: records `Rc<RTD>` + `Rc<RefCell<Vec>>` (3 allocations, plus a field vector built
  first by `%make-record`, `prim:records.rs:138-182`); `VmClosure{code_id, free_vars: Vec, globals: Rc<Environment>}`
  (slot + malloc + `Rc` clone); `MutableCell(RefCell<TV>)` is 72 B for 8 B of data.
- Type sizes: `Environment` 224, `CpsContinuation` 208, `Procedure` 120, `CallFrame` 40, `VmContinuation` 152,
  `CodeObject` 160, `CompiledMacro` 224, `Library` 152, `Instruction` 48, `VmState` 832, `ContValue` 64,
  `CpsExpr` 176 (gc-impl §9, vm-runtime §9, tree-walker §2.1).
- Tracing dispatches on the enum (`gc.rs:695-794`); the enum doc warns that misfiling a value-bearing variant as a
  leaf is a use-after-free, not a compile error (`heap/mod.rs:137-141`).
- `alloc_library` has no non-test caller; `HeapObjectData::Library` looks dead (offheap §3.2).

### 1.4 The collector [V]

- `core:heap/gc.rs` (1,812 lines): one non-moving, STW, non-incremental mark-sweep for both backends.
  `Collector` trait documents non-moving as a contract (`:200-213`); `GcRoots` (`:177-197`) with
  `trace_roots(&self, &mut GcVisitor)`, `trace_weak_ids`, `sweep_weak`.
- Marking: four fresh `BitSet`s per collection (`:85-136`); worklist of values; car-then-cdr push makes the mark
  stack O(list length) for heap-car lists (+55 MB on a 2 M-element list of 2-vectors). `Rc` graphs deduplicated
  by four `FxHashSet<usize>` keyed by address (`:422-446`, `visit_once :580`); without dedup the trace was
  exponential (6.8 s at depth 26, GC_DESIGN §9.4).
- Weak processing: one fixpoint for VM continuation ids and SRFI 124 ephemerons (`:1022-1101`). Round-based: rescans
  every pending ephemeron per round. A 16 k ephemeron chain costs **289 ms** in bad order vs 0.24 ms in good order
  (finalization §0.7). Sequencing the two kinds separately was a use-after-free (commit `1d18c49`).
- Sweep (`:891-961`): pre-marks every free-list index, walks **every slot of every arena**, tombstones dead slots
  (`HeapObjectData::Free`, `Vec::new()`; pairs poisoned only in debug), records freed raw bits (cap 65,536) and
  dead closures' code ids.
- Trigger: `note_alloc` counts **objects**; threshold `max(65,536, 2 × live slots)` (`gc.rs:996-1003` [V*]). Blind
  to vector/string/bignum buffers, continuation payloads, ports, `Rc` memory.
- Safe points: `GcController::safe_point` (`gc.rs:381-409`), fast path one `Rc<Cell<bool>>` load + hoisted
  `is_outermost`. VM: top of every dispatched instruction (`vm:runtime/vm_state.rs:1204-1209`), collect only in
  the outermost loop; `retire_registers` first. TW: top of every outermost trampoline step
  (`tw:eval/cps_eval/mod.rs:213-230`).
- `GcDeferGuard` sites [V*]: every VM dispatch loop (`vm_state.rs:1151`), `with_globals` (`:343`),
  `desugar_with_imports` (`fe:desugarer/mod.rs:1841`), `ParsedLibrary` for its lifetime
  (`rt:library_loader.rs:208`), every TW trampoline (`cps_eval/mod.rs:219`). **Library bodies, expansion, and
  nested runs never collect.**
- Infrastructure worth keeping (gc-impl §11): `GcRoots` inventory, the combined weak fixpoint, the pending-flag
  trigger, per-pc `register_roots`, debug poison, `scripts/run_gc_differential.sh` (off/default/stress × release/
  debug-poison × both backends, `EXPECTED_TOTAL=1226`), ~90 GC tests (gc.rs 22, weak_continuation 7, gc_vm 9,
  gc_tree_walker 4, gc_shared_tests 13×2, ephemerons 15, finished_forms_release_code 9).

### 1.5 Roots and off-heap holders of `TaggedValue` [V]

| Holder | How reached today | Updatable by a mover? | Source |
|---|---|---|---|
| VM register file `Vec<TV>` (all frames) | slice scan after `retire_registers` | yes | `vm:runtime/vm_state/gc_roots.rs:47-75` |
| `CallFrame.closure: Option<HeapIndex>` | special `visit_object_index` path | no (bare index) | `vm:types/mod.rs:41-60` |
| `CallFrame.code: Rc<CodeObject>` | `Rc` count keeps unit loaded | n/a (refcount, 4 `Rc` ops per call/return) | vm-runtime §2.1 |
| `CodeObject.constants: Vec<TV>` in `Rc` | every loaded code object, every GC (was 57% of root tracing pre-#353) | no (immutable `Rc`) | `vm:types/code_object.rs:140` |
| Environments: `Bindings` `SmallVec<(Rc<str>,TV)>` in `RefCell`, scoped/alias tables, import `Owner` links | `visit_env` with per-env hash dedup (8.1 ns per live closure) | yes (`RefCell`) | `core:environment.rs:208-219,487-547` |
| `Library.exports: HashMap<String,TV>` | `LibraryRegistry: GcRoots` | no (pub field in `Rc`) | `core:library.rs:22-35` |
| `CompiledMacro` pattern/template literals | `Macro` trace arm | no; `foreign_expansions` envs not traced (latent gap) | `core:compiled_macro.rs:36,262,540` |
| VM continuation stores `RefCell<FxHashMap<u64, Rc<VmContinuation>>>` | weak id fixpoint | no (`Rc` payload) | `vm_state.rs:196-199` |
| `WindRecord.handlers: Rc<[ExceptionHandler]>` (fresh copy of whole handler stack per `dynamic-wind`) | `visit_winds_with` | no | `core:continuation.rs:173-198` |
| TW `StepResult` (moved by value through Rust calls; no persistent register struct) | `StepRoots` built per safe point | locals only | tree-walker §1.2 |
| TW `CpsContinuation`/`ContValue`/`ContEnv` `Rc` graphs; `CpsExpr` literal trees | iterative + dedup walk; ~140 ns per suspended frame | no (immutable `Rc`) | tree-walker §2.4 |
| `PENDING_ESCAPE` thread-local (TW) | `EscapeRoots` | yes | `tw:cps_eval/types.rs:20-30` |
| Values returned to embedders by `Interpreter::eval_*` | **nothing** | — | `crates/patina-interpreter/src/lib.rs:278` |

Mutation channels a barrier would need (offheap §4, barrier-res §2.1): `&mut Heap` setters (`set_car`/`set_cdr`,
`vector_set`, `vector_slice_mut`, `promise_update`, `set_vm_closure_free_var`); `&Heap`+inner `RefCell`
(`write_mutable_cell`, called by the VM under a shared borrow, `vm_state.rs:2407-2414`; `break_ephemeron`); **stores
through cloned `Rc` handles bypassing the heap entirely** (record fields `prim:records.rs:262`, parameter stacks
`prim:parameters.rs:168,267`, promise state `prim:lazy.rs:134`); off-heap environment stores (`define` ~72 call
sites, `set_slot_value`, VM `StoreGlobal`/`Define`, TW continuation-return `define`, `tw:continuation.rs:239`).
`vector_slice_mut` has exactly **one** non-test caller, the VM `VectorSet` arm (`vm_state.rs:2380`) [V*].

### 1.6 Identity and raw-index escapes [V]

- `eq?` (`values_eq`, `core:heap/mod.rs:2118-2146` [V*]) compares raw bits, then for two *different* slots wrapping
  the same `Rc` returns true for `Procedure`, `RecordType`, `Record`. `%record-type-of` allocates a fresh RTD wrapper
  per call; `(current-output-port)` allocates a fresh port wrapper per call (`prim:io/ports.rs:463`), so
  `(eq? (current-output-port) (current-output-port))` is `#f` (chibi/Gauche: `#t`) (finalization E4).
- `identity-hash` = `mix(heap_index)` or `Rc::as_ptr` for those three types (`heap/mod.rs:2538-2561` [V*]); doc says
  it relies on the collector not moving. Used by SRFI 69 `(make-hash-table eq?)` via `hash-by-identity`
  (`lib/srfi/69/srfi-69-impl.scm:118-120`), and the hash is **stored in Scheme bucket vectors**. SRFI 125/128
  `make-eq-comparator` hashes structurally via `equal-hash` and never calls `identity-hash`; `equal-hash` falls
  back to heap index for non-structural objects (`:2668-2670`) (demographics §7.1: 1.66 M identity-hash calls in
  `eqtable`, 0 index fallbacks anywhere).
- Persistent raw-bits keyed maps: `syntax_sources`, `SourceMap.locations` (pruned from the capped freed-bits feed;
  overflow clears the map). Transient raw-bits sets in writer, parser, desugarer `quoted`, `OpenNodes`,
  `map_syntax_identifiers_memo`, `PrimitiveCallMap.by_value` — safe only under "no GC inside the call".
- Index arithmetic is contained: `heap_index()` 60 uses, 1 outside core (`vm:runtime/control.rs:426`); `.raw()` 32
  (18 outside core); `raw_bits()` 32 (6 outside) (heap-repr §6).

### 1.7 VM runtime facts relevant to GC and JIT [V]

- `ExecutionState` = five `Vec`s: registers, frames, prompts, winds, handlers (`vm:runtime/execution_state.rs:17-23`).
  `push_frame` resizes the register `Vec` (may relocate on any call). 10 M-deep non-tail recursion works
  (0.50 s, 1.37 GB, ~137 B/frame); `VmError::StackOverflow` is never raised.
- Per-pc `register_roots: Option<Vec<Vec<u64>>>` may-root bitsets (#423, `code_object.rs:152-158` [V*]); locals stay
  conservative for the whole frame; stubs have `None` (all slots live). Costs ~40 B/PC = 65% of the instruction
  stream (libload §2.2). Dead slots overwritten with `UNSPECIFIED`, not skipped.
- Instructions never embed heap references; all heap literals go through `constants` (`pass5_codegen.rs:697-707`).
  Codegen emits forward jumps only: every loop is a call or tail call (jit-ready §2.2).
- "Work owed after a call is a stub frame" (`control.rs:91-104`): 9 runtime stubs (`wind_jump`, `invoke_step`,
  `value_wind`, `value_cwv`, `abort_handler`, `raise_step`, `force`, `resume_stub`×5). Prompts/handlers/winds
  address frames by depth integers, never pointers.
- Globals: `LoadGlobal` clones an `Rc<Environment>` out of the current closure on every read (`frame_globals`,
  `vm_state.rs:1357-1364`; 2.6% of samples), probes an `env_id` cache, borrows a `RefCell`, follows `FORWARDED`
  import links. Globals are 14–15% of dispatches in fib/tak.
- Opcode mix (jit-ready §2.3): calls 10–31% of dispatches; LoadClosure+ReadCell 27.7% of nboyer; allocation 2–10%;
  barrier-site stores ~1–2% (WriteCell 2.2% of nboyer). Every inline op takes a heap `RefCell` borrow.
- Nested Rust-driven loops remain (`across_reentry`, `run_apply_proc`, library loading, `Step::Eval`);
  `VmApplyContext` holds a raw `*mut VmState`. Live Rust→Scheme re-entries in primitives: `environment`,
  `null-environment`, `scheme-report-environment` (library loads) and `%parameterize-swap!` on parameter-like
  procedures; the other `apply_proc` callers are shadowed fallbacks (prim-embed §3.4).
- Code lifetime: `CodeObject.live_closures` incremented at `MakeClosure`, decremented only from sweep's dead-closure
  report (`vm_state.rs:542-575`) — requires sweep to visit the dead.

### 1.8 Tree-walker facts [V]

- Runtime state is almost entirely off-heap `Rc`/`Box`/`Vec`: `(fib 25)` = 3.16 M Rust allocations (479 MB), ~13
  per Scheme call, 1.09 M `Environment`s, against **43** GC-heap allocations; GC never runs. `fib 32` profile: 22%
  malloc/free, 6% drop glue, 34% name lookup. The GC trigger sees ~1 in 10⁴–10⁵ of TW allocation (tree-walker §2).
- Safe-for-space depends on `Rc::strong_count(&env) == 1` frame sharing (`tw:step.rs:151`; SRFI 45 test: 39 MB vs
  895 MB).
- Each backend constructs its own heap; no constructor shares one — a per-heap collector policy is safe today.
- Libload on the TW adds ~390 MiB of GC-coupled `Rc` garbage freed only by tombstoning (libload §0.3).
- Debugger/hook plans (`PRD/future/TREE_WALKER_HOOK_SYSTEM.md`): hook storage must be a GC root (H1), "no GC while
  paused" today, hooks pin the interpreter tier, "safepoint metadata is designed once".

### 1.9 Measured numbers (release, macOS 27.2 arm64, M4 Pro, 16 KiB pages; single-machine)

**Collector costs** (gc-impl §9, demographics §3):

| Quantity | Value |
|---|---|
| Sweep | ~2.4 ns per arena slot (live or dead); 1.1–3.9 slots swept per allocation across 20 workloads |
| Mark | 2.8–3.0 ns per live pair; 8.1 ns per live closure (env hash probe); TW ~140 ns per suspended frame |
| Allocation (no `RefCell`) | pair 1.5 ns; 4-vector 11.2 ns (malloc); flonum 8.2 ns |
| `(gc)` pause | 72–114 µs after bootstrap; 2.9–3.1 ms with 1 M live pairs (TW 4.2–4.6); 15.4 ms VM / 22 ms TW after loading ~25 R7RS-large libraries (99.7% of 4.62 M pair + 2.89 M object slots free) |
| Pauses on the 20-workload GC set | mean 0.15–20 ms; worst 41 ms (queue3), 178 ms (first collection after 25 library loads); pauses = 2.4–21.7% of run time |
| GC cost vs `PATINA_GC=0` | −35% (ctak, fibc: GC-off reaches 5 GB RSS) to +17.6% (deeprec); typically 2–8% |
| Safe point fast path | +1.1–1.4% vs no-safepoint control [D GC_DESIGN §6.1] |

**Trigger blindness** (gc-impl §4, finalization E2, cont-repr §2): 500× `(make-vector 100000)` → 0 collections,
414 MB; gcold reaches 628 MB RSS with ≤12 MB live; 100 k unclosed file opens fail at the 1,021st under
`ulimit -n 1024` (chibi and Gauche complete); 80 k captures at depth 1000 → 5.7 GB.

**Allocation demographics** (demographics §4–§5; 20 workloads from Larceny R7RS + GC suites + extras, 348 M objects):
- By count: closures 35.3%, pairs 27.9%, flonums 19.8%, vectors 8.8%, cells 5.3%, records 1.0%, identifiers 0.9%,
  strings ~0. By hypothetical bytes: closures 39.7%, vectors 36.3% (gcold), pairs 11.6%, flonums 8.3%.
- Sizes (hypothetical, gcold excluded): 56% exactly 16 B; 80% ≤32 B; 97% ≤64 B; 99.2% ≤128 B; mean 26.9 B; 148
  objects >8 KiB. Closures: 68% have 1–4 free variables.
- Survival at a 64 K-allocation interval: ≤1.5% for 10/20 workloads; hashtable0/earley/dynamic/eqtable/gcold
  13–25%; nboyer 43%; mperm 58%; queue3 and deeprec 100%. 64 K→1 M interval helps ≤1.5×. Closures, cells,
  flonums survive ≤0.7%; excluding closures+cells, data survival is 41–56% on the hash/parse workloads.
- Nursery trace work vs today's marking per allocation: ~20× less (gcbench, nucleic), 2–5× less (hash/parse), 3–50×
  less (gcold), equal (nboyer, mperm), 1.7–1.8× **more** (queue3, deeprec). "Dead-old" 65–100% (promoted data
  mostly dies before exit) except earley.
- Rate today: 1.5–25 M allocations/s, 29–544 hypothetical MB/s; a 5–10× faster JIT ⇒ ~0.3–4 GB/s [I].

**Store mix** (demographics §6, barrier-res §2): heap stores 0.1–70 per 1,000 instructions (median ~13; libload
249). Immediate filter removes 64% in aggregate (3–99% per workload). Median workload: 99.4% of stores hit young
holders. Old→young = 10.8% of heap stores at the 64 K view, concentrated: ≤36 distinct targets per interval on
average; nboyer's 888 K edges hit one cell. Sources: boxed `set!` variables (`MutableCell`) and hash-bucket
vectors. Parameter, promise, ephemeron-break and closure-free-variable channels had zero stores. TW sends 12.48 M
`Environment::define` through nboyer (VM: 10).

**Continuations** (cont-repr §2, demographics §7): capture 3–4 µs/17 KB at 100 frames, 24 µs/171 KB at 1000;
ping-pong coroutine switch 27 µs at depth 1000; `ctak` 0.06 s/98 MB alone vs 1.04 s/4.0 GB under 500 frames;
ctak copies 1.91 M × 211 registers ≈ 3.2 GB. A toy "traced snapshot object" (design A) cuts capture overhead
~5.8×; "freeze above watermark" (C′) makes capture ~57 ns independent of depth.

**Library loading** (libload §0–§2, VM, 26 libraries): RSS 627 MiB; malloc-live peak 765 MiB = 416 MiB arena
capacity + **316 MiB syntax provenance** (136 MiB hash table + 166 MiB `Rc<SyntaxSource>` + chains) + ~33 MiB
other. Survivors: 15.8 k of 4.62 M pairs, 5.1 k of 2.89 M objects, 30 of 2.19 M identifiers. 96.7% of identifiers
are copies made by eager scope edits; scope edits = 44% of load CPU, provenance 20%, the single post-load GC 13%
(~200 ms). After load, 464 of 501 MiB malloc-live is empty arena capacity and free lists. Between-form collection
on `(nieper rbtree)` cut the malloc peak 211→118 MiB. Two provenance stores (`SourceMap.locations`, child spans)
have **no production reader**.

**Process facts** (demographics §8): up to **318 live heaps** in one `cargo test` process (4,200 across 276
processes); 272/276 processes exit with every heap still live (the `Rc` cycle heap → `VmClosure.globals` →
`Environment.heap`; dropped interpreters leak with `strong_count` 36–37). Reserving 4,096 × 16 GiB `PROT_NONE`
regions succeeds (64 TiB); only 1/64 regions came back 4 GiB-aligned; `MADV_FREE`+`PROT_NONE` returns RSS at once.

**Benchmarks available** [V*]: `~/Project/r7rs-benchmarks` (cited by TRACK_P PRD and jit-ready §6) **does not
exist**. The repo suite (41 cases) barely exercises GC (longest 99 ms; only 4 cases collect, ≤4 times). Local
Larceny suites exist: `~/Project/reference/larceny/test/Benchmarking/R7RS` (74 programs; 58 run at count 1) and
`.../GC` (gcbench, gcold, perm, queue3, pueue4, nboyer, sboyer, earley, nucleic2, dynamic, twobit, graphs, lattice,
paraffins). demographics built a 20-workload GC set (0.9–3.8 s each); reproduction in
`PRD/study/gc/probes/workload-demographics/instrumented/REPRODUCE.sh`. No pause/MMU benchmark exists.

### 1.10 Migration surface

| Metric | Count | Source |
|---|---|---|
| Heap `borrow()`/`borrow_mut()` sites, non-test | ≈501 / ≈248 (primitives 406, frontend ~199, macros 61, vm ~58, TW ~30, core ~15) | heap-repr §8, threads-cost |
| Non-test heap call sites outside core | ~1,300 | prim-embed §0 |
| Direct allocation call sites | 365 in 230 functions (floor) to ~437; name-closure upper bound 889 functions / 2,315 sites | offheap §5.2, threads-cost |
| Handle-hazard patterns (direct scan) | 19 fresh-unrooted-across-alloc; 51 value-used-after-alloc; 25 alloc-in-loop; 9 Rust collection held across allocating loop; 3 alloc + re-entry | offheap §5.2 |
| Primitive registrations | 304 = 280 heap-only `fn(&SharedHeap, &[TV])` + 19 higher-order + 5 resumable | prim-embed §3.1 |
| Primitives touching heap | 291 functions; 78 borrow-then-allocate (43 explicit `drop(guard)`); 54 interior-reference calls (`vector_slice` 9, `get_string_chars` 5, `get_bigint` 11, …) | prim-embed §3.2–3.3 |
| `HeapObjectData::` named outside core | 32 (28 in `datum_writer.rs`) — layout is encapsulated; the borrow protocol is not | heap-repr §8 |
| Off-heap struct types holding TVs needing writable-slot tracing | ~25 | prim-embed §11 |
| Environment-API users | `Rc<Environment>` 173 mentions; `SharedHeap` 598 | threads-cost |
| TW evaluator | 4,613 LOC `cps_eval` + 403 `cont_value.rs`; 84 heap borrows | tree-walker §3 |
| Public embedding API | ~15 `eval_*` returning bare `TaggedValue`; ~200 test call sites | prim-embed §7 |

Migration classes (prim-embed §10): under "context passing, GC only at safe points" ~95% of primitive/frontend/macro
sites are a receiver codemod; ~40–50 sites/structs need thought (re-entry 6, `Rc` payload stores ~10, identity ~5,
datum-writer layout match, string/bignum representation, `SourceMap`/`syntax_sources`, `CompiledMacro.heap`,
`ParsedLibrary`, `Library.exports`, environments). Under "allocation may collect" most of ~1,300 sites need rooting
and no hazard analysis exists — every report rejects this.

### 1.11 Present-day defects found in passing (need issues first, per project rule)

1. Embedder-held values are unrooted: `eval_str` result read after a later collecting `eval_program` → debug
   panic "use-after-free: pair slot 16002", release silently prints `(45294)` (prim-embed §7.2). `run_forms` /
   `eval_program_resilient` hold values across forms the same way.
2. Dropping an `Interpreter` leaks its heap (`Rc` cycle; 36–37 strong refs), so reachable unclosed file ports lose
   output when an embedder returns from `main` (finalization E5).
3. Port identity: `eq?` fails across `current-output-port` calls and `parameterize` (E4).
4. Descriptor exhaustion at 1,021 opens (E2).
5. Rebinding semantics: Patina "follows the name" for code compiled before a redefinition/re-import; chibi, Chez,
   Gauche (mostly) and Racket bind earlier code to the old location; six Rust tests pin Patina's answer, no
   `DIVERGENCES.tsv` row records it (global-cells §2).
6. Docs drift: GC_DESIGN header "not yet implemented", 26 vs 28 variants; AGENTS.md "NaN-boxed" and "24 transfer
   shapes" (the matrix has **64 rows** = 2 extent forms × 2 positions × 16 transfers [V*]); VM_RUNTIME §6
   `value_buffer`; `gc.rs:25` calls `Off` the default.

---

## 2. Hard requirements the collector must preserve

### 2.1 R7RS / SRFI semantics

| Requirement | Detail and evidence |
|---|---|
| **`eq?` identity** | One Scheme object = one identity. Today violated for wrapper objects (§1.6). A JIT needs `eq?` = one word compare, so procedures, RTDs, records and ports must be canonical heap objects first (vm-runtime §6, prim-embed §3.3e, finalization R6). |
| **`eqv?`/hash tables** | All hash tables are Scheme code (SRFI 69 base; SRFI 125 over 69+128; R6RS over 69). SRFI 69 `eq?` tables store location-derived hashes in user data; any moving design needs a stable identity hash (header bits, side table, or GC-rehashed native tables) or must keep those objects still. `eqv?` tables hash structurally (moving-safe). |
| **First-class continuations** | Multi-shot `call/cc`, re-entry, `dynamic-wind` travel one thunk per step through stub frames, delimited capture/abort/resume, re-entry boundary ids. Gate: `control_flow_matrix.rs` (64 rows, both backends) and `escape_from_primitive.rs`; VM_RUNTIME §5.6 table. Invariants: "remaining work = (frame, pc, registers)"; Rust frames are never part of a continuation; captured state is immutable; dead-slot clearing at capture (`deliver_reg` clearing fixed a 296 MB leak, `control.rs:641-654`). |
| **Proper tail calls** | In every path, including deopt from inlined primitives (`control.rs:3471-3492`). |
| **Unbounded non-tail recursion** | 10 M frames work today; a native-stack JIT must not regress it silently (jit-ready R25). |
| **Ephemerons (SRFI 124; SRFI 254 eligible)** | `(gc)` must be a **full** collection that breaks dead-key pairs of every age (`ephemerons.rs:23,52,213,226`); precise liveness in registers, frames and captured continuations (#423 tests `:94,109,127,162` forbid conservative frame scanning); `reference-barrier` must be an opaque use in JIT code; ephemerons + weak ids (+ future guardians) in **one** fixpoint; Larceny `ephemeron` suite needs an automatic major within ~100 M pair allocations. Broken state distinct from `#f`/`#f`. |
| **Ports** (finalization R1–R6) | R1 (pinned by `unclosed_output_ports.rs`): output in live or garbage file ports is written at exit, including `emergency-exit`. R2 (oracle-agreed, unpinned): a collection that proves a file port dead flushes and closes it before Scheme resumes. R3: `EMFILE` must not fail while garbage ports hold descriptors (collect-and-retry; port pressure in trigger). R4: finalization runs no Scheme and no allocation. R5: dropping the interpreter flushes/closes. R6: one port, one object. Note GC-time flushing is observable, so it conflicts with the assumption of the GC-differential lane. |
| **`parameterize`** | Currently shallow binding over a process-wide `Parameter{values: Rc<RefCell<Vec>>}` stack via `%parameterize-swap!` (`lib/scheme/base/parameters.scm`); parameter stores are barrier sites; resumable converters (`Step::Call`). A thread plan wants deep binding as heap data. |
| **Library/global bindings** | "An import installs a binding, not a value" (#406): importer and exporter share one location; `set!` through either is seen by both. Shadow-bit deopt must keep working (#442). Rebinding-time semantics are an owner decision (§6). |
| **Code lifetime** (#338/#353) | Code never freed while a frame, continuation or closure can run it; released within a bounded number of collections (`finished_forms_release_code.rs`, 7–9 tests). |
| **Source locations** | May be lost, never misattributed to a reused address (`source_map.rs:236-245`; `interpreter_api.rs:270`). |
| **Safe-for-space** | Frame sharing in the TW (SRFI 45); iterative deduplicated continuation-chain tracing (Larceny family 6). |

### 2.2 System requirements

- **Both backends.** One object model and one `patina-primitives` crate are shared; a literally separate TW
  collector is not viable, but a per-heap policy (TW heap non-moving or pinning) is (tree-walker §4e′).
- **Debugging hooks.** Root registration must be open (not a closed array) for hook storage, watchpoints, recorded
  datums, green-thread schedulers; a paused debugger evaluating Scheme either defers GC or pins the event payload;
  hooks pin the interpreter tier; the safepoint poll should be a general event word (GC, debugger, fuel/engines,
  interrupts) (tree-walker §6).
- **Embedding API.** Needs rooted handles (scoped and owned) for values held across `eval`; heap teardown on drop;
  "reader without a running VM" must keep working (`patina-compat` uses `new_shared_heap()` as a data parser);
  multiple independent heaps per process (318 live in one test binary). `PRD/FFI_DESIGN.md` must stop exporting
  `HeapIndex`/`SharedHeap`, gain foreign finalizable objects, pinning or a non-moving space for buffers passed to C,
  and global handles for callbacks (prim-embed §8).
- **Testing discipline.** Deterministic, byte-identical differential lanes; oracle-scored matrices; interleaved A/B
  performance method (GC_STAGE5 preamble); CI runs ubuntu and macos; dev platform is macOS arm64 (16 KiB pages,
  W^X for JIT code). Any moving design needs a "move everything every GC" + from-space poison stress mode.
- **Expansion.** syntax-case (planned, `PRD/macro/SYNTAX_CASE_DESIGN.md`) ends whole-form GC-atomicity of the
  expander; procedural transformers become safe points with a deep Rust stack beneath (libload §7).
- **Threading.** Single-threaded `Rc` runtime today. No committed threading plan in the repo: GC_STAGE5 lists
  concurrency as a non-goal (`PRD/ARCHIVE/GC_STAGE5_PRD.md:121`); `TREE_WALKER_HOOK_SYSTEM.md` §9.2 calls SRFI-18
  green threads on one OS thread "the realistic path"; FFI_DESIGN leaves threads open. The research
  recommendation (threads-rec) is: ship SRFI 18 as M:1 green threads; design GC interfaces for N mutators over one
  shared heap (per-carrier `Mutator` context = JIT ABI object; per-mutator barrier logs or byte cards, never
  bit-packed RMW cards or a shared log; one poll word with a deterministic tick mode; safe-region enter/leave
  around blocking I/O; deep-bound dynamic state; no new `Rc<RefCell>` payloads); isolates optional. Cost of real
  shared-heap OS threads estimated 9–18 engineer-months [I].
- **JIT plans.** No concrete JIT design exists in the repo (VM_OPTIMIZATION_ROADMAP §10 ranks it last; TRACK_P
  says JIT is a non-goal of that track; `01_META_TRACING.md` is pre-register-VM and silent on GC). The only
  concrete commitment is the `vm:runtime/control.rs` module doc: a compiled driver must use the same safe-point
  discipline and root provider and "publish all live Scheme values before servicing a safe point" (lines 27-30,
  81-82, 104).

---

## 3. The design space, axis by axis

Each axis: options → exemplars and evidence → Patina fit → JIT implications → panel-relevant lean [I].

### 3.1 Value encoding

| Option | Exemplars | Evidence | Fit / JIT |
|---|---|---|---|
| Typed `u32` index per arena (today) | Nova, safe-gc | Nova compacts typed index arenas by index-shift tables (so index ≠ non-moving). safe-gc author: "not a particularly high-performance" design | JIT decode = per-type base (moves on `push`) + scale + often enum match; worst of all options for a JIT (cranelift-gc §6.4, js-engines L6) |
| Raw tagged address, tag folded into load displacement | Chez (`car` at `ptr+7`), SM, starlark, gc-arena | One load per field (`ldr [v - 3]`) | Best JIT code; needs stable memory; enables conservative filtering via object-start maps |
| 32-bit offset from a fixed per-heap base in a 64-bit word | Wasmtime `VMGcRef`, V8 cage base | base hoisted (`readonly can_move`) or pinned register | +1 add per access; one reservation per heap (feasible: 64 TiB reserves OK; 318 heaps × 16 GiB ≈ 5 TiB; over-reserve to align) |
| 32-bit compressed *fields* | V8 (heap −43%, renderer −20%) | Cost 31-bit Smis (~1%) | Breaks 61-bit fixnums in heap fields; rejected by every report |
| Float immediates: NaN-boxing (roadmap §7) vs self-tagging | Melançon/Serrano/Feeley OOPSLA'25: rotate float bits; 89% of mandelbrot floats immediate, 2.3× on float benchmarks; encode = 3 reg ops + predictable branch | Flonums are 19.8% of all allocations, 79–99.8% in float code, survive ≤0.7% | Needs ~3 primary tags → move vector/string under headers and retire dead `110`. Must be decided **before** the JIT tag ABI freeze (jit-ready R1) |

Patina specifics: fixnum tag `000` permits `adds`/`b.vs` on tagged words (jit-ready R1); make "is heap reference" a
**single bit** so the barrier value filter is 1–2 instructions (barrier-res §4.1); 16 B alignment frees a 4th tag
bit (Chez); headerless pairs need a forwarding sentinel among the 25 spare special codes. Conservative scanning is
only conceivable with sparse addresses (dense small indices are its worst case, js-engines L8).

Lean: tagged addresses (or offsets from one per-heap reservation) in 64-bit words, 61-bit fixnums kept, float
immediates decided now.

### 3.2 Heap organisation

| Option | Exemplars / numbers | Notes for Patina |
|---|---|---|
| Typed growable `Vec` arenas + LIFO free lists (today) | safe-gc | Bases move; never shrink (464 MiB retained after libload); sweep ∝ high-water; free-list allocation degrades locality (RC Immix: free lists +7% retired instructions, mostly cell-by-cell zeroing) |
| BiBOP segments + side `seginfo` | Chez: 16 KiB segments, 512 B cards, seginfo 168 B (1.03%), mark bitmap 256 B (1.56%) on demand, 19 spaces, 2 MiB OS chunks, 3-level radix lookup | Avoid the radix (use a reserved range or aligned slabs) and the 19×8 space/generation matrix |
| Immix blocks/lines | 32 KiB blocks, 128 B lines (MMTk: 256 B), 8 KiB (MMTk 16 KiB) LOS threshold; total time 7–25% better than best canonical collector; conservative line marking; overflow allocator for medium objects | Line size should follow object demographics: 99.2% ≤128 B, mean 27 B → 128 B lines fit |
| nofl granule metadata | Whippet: 16 B granules, 1 metadata byte per granule (6.25%) packing 3-bit rotating mark state, trace kind/pinned, END bit, 2 field-log bits; 64 KiB blocks in 4 MiB aligned slabs; lazy sweep reads only metadata | "Mark table = line table at granule size"; doubles as object-start oracle; supports headerless pairs if traced from the edge |
| Size-class pools | OCaml 5: 32 classes ≤128 words, 32 KiB pools, lazy sweep, colour rotation; Go: 68 classes, Green Tea span-batched marking −10–40% GC CPU | Non-moving old space; fragmentation needs explicit compaction (OCaml 5.2) |
| Regions with region→generation bytemap | .NET 4/32 MB regions, DPAD; G1 | "Is holder old?" = shift + load |
| Large-object space | Immix/Whippet 8 KiB; Chez separate runs, >2 MiB immobile; G1 humongous eager reclaim | 148 of 348 M objects >8 KiB; also the natural home for buffers lent to Rust/FFI |
| Pointer-free spaces | Chez data space; Whippet pointerless kinds; LuaJIT-3 non-traversable arenas | Strings, bytevectors, flonums, bignum limbs never scanned |
| Immortal/static space | Chez static generation 7 (relocs dropped); Larceny static area; Gambit permanent; starlark frozen heaps | Boot image (`lib/scheme` stdlib), symbols, core syntax, code constants — removes the immortal re-marking and code-constant root cost (57% of root tracing pre-#353) |
| Cell space | global-cells §6: append-only non-moving cells, 377/510/2,806 cells (9/12/67 KB) | Removes all Rust name tables from the GC's concerns |
| Code space (non-moving) | Chez code space; cont-repr §4 | Marked, not counted; JIT machine code freed with its descriptor |

Demographics-derived parameters: granule 16 B (56% of objects are exactly 16 B), line 128 B (256 B also fine),
LOS 8 KiB.

### 3.3 Object headers

- Today: no headers; type implied by arena or enum discriminant; size by arena or `Vec` length.
- Common recommendation (heap-repr §10, hotspot §3.10, whippet §6.2, ocaml-gambit §4.1): **one 64-bit header on
  every non-pair object**: subtype (8 bits), length (≥32 bits), GC bits kept separate from mutator-written bits
  (threads-rec §2.12): forwarded, pinned, age, logged (or in side metadata), and 2 identity-hash state bits
  (unhashed / hashed / hashed-and-moved, Bacon–Fink–Grove; JDK compact headers keep a 31-bit hash, age, self-
  forwarded bit, 22-bit class pointer in 64 bits; −22% heap, −8% CPU on SPECjbb).
- Headerless pairs (Chez, Larceny) stay 16 B; MMTk's untagged `ObjectReference` would force pair headers (16→24 B,
  +50%) (immix-mmtk §10).
- Layout choices with evidence: records = header/RTD pointer + inline fields with pointer mask (Chez); closures =
  code pointer in word 0 + inline free variables (Chez), so a JIT call is tag check + load + indirect jump; cells
  16 B; flonum 16 B; strings/bytevectors inline (UTF-32 keeps O(1) `string-set!`); Wasmtime inline pointer
  bitmap in header bits gave 1.07–1.08× on splay; identifiers 72 B → 24 B (symbol id + interned scope-set id +
  source id) (libload §6.2).
- Card scanning without object parsing requires headers that decode as immediates and value-only spaces (Chez's
  vector length is a fixnum) — a constraint, not free (barrier-res §3).
- A single declarative layout spec should generate trace/copy/size/verify code and JIT field offsets (Chez
  `mkgc.ss`: fused accounting ~2× faster, 10–20% faster full GC on locked-object workloads; racket-larceny §3.2).

### 3.4 Moving vs non-moving vs mostly-non-moving

| Option | Exemplars | Evidence |
|---|---|---|
| Non-moving | Go, JSC (removed copying), Lua, Whippet mmc config, Julia/Ruby MMTk defaults (non-moving StickyImmix) | Fragmentation is the failure mode: Wingo's heap-multiplier livelock (every hole 16 B, 32 B request); "if you can't deal with fragmentation, then it is impossible to just rely on a heap multiplier" |
| Full copying | Cheney (Wasmtime copying default since 46.0, 2026-06-22: "more performant in most situations", no barriers); Larceny stop&copy | 2× space; pause ∝ live (≤50 MB live measured) |
| Copying young only | V8 scavenger, SM nursery, OCaml minor, HotSpot young | Needs every nursery referent updatable or in-place promotion for pinned blocks |
| Mostly-non-moving (opportunistic evacuation + pinning) | Immix defrag (2.5% headroom), Conservative Immix (ambiguous roots pin 0.03% of objects; within 2–3% of exact), Whippet mmc (compact when fragmentation >10%, until <5%), Chez/Racket CS mark-in-place unless <3/4 live, Racket BC selective compaction, .NET compact-if-productive, G1 region pinning (JEP 423) | Pinning is per block/segment; residual non-updatable holders become segment-local exceptions |

Prerequisites for any moving (all reports agree): a slot-based visitor (`&mut TV`/`Cell` slots; today
`visit(TaggedValue)` by value, `gc.rs:485`); updatable or pinned off-heap holders (§1.5); id-keyed persistent maps;
stable identity hash; `CallFrame.closure` as a traceable value; Drop-free young objects; traced code liveness.
Patina-specific enabler: under "allocation never collects + no Rust temporaries at safe points", moving needs no
handles in primitives (Chez precedent; hotspot §5.3). SM had to spend 2010–2013 reaching exact rooting to move;
Guile/Whippet: "the work is mostly in the embedder" (~18 k lines added to integrate). JIT: tier-1 code reloads
from the register file after every safepoint anyway; no movable constants in machine code; no derived pointers
across safepoints (Cranelift's old miscompiles).

### 3.5 Generations

| Option | Exemplars | Evidence |
|---|---|---|
| None | Go (Hudson ISMM 2018: generational barrier "simply wasn't fast enough"; ROC 30–50% slowdowns) | Wingo 2025: generational (mmc and pcc) took **more total time than whole-heap on nboyer and splay at all heap sizes**; median pauses fell, max did not |
| Copying nursery, promote on first survival | OCaml (2 MiB minor heap, direct promotion), SM (promote after one minor), Chez gen 0 | OCaml/ICFP 2020: simple two-generation design competitive |
| Copying nursery with aging | V8 (2 scavenges), HotSpot survivors, Lua 5.4 ages | — |
| Sticky mark bits (in-place) | Demers POPL'90; StickyImmix (MMTk), Whippet mmc, JSC eden, V8 MinorMS (experimental) | All three recent retrofits (Julia, Ruby, Guile) chose sticky first; Wingo: "better than nothing, not quite as good as semi-space nursery" |
| Multi-generation | Chez 0..4 + static, radix 4, card byte = youngest generation referenced, promote one generation at a time; `gc-011` specialized minor = 3/4 of collections | Precise multi-generation remset in one byte per card |
| In-place aging/promotion of dense or pinned young blocks | gen-ZGC, JEP 423 | Solves pinning and copy cost together |

HotSpot side: every collector family went generational (gen-ZGC: 1/4 heap, 4× throughput on Cassandra); Zhao &
Blackburn: generational cut G1 p95 pauses 93%; Clinger–Hansen: radioactive-decay lifetimes defeat young-first.

Patina data (demographics §5, §9): bimodal survival; promote-on-first-survival wastes little; dead-old 65–100% ⇒
regular majors; bigger nursery barely helps; queue-shaped and deep-recursion phases need survival-triggered
adaptation (grow nursery, pretenure by site, or bypass nursery above ~30–50% survival); minor GCs need a stack
watermark (deeprec: 11.5 M registers → 12–15 ms mark). Libload is the ideal nursery case (0.3% pairs survive) only
once syntax objects are Drop-free and loading has safe points.

Lean: the decisive wins today are representation + bump allocation + no high-water sweep, which a non-generational
mark-region heap with lazy sweep also delivers. Ship barrier hooks; make generational switchable; gate it on the
M5 experiment (barrier-res §8: gen on / barrier on with minors off / whole-heap).

### 3.6 Write barrier and remembered set

| Kind | Exemplars and published cost | Patina measurement (barrier-res §2.3, §4) | Constraints |
|---|---|---|---|
| Unconditional card byte | HotSpot Parallel/Serial (2 instr; Yang ISMM'12 0.9% ± 0.8%, arch-sensitive; ~20% on a store-heavy worst case) | Re-dirties a few hot cards millions of times | Needs scannable spaces + dead-object masking; misfires with sticky marks (Wingo) |
| Card + young-holder filter | G1 since JEP 522 (~50 → 12 x64 instr, +5–15% on store-heavy apps); .NET region-gen compare + 2 KB cards | Range test with contiguous nursery: 5 instr (+3 value filter) | Same parsability constraint |
| Chez SSB (slot address into nursery tail, fixnum filter only) | Chez/Racket CS (~10 instr, 3 stores) | **0.5–1.0 entries per barrier store**; queue workload 1.5 M entries (~12 MB) | Overflow is a call (Cranelift safepoint) |
| Object remembering (unlog bit) | MMTk ObjectBarrier/StickyImmix (1.6% ± 0.7%); Larceny (object + page-table gen compare, millicode) | vecsort: 7 events, each a 20 K-slot rescan; hashtab bucket vectors rescanned whole | No parsability needed |
| **Field logging (armed bit)** | Whippet mmc and pcc (2 `testb`; 1.05–1.5× faster than cards); LXR (1.6% geomean, worst 4.6%); MMTk FieldBarrier | nboyer 5 slow paths, hashtab 131 K, vecsort 51 K, queue 96, tree 46 K; exact slots | No parsability; fresh memory unarmed ⇒ young holders free |
| OCaml ref table + SATB | `caml_modify` (out-of-line call, non-allocating) | nboyer 24,504 entries vs 5 for field logging (dedup only via "old value young") | ~14 instr + load; "non-safepoint call" impossible in Cranelift |
| JSC host threshold byte | 1-byte load + compare to VmCtx threshold; ~0% idle, ~5% during marking | Object-granular rescans; pairs need a side bytemap | One fast path for generational + incremental-update |
| SATB pre-write | G1/SM/Shenandoah: idle cost one flag test | Only for incremental/concurrent marking | Immediate-value filter is invalid under SATB |

Barrier sites (barrier-res §5): `WriteCell` (93% of nboyer's barrier stores), `VectorSet`, `set-car!`/`set-cdr!`,
`list-set!`, bulk `vector-fill!`/`vector-copy!` (range entries), record set, parameter set, `promise_update`,
global cells / environment stores. No barrier: strings/bytevectors, register file and frame stores (roots),
continuation objects (fresh + immutable; pre-log large ones allocated old, G1 `on_slowpath_allocation_exit`).
Elision: static immediates (today only literals are classified; boolean-producing inline ops qualify; arithmetic
does not, because of bignum overflow); initializing stores with **no safepoint between allocation and store**
(JEP 475) — today every dispatch is a safepoint, so only constructor-internal stores qualify; unboxing write-once
`letrec*` defines removes most `WriteCell`s (jit-ready R13).

Interpreter vs JIT cost: in the interpreter the barrier hides behind a `RefCell` borrow and bounds check already
paid per store (expect <1%, to be measured as M2). In JIT code: ≤6–8 fast-path instructions, slow path out of the
hot block. Cranelift: an out-of-line slow-path `call` in a cold block still forces every `needs_stack_map` value
live across it to be spilled at definition and reloaded at every use (tier 2 only; tier 1 declares nothing). Yang
2012: forcing the object-barrier slow path inline raised overhead 1.6% → 2.6%. Resolution in barrier-res: an
inline, call-free ~10-instruction append to an SSB in reserved VA with a soft limit that sets `gc_request`.

Recommendation on record (barrier-res §7): field-logging, pre-write, armed-bit polarity (fresh memory = 0), dynamic +
static immediate pre-filter, inline call-free slow path appending exact slot addresses to an SSB; minor GC re-reads
and re-arms; major GC re-derives bits. Fast path 8–9 instr (dense 1-bit-per-word table, 1.56%) or 6 instr with 2 log
bits in a per-granule metadata byte. `BARRIER_KIND` and table constants exported in the VmCtx ABI so a card-marking
runner-up remains a one-module switch.

### 3.7 Root discovery

**VM frames.** Precise register file plus per-pc may-root maps (already). Improvements: compress maps to
safepoints only (`register_roots` costs 40 B/PC today); make `CallFrame` plain `Copy` data (`code` as non-moving
descriptor index, `closure` as a TV); non-relocating reserved register stack; a stack watermark (low-water depth
updated on `Return`) so minor GCs scan only frames touched since the last GC (OCaml `Already_scanned`, JEP 376 by
analogy, GHC dirty stack chunks; ~1 compare per return).

**JIT frames.**

| Strategy | Exemplars | Cranelift needs | Moving | Continuations | Cost |
|---|---|---|---|---|---|
| VM register file at safepoints (publish before, reload after) | V8 Sparkplug (5–15% gains), Guile 3 JIT, SM Baseline (`syncStack` before VM calls, dead locals overwritten), Racket BC runstack, Steel, LuaJIT (trace exit before atomic phase) | none; optional alias regions | yes | unchanged VM-level capture | memory traffic at calls (values crossing calls are spilled under any scheme) |
| Native frames + user stack maps | Wasmtime (FP walk, PC→map lookup, slot addresses updated in place) | `declare_value_needs_stack_map`, `preserve_frame_pointers`, own map registry (`JITModule` does not surface maps) | yes | must deopt native frames into VM frames, or copy native stacks (absolute FP chains) | spills + walker + code registry |
| Conservative native scan + pinning | JSC, Whippet stack-conservative mmc, V8 direct handles + CSS (off by default) | none | only unpinned | as above | **Forbidden by Patina's #423 ephemeron tests** (precise liveness is semantic) |

**Rust code.**

| Strategy | Exemplars | Cost evidence | Patina fit |
|---|---|---|---|
| Allocation never collects + collection only at safe points with no Rust temporaries (deferral where nested) | Chez, gc-arena ("mutation XOR collection"), piccolo (stackless), Ruffle, today's Patina | zero per-site cost | Today's model; nested deferral is the GCLocker problem (JEP 423 lesson) |
| Same, enforced by types (branded `Value<'gc>`, `&Mutation`, `collect(&mut Heap)`) | gc-arena, rune, starlark `'v`, Nova `GcScope`/`NoGcScope` | Nova: ~800 bind/unbind sites in ~100 k lines (because JS spec ops call user code everywhere); starlark retreated to module-level-only GC | Patina's may-GC surface is small after #471–#478 |
| LIFO root scopes + owned handles at boundaries only | Wasmtime `RootScope`/`Rooted` ("roughly bump allocation") and `OwnedRooted`, V8 `HandleScope`, rune `root!`, Brimstone | cheap | Library loading points A/B/D/E are strictly LIFO (libload §4.2); expansion point C needs a literal pool and memo epochs |
| Shadow stack / handles everywhere, allocation may GC | Racket BC xform (4,352-line transformer), OCaml `CAMLparam`, SM `Rooted` + sixgill hazard analysis, V8 17,000 `HandleScope`s + gcmole | very high | Rejected by all reports |
| Conservative Rust stack | Alloy, JSC | blocks moving | Rejected |

Values held in Rust structures long-term: become heap objects (environments/cells, records, cells, promises,
parameters, macro literals into a heap vector, continuations), go into an immortal space (code constants,
symbols), or use indirection handles (Wasmtime: "all of our rooting types use indirection"), or be reported as
non-transitively pinning roots (MMTk `create_process_pinning_roots_work`, Chez `lock-object`).

### 3.8 Safepoints and polling

- Today: one flag load per dispatched instruction, outermost loop only.
- Prior art: Chez trap counter (`trap -= 1; je`, 2 instructions, at entries of procedures that call and loop
  heads; timer 1000 ticks; GC requests via `something-pending`); OCaml `young_limit` (allocation limit doubles as
  interrupt word, `limit = MAX`), PRTC rule for tail-call languages; Gambit `stack_trip`; V8 stack-limit fold;
  HotSpot thread-local poll page (trap-based; **not expressible as a Cranelift safepoint**); Wasmtime epochs (2–3×
  cheaper than fuel). Lin et al. ISMM'15: untaken conditional poll 1.9%, load trap 1.2%, patched NOP 0.3%.
- Patina facts: compiled code has only forward jumps, so polls at function entry and self-tail-call back-edge
  cover all loops (jit-ready R14); allocation slow paths only raise the flag. If async interrupts in JIT code are
  not wanted, loops that neither allocate nor call need no GC poll at all (cranelift-gc §4).
- One event word for GC, debugger, engines/fuel, Ctrl-C, green-thread preemption (deterministic tick counter for
  test lanes), handshakes later (tree-walker §5, threads-rec §2.7).
- Nested loops: replace `GcDeferGuard` with rooted boundaries (libload R1–R6; Stage-5 Priority 2), keep deferral
  only for expansion point C and the TW.

### 3.9 Continuations and stacks

| Design | Capture | Invoke | Risk | Notes |
|---|---|---|---|---|
| Today | O(total depth): clone 5 `Vec`s into weak `Rc` side tables | O(depth) clone back | — | byte-blind, weak-id soundness rule "store touched within one dispatch" (`gc_roots.rs:21-24`) |
| **A** traced snapshot heap object | O(depth), ~5.8× cheaper constant (toy) | O(depth) memcpy | low | deletes side tables, byte-accounted, movable, no control-algorithm change |
| **B** seal-in-place segments | O(1) | O(1) + bounded underflow (≤128 B + 1 frame) | high | Chez (Hieb–Dybvig–Bruggeman), Gambit break frames (frames migrated to heap at GC) |
| **C** Loom freeze/thaw chunks | O(frames) | lazy thaw via return barrier | — | one-shot in Loom; young chunks need no barrier |
| **C′** freeze above a frozen watermark + immutable chunk chain + lazy thaw | O(frames resumed since last freeze); toy 57 ns at d=1000 vs 5.0 µs today | O(1) graft; 1.6–4.4× slower than A on reinstate-and-return-all | medium-high | Larceny stack-cache flush discipline; keeps the live stack one non-relocating buffer (JIT-friendly) |

Common rules (cont-repr §7): continuations are heap objects measured in bytes; frames are plain `Copy` data with a
code reference into a non-moving marked code space; depth-integer discipline (relative distances where possible);
stub frames for underflow/overflow; captured state immutable (no barriers on chunks); code liveness decided only
by complete marking (deletes `live_closures`/`gc_freed_closure_code_ids`). JIT: S1 fragments with `tail`
convention and `return_call_indirect` keep native depth constant and let escapes be "return a new target"; S2
native call/ret needs unwinding (Cranelift `try_call`, internal unstable unwinder) and a depth cap; Cranelift
`stack_switch` is x64-only, one-shot, 2 MiB per continuation in Wasmtime — unusable for multi-shot `call/cc`.
OCaml/Gambit: a captured stack reached only through a freshly allocated object needs no stack write barrier;
darken on reinstatement under incremental marking.

### 3.10 Weak references, ephemerons, guardians, finalization

- Must replace "sweep visits the dead": port flush/close, `MemoryFs` commit, cycle breaking, payload freeing, code
  release, `SourceMap` pruning.
- Mechanisms: Drop-free object model first (a drop list over 47% of allocations is a sweep in disguise);
  registrations split by generation (OCaml minor custom table, Wasmtime per-semispace externref list, rune
  `drop_stack`, Chez per-generation guardians); a non-moving "drop space" for the ≤1% residual needs-drop kinds
  (starlark `drop`/`non_drop`); port table + finalization registry (ports become `{hdr, port_id}`; `current-*-port`
  hold heap values).
- Ephemerons: key-indexed pending resolution (Whippet: every newly marked object checks a pending table) or
  segment triggers (Chez, Racket BC pages) instead of round rescans; generational rule "E never older than K or V"
  (always allocate ephemerons in the nursery; promotion age-monotone) removes remset edges for them.
- Weak-key tables processed by the collector (rekey forwarded, drop dead; Chez `tlcs_to_rehash`) for
  `syntax_sources`/`SourceMap`/SRFI 125 weak tables/a future weak symbol table — or eliminate the tables (libload:
  inline provenance).
- Host-payload tables (generalized `trace_weak_ids`/`sweep_weak`) for Rust-owned payloads (TW `CpsLambda`,
  `CpsContinuation`), with a young-entry list for minors and pinning edges for payload TVs.
- Guardians (SRFI 254, final 2026-06-30; supersedes withdrawn SRFI 246; adds transport cell guardians): optional,
  after the redesign; GC never runs Scheme, only fills queues. No Java-style finalizers (JEP 421: deprecated;
  7–11× overhead).
- Epilogue order (finalization §4.9): trace → one fixpoint (weak ids, ephemerons, guardians) → break pending
  ephemerons → transport cells → weak-key tables → weak-id tables → Rust finalizers → code release (majors only).

### 3.11 Incremental and concurrent options

- Concurrent relocation (ZGC colored pointers, Shenandoah LRB) needs spare cores and load barriers (avg 5.4% read
  barrier, Yang 2012; LXR: loads outnumber stores ~15×): reject for a single mutator.
- Cai et al. ISPASS'22: OpenJDK collectors cost 7–82% more wall time than a lower bound; newer low-pause GCs
  "sometimes deliver worse application latency than stop-the-world GCs". LXR: brief regular STW pauses + cheap
  combined barrier beat G1 by 4%, Shenandoah by 43%.
- Single-thread incremental options: Racket BC fuel-limited old-gen marking piggybacked on minors using the card
  barrier as incremental-update (with a 2× fragmentation fallback to full GC); OCaml 5 SATB incremental marking +
  lazy sweep paced at the minor heap half-way point; JSC threshold-byte retreating wavefront; Lua 5.5 incremental
  majors in generational mode. Racket CS gave up true incremental marking (its "incremental" only defers
  promotion). Larceny's regional collector bounded MMU (0.07 s max pause vs 0.80 s generational at ~160 MB live)
  at ~1.8× elapsed time and 10,135 lines vs 4,757 for the classic generational machinery.
- Patina pause data: worst today 41 ms (queue3) and 178 ms (post-load sweep, a representation artifact); live heaps
  ≤50 MB in the hypothetical layout.
- Lean: STW, lazy sweep, and keep the barrier ABI room for a later mode (`barrier_mode` byte read only by slow paths;
  pre-write barrier keeps the old value available; darken-on-reinstatement rule for continuations).

### 3.12 Heap sizing and pacing

- Today: object counts, `max(65,536, 2 × live slots)`, no decommit.
- Options: bytes allocated + bytes live with a headroom multiplier (Guile growable 1.75×; Gambit 50% live; OCaml
  `space_overhead` 120 with work-unit pacing; Racket BC 2× after last full GC, ≥20 MB; Racket CS
  `post + 8192·√post`; Chez 8 MiB gen-0 trip, radix 4, max-gen when heap doubled); MemBalancer (Kirisame et al.
  OOPSLA'22: 16% less memory at constant GC time or 30% less GC time; Whippet adaptive sizer; caveat "hyperactive
  squirrel"); GC CPU fraction goal (G1 default 4% since JDK 26; Go GOGC + soft GOMEMLIMIT); DATAS (heap ∝ live,
  >80% working-set cut for 2–3% RPS); minimum free reserve after GC (Wingo livelock fix); external memory terms
  (Chez phantom bytes, OCaml `caml_alloc_custom_mem`, V8 external memory, port buffers and descriptor counts);
  decommit after spikes (Chez `heap-reserve-ratio`; `MADV_FREE` measured instant).
- Nursery: ~64 K–256 K objects ≈ 1.5–6 MB (hypothetical layout), scaled with allocation rate; references: OCaml 2
  MiB, Racket CS 8 MB per place, Chez 8 MiB trip, Larceny 1 MB, SM 256 KiB–64 MiB with a 4 ms time goal, V8 up to
  32 MiB semispaces.
- Observability first: per-phase timings, bytes, histograms, MMU log (Larceny `gc_mmu_log.c`), tracepoints
  (Whippet); today `last_pause_micros` is computed and never read.

### 3.13 Environments and global bindings (a GC axis, not just a compiler one)

- Today: off-heap `Rc<Environment>` slot tables; imports as `FORWARDED` + `Owner` links; closures carry
  `globals: Rc<Environment>` (one link of the teardown cycle); environment stores bypass any barrier, so a
  generational design must rescan all environments every minor (25–51 µs from the global env alone) or keep a
  dirty-environment list (OCaml generational global roots).
- Cell model (global-cells): one heap cell per *owned* top-level binding (imports share the exporter's cell, giving
  #406 by construction and closing its copy exceptions); `UNBOUND` immediate; non-moving append-only `CellSpace`
  outside the `Heap` `RefCell`, treated as a root region (dirty list for minors); `LoadGlobal` = constant-table
  cell → load; shadow bits become per-site cell-value guards; `WATCHED` flag for JIT invalidation (V8
  `PropertyCell` `kConstant` + dependent code). Variant **R** (re-pointable importer slots) preserves today's
  semantics and is the safe first step; variant **C** (compile-time binding) matches chibi/Chez/Racket and flips six
  pinned tests (owner decision).
- TW: globals can go through cells while frames stay `Rc<Environment>`; parity under C needs desugar-time global
  annotation (first slice of SYNTAX_CASE_DESIGN's "resolve once before the backends").

### 3.14 Expansion-time allocation and provenance (where load-time memory actually goes)

- Provenance options (libload §5): (a) source id inside syntax objects (Racket `srcloc` field, Chez annotations):
  −300 MiB at peak, −20% load CPU, moving-safe; (b) weak id-keyed tables: keeps 316 MiB plus per-move work;
  (c) non-moving syntax space: fights the nursery for exactly the dominant load-time allocation and cannot know
  what `datum->syntax` will produce. Delete the two unread stores and the throwaway per-expansion `SourceMap`
  regardless (−26 MiB peak, −355 MiB churn).
- Identifiers: interned scope sets (64,782 distinct sets for 2.19 M identifiers) → 40 B enum −44%; 8 B header +
  symbol id + scope-set id → −78% and **no Drop payload** (precondition for free young collection of syntax);
  hash-consing −92%.
- Lazy scope propagation (Racket `apply-scope` pending propagation; Chez wraps) would remove most of the 96.7% copy
  share and 44% of load CPU — a front-end change that decides how much the collector must do during loading.
- syntax-case: expander becomes a root provider (`ExpansionContext` with LIFO roots, a `CoreExpr` literal pool,
  epoch-checked memos); transformer calls are re-entry boundaries; one cheap scheme is a minor GC at transformer
  entry over a non-moving (or evacuation-suspended) mature space.

---

## 4. Candidate architectures

### 4.0 The common core every candidate needs

The reports converge on a set of changes that are architecture-independent and dominate the cost of *any*
candidate (immix-mmtk §10: "the representation rewrite is the dominant cost either way"). The panel should treat
these as Stage 0–1 and decide them first:

| # | Change | Unblocks | Source |
|---|---|---|---|
| C1 | Explicit heap context replaces `Rc<RefCell<Heap>>`; allocation never collects (slow path refills, raises flag, overflows into new blocks under a soft limit, hard limit → out-of-memory error); only the driver holds what `collect` needs | JIT access without `RefCell`; codemod of ~1,300 sites | prim-embed §11, rust-gcs §4 |
| C2 | Slot-based tracing (`&mut`/`Cell` slots) + one layout spec generating trace/size/copy/verify/JIT offsets | moving; generational; fewer misfiled-leaf bugs | js-engines L3, chez §6 |
| C3 | Stable block/segment memory with side metadata; LOS; pointer-free kinds; immortal boot space; decommit | JIT bases; returning memory; inline allocation | all |
| C4 | Headers on non-pairs; inline payloads; 16 B cells/flonums; closures = code + inline free vars; records inline; identifiers as ids | 2.05× bytes; Drop-free objects | heap-repr §10, demographics §4.4 |
| C5 | Canonical identity (RTDs, procedures, records, ports) + stable identity hash design | one-compare `eq?`; moving | §1.6 |
| C6 | Drop-free nursery-eligible objects; host-payload tables; port table + finalization registry; drop space | copying/sticky nurseries; R1–R6 | finalization §4 |
| C7 | Global binding cells (variant R first) | barrier-free env roots; JIT `LoadGlobal`; teardown cycle | global-cells |
| C8 | Continuations as traced byte-accounted objects (design A); `Copy` frames; traced code liveness in a non-moving code space | deletes VM weak tables; copying nursery | cont-repr §6 |
| C9 | Byte trigger + external-resource terms + MemBalancer-style sizing with a free reserve | pathologies of §1.9 | demographics §9.5 |
| C10 | Rooted boundaries replace `GcDeferGuard` (loading A/B/D/E, `globals_stack`, `ParsedLibrary.body` in registry, TW trampoline stack); embedder handles; heap teardown | collection during loads; embedding UAF | libload §4, §8 |
| C11 | Store funnel: every mutation through barrier-aware API; remove `vector_slice_mut`; records/params/promises in heap | any barrier | barrier-res §7.3 |
| C12 | `#[repr(C)] VmCtx` ABI object (alloc ptr/limit, event word, barrier constants, register-stack top/limit) à la Whippet `gc-attrs.h`; non-relocating register stack | JIT independence from collector choice | jit-ready R35 |
| C13 | Inline provenance; delete unread stores; raw-bits maps transient or id-keyed | moving; libload memory | libload §5 |
| C14 | Test infrastructure: null collector, gc-zeal move-everything + from-space poison, heap verifier, Miri on the collector crate, two-mutator test mode, Larceny-derived GC benchmark set with pause/MMU metrics | everything | rust-gcs §4.7, threads-rec §3.1 |

Effort anchors [I]: the primitive/frontend receiver codemod is mostly mechanical (~95% of sites); the
needs-thought list is ~40–50 sites/structs; continuation design A is "low risk", traced code liveness "medium";
global cells phase R is a contained `patina-core` change plus test adjustments; TW host-payload ids 1–3 weeks.
The collectors below are each a few thousand lines on top (Rust Immix prototype 1,449 LOC; sticky + defrag
+2–4 k LOC; "~5 k lines of focused Immix"; Larceny classic generational machinery 4,757 lines).

### 4.1 Candidate A — Non-moving mark-region with sticky-mark generations

*Sketch.* Whippet-mmc-style nofl space without evacuation: 16 B granules, a metadata byte per granule (rotating
mark state, END bit, pointer-free/pinned kind, 2 field-log bits), 64 KiB blocks in aligned 4 MiB slabs, LOS ≥8 KiB,
pointer-free and immortal spaces. Bump allocation into holes; lazy sweep reading only metadata; blocks returned when
empty. Generations by sticky mark bits (minor = roots + remembered set; promoted-block list), switchable off.
Field-logging barrier with static field parity (~6 instructions). Nothing ever moves: identity hash may be the
address; raw-bits maps stay valid; TW needs no pinning. Exemplars: Whippet mmc (non-moving config), MMTk
StickyImmix with `sticky_immix_non_moving_nursery` (Ruby and Julia defaults), JSC.

*Strengths.* Smallest semantic risk; no identity-hash or provenance rekeying; TW and residual `Rc` holders keep
working without pinning; immediate RSS/pause wins from C3/C4/C9 (no high-water sweep, memory returned); simplest
JIT contract (no reload-after-move beyond the register file); natural first stage of B.

*Risks.* Fragmentation with no cure (Wingo's livelock; needs a free reserve and medium-object overflow blocks);
nursery in recycled holes has worse locality than a fresh bump region ("not quite as good as semi-space"); dead
young objects still cost a metadata sweep; generational may lose (nboyer/mperm neutral, queue3/deeprec worse);
Drop payloads still need a registry (lazy sweep never reads object memory). Inline allocation writes begin/end
metadata (8–10 instructions in Whippet vs 3–4 for a pure bump).

*Effort.* Common core + ~3–6 k LOC collector [I]. *JIT.* Inline bump into hole (`hp`, `limit` at fixed VmCtx
offsets + metadata bytes), field-log barrier inline with call-free slow path, polls at entry/back-edge; tier 1 needs
no stack maps; no moving means tier 2 user stack maps would only be needed for liveness, not relocation.

### 4.2 Candidate B — Mostly-marking: mark-region + opportunistic evacuation + pinning

*Sketch.* Candidate A plus Immix-style defragmentation: at a major GC, blocks selected from the previous cycle's
occupancy are evacuated using a small reserve (2–2.5%); everything else is marked in place. Optional young-survivor
evacuation inside sticky collections (GenImmix-like behaviour without a separate nursery). Per-object pin bit;
non-transitively pinning roots for holders that cannot be updated (TW Rust structures, FFI buffers, debugger event
payloads, objects whose address hash escaped). Identity hash via header hashed/moved bits (pairs: side table or
pin-on-first-hash, Guile's choice for `hashq`). Exemplars: Immix + Conservative Immix (precise heap edges, pinned
ambiguous roots, within 2–3% of exact), Whippet mmc full, Racket CS mark-in-place + sparse evacuation per 16 KiB
segment.

*Strengths.* Cures fragmentation while keeping non-moving a configuration flag; tolerates residual non-updatable
holders by pinning, so the TW can share the collector; moving work is proportional to sparse blocks only; generous
evolution path from A.

*Risks.* Requires all moving prerequisites (C2, C5, C13) before any evacuation is enabled; forwarding + pin +
log-bit interplay (Whippet's live parallel-evacuation race is a warning for any future parallel tracer); full
compaction takes ~3 cycles; pinning-heavy TW heaps barely evacuate (acceptable for a non-performance tier).

*Effort.* A + ~2–4 k LOC [I]. *JIT.* Same as A; tier-1 code reloads from the register file after safepoints;
machine code embeds no movable constants.

### 4.3 Candidate C — Chez/OCaml hybrid: copying contiguous nursery + mark-in-place old space

*Sketch.* Per-heap contiguous nursery reservation with a pure bump allocator (`ap`/`limit` in VmCtx; the limit word
doubles as the event word, OCaml-style); `is_young` = range test. Minor GC copies survivors (promote on first survival,
or aging, with adaptive bypass/pretenuring for high-survival phases) into an old space of segregated blocks marked
in place, with sparse-block evacuation at majors (Chez `use_marks` rule: evacuate segments <3/4 live). Pinned or
dense young blocks promoted in place (JEP 423 / gen-ZGC). Immortal static space for the boot image, non-moving code
and cell spaces, LOS. Barrier: field logging (Whippet generational-pcc uses exactly this with a copying nursery) or
card + young range filter if the representation commits to value-only spaces. Continuations C′ (or Chez segments).
Exemplars: Chez/Racket CS, OCaml 5 minor heap, HotSpot young collection, V8/SM nurseries, Wasmtime copying.

*Strengths.* Dead young objects cost nothing (libload's 99.7% garbage, closures/cells/flonums ≤0.7% survival);
cheapest allocation (3–4 instructions) and best locality; trivially cheap young test; Scheme-proven with first-class
continuations (Chez); in-place aging handles pinning.

*Risks.* Largest prerequisite set: Drop-free nursery objects (47% of allocations carry Drop today), all moving
prerequisites, ephemeron/weak generational rules, TW pinning or a non-moving TW policy, copy reserve; loses on
queue-shaped and deep-recursion phases (1.7–1.8× more copying) unless adaptive; Wingo's mixed generational result;
first benefit arrives late in the migration.

*Effort.* Largest collector work (copying + old space + promotion + adaptive policy ≈ A+B scale or more) [I];
must follow C6/C8 completely. *JIT.* Ideal inline allocation; barrier young test free by construction (unarmed fresh
memory) or a range compare; tier-1 reload after safepoints; stack watermark for minors.

### 4.4 Candidate D — MMTk-backed

*Sketch.* Implement MMTk's `VMBinding` (ObjectModel, Scanning, Collection, ActivePlan, ReferenceGlue, Slot) over the
new representation; start with StickyImmix non-moving nursery, move to Immix/GenImmix with moving, consider LXR
(merged to master 2026-08-19, not yet released) later. JIT inlines MMTk's `#[repr(C)] BumpPointer{cursor, limit}` and
unlog-bit barrier constants (proven by mmtk-openjdk C1/C2).

*Strengths.* Every relevant algorithm, research-grade and maintained (MIT/Apache); pinning roots, VO bits,
transitively pinning roots, `process_weak_refs` re-invocation maps onto Patina's fixpoint; `AllocationOptions
{at_safepoint: false, allow_overcommit: true}` can preserve "allocation never collects".

*Risks* (immix-mmtk §10). **One MMTk instance per process** with a fixed VA layout (issue #100 open since 2020) vs
318 heaps per test process and embedding; GC work on MMTk-spawned worker threads with `Send + Sync` bindings vs
Patina's `Rc` world (`single_worker` is still a separate thread; ≥2 handoffs per GC); untagged `ObjectReference`
forces headers on pairs (+50%); real bindings pin git revisions; aarch64 macOS only "tier 2, guaranteed to build"
since 2026-09-24; ~30 direct dependencies; nondeterministic parallel trace order vs deterministic differential lanes;
Ruby needed a dedicated team ~1 year to reach parity; Julia: moving "not supported yet" because of hidden references
in runtime and JIT code — Patina's exact situation.

*Effort.* Binding + common core; collector code saved, integration and coupling added. Reverses if MMTk gains
multi-instance support or Patina adopts native threads. *JIT.* Proven pattern; emitter coupled to a pinned MMTk
revision's constants. Recommended use: keep seams MMTk-shaped so an A/B spike stays possible (needs approval to
download crates and a one-interpreter-per-process test mode).

### 4.5 Candidate E — OCaml-5-style: copying minor heap + non-moving size-class major with incremental marking

*Sketch.* Small contiguous minor heap (≈2 MiB scale), downward bump with `young_limit` as the universal poll word;
promotion directly into a non-moving major heap of size-class pools (32 KiB pools, ≤128-word classes; large objects
separately), free lists threaded through free blocks, lazy sweep amortised into allocation, colour rotation instead of
mark clearing; incremental SATB marking in slices paced by minor-heap consumption (`space_overhead`-style knob);
combined barrier (remember old→young slot, darken old value while marking); explicit compaction added later as in
OCaml 5.2. Deferred regions allocate directly into the major heap with initializing-store logging (OCaml
`caml_alloc_shr` + `caml_initialize`), which is a clean answer for expansion point C and syntax-case.

*Strengths.* Proven generational + incremental combination with bounded pauses; old objects never move (stable
addresses for anything promoted: identity hash and raw-bits keys could rely on promotion-time stability if young
objects are excluded); pacing model well documented; continuation story via fibers' "scan stack at promotion,
darken on resume".

*Risks.* Free-list major has worse mutator locality than bump/Immix (RC Immix +7% instructions); fragmentation
needs the later compactor; OCaml's ref table has no dedup (24,504 entries vs 5 on nboyer); SATB barrier is longer
(~14 instructions + a load) and incremental marking buys little at Patina's measured pause levels; still needs every
moving prerequisite for the minor heap.

*Effort.* Comparable to C plus incremental machinery [I]. *JIT.* 3-instruction allocation (`sub; cmp; b`), poll
folded into the limit; barrier inline filter with call-free slow path; tier-1 register-file model.

### 4.6 Comparison

| Criterion | A sticky non-moving | B mostly-marking | C copying nursery + MIP old | D MMTk | E OCaml-5 style |
|---|---|---|---|---|---|
| Moves objects | never | sparse blocks (optional young) | all young + sparse old | plan-dependent | young only (+ later compaction) |
| Prerequisites beyond C1–C14 | fewest (no C2/C5/C13 strictly required) | C2, C5, C13 before enabling evacuation | all, plus complete C6/C8 | all + headers on pairs + threads/`Send` | all |
| Fragmentation answer | reserve only (livelock risk) | evacuation | evacuation | yes | later compaction |
| Dead young objects | metadata sweep | metadata sweep | free | plan | free |
| Allocation fast path | bump into holes + metadata (8–10) | same | pure bump (3–4) | bump (binding-inlined) | pure bump (3) |
| Barrier | field log (sticky) | field log | field log or card+range | MMTk unlog/field | ref table + SATB |
| TW backend | as is | pinning roots | non-moving TW heap or pinning + in-place promotion | pinning roots | non-moving TW heap |
| Identity hash | address | header bits / pin-on-hash | header bits / side table | binding-defined (address-hash states) | stable after promotion |
| Incremental later | via barrier ABI | via barrier ABI | via barrier ABI | ConcurrentImmix/LXR | built in |
| Multi-heap per process | yes | yes | yes (one reservation per heap) | **no** | yes |
| Risk | low | medium | medium-high | high (integration) | medium-high |

Observation [I]: A → B is an incremental path (same space, evacuation switched on), and B with young-survivor
evacuation approaches C's behaviour without a separate nursery. The common core (§4.0) is identical for all five, so
the panel can commit to it before choosing.

---

## 5. Lessons and pitfalls from prior art ("do not repeat this")

1. **Letting allocation collect.** Racket BC's xform source-to-source rooting, park slots and JIT retry stubs; SM's
   three-year exact-rooting effort plus a static hazard analysis; V8's 17,000 `HandleScope`s and gcmole; Nova's ~800
   bind/unbind sites; starlark retreating to module-top-level-only GC (racket-larceny §2.2, js-engines §2–3,
   rust-gcs §2.3, §2.6). Chez proves a moving generational GC works without it.
2. **Disabling GC to protect raw access.** HotSpot's GCLocker blocked applications for minutes and was replaced by
   region pinning (JEP 423). Patina's `GcDeferGuard` is the same mechanism (library body churning 5 M conses keeps a
   116 MB arena; loads peak at 627 MiB).
3. **Making finalization, cycle breaking or code release depend on visiting the dead.** Breaks under any evacuating
   nursery (Patina sweep tombstoning; `live_closures` counting). Use per-generation registration lists.
4. **Counting objects instead of bytes, and ignoring external memory.** Patina: 628 MB RSS for 12 MB live, 5.7 GB
   of continuation snapshots, `EMFILE` at 1,021 opens. Chez phantom bytes, OCaml custom memory, Gauche's byte
   trigger avoid it.
5. **Weak side tables keyed by object identity at scale.** Patina's provenance table (316 MiB) cost more than the
   arena it annotates; store data in the object (Racket, Chez) (libload §9).
6. **Untraced or by-value root visitors.** A visitor that copies values cannot support moving (`gc.rs:485`); hand-
   written per-type tracers went exponential without dedup (6.8 s per GC at depth 26).
7. **Sequencing weak kinds separately.** Patina's use-after-free before commit `1d18c49`; one fixpoint for all weak
   kinds.
8. **Round-based ephemeron rescans.** O(n²): 289 ms for a 16 k chain. Use key-indexed or segment triggers.
9. **Register-allocator-integrated stack maps and derived pointers across safepoints.** Cranelift's pre-2024 maps
   miscompiled (field address reused across a moving GC) and produced CVEs; HotSpot's `DerivedPointerTable` is
   C2-only complexity. Recompute addresses after every safepoint.
10. **Embedding movable addresses in machine code / patching code at GC.** SM patches through relocation tables; on
    macOS arm64 every patch toggles W^X. Load constants from a traced table or immortal space.
11. **Load barriers without concurrency.** ZGC/Shenandoah mechanisms cost ~5.4% read barrier on average; pay off only
    with concurrent GC threads.
12. **Barrier bloat.** G1's 40–50-instruction barrier with a StoreLoad fence and queue cost 10–20% throughput and
    distorted inlining until JEP 522 (12 instructions). Keep the fast path ≤6–8 instructions, emit it late from one
    definition shared with the interpreter (JEP 475: early expansion cost 10–20% of C2 compile time).
13. **Card marking on sticky-mark heaps.** Stores into young objects dirty cards covering old objects (Wingo; field
    logging 1.05–1.5× faster). Cards also require heap parsability (Chez's record start-finding: "abandon hope all ye
    who enter here").
14. **Assuming generational always wins.** Wingo (nboyer, splay), Hudson (Go ROC), Clinger–Hansen radioactive decay;
    Patina's queue3/deeprec. Make it switchable and measure.
15. **Multiplier-only heap sizing on a non-moving heap.** Fragmentation livelock (Wingo 2025-05-22); keep a free-block
    reserve and an evacuation escape.
16. **Conservative scanning to avoid rooting.** SM abandoned it to get a nursery and compaction; V8's direct handles +
    CSS need pinning and quarantined pages and are still off by default; dense index encodings make false positives
    rampant; Patina's #423 ephemeron tests forbid it.
17. **Inline slow paths that are calls, under Cranelift user stack maps.** Every non-tail call is a safepoint and
    forces spills of all declared values (barrier-res §6). Make slow paths call-free or keep tier-1 values in the
    register file.
18. **Retaining dead registers in snapshots.** Patina's 296 MB leak before clearing the call/cc `dst`; keep dead-slot
    clearing at capture in any representation.
19. **Car-first worklist order.** O(list length) mark stack (+55 MB); bound the mark stack (Racket BC segmented mark
    stack; Go Green Tea span batching).
20. **Free-list allocation for small objects.** RC Immix: +7% retired instructions from cell-by-cell zeroing; bump
    allocation into holes/lines is cheaper and has better locality.
21. **Maintaining many collector modes.** ZGC, Shenandoah and Lua kept a second mode for years and retired it for
    maintenance cost (JEP 474/490/535). Keep one collector plus a null/off mode for differential testing (Epsilon,
    Wasmtime null, Patina `GcMode::Off`).
22. **Ownership-partitioned parallel GC with one mutator.** Chez's design gives no speed-up for a single mutator;
    use work stealing if parallel tracing is ever wanted.
23. **Process-wide GC singletons.** MMTk's one instance per process conflicts with per-interpreter heaps (318 live in
    one test process).
24. **Retrofitting a modern GC under hidden references.** Julia's moving GC is "not supported yet" because of hidden
    references in runtime C code and JIT code; Ruby needed ~1 year to reach parity; Guile's integration was mostly
    embedder refactoring. Inventory every escape first (heap-repr §6 is that inventory).
25. **Java-style finalizers.** JEP 421 deprecated them (resurrection, unpredictable latency, 7–11× overhead). GC
    fills guardian queues; Scheme drains them; Rust finalizers do no allocation and run no Scheme.
26. **Relying on `collect` to break ephemerons of migrated objects.** Chez's manual warns `collect` may not suffice;
    Patina's tests require `(gc)` to be a full collection.
27. **One-shot stack switching for multi-shot continuations.** Cranelift `stack_switch` is x64-only and one-shot;
    Wasmtime never frees continuation stacks. Keep `call/cc` a VM-level operation.
28. **True incremental marking without measured need.** Racket CS gave it up; Larceny regional cost ~1.8× elapsed
    and 2× code for provable MMU.
29. **Interior pointers into growable buffers.** Patina's `Vec` arenas and register `Vec` relocate on growth; Guile
    stores dynamic links as offsets because "the stack can move"; reserve address space instead.
30. **Heap-per-instance without teardown.** Patina leaks every dropped interpreter's heap; with one VA reservation
    per heap, reservations would accumulate. Break `Rc` cycles (no `heap` field in `Environment`/`CompiledMacro`).
31. **Copy-on-capture continuations.** O(depth) capture made ctak 17× slower and 41× larger under 500 frames; Chez
    segments, Gambit break frames and Loom chunks make it independent of depth.
32. **Diagnostic data keyed by addresses that a minor GC recycles.** Freed-bits feeds cannot work under a copying
    nursery; Patina's 65,536-entry cap already overflows after every large import.

---

## 6. Open decisions only the project owner can make

| # | Decision | Why it matters | Evidence / default lean if unconstrained [I] |
|---|---|---|---|
| 1 | **Is the tree-walker a long-term production backend or a reference/debugging tier?** | Decides whether TW frames/continuations must become heap objects (6–12 weeks, high risk) or the TW heap runs non-moving/pinning with host-payload ids (1–3 weeks) | tree-walker §8; lean: reference tier, non-moving or pinning policy |
| 2 | **Global rebinding semantics: variant C (compile-time binding; chibi, Chez, Racket) or R (today's follow-the-name)?** | C deletes caches, shadow bits and `VmClosure.globals`, gives JIT one-load globals; flips 6 pinned tests and changes REPL redefinition behaviour | global-cells §11; R first is safe either way |
| 3 | **When does `define` rebind, and how do mid-program imports interact (p8, p9)?** | Part of the cell commit protocol | global-cells §2 |
| 4 | **Value encoding: keep 61-bit fixnums? adopt float immediates (self-tagging vs NaN-boxing)? raw pointers vs per-heap offsets?** | Freezes the JIT tag ABI; flonums are 19.8% of allocations | §3.1 |
| 5 | **String representation** (UTF-32 with O(1) `string-set!`, or UTF-8/Latin-1 + index) and **bignums** (`num-bigint` foreign payloads vs heap-native limbs) | Inline layout, Drop payloads, primitive rewrite scope (`strings.rs`/`characters.rs` 252 heap mentions) | heap-repr §13 |
| 6 | **Pause goals vs throughput.** Is STW acceptable (today worst 41 ms; post-load 178 ms) or is there a latency target for REPL/embedding users? | Decides incremental marking and the SATB half of the barrier | §3.11 |
| 7 | **Threads: is shared-memory parallelism under SRFI 18 a goal?** | M:1 green threads cost ~0 GC work; N carriers require Mutator/thread split, per-mutator logs, fences, 9–18 engineer-months | threads-rec §5 |
| 8 | **JIT frame model and tiers:** S1 fragments + tail calls, S2 native call/ret, and is an optimizing tier (S3, user stack maps, deopt) planned? | S3 is the only consumer of Cranelift stack maps; S2 needs unwinding and a depth cap | cranelift-gc §6.1 |
| 9 | **Async interrupts in JIT code** (Ctrl-C, timers, sampling profiler, green-thread preemption) | Without them, non-allocating non-calling loops need no polls | cranelift-gc §4 |
| 10 | **Identity hashing:** header hash bits, side table, GC-rehashed native eq-tables, pin-on-hash, or SRFI 254 transport cells for SRFI 69 | Required before any moving; affects pairs (headerless) | §1.6, finalization §7 |
| 11 | **Weakness and finalization scope:** SRFI 254 guardians/transport cells, SRFI 125 weak/ephemeral tables, weak symbol table | Epilogue complexity; R7RS-large eligibility | finalization §3 |
| 12 | **Port finalization semantics:** flush/close at GC (both oracles) and `EMFILE` collect-and-retry; accept that GC timing becomes observable (differential lane must exclude it) | R2/R3 | finalization §2 |
| 13 | **Embedding API:** handle styles (scoped closure, owned handles, both), "call procedure from Rust", "register host primitive", mandatory heap teardown, multiple heaps per process | Public API change (~15 methods, ~200 test call sites); one VA reservation per heap | prim-embed §7, §11 |
| 14 | **Dependencies:** acceptable to depend on MMTk (git-rev pinned, ~30 crates, worker threads) or any new crate in `patina-core`? Approve a throwaway MMTk A/B spike? | Candidate D | immix-mmtk §14 |
| 15 | **Memory footprint goals:** defaults for heap headroom, returning memory to the OS, per-heap reservation size | Sizing policy; embedding | §3.12 |
| 16 | **Sequencing with front-end work:** do lazy scope propagation, inline provenance and identifier ids land before or with the collector? | Expansion is ~80% of load CPU; decides the load-time allocation profile the GC is designed against | libload §9 |
| 17 | **syntax-case timing** | Ends expansion GC-atomicity; needs an expander root provider and literal pool | libload §7 |
| 18 | **Continuation end state:** stop at design A (O(depth), low risk) or go to C′/segments (O(1), medium-high risk)? | Deep-stack capture costs; JIT frame layout | cont-repr §6 |
| 19 | **Accepting semantic divergences that the redesign fixes or introduces** (rebinding, port `eq?`, delimited corner cases) and recording them in `DIVERGENCES.tsv` | Project's oracle discipline | global-cells §8 |
| 20 | **JIT code memory:** W^X policy on macOS, per-function freeing (can `cranelift-jit` free individual functions?), code-cache eviction | Code liveness design | finalization §7, cranelift-gc §6 |
| 21 | **Effort/stage budget:** representation-first (common core, then collector) vs collector-first | Every report says representation dominates | §4.0 |

---

## Appendix A. Contradictions flagged by the critic, resolved

| # | Contradiction | Resolution | How |
|---|---|---|---|
| 1 | **Barrier/remset:** five-plus different barriers across seven reports; "card scanning impossible with headerless pairs" (immix-mmtk) vs Chez card-scans headerless pairs (chez) | Card scanning with headerless pairs is **sound under conditions** — value-only (or single-kind) spaces, headers that decode as immediates, dead-object masking by retained mark bits, nursery never card-scanned — which are heavy representation constraints, not an impossibility. Object/field remembering needs no parsability. On Patina's measured store mix, Chez's SSB yields 0.5–1.0 entries per barrier store (reject); OCaml's ref table lacks dedup (nboyer 24,504 vs 5); JSC's threshold byte is object-granular. **Field logging with an armed bit and an inline call-free slow path** is the resolution; card + young filter is the runner-up if the representation adopts Chez value-only spaces and a copying nursery | barrier-res §1–§7 (Chez `gc.c:2290-2320`, `mkgc.ss:387-388` checked there; Whippet `gc-barrier.h`, `pcc-attrs.h`); store mix measured independently by barrier-res §2 and demographics §6 with consistent results |
| 2 | **Can the allocation slow path collect?** cranelift-gc §6.2 item 4 vs seven reports | **No.** The Rust allocator is shared with ~230–890 functions that hold unrooted values across allocation; making allocation a GC point would also make every JIT allocation site a safepoint (publish/reload, breaks initializing-store barrier elision and SSA-held partial structures). cranelift-gc's statement describes Wasmtime, whose host code uses `Rooted` handles. The JIT slow path refills/overflows and raises the flag; the next poll (entry, back-edge, call return) collects | `note_alloc` only sets the flag (`core:heap/mod.rs:581-584`, re-checked [V*]); `request_gc` likewise (`:609`) |
| 3 | **Cranelift capabilities:** "a call known never to GC needs no stack map" (ocaml-gambit §4.4); "Cranelift cannot pin a register" (ocaml-gambit §4.2) | Both claims are wrong. `user_stack_maps.rs`: "all non-tail call instructions are considered safepoints. (This does *not* allow… skipping safepoints for calls that are statically known not to trigger collections…)". `enable_pinned_reg` exists ("excluded from register allocation… get_pinned_reg/set_pinned_reg"; x21 aarch64, r15 x64 per cranelift-gc §2.7). Consequence: under tier-2 stack maps a barrier slow-path call forces spills; tier 1 (nothing declared) is unaffected. Unverified: whether the pinned register composes with `CallConv::Tail` on aarch64 | read in upstream Cranelift (wasmtime `main`, 2026-09-30): `cranelift/codegen/src/ir/user_stack_maps.rs:6-13` and the `enable_pinned_reg` setting (`meta_settings.rs:113-121` in the fetched copy) [V*] |
| 4 | **Does generational pay off?** | Now measured: bimodal. Nursery copying is 2–25× cheaper than today's marking on 7/20 workloads, neutral on nboyer/mperm, 1.7–1.8× worse on queue3/deeprec; data survival rises to 41–56% once compiler-generated closures/cells disappear; but the nursery's universal win is removing sweep, free lists and fat slots — which a non-generational mark-region also removes. Neither "adopt" nor "assume no" is supported; make it switchable and run M5 | demographics §5, §9.1; barrier-res §8 M5; whippet §1.6 |
| 5 | **Weak-id side tables: more (TW) or fewer (VM)?** | They are about different payloads. VM continuations are plain data → ordinary traced heap objects; delete the VM tables. TW `CpsLambda`/`CpsContinuation` own Rust `Drop` graphs → generalized host-payload table (the existing `trace_weak_ids`/`sweep_weak` seam) with a young-entry list and pinning edges. The O(n²) issue is the ephemeron round fixpoint (289 ms on a 16 k chain), fixed by key-indexed resolution | cont-repr §5; finalization §4.4, §4.6 |
| 6 | **Rust heap API:** `&mut Cx` receivers, no brand (prim-embed) vs branded `Value<'gc>` + shared `&Mutation<'gc>` (rust-gcs) | Both agree on the invariant (allocation never collects; only the driver can collect) and on a context object replacing `Rc<RefCell<Heap>>`. They differ on enforcement. (a) **Interior mutability is required anyway**: JIT code reads and writes `alloc_ptr`, the remset cursor and the event word in VmCtx, so those must be `Cell`/`UnsafeCell` fields; a shared `&Mutation` costs nothing extra. (b) With a stable block heap and no GC in allocation, **allocation cannot invalidate interior references**, so the borrowck benefit of `&mut` across *allocation* disappears; what still needs protection is aliasing an interior slice (`&[char]`, `&[TV]`) while a *mutation* writes the same object — handled by `&mut` receivers for mutation, or by `Cell`-typed slices/copies under a shared context (gc-arena `Lock`). (c) The brand adds compile-time proof that no `Value` survives a collect, at the cost of HRTB primitive types and a "trusted island" VM core (rust-gcs Q1). Resolution [I]: shared context with `Cell`-typed allocator/barrier state; `collect` reachable only from `&mut Heap` the driver owns; brand optional and addable later because primitive signatures already take the context | reasoning over prim-embed §11 and rust-gcs §4.1–4.3; cranelift-gc §2.3 (bump state written by compiled code) |
| 7 | **Number of Drop-carrying variants:** 16 / 19 / 13 | **19** of 28 (my enumeration of `core:heap/mod.rs:143-229` [V*]); 14 of them hold an `Rc` (prim-embed's 13 omits one). heap-repr undercounts | listed in §1.3 |
| 8 | **`vector_slice_mut` call sites:** 8 / 2 / 1 | **1** non-test caller, `vm:runtime/vm_state.rs:2380` [V*]; jit-ready confused it with read-only `vector_slice` (9 uses). Bulk stores that do exist: `vector-fill!`/`vector-copy!` (loops over `vector_set`, `prim:vectors.rs:483,560`) → range entries in any remset. heap-repr §12.7's "bulk mutable slices favour cards" argument is therefore weak | rg over `crates/` |
| 9 | **Is `eq?` already a word compare?** | **No.** `values_eq` (`core:heap/mod.rs:2118-2146` [V*]) returns true for distinct slots wrapping the same `Rc` (Procedure, RecordType, Record); `identity-hash` uses `Rc::as_ptr` for them. Ports also fail `eq?` (finalization E4). immix-mmtk §11's "eq? stays raw-bits" holds only after canonicalization (common core C5) | source read |
| 10 | **Benchmark location** | `~/Project/r7rs-benchmarks` does not exist [V*]; Larceny R7RS and GC suites are local and demographics turned 20 of them into a measured GC set with a reproduction script | `ls`; demographics §2 |
| 11 | **Chez terminology:** "never keeps Scheme values on the native stack" vs "keeps values in native frames described by live masks" | Both describe the same thing: Chez's Scheme stack is heap-allocated segments separate from the C stack, walked via `rp-header` frame size + live mask at each return address, with returns by indirect jump through `sfp[0]`. That is a VM-managed stack, i.e. Patina's `ExecutionState` model (cranelift-gc S1). Neither statement is precedent for keeping Scheme values in Cranelift native frames, which use absolute FP chains and SP-relative maps and cannot be copied for multi-shot capture | chez §12; cranelift-gc §3.1; racket-larceny §0.2 |

Minor inconsistencies also reconciled: allocation-site counts (365 floor / ~430 / 437) differ by counting method;
write-site counts (heap-repr vs offheap) likewise; teardown `strong_count` 36 vs 37 are two probes;
barrier-res did not see demographics (parallel work) but their store-mix results agree (nboyer's edges hit one
boxed variable; hash workloads hit many small entries).

## Appendix B. Source reports

understand/: heap-repr.md, gc-impl.md, vm-runtime.md, offheap.md, tree-walker.md, primitives-embedding.md,
jit-readiness.md. research/: chez.md, racket-larceny.md, ocaml-gambit.md, java-hotspot.md, immix-mmtk.md,
js-engines.md, whippet-misc.md, rust-gcs.md, cranelift-gc.md, threads-prior-art.md, threads-patina-cost.md,
threads-recommendation.md. gaps/: workload-demographics.md, global-binding-cells.md, continuation-representation.md,
barrier-remset-resolution.md, library-load-memory.md, finalization-weak-semantics.md. All under
`PRD/study/gc/`.
Measurement artefacts: `probes/workload-demographics/instrumented/` (demographics) and `probes/*` (probes); see
`probes/README.md`. The fetched third-party sources (Cranelift, Wasmtime, Guile, LuaJIT, Whippet) were not retained.
