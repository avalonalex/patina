# Gap: finalization, weak references and everything the sweep does for free

Scope: (1) every behaviour that depends on the sweep visiting dead objects; (2) the semantics each one actually owes, measured against chibi 0.12 and Gauche 0.9.15; (3) which weak and finalization facilities Patina should ship; (4) mechanisms that work with a copying nursery plus a non-moving old space, including generational ephemeron rules and what the existing tests force. Repo: `main` at `28a94f8`, read-only.

Labels: **[V]** I verified it by reading source or running a measurement. **[I]** Inference or proposal.

Reproduction lives under `PRD/study/gc/probes/finalization-weak-semantics/`:
- `progs/` holds the oracle programs; `run.sh` and `larceny.sh` are the drivers.
- `teardown/` is the embedder-teardown probe crate.
- `suites/` held copies of the chibi and Larceny suites (third-party; not retained).
- `larceny-logs/` holds the suite logs.
- `bin/patina` is a copy of the release binary built from `28a94f8`, with `lib -> repo/lib` beside it.

---

## 0. Findings in brief

1. **The sweep carries four obligations that no evacuating nursery will meet by accident** [V]:
   - It enumerates dead slots for two consumers: SourceMap pruning and VM code-unit release.
   - It runs Rust `Drop` on every dead payload. That flushes and closes file ports, commits `MemoryFs` writers, breaks closure↔environment `Rc` cycles and frees all payload memory.
   - It lets `OUTPUT_FILES` forget dead ports.
   - Everything else that looks "weak" (ephemerons, weak continuation tables, `syntax_sources`) is mark-based and needs only a liveness/forwarding query.
2. **Drop is not a corner case.** In a mixed workload, 47% of the 770,288 allocations carry a Drop payload [V]. Every `VmClosure` (227,233 of them, 29% of all allocations) holds an `Rc<Environment>` and a `Vec`. Library loading allocates 92,414 `Identifier`s (each with an `Rc<str>` and a `SmallVec`) out of 93,241 objects (heap-repr.md). A per-object needs-drop list would cost about as much as the sweep it replaces. **The object model must become Drop-free first.** Finalization should then be reserved for a handful of resource kinds.
3. **Port finalization semantics, according to the oracles** [V]:
   - chibi and Gauche both flush a garbage port's buffer when a GC reclaims it, whether `(gc)` is called explicitly or allocation triggers it. Patina does too, today.
   - All three keep unflushed output at exit. On `emergency-exit`, Gauche drops it.
   - **Descriptors:** with `ulimit -n 1024` and 100,000 unclosed `open-input-file` calls, chibi (GC-and-retry on `EMFILE`, `eval.c:1300-1323`) and Gauche (a byte-driven Boehm trigger) both finish. **Patina fails at the 1,021st open on both backends**, because its slot-count trigger never notices 8 KiB port buffers or descriptors.
