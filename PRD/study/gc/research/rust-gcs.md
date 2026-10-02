# Rust-ecosystem GC designs: what Patina should copy

Research date: 2026-09-30. Sources were read from upstream `main`/`master` via raw.githubusercontent.com and the GitHub API, except where a blog or doc URL is given. Downloaded copies were not retained; each source is cited by its upstream project and path. Patina facts were checked against the working tree at `28a94f8`.

Tags: **[V]** means verified in source or in a primary document. **[I]** means my inference. **[M]** means measured in a throwaway crate (`PRD/study/gc/probes/rust-gcs`, release build).

---

## 0. TL;DR

1. **All the fast Rust designs split time into a no-GC window and a GC point, and none collects inside arbitrary Rust code.** They differ in how they enforce the split:
   - *Statically, with a branded lifetime*: gc-arena/piccolo (`'gc` per `mutate`), starlark (`'v`), Nova (`GcScope`/`NoGcScope`), rune-emacs (`&'ob Context` for allocation, `&mut Context` for GC).
   - *Dynamically*: Wasmtime (`enter_no_gc_scope`) and Patina today (`gc_defer_depth` plus safe points at the top of the driver loop).
2. **Patina already pays the hardest cost of the static model.** piccolo had to become "stackless" so that `mutate` could return before a collection. Patina did the same refactor for continuation correctness: since #471–#478 no primitive calls back into Scheme from Rust (`Step::Call`/`Step::Eval`, `crates/patina-primitives/src/registry.rs:33-60`). So the set of code paths that may collect is small: the dispatch loop, the stubs that resume a `Step`, `eval`/library loading, and future JIT slow paths. Nova's warning ("nearly 800 places" of `.bind/.unbind` churn) comes from JS spec operations that can call user code almost anywhere. That warning mostly does not apply to Patina.
3. **The cost Patina pays today is `Rc<RefCell<Heap>>` plus typed `Vec` arenas.** Each `car` is a RefCell borrow, an `Rc` deref, a `Vec` bounds check and an index (`vm_state.rs:2119-2128`, `heap/mod.rs:731`). A non-pair object takes a 72-byte enum slot **[M]**, and vector, string and record payloads are separate mallocs. Every fast design here makes a heap reference a `Copy` word that is dereferenced with one load: gc-arena's `Gc` (`*const T`), starlark's tagged `NonZeroUsize`, and Wasmtime's `VMGcRef` (u32 offset from a heap base held in vmctx).
4. **JIT-facing designs from Wasmtime (and MMTk) to copy:**
   - an inline bump-allocation fast path that reads cursor and limit from a `#[repr(C)]` context;
   - barriers inlined into the JIT as Cranelift IR;
   - Cranelift *user stack maps* (`declare_value_needs_stack_map`), with frames walked by frame pointer plus a PC→stack-map lookup;
   - suspended continuation stacks walked by the same code;
   - host roots held through *indirection* (`Rooted`/`OwnedRooted` are indices into a root table), so a moving collector can update them.
   Steel's Cranelift JIT shows a cheaper first tier: JIT code calls `extern "C-unwind" fn(*mut VmCore)` handlers and keeps every Scheme value in the VM's own stack. No stack maps are needed, because the register file is the root set.
5. **Recommendation:**
   - *Representation*: a `Copy` 8-byte `Value<'gc>` holding a tagged pointer. The heap is owned by the machine, not by an `Rc<RefCell>`.
   - *Context split*: `&Mutation<'gc>` for no-GC work (all ~280 heap primitives); `&mut Heap` (a "GcCtx") for the few may-GC paths. This is rune-emacs's rule, and it costs nothing at runtime.
   - *Allocation*: it never collects. The slow path fetches a new chunk and raises the existing pending flag.
   - *Values across a GC point*: rooted with a LIFO `RootScope` (Wasmtime/V8 style) or carried in `Step` state (piccolo `Sequence` style). Long-lived Rust structures use indirection handles.
   - *JIT*: tier 1 keeps values in the register file; tier 2 adds stack maps.
   - *Testing*: Miri on the collector crate, a gc-zeal torture mode that moves everything, a heap verifier, a Wasmtime-style generator/mutator fuzzer, and compile-fail tests for the brand.

---

## 1. Patina today (the baseline these designs are compared against)

- **Values.** **[V]** `TaggedValue(u64)` is `#[repr(transparent)]` with a low 3-bit tag (`crates/patina-core/src/tagged_value.rs:55-56, 77-84`). Heap references are `HeapIndex = u32` arena indices (`:27-28`). `Option<TaggedValue>` is 16 bytes **[M]**: there is no niche, because fixnum 0 is the all-zero word.
- **Heap storage.**
  - **[V]** `SharedHeap = Rc<RefCell<Heap>>` (`heap/mod.rs:51`).
  - **[V]** `Heap` holds `pairs: Vec<(TV,TV)>`, `vectors: Vec<Vec<TV>>`, `strings: Vec<Vec<char>>` and `objects: Vec<HeapObjectData>` (`:304-316`).
  - **[M]** Slot sizes: pair 16 bytes, vector or string slot 24 bytes plus a separate buffer, `HeapObjectData` 72 bytes, `Environment` 224 bytes.
  - **[V]** `HeapObjectData` carries `Rc` payloads (`Procedure`, `Port`, `Macro`, `Library`, `Continuation`, `Record { fields: Rc<RefCell<Vec<TV>>> }`, `VmClosure { globals: Rc<Environment> }`) (`:143-240`).
- **Collector.**
  - **[V]** Non-moving stop-the-world mark-sweep with side bitmaps. Sweep tombstones each dead slot, which drops its `Rc` payload (`heap/gc.rs:1-27`).
  - **[V]** The `Collector` trait requires a non-moving collector (`gc.rs:200-212`). Roots come from `GcRoots` providers (`gc.rs:177`), with 9 implementors across the backends.
  - **[V]** A safe point is a single flag load. The flag is raised by `note_alloc`/`request_gc` (`mod.rs:581,609`).
  - **[V]** Collection is legal only at the outermost level (`GcDeferGuard`, `gc.rs:232`; `docs/GC_DESIGN.md` §7, lines 506-570). Five sites take guards.
- **Primitive ABI.** **[V]** `TaggedHandler = fn(&SharedHeap, &[TaggedValue])` (`registry.rs:14`), with about 280 `PrimitiveFn::new_heap` registrations (grep). Higher-order primitives use `&dyn ApplyContext` (`apply_context.rs:16`). Resumable primitives use `Step::{Done, Call{state}, Eval{state}}`.
  - Primitives hold values in Rust locals across allocations without rooting. This is sound only because allocation never collects.
