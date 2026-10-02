# Premature collection: what can land now, on today's collector

Inputs: `catalogue.md` (incidents A1–A7, near relatives B1–B6, classes R1–R8 and X) and `defenses-hazards.md` in this
directory. Source state: `gc-prd` at `f82e8e8`, which is `main` at `28a94f8` plus one PRD-only commit. During this work
`gc-prd` moved to `3682302`, which changes only `PRD/` files, so every source reference still holds and the GC_PRD
section numbers cited are unchanged. The repository was not modified by this work. Every measurement below was taken on 2026-10-01 on this machine (macOS, 12 cores), from scratch
copies of the tree built in `scratch/target/`. Timings are single runs. The chibi timings in §3.2 ran with at most one
other single-threaded job, so differences under about 5% are noise; where heavier parallel load ran, the text says
so. CI runners are slower, so read the timings as ratios.

The scratch copies, all under `scratch/`:

| Copy | Contents | Built as |
|---|---|---|
| `base` | the pristine tree | `target/base/{debug,release}` |
| `exp` | `base` + a freed-slot bitset checked by every arena accessor, the "free slot reached by marking" panic, a quarantine knob (`PATINA_GC_QUARANTINE=k`) and a zeal mode (`PATINA_GC_ZEAL=1`) | debug; release with `--features patina-core/gc-check` |
| `exp2` | `exp` + debug generation stamps in `TaggedValue`, checked by the accessors and by the marker | the same |
| `exp3` | `exp2` + `DEAD_SLOT` written by register retirement, and a panic in `reg_at` that reads one | the same |
| `exp4` | `base` + an exhaustive destructure of `CompiledMacro` in the trace, and a defer-depth assertion at collection | debug |
| `exp5` | `exp3` + a deliberately wrong liveness map (a mutation test of the `DEAD_SLOT` detector) | release, plain and `gc-check` |

`patch1.py`, `patch2.py` and `patch3.py` in `scratch/` reproduce `exp`, its zeal mode and `exp2` from `base`. The probe
crate (`scratch/probe*/`) is the one `defenses-hazards.md` §2 describes, plus a write-back case.

---

## 1. Answer

Every incident in the catalogue shares one fact: a live value sat where no root provider looks, at a moment when a
collection could run. The measures below do not change the collector. They do three things:

- **Fix the live incidents.** Root what the host holds (#605).
- **Make the next incident loud.** Make every stale reference panic, in debug builds and in a release check build. That
  covers a read after free, a read after the slot was reused, and a stale value that later reaches the marker.
- **Turn rules held by comments into checks.** Name every field in each trace function, and assert the deferral
  protocol and the register-retirement maps.

Ranked by cases caught per unit of cost. The case IDs are the catalogue's. Costs are measured where a number is given.

| # | Measure | Catches | Cost | Issue |
|---|---|---|---|---|
| 1 | Panic when marking reaches a slot that is already free (GC_DESIGN §11 item 5, never implemented) | write-back shapes of A1, A6, A7: any dangling root or traced edge, at the next collection, before its slot is reused | 3 lines; one branch per free slot per collection; no false positive on the chibi suite under stress on both backends | first commit of the issue for #2 |
| 2 | **Stale-reference checks** in debug and `gc-check` builds: a freed-slot bitset checked by every arena accessor, plus generation stamps in `TaggedValue` checked by accessors and by the marker | all five probe shapes of A6 and A7 (vector, string, bytevector, reused pair, child env), write-back, A1; A2, A3 and A4 at the first read, even after reuse | about 200 lines in `patina-core/src/heap` and `tagged_value.rs` (the experiment: `exp2.diff` less its zeal, quarantine and report code). Release `gc-check`: within noise (VM 8.7 → 9.0 s, tree-walker 12.1 → 12.1 s, chibi stress 16). Debug, as naively written: +28% VM (72.9 → 93.1 s), +9% tree-walker | **new** |
| 3 | Run the release GC lane on a `gc-check` build at stress 1 instead of 16 | the classes of #2, at 14× the collection density (129,012 against 9,308 collections) | about +4.5 min per run locally (129 s VM + 162 s TW with every check, against 9 + 12 s at 16) | with #2 |
| 4 | **#605**: rooted `Owned` handles from `eval_*` and `run_forms` | A6 and A7, the only two live incidents; R1 latent 1 and 3 | moderate (API, deprecations); gated on #604 | exists; add A7, the write-back shape and a check-build run to its acceptance |
| 5 | Field-exhaustive trace patterns | A2, A3, B2 at compile time; forces a decision on latent R4 #1 (`foreign_expansions`), which `exp4` shows | zero runtime; about a day | **new** |
| 6 | Defer-depth assertion at collection, a guard-extent check, and unconditional defer balance | R2 latent #11 and #12; any nested loop that starts collecting (stage 4e regressions) | about 15 lines, cold path only | **new** (small) |
| 7 | `DEAD_SLOT` for retired registers, read-asserted in `reg_at` | R6 latent 1–3: a wrong per-pc liveness map, which today writes a legal `UNSPECIFIED` | about 15 lines; the mutation test turns a misattributed type error into `read of a retired register r1 at pc 2` | **new**, with #8 |
| 8 | Zeal (collect at every outermost safe point), run on a subset | R6 at pcs no allocation precedes; R3 windows if a safe point is ever added inside one | 10 lines; 7.4× stress 1 on the full suite (920 s VM), so a ten-file subset only: about 7.5 min (§3.8) | with #7 |
| 9 | Stress on more programs: Larceny and `cargo test`, in the `gc-check` build | whatever shapes those suites contain that chibi lacks: ephemerons, prompts, callbacks, the hygiene and control matrices | 13 selected `cargo test` targets: 42 s under stress 16 (8 s default) per PR; Larceny needs per-suite intervals (2001 s at 16, with 6 suites timing out) | **new** (CI) |
| 10 | Crate-private collection entry points | R8 latent 1 | about 10 lines | fold into #6 |

Not recommended now (§4): a "verify" mode that re-traces with the same rules or another visitor order; quarantine of
freed slots; collection epochs stamped into values; a release poison mode switched by an environment variable.

What none of this does: it does not make a missed root impossible. Only the PRD's capability split (`&mut Heap`
against `Cx<'gc>`), derived trace rules and rooted handles do that (GC_PRD §11.3, §14). These measures make today's
invariant *checked* until then, and the same checks keep working as the redesign's lanes.

