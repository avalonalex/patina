# Other future features against the GC contract

Source: branch `gc-prd`, HEAD `f82e8e8`. The PRD is dated 2026-10-01 against `28a94f8`, and no Rust source changed
between the two commits.

**Citation forms.**
- **§n:L** is section n of `PRD/GC_PRD.md`, line L.
- Other documents and source files are cited as path:line.

**Clause ids.**
- Today's contract (`today.md`): **P1–P8** and **O1–O17**.
- The PRD's contract (`prd-contract.md`): **C1–C19**, **V1–V14**, **J1–J16**, **R1–R15**, **T1–T7**, **E1–E11**,
  **K-1–K-10** and **N1–N16**.
- The PRD's kill criteria (§20): **K1–K19**.
- **[I]** marks my own judgement.

**Verdicts.**
- **Yes:** the PRD names the mechanism and gives it a stage.
- **Partly:** the mechanism exists, but a piece the feature needs is left unspecified.
- **No:** the PRD's text conflicts with what the feature needs, or says nothing about it.

## 0. Summary

| Feature | State of the feature | Today's clauses it would break | PRD clauses it lands on | Verdict | Main gap |
|---|---|---|---|---|---|
| FFI Layer 1 (Rust plugins, foreign objects) | Proposed 2026-03-20; nothing built | O8, O13, O14, P6 | R4, E4, E7, K-4, K-5, C11 | Yes | `FFI_DESIGN.md`'s API (`HeapIndex`, `SharedHeap`, higher-order handlers, `Box<dyn Any>` payloads) is the API the PRD deletes. §19 lists the doc rewrite at stages 2–3 |
| FFI Layer 2 (C calls, buffers, foreign finalizers, callbacks) | Phases C–D, not started | O2, O8, O15, P1, P6 | C8, C11, R8, R14, J3, K-2, K-3, K-4, the LOS | Partly | A synchronous C→Scheme callback has no collecting home (it gets `NoGcScope` or an unspecified driver entry). There is no `Cx`-level API for a pinned payload pointer. A foreign finalizer may block. Raw C words conflict with invariant W |
| syntax-case | Planned for after stage 5 (decision 17) | O2, O9, O10, O15 | R5, R9, K16, point C, stage 4c | Partly | `ExpansionContext` is named but not specified. Transformer calls on the tree-walker stay deferred for good (T2). The scope-set table must be weak, not per-form. `SYNTAX_CASE_DESIGN.md` is missing from §19's list of documents to update |
| Debugger and hook system | Designs only (the tree-walker hook doc and the visual debugger) | O1 (closed root array), O2, O3, O7 | R5, R8 (`RootSet::register`), V6, V10, J13, C12 (`CodeStore::escape`), §12 `DEBUGGER` bit | Yes, with two tensions | A paused evaluation never collects and never trims (K16 counts it; nothing bounds it). The hook doc's JIT notes (stack maps, watchpoints still live under the JIT) contradict §11.2 |
| Guardians (SRFI 254) and weak hash tables | SRFI 124 ephemerons shipped; SRFI 125's weakness argument is silently dropped; no guardians | P4, P7 | C10, C13 steps 2/4/5, decision 11, `WeakRegistry` | Partly | Guardians and transport cells are a reserved slot with no design. Symbol-keyed weak tables never shrink |
| Boot image | One optional row in §19 | P4, O6 | M1, the immortal space, C12 | No (named only) | The immortal space is "never scanned" and "references nothing mortal", which a stdlib image with mutable cells breaks. Relocation and serialization of side tables are unspecified |
| AOT compilation | One paragraph in the hook doc (§10.2) | — | J1, J5, J6, J7, J8, `GcAttrs` | No | `GcAttrs` is a per-heap runtime policy that gets baked into code. AOT needs an attrs/ABI fingerprint, and must keep bytecode so it can deoptimize |
| Delimited continuations | Done on both backends | O4, O5 | V10, V11, §13, K15 | Yes | — |
| Effect handlers and one-shot fibers | Not designed | O4 | §13, C′/K15, §18.1, decision 23 | Partly | Design A costs O(depth) per perform and per resume. A fiber is a stack reachable only from the heap, a root class the scheduler rule does not cover |
| Notebook and long REPL sessions | Notebook design 2025; the REPL exists | O6, O12, O13, P3, P4 | §17 (SS1–SS5, M1–M5, the REPL rows), E1, E5, E6, SD7, decision 20 | Yes | Result history held by the host must be `Owned` and bounded. Scheme callbacks from Rust data libraries must go through `Step::Call` |
| General tail calls | The doc is stale; proper tail calls are done on both backends | O5 | V3, V5, V8, V10, J2, §11.6 tail-shape deoptimization | Yes | — |

## 1. FFI

**What it asks of the collector.** `PRD/FFI_DESIGN.md` is marked Proposed, dated 2026-03-20, and its GC section still
says "When GC is added" (:435-442).
- **Layer 1.**
  - Plugins re-export `TaggedValue`, `SharedHeap` and `HeapIndex` (:72).
  - Plugins register a `TaggedHandler` or a `HOTaggedHandler` (:89-102).
  - Opaque Rust objects become `HeapObjectData::Foreign { type_name, data: Box<dyn Any>, finalizer }` (:54-58;
    decision D1 :375-383).
