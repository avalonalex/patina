# Track P — VM Performance (Clarity-Safe) PRD

**Created:** 2026-06-20
**Status:** In progress — first profile-driven wave landed 2026-07-25/26 (PRs #149, #150, #151, #152): **2.4× on call-heavy code, ~2–2.7× across the r7rs-benchmarks quick set**. See §1.1. Second wave, 2026-07-26/29: P0 (#154), P1.1 (#155), P7 phase 1 (#157), P2 `CallPrimitive` (#158), **P3 inline opcodes (#159) — the P2+P3 pair delivered 2–3.2× on arithmetic/list-heavy code in one day**. Remaining work re-ranked by the 2026-08-03 r7rs sweep (§1.2): call path first (P7 phase 2, then P4), weak continuation tables (cross-track, `PRD/future/GC_STAGE5_PRD.md`), then P5 compiler passes. **P6 GC: complete through stage 4c (PRs #4-#6, #8, #10, #11, 2026-08-01/03)** — both backends collect, **always on** at zero standing cost (safe point = one flag load; on-vs-off at parity on dispatch- and alloc-heavy workloads); CI enforces the byte-identical differential lanes, whose env hooks are the only remaining use of `PATINA_GC`/`PATINA_GC_STRESS`. Stage 5+ pause work tracked in `PRD/future/GC_STAGE5_PRD.md`.
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
| #154 | Criterion harness benchmarks the VM by default (`PATINA_BENCH_BACKEND=tree-walker` for cross-checks); baseline table committed; nboyer/sboyer vendored | **P0 as specified** | baseline committed |
| #155 | `resolve_closure` reads `code_id` via new `Heap::get_vm_closure_code_id` — no free-var `Vec` clone | **P1.1 as specified** | small; closes P1 |
| #157 | Heap-tier handlers take `&[TaggedValue]`; **70 misfiled higher-order handlers reclassified to heap tier**; registration closures → direct fn refs; `call_any`/`call_any_sync` arg copies deleted | **P7 phase 1 as specified** — plus the reclassification the tier split implied (see §P7 findings) | map −6.8%, append −5.4%, fill −5.4%, reverse −6.9%, vectors/sum −11.5%, factorial −4–8%, float_sum −7.9%; fib/tak ±1–2% (layout-scale) |
| #158 | `CallPrimitive` wired: compile-time resolution of `GlobalRef` callees to registry ids (`primitive_calls.rs`), no callee load / no frame push, tail = `CallPrimitive`+`Return`; redefinition handled by runtime deopt bitset instead of the planned static scan | **P2 items 2–3**, deopt design replaces the spec's shadowing scan (see §P2) | sum −28–33%, map/make/append −21–23%, factorial −13–21%, tak −17%, fib −23% (wall-clock) |
| #159 | 14 inline opcodes in the dispatch loop (`Add`…`VectorSet`), emitted for exact-arity resolved callees; all fallbacks via `exec_call_primitive`; redefinition design space documented for future work | **P3 as specified** minus dead `Not` (Scheme-defined) | sum −42–46%, vectors/fill −44%, vectors/sum −42%, nqueens/8 −39%, deriv −28–32%, factorial −23–29%, fib −30% / tak −16% (wall-clock). Cumulative with #158, same day: sum 3.2×, nqueens 2.1×, fib 2.0× |

**Cumulative:** fib(32) 5.68s → 2.37s (**2.4×**); quick r7rs-benchmarks set ~2–2.7× across the board. Standing vs Chibi (same machine): `browse` 2.5× *faster*, `quicksort` 3.6× behind (was 8×), `destruc` 8× behind (was 16×). Full-workload harness runs (validated-correct results, 300s CPU cap): **`compiler` 177.7s, `maze` 150.4s, `matrix` 270.0s — all three now complete under the cap** (all were cap-outs before this wave; `compiler`/`maze`/`matrix` couldn't even *parse* before the #146/#147 lexer fixes). `parsing` still caps out — it is frontend-bound (lexer/parser), not VM-bound.

**Hard-won invariants (do not regress):**
- `CallFrame.code` must stay in sync with `CallFrame.code_id`: the `TailCall`/`TailApply` frame-reuse fast path mutates `code_id` in place and **must** update `code` too, or the VM replays stale code from pc 0 (infinite loop on any `letrec`-shaped loop). Found via a hung `destruc`; reduced to a 3-line `letrec` repro.
- **Negative result:** caching the resolved globals `Rc<Environment>` in `CallFrame` at push time is *incorrect* — `state.globals` is swapped live during library loading, so globals must be resolved per instruction (`frame_globals`). It also benchmarked slower. Any retry requires redesigning the library-loading environment swap first (affects P4's inline-cache ambitions).
- The division primitive's qualified name is `library//` (its short name is `/`): anything splitting qualified names must use `split_once('/')`, never `rsplit`.

**Also relevant:** patina is wired into the ecraven r7rs-benchmarks harness (`~/Project/r7rs-benchmarks`, local changes: `bench` script `patina` entry + `src/Patina-postlude.scm`; run `PATINA=<path> ./bench patina all`). This currently serves as the perf baseline in place of P0's Criterion repoint, which remains open.

**Next candidates, profile-ranked:** P2 compile-time `CallPrimitive` wiring + **P3 inline opcodes** (the big remaining win; also takes fib/tak off the layout-sensitive generic call path — see §P7 findings); P7 phase 2 (VM `Call` arm passes the register slice directly to heap-tier handlers, deleting the per-call collect); register-window zeroing (`memset_pattern16` in profile) after P3. `parsing` needs frontend work outside this track.

**Measurement discipline (learned in #157):** single Criterion runs drift ±5–10% on the µs-scale benches on the dev machine — enough to flag phantom regressions. Validate perf changes with an interleaved A/B: bench main with `--save-baseline`, bench the branch against it, then re-bench main against its own baseline to measure drift; cross-check with alternating wall-clock runs of the two release binaries.

### 1.2 Benchmark standing and priority re-rank (2026-08-03)

First r7rs-benchmarks sweep after wave 2 + always-on GC: 10-benchmark subset
vs a **locally-run Chibi 0.12** (same machine — the checked-in results for
other schemes are from foreign hardware and not comparable). Seconds, 300 s
CPU cap:

| Benchmark | Patina | Chibi | Ratio | vs Jul 25 |
|---|---|---|---|---|
| slatex (strings/IO) | 34.4 | 91.4 | **0.38× — faster** | −66% |
| matrix (vectors) | 196.7 | 141.3 | 1.4× | −27% |
| compiler (mixed) | 120.9 | 72.0 | 1.7× | −32% |
| maze (mixed) | 115.0 | 53.4 | 2.2× | −23% |
| diviter (list churn) | 136.8 | 46.1 | 3.0× | |
| deriv (symbolic) | 239.8 | 78.5 | 3.1× | |
| nboyer (symbolic, GC-active) | 93.3 | 25.5 | 3.7× | |
| divrec (recursion) | 140.1 | 37.5 | 3.7× | |
| tak (pure calls) | 238.0 | 43.1 | 5.5× | |
| ctak (call/cc per call) | crash | crash | — | |

**Geomean ≈ 2.2× slower than Chibi**, and the July→now deltas (−23–66% while
*also* turning GC on) confirm the waves compound. The spread orders almost
perfectly by **call density**: Rust-native infrastructure (strings/ports)
already beats Chibi, structured code sits under 2×, and pure function calls
are 5.5×. Patina's `ctak` crash is a **4 GB memory blowup**: every capture
pins register/frame snapshots in the continuation side tables that nothing
prunes — GC design §9.5, live.

**Priority for the remaining work, in order:**

1. **Call-path cost** — the defining gap (tak 5.5×, divrec 3.7×).
   Profile-first, then: **P7 phase 2** (slice ABI — the last per-call
   argument-`Vec` allocation, previously measured as the malloc/free tail
   under callees), **P4 continuation** (slot-indexed globals or the
   globals-swap redesign that unblocks it), and whatever the fresh profile
   says about closure-call frame setup.
2. **Weak continuation side tables** — GC stage 5 priority 1, tracked in
   `PRD/future/GC_STAGE5_PRD.md`; cross-listed here because `ctak` shows it
   as a 4 GB correctness-of-memory cliff, not merely pause overhead.
3. **P5 compiler passes** — constant folding, DCE, peephole; moderate,
   broad, and independent of the above.

**Scoreboard:** re-run this same subset vs local Chibi after each item lands
(`PATINA=target/release/patina ./bench patina "tak ctak deriv diviter divrec
nboyer maze slatex compiler matrix"` in `~/Project/r7rs-benchmarks`); the
per-benchmark ratios, not the geomean alone, are what confirm a lever worked.

### 1.3 Scoreboard after the inline-opcode emission fix (2026-08-04, PR #14)

The §1.2 profile-first protocol paid for itself immediately: profiling `tak`
found the P3 inline opcodes had never been emitted for real programs (see the
§P3 2026-08-04 addendum for the alias mechanism). Post-merge sweep, same
subset, same local Chibi 0.12 baseline:

| Benchmark | 08-03 | 08-04 | Delta | Ratio vs Chibi |
|---|---|---|---|---|
| slatex | 34.4 | 32.7 | −5% | 0.38× → 0.36× — faster |
| matrix | 196.7 | 153.4 | −22% | 1.4× → **1.09× — near parity** |
| compiler | 120.9 | 103.8 | −14% | 1.7× → 1.44× |
| maze | 115.0 | 98.9 | −14% | 2.2× → 1.85× |
| diviter | 136.8 | 120.3 | −12% | 3.0× → 2.61× |
| deriv | 239.8 | 224.8 | −6% | 3.1× → 2.86× |
| nboyer | 93.3 | 72.8 | −22% | 3.7× → 2.85× |
| divrec | 140.1 | 110.7 | −21% | 3.7× → 2.95× |
| tak | 238.0 | 187.4 | −21% | 5.5× → 4.35× |
| ctak | crash | crash | — | §9.5 blowup, unchanged |

**Geomean ≈ 1.87× slower than Chibi** (was 2.2×). The deltas distribute
exactly along the mechanism: call/arithmetic/vector-dense workloads −21–22%
(matrix's jump to near-parity is the `vector-ref`/`vector-set!` opcodes),
Rust-native-bound slatex −5%. The §1.2 priority order stands — call-path
cost is still the defining gap (tak 4.35×), and the post-fix tak profile
ranks the remaining levers: per-call argument-`Vec` malloc/free in the
generic `Call` path, the callee-classification probe chain in `call_value`,
`Environment::get` for repeated `LoadGlobal` of self-recursive callees (P4),
and register-window memset in `call_closure`. Operational note for sweep
ETAs: `ctak`'s crash takes ~21 min wall clock — memory thrash accumulates
CPU slowly toward the 300 s cap.

## 2. Goals
- Cut per-call and per-allocation overhead measurably (target **2–5×** on arithmetic/list-heavy code from P2+P3).
- Make VM performance **measurable** and regression-guarded.
- Add **memory reclamation** (correctness for long-running programs) without destabilizing the test baseline.
- Keep every change readable: fast paths fall back to the *existing* slow path, so semantics never diverge.

## 3. Non-goals (deferred — clarity tradeoff)
Flat `Vec<u32>` bytecode encoding, threaded dispatch, liveness-based register allocation, NaN-boxed inline floats, bytecode serialization, continuation stack-slicing, JIT. Tracked in `PRD/VM_OPTIMIZATION_ROADMAP.md` (P2/P6/P7/P8/P9/P10). If revisited, the readable match-based loop must remain as a documented reference path.

---

## 4. Work items

### P0 — Repoint benchmarks at the VM + baseline  *(done — PR #154)*
The Criterion harness benchmarks the tree-walker. Parameterize `scheme_benchmarks.rs` (or add a sibling) to construct the VM backend, and check in a baseline for tak/fib/ack/nqueens/deriv/primes + list/vector/numeric programs (`crates/patina-tests/bench_programs/*.scm`). Keep `scripts/bench_compare.sh` (wall-clock VM-vs-tree-walker via `(current-jiffy)`) as a cross-check.
- **Acceptance:** `cargo bench -p patina-tests` exercises the VM; baseline numbers committed.

### P1 — Near-free clone removals  *(done — PRs #150, #155)*
1. ~~**`free_vars` clone.**~~ **Done — PR #155** (`Heap::get_vm_closure_code_id` reads only `code_id`; `resolve_closure` uses it; slot reads stay via `get_vm_closure_free_var`).
2. ~~**`arg_vals.clone()`**~~ **Done — PR #150** (as `primitive_procedure` + `call_primitive_proc` check-then-move split; same effect as the planned `&[TaggedValue]` signature).
- **Acceptance:** `cargo test` green; closure/tail tests (`tail_recursion.rs`, `cps_features.rs`) unchanged.

### P2 — Fast primitive dispatch: integer IDs + wire `CallPrimitive`  *(done — #149 runtime index cache; #158 compile-time wiring, 2026-07-29)*
Eliminate the string `HashMap` lookup on the hot path. **Status:** PR #149 delivered item 1 in an alternate form (Vec-indexed registry, `resolve_index`/`apply_by_index`/`apply_cached`, index cached per `Procedure::Primitive` instance). PR #158 delivered items 2–3: pass 5 emits `CallPrimitive { func_id, name, args, dst }` for `GlobalRef` callees that resolve to registry primitives (`compiler/primitive_calls.rs`), skipping the callee `LoadGlobal`, `get_procedure`, and the frame push; tail position emits `CallPrimitive` + `Return`.

**Deviations from the spec below (#158 findings):**
- **Runtime deopt replaced the static shadowing scan.** Programs and libraries compile *per form*, so a same-unit define-scan cannot see a later form rebinding a name an earlier form compiled against. Instead `Define`/`StoreGlobal` mark overwritten primitive bindings in a `VmState::shadowed_primitives` bitset (indexed by registry id); each `CallPrimitive` checks its bit (load+mask) and deoptimizes to name-lookup dispatch via `call_value` (extracted from the `Call` arm). Exact R7RS top-level redefinition semantics across forms, files, and the REPL — verified by dedicated tests in `patina-tests/tests/vm_callprimitive.rs`.
- **Exclusion is by qualified-name prefix** (`patina.internal.control/`, `patina.internal.errors/`), not a `const` name set — covers the `vm_control_primitive` interception list, `apply`, and the prompt machinery in one predicate (`primitive_calls::is_excluded`).
- **Measured (#158, interleaved drift-checked A/B vs main):** sum −28–33%, map/make/append −21–23%, factorial −13–21%, tak −17%, fib −23% (wall-clock; Criterion's fib group swung ±30% run-to-run that session). `reverse`-style benches (one primitive call per iteration) are flat, as expected — the win scales with call frequency.
1. **Deterministic IDs in the registry** (`registry.rs`). Add an ordered `by_id: Vec<PrimitiveFn>` plus `id_index: HashMap<String, PrimId>`; `register()` (`:114`) pushes and records the index for both qualified and bare names. Registration order is fixed (`primitives/mod.rs`), so IDs are stable. Add `id_of(name) -> Option<PrimId>` and `apply_by_id(id, args, ctx)` (reuses `apply_tagged`'s body minus the string lookup). Define the id newtype in `patina-primitives`; make `patina-vm`'s `PrimitiveFnId` a `From`-convertible wrapper (no new crate dependency).
2. **Resolve name→ID at compile time.** Thread a read-only resolver into `compile_with_qq`/Pass-5 codegen (`crates/patina-vm/src/compiler/pass5_codegen.rs`) from the registry the backend already owns. In the `App` arm, when the callee is a non-shadowed `GlobalRef` to a primitive, emit `CallPrimitive { func_id, args, dst }` instead of `Call`.
3. **Wire the runtime arm** at `vm_state.rs:1462`: collect args, build the `VmApplyContext` (same pattern as `try_call_primitive`), call `apply_by_id` — no string hash, no `get_procedure` borrow, no `Procedure` downcast. Arity check stays inside `apply_by_id`.
- **Correctness gates (critical):**
  - **Exclude control primitives** intercepted by `vm_control_primitive` (`vm_state.rs:2350`) and `apply` — those MUST keep the `Call` path. Encode the exclusion as a `const` set referenced by codegen.
  - **Shadowing:** emit `CallPrimitive` only when the name is not redefined by a top-level user `define` in the unit (reuse Pass-1's define scan). Anything uncertain → fall back to `Call` (at worst slower, never wrong).
- **Acceptance:** disasm test shows `(+ 1 2)` emits `CallPrimitive`; full `cargo test`; `./scripts/run_chibi_tests.sh` 1163/1163.

### P3 — Specialized inline opcodes for the ~15 hottest primitives  *(done — #159, 2026-07-29)*

**Landed as specified**, with two scope notes: `Not` is dropped (`not` is
Scheme-defined in `lib/scheme/base/lists.scm`, so it never resolves to a
registry primitive — a `Not` opcode would be dead), leaving **14 opcodes**;
and each opcode carries the `(func_id, name)` deopt pair so the shadow-bit
check guards every fast path. All fallbacks funnel through
`exec_call_primitive` — the registry handler — so behavior is identical to
the generic path by construction. Emission lives in
`primitive_calls::inline_op_for` + `pass5_codegen::primitive_call_instruction`;
dispatch arms in `vm_state.rs`; ISA documented in `docs/VM_ISA.md` §4.

**2026-08-04 emission fix.** The §1.2 profile-first pass on `tak` found the
opcodes were never emitted for real programs: `inline_op_for` keyed on the
*binding's* qualified name, but stdlib bindings carry the installing internal
library's name (e.g. `patina.internal.numbers/<` from `internal_numbers.rs`),
which only reaches the registry entry through `resolve_index`'s short-name
fallback — so the exact match silently returned `None` and every site stayed
on `CallPrimitive`. Unit tests missed it because `install_primitives()`
stamps registry-canonical names; only the library-loader path aliases. Fixed
by resolving the entry and keying `inline_op_for` (and an added exclusion
re-check) on the entry's own qualified name — the contract
`ResolvedPrimitive::inline` had documented all along. Regression test
compiles `<` bound the library-loader way and asserts the `Lt` opcode.
Measured (interleaved A/B ×3): fib(32) −16%, tak(32,16,8) −9%; post-fix
profile shows the registry-dispatch path (`exec_call_primitive`,
`apply_by_index`, `check_arity`, handler bodies) gone from tak's hot list.
Bonus find from the same disasm: `(not (< y x))` costs a `LoadGlobal` + full
closure call per iteration (`not` is Scheme-defined) — a `not`+`JumpUnless`
fusion peephole is a P5 candidate.

**Trail: primitive redefinition semantics (future work).** The deopt bitset
(#158) gives exact R7RS top-level redefinition semantics at the cost of one
load+mask per optimized call. If that check ever needs to go away — or
redefinition needs different semantics — the design space, roughly in
ascending effort:
1. **Immutable library bindings (R6RS-style):** bindings imported from
   libraries may not be redefined; only true REPL toplevels stay dynamic.
   Kills the check for all library-compiled code. Requires an import-vs-
   toplevel distinction in `Environment`.
2. **Chez-style optimize levels:** a `--optimize-level` flag where level ≥2
   documents that primitive rebinding does not affect already-compiled call
   sites, and drops both the bit-check and the `Define`/`StoreGlobal`
   marking. Easy, but a semantics waiver.
3. **Code patching:** on first deopt, rewrite the instruction in the
   `CodeObject` (needs mutable code store + care with `Rc`-shared code).
   Removes the steady-state check without semantic compromise.
4. **N-ary canonicalization (Guile/Chez do this):** expand `(+ a b c)` into
   chained binary `Add`s at compile time — same association order as the
   handler's left fold, so float semantics are preserved. Extends the inline
   win to variadic call sites; orthogonal to the deopt question.
Known residual gap (documented in #158): rebinding a primitive via `import`
(rather than `define`/`set!`) after code referencing it was compiled did not
set the shadow bit. **Upgraded from "accepted" to a bug and fixed — see
P8.1** (PR #3), whose review repro showed it diverging from the tree-walker.

#### Original P3 spec
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

### P6 — Garbage collection  *(complete — stages 1-4, always on; stage 5 in `PRD/future/GC_STAGE5_PRD.md`)*
Designed and tracked in **`docs/GC_DESIGN.md`**, which supersedes the sketch that used to live here — it covers both backends (not just the VM), adds a `Collector`/`GcRoots` pluggability seam, and carries the complete root inventory and staging plan.

Both backends collect and reclaim cycles, verified by CI-enforced differential lanes (`PATINA_GC=0` vs the default adaptive mode vs `PATINA_GC_STRESS=1`, byte-identical, in release *and* in a debug build with use-after-free assertions live). Collection is **on by default** since stage 4c.

**Relevant to this track:** the safe-point check used to cost 2.5-3.5% of VM runtime with GC off and 13.7% with GC on even when no collection was due, because it polled per dispatched instruction a question whose answer only changes on allocation. Stage 4a (PR #8, 2026-08-03) moved the decision into `alloc_*` — the safe point is now one flag load; interleaved A/B shows GC-off at parity with `main` and the GC-on standing penalty gone (~1% residual vs a no-check control binary). Measurements in `docs/GC_DESIGN.md` §6.1. Remaining for stage 4b: the default-on flip and CI lanes.

### P7 — Slice-based primitive handler ABI  *(phase 1 done — PR #157, 2026-07-29; phase 2 open)*

**Findings from phase 1 (#157):**
- The tier counts in the spec below were wrong: the registry actually held **188 heap / 94 higher-order** registrations, because ~70 heap-only handlers (`cons`, `car`, `cdr`, `list`, most string/vector ops, exceptions) were misfiled as higher-order during the tree-walker extraction — using `ctx` only as `ctx.heap()`. #157 reclassified them; ~24 genuinely re-entrant or `fs()`-dependent handlers remain HO. Without the reclassification, the HO-boundary `to_vec()` would have regressed the hottest list paths.
- Measured (interleaved A/B vs main): map −6.8%, append −5.4%, vector-fill −5.4%, reverse −6.9%, vectors/sum −11.5%, factorial −4–8%, float_sum −7.9%, dynamic-wind nested −17.6%. fib/tak sit at ±1–2%: profiles show the primitive-call machinery itself got *cheaper* (`call_primitive_proc` 911→560 samples on identical fib workloads) with the delta absorbed by dispatch-loop codegen layout; P3 removes fib/tak from this path entirely.
- Bonus deletions: `call_any`/`call_any_sync` (primitive-from-primitive path) each paid an `args.to_vec()` purely for the old ABI — gone.

Eliminate the last per-primitive-call allocation: the register→`Vec<TaggedValue>` collect that every call still pays. Profiles show it as malloc/free attributed *under the callees* (`less_than`, `apply_by_index`, …) — the callee frame drops the argument vector the caller allocated. This is also why "why does `less_than` allocate?" was a false lead: the comparison itself has a clean no-allocation fixnum fast path.

**Current ABI** (`crates/patina-primitives/src/registry.rs`):
```rust
pub type TaggedHandler   = fn(&SharedHeap, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;   // ~290 handlers
pub type HOTaggedHandler = fn(&dyn ApplyContext, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>; // ~8 handlers
```

**Target ABI — two-tier, and the tiers matter:**
- `TaggedHandler` takes `&[TaggedValue]`. Heap-only handlers never touch `VmState`, so the slice may eventually point directly into the VM register file (`&state.registers[base..base+n]`) with **zero** per-call copying.
- `HOTaggedHandler` **keeps an owned `Vec`** (or copies internally). Higher-order handlers re-enter the VM (`apply_proc`, `eval_expr`), which mutates/reallocates the register file — a borrowed register slice would be invalidated mid-call. This split is the load-bearing design decision; collapsing the tiers is unsound.

**Migration plan:**
1. Change `TaggedHandler` to `&[TaggedValue]`; let the compiler enumerate the ~290 handler signatures. Most bodies use `args.len()`/`args[i]`/`.iter()` and compile unchanged; the minority that consume the `Vec` (`into_iter`, `remove`, destructuring moves) get index/copy rewrites (`TaggedValue` is `Copy`, so these are cheap and mechanical).
2. `apply_by_index`/`apply_cached`/`apply_tagged` take `&[TaggedValue]`; the `PrimitiveHandler::HigherOrder` arm materializes its `Vec` at the boundary.
3. Phase 1 (this item): VM/tree-walker call sites pass `&arg_vals` — saves the extra move-through and unlocks phase 2. Phase 2 (optional follow-up): the VM `Call` arm passes the register slice directly for heap handlers, deleting the collect itself. Phase 2 must respect the tier split above and keep owned args for control primitives, `Apply` (which splices lists), and continuation/parameter paths.
- **Estimated impact:** 5–10% on call-heavy code (profile-derived from the args-Vec alloc/drop share after wave 1).
- **Acceptance:** full `cargo test` + 1163/1163 chibi; `cargo bench -p patina-tests` vs the committed baseline shows no regression anywhere and improvement on `r7rs/fib`, `r7rs/tak`, `data/*`; tree-walker paths (`apply_primitive_tagged`, CPS eval) migrated in the same PR so the shared registry keeps one ABI.

**Related but separate (sized during the same investigation, not quick wins):** register-window zeroing removal needs a watermark redesign of the register arena (34 sites + continuation-capture interplay — `state.registers` is snapshotted whole by call/cc, and any latent read-before-write bug currently sees deterministic NULL rather than stale values). Defer until after P3, which changes the register traffic pattern anyway.

### P8 — Deopt correctness regressions  *(done — PR #3, 2026-07-30; found and fixed same day by post-P3 review)*

Two regressions in the P2/P3 deopt machinery, found by a post-landing review.
Both were demonstrated with repros, not just read from the code.

**P8.1 — `import` bypasses shadow-bit invalidation.** Only the
`Define`/`StoreGlobal` instruction handlers call
`mark_if_shadowing_primitive`; the Rust-side import machinery
(`vm_process_import_set` in `vm_state.rs`, `process_import_set` in
`backend.rs`) rebinds globals through `Environment::define` directly and
never sets the shadow bit. Rebinding a primitive name via
`(import (rename ...))` after code referencing it was compiled leaves stale
`CallPrimitive`/inline-opcode fast paths: the VM keeps calling the original
primitive while the tree-walker (name lookup per call) honors the rebind —
a backend divergence, not just a semantics waiver. Repro:
`(define (f p) (car p))` then `(import (rename (scheme base) (cdr car)))` —
`(f '(1 2))` returns `1` on the VM and `(2)` on the tree-walker. P3's
"accepted residual gap" note is upgraded by this repro to a bug.
- **Fix:** route every Rust-side global rebind through the same
  mark-before-overwrite choke point the instruction handlers use, in both
  import paths. Longer-term, if more Rust-side writers appear: an
  invalidation hook on `Environment` itself so the choke point is the layer
  that owns the mutation.
- **Acceptance:** a test proving an import rebind deoptimizes
  already-compiled call sites (VM result matches tree-walker); chibi stays
  1163/1163.
- **Fixed:** both import-set resolvers now funnel every installed binding
  through `import_define` → `mark_if_shadowing_primitive`. The mark also
  gained a value-identity guard (rebinding a name to the value it already
  has never deoptimizes), so re-imports and overlapping library exports stay
  free. Regression test: `vm_callprimitive.rs::import_rebind_deoptimizes`;
  VM and tree-walker verified to agree on the repro.

**P8.2 — Tail-position primitive sites lose proper tail calls on deopt.**
Pass 5 lowers a tail-position resolved-primitive call to `<prim-op>; Return`,
discarding `is_tail`. That is sound while the callee is a primitive (no
frame needed), but the *deopt* replacement is a frame-pushing `call_value` →
`call_closure`: after `(set! car <closure>)`, mutual tail recursion through
an already-compiled site grows the frame stack linearly — an R7RS §3.5
violation. Measured: 500k iterations = 109 MB max RSS vs 5.4 MB for the
equivalent plain mutual tail recursion.
- **Fix:** at deopt time, detect the tail shape — the site's next
  instruction is `Return` of the same `dst`, the exact pair pass 5 emits at
  tail sites — and dispatch with tail-call semantics (frame reuse) instead
  of `call_value`. The check reads the actual instruction stream, so it
  cannot go stale, and any coincidental match is semantically a tail call
  anyway; no ISA change. Requires factoring the `TailCall` arm's callee
  dispatch into a reusable helper (which also de-duplicates the
  `TailCall`/`TailApply` arms).
- **Acceptance:** rebound-primitive mutual tail recursion at 500k depth
  runs in flat memory (max RSS measured before/after the fix); results
  match the tree-walker; chibi stays 1163/1163.
- **Fixed:** `exec_call_primitive`'s deopt branch detects the tail shape
  against the live instruction stream and dispatches through the new
  `tail_call_value` (the factored-out `TailCall` arm body). Measured after:
  the 500k repro runs at **5.49 MB** max RSS vs 5.44 MB for the plain
  tail-recursion baseline (was 109 MB). Regression tests:
  `vm_callprimitive.rs::tail_deopt_returns_correct_result` and
  `::tail_deopt_runs_deep_mutual_recursion`.

---

## 5. Sequencing within the track
Original plan: **P0** → **P1** → **P2 → P3** → **P4 → P5**, with **P6** in parallel — all landed through P3/P6 as of 2026-08-03. Sequencing for what remains is now data-driven, per §1.2: **P7 phase 2 → P4 → (cross-track: weak continuation tables) → P5**, each preceded by a fresh profile and followed by the §1.2 scoreboard run. See `PRD/SNOW_AND_PERF_ROADMAP.md` for the M1–M4 interleave with Track L.

## 6. Risks & mitigations
- **Shadowed/redefined primitives** → conservative "no top-level define of this name" guard; fall back to `Call`.
- **Control primitives inlined by mistake** → explicit `const` exclusion set (the `vm_control_primitive` names + `apply`); single most important gate.
- **Error-message parity** → opcodes fall back to the named primitive on the error path, matching suite expectations.
- **GC risks** → covered in `docs/GC_DESIGN.md` §9–11 (hazards, feature-flag lanes, `--gc-stress`, poison mode).

## 7. Verification (track-wide)
- Routine: `cargo build --release && ./scripts/run_chibi_tests.sh` (must stay 1163/1163) after every item.
- Perf: `cargo bench -p patina-tests` (VM-backed after P0) vs baseline; `./scripts/bench_compare.sh` cross-check.
- Quality gate: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt`.
