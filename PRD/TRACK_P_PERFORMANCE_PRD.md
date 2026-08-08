# Track P — VM Performance (Clarity-Safe) PRD

**Created:** 2026-06-20
**Status:** In progress — first profile-driven wave landed 2026-07-25/26 (PRs #149, #150, #151, #152): **2.4× on call-heavy code, ~2–2.7× across the r7rs-benchmarks quick set**. See §1.1. Second wave, 2026-07-26/29: P0 (#154), P1.1 (#155), P7 phase 1 (#157), P2 `CallPrimitive` (#158), **P3 inline opcodes (#159) — the P2+P3 pair delivered 2–3.2× on arithmetic/list-heavy code in one day**. Remaining work re-ranked by the 2026-08-03 r7rs sweep (§1.2): call path first (P7 phase 2, then P4), weak continuation tables (cross-track, `PRD/future/GC_STAGE5_PRD.md`), then P5 compiler passes. **P6 GC: complete through stage 4c (PRs #4-#6, #8, #10, #11, 2026-08-01/03)** — both backends collect, **always on** at zero standing cost (safe point = one flag load; on-vs-off at parity on dispatch- and alloc-heavy workloads); CI enforces the byte-identical differential lanes, whose env hooks are the only remaining use of `PATINA_GC`/`PATINA_GC_STRESS`. Stage 5+ pause work tracked in `PRD/future/GC_STAGE5_PRD.md`. **P9 (2026-08-07, PR #20)** moved `not`, the car/cdr compositions, and the numeric predicates from Scheme into the registry — **geomean vs Chibi 1.44× → 1.16×** (§1.5), with compiler at near parity (1.09×).
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

### 1.4 Scoreboard after the call-dispatch pair (2026-08-05, PRs #16 + #17)

The two levers §1.3 ranked first both landed: #16 gates the callee-
classification probe chain behind a closure-first type check, and #17
removes the per-call argument-`Vec` — register-to-register copy for `Call`,
a 16-slot stack stage for `TailCall`, unchanged collected-`Vec` for
non-closure callees. (#17's first draft shared `CallPrimitive`'s scratch
buffer instead; it regressed a dynamic-wind/values microbench 7% — per-call
take/restore buys nothing in re-entrant regions — and was redesigned before
landing. The scratch-buffer approach for closure-call args is a recorded
negative result; do not revisit.) Compound sweep vs the §1.3 baseline, same
local Chibi 0.12:

| Benchmark | 08-04 (#14) | 08-05 (#16+#17) | Delta | Ratio vs Chibi |
|---|---|---|---|---|
| slatex | 32.7 | 29.9 | −9% | 0.33× — faster |
| matrix | 153.4 | 124.5 | −19% | **0.88× — faster than Chibi** |
| compiler | 103.8 | 81.4 | −22% | 1.13× |
| maze | 98.9 | 77.8 | −21% | 1.46× |
| diviter | 120.3 | 77.0 | −36% | 1.67× |
| deriv | 224.8 | 154.1 | −31% | 1.96× |
| nboyer | 72.8 | 56.5 | −22% | 2.22× |
| divrec | 110.7 | 83.8 | −24% | 2.24× |
| tak | 187.4 | 146.6 | −22% | 3.40× |
| ctak | crash | crash | — | §9.5 blowup, unchanged |

**Geomean ≈ 1.44× slower than Chibi** (2.2× on 08-03 → 1.87× → 1.44× in
three days); matrix joins slatex on the faster-than-Chibi side. Deepest
cuts landed exactly where both levers stack: list/symbolic churn (diviter
−36%, deriv −31%) paid the probe chain and the args-Vec on every cons-heavy
call. Remaining ranking is unchanged in kind, thinner in expected yield:
tak (3.40×) still marks the call-path gap — next levers are P4
slot-indexed globals (four `LoadGlobal tak` per iteration), the
register-window zeroing in `call_closure`, and the `classify_callee`
unification (also adopting `Apply`/`TailApply`); weak continuation tables
(ctak, GC stage 5) remain priority 2. Sweep hygiene, learned twice: the
harness resolves `$PATINA` per benchmark — sweep a *copied* binary, with
`PATINA_HOME` set so the copy finds `lib/` (a bare copy fails every
benchmark instantly with a bogus circular-`(scheme base)` error).

### 1.5 Scoreboard after the P9 registry move (2026-08-07, PR #20)

P9 moved `not`, all 28 car/cdr compositions, and the numeric sign/parity
predicates from Scheme definitions into the registry (see §P9 — found by
asking why hand-inlining `(not (< y x))` made tak 40% faster). Compound
sweep on merged main (`8b09501`) vs the §1.4 baseline, same local
Chibi 0.12:

| Benchmark | 08-05 (#16+#17) | 08-07 (#20) | Delta | Ratio vs Chibi |
|---|---|---|---|---|
| slatex | 29.9 | 26.3 | −12% | 0.29× — faster |
| matrix | 124.5 | 117.2 | −6% | 0.83× — faster |
| compiler | 81.4 | 78.6 | −3% | **1.09× — near parity** |
| maze | 77.8 | 65.8 | −15% | 1.23× |
| diviter | 77.0 | 52.4 | −32% | 1.14× |
| deriv | 154.1 | 142.0 | −8% | 1.81× |
| nboyer | 56.5 | 45.2 | −20% | 1.77× |
| divrec | 83.8 | 56.5 | −33% | 1.51× |
| tak | 146.6 | 93.7 | −36% | 2.17× |
| ctak | 74.6 | 57.5 | −23% | n/a (Chibi crashes) |

**Geomean ≈ 1.16× slower than Chibi** (2.2× on 08-03 → 1.87× → 1.44× →
1.16× in five days). Every benchmark improved, and the deltas distribute
exactly along call density of the moved procedures: tak (`not` per
iteration) −36%, divrec (`cddr` in the hot loop) −33%, diviter −32%,
ctak −23%, nboyer −20% (imports `(scheme cxr)` — the four-deep move
applies), maze −15%. deriv's −8% confirms it is cons-churn/`map`-bound,
not accessor-bound.

**The lever rankings above this section are now stale**: they were derived
from profiles in which 40% of tak's samples were `not`'s closure call. The
remaining gap is concentrated in tak (2.17×), deriv (1.81×), and nboyer
(1.77×) — the next item must start with a fresh profile of those three
before choosing among the queued candidates (P4 slot-indexed globals,
register-window zeroing, `classify_callee` unification, `not`+`JumpUnless`
fusion from P5).

### 1.6 Fresh profile of the remaining gap (2026-08-07, post-P9)

Sampled the three worst ratios on main `1abea44` (`/usr/bin/sample`, 10 s
top-of-stack, ~8300 samples each; workloads: tak(40,20,11), the deriv kernel
×2M, nboyer n=4; artifacts in `target/profiles/*_post_p9.txt`). Nothing
primitive-dispatch-related survives in any hot list — P2/P3/P9 closed that
chapter. What remains, as a share of samples:

| Cost center | tak | deriv | nboyer |
|---|---|---|---|
| `dispatch_one_instruction` (loop body incl. inline opcodes) | 57% | 50% | 62% |
| `run_loop_until` (loop wrapper; mostly return attribution) | 10% | 7% | 10% |
| `VmState::reg`/`set_reg`/`set_reg_in_frame` | ~8% | ~6% | ~7% |
| `call_closure_from_regs`(+`_resolved`/tail) frame setup | ~8% | ~6% | ~7% |
| `memset_pattern16` (register-window zeroing) | 4% | 7.5% | 3% |
| `Environment::get` + memcmp + `frame_globals` (P4) | ~7% | ~4.5% | — |
| malloc/free + `alloc_pair` + GC sweep (cons churn) | — | ~6.5% | ~2.5% |
| `pop_resolved_winds` + `pop_exception_handlers` per return | 1.5% | 1.4% | 1.2% |

**Two findings the old profiles under-ranked:**

1. **The register accessors don't inline.** `reg`/`set_reg` carry
   `#[inline]`, but `dispatch_one_instruction` is large enough that LLVM
   declines — every access pays a real call plus `frames.last().expect()`
   and two bounds checks. ~6–8% on all three workloads. Fix:
   `#[inline(always)]`, possibly hoisting `frame_base` once per dispatch.
2. **`code_store` is an FxHashMap hit on every closure call.**
   `CodeObjectId` is minted from a sequential `AtomicU32`
   (`pass5_codegen.rs:34`), so the store can be a `Vec` indexed by id —
   the same trick as #149's registry. The lookup (plus `Rc` clone) sits
   inside `call_closure_from_regs`.

**Re-ranked levers:**

1. **`#[inline(always)]` accessors + Vec-indexed `code_store`** — one
   small PR, ~10–14% combined target on all three benchmarks, mechanical,
   proven pattern (#149).
2. **Register-window zeroing** (watermark redesign; biggest on deriv at
   7.5%) — still the known-risky item (34 sites, call/cc snapshot
   interplay), but now near the top on merit.
3. **P4 slot-indexed globals** — ~4–7%; smaller than previously billed.
4. **P5 instruction-count reductions** (`not`+`JumpUnless` fusion,
   folding, peephole) — with the dispatch loop at 50–62% of runtime,
   fewer dispatched instructions is the macro lever long-term.
5. deriv/nboyer's residue increasingly points at **allocation/GC**
   (generational collection, GC stage 5 priority 3) rather than the call
   path.

### 1.7 Scoreboard at 2026-08-07 evening: sub-parity — geomean 0.93× (post #23/#24)

Levers 1 and 2 from the §1.6 ranking landed as PR #23 (Vec-indexed
`code_store` + `inline(always)` register accessors) and PR #24
(dispatch-loop residency: loop-cached code `Rc`, hoisted frame base with
`reg_at`/`set_reg_at`, self-tail-call fast path — plus a review pass that
collapsed dispatch to a single frame access per instruction). Full sweep on
merged main `091f18a`, copied binary + `PATINA_HOME`, vs the same-machine
Chibi 0.12 baseline:

| Benchmark | Patina (s) | Chibi (s) | Ratio | post-P9 ratio |
|---|---|---|---|---|
| slatex | 24.7 | 91.4 | **0.27×** | 0.29× |
| matrix | 98.5 | 141.3 | **0.70×** | 0.83× |
| diviter | 39.9 | 46.1 | **0.87×** | 1.14× |
| compiler | 64.9 | 72.0 | **0.90×** | 1.09× |
| maze | 53.2 | 53.4 | **1.00×** | 1.23× |
| divrec | 43.6 | 37.5 | 1.17× | 1.51× |
| nboyer | 32.5 | 25.5 | 1.27× | 1.77× |
| deriv | 113.1 | 78.5 | 1.44× | 1.81× |
| tak | 73.8 | 43.1 | 1.71× | 2.17× |
| ctak | 58.6 | CRASHED | — | — |

**Geomean of the nine ratio benchmarks: 0.93× — faster than Chibi.**
(1.16× → 0.93×; the arc since 08-03: 2.2× → 1.87× → 1.44× → 1.16× →
0.93×.) Five of nine at or past parity; ctak completes where Chibi
crashes. Per-benchmark deltas vs post-P9: −16% to −28% everywhere except
ctak (+1.9%, within single-run drift — its runtime is call/cc-capture
dominated, which these levers don't touch).

**Remaining gap ranking** (tak 1.71×, deriv 1.44×, nboyer 1.27×): the
§1.6 residue analysis stands — next levers are the pc-in-loop-local
refactor (its own PR; every `frame.pc` reader needs a sync point), the
per-primitive-call registry `Rc` clone, `value_buffer` recycling on
multi-value returns (all three surfaced by #24's review), then P4
slot-indexed globals and P5 instruction-count reductions. deriv/nboyer
residue still points at allocation/GC (generational, stage 5 priority 3).

### 1.8 Fresh profile of the remaining gap (2026-08-08, post #23/#24)

Re-profiled the three worst ratios on main `f1d60a1` before choosing the
next lever (same protocol as §1.6: `/usr/bin/sample`, 10 s top-of-stack,
~8000 samples; artifacts in `target/profiles/*_post_p24.txt`). #23/#24
fully banked their targets: the register accessors and the `code_store`
hash are absent from every hot list, and deriv's `memset_pattern16` fell
7.5% → 2.0% (the self-tail-call fast path). What remains:

| Cost center | tak | deriv | nboyer |
|---|---|---|---|
| `dispatch_one_instruction` + `run_loop_until` | 76% | 66% | 76% |
| Globals cluster: `Environment::get` + memcmp + `frame_globals` + `get_vm_closure_globals` (P4) | 10.4% | 7.3% | ~0% |
| `call_closure_from_regs` (+resolved/tail/store_args) | 7.4% | 7.0% | 7.4% |
| malloc/free + Vec growth (primitive temp Vecs, `Apply` path) | ~0.4% | ~6.5% | ~0.7% |
| `memset_pattern16` (register-window zeroing) | 3.2% | 2.0% | 3.3% |
| `alloc_*` + GC sweep/visit | — | 2.6% | 3.1% |
| `pop_resolved_winds` + `pop_exception_handlers` | 1.4% | 1.1% | 1.1% |
| `read/write_mutable_cell` | — | — | 2.1% |

**Ranking shifts vs §1.7's queue:** P4 slot-indexed globals became the top
addressable lever (it *grew* in relative terms as everything around it
shrank — and tak is the worst ratio). deriv's distinctive cost is
temporary-`Vec` churn on the primitive/`Apply` path: the `list` handler
builds and frees a `Vec::from_iter` per call, plus `list_to_vec` under
`Apply`/`map` — same family as the queued `value_buffer` recycling. The
per-primitive-call registry `Rc` clone is **not visible** (≤0.5%; P2/P3/P9
took these benchmarks off the registry path) — dropped from the queue.
Register-window zeroing is down to 2–3.3% — deprioritized. The
pc-in-loop-local refactor can't be sized by sampling but targets the
dominant dispatch-residency block; still queued.

### 1.9 Scoreboard after P4 slot-indexed globals (2026-08-08)

Sweep with the P4 branch binary (copied binary + `PATINA_HOME`), same
local Chibi 0.12 baseline:

| Benchmark | 08-07 (#23/#24) | P4 | Delta | Ratio vs Chibi |
|---|---|---|---|---|
| slatex | 24.7 | 24.5 | −1% | 0.27× — faster |
| matrix | 98.5 | 98.5 | 0% | 0.70× — faster |
| diviter | 39.9 | 39.9 | 0% | 0.86× — faster |
| compiler | 64.9 | 64.6 | −0.5% | 0.90× — faster |
| maze | 53.2 | 51.3 | −3.6% | **0.96× — crosses parity** |
| divrec | 43.6 | 41.8 | −4.1% | 1.11× |
| nboyer | 32.5 | 33.0 | +1.5% | 1.29× |
| deriv | 113.1 | 107.6 | −4.9% | 1.37× |
| tak | 73.8 | 66.7 | −9.6% | 1.55× |
| ctak | 58.6 | 55.9 | −4.6% | n/a (Chibi crashes) |

**Geomean of the nine ratio benchmarks: 0.91×** (was 0.93×); six of nine
now at or past parity. The deltas distribute exactly along `LoadGlobal`
density: tak (self-call through a cached global site every iteration)
−9.6%, deriv/divrec/ctak −4–5%, matrix/diviter (loop-local and
vector-opcode bound) flat, nboyer's +1.5% is single-run drift — its
profile has no `Environment::get`. The remaining gap ranking is
unchanged: tak 1.55×, deriv 1.37×, nboyer 1.29×, with §1.8's queue
(P5 instruction-count reductions, deriv's `Vec`-churn cluster,
pc-in-loop-local) still standing and allocation/GC still the deriv/nboyer
residue.

### 1.10 Scoreboard after P5 wave 1 (2026-08-08)

Sweep with the P5 branch binary, same local Chibi 0.12 baseline:

| Benchmark | P4 (§1.9) | P5 | Delta | Ratio vs Chibi |
|---|---|---|---|---|
| slatex | 24.5 | 23.5 | −4% | 0.26× — faster |
| matrix | 98.5 | 90.1 | −8.5% | 0.64× — faster |
| diviter | 39.9 | 33.6 | −16% | 0.73× — faster |
| compiler | 64.6 | 59.3 | −8% | 0.82× — faster |
| maze | 51.3 | 46.9 | −8.6% | 0.88× — faster |
| divrec | 41.8 | 35.2 | −16% | **0.94× — crosses parity** |
| nboyer | 33.0 | 27.2 | −17.5% | 1.07× |
| tak | 66.7 | 47.8 | −28% | 1.11× |
| deriv | 107.6 | 97.9 | −9% | 1.25× |
| ctak | 55.9 | 53.9 | −3.5% | n/a (Chibi crashes) |

**Geomean of the nine ratio benchmarks: 0.79×** (0.91× after P4; the arc
since 08-03: 2.2× → 1.87× → 1.44× → 1.16× → 0.93× → 0.91× → 0.79×).
Seven of nine at or past parity. Every benchmark improved — instruction-
count reduction is a broad lever, exactly as §1.6 predicted when dispatch
residency crossed 50%. tak collapsed from 1.55× to 1.11× (its loop went
28 → 19 dispatches); the worst remaining ratio is now **deriv at 1.25×**,
whose profile is cons-churn/`Vec`-alloc bound (§1.8) — the next levers are
the deriv `Vec`-churn cluster (scratch-free `list`, `Apply`'s
`list_to_vec`, `value_buffer` recycling), compare-branch fusion
(`Lt`+`JumpUnless`, the §P5 follow-on), and allocation/GC (generational,
stage 5 priority 3). Per protocol, re-profile before choosing.

*Queue update (2026-08-08, after this sweep):* the `Vec`-churn cluster and
`value_buffer` items are closed in §P10 — the first landed, the second
turned out to be dead code. **pc-in-loop-local was built, measured, and
rejected** (§P10 negative result); the standing queue is now
compare-branch fusion and generational GC.

### 1.11 Scoreboard after test-branch fusion (2026-08-08)

Sweep with the fusion binary, same local Chibi 0.12 baseline:

| Benchmark | P5 | fusion | Delta | Ratio vs Chibi |
|---|---|---|---|---|
| slatex | 23.5 | 24.6 | +5.1% | 0.27× — faster |
| matrix | 90.1 | 90.4 | +0.3% | 0.64× — faster |
| diviter | 33.6 | 32.8 | −2.3% | 0.71× — faster |
| compiler | 59.3 | 59.9 | +1.1% | 0.83× — faster |
| maze | 46.9 | 46.9 | 0% | 0.88× — faster |
| divrec | 35.2 | 34.4 | −2.1% | 0.92× — faster |
| nboyer | 27.2 | 27.1 | −0.5% | 1.06× |
| tak | 47.8 | 48.2 | +0.9% | 1.12× |
| deriv | 97.9 | 95.1 | −2.8% | 1.21× |
| ctak | 53.9 | 56.6 | +4.9% | n/a (Chibi crashes) |

**Geomean: 0.79×** — unchanged from the P5 sweep. The predicted wins landed
where the shape census said they would (deriv −2.8%, diviter −2.3%,
divrec −2.1%, all cons/predicate-dense), but slatex (+5.1%) and ctak
(+4.9%) drifted up on single runs and cancel them in the geomean. Neither
executes a fused predicate in its hot path — slatex is string/IO-bound and
ctak is capture-bound — so both read as sweep noise rather than
regression; the interleaved A/Bs on the same machine were tight to ±0.5%.
Worth remembering when reading any single sweep: **the geomean moves less
than the per-benchmark deltas warrant when two noisy entries sit at the
extremes.**

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

**Landed as specified**, with two scope notes: `Not` is dropped (`not` was
Scheme-defined at the time, so it never resolved to a registry primitive),
leaving **14 opcodes** — P9 later moved `not` into the registry and added
the 15th;
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

### P4 — Indexed / cached global access  *(done — #151 FxHashMap; slot cache landed 2026-08-08)*
`LoadGlobal`/`StoreGlobal` hash a `String` and walk the parent chain. **Status:** #151 swapped `Environment`'s maps to `FxHashMap` (−3–5% on globals-heavy code). The slot cache landed 2026-08-08 (branch `track-p-p4-global-slot-cache`), designed around §1.1's negative result — nothing caches the globals *environment*; `frame_globals` still resolves per instruction:
- **Slot-stable bindings:** `Environment`'s simple bindings are now a name→slot `FxHashMap<String, u32>` plus an append-only `Vec<TaggedValue>` slot table. A binding's slot never moves or disappears; redefinition overwrites the slot in place, so a cached slot always reads the *current* value of the same name.
- **Per-site inline cache:** `CodeObject` carries a pc-indexed `Vec<Cell<GlobalCacheEntry>>` (16 bytes/instruction; no ISA change — `pc` is already live in the dispatch arms). `LoadGlobal`/`StoreGlobal` validate the entry and collapse to an id compare + slot read, skipping the string hash + memcmp.
- **Soundness:** entries key on a process-unique, never-reused `u64` environment id (not an address — a dropped env's recycled address would let an address-keyed cache falsely hit). Only names resolving in the queried environment's *own* table are cached; parent-resolved names always take the full lookup, so a later local define that changes resolution can never be masked. Stale entries can only miss.
- **Measured (interleaved A/B ×3 vs main, wall-clock):** globals-heavy set!/read loop −41%, tak(32,16,8) −6.6%, deriv kernel −4.3%, nboyer flat (its profile has no `Environment::get` — confirms zero overhead where the cache can't help). Matches the §1.8 shares: the hash+memcmp portion is gone; the residual `frame_globals` heap-borrow + `Rc` clone per access (~2%) stays and is a candidate for a follow-up only if a future profile ranks it.
- **Semantics tests:** `patina-tests/tests/vm_global_cache.rs` — set!/redefine visibility through cached sites, store/load site agreement, slot stability under map growth, forward references, deep cached self-recursion, and library-vs-toplevel environment isolation (`map`'s internal `%map-cars` site vs a same-named toplevel define). Chibi 1163/1163.

### P5 — Cheap, readable compiler passes  *(first wave done — 2026-08-08)*
The compiler ran zero optimization passes; the 5-pass pipeline is structured for insertion (`docs/VM_COMPILER.md §12`). The first wave was chosen by disassembling the §1.8 kernels rather than from the spec's list — tak's 31-instruction loop body wasted ~9 dispatches/iteration on staging `Move`s, re-executed `LoadImmediate`s, an unfused `not`, and a `Move`-to-join-then-`Return` tail. All four items are emission-level (instructions chosen or replaced in place, never deleted — no pc remapping exists):

1. **In-place operands** — resolved-primitive calls whose args are all ANF atoms read `LocalRef` operands from their home registers; `CallPrimitive` and the inline opcodes accept arbitrary registers, so the pass-4 staging temps are only used when a non-atomic arg requires ordered evaluation.
2. **Immediate operands** — `AddImm`/`SubImm`/`LtImm`/`NumEqImm` absorb a fixnum-literal *right* operand. Right side only, even for commutative ops: **the first draft absorbed either side of `+`/`=` and was caught by the existing `set_after_use_deoptimizes` test** — the deopt passes `[a, imm]` to whatever the name is currently bound to, and `(set! + -)` makes operand order observable. Recorded so nobody re-tries the swap.
3. **`not`-branch fusion** (generalized to all predicates in wave 2, §P5.2) — a fused branch replaces the `Not` whose result feeds a `JumpUnless`, which is *kept at the next pc*: the fast path branches directly (one dispatch instead of two), and the shadowed-`not` deopt calls the rebound binding into `dst` and falls through to the kept `JumpUnless` — exact R7RS redefinition semantics with zero new deopt machinery, and jumps into the fused pc stay valid because it writes `dst` exactly like the pair it replaced.
4. **Return threading** — `Jump→Return` and `Move d←s; Jump→Return d` rewrite to direct `Return`s in place (orphaned slots stay, unreachable). Tail `if` arms return in one dispatch; sites the rewrite turns into `<op> dst; Return dst` pairs are recognized by the P8.2 tail-shape deopt, extending proper-tail behavior.

**Measured:** tak kernel 28 → 19 dispatches/iteration; interleaved A/B ×3 vs main: **tak −28%, nboyer −16%, deriv −10.5%**, globals loop −5%. Emission tests in `patina-vm/tests/callprimitive.rs` (in-place, imm-absorption, left-literal stays registered, fusion shape, effectful-arg fallback); semantics in `patina-tests/tests/vm_instruction_fusion.rs` (both fused branches, rebind deopts through the fused site, overflow promotion, deopt operand order, deep threaded recursion). Chibi 1163/1163.

**Still open from the original list:** constant folding (hazard: a folded call leaves no deopt escape for redefinition — needs the §P3 design-space decision first), DCE, and the natural next fusion: compare-branch (`Lt`+`JumpUnless` etc.) via the same kept-landing pattern. Skip contification / copy-propagation / liveness regalloc (deferred).
- **Acceptance:** golden disasm tests; `cargo test`.

### P5.2 — Test-branch fusion  *(done — 2026-08-08)*

Wave 2 of the instruction-count work, and the follow-on §P5 named. Chosen
the same way as wave 1 — by counting shapes in the emitted bytecode of the
benchmark set (`patina --dump`) rather than from the spec's list:

| Shape | Sites |
|---|---|
| predicate → `JumpUnless` (fusable, this item) | **32** — `null?` 16, `eq?` 8, `=`-imm 3, `pair?` 3, `<`-imm 1, `=` 1 |
| predicate → `NotJumpUnless` (3-instruction chain, not fused) | 11 |
| `JumpUnless` fed by a call/move (not fusable) | 23 |

**Landed:** `NotJumpUnless` is *replaced* by a single `TestJumpUnless`
carrying a `TestOp` discriminant (`Not`, `NullP`, `PairP`, `VectorP`,
`Eq`, `Lt`, `NumEq`). One variant, one dispatch arm, one deopt path and
one emission site cover all seven predicates — the answer to the
combinatorial worry recorded when wave 1 landed (a variant per predicate
would have been eight, doubling again with the imm forms). The kept
`JumpUnless` now serves *two* fall-through roles, not just deopt: a
non-fixnum comparison (`(< 1.5 2)`) also routes through
`exec_call_primitive` and lands on it, so fused and unfused forms agree by
construction rather than by parallel implementation.

**Measured (interleaved A/B ×3–9 vs main, wall-clock):** `null?`-driven
list walk **−7.5%**, deriv **−2.4%**, nboyer **−1.2%**, tak parity.

**Negative result folded in:** the first version put *all* seven
predicates behind the inner `match`, which cost **+0.9% on tak** (9 pairs,
consistent) — `not` fusion had been a specialized arm before, and became
an extra jump-table dispatch. Hoisting `Not` to a predictable
compare-and-branch ahead of the match restored parity while keeping every
other gain. If more predicates join, measure the same way before assuming
the jump table is free.

**Two more negative results, from the post-landing review round:**
- **Hoisting by emission count is wrong.** The review correctly caught that
  the hoist comment claimed `not` was the most-emitted predicate when a
  census says otherwise (`null?` 150 sites, `pair?` 71, `not` 47 across
  `lib/**` + benchmarks). Acting on that census made things *worse*:
  hoisting `null?`+`pair?` as well cost **+1.7% on tak** and bought
  **nothing** on a `null?`-driven loop. Static site counts measure BTB
  pressure; what the compare chain must be ordered by is *dynamic*
  weight, and `not` sits in tight recursive loops. The chain stays at one,
  now with an accurate comment.
- **`unreachable!` on the hoisted arm is load-bearing.** Spelling the
  hoisted `Not` expression out in the residual match instead — safer-looking,
  and the review's suggestion — cost **+1.2% on tak**: it keeps a live
  switch case LLVM otherwise prunes.
- Also measured and dropped: hoisting `values_eq`'s raw-bits compare into
  the caller to skip the heap borrow. It measured **+2% on an `eq?`-heavy
  loop** — LLVM already hoists that compare through the inlined callee, so
  the hand-written version only added checks.

**Not done:** the imm operand forms (`(if (= n 0) …)` → `LtImm`/`NumEqImm`
+ branch) are 4 of the 32 sites and need a second operand shape on
`TestJumpUnless`; and the 11 predicate→`not`→branch chains, which would be
a 3-into-1 fusion. **Where they belong:** the review identified that
fusion runs at the `If` emission site while the invariant it creates
("replace in place, keep the neighbour at i+1") is the same one
`thread_returns` owns as a post-pass. Moving fusion into
`finalize_instructions` would make the kept-landing contract structural
rather than a caller protocol, delete the `patch_jump` arm and the `fused`
threading, and — the reason it matters for the next wave — put the
arbitration between literal-absorption and branch-fusion somewhere that can
see both. Today they compete: `primitive_operands` absorbs the literal at
the `App` site, so `(if (= n 0) …)` is already `NumEqImm` by the time the
`If` arm looks, which is exactly why those 4 sites are unfused. Do the
post-pass move together with the imm forms, with its own A/B.

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

### P9 — Registry-native trivial stdlib procedures  *(done — 2026-08-07)*

Found by asking why hand-inlining `(not (< y x))` as `(< y x)` with swapped
branches made `tak` 40% faster: `not`, the car/cdr compositions, and the
numeric sign/parity predicates were **implemented in Scheme**
(`base/lists.scm`, `base/numbers.scm`), so every use paid a full closure call
— LoadGlobal, arity check, register window, frame push, Return — for
one-instruction work. Call-site counts across the r7rs-benchmarks suite:
`not` 399, `cadr` 220, `cddr` 91, `caddr` 60, `zero?` 42.

**Landed:** `not` (registry + the `Not` inline opcode P3 specced and
dropped), all 28 car/cdr compositions — two- through four-deep — via a
depth-agnostic `cxr!` macro (registry → `CallPrimitive`; one heap borrow per
call, per-step `is_pair` guards emitting the same "car/cdr expects a pair"
errors the old Scheme bodies produced), and
`zero?`/`positive?`/`negative?`/`odd?`/`even?` (registry → `CallPrimitive`;
slow paths compose the same heap ops — `numeric_eq_cmp`/`numeric_gt`/
`numeric_lt`/`numeric_remainder` — the old Scheme bodies called, with fixnum
fast paths, but errors labeled with the predicate's own name).
`base/lists.scm` and `base/numbers.scm` are deleted; `(scheme cxr)` is now
pure re-exports and — per R7RS — exports the three-deep names too (it
previously only had the four-deep ones, a conformance gap). All get the
standard shadow-bit deopt for free; regression tests cover the
library-loader alias binding (the P3 2026-08-04 bug class), rebinding deopt,
every composition against its hand-inlined car/cdr chain, and both value
paths per predicate.

**Measured (interleaved A/B vs main ×3, wall-clock):** predicate-heavy loop
−50%, cadr/caddr-heavy loop −51%, tak-shaped `(not (<))` recursion −36%,
cddr list churn −31%; fib control flat. Residual gap vs hand-inlined `not`:
one extra dispatched `Not` per iteration — the `not`+`JumpUnless` fusion
peephole stays a P5 candidate.

### P10 — Variadic/primitive-call allocation churn  *(first slice done — 2026-08-08)*

The §1.10 "deriv `Vec`-churn cluster", attributed by a debug-info profile
(`atos` on the sampled addresses — worth the rebuild every time):
deriv's malloc/free was 6.7% of runtime, split across the variadic
rest-list staging `Vec` in `call_closure_from_regs` (every `map` call —
the "variadic calls are the uncommon shape" comment was wrong in
practice), `list_from_iter`'s collect-then-cons staging (every `list`
call), `MakeClosure`-adjacent object-arena growth, and `Apply`'s
`list_to_vec`.

**Landed:** the two staging `Vec`s are gone — both sites now cons
directly, back to front. This is safe because collection only runs at
the interpreter loops' safe points, never inside `alloc_pair`, so
partial lists need no root (the staging was never load-bearing for GC);
`list_from_iter` now requires `DoubleEndedIterator` (all callers
qualify). The tail-call variadic path was already `Vec`-free via the
#17 stack stage. Measured (interleaved A/B ×3): deriv −2.2%, tak flat;
deriv's malloc cluster 533 → 367 samples, `lists::list` self-time
43 → 11.

**Deliberately not done:** `Apply`'s `list_to_vec` scratch-buffer reuse —
the #17 negative result (scratch take/restore for closure-call args
regressed re-entrant code) covers it; revisit only with a design that
avoids per-call take/restore. `MakeClosure`'s residue is object-arena
growth between collections (captures are empty on the hot path — no
staging `Vec` exists there), which is generational-GC territory
(stage 5 priority 3).

**Queued-item closures (2026-08-08, follow-up PR):**
- **`value_buffer` recycling — resolved as dead code.** Investigating the
  queued item found the live multi-value channels already recycle: the
  `values` interception and continuation resume refill the buffer in
  place, and `CallWithValues`' `mem::take` is load-bearing (re-entrant
  code must see a clean channel — same family as the #17 negative
  result). The only *allocating* sites were the `ReturnMulti` /
  `ReceiveValues` dispatch arms — instructions **no compiler pass ever
  emitted** (vestiges of a pre-`CallWithValues` design). Both
  instructions, their arms, and the `ContinuationValueMismatch` error
  only they raised are deleted; `docs/VM_ISA.md` §4.6 now documents the
  actual mechanism.
- **`list_from_iter_with_tail`** — dotted-list twin of `list_from_iter`;
  every production back-to-front cons loop now delegates to the one
  `Heap` impl: tree-walker quasiquote and variadic rest args, parser
  dotted lists, macro-expander dotted templates, `Heap::list_append`,
  and the `append`/`list-copy`/`command-line`/
  `get-environment-variables`/`features` primitives. The deliberate
  exceptions: `records.rs`'s field-name build (interleaves interning
  with consing — not a pure list build) and a handful of test-local
  helpers.
- **pc-in-loop-local — NEGATIVE RESULT, do not retry as specified.**
  Built and fully working (chibi 1163/1163, suite green) on a scrapped
  branch: `run_loop_until` holds the program counter in a loop local, the
  dispatch prologue increments it locally, jump arms write it locally, and
  `flush_pc`/`reload_pc` sync it with the frame at every site that can
  suspend or observe the frame (calls, deopt, continuation capture/invoke,
  raise). Three measured variants, interleaved A/B ×4–7 vs main:

  | Variant | tak | globals loop |
  |---|---|---|
  | v1: flush + reload at every transfer | −1.3% | **+1.8%** |
  | v2: constant `pc = 0` for fresh/reused frames (no reload read) | −1.4% | **+1.1%** |
  | v3: v2 + drop the dead flush before tail calls (frame's pc is
    overwritten with 0 anyway) | −2.0% | **+0.7%** |

  **Why it loses:** the pre-existing prologue already fused the `pc` read
  and write into the *one* frame access it needs anyway for
  `register_base` (#24). So hoisting `pc` removes no frame access — it
  *adds* one at each control-transfer site. Only straight-line and
  jump-dense code (tak) profits; tail-call-dense code (the globals loop)
  regresses, and no amount of eliding recovered parity across three
  iterations.

  **Verdict:** a ≤2% win on the most dispatch-dense benchmark, a
  regression elsewhere, bought with a permanent multi-case invariant —
  the frame's `pc` is stale while the loop runs, flush before *some*
  transfers but not others, reload from the frame in some paths and
  constant 0 in others — which every future dispatch arm must get right
  or silently misbehave. That fails this track's clarity-safe scope bar
  (§Scope decision). Any retry needs a design where the loop local
  *replaces* the frame access rather than supplementing it (e.g. frames
  storing a resume pointer rather than an index), not this one.

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