- **Layer 2.**
  - `ffi:string` is copied into a temporary `CString` (:261).
  - A bytevector passed as `void*` is "pinned for call duration" (:262).
  - A `void*` result is wrapped as `ForeignPointer(*mut ())` (:272; decision D5 :415-421).
  - Phase D adds `ffi-callback`, which turns a Scheme closure into a C function pointer, and pinning for buffers
    (:366-367).
  - Open question Q2 asks about `eq?` on foreign objects (:455); Q3 asks about threads (:457).

**Today.** None of this exists. Built on today's contract, it would break:
- O8: higher-order handlers call back into Scheme from Rust (`crates/patina-primitives/src/registry.rs:16-21`);
- P6 and O14: a `Box<dyn Any>` payload is a `Drop` that runs at sweep, inside the pause;
- O13: nothing a host holds is rooted, so a closure stored by C would be freed, as in #605.

Today's non-moving heap (P1) makes "pinned for call duration" free.

**Against the PRD.**

*Layer 1: Yes.*
- `register_primitive(lib, name, PrimSpec { f: Prim, arity, class })` and `register_resumable` replace
  `LibraryBuilder` (§11.5:955, E4).
- "Plugins never see `HeapIndex` or the encoding" (§11.5:958).
- Higher-order handlers go away: "a procedure argument is called only through `Step::Call`" (R4).
- §19 updates `FFI_DESIGN.md` twice:
  - at stage 2: handles, pinning, `Foreign` finalization and global handles for callbacks (§19:1891-1893);
  - at stage 3: plugins through `register_primitive` (§19:1896).

*Foreign objects: Yes.*
- No heap object owns a Rust value. FFI objects are named among the host payloads (§6:430-434).
- They are registered as `FinalKind::Foreign` (§9.6:599, §14:1195).
- They are traced through `HostPayload::trace` and charged through `external_bytes` (§14:1184-1188; M3,
  §17.2:1440).
- `HostPayload` is not `Send` (§14:1183), which matches FFI Q3 for now. N15 says a threaded VM heap needs its own
  `Send + Sync` payload type.

*Foreign finalizers: Partly.*
- F4 says finalization "runs no Scheme and allocates nothing" (§9.6:612).
- Queued entries run "after the pause and before Scheme resumes, in the poll slow path" (§9.6:602-604).
- A foreign finalizer such as `sqlite3_close` or a socket close can block, which stalls Scheme at that poll. The PRD
  says nothing about finalizers that block or fail [I]. The options are:
  - require foreign finalizers to be non-blocking;
  - run blocking ones in a safe region;
  - leave Scheme-visible cleanup to a guardian that Scheme drains (`PRD/study/gc/gaps/finalization-weak-semantics.md`
    §4.7, :326-334).

*`ForeignPointer`: Partly.*
- A raw C address stored in a heap word breaks invariant W (§6:375-380), unless the kind is pointer-free and its
  field is declared raw, as `CodeDesc`'s raw fields are.
- The danger is concrete: an unaligned `char*` with bit 2 set reads as a heap reference both to the verifier
  (§16:1334-1339) and to a minor's re-read of a logged granule (§10).
- K-1 and K-2 cover this if FFI declares the kind through `declare_layouts!` (§6:436-439). The layout table
  (§6:384-417) has a "host handle" kind (§6:408), and a pointer-free kind with raw words already has precedent: bignum's
  raw `u64` limbs (§6:398), strings and bytevectors (§6:394, 396), and `CodeDesc`'s declared-raw w1–w3 (§6:422-425).

*Buffers lent to C: Partly.*
- The pieces exist:
  - the LOS never moves, and "buffers lent to FFI live here" (§7:465);
  - pins are counted per block and return a `PinToken` (§7:484-486; `Collector::pin`, §14:1157);
  - a `PinToken` may hold a value across a collection (§11.3:921-925);
  - the heap advertises `GcAttrs.can_pin` (§14:1253).
- Missing [I]: a `Cx`-level API that yields a payload pointer for as long as the pin lives.
  - §11.3 says no public API returns `&mut [Word]` into the heap (§11.3:917-919).
  - Unsafe heap access is confined to core, `patina-gc`, the VM, the tree-walker and the JIT crate (§11.3:912-917).
  - So `patina-ffi` either needs a `pub unsafe fn` in `patina-core`, for example
    `Cx::pin_bytes(bv) -> (PinToken, *mut u8, usize)`, or has to join the unsafe list.
- When a pin is needed at all [I]:
  - not for a `Leaf` C call that neither blocks nor calls back, under M:1 and before stage 8;
  - yes once the call can reach a collection (a callback, or a safe region with N > 1 carriers);
  - yes for every call once evacuation exists (stage 8).

*Strings.* Decision 5, UTF-32 inline strings (§2:199), makes every `ffi:string` a transcode and a copy, so zero-copy
`char*` is impossible [I]. `FFI_DESIGN.md` copies anyway (:261), so this is a cost, not a conflict.