4. **Nothing pins GC-time flushing or closing** [V]:
   - The chibi suite gives 1226/1226 with `PATINA_GC=0` on both backends.
   - All 32 Larceny R7RS suites except `ephemeron` produce byte-identical logs with GC on and off on the VM, as do 8 port-touching suites on the tree-walker and R6RS `io/simple`.
   - No Rust test exercises it.

   The only pinned port behaviour is the exit flush (#343/#346, `crates/patina-repl/tests/unclosed_output_ports.rs`). Mid-program GC flushing is oracle-agreed but unpinned.
5. **Two port defects found in passing** [V]:
   - `(eq? (current-output-port) (current-output-port))` is `#f` in Patina and `#t` in chibi and Gauche. The cause is that `ports.rs:463` allocates a new wrapper on every call. `parameterize`d ports fail `eq?` too.
   - An embedder that drops an `Interpreter` loses the output in reachable but unclosed file ports. The heap leaks with `strong_count = 36`, so the `BufWriter` never drops, and only `end_process` calls `flush_open_output_files`.
6. **Feature scope** [V]:
   - SRFI 254 "Ephemerons and Guardians" reached **final on 2026-06-30**. It supersedes SRFI 246 (guardians, withdrawn 2024-09-11) and is "see also" for SRFI 124 (final 2015).
   - It adds guardians and *transport cell guardians*, which are address-hash support for moving collectors. The bundling policy makes it eligible.
   - SRFI 125's weakness arguments are only "should": its text has no error clause, contrary to the comment at `lib/srfi/125.sld:67`.
   - No library in the compat corpus needs working weakness. `in-progress/hash/tables` signals an error for it.
7. **The ephemeron fixpoint is quadratic in practice** [V]. A 16,000-ephemeron chain costs **289 ms** in one `(gc)` in the bad discovery order and **0.24 ms** in the good one. From 1k to 16k: 0.98 → 4.0 → 15.0 → 57.4 → 289 ms.
8. **The ephemeron tests constrain the redesign more than SRFI 124 does** [V]/[I]:
   - `(gc)` must be a full collection that breaks dead-key pairs of every age.
   - Four #423 tests forbid conservative scanning of VM or JIT frames.
   - `reference-barrier` must be an opaque use in JIT code.
   - The Larceny suite needs the automatic policy to reach a major collection within about 100M pair allocations of keys allocated at load time.

---

## 1. Inventory: what depends on the sweep visiting dead objects

### 1.1 Explicit sweep-time hooks (enumerate the dead) [V]

| # | Hook | Where | Consumer |
|---|---|---|---|
| H1 | `record_freed_bits`: the raw bits of every freed slot, capped at `GC_FREED_BITS_CAP = 65,536`, then `Overflowed` | `heap/gc.rs:857-873`, called per arena in `sweep` `:891-961`; buffer `heap/mod.rs:401` | `prune_freed_locations` (`source_map.rs:255-261`) at form boundaries: `patina-interpreter/src/lib.rs:512`, `parser/mod.rs:206,231`, `patina-repl/src/program_stream.rs`. On overflow, `clear_locations`. |
| H2 | `code_id` of every freed `VmClosure` | `gc.rs:943-948`; buffer `heap/mod.rs:412`; drained `:636` | `VmState::after_collection` (`vm_state.rs:542-575`) decrements `CodeObject.live_closures`, then `unit_in_use` (`:1003`) and `release_unit` drop code (#338, #353). `retire_vm_closure` (`heap/mod.rs:1351`) pre-counts `eval`'s one-shot closures with the `RETIRED_VM_CLOSURE_CODE` sentinel. |
| H3 | Tombstone write: `HeapObjectData::Free` / `Vec::new()` | `sweep_arena` `gc.rs:828-852` | Implicit `Drop`; see 1.2. Pairs write a poison value only in debug builds. |

### 1.2 Drop-time effects (implicit finalization) [V]

`rg "impl Drop" crates` finds seven implementations:

| impl | GC-relevant? |
|---|---|
| `MemoryWriter` (`vfs.rs:294`) | **Yes.** It commits the buffer into the `MemoryFs` map (`finalize`) when a `MemoryFs` file port dies unclosed. |
| `Loading` (`library_registry.rs:25`) | No, RAII. |
| `TrampolineGuard` (`types.rs:126`) | No, RAII. |
| `GcDeferGuard` (`gc.rs:264`) | No, RAII. |
| `PhaseGuard` (`scope_trace.rs:130`) | No, RAII. |
| Two `TempFile`s | Tests only. |

The GC-relevant effects come mostly from **std Drop impls inside payloads**:

| Effect | Chain | Notes |
|---|---|---|
| Flush a file output port | `HeapObjectData::Port(Rc<Port>)` → `Rc<RefCell<PortData>>` → `FileHandle::Output(Box<dyn WritePort>)` → `BufWriter<File>` (`vfs.rs:119`) | `BufWriter::drop` flushes and **ignores errors**. It fires only when the *last* `Rc<Port>` drops. `CURRENT_*_PORT` thread-locals (`ports.rs:41-45`) and the per-call wrappers (`ports.rs:463`) share the `Rc`. |
| Close a descriptor | `File` inside `BufWriter`/`BufReader`/`WholeCharReader` | Applies to input ports too. |
| Forget a dead port at exit | `OUTPUT_FILES: Vec<Weak<RefCell<PortData>>>` (`port.rs:173-201`), pruned amortised by `strong_count() > 0` | `flush_open_output_files` (`:211`) is called only by `exit_status::end_process` (`exit_status.rs:62`). |
| Break closure↔environment cycles | `VmClosure.globals`, `Procedure(Rc<Procedure>)` → CpsLambda env, `EnvironmentSpecifier.env`, `Macro(Rc<CompiledMacro>)` → `definition_env` | Env→heap edges are bare indices and heap→env edges are owning `Rc`s, so every cycle passes through a slot (GC_DESIGN §8). Pinned by `tombstone_drops_rc_payload_breaking_env_cycle` (`gc.rs:1429`). |
| Release tree-walker code and continuation trees | `Procedure` (`Rc<CpsExpr>`, `Rc<Environment>`), `Continuation(Rc<CpsContinuation>)` | tree-walker.md §1.9. |
| Free payload memory | 19 of 28 variants (offheap.md §3.6) plus every vector and string slot | See the census below. |

**Census of allocated kinds** [V]. I ran the `census` binary of `PRD/study/gc/probes/heap-repr` (`src/bin/census.rs`) under `PATINA_GC=0`, so nothing was freed, on `progs/w1.scm`. The workload mixes records, flonums, closures, a SRFI 69 table, strings, a bignum and `call/cc`.

| Arena / kind | Count | Drop payload? |
|---|---|---|
| pairs | 231,890 | no |
| vectors | 25,009 | `Vec` |
| strings | 65,050 | `Vec<char>` |
| **VmClosure** | **227,233** | `Rc<Environment>` + `Vec` |
| Real | 100,000 | no |
| MutableCell | 74,418 | no |
| Identifier | 24,153 | `Rc<str>` + `SmallVec<[ScopeId;3]>` (`scope.rs:110-114`) |
| Record | 20,001 | 2 × `Rc` |
| VmContinuationRef | 2,000 | no (the side table is mark-based) |
| Procedure / BigInt / Macro / Symbol / RecordType | 314 / 163 / 20 / 10 / 2 | yes |

**Total: 361,955 of 770,288 allocations (47%) carry Drop.**

### 1.3 Mark-based weak processing (survives a redesign if it can query liveness and forwarding) [V]

- **Ephemerons.** Broken at the end of `run_mark_phase` (`gc.rs:1022-1101`, break at `:1093`).
- **Weak continuation stores.** `trace_weak_ids` / `sweep_weak` (`gc.rs:1097`; `vm_state/gc_roots.rs:132-149`).
- **`syntax_sources`.** `retain(marked)` and then shrink (`gc.rs:891-900`).

  These need two capabilities from a moving collector:
  - **updatable** visitors. Today they take `&self` and copy values.
  - a **generational rule** (§4.4).

---

## 2. Required semantics, with oracle evidence

### 2.1 Experiments [V]

Programs are in `progs/`. Each ran under `timeout -s KILL`, with `$?` read immediately afterwards.

**E1/E1b: a port that becomes garbage mid-program.** The program opens a file, writes `hello` and drops the port. It reads the file back before and after a collection, once with an explicit `(gc)` (E1) and once with a collection triggered by allocating about 8M vector slots (E1b).

| | before | after GC |
|---|---|---|
| chibi | `""` | `"hello"` |
| Gauche | `""` | `"hello"` |
| Patina VM | `""` | `"hello"` |
| Patina TW | `""` | `"hello"` |

**E1c–e: an unreachable, unclosed port when the program ends.** The table shows the file contents.

| End of program | chibi | Gauche | Patina VM/TW |
|---|---|---|---|
| runs off its end | hello | hello | hello |
| `(exit 0)` | hello | hello | hello |
| `(emergency-exit 0)` | hello | **(empty)** | hello |

This matches the #343 table for *reachable* ports in `unclosed_output_ports.rs:9-22`. Mechanisms differ:
- **Gauche:** `active_buffered_ports` is a 256-entry weak vector flushed by `Scm_FlushAllPorts` (`src/port.c:1280-1400`). Garbage ports not yet collected are still in it.
- **chibi:** finalizes at context teardown.
- **Patina:** an unswept port's `Weak` still upgrades at exit.

**E2: descriptor exhaustion** (unclosed opens in a loop, `guard` reports the failing iteration).

| Setup | chibi | Gauche | Patina VM | Patina TW |
|---|---|---|---|---|
| `ulimit -n 128`, 5,000 input opens | ok 5000 | fails at 124 | fails at 125 | fails at 125 |
| `ulimit -n 128`, 5,000 output opens | ok 5000 | fails at 124 (not a `file-error`) | fails at 125 | fails at 125 |
| `ulimit -n 1024`, 100,000 input opens | **ok** | **ok** | **fails at 1021** | **fails at 1021** |
| `ulimit -n 1024`, 20,000 opens plus a 50-element list each | ok | ok | fails at 1660 | fails at 1661 |
| control: ports kept in a list, `ulimit -n 128` | fails at 124 | fails at 124 | fails at 125 | fails at 125 |

How each implementation behaves:
- **chibi** collects and retries once on `EMFILE` (`eval.c:1300-1323`, `sexp_out_of_file_descriptors` in `sexp.h:69`). Its finalizer `sexp_finalize_port` flushes and closes (`sexp.c:204-232`).
- **Gauche** registers `port_finalize` for file ports (`port.c:256-316`), and Boehm's byte-based trigger counts the port buffer. Finalizers run on demand at a VM attention point (`core.c:186-187,421-436`; `vm.c:3935`). Gauche also forces `GC_gcollect` when its port table fills (`port.c:1301-1333`).
- **Patina** collects after `max(65,536, 2 × live)` slot allocations and is blind to buffers and descriptors (gc-impl.md §4). In the work variant one collection happens, frees the descriptors, and the next one is too far off.

**E3: 400 live output ports, each holding one byte.** chibi and Patina keep all 400 bytes at exit. Gauche aborts with `active buffered port table overflow` (rc=1, 255 files, 0 bytes). Patina should not copy that limit.

**E4: port identity.** `(list (eq? (current-output-port) (current-output-port)) <same inside with-output-to-file> (eq? p p) (parameterize ((current-output-port p)) (eq? (current-output-port) p)))`.

| | Result |
|---|---|
| chibi, Gauche | `(#t #t #t #t)` |
| Patina VM and TW | **`(#f #f #t #f)`** |

The cause is that `ports.rs:463` returns `alloc_port(get_current_output_port())`, a new heap object every time, and `values_eq` (`heap/mod.rs:2118-2146`) has no `Port` arm. I found no existing GitHub issue.

**E5: embedder teardown** (crate `teardown/`).

| Case | VM | TW |
|---|---|---|
| Port reachable from a global, `Interpreter` dropped | file `""`, heap still alive (`strong = 36`) | same |
| …then `flush_open_output_files()` | `"hello"` | `"hello"` |
| …then returning from `main` without it | **`""` after the process exits** | same |
| Unreachable port, then `(gc)` before the drop | `"hello"` | `"hello"` |

**E6: suite dependence on GC-time finalization** (`PATINA_GC=0` against default; on copies of the suites, so the repo's `scheme_tests/reports` are untouched).

| Suite | Result |
|---|---|
| chibi suite | 1226/1226, both modes, both backends |
| Larceny R7RS lane | 32 suites on the VM, no log differs (after filtering timing lines). Same counts on the TW for `base file read write repl load eval process-context`. |
| R6RS `io/simple` | 7 of 56 failures in both modes |
| `ephemeron` | requires a GC: 6/6 on the VM in 4.0 s, peak RSS 830 MB |

All Larceny `file.sld` I/O closes its ports or uses `call-with-*`/`with-*-file`. Rust tests: nothing in `crates/patina-tests` calls `(gc)` around a port. `scripts/run_gc_differential.sh` treats any GC-on/GC-off divergence as a bug, but **GC-time flushing is an observable divergence (E1)**. The lane is green only because the chibi suite never observes it.

### 2.2 Requirements derived

| Id | Requirement | Strength |
|---|---|---|
| R1 | Output left in any file port, live or garbage, is written when the program ends, however it ends, including `emergency-exit`. | **Pinned** by 3 tests in `unclosed_output_ports.rs:64,144,164`. Oracles agree, except Gauche on `emergency-exit`. |
| R2 | When a collection proves a file port unreachable, its buffer is flushed and its descriptor closed **before the mutator next runs Scheme code**. Errors are ignored. | Both oracles (E1, E1b). Unpinned. |
| R3 | Opening a file under descriptor pressure does not fail while garbage ports hold descriptors: collect and retry once on `EMFILE`/`ENFILE`, and raise collection pressure from open file ports. | chibi plus Gauche's effective behaviour (E2). Unpinned. |
| R4 | Finalization runs no Scheme code and no allocation. chibi: "we can't run arbitrary scheme code in the finalizer" (`eval.c:1348`). | Design rule [I]. |
| R5 | Dropping an `Interpreter` (or its heap) flushes and closes its ports. | No oracle; it is process-level. E5 shows the loss. |
| R6 | One port is one object: `eq?` holds across `current-*-port` calls and `parameterize`. | Both oracles (E4). |
| R7 | Unreachable closures, environments, continuations and code trees are reclaimed (no `Rc` cycles). | GC_DESIGN §8; `gc.rs:1429`. |
| R8 | Code is never freed while a frame, continuation or closure can run it, and is released within a bounded number of collections after that stops. | `finished_forms_release_code.rs` (7 tests), `patina-vm/tests/integration.rs:376-409`. |
| R9 | Source locations may be lost but are never misattributed to a reused address. | `source_map.rs:236-245`; `interpreter_api.rs:270`; `heap/source.rs:212`. |
| R10 | Ephemeron semantics as pinned in §4.6. | `ephemerons.rs` (13 tests), Larceny `ephemeron`. |

---

## 3. Feature scope

**What exists** [V]:
- `(srfi 124)` / `(scheme ephemeron)` is a true ephemeron (`primitives/ephemeron.rs`; `heap/mod.rs:190-203`).
- For comparison, Gauche's `(scheme ephemeron)` is only a weak-vector approximation: its datum is held strongly, so a datum referring to its key keeps it alive (`Gauche/lib/scheme/ephemeron.scm`).
- chibi exposes `(chibi weak)` ephemerons. Its weak vectors are commented out.
- Patina has no weak pairs, weak boxes, weak or ephemeron tables, guardians or wills (gc-impl.md §3.3).

**Standards status** [V]:
- **SRFI 124:** final 2015-11-06. It is R7RS-large Red as `(scheme ephemeron)`. "There are no guarantees that the GC will ever break any ephemerons" (srfi-124.html).
- **SRFI 125:** the weakness symbols (`weak-keys`, `ephemeral-keys`, `weak-values`, `ephemeral-values`) are "should create". Using ephemerons for weak is allowed. There is no error clause.
- **SRFI 246 (guardians):** withdrawn 2024-09-11, superseded by 254.
- **SRFI 254 (Nieper-Wißkirchen):** final **2026-06-30**. It defines:
  - ephemerons, with *value* renamed from *datum*, plus `ephemeron-ref` and `reference-barrier`;
  - guardians: `(make-guardian)`, `(g obj rep)`, `(g)` returning a resurrected element or `#f`, with no specified ordering;
  - transport cell guardians and `current-hash`, "needed when hash values are to be derived from object addresses".
  - Its libraries are `(srfi :254 ephemerons-and-guardians …)`.

**Demand** [V]: none in `compat/vendor`. The only weakness reference is an error in `in-progress/hash/tables.body.scm:98-107`. There are no guardian users.

**Decision** [I]:

| Facility | Verdict | Reason |
|---|---|---|
| SRFI 124 / `(scheme ephemeron)` | **Keep, with stronger collector guarantees** | Shipped and R7RS-large. The tests depend on `(gc)` breaking pairs. |
| SRFI 254 ephemerons (`ephemeron-value`, `ephemeron-ref`) | **Add (cheap)** | Same object. Renames plus one procedure. Eligible under the bundling policy. |
| SRFI 125 `weak-keys`/`ephemeral-keys`/`weak-values`/`ephemeral-values` | **Support via ephemeron buckets** | Ends the silent drop. Fixes the `immutable-tables` leak (`125.sld:41-49,137-145`) with an internal ephemeral-keys table. Needs a move-stable identity hash (open question 1). |
| SRFI 254 guardians, unordered | **Add after the redesign lands, low priority** | The only user-level finalization in Patina's standards set. It costs one per-generation list plus one fixpoint step (§4.7). No demand yet. |
| SRFI 254 transport cells | **Defer, but design the hook** | Only needed if Scheme-level `eq?` tables keep address hashing. SRFI 69's `hash-by-identity` already stores arena-index hashes in buckets (offheap.md §3.7), which a moving nursery breaks anyway. |
| Weak pairs/boxes (Chez `weak-cons`), Racket wills, ordered guardians, Java-style object finalizers | **No** | No standard or consumer needs them. JEP 421 lists unpredictable latency, resurrection, always-on cost and unspecified threading, with "7-11x" overhead for finalizable classes; Java 18 deprecated finalization. Go 1.24 `AddCleanup` gives no resurrection and no guarantee at exit. |
| Internal (Rust) finalization registry, weak-id tables, rekeying weak-key tables, external-resource trigger accounting | **Required by the redesign** | §4. |

---

## 4. Mechanisms for a copying nursery plus a non-moving old space

### 4.1 Principles [I]

1. **No Rust `Drop` on any object that the nursery or JIT inline allocation can create.** Payloads become inline heap data, such as variable-length vectors, strings, bytevectors, record fields and closure free variables. Names become symbol references. Scope sets become interned ids. `Rc<Environment>` becomes a heap reference or a table id. Whatever still needs Rust ownership is either an id into a side table (Wasmtime `ExternRefHostDataTable`, `gc_runtime.rs`; tree-walker.md §5.3) or a registered finalizable resource.
2. **Every per-object obligation is a registration list, split by generation.** A minor collection walks only young registrations, and a registration is promoted together with its object. Precedents:
   - OCaml's minor custom table: `add_to_custom_table` at allocation, `custom_finalize_minor` after a minor GC (`runtime/custom.c:62-94`, `minor_gc.c:786-806`);
   - Wasmtime's per-semispace externref list, swept after copying (`copying.rs:1111-1133`);
   - rune's `drop_stack`/`lisp_hashtables` (`context.rs:197-208`);
   - Chez's per-generation `S_G.guardians[g]` (`c/gc.c:1361-1364`).
3. **Weak side structures are never older than the object they hang off.** This is Whippet's invariant ("an ephemeron E is never older than its K or V", wingolog 2025-01-09), and it extends to weak-id entries, finalization registrations and transport cells.
4. **The collector never runs Scheme code.** It only fills queues that Scheme drains: guardians (Chez, Whippet `gc_pop_finalizable`, Gambit's executable-wills list `mem.c:5032-5110`). Rust finalizers (flush, close, free) run in the collection epilogue or at the next safe point.

### 4.2 Ports: a port table plus a finalization registry (R1–R6) [I]

- **Representation.**
  - A port is a small Drop-free heap object `{hdr, port_id: u32, flags}`.
  - A per-heap `PortTable` slab owns the `PortData` (writers, readers, string buffers).
  - `current-*-port` parameters hold the **heap value**, not an `Rc<Port>`, which fixes R6.
  - Standard ports are immortal roots allocated at startup.
- **Registration.** Each table entry records its object and whether it is young or old.
  - After a minor collection: a forwarded entry is updated and moved to the old list; an unforwarded one is **finalized**.
  - After a major collection: an unmarked old entry is finalized.
  - Finalizing means flushing (ignoring errors, as `BufWriter::drop` and chibi's `sexp_flush_forced` do), closing and freeing the slot.

  This replaces both `OUTPUT_FILES` and drop-at-sweep.
- **Exit and teardown.** `end_process` flushes every live table entry, keeping #346's error reporting. Dropping the heap finalizes every entry (R5). Gauche runs `Scm_FlushAllPorts` from its cleanup handlers.
- **Pressure.** Add an external-resource term to the trigger:
  - charge each file port its buffer size (8 KiB default for `BufWriter`/`BufReader`), like OCaml `caml_alloc_custom_mem` (`custom.c:113`) or Chez phantom bytes (`c/alloc.c:1127-1161`, applied in `gc.c:1552-1558`);
  - raise `gc_pending` once N file ports have opened since the last collection. Gauche's table-full rule (`port.c:1301-1333`) is the model, and N = min(128, RLIMIT_NOFILE/4) is a reasonable choice.
- **`EMFILE`/`ENFILE` retry.** Collection happens only at safe points, so the open primitive cannot collect inside itself. It returns a distinguished failure. A Scheme wrapper, or a resumable `Step`, then requests `(gc)`, passes the next safe point and retries once (chibi `eval.c:1300-1323`).
- **`MemoryFs`.** Its finalizer calls `WritePort::finalize`, which is what `MemoryWriter::drop` (`vfs.rs:294`) does today.

### 4.3 Payload memory and `Rc` cycles (R7) [I]

- Under 4.1 this disappears: a dead object owns nothing.
- Cycles cannot form because no heap→Rust edge owns anything.
- The tree-walker's `Procedure`/`Continuation` become ids into an evaluator-owned weak-id table (tree-walker.md §5.3).
- `VmClosure.globals` becomes a heap reference to a global-environment object, or an id into a weak-id environment table (§4.4).
- **The residual needs-drop kinds should be ≤ 1% of allocations.** By the census: `Library`, `Macro`, `RecordType` and ports. They may live in a non-moving "drop space" swept at majors (starlark's `drop`/`non_drop` split, `arena.rs:179-183`), so the nursery never sees them.

### 4.4 Weak-id side tables (continuations, CPS payloads, environments) [I]

The existing `trace_weak_ids`/`sweep_weak` protocol stays and gains two rules.

1. **Generational rule.** An entry is created together with its reference object, and the id→payload mapping never changes. So an entry is young iff its ref is young.
   - A minor collection examines only entries created since the last collection (a young-id list, like OCaml's `ephe_ref` table, `minor_gc.c:455-500`). Recorded entries are promoted; unrecorded ones are dropped.
   - A major collection examines all entries.
   - Pruning old entries in a minor, which is today's logic applied naively, would free live continuations.
2. **Updatable payloads.** Payload tracing must forward young references in place (`&mut` visitor).
   - `VmContinuation` and `CpsContinuation` are immutable once captured, so one tracing pass leaves them old-only.
   - Mutable payloads, such as an off-heap `Environment`, need a dirty bit on store (tree-walker.md §5.5), or they should move into the heap.

### 4.5 Code-unit release without `live_closures` counting (R8) [I]

- Tracing a `VmClosure` (and scanning a frame or continuation) records its `code_id` in a per-collection bitset. This is the weak-id pattern applied to code.
- After a **major** collection, release every unit with no recorded id. Minor collections release nothing, because they do not trace old closures.
- This deletes H2, `gc_freed_closure_code_ids`, `live_closures` and the `RETIRED` bookkeeping. `retire_vm_closure` keeps its eager path by clearing the closure's code id.
- The better long-term shape is Chez's: code objects as non-moving heap objects in a code space, which closures and JIT frames reference directly, with a finalization registration only for the Rust or JIT memory behind them.

### 4.6 Ephemerons across generations, and what the tests force [I]/[V]

**Algorithm.**
- Replace the round-based fixpoint with key-indexed pending resolution:
  - Whippet: every newly marked or copied object looks itself up in a pending table keyed by K (`gc-ephemeron.c` header; mmc `check_pending_ephemerons`).
  - Chez: per-segment triggers (`check_ephemeron` `c/gc.c:2717-2763`, `check_triggers` `:747-766`).
- Measured need: the 16k chain costs 289 ms today against 0.24 ms in the best order.
- With Chez-style triggers the worst case is quadratic only within one segment ("quadratic in the number of objects that fit into a segment", `gc.c:747-752`).

**Generations.**
- SRFI 124 and 254 ephemerons are immutable apart from breaking, and they are allocated after their K and V. So E is never older than K or V, provided:
  - ephemerons are **always allocated in the nursery**, never pretenured or put in a large-object space;
  - promotion is **age-monotone**. Copying with ages and sticky mark bits both satisfy this.
- A minor collection skips old Es, because their K cannot die in a minor. It resolves young Es by K's forwarding state; an old or immediate K counts as live.
- No remembered-set entries for ephemeron edges are needed. Chez needs `check_dirty_ephemeron` (`gc.c:2777-2810`) only because Chez ephemeron pairs are mutable.
- Weak tables built from ephemerons are fine. The table is an ordinary old object written with ordinary barriers, and each new entry is a young E.

**Constraints from tests** [V source, I consequences]:

| Test | Constraint |
|---|---|
| `ephemerons.rs:23,52` (a dead key breaks after one `(gc)`), and `:213,:226` | `(gc)` must be a **full** collection that breaks dead-key pairs of every age. Chez's own manual warns that `collect` may not suffice once an object has migrated (`csug/smgmt.stex:740-742`). Immediate keys never break, and broken stays a distinct state, not `#f`/`#f`. |
| Larceny `ephemeron.sld` | Keys and pairs are allocated at load time, then 10 × `iota 10,000,000`; there is no explicit `gc`. The automatic policy must run a major collection within about 100M pair allocations, after the keys have been promoted. Today: 4.0 s and 830 MB on the VM. |
| `ephemerons.rs:94,109,127,162` (#423: tail-call windows, argument copies, continuation snapshots, delimited snapshots) | **Precise liveness** in registers, frames and captured continuations. Conservative scanning of VM or JIT frames (Whippet/Guile style) would retain dead keys and fail these tests. JIT stack maps must list only values live across each safepoint. Cranelift's user stack maps do this by construction (cranelift-gc.md). |
| `:67` (an earlier operand stays live) and `:78` (`reference-barrier`) | Pending operands and the `reference-barrier` argument must stay in the root set until their use. In JIT code, `reference-barrier` (and SRFI 254 `ephemeron-ref`'s key) must be an opaque use that cannot be dead-code-eliminated, such as a runtime call or a use the stack map must include. It must not be inlined to nothing. |
| `:195` (an ephemeron holding a continuation) | The ephemeron, weak-id and (future) guardian steps form **one** fixpoint (commit `1d18c49`). |

### 4.7 Guardians and transport cells (SRFI 254), if added [I]

**Guardians.**
- Each registration `{obj, rep, queue}` lives on the list of the generation it was registered in (Chez `gc.c:1330-1565`).
- After the trace and ephemeron resolution reach a fixpoint, each registration whose `obj` is unreached is handled by its queue:
  - **queue reached:** move `rep` to the queue and trace it. When `rep` is `obj`, this resurrects `obj`. Then iterate the fixpoint again.
  - **queue unreached:** drop the registration.
- **Only then** break the remaining pending ephemerons. Chez documents that a guarded object stays in weak and ephemeron cars until it is retrieved and dropped (`csug/smgmt.stex:799-845`), and Whippet keeps ephemeron associations while the finalizable object is live (`gc-finalizer.h`).
- Use unordered semantics, as Chez does by default. SRFI 254 leaves ordering open.

**Transport cells.** Each registration is checked after a collection:
- key forwarded (it moved) → enqueue the cell;
- key dead → break it.

Only the nursery, and any evacuation in the old space, moves objects, so cost is proportional to young registrations.

### 4.8 Address-keyed side tables become collector-processed weak-key tables (R9) [I]

- `Heap.syntax_sources` and `SourceMap.locations` are keyed by `raw_bits()`. A copying nursery reuses addresses after *every* minor collection, so freed-bit reporting cannot work.
- Replace them with a heap-registered `WeakKeyTable<V>`. After each collection:
  - entries with young keys are rekeyed if forwarded and removed if dead;
  - old-key entries are processed only at majors, via separate young and old lists.

  This is Chez's moved-entry rehash (`tlcs_to_rehash`, `gc.c:1734-1773`) without the tlc cells.
- `SourceMap` must therefore register with the heap. That deletes H1, the 65,536 cap and the `Overflowed` misfeature.
- The same table serves the SRFI 125 internals and a later weak symbol table (chez.md §10).

### 4.9 Collection epilogue order [I]

1. Trace (copy or mark).
2. Fixpoint over: newly recorded weak ids, then key-indexed ephemeron resolution, then the guardian pass when nothing else progresses.
3. Break pending ephemerons.
4. Transport cells.
5. Weak-key tables: rekey or remove.
6. Weak-id tables: prune and promote.
7. Finalization registry: run the Rust finalizers for dead ports and needs-drop entries, and promote surviving registrations.
8. Release code units (majors only).
9. Resume. Guardian queues become visible to Scheme.

---

## 5. Deliverable table

| Behaviour | Today's mechanism | Required semantics (evidence) | Proposed mechanism | Tests that pin it (existing / to add) |
|---|---|---|---|---|
| Unflushed output of an unreachable file port, mid-program | `Drop` of the `BufWriter` when sweep tombstones the last `Port` wrapper (`gc.rs:828-852`, `vfs.rs:119`) | Flushed at the collection that reclaims it, before Scheme resumes. Errors ignored. **R2**: chibi, Gauche and Patina all `""`→`"hello"` (E1, E1b). | Port table plus per-generation finalization registry; Rust-only finalizer in the epilogue (§4.2) | None. **Add** E1 as a cargo test on both backends, *outside* the GC-differential lane. |
| Output left in open or garbage ports at exit | `OUTPUT_FILES` weak list (`port.rs:173-235`) plus `end_process` (`exit_status.rs:62`) | Kept however the program ends, including `emergency-exit`. **R1**: oracle table in `unclosed_output_ports.rs`; E1c–e. | `end_process` flushes all live `PortTable` entries | `unclosed_output_ports.rs:64,144,164`. **Add** the garbage-port variant (E1c–e). |
| Descriptor release, and opening under pressure | Drop at sweep; the trigger counts slots only | Garbage ports must not exhaust descriptors. **R3**: chibi and Gauche complete 100k opens at `ulimit -n 1024`; Patina fails at 1021 (E2). | Close in the finalizer; file-port pressure trigger; external-bytes term; `EMFILE` collect-and-retry through a safe point (§4.2) | None. **Add** E2 with `setrlimit(RLIMIT_NOFILE, 256)` in a subprocess test. |
| `MemoryFs` writer commit | `MemoryWriter::drop` (`vfs.rs:294`) | Unclosed in-memory output is committed when the port dies | The port finalizer calls `finalize()` | `vfs.rs:623` `test_memory_fs_drop_writes` (Rust-level). **Add** a heap-level variant. |
| Port identity | New wrapper on every `current-*-port` call (`ports.rs:463`) | `eq?` holds. **R6**: chibi and Gauche `(#t #t #t #t)`; Patina `(#f #f #t #f)` (E4). | Parameters hold heap port objects; immortal stdio objects | None. **Add** E4. File an issue first (project rule). |
| Embedder teardown | None: the heap leaks (`strong_count = 36`) | Dropping the interpreter flushes and closes. **R5**: E5 loses `"hello"`. | Heap drop finalizes the `PortTable`; break the env↔heap `Rc` graph | None. **Add** E5 (crate `teardown/`). |
| Closure↔environment cycles and payload memory | Tombstone `Drop` | Unreachable closures and environments reclaimed. **R7** (GC_DESIGN §8). 47% of allocations carry Drop today. | Drop-free object model; heap or id environments; non-moving drop space for under 1% of kinds (§4.3) | `gc.rs:1429` (mechanism). **Add** an RSS/live-count plateau test for closure churn on both backends. |
| Tree-walker CPS payloads | Drop of `Rc<Procedure>` / `Rc<CpsContinuation>` | Reclaimed when unreachable. **R7** (tree-walker.md §1.9). | Ids into an evaluator weak-id table with the generational rule (§4.4) | `gc_tree_walker.rs` reclamation tests; GC differential lane. |
| VM code-unit release (#338, #353) | Sweep reports dead closures' code ids, then `live_closures` counting (`vm_state.rs:542-575`) | Never free runnable code; release after it becomes unreachable. **R8**. | Record code ids during tracing; release after majors; or code objects in a non-moving code space (§4.5) | `finished_forms_release_code.rs:72-217` (7 tests); `patina-vm/tests/integration.rs:376,388,409`. |
| SourceMap pruning | H1 freed raw bits, cap 65,536, then overflow clears the map | Never misattribute; may lose. **R9**. | Heap-registered `WeakKeyTable` with rekeying (§4.8) | `interpreter_api.rs:270`; `gc.rs:1611,1647` (mechanism, to be replaced); `source_map.rs:423`. |
| `syntax_sources` pruning | `retain(marked)` plus shrink in `sweep` (`gc.rs:891-900`) | As R9 | Same `WeakKeyTable` | `heap/source.rs:212`. |
| Weak continuation stores | `trace_weak_ids`/`sweep_weak` fixpoint, by mark | Payload live iff its ref is live (#19, ctak's 4 GB) | Same protocol plus the generational rule plus updatable payloads (§4.4) | `weak_continuation_tests.rs:136,190,209`; `gc_vm.rs:79`; `ephemerons.rs:195`. |
| Ephemerons (SRFI 124) | Round-based fixpoint, break at end of mark (`gc.rs:1050-1094`) | Break iff key unreachable; `(gc)` breaks every age; precise liveness; `reference-barrier` holds; one fixpoint with weak ids. Measured O(n²): 16k chain 289 ms. | Key-indexed pending resolution; E never older than K or V; nursery-only allocation; `(gc)` = major (§4.6) | `ephemerons.rs` (13 tests; lines 23–226); Larceny `ephemeron` (6/6). **Add** a chain-scaling bound test (time or round count). |
| SRFI 125 weakness arguments | Silently dropped (`125.sld:64-74`) | "Should create" weak or ephemeral tables; ephemeral may stand in for weak (SRFI 125 text) | Ephemeron-bucket tables over a move-stable identity hash | None. **Add** a `weak-keys` table whose dead key disappears after `(gc)`. |
| `immutable-tables` leak | Strong `eq?` table for the process lifetime (`125.sld:41-49,137-145`) | No leak | Internal ephemeral-keys table | None. **Add** a reclamation test. |
| Guardians / wills | None | Not required by any consumer. SRFI 254 final 2026-06-30 (eligible). | Per-generation registrations, resurrect before breaking, queue drained by Scheme (§4.7) | **Add** with SRFI 254: Chez manual examples `smgmt.stex:780-845`. |
| Address hashing under moving | `hash-by-identity` = arena index stored in SRFI 69 buckets | Lookups survive a move | Header hash or GC rehash of moved entries; SRFI 254 transport cells as the Scheme-level contract | Hashing gap (out of scope here). |

---

## 6. Constraints and lessons for the redesign

1. **Never rely on visiting the dead.** Every effect must hang off a registration list sized to its population, as OCaml, Wasmtime, rune and Chez do.
2. **Make nursery objects Drop-free before building the nursery.** With 47% of allocations Drop-carrying, a drop list is a sweep in disguise. Identifiers and `VmClosure` are the blockers.
3. **One epilogue owns all weakness, in a fixed order (§4.9).** Ephemerons, weak ids and guardians share one fixpoint (`1d18c49`).
4. **The never-older invariant covers ephemerons, weak-id entries, registrations and transport cells.** Ephemerons must not be pretenured.
5. **`(gc)` is always a full collection.** The ephemeron tests require it.
6. **Precise root liveness is a semantic requirement, not an optimization.** The #423 tests forbid conservative frame scanning. The JIT must emit precise stack maps and keep `reference-barrier` uses.
7. **Inline JIT allocation only for kinds with no finalization or weak registration.** Ports, ephemerons, guardians and transport cells allocate through the runtime slow path, so the registration happens there.
8. **Finalizers are Rust-only, allocation-free and non-reentrant.** Scheme-visible finalization is guardian queues drained by Scheme. Do not reintroduce Java-style finalizers (JEP 421).
9. **The trigger must see resources the heap does not.** Count port buffers and descriptors, as Chez phantom bytes, OCaml custom `mem` and Gauche's table-full GC do.
10. **The GC-differential lane assumes GC timing is unobservable.** Port finalization is observable, so tests for it must live outside that lane or the lane must say so.

## 7. Open questions

1. **Move-stable identity hashing** (needed by weak `eq?` tables): a header hash word (and what pairs do without a header), Chez-style rehash of moved entries, or SRFI 254 transport cells under SRFI 69?
2. **Per-function JIT code release:** can `cranelift-jit` free individual functions, or do code units need their own `JITModule` or arena? This determines whether §4.5's release hook can free machine code.
3. **Where port finalizers run:** in the pause, or queued to the next safe point? A flush to a pipe can block, which favours the queue. Pause budgets favour the queue too.
4. **The `EMFILE` retry vehicle:** a Scheme wrapper or a resumable `Step`, and whether `GcDeferGuard` scopes (nested runs, library bodies) may suppress the forced collection.
5. **Ordering:** should guardians land with the redesign, or wait for demand? Should Patina expose `(srfi 254)` under R6RS-style names (`(srfi :254 …)`) only, or also an R7RS `(srfi 254)` spelling?
6. **The `lib/srfi/125.sld:67` comment** claims SRFI 125 requires an error for unsupported weakness. The current SRFI text does not. Correct it when weakness support lands.
7. **E4 and E5 are present-day defects** independent of the redesign. They need issues filed before fixing (project rule).

## Sources

**Repo** (paths above, at `28a94f8`).

**Oracles:**
- chibi: `sexp.c:196-232`, `eval.c:1300-1356`, `include/chibi/sexp.h:69`, `lib/chibi/weak.sld`.
- Gauche: `src/port.c:250-316,1280-1400`, `src/core.c:186-187,359-436`, `src/vm.c:3935`, `lib/scheme/ephemeron.scm`.

**Prior art:**
- Chez: `c/gc.c:747-766,1330-1565,2680-2810`; `csug/smgmt.stex:459-480,668-860`.
- OCaml: `runtime/minor_gc.c:455-500,745-806`; `runtime/custom.c:62-117`.
- Gambit: `lib/mem.c:4997-5110`.
- Whippet: `src/gc-ephemeron.c`, `src/gc-finalizer.c`, `api/gc-finalizer.h`, `doc/manual.md` §Finalizers.
- Wasmtime: `runtime/vm/gc/enabled/copying.rs:151-170,766-791,1111-1133`.
- rune: `context.rs:185-208`.
- starlark: `heap/arena.rs:179-183`.

**SRFIs and standards:**
- https://srfi.schemers.org/srfi-124/ (final 2015-11-06)
- https://srfi.schemers.org/srfi-125/srfi-125.html
- https://srfi.schemers.org/srfi-246/ (withdrawn 2024-09-11)
- https://srfi.schemers.org/srfi-254/srfi-254.html (final 2026-06-30)
- https://openjdk.org/jeps/421
- https://pkg.go.dev/runtime#AddCleanup

**Wingo:**
- https://wingolog.org/archives/2025/01/09/ephemerons-vs-generations-in-whippet
- https://wingolog.org/archives/2024/07/22/finalizers-guardians-phantom-references-et-cetera