- **VM register file.** **[V]** `ExecutionState { registers: Vec<TaggedValue>, frames: Vec<CallFrame>, … }` (`crates/patina-vm/src/runtime/execution_state.rs:16-23`). `CallFrame.closure: Option<HeapIndex>` (`types/mod.rs:50`). The register file is already a heap-allocated shadow stack.
- **Borrow traffic.** **[V, grep]** About 256 `heap(.)borrow_mut()` and 531 `heap(.)borrow()` sites.

**Conclusion [I]:** Patina's rule already *is* gc-arena's "mutation XOR collection". It is enforced dynamically and by convention, and it is paid for with a RefCell on every heap access.

---

## 2. Survey

### 2.1 gc-arena 0.7 (kyren, moulins, Aaron Hill), piccolo and Ruffle

**Core model.**
- **[V]** `Arena<R>::mutate(|mc: &Mutation<'gc>, root| …)`. The `'gc` lifetime is generative and invariant (`types.rs: Invariant<'a> = PhantomData<Cell<&'a ()>>`), so no `Gc<'gc,_>` can escape the callback or move to another arena.
- **[V]** Collection runs only through `&mut Arena` methods: `collect_debt`, `mark_debt`, `finish_cycle`, … (`arena.rs:279-354`).
- The README calls this "Mutation XOR Collection": while `mutate` is running nothing is collected, so no root on the Rust stack can be missed. There is no rooting cost inside a mutation.

**Pointers and objects.**
- **[V]** `Gc<'gc,T>` is a `Copy` newtype around `*const T`. Since 0.7 it points to the `T` itself, and the header sits at a negative offset (CHANGELOG 0.7.0).
- **[V]** Each object carries a 16-byte `GcHeader`: an intrusive `next` pointer for the global `all` list (`gc_ptr.rs:212-222`, `context.rs:167`) and a vtable pointer. The color and flags are stored in the vtable pointer's low bits; `GcVtable` is `#[repr(align(16))]` (`gc_ptr.rs:313-316`).
- **[V]** Each object is a separate global-allocator allocation (`alloc::alloc(alloc_layout)`, `gc_ptr.rs:55`). Sweeping walks the linked list.
- **[I]** That is fine for Lua or Flash object graphs but slow for a Scheme allocation rate: there is no bump allocation, and every death costs a `free`.

**Incremental marking and barriers.**
- **[V]** Incremental tri-color mark-sweep "very similar to … PUC-Rio Lua", paced by "debt".
- **[V]** The backward barrier runs when `phase == Mark && parent is Black && child is White/WhiteWeak`, and turns the parent gray again. The check is inlined; the work is outlined as `#[cold]` (`context.rs:376-398`). `Gc::write(mc, gc)` applies an unrestricted backward barrier and returns `&Write<T>` (`gc.rs:421`). `Lock`/`RefLock` are the barriered cells.
- **[V]** Pacing changed in 0.7 to count `Gc` pointers rather than bytes, "because it is has been too difficult in Rust to … integrate with Rust's other ways of allocating". Defaults: `sleep_factor 0.5, min_sleep 256, mark 0.1, trace 0.4, keep 0.05, drop 0.2, free 0.3` (`metrics.rs:139-147`).

**Soundness rules for unsafe code.**
- **[V]** `unsafe trait Collect<'gc>` with three rules (`collect.rs` header): trace every `Gc`; never touch `Gc`s inside `Drop`; use no interior mutability without barriers.
- **[V]** The derive macro forbids `Drop` through a conflicting blanket impl (`no_drop.rs`: `impl<T: Drop> __MustNotImplDrop for T`). `Cell`/`RefCell` deliberately do not implement `Collect` in a way that could hold a `Gc`. `Collect::NEEDS_TRACE` lets the derive skip leaf types.
- **[V]** CI runs `cargo +nightly miri test --all --tests --all-features -- --skip ui` (`.circleci/config.yml:45-47`), plus UI compile-fail tests.

**Values that outlive a `mutate` call.**
- **[V]** `DynamicRootSet::stash` returns a `DynamicRoot<R>` that may be stored outside the arena. It transmutes `'gc` to `'static` and checks identity at `fetch` time with `Rc`/`Weak` pointer equality on a shared slot table (`dynamic_roots.rs:14-75`).
- **[V]** Ruffle stores one `DynamicRootSet` in its root (`ruffle core/src/player.rs:229`).

**piccolo (the Lua VM built on gc-arena).**
- **[V]** The VM is "stackless" because `mutate` must return before a collection. Callbacks return `CallbackReturn::{Return, Sequence, Call{…then}}` (`callback.rs:16-28`).
- **[V]** Long-running Rust callbacks are `Sequence<'gc>: Collect` objects polled by the `Executor` (`callback.rs:287-298`). Their state is a GC object, so it is traced. `async_sequence` uses a "shadow stack" of stashed values because a Rust future cannot implement `Collect`.
- **[V]** `Fuel` is roughly one unit per VM instruction (`fuel.rs`) and bounds work between returns to the driver.
- **[V]** `Value<'gc>` is a Rust enum (`value.rs:12-22`). **[I]** It is 16 bytes, with no NaN-boxing.
- **[V]** The README admits the VM "sorely needs optimization" and gives no benchmarks; the blog post gives none either (kyju.org/blog/piccolo-a-stackless-lua-interpreter).

**Ruffle (Flash emulator on gc-arena 0.7.0, `Cargo.toml:72`).**
- **[V]** The arena is wrapped in `Rc<RefCell<GcArena>>` (`player.rs:350`), so the RefCell is borrowed once per `mutate`, not once per access.
- **[V]** It collects only after a frame or event update returns (`player.rs:2443`, `self.gc_arena.borrow_mut().collect_debt()`).
- **[I]** Garbage produced inside one long ActionScript call cannot be reclaimed until that call returns. This is the same limitation as Patina's defer rule.

**Relevance.**
- **[I]** Patina's `Step::Call { state }` is piccolo's `Sequence` in miniature.
- **[I]** The brand gives Patina's existing invariant a compile-time proof.
- **[I]** gc-arena itself is the wrong allocator for Patina: malloc per object, a 16-byte header, a linked-list sweep, and no moving.
- **[I]** Adopt the *API discipline* (brand, unsafe derivable trace trait, barriered cells, no-`Drop` rule), not the crate.

### 2.2 boa_gc, and Boa's replacement experiment "oscars"

**boa_gc today.**
- **[V]** A thread-local `BOA_GC` (`core/gc/src/lib.rs:45`). Every object is `Box::into_raw(Box::new(..))` (`lib.rs:138`).
- **[V]** Roots are inferred: `GcHeader { ref_count, non_root_count|mark_bit }` (`internals/gc_header.rs:15-18`). Cloning a `Gc` increments `ref_count`. Before marking, `trace_non_roots` counts the references found inside the heap (`lib.rs:280`). An object is a root iff `non_root_count < ref_count`. This is CPython's cycle-collector trick, used for rooting.
- **[V]** Threshold 1 MiB, then grown so usage stays at or below 70% (`lib.rs:65-70, 188-199`). Miri runs in CI (`.github/workflows/rust.yml:316-348`).
- **[I]** Every `Gc` clone and drop touches a counter in the header, so stack traffic costs refcount-like work, and a moving collector is impossible because roots are unknown.