*Callbacks (Phase D): Partly. This is the largest gap.*
- **Lifetime.**
  - A function pointer that C holds must root its closure until it is explicitly freed.
  - `Owned` and `RootToken` (R8, §11.3:921-925) give the JNI global-reference model the study asked for
    (`PRD/study/gc/understand/primitives-embedding.md:274`).
  - The libffi trampoline must also outlive every copy of the pointer that C keeps, so callback lifetime has to be
    explicit, not driven by the collector [I].
- **Why collection is a problem.**
  - A C library that calls back synchronously is a Rust/C frame calling into the program. AGENTS.md forbids that for
    primitives, because such frames cannot be part of a continuation (#471).
  - `Step::Call` cannot express it, because the C frame is still on the native stack.
  - So the callback re-enters the machine from beneath the FFI primitive. If that primitive holds a `Cx`, the callback
    runs under `NoGcScope` (§11.3:891-893) and never collects.
  - Consequence: a GUI or event loop that lives in C and calls Scheme for ever allocates without bound until K16
    fires (§20:1955).
- **The collecting design** is the safe-region pattern [I]:
  1. Marshal every argument first.
  2. Hold only `PinToken` and `Owned` values across the C call, and release the `Cx`.
  3. Enter the callback as a driver entry, like `Interpreter::call` (E3, §11.5:954).
  4. Put a continuation barrier at the callback boundary, as `across_reentry` does for re-entries today
     (`crates/patina-vm/src/runtime/control.rs:2169`).
- **What the PRD lacks.** It chooses neither path. K16's list of sites (point C, the `apply_proc` fallbacks, nested
  tree-walker trampolines, a paused debugger; §20:1955) should gain "FFI callbacks", or §11.3 should specify the
  callback entry as a driver entry [I].

*JIT: Partly.*
- A C call's helper class comes from its declaration: the `class` in `PrimSpec`, or `Transfer` when none is declared
  (§11.5:955; decision 13, §2:212).
- `ffi-procedure` builds procedures at run time from type lists (:192-215) and has no flag for blocking or
  callbacks. Every C procedure would therefore be `Transfer` unless `FFI_DESIGN.md` adds such a flag.
- Nothing checks a `Leaf` declaration that actually calls back (`prd-contract.md` §11, item 2) [I].

*Threads: Yes, on paper.*
- Safe regions wrap "blocking I/O and the future FFI" and are no-ops under M:1 (§12:1044-1048, R14). Stage 9 does
  the restructuring.
- Under N carriers, a blocking C call made outside a safe region stalls stop-the-world (rule 5, §18.2:1665).

*Identity (FFI Q2).*
- Whether two wrappers of one C resource are `eq?` is a library-level decision; K-10 and F6 give the canonical-object
  pattern.
- `eq?` stays address identity, and the BFG hash survives moves (§9.8).

**What would close it.**
- In the stage 2–3 rewrite of `FFI_DESIGN.md`:
  - foreign objects as host payloads, with non-blocking Rust finalizers;
  - `ForeignPointer` as a pointer-free kind with a raw field;
  - a pinned-payload API in `patina-core`;
  - callbacks as explicitly freed, `Owned`-rooted handles, entered as driver entries behind a continuation barrier;
  - a blocking or callback flag that maps to `Transfer`.
- In the PRD: add FFI callbacks to K16's sites or to §11.3's driver entries, and state the rule for foreign cleanup
  that blocks.

## 2. syntax-case

**What it asks.** Procedural transformers run Scheme during expansion.
- The design doc's sketch calls `apply_transformer(transformer, stx, env)` from inside `desugar_application`
  (`PRD/macro/SYNTAX_CASE_DESIGN.md:996-1010`).
- It represents syntax as `Syntax(Box<SyntaxObject>)`, with `datum: Value` and `Rc<str>` locations (:450-484). That
  is the deleted `Value` enum, and a `Drop` payload.
- Its "Resolve Once, Before the Backends" decision (:89-135) still needs a story for code created at run time
  (:119-122).

**Today.** syntax-case breaks O9 outright.
- The expander is GC-atomic only because `syntax-rules` runs no Scheme. The code says "No GC runs while desugaring"
  (`crates/patina-frontend/src/desugarer/mod.rs:370-372`).
- Its raw-bits memos (`quoted`, `open_forms`; O10) are sound only inside that atomic window.
- Imports are the one exception, deferred by a `GcDeferGuard` (O2; `desugar_with_imports`, `desugarer/mod.rs:1841`).
  Applying the same deferral to transformers would cover whole forms.
- The study recorded the consequences in `PRD/study/gc/gaps/library-load-memory.md:358-382`.

**Against the PRD: Partly.**

What is already planned:
- **Decision 17** (§2:217): after stage 5, "an `ExpansionContext` root provider (a literal pool and epoch-checked
  memos) that retires the last `NoGcScope` at point C; sooner if K16 fires".
- **Its prerequisites are scheduled:**
  - open `RootSet::register` at stage 2 (§14:1133);
  - the `CoreExpr` literal pool at stage 3 (§11.3:908-910, §17.2:1450);
  - the raw-bits stores deleted at stage 1;
  - identifiers as ids, with inline provenance, at 4c (§19:1866).
