# Library loading and expansion memory under a new collector (gap: library-load-memory)

Revision: `main` at `28a94f8`, clean tree. Machine: macOS 27.2 arm64, rustc 1.97.1 (pinned), release build with `debug = 1`.
Tags: **[V]** means verified by source reading or by a measurement made for this report. **[D]** means quoted from another report. **[I]** means inference.
Units: MiB (2^20) unless written "MB". RSS figures from `ps`/`getrusage` are converted to MiB.

Probes, retained under `PRD/study/gc/probes/lib-load-mem/`:
- `src/prof.rs` is a sampling heap profiler installed as the global allocator. It counts every malloc, free and realloc. It keeps live bytes per power-of-two size class. For one allocation per 32 KiB (256 B for small runs) it samples the return-address stack with libSystem `backtrace()`, and it keeps live samples in a map so that live memory can be attributed at any moment, including at the peak.
- `sym.py` / `classify.py` / `drill.py` symbolize with `atos -i` (inlined frames) and Xcode `llvm-cxxfilt` (v0 demangling). They classify each stack by its innermost Patina frame.
- `src/main.rs` modes:
  - phase profile: boot → load → `(gc)`;
  - `CENSUS=1`: arena census by `HeapObjectData` variant, identifier scope-set statistics and provenance coverage;
  - `FORMS=1`: heap allocations per top-level form;
  - `SIZES=1`: type sizes.
- `incl.py` computes inclusive CPU shares from a `sample(1)` call tree.
- Programs: `progs/big.scm` is the 26-library import from `biglibs.scm` (import only); `p3.scm` is `(scheme base) (srfi 1) (scheme hash-table)`; `rb-*.scm` and `s146-*` are the per-form experiments.

---

## 0. Summary

1. **Loading memory is almost all garbage, and almost all of it is either in the GC arenas or tied to them.** For the 26-library load on the VM:
   - malloc-live peaks at **765 MiB**; RSS is 627 MiB (658 MB). **[V]**
   - 416 MiB is arena capacity (288 MiB objects, 128 MiB pairs), of which about 268 MiB is touched slots.
   - **316 MiB is syntax provenance**: a 136 MiB `syntax_sources` hash table, 166 MiB of `Rc<SyntaxSource>` entries and child spans, and 15 MiB of expansion chains.
   - Everything else is about 33 MiB.
   - After the load only **15.8k pairs and 5.1k objects survive** of 4.62M and 2.89M; only 30 of 2.19M identifiers survive. **[V]**
   - The "≈380 MB unexplained" in gc-impl is the provenance table plus its entries. **[V]**
2. **Syntax provenance is the largest Rust-side cost, and it exists only because of how syntax is copied.**
   - Each entry costs 96 B (`Rc<SyntaxSource>`: 80 B + 16 B header) plus 17–34 B of hash table, more than the 16–72 B node it describes.
   - It is pruned by sweep, so it is "GC-coupled": a collector that runs during loading frees it, and one that does not keeps all of it.
   - Two of its three stores have **no production reader**: `SourceMap.locations` and `Heap::child_source` are read only in tests (`rg`, §5.1). **[V]**
3. **The tree-walker adds about 390 MiB of GC-coupled Rust garbage**: call environments' scoped-binding tables (163 MiB), call frames and steps (140 MiB) and closure payloads (87 MiB). These are `Rc`s owned by 468k dead `Procedure` objects, so they are freed only when sweep tombstones those objects. TW peak: **1,190 MiB malloc-live, 1,058 MiB max RSS**. **[V]**
4. **Load CPU is expansion, not GC or execution.** `desugar_with_imports` takes 82% of samples:
   - eager scope-edit copying (`map_syntax_identifiers` from `edit_scope_on_tagged`): **44%**
   - provenance maintenance: 20%
   - `stamp_expansion_source`: 12%
   - the single post-load collection: 13% (≈200 ms: it sweeps 7.5M slots and drops their `Rc` payloads)
   - VM compile: 2.5%; parsing: 2.6%

   **[V]** (1,453 1-ms samples; categories overlap)
5. **Retention after the load is a high-water artifact.** Of 501 MiB malloc-live after the load and its collection, 464 MiB is empty arena capacity plus free lists. Truly live data is ≈35 MiB on the VM (code objects 21 MiB) and ≈60 MiB on the TW (CPS trees 44 MiB). RSS does not drop at all after the first collection (627 → 627 MiB). **[V]**
6. **What a new collector can address** (§3):
   - in principle about 96% of the load peak;
   - in practice only once loading has safe points. With collection between top-level forms only, the floor is the largest single form. In this set that is one `tree-match` form in `(nieper rbtree)`: 746k heap allocations, ≈10% of the whole load.
   - Measured on rbtree, between-form collection cuts the malloc peak **211 → 118 MiB**. **[V]**
   - Expansion CPU (≈80% of load time) is not GC-addressable. Representation is.
7. **Rooting (§4).** Between libraries and between body forms, every Rust-held `TaggedValue` sits in a strictly LIFO structure: the program's import form, the import-set `Vec`, `ParsedLibrary.body`, `with_globals`' saved environment, and suspended trampolines on the TW. **A Wasmtime-style LIFO `RootScope` suffices there.** Inside `desugar_with_imports`, after an import returns mid-form, the live set includes partial `CoreExpr` trees holding literal TVs and raw-bits memos (`quoted`, `OpenNodes`). A `RootScope` alone is not enough at that point; it needs literal pools and memo invalidation, or the import must stay deferred.
8. **Identifiers (§6).**
   - 2.19M identifiers:
     - scope-set length 0/1/2/3/≥4 = 30.9/55.2/12.0/1.3/0.55%; only 12k spill the `SmallVec`;
     - **64,782 distinct scope sets** (34 identifiers per set);
     - **356,820 distinct (name, scope-set) pairs** (16.3%);
     - only 73,023 (3.3%) were written in source. **[V]**
   - Estimated identifier arena bytes:
     - interned scope-set ids with a 40 B enum: −44%;
     - a 16 B header layout: −78%;
     - hash-consing: −96%.
   - The larger lever is to stop copying, i.e. lazy scope propagation as in Racket.