**oscars (boa-dev/oscars), Boa's testing ground for a new GC.**
- **[V]** `notes/arena2_vs_boa_gc.md` (2026-03-06), comparing a bump-allocated arena with page sweep against boa_gc:
  - allocating 1000 nodes: 27.3 µs vs 56.2 µs;
  - sweeping 1000 objects: 29.5 µs vs 74.9 µs;
  - the "mixed" and "memory pressure" benchmarks: about equal (17.8 µs / 46 µs).
- **[V]** `notes/allocation_overhead_analysis.md` warns about a "double header": the allocator's `next` pointer plus the GC header and vtable. It recommends per-page bitmaps instead.
- **[V]** The API RFC (`notes/api-redesign-prototype/api_redesign_proposal.md`) moves to a gc-arena-style `Gc<'gc,T>: Copy` plus an explicit `Root<'id,T>`:
  - roots are an intrusive list from a pinned sentinel;
  - the `'id` brand comes from `with_gc(for<'id> …)`;
  - the stated motivation: "Every clone touches root counts, adding overhead in hot VM paths. It also needs `thread_local`".
  - Compile-fail tests: `examples/api_prototype/tests/ui/gc_cannot_escape_mutate.rs`, `root_cross_context.rs`.
- **[V]** Their Wasmtime study (`notes/wasm_gc_research/conclusion.md`) concludes:
  - "Precise Roots Are Not Optional", because conservative scanning "would block future moving and generational collectors";
  - "Build the Null Collector First";
  - "Reserve Header Space Early";
  - keep collector traits private behind a public enum;
  - separate root discovery from collection.
- **[V]** oscars runs Miri with `-Zmiri-strict-provenance`.

**Relevance [I].** This is an independent team reaching the same conclusion: implicit rooting by counting is the slow path, and Rust engines are converging on a brand plus explicit roots. Its numbers (about 2× for bump allocation over Box-per-object) are a lower bound on what Patina gains by leaving `Vec<Vec<…>>`.

### 2.3 starlark-rust (Meta, used by Buck2)

**Values and allocation.**
- **[V]** `Value<'v>` is a tagged `NonZeroUsize` raw pointer (`values/layout/pointer.rs` header). Tag patterns: `000` frozen pointer, `001` unfrozen pointer, `010` 32-bit int, `100` frozen string, `101` unfrozen string.
- **[V]** The object header is *one word*: `AValueHeader(&'static AValueVTable)` (`heap/repr.rs:40`). The comment says it "must stay exactly one word wide" (`:161-166`).
- **[V]** Allocation is bump allocation into an `Arena` with two bumps, `non_drop` and `drop` (`heap/arena.rs:179-183`). Only values that need `Drop` go into the arena that must be walked.

**Copying collector.**
- **[V]** A Cheney-like copying GC with forwarding records written over the header (`docs/gc.md`; `repr.rs:69-77,126`).
- **[V]** Collection is `unsafe fn garbage_collect`: "any `Value<'v>` not returned by `Tracer` _will become invalid_" (`heap/unfrozen.rs:288-310`). It is gated by `ban_gc` (default true), and `allow_gc` is documented as "basically impossible to reason about" (`:275-282`).
- **[V]** The evaluator collects *only when executing a statement at the root of the module* (`eval/compiler/stmt.rs:565-590`). The reasons given: values held in Rust locals, native functions that call back (for example `sort` with a key), and iteration freezing. Threshold: `GC_THRESHOLD = 100000` bytes, then the next level is `max(2×allocated, threshold)` (`evaluator.rs:127`, `stmt.rs:588`).

**Branding and frozen heaps.**
- **[V]** `Heap<'v>(&'v OwnedHeap, PhantomData<fn(&'v ()) -> &'v ()>)` is invariant (`unfrozen.rs:106`). Heaps are created only through `for<'v>` closures (`Heap::temp`). `HeapEdge<'v,'dep>` witnesses that one heap keeps another alive (`heap/branding.rs`).
- **[V]** Frozen heaps hold immutable values produced by `freeze`. They are never traced again and are shared across threads through `OwnedFrozen`.

**Relevance [I].**
- Starlark is the cautionary case: a fast copying GC whose *safe-point set collapsed to module top level*, because Rust frames could not be rooted. Patina must not repeat this. Its precondition (all may-GC points sit where the VM state holds every value) is already met by the stackless refactor.
- Worth copying: the one-word header, the drop/non-drop split, and frozen or immortal heaps. Library code constants, symbols and macros would be the frozen part. GC_DESIGN §9.5 measured root-set growth from `code_store` constants; a frozen space removes that tracing.

### 2.4 The refcount family: rust-gc, bacon-rajan-cc, dumpster, Steel, Rhai, RustPython

- **rust-gc (Manishearth).**
  - **[V]** Per the tour post (manishearth.github.io/blog/2021/04/05/a-tour-of-safe-tracing-gc-designs-in-rust), `Gc<T>` keeps a root count, and moving a `Gc` into or out of the heap roots or unroots it. That produces a "fair amount of reference count traffic on any kind of write".
  - **[V]** The same post's taxonomy names the core problems in Rust: root discovery, destructors, interior mutability with write barriers, and "a moment of global mutation".
- **dumpster (Clayton Ramsey).**
  - **[V]** Refcounting plus cycle detection on drop: a DFS compares in-subgraph indegree with the refcount, amortized through a "dumpster" of dirty allocations (claytonwramsey.com/blog/dumpster).
  - **[V, approximate, read off violin plots]** 1M operations, single thread: Rc ~50 ms, bacon-rajan-cc ~60, dumpster unsync ~65-70, rust-gc ~85, shredder ~3700.
- **Steel (Scheme in Rust).**
  - **[V]** `Gc<T>` is a newtype over `Rc`/`Arc`/a biased RC chosen by feature flag (`crates/steel-core/src/gc.rs:44-86, 455`).
  - **[V]** Mutable cells live in a separate `Heap` of `Rc<RefCell<…>>` with weak `HeapRef`s and a mark-and-sweep pass only over them (`values/closed.rs:1623-1629, 1981`; `GC_THRESHOLD 256*1000`, `:457`).
  - **[V]** The Cranelift JIT (cranelift 0.119, features `jit`/`jit2`) dispatches to `extern "C-unwind" fn(*mut VmCore) -> bool` handlers (`steel_vm/vm/jit.rs:208-217`). The handlers read and write `ctx.thread.stack` with `.clone()` (refcount increment) (`:900`).
  - **[I]** The JIT never holds a GC reference in a machine register across a call, so it needs no stack maps.