- **The `'gc` brand forces the design.** Its trybuild tests make "a value used after a may-collect call" a compile
  error (§11.3:906-907). The expander therefore has to keep its syntax in the `ExpansionContext` across a transformer
  call, not in Rust locals [I].
- **Macro bodies become host payloads** (§6:430-434), so a transformer closure is traced through
  `HostPayload::trace`. That also closes today's untraced `foreign_expansions` gap
  (`crates/patina-core/src/compiled_macro.rs:540`; O3).
- **Resolve-once fits variants R and C.** Link tables over binding records are the split between compile-time tables
  and runtime heap cells that the syntax-case doc asks for (`PRD/study/gc/gaps/global-binding-cells.md:238`;
  §11.6:969-976).
- **Introduced definitions are handled.** From 5c, a macro-introduced definition is reclaimed only when no live link
  table references it **and** no live identifier carries its expansion scope (§11.6:974). That is the right rule once
  user code can store identifiers anywhere.

**Gaps [I].**
- **`ExpansionContext` is only a name.** No section specifies:
  - its LIFO discipline across nested phase-1 library loads ("load inside expand inside load nests arbitrarily",
    `library-load-memory.md:381`);
  - the epoch that the memos check;
  - the continuation barrier at each transformer call (`library-load-memory.md:380`).

  §11.3's "Loading and nested loops" (§11.3:927-934) covers points A–D only.
- **The tree-walker keeps deferral.** Nested trampolines keep `NoGcScope` for good (T2, §11.4:938-941).
  - A transformer evaluated on the tree-walker runs in a nested trampoline, so decision 17 retires point C on the VM
    only.
  - Expansion heavy in transformers (a `match` written in syntax-case) stays deferred on the tree-walker unless the
    optional evaluator-owned `StepRoots` row lands (§19:1882).
- **The scope-set table must be weak.** Stage 4c allows "a weak (or per-form) scope-set table" (§19:1866).
  - Per-form is unsound once `datum->syntax` and `syntax` hand identifiers to user data that outlives the form.
  - syntax-case therefore requires the weak variant (`library-load-memory.md:379`).
- **A design doc is missing from the update list.**
  - §19 updates `docs/MACRO_SYSTEM.md` at 4c (§19:1905).
  - It does not update `PRD/macro/SYNTAX_CASE_DESIGN.md`, whose syntax-object and integration sketches contradict §6
    and AGENTS.md's callback rule.

## 3. Debugger and hook system

**What it asks.** `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` and `PRD/future/VISUAL_DEBUGGER_DESIGN.md` want:
- a `DebugHook: GcRoots` that retains values: watch targets, recorded macro datums (:288-292);
- hooks that block while paused and evaluate Scheme re-entrantly: `p <expr>`, conditional breakpoints (:297-302,
  :360-370);
- "Hooks attached ⇒ don't tier up" (:620-629);
- per-thread stepping, with all-stop at safe points (:562-568);
- continuation ids for the shadow stack (visual debugger :169-172);
- a capped retention table for macro datums (visual debugger :347-348).

**Today.**
- O1 and O3: root sets are closed literal arrays (`crates/patina-tree-walker/src/eval/cps_eval/mod.rs:129-130`; VM
  `crates/patina-vm/src/runtime/vm_state.rs:1305-1308`). The hook doc therefore has to forward roots through
  `Evaluator` (:351-358).
- O2: a paused hook's evaluation runs under the outer trampoline's guard and never collects (:360-370; visual
  debugger :134-139).
- The VM's `StepTracer` already roots its snapshots (`crates/patina-vm/src/tracer.rs:283-288`).

**Against the PRD: Yes, with tensions.**

*Retained values.*
- `DebugHook: GcRoots` becomes `DebugHook: RootProvider`, registered through `RootSet::register` (§19:1893-1894,
  §14:1131-1133).
- Tree-walker holders report through `pinned` (T3).
- Once the hook moves to `patina-runtime` for the VM (hook doc :598-607), retained VM values must be updatable
  `slot`s or `Owned` handles before stage 8 [I].
- The macro pane's cap satisfies M2 (§17.2:1439). Retained post-expansion identifiers do keep their expansion scopes,
  and so keep macro-introduced definitions alive past 5c (§11.6:974); the cap bounds that [I].

*Paused evaluation.*
- It "runs under `NoGcScope` and its allocation is counted by K16" (§11.2:883; §19:1897-1899).
- The PRD makes the window visible. It bounds it only through K16's list of remedies (§20:1955) and the pre-entry
  major (§17.3:1497).
- A client that evaluates in a loop while paused still grows the heap [I].
- No idle trim runs at a debugger pause, because "where collection is deferred, no trim runs" (§17.1:1427) [I].

*Tier policy.*
- Attaching any `StepTracer`, breakpoint, watchpoint or `DebugHook` pins the interpreter tier (§11.2:879-883; J13).
- That is stricter than hook doc §10.1, which keeps watchpoints and break-on-exception live under the JIT
  (:633-637).
