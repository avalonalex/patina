# Preventing premature collection: a prevention map

Inputs: `catalogue.md` (classes R1–R8 and X; incidents A1–A7; near relatives B1–B6; latent entries) and
`defenses-hazards.md`, both in this directory; `PRD/GC_PRD.md`; the research corpus under `PRD/study/gc/`. The source
state is `28a94f8`. The repository was not modified.

**Labels.**
- **[C]**: from the catalogue or `defenses-hazards.md`.
- **[P §n]**: stated in GC_PRD at that section.
- **[R file]**: from the research corpus.
- **[R, recalled]**: prior art I know but did not re-verify in the corpus.
- **[S]**: read in source for this report.
- **[I]**: my inference or estimate.

All costs are focused engineer-days or engineer-weeks [I], except stage efforts, which are the PRD's (§19).

**Effect codes** in the tables:

| Code | Meaning |
|---|---|
| **P** | prevents: the bug cannot be written, or adding the field or call site fails to compile until the author decides |
| **F** | flags: the site needs an explicit, reviewable escape hatch, but nothing decides for the author |
| **C** | catches: a test or lane fails on the bug as it was written |
| **C\*** | catches, but only if some test has the right shape |
| **~** | partial |
| **—** | no effect |

**Case keys.**
- A1–A7 are the incidents and B1–B6 the near relatives (catalogue §2).
- *Rk*-L*n* is latent entry *n* of class *Rk*. For example, R2-L8 is `ParsedLibrary.body`.
- X-*n* is blind spot *n* of class X.

---

## 1. The answer

**What the seven incidents share.** Each live value sat where no root provider looks, at a moment when a collection
could run (catalogue §1). Every value raises three questions:
- Who may collect, and when?
- Where may a value live across that moment?
- How does the collector learn every edge?

Today each answer is a convention: a counter, a comment, or a hand-written trace arm. Prevention means replacing each
convention, in this order of preference:
1. a **type**;
2. a **generated artifact**;
3. an **assertion**;
4. a **lane** that reaches the code.

Process covers whatever is left after that.

### 1.1 The five rules that hold today's safety, and what replaces each

| Rule today (catalogue §1, item 4) | Enforced by | Replacement by construction | Interim, on today's collector |
|---|---|---|---|
| "Values are bare `Copy` indices" (the holder looks like a root) | nothing | `Value<'gc>` brand; `Owned`/`Rooted` handles at boundaries (stages 2–3) | embedding regression tests; a freed bitset so a stale read panics |
| "Take a `GcDeferGuard` before you re-enter" | call-site memory | `&mut Heap` versus `Cx<'gc>`; `NoGcScope` by type; `Step::Call` (stage 3) | the guard owned by the data; clippy `disallowed-methods` on entry points |
| "Add a trace arm for each new field" | review | `declare_layouts!` (heap kinds, 5a) plus **a `Trace` derive for host payloads and off-heap structs (not in the PRD)** | exhaustive destructuring in trace functions (no `..`) |
| "Nothing between here and there reaches a safe point" | comments | design A deletes the weak tables (4e); `return_into` (§11.1) | an **`AssertNoGc` scope (not in the PRD)**; zeal at every safe point |
| "Weak kinds share one fixpoint" | one function (`run_mark_phase`) | a collector contract plus a conformance suite (5a) | keep `value_is_live` private |

### 1.2 Findings

1. **The PRD closes R1, R2, R5 and R8 by construction.**
   - R1: `Owned` handles and a branded `with` (§11.5).
   - R2: the collect capability, `Cx`, `NoGcScope` by type, and moving holders into the machine (§11.3, §12).
   - R5: one fixpoint as a contract obligation (§9.5, §14); continuation side tables deleted (§13).
   - R8: collection reachable only through the capability (§11.3, §14).

   These classes produced **A1, A4, A6 and A7**, four of the seven incidents.

2. **The PRD does not close R4 where R4 actually happened.**
   - `declare_layouts!` generates the trace of *heap objects* (§6).
   - A2 (`CompiledMacro`, `Environment`) and A3 (`CpsContinuation`) were off-heap Rust structures. In the redesign these
     become `HostPayload`s, and `HostPayload::trace` is still written by hand (§14).
   - **Two fixes, neither in the PRD:**
     - A `#[derive(Trace)]` in the style of gc-arena's `Collect` derive [R rust-gcs §2.1], applied to every host payload
       and every Rust structure a root provider walks.
     - Today, exhaustive destructuring in every trace function. `trace_object_children` has 7 rest patterns (`{ .. }`)
       [S], and it reaches `CompiledMacro` and `CpsContinuation` by field access. That is how `foreign_expansions` (A2's
       recurrence, R4-L1) and `resume` (A3) went untraced.
   - A missing edge is **invisible to every reachability-based verifier**, because a verifier walks the same edges. When
     the value is also reachable another way (A3), it is invisible to every dynamic check too. Only generated tracing
     closes R4.

3. **R3 and R6 live in the VM's trusted island (§11.3), where no type applies.**
   - The PRD answers with design A, `return_into`, `DEAD_SLOT`, a map-presence verifier and zeal-`entry`.
   - **Two additions, not in the PRD:**
     - An **`AssertNoGc` scope**, modeled on SpiderMonkey's `AutoAssertNoGC` and Wasmtime's `enter_no_gc_scope`
       [R js-engines §3.4, rust-gcs §2.10]. It goes around every window whose soundness today is argued from where
       safe points are. `safe_point` panics if it is *reached* inside the scope, whether or not it would collect.
     - An **independent liveness recomputation** over emitted bytecode. In debug builds it checks that every
       register map is a superset of the true live set, so a wrong map fails when the unit compiles, not only if a
       collection happens to land at that pc.

4. **The lanes caught one incident of seven. The cheapest improvements fit today's collector and take days.**
   - A debug freed-bitset checked in every accessor: vectors, strings and the nine bypassing accessors included.
   - A quarantine against LIFO reuse.
   - GC_DESIGN §11.5's one-line pre-sweep assertion.
   - Zeal at every outermost safe point.
   - Stress over `cargo test`, the matrix, both Larceny lanes and an embedding test.
   - A `gc-poison` feature, so those lanes can run on an optimized build.

   With these, three of the seven incidents (A1, A6, A7) would have been caught by a lane, and two more (A2, A4) by a
   lane given a test of the right shape (§3).