- **Rhai.** **[V]** "No garbage collection". Captured variables become `Rc<RefCell<Dynamic>>` (`Arc<RwLock>` under `sync`). Rhai "avoids this by clone-copying most data values, so reference loops are hard to create" (rhai.rs/book/language/fn-closure.html).
- **RustPython.** **[V]** Refcounting plus a CPython-style 3-generation cycle collector on intrusive linked lists (`crates/vm/src/gc_state.rs:1-3, 298-300`).
- **Relevance [I].** Patina's environments and closures form cycles constantly, so refcounting needs a cycle collector, and RC traffic on every register move would cancel the JIT's gains. Wasmtime's DRC collector (§2.10) is the only JIT-grade RC design here, and it cannot collect cycles. **Reject the RC family.** Keep one lesson from Steel: a first-tier JIT that leaves values in the VM stack.

### 2.5 safe-gc (Nick Fitzgerald)

- **[V]** `#![forbid(unsafe_code)]` with zero dependencies (fitzgen.com/2024/02/06/safe-gc.html).
  - The heap is a map `TypeId → Arena<T>`. Each arena is a `Vec` with a free list, per-type mark bits and a per-type mark stack.
  - `Gc<T>` holds `(index, heap_id)`, with `assert_eq!(self.id, gc.heap_id)` on access.
  - `Root<T>` holds `Rc<RefCell<FreeList<Gc<T>>>>` and unregisters on `Drop`.
  - A dangling `Gc` is memory-safe: it panics on a free slot or hits the "ABA problem" and reads the wrong object.
  - The author: "not a particularly high-performance garbage collector."
- **[I]** This *is* Patina's current architecture (typed arenas, u32 indices, side mark bits, free lists, tombstones) without the root handles. It confirms that Patina's design is the safe-but-slow corner of the design space. The safety it buys (bounds-checked indices) is the cost Patina pays on every `car`.

### 2.6 Nova (data-oriented JavaScript engine, trynova/nova)

- **[V]** Per `nova_vm/src/heap/README.md`:
  - a heap of per-type `Vec` arenas, with references as "a 32-bit handle and a type";
  - side mark-bit vectors;
  - **compaction by index shifting**: build a list of shifts ("index 30 … shift down 2, … 45 shift down 3") and rewrite every internal reference while walking the vectors.
  - Admitted risks: "An off-by-one error or a missed shift … will cause the JavaScript heap to become corrupted". Vector reallocation on growth may force a switch to "vectors of heap chunks".
- **[V]** Rooting is done with the borrow checker (trynova.dev/blog/garbage-collection-is-contrarian, 2026-01-09):
  - `GcScope<'a,'_>` (may GC) and `NoGcScope`;
  - a handle is `.bind(nogc)`-ed to a scope lifetime, and `gc.reborrow()` for a may-GC call invalidates every unrooted handle;
  - `Scoped<T>` (`scope()`/`get()`) pushes onto a root stack.
  - The cost: "nearly 800 places" of bind/unbind churn, and the author now proposes contravariant lifetimes to reduce it.
- **Relevance [I].**
  - (a) A compacting collector over typed index arenas *is* feasible. Nova does it, so GC_DESIGN §3.4's "moving ruled out" is not a law even without changing to pointers.
  - (b) The `GcScope`/`NoGcScope` split is the right shape, but the may-GC surface must be kept small, or the API tax swamps the code. Patina's surface is small (§0.2).

### 2.7 Brimstone (Hans Halverson's JS engine, "written in very unsafe Rust")

- **[V]** A Cheney semispace collector (`src/js/runtime/gc/garbage_collector.rs:20`). The forwarding pointer replaces the first word (the shape pointer), tagged in its low bit (`:901-905`).
- **[V]** V8-style `Handle<T>` points at a handle slot; `HandleScope` saves and restores the handle-allocation cursor; handles come in 512-entry (4 KB) blocks (`gc/handle.rs:24-27, 124, 249`). "Handles are safe to store on the stack during a GC, since the handle's pointer does not change but the address of the heap item behind the pointer may be updated."
- **[I]** This is the conventional engine design: fast, but the type system does not check it. It is a useful model for a `RootScope` implementation (a bump allocator of root slots).

### 2.8 rune (Emacs Lisp core in Rust, CeleritasCelery/rune)

- **[V]** Allocation goes through `&'ob Context`/`Block` into `bumpalo::Bump` (`src/core/gc/context.rs:44-58, 67`). Objects are `Object<'ob>`, bound to that shared borrow.
- **[V]** `garbage_collect(&mut self, …)` (`context.rs:180`) takes `&mut Context`, so the borrow checker invalidates every unrooted `Object<'ob>`. This is rune's rule as described in coredumped.dev/2022/04/11/implementing-a-safe-garbage-collector-in-rust.
- **[V]** It is now a copying collector: `GcState.to_space`, forwarding pointers, and `self.block.objects = state.to_space` (`context.rs:180-213`; `gc/README.org`). Growth: `next_limit = live*12/10`, `MIN_GC_BYTES = 2000`.
- **[V]** The `root!` macro creates a `__StackRoot` that pushes a `*mut dyn Trace` onto a LIFO root stack and pops it on `Drop` (`root.rs:108-160`).
  - "An owned StackRoot must never be exposed … could result in calling `mem::forget`", so it is macro-only.
  - `Rt<T>` is `PhantomPinned`, so rooted data never moves (`:268-270`).
  - `Slot<T>` is an `UnsafeCell` that the moving GC rewrites (`:284-288`).
  - `__HeapRoot` (`Box<Rt<T>>`) is for roots that outlive a frame (`:163-200`).
- **[V]** Objects that own external memory (hash tables, vectors before promotion) are kept on a `drop_stack` / `lisp_hashtables` list. After copying, unforwarded entries are dropped and forwarded ones are updated (`context.rs:194-208`).
- **Relevance [I].** This is the closest Rust analogue of the design proposed in §4: `&'ob Ctx` to allocate, `&mut Ctx` to collect, LIFO roots that the GC can update, and a needs-drop list because a copying collector never visits dead objects.

### 2.9 Other designs: zerogc, shifgrethor, Josephine, Alloy, sandpit