- The same section says "a Cranelift tier needs GC safepoints and stackmaps regardless" (:638-641), but decision 8
  rules Cranelift stack maps out (§11.2:851-857).
- So the hook doc's JIT notes are stale against the PRD. §19's stage-3 update covers its §10.1 (§19:1897-1899).

*Reading registers.*
- Raw register reads happen only at suspension points (invariant 4, §11.1:832; V6). The datum writer renders
  `DEAD_SLOT` as `#<dead>`.
- With precise maps, a locals pane would show retired registers as dead. That is a user-visible effect the visual
  debugger does not anticipate; a debug-mode map that keeps locals live would avoid it [I].

*Writing into frames.*
- Writes into a suspended frame (setting a local, returning a value) go through `return_into(frame)`
  (§11.1:839-843; V10). Its list includes "debugger writes".

*Code lifetime.*
- A debugger, profiler or tracer obtains code references only through `CodeStore::escape` (§9.7:628-633), so eager
  release never frees code that a breakpoint names.
- Breakpoints keyed by `(source, line, column)` (visual debugger :115-121) need no code references at all.

*Identity.*
- From stage 8, shadow-stack keys must be ids (visual debugger item D0), never addresses.
- Tree-walker continuations become host-payload ids at 4f (T4). A `HostId` reused after finalization would alias, so
  the debugger should keep its own monotone ids [I].

*Threads.*
- `DEBUGGER` is an event bit of the poll word (§12:993-995). That is the "one flag, two clients" the hook doc asks
  for (:565-567).
- Under N carriers, a hook that blocks while holding borrowed event payloads cannot enter a safe region, so it stalls
  every other carrier's collection [I]. Hook doc §9.1 already predicts the client/server split.

*Observability and drift.*
- `PATINA_GC_TRACE` emits JSON phase events for the hook system (§15:1320).
- Stage 1 turns `prune_freed_locations` into a no-op (§19:1888-1890). The visual debugger's runner calls it on every
  form (visual debugger :108).

## 4. Guardians (SRFI 254) and weak hash tables

**What it asks.**
- SRFI 254 (final 2026-06-30) adds guardians and transport-cell guardians.
- SRFI 125 asks for weak and ephemeral tables.
- Today Patina silently drops SRFI 125's weakness argument (`lib/srfi/125.sld:66-73`) and has no guardians (P7;
  `PRD/study/gc/gaps/finalization-weak-semantics.md:211`).