9. **Provenance under moving (§5).** Recommend option (a): an inline source id in identifiers, with spans only for reader-produced pairs, and delete the two unread stores. Option (b), weak id-keyed tables, keeps the 316 MiB and adds per-move maintenance. Option (c), a non-moving syntax space, fights the nursery for exactly the allocation that dominates loading.
10. **syntax-case (§7)** ends whole-form atomicity. The expander needs:
    - a registered root provider;
    - `CoreExpr` literals in a rooted pool;
    - memos keyed by stable ids or invalidated by a GC epoch;
    - a continuation barrier around transformer calls.

    An easy scheme that makes this tractable is a non-moving (or evacuation-suspended) mature space, with a minor GC at transformer entry.

---

## 1. Totals by phase, both backends [V]

| | VM, GC on | VM, GC off | TW, GC on | TW, GC off |
|---|---|---|---|---|
| Boot (`Interpreter::new_*`) | 7–10 ms, 4.1 MiB live | same | 7–10 ms, 4.4 MiB | same |
| Load 26 libs, wall | 1.92 s | 1.68 s | 3.70 s | 3.54 s |
| Allocated during load (malloc) | 3,204 MiB / 13.97M calls | 3,098 MiB | 7,448 MiB / 42.4M calls | 7,345 MiB |
| malloc-live peak | 771.5 MiB | 770.6 | 1,188.9 | 1,184.6 |
| malloc-live at end of load | 501.1 | 770.6 | 525.6 | 1,184.6 |
| Max RSS (`/usr/bin/time -l`) | 627 MiB (658 MB) | 626 MiB | 1,058 MiB | 1,058 MiB |
| `(gc)` after load: time, freed | 14.8 ms, 0.5 MiB | **201.5 ms, 269.9 MiB** | 21.5 ms | **276 ms, 659.5 MiB** |

- With GC on, the one collection happens inside the load: at the outermost safe point when the program's import form finally executes, after all expansion.
- Peak is unchanged, which confirms gc-impl's 662 vs 659 MB.
- That first collection costs about 200 ms, ≈10% of load time.

---

## 2. Memory breakdown

### 2.1 GC arenas by kind, after the load, GC off (census) [V]

| Arena / variant | VM count | TW count | Slot bytes | Live after `(gc)` (VM / TW) |
|---|---|---|---|---|
| pairs | 4,617,086 | 4,617,086 | 70.5 MiB (16 B) | 15,818 / 15,725 |
| objects, all | 2,885,727 | 2,747,530 | 198.1 / 188.7 MiB (72 B) | 5,052 / 5,039 |
| — `Identifier` | **2,189,740** | 2,189,740 | 150.4 MiB | 30 |
| — `VmClosure` / `Procedure` | 468,183 | 468,541 | 32.1 MiB | 2,214 / 2,594 |
| — `MutableCell` | 138,197 | 0 | 9.5 MiB | 46 |
| — `Real` (flonum box) | 84,296 | 84,296 | 5.8 MiB | 600 |
| — `Symbol`, `Rational`, `Record`, `RecordType`, `Values`, `Macro`, … | < 4k total | | < 0.3 MiB | |
| vectors | 983 slots, 26,713 elements | same | < 0.3 MiB | 160 |
| strings | 746 slots, 7,774 chars | same | ≈0 | 671 |

Arena capacity, from the profiler's largest blocks:
- objects: 4,194,304 slots × 72 B = **288 MiB**;
- pairs: 8,388,608 × 16 B = **128 MiB**;
- after the first sweep the free lists add **32 + 16 MiB** of `Vec<u32>`.

`Identifier` objects are 76% of objects in this set. heap-repr measured 99% for a 3-library load **[D]**. The rest are runtime products of library bodies that execute at load time. Example: `(scheme flonum)` alone makes 391k closures and 126k cells (§2.4).

### 2.2 Rust and malloc memory by owner: peak, retained, churn [V]

The profiler classifies every sampled live block by its innermost Patina frame. "Peak" is the snapshot taken within 2% of the malloc-live maximum. "Retained" is after the load and `(gc)`. "Churn" is cumulative bytes allocated.