- **zerogc.** **[V]** `Gc<T>: Copy`, collection only at explicit `safepoint!` calls that move roots through the safepoint and rebrand them ("dark magic"). It is designed for moving collectors; "extremely experimental"; only `zerogc-simple` mark-sweep exists (github.com/DuckLogic/zerogc).
- **shifgrethor and Josephine.** **[V, via the Manishearth tour]** shifgrethor pins roots on the stack frame with `Pin`. Josephine ties roots to `JSContext` borrows for SpiderMonkey.
- **Alloy (Hughes and Tratt, OOPSLA/SPLASH 2025, arXiv 2504.01841).** **[V]** Conservative scanning over BDWGC so that `Gc<T>` is a plain pointer and is `Copy`. It runs Rust destructors as finalizers, which "introduces surprising soundness and performance problems".
- **[I]** Conservative scanning conflicts with Patina's wish for moving or generational collection (MMTk offers only *pinning* roots for that case, §2.11). Reject it as the primary scheme. The finalizer findings support "no `Drop` on GC types".
- **sandpit 0.5.3.** **[V]** A gc-arena-like API with a `gc_yield()` signal for leaving a long mutation, concurrent marking and write-barrier callbacks. "An educational project" (lib.rs/crates/sandpit).

### 2.10 Wasmtime GC (the most JIT-relevant Rust design)

**Reference representation and the heap as a sandbox.**
- **[V]** `VMGcRef(NonZeroU32)` is "not actually a pointer, but a compact index into a Wasm GC heap", or an inline `i31` (`crates/wasmtime/src/runtime/vm/gc/gc_ref.rs:93-120`). It deliberately does **not** implement `Clone`/`Copy`, "to encourage correct usage of GC barriers". Writes go through `GcStore::{clone,write,drop}_gc_ref`, with an `unchecked_copy` escape hatch.
- **[V]** `unsafe trait GcHeap` (`vm/gc/gc_runtime.rs:60-88`):
  - the heap is one contiguous region and indices are bounds-checked;
  - "Every heap is a mini sandbox… native pointers should never be written into or read out from the GC heap". Host data lives in a side table (`ExternRefHostDataTable`), keyed by id;
  - "the downside is … `heap_base + index` computations and bounds checking … deemed to be a worthy trade off", compensated by "32-bit 'pointers'… improved cache utilization".
  - `vmctx_gc_heap_data()` gives JIT code a pointer to collector-specific state; `vmmemory()` gives base and length.
  - `enter_no_gc_scope` panics if a collection is attempted inside.

**Three collectors behind one trait.**
- *null* **[V]**: bump allocation, an OOM error when the space is full, no barriers (`enabled/null.rs`).
- *DRC* **[V]**, deferred reference counting (`enabled/drc.rs:1-45`):
  - host code uses plain RC;
  - JIT code does **no** RC operations on locals or calls. Refs handed to Wasm go into an "over-approximated stack roots" list threaded through object headers;
  - at GC, stack maps give the precise stack set, and the difference is decremented;
  - "cannot collect cycles".
- *copying* **[V]**, Cheney semispace (`enabled/copying.rs:1-11`):
  - "This collector does not require any read or write barriers";
  - the JIT inlines a bump allocation from `VMCopyingHeapData {bump_ptr, active_space_end}`, falling back to a `gc_alloc_raw` libcall (`crates/cranelift/src/func_environ/gc/copying.rs:1-70`);
  - header 16 bytes, `ALIGN = 16`, `MIN_OBJECT_SIZE = 16 + 4` to leave room for the forwarding ref, a copied bit in the header's reserved bits (`crates/environ/src/gc/copying.rs:6-39`);
  - **inline trace info**: a 23-bit bitmap in the header marking which `u32` fields are references, so tracing small structs needs no side-table lookup (`:45-75`).
  - The base `VMGcHeader` is 8 bytes: `VMGcKind` with 26 reserved bits, plus a type index (`crates/environ/src/gc.rs:59-68`).

**Stack roots and JIT frames.**
- **[V]** `trace_wasm_stack_frame` (`runtime/store/gc.rs:752-796`): walk frames by frame pointer; for each PC, `StackMap::lookup(offset, …)`; `stack_map.sp(fp)`; `live_gc_refs(sp)`; each slot becomes a `RawGcRoot::Stack(*mut u32)`, which the copying collector updates in place.
- **[V]** `trace_wasm_continuation_roots` walks *suspended continuation stacks* with the same frame walker and also traces payload buffers (`:838-890`). The order of `trace_roots` is: stack, continuations, vmctx, instances, user roots, pending exception (`:713-750`).
- **[V]** Cranelift *user stack maps* (bytecodealliance.org/articles/new-stack-maps-for-wasmtime, 2024-09-10):
  - the frontend calls `FunctionBuilder::declare_var_needs_stack_map` / `declare_value_needs_stack_map` (`cranelift/frontend/src/frontend.rs:442, 557`);
  - `cranelift-frontend` does a liveness analysis, spills before each safepoint and reloads after;
  - the old regalloc-integrated approach was abandoned because it caused misoptimizations: the mid-end reused a field address computed before a call across a moving GC, writing to "the object's old, vacated location".

**Host rooting API.**
- **[V]** `rooting.rs:1-140`. Goals: safety first, then moving-GC support, then low overhead, then ergonomics. "All of our rooting types below use indirection."
  - `RootScope` + `Rooted<T>` are LIFO and "roughly equivalent to bump allocation", like V8's `HandleScope`.
  - `OwnedRooted<T>` is RAII, allocates an `Arc<()>` per root, and is trimmed lazily, like SpiderMonkey's `PersistentRooted`.
  - Both are "tagged indices into the store's `RootSet`".
  - "`Rooted<T>` can't be statically tied to its context scope via a lifetime parameter… would allow the creation of only one `Rooted<T>` at a time."

**Testing.**
- **[V]** `#[cfg(gc_zeal)]`: an allocation counter forces a GC every N allocations by returning a fake OOM (`vm/gc.rs:71-79, 323-330`). The `POISON` byte is `0b00001111` (`environ/src/gc.rs:30`).
- **[V]** A structured fuzzer: `crates/fuzzing/src/generators/gc_ops/{ops,mutator,types,limits}.rs` (about 270 KB), `fuzz/fuzz_targets/gc_ops.rs`, and the `gc_access` oracle.

**Relevance [I].** This is the template for the JIT seam:
- the context struct with the bump pointer;
- the inline fast path with a libcall slow path;
- barriers emitted as IR by a per-collector "GcCompiler";
- stack maps from Cranelift's frontend;
- a frame walker shared by live and captured stacks;
- indirection roots for host code;
- a null collector first;
- gc-zeal.

### 2.11 MMTk (mmtk-core 0.33): ergonomics for a Rust binding