**Today.**
- P7: an ephemeron is queued when marking reaches it, and resolved in one round-based fixpoint shared with the weak
  continuation ids (`crates/patina-core/src/heap/gc.rs:711-717`, `:1050-1088`). It is O(n²) (#609).
- P4: symbols are immortal (`gc.rs:449-464`).

**Against the PRD: Partly.** Decision 11 (§2:210): "SRFI 124 and 254 ephemerons now; SRFI 125 weak tables over
ephemerons; guardians and transport cells after stage 7".

*Weak tables: Yes.*
- They are built in Scheme over ephemerons; Patina's tables are Scheme (§3:255).
- They rest on:
  - key-indexed linear resolution (§9.5:591);
  - the rule that an ephemeron is never older than its key or value (§9.5:592);
  - an identity hash that survives moves (BFG, §9.8:641-659), so SRFI 69's stored hashes stay valid.
- `reference-barrier` stays an opaque `Leaf`/`NoAlloc` use in JIT code (§9.5:594).
- Dead entries go only when Scheme touches the table. Until then the broken ephemerons (32 B each, §6:403) stay in
  the buckets [I].

*Symbol keys.*
- The rule: "an ephemeron keeps a symbol key alive, so symbol-keyed ephemerons never break and symbol-keyed weak
  tables answer as today … although the symbol table is weak (SD2)" (§9.5:590).
- So a weak table keyed by run-time symbols (say, `string->symbol` of request ids) never shrinks.
- That is a class-A leak under SS1 and SS5 that §17 does not classify. §17.1:1430-1432 lists only growing name sets,
  threads blocked for ever, partly occupied blocks and `NoGcScope` windows as outside the contract [I].
- Chez breaks such pairs. chibi and Gauche keep symbols strong, which is what the rule preserves.

*Flonum keys.* From 5f, flonum-keyed ephemerons never break (§1:157; §5:345-347), with `DIVERGENCES.tsv` rows.

*Guardians and transport cells: Partly. The slots are reserved:*
- the fixpoint runs host payloads → ephemerons → "(later) guardians, which resurrect before any breaking"
  (§9.9:666-667);
- step 4 is "(Later) SRFI 254 transport cells" (§9.9:669);
- `WeakRegistry` carries `// stage 7 or later: pub guardians: GuardianTable` (§14:1181);
- the collector obligation already says "guardians resurrecting before breaking" (§14:1207).

*Unspecified [I]:*
- how registrations are represented, and their young and old lists. Chez keeps one list per generation
  (`finalization-weak-semantics.md` §4.7, :326-334).
- the never-older rule for registrations. The gap study extends it beyond ephemerons (:396); §9.5:592 states it for
  ephemerons only.
- how the collector appends `rep` to a Scheme-visible queue inside the pause without allocating. Chez preallocates
  the queue cell at registration.
- how that store, made by the collector into a possibly old queue, is remembered under generations. §10's barrier
  covers mutator stores only.
- the conformance tests for resurrection.

*Other points.*
- Transport cells fire only when something moves (stage 8, or a K4 nursery), so before then they are trivially empty.
- A guardian registration must be a slow-path allocation (`finalization-weak-semantics.md:399`). That matches the
  rule that finalization entries are "created only by slow-path constructors" (§9.6:599).

## 5. Boot image and AOT

### Boot image

**What the PRD says.** The image is named, not designed:
- the immortal space holds "a future boot image" (§7:467; M1, §17.2:1438);
- "boot-image static space" is an optional row (§19:1882).

The study's motivation is to stop re-marking the standard library and code constants, which were 57% of root tracing
before #353 (`PRD/study/gc/DIGEST.md:393`).

**Verdict: No [I].**

*Mutable state in an unscanned space.*
- The immortal space is "never swept and never scanned: whatever it references is immortal too" (§7:467).
- "Immortal objects reference nothing mortal and need neither" arming nor the remember-whole list (§10:753-754).
- A standard-library image holds mutable state:
  - library global cells that a program's `set!` reaches (#406; AGENTS.md, "An import installs a binding");
  - parameter objects and promise boxes;
  - SRFI 69 and 125 tables;
  - SRFI 128's comparator registry (§18.3:1695).
- A mortal value stored into an immortal, unscanned cell is a missed root.
- The image therefore has to do one of two things:
  - allocate its mutable objects old in the NMS (`alloc_old` plus the remember-whole list, §10:750-755) and keep only
    immutable objects immortal; or
  - make the immortal space track dirty granules, whose contents are roots at every collection ("dirty slots are
    major roots", `PRD/study/gc/design/proposal-bounded.md:192`).
- Every non-generational heap reports `BarrierKind::None` (§10:759), so there is no log to find dirty granules.
  Such a heap would scan the image's mutable part at every major: a constant term, inside §9.10's c [I].

*Relocation.*
- Values are raw tagged addresses (§5:309-319), and each heap is a fresh reservation (§7:443-450), so an image must
  be relocated when it is loaded.
- The `trace` that `declare_layouts!` generates, with writable slots (§14:1111-1118), is exactly the walker needed.
- Identity hashes are relative to the heap base (§9.8:654) and symbol hashes come from the name (§6:395), so both
  survive a relocation that keeps offsets.

*Side tables.*
- A descriptor's raw `w1`–`w3` fields point at Rust-owned `CodeBody` and resume tables (§6:422-428).
- Macro bodies, libraries and namespaces are host payloads (§6:430-434).
- `HostPayload` has only `trace` and `external_bytes` (§14:1184-1188). An image needs a serialize and restore hook
  for each payload kind, and a way to serialize the `CodeStore`.

*Code lifetime.* Boot units are never released, which M1 permits. Eager release (§9.7:626-633) applies only to units
created at run time.

### AOT

**What the repo says.** Hook doc §10.2 (:647-655) is the only mention.

**Verdict: No [I].**

*`GcAttrs` cannot follow a policy change.*
- `GcAttrs` is per heap and "the JIT reads it" (§14:1149); emitters switch on it at compile time (§14:1260), and a
  non-generational heap's `BarrierKind::None` emits nothing (§10:758-760).
- The per-heap policy `{generational, evacuation}` is a runtime field that measured gates flip (§9:530-532).
- JIT code can be recompiled when the policy changes; AOT code cannot. A body compiled for `BarrierKind::None` is
  unsound on a generational heap.
- AOT therefore needs one of two things:
  - code compiled for the worst-case attributes (the barrier always emitted, its armed branch never taken on
    whole-heap heaps); or
  - an attributes and ABI fingerprint checked when the code is loaded.
- §14 asserts the offsets in CI (§14:1217-1218) but specifies no check at load time.

*Embedded addresses.*
- Only §11.2's four cases may embed an address (§11.2:866-872).
- AOT code knows no heap address when it is compiled, so everything goes through descriptor constants (one load), or
  through relocation at load time inside `install_code`. Patching anywhere else is excluded (§14:1288-1293).

*An interpreter tier to fall back to.*
- `(code, pc)` is where a frame resumes. `ret` is only a cache, re-derived "from `desc.resume[pc]` (or the
  interpreter trampoline)" (§13:1077-1082).
- These all need an interpreter tier to fall back to:
  - invalidation by a `WATCHED` store;
  - re-pointing a binding record under variant R;
  - a program's `set!` of a standard-library primitive;
  - attaching a hook.
- An AOT image must therefore keep bytecode and safepoint maps beside its native code.

*Continuations stay portable.* Frames hold only tagged values and `(code, pc)`, so a continuation captured in AOT
code resumes in any tier (J1, J5).

## 6. Delimited continuations and effect handlers

### Delimited control

**Status.** Done on both backends (`PRD/phase1/DELIMITED_CONTINUATIONS_DESIGN.md:5-7`).

**Today.** It rests on O4 (the weak side tables, `crates/patina-vm/src/runtime/vm_state/gc_roots.rs:8-24`) and O5
(retiring dead registers in snapshots).

**Against the PRD: Yes.** Design A keeps the semantics:
- "delimited capture covers `[prompt_offset .. top]` and relocates by byte-offset arithmetic" (§13:1084-1086);
- prompts, handlers and winds record byte offsets from the stack base (§11.1:820-821);
- "abort landing, delimited append" go through `return_into` (§11.1:839-843);
- a continuation object's meta words carry the re-entry boundary ids (§13:1054-1056);
- the matrix and `escape_from_primitive.rs` gate stage 4e on both backends (§13:1085-1086).

Issue #587 (callback and prompt interactions under GC stress) is the test plan that exercises this.

### Effect handlers and one-shot continuations

**Status.** Nothing in the repo designs them.

**Verdict: Partly [I].**

*Cost under design A.*
- Built over prompts, every `perform` is a delimited capture, and every resume copies the frames back out.
- Under design A that is O(frames between the handler and the `perform`) (§13:1059-1066). The estimate is about 88 KB
  per capture at depth 1,000 (§13:1066-1069).
- Generators and effect loops are where this hurts.
- C′ is the remedy: 57 ns per capture at depth 1,000 in the toy (§13:1088-1090). K15 already measures `generator`
  (§20:1954, :1978).
- `stack_switch` is excluded (§14:1291): it is one-shot and lowered only on x64 (`PRD/study/gc/research/cranelift-gc.md:158-165`).

*One-shot fibers.*
- OCaml 5-style fibers (`PRD/study/gc/research/ocaml-gambit.md:215-232`) map onto the green-thread machinery:
  - a register stack;
  - `ThreadGcState { ran_since_gc, watermark }`;
  - `FinalKind::Thread` to give the stack back (§18.1:1613-1627).
- The root rule is wrong for them, though:
  - the scheduler roots every started, non-terminated thread until it terminates (decision 23, §18.1:1626);
  - a suspended fiber must be reachable only through its continuation object, and freed when that object dies.
    OCaml leaks dropped fibers (`ocaml-gambit.md:222`).
- Fibers therefore need a second root class:
  - a heap object that owns a stack as a host payload;
  - traced by a `HostPayload::trace` that calls the VM's frame walker;
  - re-scanned through `dirty(id)` or the thread's watermark after it runs.
- The PRD's `MutatorSet::threads()` lists only rooted threads (§14:1143).

*Other points.*
- Continuations refer to no carrier (rule 6, §18.2:1666), which resuming a continuation on another thread needs.
- Chez promotes one-shot continuations to multi-shot at every collection, because its collector moves stack
  segments (`PRD/study/gc/research/chez.md:231`). Register stacks that are fixed and never relocated (V1) avoid that
  problem [I].

## 7. Notebook and long REPL sessions (steady state)

**What it asks.**
- `PRD/future/phase4/NOTEBOOK_DESIGN.md`:
  - a persistent environment, with cells re-evaluated in any order (:50-55);
  - a host-held `CellOutput { result: Value, … }` (:188-189).
- `PRD/future/phase4/NOTEBOOK_DATA_SCIENCE_VISION.md:58-76`: "zero-copy FFI" to Polars, with Scheme lambdas passed to
  Polars operations.

**Today.**
- O13: a result the host holds is freed by a later collection (#605). `run_forms` keeps the previous form's value in
  a Rust local (`crates/patina-interpreter/src/lib.rs:491,517`).
- P3: Rust tables are invisible to the trigger (#615).
- P4 and O12: symbols are immortal; the alias and `owners` tables are never pruned (#611, #613, #614).
- O6: code units are released only through sweep.
- Ctrl-C kills the REPL instead of interrupting the form: `crates/patina-repl/src` installs no SIGINT handler, so the
  default action ends the process and the session is lost.

**Against the PRD: Yes.** §17 is written for this case.
- **The steady-state lane.**
  - REPL rows: redefinition streams, the `guard` stream in REPL `-i`, `eval-redefine`, `eval-lambda`, `macro-eval`,
    `hidden-define`, `unbound-ref`, `load-repeat`, `reimport` and library redefinition (§17.4:1512).
  - The clauses SS1–SS5 (§17.1:1400-1406) and the rules M1–M5 (§17.2:1436-1442).
  - A nightly REPL soak (§17.4:1553).
- **The mechanisms.**
  - An idle trim at the REPL prompt and at `notify_idle()` (SD7, §17.1:1425-1427).
  - Ctrl-C through `InterruptHandle` (decision 9, §12:1005-1012; E6).
  - JIT code freed per unit, because "`cranelift-jit`'s `JITModule` cannot free single functions, so long REPL
    sessions would grow" (decision 20, §2:220).
  - A weak symbol table (5c).
- **Outside the contract.** Growing name sets are class D: a fresh top-level name costs 365 B (§17.1:1430). A
  notebook that defines new names for ever grows, by design.

**Host side [I].**
- `CellOutput.result` must be an `Owned` handle (E1, §11.5:953).
- An output history (`Out[n]`, `_`) is a strong table owned by the host, and M2 does not police it. It must be
  capped or clearable by the user, or it is the classic notebook leak.

**Rust data libraries such as Polars [I].**
- Data held in Rust must be reported through the external-bytes accounter (E5, §11.5:956), or the trigger is blind
  again, in #615's shape.
- A Scheme predicate passed to a Rust library can be called only through `Step::Call` (E4, §11.5:955): one
  resumption per row. Compiling the predicate to a Polars expression is the fast path.
- "Zero-copy" between Scheme bytevectors and Arrow buffers needs either a pinned LOS buffer (section 1) or a new
  object kind that views foreign memory, which K-1 would declare.

**Re-evaluating cells.**
- Rebinding semantics are decisions 2 and 3. Under variant C, a cell that shadows an import changes only code
  compiled afterwards (§11.6:978-984; #603).
- That is not a GC defect, but notebooks are where users will see it.

**An asynchronous kernel** (a UI thread beside an evaluation thread) needs:
- the isolates' audited `unsafe impl Send` wrapper (E11, §18.1:1650-1655);
- current ports owned by the interpreter (F6, #618).

## 8. General tail calls

**Status.** `PRD/future/GENERAL_TAIL_CALL_OPTIMIZATION.md` is stale. It describes the evaluator from before the CPS
rewrite (`EvalResult`, the `Value` enum) and records completion on 2025-11-11 (:306-325). Today both backends have
proper tail calls, including:
- tail-position resumable primitives (`tail_start_resumable`, `crates/patina-vm/src/runtime/control.rs:1902`);
- tail-shape deoptimization of rebound primitives (Track P item P8.2, `PRD/TRACK_P_PERFORMANCE_PRD.md:689-712`).

**Against the PRD: Yes.** Every GC clause that tail calls touch is accommodated:
- **Safe-for-space.** A tail call must not retain the replaced frame's dead values.
  - Today this is O5's retirement and the tail-call window test in `crates/patina-tests/tests/ephemerons.rs:94`.
  - In the PRD it is window initialization at every frame push (invariant 1, §11.1:829) and clearing through maps
    (invariant 3, :831). Dead-slot clearing "stays for good" (§19:1846-1848).
- **Polls.**
  - "Every tail call (self or not, whatever the window)" polls at frame entry, and a self tail call is a loop's
    back-edge (§12:1020-1021).
  - "No collection sees a half-built frame", although `TailCall` stages its arguments in a Rust buffer today
    (§12:1025-1026; `vm_state.rs:1601`).
- **Watermark.** "A tail call copies `link` and `ret`" (§11.1:839).
- **JIT.** Fragments use `CallConv::Tail` with `return_call_indirect` (§11.2:853-855, J2), so proper tail calls hold
  in every tier. K11's native call/ret alternative would need its own tail-call design (§20:1950).
- **Variant C** keeps "the tail-shape deoptimization" (§11.6:982).

## 9. Cross-cutting findings

1. **Every feature that calls Scheme from beneath a Rust or C frame ends up under `NoGcScope`.**
   - The cases: syntax-case transformers (until decision 17's `ExpansionContext`, and for good on the tree-walker),
     a paused debugger's evaluation, synchronous FFI callbacks, and nested tree-walker trampolines.
   - K16 (§20:1955) is the one control on all of them.
   - Its list of sites should name FFI callbacks, and decision 17 should say what happens on the tree-walker [I].
2. **No feature needs a new rooting mechanism.**
   - Values a host holds map onto `Owned` handles (callbacks, notebook results).
   - Retained state maps onto registered `RootProvider`s (the debugger, `ExpansionContext`).
   - Rust resources map onto host payloads (foreign objects, macro bodies, fibers).
   - C buffers map onto `PinToken`s.
   - What is missing is API surface: a pinned payload pointer, a serialize hook on `HostPayload`, and a second root
     class for stacks reachable only from the heap.
3. **One PRD rule and one inferred gap break for planned features [I].**
   - The rule "Immortal objects reference nothing mortal" (§10:753-754) breaks for a boot image with mutable cells.
   - The gap: emitters bake `GcAttrs` at compile time (§14:1149, :1260) while policy is a runtime field that gates flip
     (§9:530-531). Nothing says what a flip does to compiled bodies; JIT bodies could be invalidated, AOT code cannot
     (`followup/contract/ANSWER.md` §4, A1).
4. **Finalization stays Rust-only, allocation-free and non-reentrant (F4).**
   - Cleanup visible to Scheme is the job of guardians.
   - FFI is the first client whose Rust cleanup may block, and the rule for that is missing.
5. **The weakness rules have one hole in steady state.** Symbol-keyed ephemerons never break (§9.5:590), so a weak
   table keyed by symbols leaks, and the steady-state contract does not classify that leak.
6. **Several design docs have drifted from the PRD.**
   - On §19's update list (§19:1888-1899): `FFI_DESIGN.md`, the two debugger documents, and the hook doc's JIT notes.
   - Missing from the list: `SYNTAX_CASE_DESIGN.md` and `GENERAL_TAIL_CALL_OPTIMIZATION.md`. Both still describe the
     deleted `Value` enum.