---

## 2. Probe results: what each build reports

The probe holds a value across a later collecting `eval_program` (200,000 churned pairs plus `(gc)`), then reads it.
The write-back case instead binds the stale value into the global environment and calls `(gc)` without reading it.

| Shape (catalogue ID) | `base`, debug | `base`, release | check build: debug, or release `gc-check` |
|---|---|---|---|
| held vector (A6) | prints `#()`, no panic | `#()` | `use-after-free: vector slot 0 was reclaimed by the GC` |
| held string (A6) | prints `""`, no panic | `""` | `use-after-free: string slot 32 was reclaimed by the GC` |
| held bytevector (A6) | panics (object `Free`) | `#<gc-freed-slot>` | panics |
| host child env, tree-walker (A7) | panics `pair slot 16004` | `(45295 . 45295)` | panics `pair slot 16004` |
| held pair, slot since reused by 300k live pairs (A6) | prints `(49255 49254 …)`, part of the new list, **no panic** | the same | **`stale pair reference, slot 15990 generation 0 (now 3): freed and reused since this value was made`** (generations only) |
| write-back: stale pair bound into the global env, then `(gc)` | **no detection** | no detection | `dangling reference: pair slot 15990 is free, but marking reached it` (#1), or `dangling reference reached by marking` (#2) |

Quarantine (`exp`, `PATINA_GC_QUARANTINE=2`) did not catch the reused-pair case: the 300k allocations ran more than two
collections, so the slot was reused anyway. Generation stamps caught it on both backends.

---

## 3. The measures

### 3.1 Panic when marking reaches a free slot (#1)

**What.** `sweep_arena` pre-marks every free-list slot so that sweep does not free it twice, and ignores whether the
bit was already set (`crates/patina-core/src/heap/gc.rs:836-838`). A bit that was already set means marking reached a
slot that was free when the collection began. A root or a traced edge names a dead slot. GC_DESIGN §11 item 5
(`docs/GC_DESIGN.md:840-841`) planned this assertion; it was never written.

```rust
for &idx in free_list.iter() {
    if !marks.set(idx as usize) && GC_CHECK {
        panic!("dangling reference: {arena} slot {idx} is free, but marking reached it");
    }
}
```

**Catches.** A stale value that is written back into anything traced before its slot is reused: a Rust holder (A1's
`saved_globals`, R2's `olds`, R3's parked values) or the host (A6, A7) handing it back. It fires at the next collection even if nothing ever reads the value. Under stress the next
collection comes within 16 allocations, so a slot is rarely reused before it, except the few highest indices that LIFO
reuse takes first. It cannot see a value that is never re-reached, or one whose slot was reused first; #2 covers both.

**Measured.** The write-back probe panics on both backends; `base` reports nothing. The chibi suite under
`PATINA_GC_STRESS=16` is byte-identical to GC-off on both backends in debug, so there is no false positive. The VM
traces its whole register vector (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:75`), so stale words above the top
frame are retained, not dangling. Cost: one branch per free slot per collection.

**Issue.** The first commit of #2's issue. It can also land alone, today.

### 3.2 Stale-reference checks (#2)

Three parts. All are compiled in when `GC_CHECK = cfg!(any(debug_assertions, feature = "gc-check"))`, with a new
`gc-check` feature on `patina-core`. The shipped release binary is unchanged.

**(a) A freed-slot bitset per arena, checked by every accessor.** Sweep sets the bit for each slot it frees
(`sweep_arena`, `gc.rs:828-850`). The reuse branch of each `alloc_*` clears it (`heap/mod.rs:705`, `:772`, `:833`,
`:1471`). Every accessor panics on a set bit:
- `get_pair`, `set_car` and `set_cdr` (`:718-763`);
- the five vector accessors (`:790-819`) and the three string accessors (`:851-868`). These two arenas have no detection
  today, because their tombstone `Vec::new()` is a legal value (`gc.rs:917-936`);
- `get_object` (`:1484`);
- the nine accessors that index the object arena directly and answer `None` on a `Free` slot:
  `get_exception` (`:962`), the promise write (`:1085`), `retire_vm_closure` (`:1351`), `get_vm_closure_globals`
  (`:1366`, which feeds `frame_globals`' silent fallback to the global environment, `vm_state.rs:1357-1363`),
  `get_vm_closure_free_var` (`:1380`), `set_vm_closure_free_var` (`:1394`), and the three bytevector writers
  (`:2871`, `:2886`, `:2899`). The experiment did not instrument the promise write at `:1085`; the issue should.

This set is closed. The arenas are private fields of `Heap`, so no code outside `patina-core/src/heap/` can index
one, and inside it only the collector's trace and sweep do so directly.

**(b) Generation stamps.** A pair, vector, string or object `TaggedValue` carries a 16-bit allocation generation in
payload bits 40–55. `heap_index()` reads `(bits >> 3) as u32`, bits 3–34 (`tagged_value.rs:410-413`), so it ignores
them with no change. Each arena keeps a debug-only `Vec<u16>` of current generations, bumped when sweep frees a slot.
- **Stamped where a reference is built from an index.** There are exactly 12 such sites, all in `patina-core/src/heap`:
  - the four `alloc_*` returns (`heap/mod.rs:713`, `:780`, `:841`, `:1479`);
  - the interned-symbol and core-syntax lookups (`:983`, `:999`);
  - the four `record_freed_bits` calls in `sweep` (`gc.rs:914-944`), which must record the *pre-bump* generation so
    that `SourceMap` pruning still matches its raw-bit keys;
  - `visit_object_index` (`gc.rs:514`) and the `syntax_sources` prune (`gc.rs:895`, `from_raw`), which need no stamp
    because they read only the index.
- **Checked** by every accessor in (a) that takes a `TaggedValue`, and by `GcVisitor::visit` (`gc.rs:485`). In
  `visit`, a mismatch means a root or traced edge holds a reference to a slot that was freed *and reused*, which the
  bitset cannot see.
- **Safe for raw-bit identity.** Every copy of a value carries the same stamp, so `eq?`, raw-bit hash keys
  (`syntax_sources`, the datum writer's labels, the desugarer's `seen`) and `hash-by-identity` stay consistent. A stale
  raw-bit key stops matching a reused slot in check builds, which removes R7's misattribution there. The differential
  lanes do not compare identity-hash values (GC_PRD §16), and slot assignment already differs between GC modes.
- **Bare-index accessors** (`CallFrame.closure`, the `*_vm_closure_*` family) keep only the bitset check, since an
  index carries no generation. Frames are roots, so their closures are never freed.

**(c)** #1, above.

**Catches.**
- Every probe shape (§2), read or written back: A6 and A7.
- A1, as today.
- A2 and A3, at the first read of the swept value, *including after its slot is reused*. Today that read prints the new
  tenant.
- A4 when the continuation is invoked. Its payload is never traced in the buggy shape (the ref is marked only after the
  weak-id loop has stopped), so it is caught at the read, not at a collection.
- A5, if a safe point ever lands in its window.
- It does not catch R6 (a retired register holds a legal value; that is #7), or a dangling value that is never read and
  never reaches the marker.

**Measured** (all on the chibi suite; every run byte-identical to GC-off, with no false positive):

| Build, lane | VM | Tree-walker |
|---|---|---|
| `base` debug, stress 16 (today's "strong" CI lane) | 72.9 s | 119.2 s alone; 124.5 s in the batch below |
| debug + bitset + #1 | 94.6 s (+30%) | 133.6 s (+7%) |
| debug + bitset + #1 + generations | 93.1 s (+28%) | 135.9 s (+9%) |
| `base` release, stress 16 | 8.7 s (7.7 s alone) | 12.1 s (10.5 s alone) |
| release `gc-check`, bitset + #1 | 9.0 s | 12.3 s |
| release `gc-check`, + generations | 9.0 s | 12.1 s |
| release `gc-check`, + generations + `DEAD_SLOT`, stress 16 | 9.0 s, 9,308 collections | 12.3 s, 9,205 collections |
| the same, stress 1 | 129.0 s, 129,012 collections (117.2 s without `DEAD_SLOT`; `base`: 103.9 s) | 162.0 s, 118,623 collections (150.6 s without; `base`: 132.8 s) |
| the same, default mode | 0.38 s, 12 collections | 0.47 s, 12 collections |

The debug overhead comes from the bitset lookup in unoptimized accessors: the experiment wrote it as
`Vec::get(..).is_some_and(..)`. Plain indexing, or `[profile.dev.package.patina-core] opt-level = 1`, should recover
most of it; the issue should measure that. Release `gc-check` costs nothing measurable.

**Design notes for the issue.**
- The stamp is a property of today's encoding. GC_PRD §5 replaces it at stage 5, where the poison sweep, the
  2-collection quarantine and the verifier take over (GC_PRD §16). So keep the code inside `heap/` and small.
- A value used with another interpreter's heap fails the generation check. That is a correct refusal, and it
  anticipates #605's heap-id check.
- Make the index constructors `TaggedValue::{pair, vector, string, object, closure}` (`tagged_value.rs:376-403`)
  `pub(crate)`. Nothing outside `patina-core/src/heap/` and `tagged_value.rs`'s own tests calls them (grep), and an
  outside caller would mint an unstamped reference.
- Show the generation in `TaggedValue`'s `Debug` output in check builds. The one failing unit test below prints
  `TaggedValue::object(1)` on both sides of a failed `assert_eq!`.
- One `patina-core` unit test of 269 breaks (`heap/source.rs:234`, `assert_eq!(reused, dead)`). It should compare
  `heap_index()`, since a reused slot's value is now deliberately unequal to the dead one's.

**Issue: new.** Suggested title: "GC: make every stale reference panic in debug and `gc-check` builds".

### 3.3 Run the release GC lane on a `gc-check` build, at stress 1 (#3)

**What.** In `.github/workflows/ci.yml:92-105`, build with
`cargo build --release -p patina-repl --features patina-core/gc-check` and run
`PATINA_GC_STRESS_INTERVAL=1 ./scripts/run_gc_differential.sh`. The other jobs keep the plain release binary.

**Why.** Today the release lane cannot report a use-after-free at all; it can only show changed output. With the
check build it gets #2's panics in optimized code, at no measurable cost. Stress 1 then becomes affordable, which closes
the blind spot the interval opened in #39 ("15 of every 16 allocation sites").

**Cost.** 129 s (VM) plus 162 s (tree-walker) for the stress runs with every check of #2 and #7 compiled in, against
9 + 12 s at 16. That is about +4.5 minutes locally per job. The stress reclamation proof asserts over 1000 collections across 20k allocations, and it
holds at 1 (it measured 1255 at 16).

**Issue.** Part of #2's issue, or its own CI change once #2 lands.

### 3.4 #605: rooted handles for the host (#4)

This is the only measure that removes a live incident, and it is already specified. Three additions to its acceptance:
1. **A7.** A host-built `Environment::with_parent(global)` passed to `Backend::eval` loses its bindings on the
   tree-walker: debug `pair slot 16004`, release `(45294 . 45294)`. The VM survives only because `VmBackend::eval`
   ignores `env` (`crates/patina-vm/src/backend.rs:675-693`). Either root host-held environments too (an
   `OwnedEnv`, or the handle table holding `Rc<Environment>`), or remove the public path that builds one.
2. **The write-back shape.** A stale value passed back through `global_env().define` must be impossible once results
   are handles. Today #1 and #2 panic on it.
3. **Run the acceptance tests with #2's checks compiled in** (debug once #2 lands, or `gc-check`), so the reused-slot
   case (§2, row 5) is covered. Today's debug poison misses it.

**Implementation note.** A handle table owned by the `Heap` and marked in `GcVisitor::new`, exactly as `symbol_table`
and `core_syntax_table` are (`gc.rs:449-465`), roots handles for both backends without touching either backend's root
provider. An `Owned` holds `Weak<RefCell<HandleTable>>` (or a `Weak` heap plus `try_borrow_mut`), so a drop after
teardown is a no-op, as #605 asks.

### 3.5 Field-exhaustive trace patterns (#5)

**What.** Make each trace function name every field of the struct it walks, with no `..`, so that a new field is a
compile error until it is traced or marked untraced with a reason:

```rust
HeapObjectData::Macro(m) => {
    let CompiledMacro {
        name: _, rules: _ /* literals: for_each_literal */, max_pvars: _, definition_scopes: _, heap: _,
        template_symbols: _, inherited_identifiers: _, definition_env, foreign_expansions,
    } = &**m;
    // trace definition_env, and decide foreign_expansions
}
```

Sites:
- the `Macro` arm (`gc.rs:746-753`, 9 fields);
- `trace_continuation_children` (`gc.rs:796-818`, 11 `CpsContinuation` fields; `resume` was A3);
- the `{ .. }` arms: `Exception`, `Record`, `EnvironmentSpecifier`, `VmClosure`, and `CpsLambda` under `Procedure`
  (`gc.rs:734-787`);
- `Environment`'s 9 private fields, in one `for_each_gc_edge` in `environment.rs` that `visit_env` (`gc.rs:539-562`)
  calls. `alias_bindings` was A2;
- `VmState` and `ExecutionState` in `impl GcRoots for VmState` (`gc_roots.rs:70-115`; fields at `vm_state.rs:~130-220`
  and `execution_state.rs:18-22`);
- the wind, handler, prompt and `ContValue` trace helpers.

**Measured.** In `exp4`, destructuring `CompiledMacro` without `foreign_expansions` fails with
`error[E0027]: pattern does not mention field 'foreign_expansions'` (`gc.rs:757`). That is catalogue R4 latent #1,
untraced since #462, surfacing at compile time. Adding `foreign_expansions: _` with its reason compiles.

**Catches.**
- A2 (`definition_env` and `alias_bindings`), A3 (`resume`), and B2, all at compile time.
- R4 latent #1 now; latent #2 for struct fields.
- It does **not** reach payload types matched as leaves (`Port`, `RecordType`, `PromptTag`, `Identifier`). Add one
  runtime test per `HeapObjectData` variant, built by a struct literal (so a new field also breaks the test) with a
  fresh sentinel in every value-bearing field, rooted only through that object. After `(gc)`, #2's checks make any
  untraced sentinel panic on read. That is the #164 rule codified: the value under test is reachable only through the
  edge under test.

**Cost.** Zero at run time; about a day of mechanical edits. **Issue: new.**

### 3.6 Assert the deferral protocol (#6)

**What.**
1. In `GcController::safe_point_cold` (`gc.rs:400-409`): `debug_assert_eq!(h.gc_defer_depth(), 1)`. A collection may
   run only while the outermost loop's own guard is the only one alive. `is_outermost` is hoisted at loop entry
   (`vm_state.rs:1151-1154`, `cps_eval/mod.rs:219-221`), so a guard that a callee takes and keeps past its instruction
   would not stop the running loop. That is R2 latent #11, today a comment-level argument. `exp4` adds the line. The chibi suite
   in debug, at stress 16 and in the default mode on both backends, never trips it and stays byte-identical to GC-off.
2. Record `gc_collections` in each `GcDeferGuard` (`gc.rs:232-268`). On drop, assert it is unchanged for every guard
   that was not outermost, and for the holder guards (`ParsedLibrary`, `desugar_with_imports`, `with_globals`), which
   should be built by a `GcDeferGuard::holding` constructor. A nested loop that starts collecting then fails at the
   holder that needed it deferred. That is exactly what stage 4e must not regress.
3. Make the balance check in `exit_gc_defer` (`heap/mod.rs:674-680`) an `assert!`. It runs once per loop exit and is
   off the hot path. That is R2 latent #12.
4. Add the observable-deferral test the catalogue asks for (R2): a parameter-like procedure that calls `(gc)` inside
   `parameterize`, with its old value reachable only from `olds`. Repoint the drifted
   `gc_tree_walker.rs:38-48 collection_inside_higher_order_primitive` at a path that still nests a trampoline.
5. Fold in R8: make `run_mark_phase`, `Heap::sweep`, `Collector::collect` and `GcController::collect` crate-private
   (`patina-core/src/lib.rs:70-73`). The only outside user is `patina-vm`'s
   `weak_continuation_tests.rs:18,132`, which can use a `#[doc(hidden)]` test entry.

**Catches.** R2 latent #11 and #12, misuse of a holder guard, and stage 4e regressions; R8 latent 1. It does **not**
catch A1 itself, a holder with no guard at all. #2 and the stress lanes cover that, as they did in #6.

**Issue: new, small.**

### 3.7 `DEAD_SLOT` for retired registers (#7)

**What.** `retire_registers` writes `UNSPECIFIED` into each register outside the pc's liveness map
(`gc_roots.rs:47-68`, the store at `:64`). Under `GC_CHECK`, write a dedicated special immediate instead
(`exp3` uses `0xE8 | TAG_SPECIAL`, beside `GC_POISON` and `FORWARDED` in `tagged_value.rs:103-110`), and panic in
`reg_at` (`vm_state.rs:620-623`) when it reads one. This is GC_PRD §11.1 invariant 3, moved forward.

**Measured.**
- **Mutation test** (`exp5`): drop the highest live register from every map.
  - Check build, stress 1: `read of a retired register r1 at pc 2 (liveness map said dead)`.
  - Plain build: `type error: expected a procedure, got unspecified` at an unrelated call (stress 1), or
    `length expects a proper list` (stress 16).
  - GC off: correct output.
- **No false positive**, every run byte-identical to GC-off:
  - the chibi suite at stress 16 and stress 1 on both backends (the table in §3.2);
  - the chibi suite under zeal on the VM: 953,262 collections, every one retiring registers at its pc;
  - `tests/scheme/control/*.scm` (11 files) under zeal and stress 1 on both backends.

**Catches.** R6 latent 1–3: a wrong per-pc map, a codegen change or new opcode, or a runtime landing at a pc without a
map. Today each of these writes a legal value. It catches them only at pcs where a collection happens, so it needs #3's
density or #8's zeal.

**Cost.** One compare per register read in check builds; none in release. **Issue: new**, together with #8.

### 3.8 Zeal: collect at every outermost safe point (#8)

**Today.** `PATINA_GC_STRESS=n` raises the pending flag once `n` allocations have happened since the last collection
(`heap/mod.rs:570-584`). The collection runs at the next outermost safe point. So stress 1 is not "every safe point": a
stretch of non-allocating instructions never collects. The interval is forced to at least 1 (`gc.rs:309`, `.max(1)`), and
`0` reads as unset.

**What.** Add `GcMode::Zeal`, whose threshold is 0. `GcController::collect` re-installs the threshold after sweep
(`gc.rs:350-356`), so the flag is raised again at once and every outermost safe point collects. One change is needed in
the VM loop (`vm_state.rs:1204-1208`). Its "did a collection happen" test, `pending && !gc_pending.get()`, never fires
under zeal, so `after_collection` (code release, #338) would never run. Make `maybe_collect` (`:1287-1310`) return
whether it collected, which also removes the two-load trick. `exp` does exactly this: about 10 lines.

**Measured.**
- Full chibi suite, VM, release `gc-check` with every check: **920 s, 953,262 collections**, against 129 s and 129,012
  collections at stress 1 (7.4×). Byte-identical to GC-off. The tree-walker run was stopped; at the same ratio it
  would take about 20 minutes.
- Per file, `tests/scheme/control/`: zeal costs about 7× stress 1 (`callability.scm`: 19.4 s against 2.6 s on the VM,
  28.3 s on the tree-walker; `apply.scm`: 2.8 s against 0.4 s).
- All 11 files of `tests/scheme/control/`: VM 452 s under zeal against 33 s at stress 1; tree-walker 902 s against
  49 s. `tail-recursion.scm` alone is 277 s and 627 s (27× and 42× stress 1): a long loop that allocates little
  collects at every instruction. Without it the ten files take 176 s (VM) and 275 s (tree-walker), about 7.5
  minutes together, all byte-identical to GC-off.

So zeal belongs on a subset, never the whole suite: `tests/scheme/control/*.scm` without `tail-recursion.scm`
(about 7.5 minutes locally), later the control-flow matrix and `ephemerons.rs` as Scheme files. Run it in the
`gc-check` build, nightly, or per PR that touches `patina-vm/src/runtime`, `patina-vm/src/compiler` or `heap/`.

**Catches.** R6 at pcs that no allocation precedes (with #7). R3: if a future change puts a safe point inside A5's or
B1's window (`vm_state.rs:1819-1832`, `:1865-1875`), zeal exercises it instead of leaving it argued. On its own it adds
little for R1, R2, R4 and R5 beyond stress 1.

### 3.9 Stress on more programs (#9)

**Gap.** The stress lanes run only the chibi suite, through the CLI. The catalogue's blind spots X.5 and X.6 follow:
no ephemerons, few prompts, and no embedding.

**Measured.**
- **Larceny R7RS lane, VM.**
  - Plain release, as `scripts/run_larceny_tests.sh` runs it today (it is not in CI): 53 s, 8509 of 8536 assertions.
  - The same lane, `gc-check` build with every check, at stress 16: 2001 s. 27 of 33 suites finish with the same
    tallies as the plain run, and no check fires. Six exceed the script's 300 s budget: `char`, `ephemeron`, `flonum`,
    `lazy`, `sort` and `stream`, which take 0–36 s plain.
  - The six alone at larger intervals, in 12 parallel jobs (so inflated):

    | Suite | plain | N=4096 | N=256 |
    |---|---|---|---|
    | `sort` | 1 s | 3 s | 24 s |
    | `flonum` | 1 s | 3 s | 36 s |
    | `lazy` | 5 s | 9 s | 52 s |
    | `stream` | 36 s | 69 s | 518 s |
    | `char` | 2 s | 72 s | over 900 s |
    | `ephemeron` | 4 s | over 900 s | over 900 s |

  - A collection costs time proportional to the live heap, so a stress lane costs about
    allocations × live / N. Suites with a large live heap or many ephemerons blow up. `ephemeron` is #609: every
    collection rescans the pending ephemerons, so that suite cannot run under a useful interval until #609 lands.
- **`cargo test -p patina-tests`, debug, `SKIP_CHIBI_TESTS=1`.**
  - Default mode: 105 s.
  - Whole package under `PATINA_GC_STRESS=16`: `scheme_suite.rs` alone ran over 25 minutes before I stopped it (19 s in
    the default mode).
  - Five tests fail under the variable for a reason that is the lane's, not the code's. They compare collecting with
    not collecting, or pin when code is released, and the process-wide variable makes the "not collecting" side
    collect too:
    - `collecting_keeps_the_arena_smaller_than_not_collecting` and `unreachable_cycles_are_reclaimed`, in both
      `gc_vm.rs` and `gc_tree_walker.rs`;
    - `finished_forms_release_code.rs:160`, `a_form_whose_continuation_has_died_lets_its_code_go_at_a_collection`.
  - **A selected set** of 13 GC- and control-relevant targets (`control_flow_matrix`, `escape_from_primitive`,
    `ephemerons`, `hygiene_matrix`, `interpreter_api`, `callability`, `gc_vm`, `gc_tree_walker`,
    `finished_forms_release_code`, `library_loading`, `vm_callprimitive`, `macro_definition_env`, `cps_features`;
    170 tests):

    | Build | default | stress 16 |
    |---|---|---|
    | `base` | 6 s | 43 s; the five lane-induced failures |
    | every check of #2 and #7 (debug) | 8 s | 42 s; the same five, and no check fires |

  - `patina-core`'s unit tests with the checks: 268 of 269 pass. The one that fails (`heap/source.rs:234`) asserts
    `assert_eq!(reused, dead)`, that a reused slot's value equals the dead one's. The generation stamp makes that
    false by design; the test should compare `heap_index()`.

**Proposal.**
- **Per PR:** the 13-target set under stress 16 in debug with the checks, about 45 s. First make the five
  lane-sensitive tests choose their GC mode per interpreter rather than from the process environment, or skip under
  `PATINA_GC_STRESS`.
- **Nightly:** both Larceny lanes with a per-suite interval: 16 for the 27 cheap suites, 4096 for `char`, `flonum`,
  `lazy`, `sort` and `stream`. Leave `ephemeron` out until #609 lands, and put "the ephemeron suite under stress" in
  #609's acceptance. Also `scheme_suite.rs` at a larger interval, after finding which file dominates.
- Each run asserts that it collected (#606's rule: `bytes-reclaimed > 0`, or a pinned collection count). #587's
  generator joins the same lane when it lands.

**Issue: new** (CI).

---

## 4. Evaluated and not recommended now

**A "verify" mode that runs an independent second trace, or re-marks in another visitor order.**
- A second trace that uses the same trace rules produces the same mark set, so it is blind to the same missing edges.
  That blindness is R4, the class with the most incidents.
- Marking is a monotone fixpoint, so worklist order, root-provider order or BFS against DFS cannot change a correct
  result.
- A4 was a *phase-structure* bug: the weak-id loop ended before the ephemeron loop. Reordering the visitor would not
  have exposed it. A *naive reference fixpoint* would have: repeat {roots, weak payloads of marked refs, ephemerons with
  marked keys} until nothing changes, then compare the mark sets.
- That oracle needs a side-effect-free mark, because `run_mark_phase` breaks ephemerons and prunes the weak tables
  (`gc.rs:1093-1097`).
- It protects exactly one planned change, #609's rewrite of the ephemeron loop. **Put it in #609's acceptance**, not in
  a standalone mode.
- The "verify" checks that do have power are #1 and the marker check in #2. Both test each traced reference against
  the free state of its slot, and that state is independent of the trace rules.

**Quarantine** (delaying reuse of a freed slot for K collections).
- Measured in `exp`. At K=2 the default-mode reclamation proof fails: 186,711 pairs against the bound of 150,000
  (`run_gc_differential.sh`, churn-default). K=1 passes at 131,072, and the stress proof passes at both.
- It did not catch the long-held probe value (§2), and generation stamps catch reuse at any distance.
- Without a check build it detects nothing, because nothing asserts.
- GC_PRD §16 keeps a 2-collection quarantine for the new heap, where lazy sweep makes reuse harder to observe. Today the
  generation stamp dominates it.

**A collection epoch stamped into debug-only values or handles.** This checks the wrong predicate. A value rooted
correctly legitimately survives many collections, so "this value has lived through a collection" is not a bug. The bug
is "this value's slot was freed", and that is what the generation stamp checks. The per-*holder* form of an epoch
(a Rust window that must see no collection) is right, and it is #6.2. The per-*handle* form is #605's generation.

**Release poison switched by an environment variable.** Every hot accessor in the shipped binary would test a runtime
flag. The `gc-check` cargo feature gets the same lane with no cost to the production binary.

**Generation counters as a separate item.** They are #2(b). The encoding made them cheap: there are 29 spare payload
bits above the 32-bit index, and only 12 construction sites, all inside `heap/`.

---

## 5. Issues to file, in order

1. **GC: make every stale reference panic in debug and `gc-check` builds** (#1, #2 and #3).
   - Acceptance: the five probe shapes plus write-back panic on both backends, as `patina-tests` tests run in the debug
     lane, with the embedding ones `#[ignore = "#605"]` until #605 lands.
   - The chibi stress lanes stay byte-identical.
   - Debug overhead is measured and reduced.
   - The release GC job builds `gc-check` and runs stress 1.
2. **#605** (exists): add A7, write-back and the `gc-check` run to its acceptance (§3.4). It stays after #604.
3. **GC: name every field in the trace functions** (#5), with the per-variant sentinel test. Decide
   `foreign_expansions` there, or under #614.
4. **GC: assert the deferral protocol** (#6, with R8's crate-private entry points).
5. **VM: `DEAD_SLOT` for retired registers, and a zeal lane on the control-flow subset** (#7 and #8).
6. **CI: stress the Larceny lanes and `cargo test` in the `gc-check` build** (#9).
7. **#609** (exists): add the naive-fixpoint mark oracle to its acceptance (§4).

Items 1, 3 and 4 are independent and each about a day. Item 1 makes every catalogued incident except A5 panic
deterministically as soon as a program reads the stale value or hands it back to anything traced, even after its slot
is reused. A5 had no safe point in its window. Today's debug poison does this only for pairs and objects, and only
before the slot is reused. Item 3 moves A2's and
A3's class to compile time. A6 and A7 still need #605 to stop happening, and a test that holds a host value, because
no CLI lane can reach them.

---

## 6. Reproducing

Everything is under
`<scratch>/`
(`$S` below).

| What | How |
|---|---|
| The copies | `base` is `git archive HEAD`. `exp` is `base` + `patch1.py` + `patch2.py`, and `exp2` is `exp` + `patch3.py`. `exp3`, `exp4` and `exp5` were edited inline from `exp2`, `base` and `exp3`. `$S/<copy>.diff` holds each copy's diff against `base` |
| Check builds | `cd $S/exp3 && CARGO_TARGET_DIR=$S/target/exp3-check cargo build --release -p patina-repl --features patina-core/gc-check --bin patina`. Each target directory needs a `lib` symlink to its copy's `lib/`, because the binary looks in `exe/../lib` under `PATINA_ISOLATED_LIBRARIES=1` |
| Chibi lanes | `$S/timeit.sh <copy> <binary> <label> ENV=… -- [--tree-walker]`; `timeit-gc.sh` also prints `(gc-stats)` collections. Outputs are in `$S/outs/` |
| Batches | `timing-batch.sh` (§3.2), `zeal-subset.sh` (§3.8), `larceny-batch.sh` and `larc-heavy.sh` (§3.9), `cargo-test-sel.sh` (§3.9) |
| Probes | `$S/probe-base`, `$S/probe2` (`exp`) and `$S/probe3` (`exp2`): `vec-vm`, `env-tw`, `reuse-vm`, `wb-vm` and their twins (`str-vm` in `probe3` and `probe-base` only), run from the copy's root with `PATINA_LIBRARY_PATH=$PWD/lib` |

Every chibi output was compared with GC-off after normalising colour and timings, and all are byte-identical.