- **[V]** A binding implements `VMBinding` traits: `ObjectModel`, `Scanning`, `Collection`, `ActivePlan`, `ReferenceGlue`, `Slot` and `MemorySlice` (docs.mmtk.io/portingguide/howto/nogc.html). "We always start a port with NoGC."
- **[V]** `Slot` is the abstraction for tagged, compressed or offset references. `load` strips the tag; `store` "must preserve existing tag bits". NaN-boxed or tagged VMs implement it themselves (docs.rs/mmtk/latest/mmtk/vm/slot/trait.Slot.html).
- **[V]** `RootsWorkFactory` has three root kinds (docs.rs …/trait.RootsWorkFactory.html): ordinary slots that may be updated, *pinning* roots for "conservative stack scanning or VMs unable to update certain root slots", and *transitively pinning* roots.
- **[V]** Fast path: the binding embeds `BumpPointer { cursor, limit }` in its own thread-local state and inlines `cursor + size < limit`. The slow path copies the bump state back and calls `alloc_slow` ("may trigger a GC"). "Real-world fast-path implementations for high-performance VMs are usually JIT-compiled, inlined, and specialized for the current plan and allocation site" (docs.mmtk.io/portingguide/perf_tuning/alloc.html).
- **Relevance [I].**
  - MMTk is VM-neutral, works on raw addresses, and pulls in work-packet threads. That is heavyweight for a single-threaded interpreter, but its interfaces are the right seams.
  - Pinning roots are the escape hatch for any Rust-side value the collector cannot update.
  - `Slot` is how a tagged `Value` participates in moving.
  - Adopting MMTk itself (Immix, StickyImmix, GenImmix) is a possible later backend if Patina's object model satisfies `ObjectModel`: word-aligned `ObjectReference` and a header where forwarding bits can live.

---

## 3. Cross-cutting analysis

### 3.1 How each design avoids `Rc<RefCell<…>>` on every access

| Design | Heap handle | Per-access cost | How exclusive mutation is proven |
|---|---|---|---|
| gc-arena / piccolo | `&Mutation<'gc>` passed in | 1 load (`*const T`) | GC needs `&mut Arena`; mutation of objects goes through `Lock`/`RefLock` plus a barrier |
| starlark | `Heap<'v>` (`Copy`, a `&'v OwnedHeap`) | 1 load | GC is `unsafe` and only at module statements |
| rune-emacs | `&'ob Context` | 1 load | GC takes `&mut Context` |
| Nova | `GcScope`/`NoGcScope` | index into a typed `Vec` | `reborrow()` invalidates handles |
| Wasmtime | `&mut StoreOpaque` / `AutoAssertNoGc` | base + u32 offset (+ bounds check) | no-GC scope counter |
| Ruffle | `Rc<RefCell<Arena>>` at the top | one RefCell borrow per `mutate`, then raw | gc-arena |
| **Patina today** | `Rc<RefCell<Heap>>` everywhere | RefCell check + `Rc` + `Vec` bounds check (+ 2nd indirection for vectors, strings, records) | the defer counter, by convention |

**[I]** Two common moves. First, **pass the heap context explicitly** to functions instead of reaching it through shared ownership. Second, **make object identity a raw word**. Interior mutability of objects is then handled per object (barriered cells), not with a global RefCell.

### 3.2 Making rooting sound in Rust: the techniques

1. **Make collection exclusive**, using a lifetime brand plus `&mut` for collection: gc-arena, rune, starlark's `'v`, Nova.
   - Zero runtime cost.
   - Constraint: a GC point must be a place where all live values are in traced structures.
2. **LIFO root scopes**: rune `root!`, Wasmtime `RootScope`, V8/Brimstone `HandleScope`, Nova `Scoped`.
   - Cost is a bump allocation of a root slot.
   - Soundness needs either macro-only construction (rune: `mem::forget` would break LIFO) or an owned scope that checks at runtime.
3. **Indirection handles in a root table**: Wasmtime `OwnedRooted`, gc-arena `DynamicRoot`, safe-gc `Root`, oscars `Root<'id,T>`.
   - The GC updates the table slot, so this works with moving collectors.
   - Cost: one extra hop, plus allocation or trimming bookkeeping.
4. **Infer roots by counting**: boa_gc. Refcount traffic everywhere, and no moving. Rejected by Boa's own successor.
5. **Conservative scanning**: Alloy, and MMTk pinning roots. Blocks moving for anything a stack word might reference.
6. **Precise stack maps for JIT frames**: Wasmtime with Cranelift user stack maps.

**[I]** Patina should combine (1), (2) and (3), plus (6) once JIT tier 2 exists. It should never use (4) or (5).

### 3.3 Exposing objects to JIT and native code

- **Wasmtime** **[V]**: vmctx → GC heap base and bound, plus a collector-specific heap-data struct (bump pointer). The JIT emits the allocation fast path, barriers and stack-map declarations through a per-collector "GcCompiler" (`crates/cranelift/src/func_environ/gc/{null,drc,copying}.rs`).
- **MMTk** **[V]**: `BumpPointer {cursor, limit}` in binding-owned thread-local state; per-plan barrier code chosen when code is generated.
- **Steel** **[V]**: the JIT calls Rust handlers with `*mut VmCore`; all values stay in the VM stack.
- **gc-arena and starlark**: no JIT. **[I]** Their raw-pointer `Gc`/`Value` would be JIT-friendly, but gc-arena's barrier checks the color in the vtable pointer's low bits plus a phase field. That is expressible in IR, but awkward.

### 3.4 Unsafe-code discipline worth copying

- One `unsafe trait Trace` that is safely derivable, with no `Drop` on traced types and with plain `Cell`/`RefCell` unable to hold references (gc-arena).
- Raw references that are deliberately not `Copy` or `Clone`, so barriers cannot be skipped by accident (Wasmtime `VMGcRef`). **[I]** For Patina, keep `Value` `Copy` for speed, but make *object fields* writable only through barriered accessors.
- No native pointers inside the GC heap; Rust-owned payloads go in side tables keyed by id (Wasmtime). Or use split arenas, so objects that need `Drop` are enumerable (starlark `drop`/`non_drop`, rune `drop_stack`).
- An invariant to keep: every brand is introduced only by a `for<'x>` closure, and no constructor builds a handle from a plain borrow (starlark `branding.rs`).

### 3.5 Testing strategies seen

| Strategy | Who | Notes |
|---|---|---|
| Miri on the whole test suite | gc-arena (CI), Boa (`cargo miri test … miri`), oscars (`-Zmiri-strict-provenance`) | Needs a GC core testable without a JIT |
| Compile-fail UI tests for the brand | gc-arena (`--skip ui` under Miri, run normally), oscars `tests/ui/*.rs`, starlark doc examples | Lock in that `Value<'gc>` cannot escape or cross heaps |
| GC torture (forced GC every N allocations) | Wasmtime `cfg(gc_zeal)` | Plus poisoning freed memory (`POISON`) |
| Structured fuzzing with an oracle | Wasmtime `gc_ops` generator, mutator and oracle | Random op sequences over refs, tables and globals |
| Null collector first | Wasmtime, MMTk, oscars | Separates object-model bugs from collector bugs |
| Differential and poison lanes | Patina already (`run_gc_differential.sh`, debug poison) | Keep these |