| Owner | VM peak | VM retained | VM churn | TW peak | TW retained | TW churn |
|---|---|---|---|---|---|---|
| A1 object arena (`Vec<HeapObjectData>` growth) | 288.0 | 288.0 | 576 | 288.0 | 288.0 | 576 |
| A2 pair arena | 128.0 | 128.0 | 357 | 128.0 | 128.0 | 354 |
| A4 free lists (`sweep_arena`) | 0 | 48.0 | 100 | 0 | 48.0 | 100 |
| P1 `Heap.syntax_sources` hash table | **136.0** | ≈0 | 272 | **136.0** | ≈0 | 272 |
| P2 `Rc<SyntaxSource>` entries + child spans | **165.7** | 1.3 | 167 | **165.1** | 1.4 | 165 |
| P3 expansion chains (`Arc<[String]>`, `stamp_expansion_source`) | 14.6 | 6.6 | 88 | 15.5 | 7.0 | 91 |
| P4 `SourceMap` records + `SourceDocument` text | 1.9 | 1.7 | **355** | 1.9 | 1.8 | 353 |
| C1–C5 `CodeObject`: instructions / `source_map` / `register_roots` / `global_cache` / store and constants | 7.2 / 6.7 / 4.8 / 1.1 / 1.8 | 7.1 / 6.7 / 4.6 / 1.1 / 1.8 | 37 | — | — | — |
| C6 VM compiler passes (transient IR) | <0.5 | 0.6 | 95 | — | — | — |
| T1 `CpsExpr` trees (patina-ir) | — | — | — | 45.3 | **43.9** | 68 |
| T2 TW closures (`Procedure` payload) | — | — | — | 87.3 | 0.6 | 87 |
| T3 TW runtime (call envs, steps, `ContEnv`) | 1.8 | 0.7 | 8 | **139.7** | ≈0 | 2,541 + 817 |
| E1 `Environment`/`Library` binding tables | 2.6 | 2.7 | 20 | 14.1 | 3.0 | 89 |
| E2 `Environment::insert_scoped` | ≈0 | ≈0 | 3 | **163.4** | ≈0 | 820 |
| F1–F3 reader, desugarer (`CoreExpr`, `quoted`, `EarlyBinding`), other front end | <0.5 | <0.5 | 39 + 60 + 45 | <0.5 | 0.4 | 145 |
| M1 `CompiledMacro` | 0.7 | 0.7 | 1 | 0.8 | 0.8 | 1 |
| M2 scope-edit temporaries (`map_syntax_identifiers_memo`: `seen`, `parents`, children `Vec`s, `replacements`) + matcher | <0.5 | 0 | **950** | <0.5 | 0 | 950 |
| I1 identifier names `Rc<str>` (parser) | 2.5 | 0.7 | 3 | 1.8 | 1.3 | 2 |
| S1 `ScopeSet` spills | 0.8 | <0.3 | 8 | 1.9 | 0.3 | 10 |
| **Total sampled** | **765** | **501** | **3,222** | **1,190** | **526** | **7,474** |

Notes on the owners the task named:
- **ParsedLibrary.body** is a `Vec<TaggedValue>` with at most a few hundred entries, so it is negligible as Rust memory. The forms it points to are reader-built pairs and identifiers in the arenas: about 73k written identifiers. **[V]**
- **CodeObject:**
  - `Instruction` is 48 B.
  - `register_roots: Vec<Vec<u64>>` costs about 40 B per PC (24 B header plus a 16 B malloc), i.e. 4.6 MiB beside 7.1 MiB of instructions. That is 65% of the instruction stream for a map the dispatch loop never reads (`code_object.rs:152-158`).
  - The per-PC `source_map: Vec<(usize, SourceLocation)>` is 72 B per entry.
- **Desugarer memo tables:** the per-form `quoted` map is small. The big transient is the scope-edit memo inside `Heap::map_syntax_identifiers_memo` (`heap/source.rs:100-136`): 950 MiB of churn, std `HashMap`s with SipHash keyed by raw bits.
- **Throwaway `SourceMap`s.** A library body's desugarer has no source map. `desugar_list_tagged` therefore creates a fresh `SourceMap` for every expansion (`desugarer/mod.rs:2136-2149`) and `stamp_expansion_source` fills it, then drops it. That is the 355 MiB of P4 churn.

### 2.3 Live vs transient [V]

| Category | Fate | VM bytes |
|---|---|---|
| Arena slots that die by end of load | dead, but held to the end by deferral | ≈268 MiB touched (99.7% of slots) |
| Provenance entries keyed by those slots | dead, freed only when sweep prunes | 316 MiB at peak |
| TW `Rc` envs/closures/steps owned by dead `Procedure`s | dead, freed only by tombstoning | ≈390 MiB at peak (TW only) |
| Rust temporaries (scope-edit memos, throwaway `SourceMap`s, IR, `CoreExpr`) | freed per call or form | ~1.5 GiB churn, < 20 MiB at any instant |
| Code objects, envs, macros, documents, chains, CPS trees | live | VM ≈35 MiB, TW ≈60 MiB |
| Empty arena capacity + free lists after the load | retained forever (no shrink) | 464 MiB |

### 2.4 Per-library cost, VM, GC off, each library with `(scheme base)` [V]

| Library | Load ms | malloc peak MiB | pairs | Identifiers | Closures | Cells |
|---|---|---|---|---|---|---|
| (scheme mapping) | 576 | 286 | 1.50M | **905k** | 497 | 34 |
| (scheme show) | 376 | 142 | 849k | 480k | 792 | 68 |
| (scheme text) | 265 | 112 | 665k | 382k | 53k | 4k |
| (scheme regex) | 242 | 98 | 917k | 236k | 22k | 7.6k |
| (scheme flonum) | 139 | 97 | 510k | 63k | **392k** | **126k** |
| (scheme sort) | 58 | 29 | 165k | 103k | 117 | 4 |
| (scheme base) only | — | 4 | 16k | 9.4k | 17 | — |

Dependencies overlap, so the rows do not sum. Expansion dominates everywhere except `(scheme flonum)`, whose body computes at load time.

**Per-form distribution.** Running a library body as top-level program forms with GC off and `(gc)` between forms, in `FORMS` mode:
- `(nieper rbtree)`, the bulk of `(scheme mapping)`: 48 forms, 1.99M heap allocations.
  - `(define (balance tree) (tree-match …))` alone is **745,676 (37.4%)**;
  - the top 2 forms are 66%;
  - the top 5 are 87.6%.
- `srfi/146.scm`: 107 forms, 144k allocations, largest form 7.7%. **[V]**

So expansion garbage is concentrated in a few forms that use heavy `syntax-rules` macros.

### 2.5 CPU share of the VM load [V]

`sample` at 1 ms, inclusive counts with recursion de-duplicated:

