# Track P — VM Performance (Clarity-Safe) PRD

**Created:** 2026-06-20
**Status:** In progress — first profile-driven wave landed 2026-07-25/26 (PRs #149, #150, #151, #152): **2.4× on call-heavy code, ~2–2.7× across the r7rs-benchmarks quick set**. See §1.1.
**Scope decision:** clarity-safe optimizations only — aggressive, readability-costing items are explicitly deferred.
**Umbrella:** `PRD/SNOW_AND_PERF_ROADMAP.md` (cross-track sequencing) · **Catalog:** `PRD/VM_OPTIMIZATION_ROADMAP.md` (P1–P10 superset)

---

## 1. Context & problem

The register bytecode VM is the default backend and passes 1163/1163 R7RS tests, but it is a *first-generation* machine (~4.2× the tree-walker, far from production Schemes). The dominant costs are on the per-call and per-allocation hot paths, and there is **no memory reclamation at all**. None of the wins below require an architectural rewrite, and each preserves — or improves — readability, in line with Patina's educational goals.

### Verified current-state evidence

| Observation | Evidence |
|---|---|
| ~~Every primitive call (`+`, `car`, …) goes through a **string `HashMap`** lookup.~~ **Fixed in #149** (Vec-indexed registry + cached index on `Procedure::Primitive`). | `PrimitiveRegistry { primitives: HashMap<String, PrimitiveFn>, name_index: HashMap<&'static str, String> }` — `crates/patina-primitives/src/registry.rs:101`; dispatch `apply_tagged` → `lookup_primitive` at `:170`/`:184`. |
| The fast-path opcode exists but is **dead**. *(Still true; upside reduced by #149's runtime index cache.)* | `Instruction::CallPrimitive { func_id: PrimitiveFnId, .. }` (`crates/patina-vm/src/types/instruction.rs:118`); `PrimitiveFnId(pub u32)` at `:213`; runtime arm errors `"CallPrimitive not yet wired (A3+)"` — `crates/patina-vm/src/runtime/vm_state.rs:1462`. |
| ~~Args are **cloned** on the generic primitive path at **four** sites.~~ **Fixed in #150** (check-then-move split). | `try_call_primitive(state, func_val, arg_vals.clone())` — `vm_state.rs:884, 962, 1054, 1079`. |
| *(Found by profiling, not in original survey:)* ~~dispatch paid a SipHash `code_store` lookup + `Rc` churn + `Instruction::clone` per instruction executed.~~ **Fixed in #152** (frame-cached `Rc<CodeObject>`, borrowed instructions). | `dispatch_one_instruction` fetch path, `vm_state.rs`. |
| Closures **clone their whole free-var `Vec`** per call, then discard it. | `Heap::get_vm_closure → (u32, Vec<TaggedValue>)` clones — `crates/patina-core/src/heap/mod.rs:753`; caller `resolve_closure`/`call_closure` discard the vec — `vm_state.rs:1514`/`:598`. Per-slot reads already exist: `get_vm_closure_free_var` at `:780`. |
| **No GC.** Free lists exist and are drained by `alloc_*`, but **nothing ever fills them**. | `free_pairs/free_vectors/free_strings/free_objects` — `heap/mod.rs:258-267`; drained at `alloc_pair:307`, `alloc_vector:357`, `alloc_string_chars:411`, `alloc_object:854`. No `sweep`/`collect`/`mark` anywhere. |
| Fixnum fast ops already exist (basis for inline opcodes). | `fixnum_add:154`, `fixnum_sub:168`, `fixnum_mul:182`, `fixnum_lt:196`, `fixnum_eq:204`, `fixnum_le:211`; `is_fixnum:284`, `is_pair:302`, `is_vector:308` — `crates/patina-core/src/tagged_value.rs`. |
| Benchmarks measure the **wrong backend**. | `crates/patina-tests/benches/scheme_benchmarks.rs` constructs `TreeWalkInterpreter::new_tree_walker()`. |

### 1.1 Progress log — first wave (2026-07-25/26, PRs #149–#152)

Executed profile-first (macOS `sample`/`samply` on `fib(34)` and `destruc`) rather than in the planned P-order; the profiles reordered the work and surfaced one large item the plan had missed. All measurements same-machine (M-series macOS), medians of repeated runs; every PR gated on the full workspace suite + 1163/1163 chibi + clippy + fmt.

| PR | What landed | Relation to plan | Measured |
|---|---|---|---|
| #149 | `PrimitiveRegistry` storage is `Vec`-indexed; `Procedure::Primitive` carries a `registry_index: Cell<Option<usize>>` (VM resolves eagerly at install, tree-walker lazily); hot dispatch via `apply_cached` — no name hashing | **P2, alternate design:** runtime-cached integer IDs instead of compile-time `CallPrimitive` emission. Banked most of P2's win without compiler changes; `CallPrimitive` wiring remains open with reduced upside | fib(32) −20%, destruc −23% |
| #150 | `try_call_primitive` split into `primitive_procedure` (type check) + `call_primitive_proc` (consumes args by move); all four defensive `arg_vals.clone()` sites and two unconditional `to_vec()` sites removed | **P1.2 as specified** | fib −6.5%, destruc −12% |
| #151 | `Environment` binding maps → `rustc-hash::FxHashMap` | **P4, first slice** (cheap-hash; slot indices still open) | fib −4.7%; others −3–5% |
| #152 | `CallFrame` caches its `Rc<CodeObject>`; dispatch borrows instructions (`match *instr` + `ref`) instead of cloning; `code_store` → FxHashMap | **Not in plan** — profiling found the dispatch loop paid a SipHash lookup + `Rc` churn + an `Instruction::clone` (heap-allocating `Vec<Reg>` for call-shaped ops) **per instruction executed** | fib −40%, destruc −38%, compiler −40%, matrix −36%, maze −30% |

**Cumulative:** fib(32) 5.68s → 2.37s (**2.4×**); quick r7rs-benchmarks set ~2–2.7× across the board. Standing vs Chibi (same machine): `browse` 2.5× *faster*, `quicksort` 3.6× behind (was 8×), `destruc` 8× behind (was 16×). Full-workload harness runs (validated-correct results, 300s CPU cap): **`compiler` 177.7s, `maze` 150.4s, `matrix` 270.0s — all three now complete under the cap** (all were cap-outs before this wave; `compiler`/`maze`/`matrix` couldn't even *parse* before the #146/#147 lexer fixes). `parsing` still caps out — it is frontend-bound (lexer/parser), not VM-bound.

**Hard-won invariants (do not regress):**
- `CallFrame.code` must stay in sync with `CallFrame.code_id`: the `TailCall`/`TailApply` frame-reuse fast path mutates `code_id` in place and **must** update `code` too, or the VM replays stale code from pc 0 (infinite loop on any `letrec`-shaped loop). Found via a hung `destruc`; reduced to a 3-line `letrec` repro.
- **Negative result:** caching the resolved globals `Rc<Environment>` in `CallFrame` at push time is *incorrect* — `state.globals` is swapped live during library loading, so globals must be resolved per instruction (`frame_globals`). It also benchmarked slower. Any retry requires redesigning the library-loading environment swap first (affects P4's inline-cache ambitions).
- The division primitive's qualified name is `library//` (its short name is `/`): anything splitting qualified names must use `split_once('/')`, never `rsplit`.

**Also relevant:** patina is wired into the ecraven r7rs-benchmarks harness (`~/Project/r7rs-benchmarks`, local changes: `bench` script `patina` entry + `src/Patina-postlude.scm`; run `PATINA=<path> ./bench patina all`). This currently serves as the perf baseline in place of P0's Criterion repoint, which remains open.

**Next candidates, profile-ranked:** slice-based primitive handler ABI (`&[TaggedValue]`) to remove the last per-call collect (~300 signatures, mechanical); P1.1 `free_vars` clone in `resolve_closure` (visible in profiles via `get_vm_closure`); register-window zeroing (`memset_pattern16` in profile) in `alloc_registers`; then P3 inline opcodes. `parsing` needs frontend work outside this track.

## 2. Goals
- Cut per-call and per-allocation overhead measurably (target **2–5×** on arithmetic/list-heavy code from P2+P3).
- Make VM performance **measurable** and regression-guarded.
- Add **memory reclamation** (correctness for long-running programs) without destabilizing the test baseline.
- Keep every change readable: fast paths fall back to the *existing* slow path, so semantics never diverge.

## 3. Non-goals (deferred — clarity tradeoff)
Flat `Vec<u32>` bytecode encoding, threaded dispatch, liveness-based register allocation, NaN-boxed inline floats, bytecode serialization, continuation stack-slicing, JIT. Tracked in `PRD/VM_OPTIMIZATION_ROADMAP.md` (P2/P6/P7/P8/P9/P10). If revisited, the readable match-based loop must remain as a documented reference path.

---

## 4. Work items

### P0 — Repoint benchmarks at the VM + baseline  *(blocking; gates all perf claims)*
The Criterion harness benchmarks the tree-walker. Parameterize `scheme_benchmarks.rs` (or add a sibling) to construct the VM backend, and check in a baseline for tak/fib/ack/nqueens/deriv/primes + list/vector/numeric programs (`crates/patina-tests/bench_programs/*.scm`). Keep `scripts/bench_compare.sh` (wall-clock VM-vs-tree-walker via `(current-jiffy)`) as a cross-check.
- **Acceptance:** `cargo bench -p patina-tests` exercises the VM; baseline numbers committed.

### P1 — Near-free clone removals  *(1 of 2 done)*
1. **`free_vars` clone.** *(Open — visible in profiles as `get_vm_closure` allocation under `resolve_closure`.)* Add `Heap::get_vm_closure_code_id(val) -> Option<u32>` reading only `code_id` (no `Vec` clone). Change `resolve_closure` to use it; `call_closure` already discards the vec. Slot reads stay via `get_vm_closure_free_var`.
2. ~~**`arg_vals.clone()`**~~ **Done — PR #150** (as `primitive_procedure` + `call_primitive_proc` check-then-move split; same effect as the planned `&[TaggedValue]` signature).
- **Acceptance:** `cargo test` green; closure/tail tests (`tail_recursion.rs`, `cps_features.rs`) unchanged.

### P2 — Fast primitive dispatch: integer IDs + wire `CallPrimitive`  *(core win banked in #149; compile-time wiring still open)*
Eliminate the string `HashMap` lookup on the hot path. **Status:** PR #149 delivered item 1 in an alternate form (Vec-indexed registry, `resolve_index`/`apply_by_index`/`apply_cached`, index cached per `Procedure::Primitive` instance) — the per-call hash is gone without compiler changes. Items 2–3 (compile-time `CallPrimitive` emission) remain open but now buy less: mainly skipping `get_procedure` + the `Cell` read, and enabling P3's inline opcodes.
1. **Deterministic IDs in the registry** (`registry.rs`). Add an ordered `by_id: Vec<PrimitiveFn>` plus `id_index: HashMap<String, PrimId>`; `register()` (`:114`) pushes and records the index for both qualified and bare names. Registration order is fixed (`primitives/mod.rs`), so IDs are stable. Add `id_of(name) -> Option<PrimId>` and `apply_by_id(id, args, ctx)` (reuses `apply_tagged`'s body minus the string lookup). Define the id newtype in `patina-primitives`; make `patina-vm`'s `PrimitiveFnId` a `From`-convertible wrapper (no new crate dependency).
2. **Resolve name→ID at compile time.** Thread a read-only resolver into `compile_with_qq`/Pass-5 codegen (`crates/patina-vm/src/compiler/pass5_codegen.rs`) from the registry the backend already owns. In the `App` arm, when the callee is a non-shadowed `GlobalRef` to a primitive, emit `CallPrimitive { func_id, args, dst }` instead of `Call`.
3. **Wire the runtime arm** at `vm_state.rs:1462`: collect args, build the `VmApplyContext` (same pattern as `try_call_primitive`), call `apply_by_id` — no string hash, no `get_procedure` borrow, no `Procedure` downcast. Arity check stays inside `apply_by_id`.
- **Correctness gates (critical):**
  - **Exclude control primitives** intercepted by `vm_control_primitive` (`vm_state.rs:2350`) and `apply` — those MUST keep the `Call` path. Encode the exclusion as a `const` set referenced by codegen.
  - **Shadowing:** emit `CallPrimitive` only when the name is not redefined by a top-level user `define` in the unit (reuse Pass-1's define scan). Anything uncertain → fall back to `Call` (at worst slower, never wrong).
- **Acceptance:** disasm test shows `(+ 1 2)` emits `CallPrimitive`; full `cargo test`; `./scripts/run_chibi_tests.sh` 1163/1163.

### P3 — Specialized inline opcodes for the ~15 hottest primitives
Add fixed-arity opcodes executed inline in the dispatch loop — `Add/Sub/Mul/Lt/NumEq/Eq/Car/Cdr/Cons/NullP/PairP/VectorP/VectorRef/VectorSet/Not` — each a plain struct of `Reg`s.
- **Fast path:** guard `is_fixnum()` on both operands and use `fixnum_add/sub/mul/lt/eq` (`tagged_value.rs:154-211`); predicates/`eq?`/`cons`/`car`/`cdr` are immediate or a single heap op (`values_eq`, `alloc_pair:307`, `car`/`cdr`).
- **Slow path = existing path.** On non-fixnum / overflow / type-error, fall back to the *same* function the primitive already calls (`Heap::numeric_add/sub/mul`, the `car`/`cdr`/`vector-ref` primitives). No duplicated numeric logic; bignum promotion, rationals, reals, complex, NaN propagation, and error messages are byte-for-byte identical.
- **Codegen:** emit only for the common fixed-arity shape; variadic `+ - * < =` route their **2-arg** case to the binary opcode and everything else (0/1/≥3 args, unary `-`) to `CallPrimitive`.
- **Why readable:** each arm is ~6 lines naming the op; the "fixnum is one machine add, overflow promotes to bignum" story is clearer inline than in a variadic fold.
- **Acceptance:** per-opcode tests on **both** paths — fixnum (`(+ 1 2)`→3), overflow→bignum (`verify_bigint_promotion.rs` semantics), non-fixnum (`(+ 1.5 2)`→3.5), type error (`(car 5)`, `(vector-ref v 99)`); full `cargo test`.
- **Estimated P2+P3 impact:** 2–5× on arithmetic/list-heavy code.

### P4 — Indexed / cached global access  *(first slice done in #151; caution flag added)*
`LoadGlobal`/`StoreGlobal` hash a `String` and walk the parent chain. **Status:** #151 swapped `Environment`'s maps to `FxHashMap` (−3–5% on globals-heavy code). Slot indices / inline caches remain open — **but see §1.1's negative result:** `state.globals` is swapped live during library loading, so any push-time or callsite-cached resolution of the globals *environment* is unsound today; only caching within a stable environment (e.g. slot index into a map that is mutated but never replaced) or redesigning the library-loading swap can go further.
- **Acceptance:** `cargo test`; benchmark a global-bound loop against the P0 baseline.

### P5 — Cheap, readable compiler passes
The compiler runs zero optimization passes; the 5-pass pipeline is structured for insertion (`docs/VM_COMPILER.md §12`). Add the low-complexity ones: **constant folding** (`(+ 1 2)`→`3`), **dead-code elimination** of unused bindings, **peephole** (dead `Move` removal, `LoadConst+Add`→immediate). Skip contification / copy-propagation / liveness regalloc (deferred).
- **Acceptance:** golden disasm tests; `cargo test`.

### P6 — Garbage collection (parallel correctness sub-track; feature-flagged)
Stop-the-world **mark-and-sweep** over the typed arenas, gated behind a `gc` Cargo feature so the baseline is untouched until proven.
- **Mark bits:** parallel bit-vectors per arena added to `Heap` (no `TaggedValue` bloat, no header change): `mark_pairs/mark_vectors/mark_strings/mark_objects`.
- **Roots:** VM `registers`; every `CallFrame.closure` index; `value_buffer`; the `globals` `Environment` + parent chain (needs a new read-only `Environment::for_each_value`); `code_store` `CodeObject.constants`; continuation/delimited-continuation snapshots (`registers` + frame closures); the `symbol_table` (`heap/mod.rs:255`).
- **Tracer:** iterative worklist; skip immediates; PAIR→push car/cdr; VECTOR→push elements; STRING→leaf. For OBJECT, push the embedded `TaggedValue`s of exactly the value-bearing variants (`heap/mod.rs:119-179`): `Complex{real,imag}`, `Exception{irritants}`, `Record{fields}`, `Parameter{values}`, `Promise(Delayed/Forced)`, `MutableCell`, `VmClosure{free_vars}`, `Values`. All other variants are leaves for the arena GC.
- **Sweep:** push unmarked slots onto the **existing free lists** (already drained by `alloc_*`). Indices are reused **in place** → no pointer rewriting; all tagged pointers stay valid. Do not shrink arena `Vec`s.
- **Trigger:** allocation-count threshold checked at a **VM safe point** (top of `dispatch_one_instruction`/`run_loop_until`, no heap borrow held); adaptive `threshold = max(min, live*2)`. Never call GC from inside `alloc_*` (re-entrant borrow risk).
- **Staging:** sweep **pairs + vectors only** first (highest churn, no `Rc` payloads, sidesteps symbol/continuation liveness); expand to the object arena once differential tests pass.
- **Tree-walker coexistence:** the same `SharedHeap` backs both backends; GC runs **only from the VM driver at a safe point**, never mid tree-walk. Feature flag keeps default/mixed builds unaffected. Reclaiming an object slot drops its `Rc` payload naturally.
- **Acceptance:** two CI lanes — `--no-default-features` (must equal today) and `--features gc`; a `--gc-stress` mode (threshold=1) runs the full compliance suite to identical output; liveness stress (sum `(iota 100000)`), reuse proof (arena doesn't grow by 2N), cycle test (`set-cdr!` loop), paranoid pre-sweep assertion in debug builds.

---

## 5. Sequencing within the track
**P0** (must be first) → **P1** (warm-up) → **P2 → P3** (the big win) → **P4 → P5** (compounding) ; **P6** runs in parallel behind its feature flag and lands by milestone M4. P1 is independent and can be done anytime; doing it first banks an easy win and de-risks the P2/P3 diff. See `PRD/SNOW_AND_PERF_ROADMAP.md` for the M1–M4 interleave with Track L.

## 6. Risks & mitigations
- **Shadowed/redefined primitives** → conservative "no top-level define of this name" guard; fall back to `Call`.
- **Control primitives inlined by mistake** → explicit `const` exclusion set (the `vm_control_primitive` names + `apply`); single most important gate.
- **Error-message parity** → opcodes fall back to the named primitive on the error path, matching suite expectations.
- **GC missed root → use-after-free** → pairs+vectors-only first cut, feature flag, `--gc-stress`, paranoid assertion lane, differential testing as the acceptance bar.

## 7. Verification (track-wide)
- Routine: `cargo build --release && ./scripts/run_chibi_tests.sh` (must stay 1163/1163) after every item.
- Perf: `cargo bench -p patina-tests` (VM-backed after P0) vs baseline; `./scripts/bench_compare.sh` cross-check.
- Quality gate: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt`.