---

## 4. What a Rust-idiomatic but fast design for Patina looks like

Everything in this section is a proposal **[I]**, grounded in the precedents cited.

### 4.1 Principle: allocation never collects, and only `&mut Heap` may collect

- Keep Patina's existing semantics: GC only at machine safe points where the root set is complete. Make them a *type-level* fact instead of a counter plus convention.
- The heap is owned by the machine (`VmState`, the tree-walker driver), not shared through `Rc<RefCell>`. `GcDeferGuard` and its depth counter disappear. "Not outermost" becomes "you only hold `&Mutation`, so you cannot call `collect`".
- Lifetime split, copying rune and gc-arena:

```rust
#[derive(Copy, Clone)] #[repr(transparent)]
pub struct Value<'gc>(u64, Invariant<'gc>);        // tagged word; heap refs are tagged pointers (or offsets, see 4.6)

pub struct Heap { /* nursery chunks, old space, root table, pending flag, … */ }

#[repr(transparent)]
pub struct Mutation<'gc> { heap: HeapCell, _brand: Invariant<'gc> } // no collect() method

impl Heap {
    // may-GC context: requires &mut, so no Value<'gc> obtained from an earlier
    // mutation window can still be alive (rune's `garbage_collect(&mut self)`)
    pub fn collect_if_pending(&mut self, roots: &mut dyn Roots) { … }
    pub fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&Mutation<'gc>) -> R) -> R { … }
}
impl<'gc> Mutation<'gc> {
    #[inline] pub fn cons(&self, a: Value<'gc>, d: Value<'gc>) -> Value<'gc>;  // bump; never collects
    #[inline] pub fn car(&self, p: Pair<'gc>) -> Value<'gc>;                    // one load
    #[inline] pub fn set_car(&self, p: Pair<'gc>, v: Value<'gc>);               // store + barrier
}
```

**Why this fits Patina better than it fits Nova:**
- After #471–#478, primitives are leaf code. ~280 `new_heap` handlers, the Scheme-written higher-order procedures, and resumable primitives that hand calls to the machine (`registry.rs:20-60`) never need a may-GC context.
- The may-GC surface is the dispatch loop, the stubs that resume a `Step`, `eval`/`load`/library loading, and JIT runtime stubs: a few dozen functions, not 800.

### 4.2 Primitive ABI, and holding values across allocations

```rust
type Prim = for<'gc> fn(mc: &Mutation<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, Error<'gc>>;
type Resume = for<'gc> fn(mc: &Mutation<'gc>, state: Value<'gc>, result: Value<'gc>) -> Result<Step<'gc>, Error<'gc>>;
```