5. **Static hazard analysis: do not build a sixgill/gcmole equivalent** (§4.2).
   - A Dylint MIR lint would cost about 2–4 weeks [I] and a separate nightly toolchain.
   - It would be noisy through the function-pointer primitive table and trait objects.
   - It becomes obsolete at stage 3, when the brand turns the same hazard into a borrow-check error.
   - **Do this instead:** a workspace `clippy.toml` with `disallowed-methods` and `disallowed-macros` (about 1 day; none
     exists today [S]).
     - Each call of a collecting entry point or of the collector then needs a local
       `#[allow(..., reason = "...")]`. That is the annotation half of SpiderMonkey's hazard analysis, made reviewable.
     - It would have flagged A1's new call site.

6. **Process.**
   - Checklists keyed to the classes, in the existing AGENTS.md and GC_DESIGN §5 rather than a new file.
   - A CI **holder census** that fails when a new value-holding field appears outside the inventory.
   - Test rules: #164's exclusivity rule, and break-tests.
   - **Mutation testing of trace code**, to find fixes with no regression test. Five of the seven incidents have none
     (catalogue §2.1).

7. **Sequencing is the hidden risk.**
   - Two stages remove a deferral that today makes R2's latent holders (R2-L1 to L10) safe:
     - stage 2: library bodies collect at points A, B and D;
     - stage 4e: driver-level nested loops collect.
   - §19 already forbids removing a safety property before its replacement lands, but it says nothing about detectors.
   - **The detection bundle in point 4 should be in CI before stage 2 starts.** The first collection at each new point
     then runs where a mistake panics.
   - Run a one-off "ignore-defer" discovery pass before each of those stages (§4.3).

---

## 2. The map, class by class