| Component | Share |
|---|---|
| `desugar_with_imports` (all expansion) | 82.0% |
| — `Desugarer::expand_macro` | 54.5% |
| — scope edits: `map_syntax_identifiers` via `edit_scope_on_tagged` (`patina-macros/src/macro_expander/mod.rs:279-298`) | **43.8%** |
| — provenance: `Heap::{record_source, inherit_source, source}` | **19.7%** |
| — `stamp_expansion_source` | 11.9% |
| GC: one post-load collection | 13.4% |
| malloc/free, self time | 10.7% |
| Library parse | 2.6% |
| VM compile (`compile_with_qq_resolving`) | 2.5% |
| Library body execution (nested `execute`) | ≈7% |

The categories overlap. In self time, SipHash over raw-bits keys is visible across three crates.

### 2.6 Bootstrap [V]

Bootstrap is `(scheme base)` plus `(patina debug)` and takes 7–18 ms. Live afterwards is 4.1 MiB. Of that, provenance is 1.9 MiB (46%), arenas 1.4 MiB, macros 0.2, environments 0.2. The arenas hold 15,975 pairs and 9,805 objects, 9,429 of them identifiers. A first collection frees about 96% of the objects (gc-impl **[D]**). Bootstrap is small; the problem is in loading R7RS-large.

---

## 3. What a new collector could address

| Bucket (VM big load) | MiB | GC-addressable? |
|---|---|---|
| Dead arena slots during load | 268 touched + 148 reserved | **Yes**, if loading has safe points |
| Provenance keyed by dead syntax | 316 | **Yes**, through weak pruning as today; or remove it by representation (§5) |
| TW `Rc` payloads of dead procedures | ≈390 (TW) | **Yes**, the same way; and representation (`Rc<Environment>` off-heap) |
| Arena high-water after load | 464 | **Yes**: a page- or block-based space that returns or reuses memory (Immix blocks, Chez segments). Today `Vec` arenas never shrink. |
| Live code, environments, CPS trees | 35 (VM) / 60 (TW) | No: live data. Representation only (`register_roots`, 48 B `Instruction`, 176 B `CpsExpr`). |
| Transient Rust churn | ~1.5 GiB volume, small at any instant | No: this is CPU and allocator pressure. Representation only. |
| Expansion CPU | ≈80% of load time | No. Lazy scope propagation and inline provenance address it (§6.3). |

**Peak reachable with each safe-point granularity [I, anchored by V].**
- *Between libraries:* the floor is the largest single library. `(scheme mapping)` peaks at 286 MiB with its dependencies.
- *Between top-level forms:* rbtree measured 211 → 118 MiB malloc peak and 190 → 137 MiB max RSS (`rb-import` vs `rb-inline`, GC on). Across the 26-library set, the largest form is about 10% of all heap allocations. A floor of about 100–150 MiB against 627 MiB RSS is plausible.
- *Inside expansion* (per macro step): the floor is the largest syntax live at one step. This needs §7's rooting of the expander anyway.

**Time.** The 200 ms post-load sweep exists because 7.5M dead slots are visited and 2.9M `Drop` payloads (`Rc<str>`, `SmallVec`, `Rc` envs) are released.
- A copying or sticky-mark nursery would make dead syntax free, **but only if syntax objects have no `Drop` payload.** Otherwise every dead object must still be visited to release its `Rc`.
- Identifier names as symbol ids and scope sets as interned ids (§6) are therefore a precondition for cheap young collection of syntax, not only a size win.

---

## 4. Where collection can legally happen during a load (Task 2)

### 4.1 The nesting, VM, for a program's `(import …)` [V]

```
run_forms                       interpreter/src/lib.rs:483-528   datum: TV; program SourceMap (raw-bits keyed)
 └ VmBackend::eval_datum(expr)  vm/backend.rs:229-285
   └ desugar_with_imports       frontend/desugarer/mod.rs:1831-1897   [GcDeferGuard #1]
     └ desugar_tagged → desugar_form → … → desugar_import_tagged (:2847)
         import_sets: Vec<TV>; outer `quoted`, `open_forms`, partial CoreExpr if inside a top-level begin
       └ handler: for &set in sets → parse_import_set_tagged → staged env
         └ VmBackend::process_import_set → load_library (:502)   Loading guard (registry loading_stack)
           └ ParsedLibrary::new       runtime/library_loader.rs:195-216   [GcDeferGuard #2, body: Vec<TV>]
             └ evaluate_parsed_library (:567)  lib_env (unregistered)
               ├ Step 1: process_import_set per import  → recursion (point B)
               └ state.borrow_mut().with_globals(lib_env)  vm_state.rs:337-347  [GcDeferGuard #3, `saved` env local]
                   for tv in &parsed.body:            ← point A between iterations
                     desugar_with_imports(*tv, …)     [GcDeferGuard #4] → vm_process_import_set (vm_state.rs:868) → vm_load_library → …
                     compile_with_qq_resolving → load_unit (rooted via code_store) → execute → run_loop_until_outcome [guard #5]
```

The tree-walker has the same shape (`eval/mod.rs:582-690`, `:768-826`). Each body form runs on a *nested trampoline* through `CallbackContext::eval_core` → `eval_cps_with` (`cps_eval/callback.rs:32-49`), and every `run_trampoline` takes a guard (`cps_eval/mod.rs:219`).

**Corrections to earlier inferences [V]:**
- The library registry is **not** held borrowed during a load. `begin_loading_scoped` borrows mutably only to push a name (`library_registry.rs:529-538`), and every other access is a scoped block.
- `LibraryRegistry::try_roots` would therefore succeed at points A and B. The only thing stopping collection there is the five guards.
- VM roots (`vm_state/gc_roots.rs`) cover registers, frames, `code_store` constants, `self.globals`, and the prompt/wind/handler stacks. `with_globals`' `saved` environment is **not** traced. During a library load from a program, `saved` is the program's global environment.