Inside a primitive:
- Any number of allocations is fine, because allocation never collects (gc-arena "debt", Patina's pending flag).
- Values are plain `Copy` locals.
- **[I]** Heap overshoot is bounded by the garbage one primitive generates. That is small for leaf primitives; the large cases are single allocations that are live anyway (`make-vector`, `list-copy`).

To keep a value across a point that may collect, there are three tools:
1. **`Step` state.** Already exists. It is piccolo's `Sequence`; the machine traces `state`.
2. **`RootScope` / `root!`** for may-GC Rust code such as library loading, `eval` and JIT stubs. Use a LIFO root-slot stack (Brimstone's 512-slot blocks; Wasmtime "roughly equivalent to bump allocation"). The GC updates the slots when it moves objects.
   - `Rooted` is not lifetime-tied to the scope (Wasmtime's argument). The scope checks LIFO order at runtime in debug builds.
   - Alternatively, make it macro-only like rune's.
3. **`Root` / `OwnedRoot`** for long-lived Rust structures: library registry, REPL state, `SourceMap` keys. Use an indirection slot in a root table, released on drop or trimmed lazily (Wasmtime `OwnedRooted`, gc-arena `DynamicRoot`).

Recommended use of the "free to change representations" licence:
- **Move hot Rust-side holders into the heap**: environments and globals cells, closures' globals, `MutableCell`, records, promises, parameters, compiled-macro literals. They then need no roots at all.
- Keep `Root` for the cold remainder.
- This removes most of GC_DESIGN §3.4's obstacles to moving. The symbol table and `CallFrame.closure` become ordinary traced slots, `eq?` hashing gets a header hash (another agent covers this), and `SourceMap` raw-bits keys become weak roots or header-carried ids.

### 4.3 Allocation fast path, and what the JIT sees

- One `#[repr(C)] struct VmCtx { alloc_cursor, alloc_limit, gc_pending: u8, card_table_base, regs_base, … }`, owned by the machine. Rust's `#[inline] alloc` and the JIT's inline IR read the same fields (Wasmtime `VMCopyingHeapData`, MMTk `BumpPointer`).
- Slow path (`#[cold]`): take a new nursery chunk and raise `gc_pending`; **do not collect**. The next safe point (a loop back-edge, a call, a return, or the poll emitted by the JIT) sees the flag and enters the may-GC context.
- **[I]** This makes the slow path legal from inside a `&Mutation` context. Under the brand rule, a slow path that collected would be unsound.
- Objects with Rust destructors (ports, bignums if they stay as `num_bigint`, `Library`) cannot be reclaimed silently by a copying nursery, because it never visits dead objects. Either:
  - allocate them in a non-moving "needs-drop" space that is swept, as starlark's `drop` bump and rune's `drop_stack` do; or
  - (preferred) keep Rust-owned payloads in side tables keyed by id (Wasmtime's `ExternRefHostDataTable`) and keep the heap free of native pointers.

### 4.4 Barriers

- Every heap store goes through `Mutation` accessors that carry the barrier (gc-arena `Gc::write`/`Lock`; Wasmtime `write_gc_ref`).
- Do not expose `&mut` to object fields. Expose `get`/`set` (and `Cell`-like access for vectors).
- For a generational collector, the barrier is a card mark or a remembered-set log, emitted identically by the JIT as Cranelift IR. Keep the check-then-`#[cold]`-call shape gc-arena uses.
- The collector chooses which barrier the JIT emits (Wasmtime's per-collector `GcCompiler`). This allows a null collector first, then semispace or Immix nursery, then a generational old space.

### 4.5 JIT tiers and roots

**Tier 1 (Steel-style, no stack maps).**
- Compiled code reads and writes Scheme values in the existing `ExecutionState.registers` (`execution_state.rs:18`) through `regs_base` in `VmCtx`.
- Calls to runtime stubs are `extern "C-unwind" fn(*mut VmCtx, …)`.
- Any Cranelift value holding a reference is dead across a stub call, or reloaded from the register file after it.
- GC inside a stub is safe because the root set is exactly the register file plus the frame stack, as today.
- **[I]** Continuation capture is unchanged, because frames still live in `frames: Vec<CallFrame>`.
- Caveat: the `Vec` may reallocate on growth, so `regs_base` must be reloaded after any stub, the same rule as Wasmtime's memory base.

**Tier 2 (Wasmtime-style).**
- Keep references in machine registers and declare them with `declare_value_needs_stack_map`.
- At a GC, walk frames by frame pointer, look up the stack map by return PC, and update slots in place (`store/gc.rs:752-796`).
- Treat every safepoint as clobbering derived addresses (the misoptimization in the stack-maps article).
- Use the same walker for suspended or captured native stacks (`trace_wasm_continuation_roots`).
- **[I]** Patina will probably still prefer to *materialize* JIT frames into `CallFrame`s when `call/cc` captures them (deopt on capture), rather than copying native stacks.

**Constants embedded in machine code.**
- Either load them through the code object's constant vector (one indirection, movable), or allocate literals and symbols in a non-moving frozen or immortal space whose addresses may be immediates (starlark frozen heaps; **[I]** like Chez's static generation).

### 4.6 Representation: tagged pointers vs u32 offsets

| | Tagged raw pointer in the 64-bit word (starlark, gc-arena, Brimstone) | u32 offset from a single heap base (Wasmtime, V8 cage) |
|---|---|---|
| Dereference | 1 load (tag folded into the displacement) | base register + offset (+ bounds check) |
| Value size | 8 bytes either way | 8 bytes (a NaN-box or 61-bit fixnum still needs 64 bits) |
| Object fields | 8 bytes per reference | can be 4 bytes, but only if fields are not general `Value`s |
| Heap growth | chunks anywhere | needs one reserved virtual range (no realloc) |
| Sandbox property | no | yes (not a Patina goal) |
| Moving | fine with precise roots | fine |

**[I]** Scheme fields hold full `Value`s (fixnums, flonums). Compressed 4-byte fields therefore buy little, while costing the base addition everywhere. Prefer tagged pointers.

Patina's current *typed-index* scheme is a third option, and the worst for a JIT: a different base per type, a `Vec` that reallocates, and a second indirection for vectors and strings.

**[V]** Nova proves typed index arenas can be compacted. **[I]** But that does not fix the JIT cost.

### 4.7 Testing plan, copying the survey

- Put the collector in its own crate, with **no JIT dependency** so Miri can run it. Run `cargo miri test` with `-Zmiri-strict-provenance` on a small-heap test suite (gc-arena, Boa and oscars all do this). Miri cannot execute Cranelift code, so keep a Rust reference implementation of every barrier and allocation path the JIT inlines, and test the JIT against it differentially.
- **gc-zeal** modes:
  - collect at every safe point;
  - collect every N allocations;
  - "move every object every GC";
  - poison the evacuated space. **[I]** In debug builds, also `mprotect` it so stale pointers fault.
  Add a **heap verifier** after each GC: every reference points to a valid object start in to-space or old space, and old→young references appear in the remembered set or card table.
- **Compile-fail tests** with trybuild: `Value<'gc>` cannot escape `mutate`, cannot cross heaps, and no `collect` while a `Value` is alive (oscars `tests/ui`).
- **Fuzzing**: a generator and mutator of Scheme programs over `cons`/`vector`/`set-car!`/`call/cc`/`dynamic-wind`/ephemerons, with gc-zeal on, compared against a run with GC disabled (Wasmtime `gc_ops` + oracle).
- Keep Patina's existing GC differential lanes and the chibi/Gauche oracles as the semantic ground truth.

### 4.8 Migration order (sketch)

1. Introduce `Mutation<'gc>` and `Value<'gc>` as zero-cost wrappers over today's `Heap`/`TaggedValue`. Convert the ~280 primitives mechanically: `&SharedHeap` becomes `&Mutation`. This removes per-access `RefCell` traffic before any collector changes.
2. Move `Rc`-held value structures into the heap (environments, records, cells), so roots shrink to the VM state plus a few `Root`s.
3. Change object layout: a one-word header and inline payloads. Add a null collector, then a copying nursery over a non-moving old space. Run gc-zeal "move everything" across the chibi, Gauche and Larceny lanes.
4. JIT tier 1 against `VmCtx`, then tier 2 with stack maps.

---

## 5. Lessons and constraints for the redesign (summary)

- Preserve **"allocation never collects; only `&mut Heap` may collect."** It is free, and it lets the hundreds of primitives stay simple. Every Rust GC that let allocation collect either needed pervasive rooting (Nova: 800 sites; rust-gc and boa: refcount traffic) or retreated to rare safe points (starlark: module top level only).
- Keep **the may-GC surface small**, and keep the stackless primitive discipline (#471–#478). It is what makes the brand cheap.
- **No native pointers or `Drop` payloads in the movable heap.** Use side tables or a needs-drop list (Wasmtime, starlark, rune).
- **Roots outside the machine use indirection** (Wasmtime's rationale), so moving stays possible. LIFO scopes cover transient roots.
- **Barriers sit behind accessors, the JIT emits the same barrier, and the collector chooses which barrier is emitted** (per-collector compiler).
- **Do null first, keep a gc-zeal mode, run Miri on the core, and fuzz with an oracle.**
- **For JIT tier 1, the register file is the root set** (Steel). Use Cranelift user stack maps only for tier 2. After every safepoint, reload references and the base pointer.

## 6. Open questions

1. Can the VM's register file and frames live *inside* the branded heap world (gc-arena's "root" pattern) without a rebrand per dispatch? Or should the VM core stay a trusted `unsafe` island with raw words, as Wasmtime's VM does with `VMGcRef`? This report leans toward the trusted island, with the brand enforced at the primitive and library API.
2. The tree-walker's CPS continuations (`Rc<CpsContinuation>`) are an `Rc` graph. Do they move into the heap, or is the tree-walker frozen at "non-moving old space only"?
3. `eq?` hashing and `SourceMap` keys use raw bits today. Moving requires header hash codes or rehash-on-GC (Chez/Racket practice; see the other reports).
4. Is a Wasmtime-style contiguous reserved heap region (needed for u32 offsets or simple card tables) acceptable on all CI targets (macOS and Linux runners)? Does it conflict with running multiple interpreter instances in one test binary (`heap/mod.rs` notes both backends coexist)?
5. Incremental vs generational. gc-arena's incremental Lua-style collector optimizes pauses. For Scheme allocation rates, a generational copying nursery is probably the bigger win. The PRD (GC_DESIGN §10, line 787) planned non-moving sticky-mark-bit generations, which should be re-evaluated against a copying nursery now that moving is allowed.
6. How much of `num_bigint`/`BigRational` should become heap-native bignums so they need no `Drop`?