### R1. A holder above the outermost loop (A6, A7; R1-L1 to L3)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | `eval_*` returns `Owned {heap_id, index, generation}` plus a `Weak<HandleTable>`; `run_forms` holds its value in one; `eval_program_resilient` returns one | Wasmtime `OwnedRooted`; V8 `Persistent`/`Global`; SpiderMonkey `PersistentRooted`; gc-arena `DynamicRoot` [R rust-gcs §3.2] | **P** A6; **P** R1-L1 (a host reads a global as an `Owned`); **P** R1-L3, #604 (`get` after teardown is an error, not a read of a torn-down heap) | in stage 2 (5–7 weeks for the stage) | §11.3, §11.5; stage 2; #605 |
| A | **A host-built environment is a handle.** `Environment::with_parent` leaves the public API; hosts get an owned environment-specifier or namespace handle that is rooted while it is held | same | **P** A7; **P** R1-L2 once `VmBackend::eval` honours `env` | 2–3 days in stage 2 | **Not in the PRD for stage 2.** DESIGN E.3 replaces `&Rc<Environment>` with a namespace handle only at 4b. Add A7 to #605's acceptance |
| A | `interp.with(\|cx\| …)`, branded and taking `&mut self`; trybuild case "a value escapes `with`" | gc-arena `mutate`; oscars UI tests [R rust-gcs §2.2] | **P**: no raw value leaves a window that cannot collect | in stage 3 | §11.3, §11.5 |
| A | Bare-value `eval_*` deprecated (#601's convention), then removed at 5e | — | **P**: the unsafe shape leaves the API | in stage 2 | DESIGN E.3 |
| B | **Embedding regression lane.** Integration tests hold `eval_*` results and a host environment across collecting calls, on both backends, under stress and zeal in a debug build | — | **C** A6; **C** A7 on the tree-walker (and on the VM once it honours `env`) | 1 day | stage 2's gate implies an A6 test; **the lane is not in the PRD** |
| B | Debug freed-bitset in every accessor, plus a quarantine (§4.5) | ASan's quarantine | **C** the probe's silent `#()`, `""` and reused pair (X-1, X-2) | 1–3 days | PRD from 5b only (§16); **nothing for today** |
| B | Embedding-API fuzzer: random sequences of eval, hold, drop and call, compared with an off-mode run | Wasmtime `gc_ops` [R rust-gcs §2.10] | **C** A6 and A7 shapes, systematically | about 1 week | not in the PRD |
| C | Rule: an API that hands a heap value to code above the driver loop hands out a handle. Keep the `pub unsafe` raw-word API (§11.3) out of `patina-interpreter`'s public surface | V8's embedder rules | **F** | — | §11.5 |

**Residual risk.** Host code outside the workspace is not linted. So for R1 the handle types, and their removal of
the bare forms, are the whole defense.

### R2. A Rust frame holds values across a re-entry (A1; R2-L1 to L14)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | **The collect capability.** `&mut Heap` collects; no method on `Cx<'gc>` does. `Value<'gc>` carries an invariant brand. A nested entry beneath a `Cx` is `NoGcScope` by type. trybuild cases: "a value used after a may-collect call", "a slice held across `load_library`" | gc-arena "mutation XOR collection"; rune (`&'ob Context` to allocate, `&mut Context` to collect); Nova `GcScope`/`NoGcScope` [R rust-gcs §2.1, §2.6, §2.8] | **P** A1. **P** R2-L1 to L10 for code outside the island. **P** R2-L11: the poll checks `no_gc_depth` at each poll instead of hoisting it at loop entry (§12). **P** R2-L12: the scope is structural, not a counter | in stage 3 (10–13 weeks for the stage) | §11.3, §12; stage 3 |
| A | **Move the holder into the machine:** `Step::Call`/`Step::Eval` state in `resume_stub` or `ResumePrimitive`; `%parameterize-swap!` stops calling back (4a); the Rust `call-with-port`, `member` and `assoc` variants go or become resumable; loader values go in a `RootScope` | piccolo `Sequence` [R rust-gcs §2.1]; #478 | **P** R2-L2 to L5, L7, L8, L9 | 0.5–2 days per site | §11.3; 4a; stage 2 (rooted loading); DESIGN E.1 |
| A | A guard owned by the data or bound to the operation (A1's fix, #602). **Interim:** make `ParsedLibrary::new`'s heap non-optional, since `None` installs no guard (`library_loader.rs:208`) | — | **P** R2-L8's latent `None` path | 1 hour | not in the PRD (stage 3 subsumes it) |
| A | **Allocation never collects.** `Heap::alloc_*` cannot reach the root providers, so it cannot collect | Chez `c/alloc.c` [R chez]; gc-arena | **P** R2-L14 (all census sites) by construction, today | 0 | §8; B2; K16 (never make allocation a collection point) |
| A | No `Rc` or `Drop` payloads: record fields, parameter values and closure globals inline, written through the funnel | Wasmtime "no native pointers in the heap" [R rust-gcs §2.10] | **P** R2-L13 (the container survives sweep, its values do not) | stages 4b, 4g, 5 | §6, §10 |
| B (static-lite) | `clippy.toml` `disallowed-methods` on `execute`, `run_loop_until`, `run_synchronously`, `Backend::eval` and `Interpreter::eval_*`. Every sanctioned call carries `#[allow(clippy::disallowed_methods, reason = "holds no heap value across this call: …")]` | SpiderMonkey `AutoSuppressGCAnalysis`, the annotation half of the hazard analysis [R js-engines §3.4] | **F** A1: the second loading path would not compile without an allow and a written reason | 1 day | not in the PRD |
| B | `PATINA_GC_ZEAL=every` (collect at every outermost safe point), plus stress over `cargo test` and both Larceny lanes (§4.3) | SpiderMonkey `gczeal`; HotSpot `FullGCALot` / `GCALotAtAllSafepoints` [R, recalled] | **C** A1 (stress already caught it); **C\*** for R2 paths the chibi suite never runs | hours, plus CI time | zeal-`entry` from stage 3, VM only (§14, DESIGN F.1); **stress over other suites is not in the PRD** |
| B | **Ignore-defer discovery run** (§4.3): a debug-only mode that lets nested loops collect, run once over every lane before stage 2 and again before 4e. Each panic names a holder that must move into the machine first | SpiderMonkey's dynamic rooting analysis, which poisoned unrooted pointers [R js-engines §3.2] | **C** R2-L1 to L10, and checks that the list is complete | 1 day plus triage | not in the PRD |
| B | Tests that make deferral observable: a parameter-like procedure calls `(gc)` inside `parameterize` with the old value held only in `olds`; repair the drifted `collection_inside_higher_order_primitive` | — | **C** R2-L2 when 4a or 4e changes it | hours | catalogue R2; not in the PRD |
| B | A Dylint MIR lint, "value live across a may-collect call" | V8 gcmole; SpiderMonkey sixgill | would **C** A1 | 2–4 weeks plus a nightly toolchain | **rejected** (§4.2) |
| C | Review rule: a Rust function that holds a value and calls anything that reaches the evaluator must be resumable, rooted, or guarded on its data, never by a guard at the call site | the catalogue's R2 progression | — | — | AGENTS.md has the rule for callbacks; extend it to loading |

### R3. A value parked off-root across the next safe point (A5, B1; R3-L1, L2)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | **Design A continuations:** `VmContinuationRef` and the weak side tables are deleted, and with them the rule "store touched within one dispatch" | Larceny, Gambit [R racket-larceny] | **~** A5: a parked handle no longer prunes a payload, so the damage shrinks from "a continuation over swept slots" to an ordinary lost value; **P** R3-L1 | stage 4e | §13 |
| A | `return_into(frame)` as the only way to write into or activate a suspended frame; frame invariant 1 (every window initialized) | JEP 376 [R java-hotspot] | **~**: one choke point that can assert the target is rooted | frames 4d, watermark 7; the PRD does not say when the choke point lands | §11.1 |
| A | **Carried-value rule:** a value that must outlive the current instruction is first moved into a traced `VmState` field (the `pending_escape` pattern, #78), and only then is a window freed. Enforce it with an exhaustive destructure, or the derive, of `VmState` in `trace_roots`, so a new field forces a decision | #78 done right | **P** A5's shape for new state fields; **F** for locals | 1 day | not in the PRD |
| B | **`AssertNoGc` scope** around each window that comments argue contains no safe point: capture to insert (`vm_state.rs:631-656`), invoke to copy-back (`gc_roots.rs:21-24`), and B1's freed window (`vm_state.rs:~1825-1832`). `safe_point` asserts the depth is 0 on **every** call, collecting or not | SpiderMonkey `AutoAssertNoGC`/`AutoCheckCannotGC`; Wasmtime `enter_no_gc_scope` panics [R js-engines §3.4, rust-gcs §2.10] | **C** an A5 or B1 shape in the first debug test that reaches a poll someone added inside the window, without needing zeal. B1's "argued safe" becomes checked | hours | **not in the PRD** (`NoGcScope` defers; it does not assert) |
| B | Zeal-`entry`: a collection at every poll site | SpiderMonkey `gczeal`; HotSpot | **C** A5 and B1 once a poll exists in the window | stage 3 | §12, §14 |
| B | A debug `DEAD_SLOT` fill of freed register windows | HotSpot `ZapUnusedHeapArea` [R, recalled]; SpiderMonkey Baseline overwrites dead locals [R js-engines §3.6] | **C** any read of a freed window | hours | `DEAD_SLOT` fill: §14, §16 (from 5b) |
| C | Review rule: a comment arguing "no safe point between X and Y" comes with an `AssertNoGc` at the same lines (§5.2 lists today's) | — | — | — | not in the PRD |

**Residual risk.** Creating a new window like A5 still depends on review; nothing forces its author to annotate it.
What changes is activation: the day a poll lands inside an annotated window it panics, and zeal-`entry` panics on an
unannotated one.

### R4. An edge or field with no trace rule (A2, A3, B2; R4-L1 to L3)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | `declare_layouts!` generates `size`, `trace`, `copy`, `verify`, the printer and the JIT offsets for heap kinds | Chez `s/mkgc.ss` [R chez, racket-larceny §3.2] | **P** a misfiled leaf (`heap/mod.rs:137-141`) and R4-L2 for heap kinds | stage 5a | §6, §14 |
| A | **`#[derive(Trace)]` for host payloads and for every Rust structure a provider walks:** `Environment` (until 4b), `CompiledMacro`, `CpsContinuation`, `ContValue`, `WindRecord`, `ExceptionHandler`, `PromptFrame`, `VmState`, `Library` and others. Every field is visited unless marked `#[trace(skip, reason = "...")]`; leaf types implement `Trace` as a no-op (`NEEDS_TRACE = false`). A field whose type does not implement `Trace` does not compile | gc-arena `Collect` derive; rune `Trace` [R rust-gcs §2.1, §2.8] | **P** A2, A3 and R4-L1 (`foreign_expansions`); **P** B2 (no hand-written copies) | about 1 week [I] | **not in the PRD** (§14's `HostPayload::trace` is hand-written) |
| A | **Interim, today: exhaustive destructuring.** Trace functions contain no `..`. Each traced struct is taken apart as `let S { a, b, c } = s;`, and a field deliberately not traced is written `name: _` with a comment. Adding a field is then error E0027 until someone decides | Rust's exhaustive struct patterns | **P**/**F** A2, A3 and R4-L1 at the next added field: it forces a decision but does not make it | 1–2 days | not in the PRD |
| A | Move off-heap structures into the heap: namespaces and binding records (4b), continuation objects (4e), tree-walker payloads as host ids (4f), inline payloads (4g) | Wasmtime; rune [R rust-gcs] | **P**: those edges become generated slots | stages 4b–4g | §6, §11.4, §11.6 |
| A | Host edges reported through `SlotVisitor::host(id)` and traced inside the one fixpoint; open `RootSet::register` | MMTk; Whippet [R immix-mmtk, whippet-misc] | **~** R4-L3: registration is easy, but nothing requires it | stage 5a | §14 |
| B | Test rule from #164: the value under test is reachable *only* through the edge under test | — | **C\*** A2; **C\*** A3, and only when the test is written this way, because aliasing hides A3 from every dynamic check | — | not in the PRD as a rule |
| B | **Mutation testing of trace code** (§4.8) | — | detects missing detectors. It would flag #164's vacuous test, the drifted test, and today's untested `visit_env(definition_env)` (A2 has no regression test) | about 2 days, then nightly | not in the PRD; the tool needs approval (decision 14) |
| B | Freed-bitset, quarantine and zeal over `cargo test` | — | **C\*** A2 (needs a test that collects; `macro_definition_env.rs` never calls `(gc)`); **—** A3 | shared with §4.5 | 5b; nothing today |
| B | `move-all` with a `PROT_NONE` from-space | HotSpot UnhandledOops-style lane [R java-hotspot §3.9]; SpiderMonkey compacting zeal | **C** a missed *slot* in VM heaps even while aliasing keeps the referent alive, because the stale copy points at from-space. **—** for the tree-walker (its heaps never move), so **—** A3 | stage 8 | §9.4, §14 |
| B | Heap verifier, `VERIFY` and `VERIFY_ROOTS` | HotSpot `VerifyBeforeGC`/`VerifyAfterGC` [R, recalled] | **—** a missing edge (the verifier walks the same edges); **C** a dangling root | stage 5a | §16 |
| C | Checklist "a new field in a traced struct"; the holder census in CI (§5.3) | — | **F** R4-L3 | 1 day | stage 0's inventory; **the census is not in the PRD** |

### R5. The order of weak processing (A4, B3; R5-L1 to L3)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | **One fixpoint, owned by the collector,** covering host payloads, ephemerons and later guardians, as an obligation of every `Collector`. `WeakRegistry` is lent to `collect`, and no root provider does weak processing | Whippet `gc-ephemeron.c` [R whippet-misc] | **P** A4; **P** R5-L2 | stage 5a | §9.5, §9.9, §14 |
| A | Delete the second weak kind, the continuation side tables | — | **P** A4's shape | stage 4e | §13 |
| A | Key-indexed ephemerons inside the one loop (#609) | Whippet | **P** R5-L1 | stage 5a | §9.5 |
| A | Liveness queries are made only by the collector, after marking (today `value_is_live` is private) | — | **P** R5-L3 | done; 5a | — |
| B | The conformance suite on `MarkRegion`, `NullGc` and a `TestModel` | — | **C** the A4 class in any future collector | stage 5a | §14 |
| B | **A whole-heap verifier after each collection:** every edge of every *retained* object or table entry, not only the reachable ones, points at a live start | HotSpot `VerifyAfterGC` [R, recalled] | **C** A4 at the collection that frees: the entry was retained but its payload went untraced | stage 5a (the verifier checks "every heap-bit word") | §16 |
| B | A program generator for ephemerons × `call/cc` × `dynamic-wind`, run under zeal (#587) | Wasmtime `gc_ops` | **C** A4, which its commit calls "the case no test covered" | 1–2 weeks | #587 is open; the PRD does not schedule it |

### R6. Liveness precision that is too aggressive (B4, B5; R6-L1 to L5)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | Clear dead slots instead of skipping them, writing `DEAD_SLOT` in debug and zeal builds; `reg_at` asserts no instruction reads one | SpiderMonkey Baseline overwrites dead locals [R js-engines §3.6] | turns R6-L1 to L3 from silent wrong values into panics (**C**, not **P**) | hours today; PRD at 4d and 5b | §11.1 invariant 3 |
| A | Traced code liveness: a unit is live if any of its descriptors is marked, plus the `escaped` bit | Chez; HotSpot nmethods | **P** R6-L4: there is no separate count to get wrong | stage 4e | §9.7 |
| B | **Independent liveness recomputation.** In debug builds, at unit load: a backward dataflow over the emitted bytecode with exhaustive per-opcode read and write sets, asserting that each map is a superset of the live set at every suspension point | the JVM's bytecode verifier against `StackMapTable` [R, recalled] | **C** R6-L1 and R6-L2 when any test program compiles, with no need for a collection at that pc | about 1 week [I] | **not in the PRD** (invariant 5 checks that a map exists, not that it is right) |
| B | Map verifier: a map exists at every frame pc reached | — | **C** a missing map | stage 5a | §11.1 invariant 5, §16 |
| B | Zeal-`entry`: every pc that can collect does | — | **C** wherever the output changes or a `DEAD_SLOT` is read | stage 3 | §12 |
| B | A kill switch for each precision optimization (register retirement, code release, #614's owner count), with an on/off differential under zeal | — | **C** | hours per switch | not in the PRD |
| C | Rule: each change that stops tracing what an analysis calls dead ships with an assertion that turns a wrong answer into a panic, break-tests in the dangerous direction, and a kill switch | B4 and B5 practice | — | — | partly: #614's acceptance |

### R7. Raw bits or an index outlive a sweep (B5 generations, B6; R7-L1 to L3)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | Handles carry a generation (`Owned`, and `Rooted` with `{heap_id, generation}`); code ids `{slot, generation}` (done) | Wasmtime [R cranelift-gc] | **P** stale handles | stages 2–3 | §11.3 |
| A | Weak tables are maintained by the epilogue (steps 5–6) and never pruned from outside; raw-bits keyed tables are replaced (`syntax_sources` deleted at stages 1 and 4c; identity hash per §9.8) | Chez's tlc rehash [R chez] | **P** R7-L1, R7-L2 | stages 1, 4c, 5c | §9.8, §9.9 |
| A | Slot-based visitors (`SlotVisitor::slot`); `pinned` only in tree-walker heaps | V8 `VisitRootPointers`; SpiderMonkey `TraceRoot(&ptr)` [R js-engines L3] | **P** R7-L2 under moving | stage 2 | §14 |
| B | Quarantine of freed holes: 2 collections in the PRD, longer or never reused in zeal lanes (§4.5) | ASan | **C** a stale key that meets a reused slot | 5b; nothing today | §16 |
| B | `move-all` with `PROT_NONE` | — | **C** R7-L2 before evacuation ships | stage 8 | §9.4 |
| C | clippy `disallowed-methods` on the raw-bits accessor outside allowlisted modules; checklist item "a table keyed by bits needs a pruning story at sweep" | — | **F** | hours | not in the PRD |

### R8. A collection with a partial or foreign root set (R8-L1 to L4)

| Tier | Technique | Prior art | Effect | Cost | Where |
|---|---|---|---|---|---|
| A | Collection only through `&mut Heap`; `collect` takes `&mut self` behind the capability. **Today:** make `Collector`, `MarkSweepCollector`, `run_mark_phase` and `Heap::sweep` crate-private. They are re-exported from `patina-core/src/lib.rs:71-72` [S]; their three test callers move into the crate or behind a test-only feature | gc-arena | **P** R8-L1 | hours today | §11.3, §14 |
| A | `PENDING_ESCAPE` leaves `thread_local!`; no process-wide GC singleton; a CI check against new `thread_local!`, implementable as clippy `disallowed-macros` with an `allow` on the 11 existing invocations in 9 files [S] (the PRD counts 19 statics) | Wasmtime's per-store state | **P** R8-L2 | stage 2 (`PENDING_ESCAPE`), stage 3 (C9) | §11.3, §16, §18.6 |
| A | Handles and `RootScope`s carry `heap_id`; release-mode checks at trust boundaries | — | **P** R8-L3, R8-L4 | stages 2–3 | §11.3 |
| B | Make today's debug-only identity checks release asserts: `with_globals` (`vm_state.rs:342`), and add the missing `Rc::ptr_eq(macro.heap, heap)` from GC_DESIGN §9.6 | — | **C** R8-L3, R8-L4 | hours | not in the PRD |

### X. Detector blindness: the fix for each blind spot

| Blind spot | Fix today | PRD |
|---|---|---|
| X-1: a swept vector or string reads back as a legal empty value | debug freed-bitset checked by every accessor | inline payloads (5d); eager poisoning sweep (5b) |
| X-2: a freed slot already reused (LIFO) | a FIFO quarantine in debug; never reuse in zeal lanes | 2-collection quarantine (5b); `PROT_NONE` runs |
| X-3: nine accessors bypass the `Free` check; `frame_globals` falls back silently | route all of them through the checked accessor; make the fallback panic in debug | accessor poison assertions (5b) |
| X-4: the collector absorbs dangling references | GC_DESIGN §11.5's assertion; `trace_children` asserts on `Free` or poison in debug | verifier (5a) |
| X-5: stress reaches only the chibi suite, through the CLI, at n=16 | zeal at every safe point; stress over `cargo test`, the matrix, Larceny and embedding; `gc-poison` to afford them | zeal-`entry` (3); more lanes per stage |
| X-6: every `(gc)` is placed by hand | zeal; seeded placement fuzzing; the #587 generator | zeal; the generator is not scheduled |
| Vacuous proofs (#606) | assert `bytes-reclaimed > 0` | §16, stage 1 |

---

## 3. Which incident each technique reaches

**P** prevents, **F** flags, **C** catches, **C\*** catches given a test of the right shape, **~** partial, **—** none.
"Status quo" is review and audit as practised.

| Technique (stage, or "now") | A1 | A2 | A3 | A4 | A5 | A6 | A7 |
|---|---|---|---|---|---|---|---|
| `Owned` handles (2) | — | — | — | — | — | **P** | — |
| Host environment handle (2, gap) | — | — | — | — | — | — | **P** |
| Brand and collect capability; `RootScope` for loading (2–3) | **P** | — | — | — | — | **P** | — |
| clippy `disallowed-methods` (now) | **F** | — | — | — | — | **F** (`run_forms`) | — |
| Exhaustive destructuring (now), or `derive(Trace)` | — | **P** | **P** | — | ~ (`VmState`) | — | — |
| `declare_layouts!` (5a; heap kinds only) | — | — | — | — | — | — | — |
| One fixpoint and weak contract (5a; done for A4) | — | — | — | **P** | — | — | — |
| Design A continuations (4e) | — | — | — | **P** | ~ | — | — |
| `AssertNoGc` windows (now) | — | — | — | — | **C** once a poll is added | — | — |
| Freed-bitset and quarantine (now) | **C** | **C\*** | — | **C\*** | — | **C\*** | **C\*** |
| GC_DESIGN §11.5 pre-sweep assertion (now) | **C** | — | — | **C\*** | — | **C\*** | — |
| Zeal at every safe point (now), `entry` (3) | **C** | **C\*** | — | **C\*** | **C** once a poll exists | **C\*** | **C\*** |
| Stress and zeal over `cargo test` plus embedding tests (now) | **C** | **C\*** | — | **C\*** | — | **C** | **C** |
| #587 generator and embedding fuzzer | **C\*** | **C\*** | — | **C** | — | **C** | **C** |
| Whole-heap verifier with quarantine (5a, 5b) | **C** | — | — | **C\*** | — | — | — |
| `move-all` (8; VM heaps only; A4's side table is gone by then) | **C** | **C\*** | — | — | **C\*** | **C\*** | — |
| Status quo: review and audit | — | **C** | **C** | **C** | **C** | — | — |

The freed-bitset, assertion and verifier rows mark A4, A6 and A7 as **C\*** because each needs a program of the right
shape. The embedding-test and generator rows supply that program, which is why they mark the same incidents **C**.

**Reading the matrix.**
- **The "now" rows reach every incident.**
  - A2 and A3 are prevented by exhaustive destructuring.
  - A1, A6 and A7 are caught by the zeal and embedding lanes.
  - A4 is caught by the generator (#587), or by any test of its shape run under the freed-bitset.
  - A5 is not caught as written. No safe point runs in its window today, so it is a hazard rather than a bug. It is
    caught the day it would become one.
- **The redesign then makes A1, A4, A6 and A7 unrepresentable.**
- **A3 is the instructive case.** No dynamic technique would ever have caught it. Only generating the trace, or
  forcing a decision per field, prevents its class.

---

## 4. Cross-cutting techniques, evaluated once

### 4.1 The capability and the brand: what they buy and where they stop

**What they cover.** Every crate outside the island: primitives, frontend, macros, runtime, interpreter and compat.
That is where SpiderMonkey and V8 needed a static analysis over thousands of sites [R js-engines §2.5, §3.4]. Patina's
may-collect surface stays small because allocation never collects and primitives no longer call back from Rust
[R rust-gcs §0.2].

**What they do not cover:**
1. **The VM and tree-walker cores**, the trusted island (§11.3). Their values live in VM memory as raw words, so R3
   and R6 remain obligations there.
2. **The `pub unsafe` raw-word API** used for `CoreExpr`/`CpsExpr` literals, macro literals and `Step` state.
   - Soundness there rests on `NoGcScope`, a traced root, or the `CoreExpr` literal pool (§11.3).
   - Keep its callers few, with a clippy `disallowed-methods` entry outside core.
3. **Values inside `Rc` containers**, until 4g and 5 inline them.
4. **Trace completeness.** The brand makes a missed *root on the Rust stack* impossible, but not a missed *edge*,
   which is R4's whole class. Generation (§2, R4) is the complement.

### 4.2 Static hazard analysis in Rust: the options

| Option | What it checks | Cost | Upkeep | Verdict |
|---|---|---|---|---|
| The brand (`Value<'gc>`, `&mut Heap` collects) | a value live across a may-collect call is a compile error | stage 3 | none | **adopt** (the PRD has it) |
| `clippy.toml` `disallowed-methods`, `disallowed-macros`, `disallowed-types` | call sites of collecting entry points and of the collector; `thread_local!`; the raw-bits accessor; `Environment::with_parent` and the raw-word API outside core. Each sanctioned site carries `#[allow(lint, reason = "...")]` (lint reasons are stable since Rust 1.81; `clippy::allow_attributes_without_reason` can require them) | about 1 day | stable clippy; CI already runs it with `-D warnings` on the pinned 1.97.1; renames update the config. **Check how the pinned clippy treats a path that does not resolve in a crate** | **adopt now** |
| No `..` in trace functions, checked by a grep in CI | that each trace arm accounts for every field | 1–2 days | none | **adopt now** |
| `#[derive(Trace)]` proc-macro | field completeness for off-heap structures and host payloads | about 1 week | small | **adopt** (gap in the PRD) |
| Dylint MIR lint, the gcmole/sixgill equivalent | liveness of a `TaggedValue` local across a call into the may-collect call graph | 2–4 weeks [I] | needs a nightly toolchain beside `rust-toolchain.toml`'s 1.97.1; rustc-internal churn; the call graph through the function-pointer primitive table, `dyn ApplyContext` and `Backend` trait objects must be assumed to collect, which makes it noisy | **reject**. It pays only until stage 3, and in the island values sit in VM memory, not Rust locals |
| The PRD's helper call-graph test ("no `Leaf` reaches a `Transfer` function", §16) | gcmole-shaped, but limited to the helper table | — | — | **start dynamic:** a debug `in_leaf` flag that every `Transfer`, poll and collect path asserts. That is cheap and catches what tests reach; go static only if escapes appear |

**Why SpiderMonkey and V8 needed static analysis, and Patina does not.**
- In those engines allocation can collect, so every function is a GC point. That produced thousands of handle sites
  that only a tool could police.
- Patina's "allocation never collects", together with a small may-collect surface, is what the corpus calls the
  "fourth, cheaper answer" [R js-engines §0, L1].
- The Rust form of that answer is the brand, not a linter.

### 4.3 The zeal family

| Mode | Collects | Catches | Cost | Where |
|---|---|---|---|---|
| stress *n* (today) | at the first outermost safe point after *n* allocations | the shapes the chibi suite runs | exists | today; every *n* polls after 5e |
| **every** (`PATINA_GC_ZEAL=every`) | at every outermost safe point, allocation or not | stretches without allocation; and for a deterministic program on a non-moving collector, **one run covers every placement**: a value that is live but unrooted at poll *k* is freed at poll *k*. Pair it with never-reuse (§4.5), because collecting every poll otherwise returns slots to the free list within a few polls | hours | not in the PRD as such; `entry` is its stage-3 form |
| `entry` | at every poll site, frame entry included (VM) | R3 windows the day a poll lands in them; R6 at every pc that can collect | stage 3 | §12, §14 |
| **`random:<seed>:<p>`, `nth:<k>`** | with probability *p* at each safe point, or exactly at the *k*-th | **GC-placement fuzzing.** Interactions between collections that "every" hides: minor/major sequences, moving, the timing of weak breaking, finalizers, #614-style counts. Cheaper than "every" on large suites; a failure replays from its seed | hours | not in the PRD |
| **ignore-defer** (a discovery mode, not a lane) | nested loops collect too | lists the R2-L1 to L10 holders, empirically, before stages 2 and 4e lift deferral | 1 day plus triage | not in the PRD |
| `minor`, `alternate` | minors (stage 7) | missed young referents. Add a sanity trace (§4.4) | stage 7 | §14 |
| `move-all` (with `PROT_NONE` from-space) | evacuates every unpinned block | missed slots even when aliasing hides them (R4); stale raw indices (R7-L2) | stage 8 | §9.4, §14 |
| `jit-invalidate` | invalidates every JIT body at every poll | JIT-frame publication | JIT | §9.7 |

### 4.4 Verification

- **GC_DESIGN §11.5's assertion**, written there and never implemented.
  - Assert in debug that sweep's pre-marking of the free list finds every slot unmarked
    (`debug_assert!(was_clear)` at `gc.rs:836-838`).
  - Every reachable edge into a freed slot then panics at the next collection, even if nothing reads it.
  - Add the same check where `trace_children` meets `Free` or poison (`gc.rs:679-692`). About one hour.
- **Verify before and after collection** (5a, §16). A whole-heap walk of marked objects and retained entries catches
  the A4 shape and dangling roots. It cannot see a missing edge.
- **`VERIFY_ROOTS`** (5a): every word held by `RootScope`, `Owned` and the providers.
- **A sanity trace** in the style of MMTk's `sanity` feature [R immix-mmtk §12].
  - An independent, unoptimized full trace from the roots, whose marks are compared with the production marker's.
  - It catches bugs in the marker's own optimizations: `RESCAN` overflow, range entries, `KEYHINT` chains.
  - Above all, in generational mode it catches **a minor that frees a live young object**, because of a gap in the
    remembered set or the watermark. That is premature collection by a minor.
  - The PRD's verifier checks the sufficient conditions (remembered-set completeness, no young reference below a
    watermark). A sanity trace after every minor in the zeal-minor lane checks the conclusion directly.
  - About 2–3 days once the verifier exists. **Not in the PRD.**
- **The liveness-map recomputation** of R6.

### 4.5 Poison

**Today.**
- Only pairs (`GC_POISON`) and objects (the `Free` tombstone) are checked, and only in debug builds.
- Vectors and strings are blind (X-1); reuse defeats both checks (X-2); nine accessors bypass the check (X-3).

**Freed bitset.**
- One debug-only bit per slot in each arena, set at sweep and cleared at allocation.
- Every accessor checks it, including those for vectors, strings and the nine bypassing accessors.
- 1–3 days [I].

**Quarantine.**
- Freed slots enter a FIFO before reaching the free list: bounded, for example 1 M slots, in debug lanes; unbounded
  ("never reuse") in zeal lanes.
- The PRD's 2-collection quarantine (§16) is too short under zeal-every, where collections are a few polls apart.

**Poison in optimized builds.**
- A `gc-poison` cargo feature gates poison writes and the freed bitset as `cfg(any(debug_assertions,
  feature = "gc-poison"))`.
- It is additive, not mutually exclusive, so `--all-features` clippy stays green.
- It lets stress and zeal run over `cargo test`, Larceny and the GBS at release speed.
- Hours of work. **Not in the PRD**, whose poison lanes are debug only.

**From 5b** (PRD §16): an eager poisoning sweep, `PROT_NONE` on wholly free 4 MiB runs, and the `DEAD_SLOT` fill.

### 4.6 Miri

**What it can catch.**
- Undefined behaviour in unsafe heap access: `HeapSlot` provenance, `RootScope` LIFO broken through `mem::swap`.
- An `Owned` used after teardown, where tables are Rust allocations.

**What it cannot catch.**
- A missed root. A logical free inside a `Vec` arena, or a hole in a reservation, is not a Rust deallocation, so a
  read through a missed root is memory-safe as far as Miri can tell.
- `patina-core/src` contains no `unsafe` today [S], so Miri adds nothing to premature-collection detection now.

**Verdict.** Keep the PRD's 5a Miri lane for the unsafe boundary it guards (§11.3). Do not count it as a defense
against premature collection.

### 4.7 Fuzzing

- **A program generator** (#587, on the pattern of Wasmtime's `gc_ops`).
  - It writes Scheme programs over `cons`, vectors, `set-car!`, `call/cc`, `dynamic-wind`, ephemerons,
    `parameterize`, `guard` and prompts.
  - Each program runs under zeal and is compared with off mode and with the other backend.
  - It targets the shapes nobody anticipated: A4 was "the case no test covered".
  - `proptest` is already a dev-dependency of `patina-core`, `patina-primitives` and `patina-frontend` [S], so this
    needs no new dependency. 1–2 weeks [I].
- **An embedding-API fuzzer** for R1: random host sequences of eval, hold, drop and call.
- **GC-placement fuzzing**, through zeal `random` (§4.3).

### 4.8 Mutation testing: a detector for missing detectors

**What it does.** cargo-mutants, run over the trace code (`heap/gc.rs`, `vm_state/gc_roots.rs`,
`cps_eval/gc_roots.rs`). Each surviving mutant is a trace line that no test needs.

**What it would have found:**
- #164's vacuous test;
- `collection_inside_higher_order_primitive`'s drift;
- the missing regression tests behind the fixes for A2, A3 and A5.

It does not find a line that was never written (that is the derive's job). It does show which fixes would silently
regress.

**Cost.** About 2 days, then a nightly run. It is a tool, not a crate dependency, but decision 14's approval rule
should be applied to it.

**Manual form.** Make B5's break-test practice a PR checklist item.

### 4.9 Differential and oracle lanes

- **Keep:** off against stress and zeal, byte-identical, with the tally pinned at 1226.
- **Cross-backend runs are an oracle.**
  - A4 was wrong on the VM and right on the tree-walker. A7 is the reverse, and is masked on the VM.
  - Run `gc_shared_tests!`, the matrix and the embedding tests on both backends under zeal, and compare.
- **Precision on and off, under zeal** (R6).
- **Reclamation proofs that guard `bytes-reclaimed > 0`** (#606).

---

## 5. Process tier

### 5.1 Checklists

These belong in AGENTS.md's "When Adding Features" and GC_DESIGN §5, both of which exist. A new file needs the owner's
approval.

**A new heap object type (R4, X).**
- Add it to the trace function with a full destructure, no `..`; later, to `declare_layouts!`.
- A leaf arm needs a comment stating that the type holds no value.
- Make sure the freed-bitset accessor covers it.
- Add a GC test in which the value is reachable only through the new kind, run under zeal on both backends.

**A new off-heap holder of values.** That covers an `Rc` struct field, a side table, and a Rust collection held
across a call (R2, R3, R4).
- Choose and record one fate:
  - traced by a named provider, with `derive(Trace)` or a destructure;
  - carried in machine state;
  - covered by an `AssertNoGc` window.
- Add a row to the holder inventory (§5.3).
- Add a test that collects while the holder is the value's only reference.

**A new entry point that can collect, or a Rust path that calls one (R1, R2, R8).**
- Add the clippy entry and its reasoned `allow`.
- Name the guard on the data, or the `RootScope`.
- Add an embedding or nested-load test under zeal.

**A new weak kind (R5).**
- It joins the one fixpoint and the conformance suite.
- Add a test of a weak payload that holds the other weak kind, as A4's test does.

**A new precision optimization (R6).**
- A debug assertion that turns a wrong answer into a panic.
- Break-tests in the dangerous direction.
- A kill switch with an on/off zeal differential.

**A new table keyed by value bits (R7).**
- State how it is pruned at sweep, or which generation check protects it, and what happens under evacuation.

**A new embedding API that returns or accepts values (R1).** It uses handles, and it has a test that holds its result
across a collecting call.

### 5.2 Review rules

1. **A comment that argues soundness from where safe points are needs an `AssertNoGc` at the same lines.** Today's
   such comments:
   - `gc_roots.rs:21-24`: the weak-store touch, with capture at `vm_state.rs:631-656`;
   - B1's freed window, `vm_state.rs:~1825-1832`;
   - `control.rs:31-33`.

   A5's retention at `vm_state.rs:1865-1875` is a retention, not a window. Its guard is a debug assertion that the
   target handle is still in a rooted register when it is used.
2. **A trace function has no `..`,** and reaches no field without destructuring.
3. **A GC test states its exclusivity:** which edge it exercises and why the value is reachable only through it. The
   PR body records its break-test.
4. **A premature-collection fix lands with a regression test** that fails without the fix in the debug stress or zeal
   lane. Where no failing program exists (A3, A5), it lands with an assertion that would fire.
5. **A leak fix that frees a root (B1, #172) needs the same evidence as a rooting fix:** an `AssertNoGc` on the window
   and a zeal run.
6. **A GC-relevant PR names its class (R1–R8) in its body,** so the next catalogue is a query rather than an
   archaeology project.

### 5.3 The holder inventory, and a census that enforces it

**What exists.** Stage 0 puts the inventory (DESIGN E.1) on the tracking issue. GC_DESIGN §5 calls itself "the
checklist implementations must satisfy".

**Add teeth with a census script in CI.**
- It greps struct definitions across the workspace for fields typed `TaggedValue`, `Option<TaggedValue>`,
  `Vec<TaggedValue>`, `HeapIndex`, `Rc<Environment>`, and `Rc` of a known holder.
- It compares them with a checked-in allowlist.
- A new holder fails CI until a row is added naming it, how it is rooted, and its test. That is SpiderMonkey's
  "expected hazard counts under version control" [R js-engines §3.4], applied to holders.
- Cost: 1 day.
- Location: GC_DESIGN §5's table if a script can parse it; otherwise a new TSV, which needs approval.

**From stage 5,** the rule becomes "no value-holding Rust field outside the island and the payload registry".

---

## 6. What to do, in order

### 6.1 Now, on today's collector

None of these depends on the redesign. Items 1–9 are about 2–3 engineer-weeks [I]; all thirteen about 5–7.

| # | Action | Closes or catches | Cost [I] | In the PRD? |
|---|---|---|---|---|
| 1 | Debug freed-bitset in every accessor, including vectors, strings and the nine bypassing accessors; `frame_globals`' fallback panics in debug; FIFO quarantine in debug lanes, never-reuse in zeal | X-1, X-2, X-3; **C** A1; **C\*** A4, A6, A7 (with item 5, **C** A6, A7) | 1–3 days | from 5b only |
| 2 | GC_DESIGN §11.5's assertion, and `trace_children` assertions on `Free` or poison | X-4; **C** A1; **C\*** A4 | 1 hour | verifier at 5a |
| 3 | `PATINA_GC_ZEAL=every` and `random:<seed>:<p>` | X-5, X-6 | hours | `entry` at stage 3 |
| 4 | Stress or zeal in debug over `cargo test -p patina-tests`, the matrix and both Larceny lanes (nightly if not per PR); the `gc-poison` feature to run them optimized | X-5; **C\*** for every class | 1 day plus CI | not for these suites |
| 5 | Embedding regression tests for A6 and A7 on both backends; add A7 to #605's acceptance | **C** A6, A7 | 1 day | A6 via #605; A7 no |
| 6 | Exhaustive destructuring in every trace function, with a grep check; then `derive(Trace)` | **P** A2, A3, R4-L1 | 2 days, then 1 week | no |
| 7 | `AssertNoGc` scopes on the comment-held windows (§5.2) | **C** A5, B1, R3-L1, R3-L2 shapes | hours | no |
| 8 | `clippy.toml`: `disallowed-methods` on the collecting entry points, the collector, the raw-bits accessor and `Environment::with_parent` outside core; `disallowed-macros` on `thread_local!` | **F** A1, R7, R8-L2 | 1 day | the C9 check at stage 3 only |
| 9 | Make the collector entry points crate-private; release asserts for heap identity; `ParsedLibrary::new` without `Option` | **P** R8-L1, R2-L8; **C** R8-L3, R8-L4 | hours | stage 3 |
| 10 | `DEAD_SLOT` from `retire_registers` with a `reg_at` assertion; then the liveness recomputation | R6-L1 to L3 | hours, then about 1 week | `DEAD_SLOT` 4d/5b; the recomputation no |
| 11 | Checklists, review rules, the census | R4-L3, process | 1–2 days | stage 0 inventory only |
| 12 | Mutation testing of trace code (after approval) | missing regression tests | 2 days | no |
| 13 | The #587 generator under zeal | R3, R5, R6 shapes | 1–2 weeks | no |

### 6.2 By stage

These confirm the PRD's items and add the gaps.

| When | What lands | Gaps to add |
|---|---|---|
| Before stage 1 | — | items 1–4 in CI. Stage 1's byte trigger moves collections to new points, which can surface latent hazards; that is welcome only if a detector is watching |
| Before stage 2 | — | an ignore-defer discovery run over loading. Stage 2 lets library bodies collect at points A, B and D (R2-L8, R2-L9) |
| Stage 2 | `Owned`, slot visitor, rooted loading, `PENDING_ESCAPE` as an evaluator field | the host-environment handle (A7); the embedding lane |
| Stage 3 | `Cx`, the brand, trybuild, `NoGcScope`, zeal-`entry`, the collect capability | a debug `in_leaf` assertion for helper classes |
| Before 4e | — | ignore-defer discovery for driver-level nested loops; tests that make deferral observable |
| 4a / 4e | `%parameterize-swap!` stops calling back; design A deletes the weak tables | — |
| 5a | `declare_layouts!`, the verifier, the conformance suite, Miri | `derive(Trace)` for `HostPayload`; the sanity trace |
| 5b | eager poisoning sweep, quarantine, `PROT_NONE`, `DEAD_SLOT` fill | longer quarantine under zeal |
| 7 | zeal-minor | a sanity trace after each minor |
| 8 | `move-all` | — |

---

## 7. Residual risk once the PRD and the additions have landed

- **R3 inside the island.** Creating a new parked window stays review-dependent. Activating one is caught by
  `AssertNoGc` and zeal-`entry`.
- **R6 maps.** These are caught at compile time by the recomputation, and at run time by `DEAD_SLOT`. A wrong analysis
  of a new kind, of the #614 sort, still needs its own assertion, which is the checklist's job.
- **The raw-word API** (`pub unsafe`). It stays sound only as long as its callers are few. Keep it under clippy and the
  census.
- **The tree-walker** never moves (decision 1), so `move-all` never covers it. Its host-payload traces depend entirely
  on the derive. That is the main reason the derive should not wait for stage 5.
- **Hidden references** (PRD §20, "Julia's lesson"). These are covered by the census, `VERIFY_ROOTS` and the poison
  and `move-all` lanes. The census is the one piece of that set with no PRD home.