### 4.2 Candidate points

| Point | Rust-held `TaggedValue`s live there | Raw-bits maps live | LIFO `RootScope` enough? |
|---|---|---|---|
| **B. Between libraries**: after `load_library` registered one, before the next import set | Program level: `datum`/`expr` (the import form), `import_sets: Vec<TV>`, the handler's `sets` slice. Nested in A's step 1: A's `parsed.body`. A's `lib_env` is unregistered but holds only forwarded imports (owners are registered libraries). Nested in a body form: everything at point C. | `Heap.syntax_sources` (persistent); program `SourceMap.locations` (persistent); outer desugarer's `quoted` and `open_forms` (usually empty for a bare `(import …)` form) | **Yes**, for the bare-import shape: three or four slots per nesting level, strictly LIFO. Persistent maps need weak or id keys (§5). |
| **A. Between top-level forms of a library body**: after `execute`, before the next `desugar_with_imports` | Everything from B's outer frames, plus `parsed.body` (all forms), `saved` globals (Rust local), and `lib_env` (= `state.globals`, rooted). The body desugarer is idle: `quoted` cleared (`:1821`), `EarlyBinding` cleared, `Declarations` reset. The unit is already released. | Same as B | **Yes.** Root `body` as one heap list, and keep `saved` on a VM-side `globals_stack` (or a root slot). |
| **C. Inside `desugar_with_imports` after an import returns** (handler, `desugarer/mod.rs:1844-1875`) | The whole form under desugaring: `tagged` and every TV in the recursive desugarer frames (`list`, `car`/`cdr`, `args`). `import_sets`. The `staged` env. For a top-level `begin`: the remaining body `Vec<TV>` and the **already-built `CoreExpr`s, which hold `Literal`/`Quote`/`Import` TVs inside Rust `Box` trees**. If the import came from a macro: `list`, `expanded_tagged`, `Rc<CompiledMacro>`. | `quoted` (raw bits → TV, the form's memo); `open_forms`/`OpenNodes` (raw bits of every open pair); plus the persistent maps | **Not by itself.** Rooting is possible but touches every desugarer frame. `CoreExpr` literals are not stack slots. Raw-bits memos go stale under moving. Needs a literal pool (§8, R5) and an epoch check on memos, or keep deferral for this shape. |
| D. Inside a library body's VM execution (nested `run_loop_until_outcome`) | The VM state is consistent, but everything from A and B lives below it on the Rust stack | as above | Yes, once A and B are rooted. This is just "nested loops may collect when every outer frame is rooted". |
| E. TW: between body forms | Each suspended outer trampoline's `StepResult` (the whole outer machine state), and `CallbackContext` slices (prompt, wind and handler vectors) | as above | **Yes.** Nested trampolines are LIFO, so register each trampoline's `StepRoots` on an evaluator-owned stack. |

**What breaks if C is collected under today's code with a non-moving collector [I]:**
- Unrooted literal TVs in partially built `CoreExpr`s would be freed.
- `quoted`'s values would be freed. Its keys are raw bits of input syntax, so they stay valid only if the input is rooted.
- Under a moving collector, every key in `quoted`, `OpenNodes`, `syntax_sources` and `SourceMap.locations` also goes stale.

### 4.3 Recommendation for loading [I]

1. Make **A, B, D and E** collectable. Use a LIFO `RootScope` (Wasmtime `RootScope`/`Rooted`: "roughly equivalent to bump allocation", `research/rust-gcs.md:256`, `research/cranelift-gc.md:107`, Wasmtime `crates/wasmtime/src/runtime/gc/enabled/rooting.rs`) and turn the structural roots into traced state:
   - `Loading` already lives on the registry's `loading_stack`. Give it the in-progress `lib_env` and the body list, so `LibraryRegistry::trace_roots` covers them.
   - Replace `with_globals`' Rust local with `VmState.globals_stack`.
2. Keep **C** deferred, but shrink it. In R7RS an import is a top-level form, and eval_datum already recognizes `define-library` before desugaring (`vm/backend.rs:238`). Recognize a bare `(import …)` there the same way and run the loads with only the import-set list rooted. Only `begin`-spliced or macro-generated imports would still take the deferred path.
3. This alone removes the "library body never collects" pathology: gc-impl's 5M-cons library body kept a 116 MiB arena **[D]**. It also bounds load peaks by single forms.

---

## 5. Provenance under a moving GC (Task 3)

### 5.1 What exists [V]

| Store | Key | Writers | Production readers |
|---|---|---|---|
| `Heap.syntax_sources: HashMap<u64, Rc<SyntaxSource>>` (`heap/mod.rs:305`, `heap/source.rs:8-63`); `SyntaxSource` = `Option<SourceLocation>` (64 B) + `Option<Rc<[Option<SourceLocation>]>>` | raw bits | parser `record_location` (`parser/mod.rs:289`), reader child spans (`parser/datum.rs:251,273`), `inherit_source` in every scope-edit copy (`heap/source.rs:117,152`), `hygiene.rs:55,107,155`, `stamp_expansion_source` (`desugarer/mod.rs:151`), `rewrite_refs` (`:1733,1751`) | `source()` from desugarer `lookup_source` (`:1790`), `stamp_expansion_source` (`:134`), `rewrite_refs` (`:1745`), the macro template compiler (`template.rs:33`), parser `datum.rs:376`. **`child_source` has no production reader** (`rg`: tests only). |
| `SourceMap.locations: HashMap<u64, SourceLocation>` (`source_map.rs:61`) | raw bits | parser `record_location` (`parser/mod.rs:294`), `stamp_expansion_source` (`desugarer/mod.rs:156`) | **None in production.** `get`/`iter_locations` are read only in tests (`reader.rs:634`, `interpreter/src/lib.rs:967`, source_map tests). Error formatting uses `format_context` and `get_expansions`, which are keyed by document line and column. |
| `CodeObject.source_map: Vec<(usize, SourceLocation)>` | PC | codegen | error locations |
| `SourceDocument.expansions: HashMap<(u32,u32), Vec<String>>` | text position | `record_expansion` | `get_expansions` |

Maintenance today: sweep prunes `syntax_sources` and shrinks the table (`gc.rs:892-900`). `SourceMap` is pruned between program forms from `gc_freed_bits`, which is capped at 65,536 (`gc.rs:857`). On overflow the whole map is cleared (`source_map.rs:255-261`), and the post-load collection frees 7.5M slots, so that overflow always happens after a large import.

Measured cost, VM big load: **316 MiB at peak**, ≈20% of load CPU. There are 2.19M identifier entries, 2.38M pairs with locations and 924k pairs with child spans. The table had grown to 2^23 buckets × 17 B.

### 5.2 Options

| | (a) Source info stored in syntax objects | (b) Weak tables keyed by stable ids | (c) Syntax in a non-moving space |
|---|---|---|---|
| Prior art | Racket: `syntax` struct field `srcloc` (`racket/src/expander/syntax/syntax.rkt:52-58`). Chez: `annotation` records `(expression source stripped flags)` and `source` records `(sfd bfp efp)` made by the reader per datum (`ChezScheme/s/types.ss:18-46`, `s/read.ss:1196`). Both are heap data that the collector moves like anything else. | SpiderMonkey per-cell unique ids in a side table, transferred on move (`TransferUniqueId`, `StableCellHasher`), `research/js-engines.md:204`. V8 in-object identity hash. Chez rehashes moved eq-table cells after GC (`s/mkgc.ss:788-809`, `c/gc.c:1734-1773`, `research/chez.md` §9). | Immix pinning / Whippet "pinned" allocation. Non-moving mark-region mature spaces. |
| Representation | Identifier gets `src: u32`: an index into a per-document or per-expansion location table, with expansion chains interned as ids. Copies copy the u32. Pairs: (i) the reader emits "annotated" syntax pairs only for program and library text, or (ii) only the head identifier carries the location, which is what `lookup_source(list)` mostly needs: macro call sites and form starts. | Object id (header word or side table) → weak `HashMap<Id, SourceInfo>`. Copies insert new entries, as today. | Raw-address keys stay valid. Copies insert, as today. |
| Memory, big load | Identifiers +4 B each = **8.4 MiB** (fits in padding with §6's 16 B layout). Location tables per document: tens of KiB. **−300 MiB at peak.** | Same entry count as today (≈4.6M entries, ≈316 MiB). Ids add 8 B per object, or a second side table. | Same as today. Pinned blocks cause fragmentation. |
| GC cost under moving | None. The data moves with the object. | Per-move id transfer or rehash, proportional to young provenance entries. That is about every syntax node during expansion, i.e. most of the nursery. Weak processing per GC. | None for moving, but the nursery cannot hold syntax. Syntax is 76–99% of load-time objects, so a generational design loses its main load-time win. |
| Allocation-time knowledge | Not needed | Not needed | Needed: the allocator must know an object is syntax. Not knowable for `datum->syntax` results built by user `cons` in syntax-case (§7), short of pin-on-annotate, which a copying nursery cannot do after the fact. |
| Code impact | Medium. The parser, desugarer (`lookup_source`, `stamp_expansion_source`, `rewrite_refs`), `hygiene.rs`, `template.rs:33` and codegen source recording all change. The `SourceMap.locations` and `child_source` stores are deleted (no production readers). `stamp_expansion_source` stops walking the expansion: one chain id per expansion. | Small at use sites (`heap.source(tv)` keeps its signature). Medium in the GC (weak phase plus move hooks). | Smallest API change. Large GC change (space selection, pinning). |
| CPU | Removes ≈20% of load CPU (table inserts and Rc clones). | Unchanged, plus GC work. | Unchanged. |

### 5.3 Recommendation [I]

Choose **(a)**. It is what both reference expanders do, it is moving-safe by construction, and it removes the dominant Rust-side load cost. As an immediate, collector-independent step:
- delete `SourceMap.locations` and the child-span store (no production readers);
- stop creating a throwaway `SourceMap` per expansion.

Together these remove 26 MiB at peak and 355 MiB of churn. Keep a weak table only if a later feature needs per-pair spans. Key it by an object id, and populate it only for reader output.

---

## 6. Identifier representation (Task 4)

### 6.1 Distribution at load, big set [V]

- Identifiers: 2,189,740. Written by the reader: 73,023 (3.3%). The rest are copies made by scope edits, renames or `rewrite_refs`.
- Scope-set length:

  | Length | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
  |---|---|---|---|---|---|---|---|
  | Identifiers | 677,539 | 1,207,639 | 263,391 | 29,152 | 7,360 | 3,421 | 1,238 |

  Mean length 0.86. Spilled (more than 3 scopes) 12,019 (0.55%), so the `SmallVec`'s inline capacity is right, and its 40 B is the cost.
- Distinct scope sets: **64,782**. By length, 1/2/3/4/5/6 = 17,847 / 35,955 / 7,980 / 1,606 / 830 / 563, about 128k `ScopeId`s in total. The largest `ScopeId` is 26,241, so a u32 is ample.
- Distinct (name pointer, scopes): 789,605. Distinct (name string, scopes): **356,820**. Distinct name strings: 4,780. Distinct `Rc<str>` names: 73,896 (0.4 MiB).
- `(scheme base)`+`(srfi 1)`+`(scheme hash-table)`: 92,414 identifiers, 2,272 distinct sets, 19,666 distinct (name, sets).
- Bootstrap: 9,429 identifiers, 187 sets.

### 6.2 Estimated savings, big set [I from V counts]

| Layout | Identifier bytes | Δ vs today | Notes |
|---|---|---|---|
| Today: 72 B enum slot (`Rc<str>` 16 + `ScopeSet` 40 + bool) | 150.4 MiB (+0.6 MiB spills) | — | `Drop` payloads (`Rc<str>`, `SmallVec`) |
| Intern scope sets (`u32` id) and keep `Rc<str>`, in the 40 B enum heap-repr measured with Exception/Rational boxed | 83.5 MiB | −44% | Shrinks every object: the objects arena 198 → 110 MiB. A scope-set table of 64,782 sets ≈ 3–4 MiB. |
| Header layout: 8 B header (type, `written` bit) + symbol id u32 + scope-set id u32 | 33.4 MiB | **−78%** | No `Drop`. Nursery-friendly. Add `src: u32` (§5a) → 24 B = 50 MiB. |
| Hash-consed `(symbol, scope-set)` identifiers, 16 B | 5.4 MiB + ≈7 MiB intern table | −92% | Identity is no longer per occurrence, so provenance must go to (a) on the containing syntax. Each alloc becomes a hash probe on a weak table. `bound-identifier=?` and `free-identifier=?` compare names and scopes anyway. |

Interned scope sets also make set operations cheap: `flip_scope`, `with_scope` and subset checks can memoize on (set id, scope) → set id. The scope-set table must be collectable: either a weak table, or sets as heap objects (Racket uses immutable hash sets of scope structs).

### 6.3 The bigger lever: stop copying [I, V for the numbers]

96.7% of identifiers and most pairs are copies made by eager scope edits. `edit_scope_on_tagged` rebuilds every affected identifier and every enclosing pair or vector through `map_syntax_identifiers_memo`, with four raw-bits hash tables per call. That is 44% of load CPU and 950 MiB of churn.

Racket applies add, remove and flip lazily: `apply-scope` struct-copies only the top node and records a pending propagation, which `syntax-e` pushes down when a subform is opened (`racket/src/expander/syntax/scope.rkt:430-470`). Chez's syntax-case similarly keeps a `wrap` on `syntax-object` (`s/types.ss:43-46`).

Lazy propagation would cut both the allocation volume the collector sees and the provenance problem: nothing is copied, so nothing inherits. It is a front-end change and independent of the collector, but it decides how much a new collector has to do during loading.

---

## 7. syntax-case: what changes when transformers run Scheme mid-expansion (Task 5)

Today expansion is GC-atomic per top-level form. `desugar_with_imports` holds a guard, and the comment on `quoted` says "No GC runs while desugaring" (`desugarer/mod.rs:369-372`). `syntax-rules` never calls Scheme. Procedural transformers end this.

1. **Transformer calls are safe points with a deep Rust stack beneath them.** The call runs the VM (a stub frame or nested loop) inside `desugar_list_tagged`. Every value from §4.2 point C is live:
   - the input form;
   - the transformer result;
   - partial `CoreExpr`s with literals;
   - `Renames`/`Aliases` TVs;
   - `MatchEnv` (`pvref.rs:236-241`), for `syntax-rules` helpers used inside `syntax-case`.

   Deferring for the whole form is still an option, but it reproduces today's pathology for compute-heavy macros (`match` written in `syntax-case`, CK-style macros). It also cannot be bounded.
2. **The expander becomes a root provider.** Put the desugarer's per-form state in an `ExpansionContext` registered with the collector (`impl GcRoots`), holding:
   - a LIFO root stack for frame-local syntax;
   - a **literal pool** (`Vec<TV>`, traced and updatable) that `CoreExpr::Literal/Quote/Import` refer to by index. The VM's constant pool and JIT constant tables want this anyway: no embedded heap pointers in IR or code.
   - the memos.
3. **Raw-bits memos.**
   - `quoted` (per form): make it keyed by a stable id, or tag it with a GC epoch and clear it when the epoch changes. Clearing is safe except for literal sharing across the GC point.
   - `OpenNodes` (cycle detection over the open spine): store rooted handles, or re-key on epoch change.
   - The memos inside `map_syntax_identifiers_memo`, `stamp_expansion_source`'s `seen`/`histories`, the matcher's tables and `mark_substituted_in` (`hygiene.rs:87-155`) never span a Scheme call, so they stay plain transient maps if the walks stay in Rust. They disappear with lazy propagation (§6.3).
   - `syntax_sources` and `SourceMap.locations` must already be gone or id-keyed (§5).
4. **Identifiers and scope sets become user-visible values.** `datum->syntax`, `syntax->datum`, `generate-temporaries` and `bound-identifier=?` hand syntax to user code, which stores it in arbitrary data. This is the case where (c) fails: the allocator cannot know what will become syntax. (a) and the header layout of §6.2 handle it naturally.
5. **Continuations in transformers.** The expander's Rust frames cannot be captured. Mark each transformer call as a re-entry boundary: `across_reentry`, as library body forms already are (`vm_state.rs:836-850`), or a continuation barrier. This rejects re-entry after the transformer returns. That matches Patina's existing rule that a Rust frame cannot be part of a continuation (AGENTS.md).
6. **Phase-1 library instantiation inside expansion.** Libraries imported for syntax are loaded mid-expansion, so load inside expand inside load nests arbitrarily. The LIFO rooting of §4 has to compose at any depth.
7. **A collector shape that makes this tractable [I].** If the mature space is non-moving (mark-region), or evacuation is suspended while an expansion context is active, run a minor GC at transformer entry. That promotes the expander's live syntax. During the transformer only objects it allocated can move, so raw-bits keys that point at promoted syntax stay valid. This is Chez's "allocation never collects, safe points only at calls" discipline plus a pinning epoch. The cost is one minor collection per transformer call, cheap if the nursery is mostly dead.

---

## 8. Rooting and representation changes to make loading memory collectable

**Rooting (collector-independent; needed even for a non-moving successor):**
- **R1.** Drop `ParsedLibrary`'s lifetime guard (`library_loader.rs:195-216`). Store the body as one heap list held by the registry's `Loading` entry, together with the in-progress `lib_env`. `LibraryRegistry::trace_roots` traces both. Loading is already LIFO through `begin_loading_scoped`.
- **R2.** `VmState::with_globals`: push `saved` onto `globals_stack: Vec<Rc<Environment>>`, traced by `trace_roots`, instead of a Rust local plus guard (`vm_state.rs:337-347`). Also root `VmBackend.global_env` explicitly.
- **R3.** Allow nested loops (`run_loop_until_outcome`, `run_trampoline`) to collect when every frame below is registered. Turn the "outermost" boolean into "no unregistered Rust frame below", i.e. keep the defer counter only for the truly unrooted paths: point C, and the remaining `apply_proc` callers from gc-impl §5.4.
- **R4.** TW: an evaluator-owned stack of `StepRoots` for nested trampolines, plus the `CallbackContext` slices.
- **R5.** Use a `CoreExpr` literal pool (index-based) for both backends. It is needed for C, for syntax-case and for a moving collector.
- **R6.** Hoist bare top-level `(import …)` out of the desugarer, as eval_datum does for `define-library`. Keep point C deferred only for spliced or macro-produced imports.

**Representation:**
- **P1.** Inline provenance (`src: u32` on identifiers, chain ids per expansion). Delete `SourceMap.locations`, the child-span store and the throwaway per-expansion `SourceMap`s. Saves −316 MiB at peak and about 20% of load CPU.
- **P2.** Identifiers become a 16–24 B header object: symbol id, interned scope-set id, `src`. No `Drop` payload, so a nursery can discard dead syntax without visiting it.
- **P3.** Lazy scope propagation (Racket) or wraps (Chez) instead of eager `map_syntax_identifiers` copies. Cuts identifier and pair allocation by most of the 96.7% copy share and 44% of load CPU.
- **P4.** Spaces that return memory: Immix blocks or Chez-style segments with page release, instead of `Vec` arenas that never shrink. The retained 464 MiB of empty capacity goes away.
- **P5 (TW).** Stop allocating per-call `Rc<Environment>` scoped tables for macro-introduced parameters. This follows SYNTAX_CASE_DESIGN's "resolve once before the backends" (`PRD/macro/SYNTAX_CASE_DESIGN.md:89-138`), and removes most of the TW's 390 MiB of GC-coupled garbage.

---

## 9. Constraints and lessons for the redesign

- The workload that hurts at load time is allocation-heavy syntax manipulation with a tiny survivor set: 0.3% of pairs and 0.2% of objects survive. That is the ideal case for a nursery, but only once syntax objects carry no `Drop` payloads and loading has safe points.
- Weak, GC-pruned side tables keyed by object identity are an anti-pattern at this scale. Here one cost more than the arena it annotates. Prefer data stored in the object.
- Any raw-bits-keyed map is either transient (scoped to a no-GC region) or must move to stable ids. Inventory today:
  - persistent: `syntax_sources`, `SourceMap.locations`;
  - per-form: `quoted`, `OpenNodes`;
  - per-call: scope-edit memos, `stamp_expansion_source`, `mark_substituted_in`.
- The registry is not the obstacle to collecting during loads. The guards are. Roots for loading are few, coarse and LIFO, so they are cheap to add.
- Arena capacity is the retention problem after a load: 464 of 501 MiB.
- `register_roots` costs about 40 B per PC (65% of the instruction stream) for a map that only GC and capture read. A JIT-era stack map should be sparse (safepoints only) and compressed.
- Changes to the expander (lazy scopes, inline provenance) reduce load time far more than any collector can (expansion is about 80% of load CPU). Sequence them together with, or before, the collector, so that the collector is designed against the real allocation profile.

## 10. Open questions

1. Which provenance granularity is required?
   - Per-identifier `src` plus form heads may be enough for every current error message: `lookup_source(list)` and the call-site source.
   - Per-pair spans exist but are unread. Confirm with the error-message tests and the hygiene matrix.
2. Do identifiers need per-occurrence identity anywhere?
   - Possible places: `values_eq` on identifier objects, the `quoted` sharing, and `is_source_identifier`, which skips written identifiers in scope edits (`macro_expander/mod.rs:288`).
   - This decides whether hash-consing (§6.2, row 4) is viable.
3. Does lazy propagation interact badly with the desugarer's cycle handling for circular syntax (#459, `rewrite_refs`)?
4. Is the CPU saving from deleting the per-expansion `SourceMap` and the child spans as large as the churn suggests? Re-measure after removing them, since that is a small and independent PR.
5. For syntax-case, choose between:
   - deferral per top-level form (simple; unbounded memory for heavy macros);
   - a full expander root provider;
   - the minor-GC-at-transformer-entry plus non-moving mature space scheme (§7.7), which depends on the mature-space choice made elsewhere in the redesign.
6. The reported max RSS (627 MiB) sits about 16% below the malloc-live peak (765 MiB), because reserved arena capacity is untouched. How much of a future page-based space's footprint would be reserved but untouched address space? This matters for comparisons against the current numbers.
