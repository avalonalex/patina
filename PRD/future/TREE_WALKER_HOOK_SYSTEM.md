# Tree-Walker Evaluation Hook System — Design and Feasibility

**Status:** Feasibility study / design. No implementation is scheduled by this document.
**Scope:** Tree-walker backend only (`--tree-walker`). Interpreter-side infrastructure —
the trace-hook layer and its prerequisites. §10 defines how this scope relates to the
VM, a future Cranelift JIT, and AOT — the short version: the *event vocabulary* is the
durable, backend-neutral contract; fire sites are per-backend; and a JIT interacts with
hooks through one tier-policy flag, not a redesign.
**Companion:** `PRD/future/VISUAL_DEBUGGER_DESIGN.md` — the visual step debugger built
*on* this layer (session, stepping policy, TUI, macro pane, watch UX). The two are
separate pieces of work: this layer can land, be tested headlessly, and serve other
clients (§8) before any debugger UI exists.
**Date:** 2026-08-31 (file:line references checked against commit `13e68a40`).
**Supersedes:** `PRD/ARCHIVE/phase1_completed/DEBUGGER_HOOK_SYSTEM.md` (2025-11) — that
design targets the pre-CPS recursive evaluator and the deleted `Value` enum, and was
never implemented. Its survey of Racket continuation marks, chibi's `trace.scm`, and GDB
breakpoint strategy is still worth reading; none of its signatures are.

---

## 1. Summary and verdict

The goal is the trace-hook layer that mainstream interpreted runtimes expose — patina's
`sys.settrace`: a callback interface the evaluator fires on a small fixed event set,
itself knowing nothing about breakpoints, stepping, or UI.

**Verdict: feasible, and the CPS tree-walker is unusually well suited to it.** The
evaluator is already a trampoline whose entire machine state passes through a single
`StepResult` value on every step — an observer's snapshot is a borrow of that value, not
a reconstruction. The genuine gaps owned by this document:

1. **Source coverage** — only applications carry a `SourceLocation` on CPS nodes today;
   line-granular tooling needs the CPS transform to stamp source on all node kinds
   (mechanical; the data already exists on every `CoreExpr`). (§6)
2. **A second, hidden trampoline** — `dynamic-wind` thunks and primitive-driven applies
   run in a duplicated inner loop that a naive hook would never see; it must be hooked
   too, or refactored to share dispatch first. (§4)
3. **Visibility** — `StepResult` and the step functions are `pub(super)`; the hook fires
   inside `cps_eval` and only the hook trait is exported. (§4)

Two further gaps — the synthesized call stack and macro-expansion retention — belong to
the debugger client and are designed in the companion document.

---

## 2. Where this sits: the canonical stack

Across mainstream interpreted runtimes the debug/observability architecture is
remarkably uniform — three layers, with a fourth becoming standard over the last decade:

1. **A trace-hook layer in the interpreter core.** A callback firing on a small fixed
   event set — canonically **call, line, return, exception** — with the current frame in
   hand. Instances: CPython `sys.settrace` (per-thread; since 3.12 also
   `sys.monitoring`, PEP 669), Ruby `TracePoint`, Lua `debug.sethook`
   (call/line/return/count masks, per coroutine), V8's inspector hooks.
2. **A debugger-policy library** (breakpoint tables, step bookkeeping, frame walking):
   CPython's `bdb`, the core of Ruby's `debug` gem, Delve's core. Pure logic, no UI.
3. **A UI** — classically one that *blocks inside the trace callback* (CPython's `pdb`
   runs its command loop inside the `sys.settrace` callback).
4. **A wire protocol** — DAP (debugpy, Ruby `debug`, Delve, js-debug) or engine
   protocols (CDP, JDWP), putting layers 1–2 in the debuggee and any editor on the
   other end.

**This document is layer 1.** The companion debugger document is layers 2–4.

There is a second family: **annotation/instrumentation** instead of interpreter hooks.
Racket — patina's closest relative — annotates the syntax tree, wrapping expressions in
break-check calls (DrRacket's debugger via `gui-debugger/annotator`; its stepper goes
further with full rewriting plus continuation marks), and V8 sets breakpoints by
recompiling functions with debug slots. PEP 669 moved CPython partway into this family
for cost reasons: instrument only the code objects that need it.

This study chooses the **hook family** for reasons specific to patina rather than habit:
annotation exists mostly to *create* pause points in evaluators that don't naturally
surface them, and patina's trampoline already surfaces every step with the whole machine
state in one value (§4); annotation would also have to survive macro expansion and the
absence of a CpsExpr cache (§6), which is exactly where it gets hard. The escape hatch
is recorded anyway: if per-step hook checks ever measure, PEP 669-style selective
instrumentation (compile a debug variant of a procedure's CPS tree) is the known
optimization — and the no-cache fact cuts both ways, since re-transformation per
top-level form is where a debug variant would be cheap to produce.

The event taxonomy (§5) is the canonical call/line/return/exception set —
`Apply`/`Step`/`Return`/`Raise` — plus `SetVar`/`Define`, which most trace layers lack
(the companion document's watchpoint section explains why that is an advantage, not an
eccentricity).

---

## 3. Precedents

### 3.1 The VM's `StepTracer` — in-repo precedent

`crates/patina-vm/src/tracer.rs` already implements a per-instruction observer for the
*other* backend: `TraceEvent { Instr, RegWrite, FramePush, FramePop, ControlPrimitive,
ThunkEnter, ThunkExit, CellAccess, Watchpoint }`, stored as
`pub tracer: Option<TracerHandle>` on `VmState` (`runtime/vm_state.rs:85`), fired
pre/post instruction (`vm_state.rs:982`, `:1942`), traced as a GC root
(`vm/runtime/gc_roots.rs:79`), attached via the inherent method
`VmBackend::set_tracer` (`vm/backend.rs:159`), surfaced as `--trace`, documented in
`docs/VM_STEPPER.md`. This settles three questions by precedent:

1. Observer attachment is an **inherent backend method, not a `Backend` trait method**
   (the trait stays three members; backend-specific capability lives off-trait, exactly
   like `eval_with_source_map` and the REPL's local `LibraryPaths` shim).
2. `Option<Handle>` checked per step is an accepted hot-loop cost.
3. Anything the observer retains must be a **GC root**.

The tree-walker hook is the tree-walker's `set_tracer`, grown a return value.

### 3.2 gyrus's `ExecutionHook` — the API shape, proven

The bf debugger in `~/Project/gyrus` runs on a hook trait of exactly this kind
(`ExecutionHook::before_instruction/after_instruction/on_loop_enter/on_loop_exit`,
returning `HookDecision { Continue, Break, Skip }` —
`~/Project/gyrus/crates/gyrus/src/hooks/mod.rs:436,367`), with an **immutable** context
struct, pausing implemented as the hook simply *not returning* (the `pdb` model, §2),
and `Break` unwinding the interpreter as an error only for quit/restart. Its measured
hot-path lesson transfers: the stopping rule must be cached in cheap
comparison-friendly form outside any shared cell — locking a mutex per instruction cost
~20% there. (Everything gyrus teaches about *UI and stepping policy* lives in the
companion document.)

### 3.3 Existing tree-walker debug machinery (mostly vestigial)

- `DebugConfig`/`DebugStage` (`crates/patina-tree-walker/src/eval/debug.rs:67,80`):
  only `Apply` is consumed, and only on the direct-application path — never from the CPS
  trampoline. No Scheme-level way to enable any stage exists; the primitive registration
  is an explicit stub waiting for exactly the kind of session object the companion
  document proposes. The `debug: Rc<DebugConfig>` field on `Evaluator`
  (`eval/mod.rs:44`) is the precedent for where hook storage lives.
- `PATINA_SCOPE_TRACE` (`crates/patina-core/src/scope_trace.rs`): proves an
  env-var-gated `OnceLock` check is acceptable on the per-read hot path — and its
  documented 14× slowdown with 352 MB of output on a 300k-iteration loop is the
  anti-pattern the budget in §7 designs against (never format eagerly).
- `MacroTracer` (`crates/patina-macros/src/tracer.rs:46`): thread-local, per-macro-name
  selective, depth-capped recorder of `Vec<ExpansionStep>`, already called on every
  expansion (`macro_expander/mod.rs:482`). It is the *expansion-phase* hook — a separate
  mechanism from this run-phase one, and the engine for the debugger's macro pane.

---

## 4. The machine: why this is feasible now

The tree-walker is not a recursive evaluator. It is a **trampoline**:

- Driver loop: `crates/patina-tree-walker/src/eval/cps_eval/mod.rs:195` (inside
  `eval_in_env`, `mod.rs:153`). It matches on the current `StepResult` and calls one of
  exactly three step functions, reassigning the state until `Done`.
- The state is `StepResult` (`cps_eval/types.rs:58`):

  ```rust
  pub(super) enum StepResult {
      Done(TaggedValue),
      Continue           { expr: CpsExpr, env: Rc<Environment>, cont_env: ContEnv,
                           prompt_stack: Vec<PromptFrame>,
                           dynamic_winds: Vec<DynamicWindRecord>,
                           exception_handlers: Vec<ExceptionHandler> },
      InvokeContinuation { cont: ContValue, value: TaggedValue, env: Rc<Environment>,
                           cont_env: ContEnv, /* + same tail */ },
      ApplyProc          { proc: TaggedValue, args: Vec<TaggedValue>, cont: ContValue,
                           env: Rc<Environment>, cont_env: ContEnv, /* + same tail */ },
  }
  ```

  This is the **complete** live machine state — the GC root providers say so
  (`cps_eval/gc_roots.rs:47`, `StepRoots { step, expr }`). An observer's "snapshot of
  the machine" is a borrow of this value plus the current expression. Nothing has to be
  reconstructed from a Rust stack, because there isn't one.

- The three step functions:

  | Function | Location | Meaning for an observer |
  |---|---|---|
  | `eval_one_step` | `cps_eval/step.rs:21` | evaluate CPS nodes until a control transfer |
  | `apply_cps_step` | `cps_eval/application.rs:21` | a call is happening (proc + args in hand) |
  | `invoke_continuation_step` | `cps_eval/continuation.rs:115` | a value is returning into a continuation |

Three structural facts the hook design must absorb:

**(a) The inner loop.** `eval_one_step` does not return to the driver per node. It loops
locally (`step.rs:73`) over the forms that merely rewrite `current_expr`/`current_env` —
`LetVal`, `LetCont`, `If`, `Set`, `Define`, `Prompt` — and only returns a `StepResult`
for control transfers. A hook placed at the driver loop would never see an `If` test
resolve or a `set!` happen. **The per-node hook must sit at the top of the inner loop
(`step.rs:74`), where `current_expr`, `current_env`, and `cont_env` are all in hand.**

**(b) The hidden second trampoline.** `apply_from_direct_tagged`
(`cps_eval/wind.rs:164`, its driver-loop copy at `:195`) duplicates the entire
driver-loop match so that `dynamic-wind` before/after thunks and primitive-initiated
applications can run nested. A hook installed only in the main driver is blind to
everything a wind handler does. Either both loops fire the hook, or — the better
refactor, recommended as prerequisite H0-b — the dispatch match is factored into one
shared function both loops call.

**(c) Continuation escapes are `Err`.** Invoking a captured continuation runs the wind
handlers, stashes the payload in a thread-local, and returns
`Err(EvalError::ContinuationEscape)`, which the driver catches (`mod.rs:279`) and turns
into a resumed `invoke_continuation_step`. **A hook observing step results must treat
this error as a control event, not a failure.** (Re-entering a captured continuation
also resets `prompt_stack` and `exception_handlers` to empty — `cps_eval/mod.rs:312`, a
known, quarantined divergence from the VM — so clients must not assume those recover.)

Visibility notes: `StepResult` and the step functions are `pub(super)`, so the hook
fires *inside* `cps_eval` and only the hook trait itself is exported; and
`CpsEvaluator` is a stateless `&'a Evaluator` (`mod.rs:91`), so hook storage belongs on
`Evaluator` (`eval/mod.rs:42`) beside `debug: Rc<DebugConfig>`.

---

## 5. The hook API

### 5.1 Trait sketch

```rust
// crates/patina-tree-walker/src/eval/hooks.rs  (new module)

pub enum DebugEvent<'a> {
    /// Top of eval_one_step's inner loop: every CPS node, before it is consumed.
    Step      { expr: &'a CpsExpr, env: &'a Rc<Environment> },
    /// apply_cps_step entry: a procedure application (the step-into boundary).
    Apply     { proc: TaggedValue, args: &'a [TaggedValue], cont: &'a ContValue,
                source: Option<&'a SourceLocation> },
    /// invoke_continuation_step entry: a value returning into a continuation.
    Return    { value: TaggedValue, cont: &'a ContValue },
    /// set_var_tagged (cps_eval/environment.rs:91): every user-level set!.
    SetVar    { name: &'a str, old: Option<TaggedValue>, new: TaggedValue,
                env: &'a Rc<Environment>, source: Option<&'a SourceLocation> },
    /// The Define arm (step.rs:167): every user-level define.
    Define    { name: &'a str, value: TaggedValue, env: &'a Rc<Environment>,
                source: Option<&'a SourceLocation> },
    /// maybe_route_error_through_cps (cps_eval/exceptions.rs:29): a Rust error is
    /// about to become a Scheme exception — the break-on-throw site.
    Raise     { error: &'a EvalError },
    /// A captured-continuation escape was caught by the driver (mod.rs:279).
    Unwind    { },
}

pub enum DebugDecision {
    Continue,        // proceed normally (pausing happened *inside* the hook, if at all)
    Abort,           // unwind evaluation (quit/restart), as an EvalError variant
}

pub trait DebugHook {
    fn on_event(&mut self, ev: &DebugEvent<'_>) -> DebugDecision;
}
```

Storage and attachment, mirroring the VM tracer:

```rust
// eval/mod.rs — on Evaluator, beside debug: Rc<DebugConfig>
debug_hook: RefCell<Option<Rc<RefCell<dyn DebugHook>>>>,

// crates/patina-tree-walker/src/backend.rs — inherent, like eval_with_source_map
impl TreeWalker {
    pub fn set_debug_hook(&self, hook: Option<Rc<RefCell<dyn DebugHook>>>);
}
```

Design decisions, with rationale:

- **Hooks may block.** The evaluator makes no liveness assumption about `on_event`; a
  debugger pauses by not returning until the user resumes (the `pdb` model — CPython's
  debugger runs its command loop inside the `sys.settrace` callback; gyrus works the
  same way). Single-threaded, no suspension machinery in the evaluator, no `Send` bound
  and no mutex, because patina's evaluator is single-threaded `Rc`-world already. (§9
  covers what changes if threads ever arrive; the short version: this contract is right
  until then, and the layering makes the migration additive.)
- **The context is immutable.** No `Skip`, no `ReplaceResult` (the archived 2025 design
  had both). Mutating evaluation from an observer is a can of worms (what does skipping
  an `If` test mean?), and *evaluating* things is already possible the honest way: a
  client holding the `Evaluator` can call `eval_in_env` re-entrantly against the paused
  environment. Note the re-entrant eval runs under the hook — a client must set a
  "suspended" flag so its own hook invocations return `Continue` immediately.
- **`Abort` is an `EvalError` variant** (e.g. `DebugAborted`), following the existing
  `ContinuationEscape` sentinel pattern, caught by the client's driver above
  `eval_in_env`. Used only for quit/restart — and for fuel/metering clients (§8).
- **One hook, not a handler list.** The archived design's `HookManager` with handler
  vectors is speculative generality; the VM tracer takes one handle and has needed
  nothing more. Composition, if ever needed, is a hook that fans out.

### 5.2 Fire sites (the complete set)

| Event | Site | Notes |
|---|---|---|
| `Step` | `cps_eval/step.rs:74` (top of inner loop) | the per-node hook; sees every CPS form |
| `Step` | `cps_eval/wind.rs` inner loop | **required** — or factor shared dispatch first (H0-b) |
| `Apply` | `cps_eval/application.rs:21` | proc/args/cont in hand |
| `Return` | `cps_eval/continuation.rs:115` | |
| `SetVar` | `cps_eval/environment.rs:91` (`set_var_tagged`) | single choke point for both `set!` paths |
| `Define` | `cps_eval/step.rs:167` (the `Define` arm) | **not** `Environment::define` — see below |
| `Raise` | `cps_eval/exceptions.rs:29` | the single Rust-error→Scheme-exception policy point |
| `Unwind` | `cps_eval/mod.rs:279` (escape catch) | control-transfer notification |

**Why mutation events fire at the CPS arms and never at the `Environment` layer:**
`Environment::define` is *hot* — it runs on every `LetVal` binding (`step.rs:117`),
every continuation invocation (`continuation.rs:137`), and every lambda parameter bind
(`application.rs:87–113`), mostly for gensym'd names the user has never seen. The CPS
`Set`/`Define` arms are exactly the user-visible mutations. (The lower
`Bindings::write_slot` / `Bindings::insert` chokepoints at `environment.rs:89,98` exist
but are private and too noisy; recorded only for completeness.)

---

## 6. Source coverage and mapping

What exists:

- `SourceLocation { source: Arc<str>, line: u32, column: u32, length: Option<u32> }`
  (`patina-core/src/error.rs:59`) — `length` is exactly what a sub-line highlight needs.
- The desugarer stamps a location on **every pair it desugars**
  (`patina-frontend/src/desugarer/mod.rs:1058–1063`), so every `CoreExpr` has source.
- `SourceMap` holds the full source text and renders caret context
  (`format_context`, `patina-frontend/src/source_map.rs:72`).

The gap: **only three sites in the CPS transform propagate source to `CpsExpr`**
(`patina-ir/src/cps_transform.rs:509` for `App`, `:538` for `Apply`, `:587` for the
argument-evaluation `LetVal` wrappers). `If`, `Set`, `Define`, `Lambda`, and everything
else are built with `source: None`. Today's effective granularity for any
location-driven client is therefore *applications only*.

**Prerequisite work item H0-a:** thread `CoreExpr.source` through the remaining
`CpsExpr::new` sites in `cps_transform.rs`. Mechanical (the transformer has the
`CoreExpr` in hand at every site); the risk is not difficulty but diff noise.
Acceptance: an `If` test, a `set!`, and a `define` each report a correct caret;
chibi suite stays 1226/1226.

Facts every location-driven client must design around:

- **Node addresses are unstable.** There is no CpsExpr cache — every top-level form is
  re-transformed (`cps_eval/mod.rs:367`), so `CpsExpr` addresses are unstable across
  forms (lambda bodies are `Rc`-shared only *within* a procedure). Clients key by
  `(file, line, column)` and match against `expr.source` at `Step` events, never by
  node identity.
- **`SourceMap.locations` is keyed by raw NaN-box bits** and pruned at top-level-form
  boundaries (`prune_freed_locations`) because the GC reuses addresses. Clients never
  store those keys; they store `SourceLocation` values, which are self-contained.
- The map is created per `eval_*_with_source_name` call
  (`patina-interpreter/src/lib.rs:461+`) and handed back — a long-lived client must own
  the returned `Rc<RefCell<SourceMap>>` itself.
- REPL/eval sources are named `<repl-N>`/`<string>` — location-driven clients must
  handle a program assembled from multiple named chunks.

---

## 7. Performance budget

Baseline reality of the step loop (measured shape,
`benchmark_reports/profiling_results.md`: dispatch and state-shuffling ≈ 85%, real
arithmetic < 2%):

- `eval_one_step` **already clones the expression** on entry and per inner iteration
  (`step.rs:47,120,135,149,163,184,192`); `CpsExpr::clone` deep-copies the
  `Vec<CpsExpr>` args of `App`/`Apply`/`PrimOp` — an allocation per application.
- Every `StepResult` moves three `Vec`s plus two `Rc`s.
- A GC safe point (one cached-flag load when idle) already runs per driver iteration
  (`mod.rs:198` → `maybe_collect`).

Against that baseline, `if let Some(hook) = …` is one predictable branch per inner-loop
iteration: **well under 1% disabled**, in line with the VM tracer's accepted
`Option<TracerHandle>` check. Rules for the enabled path, learned from
`PATINA_SCOPE_TRACE`'s 14× cautionary tale:

1. Never format, `Display`, or collect `bindings()` in a hook's fast path — only after
   deciding to act. The pause path can be arbitrarily slow; the continue path must be a
   few comparisons.
2. Fast-out order for a free-running client: `expr.source.is_none()` → return; then a
   cheap set/bitmap test; mutation tests only on `SetVar`/`Define` (already rare).
3. A UI-owning client polls the terminal every N events, not every event (gyrus: 2048).
4. No mutex — single thread, `Rc<RefCell<…>>`, borrow discipline per CLAUDE.md (extract
   values before re-borrowing; the hook runs inside the evaluator, so it must never call
   back into anything that borrows evaluator state it is holding).

---

## 8. Clients of the hook layer

General practice says a trace layer never stays single-purpose: `sys.settrace` carries
coverage.py and profiling tools as well as `pdb`; JVMTI carries JaCoCo and every Java
profiler as well as JDWP. Clients this layer enables, roughly in order of
effort-to-value:

- **The visual step debugger** — the driving client; designed in
  `PRD/future/VISUAL_DEBUGGER_DESIGN.md`.
- **Post-mortem debugging** (pdb's `pm()`): on an uncaught `Raise`, drop into the
  paused environment instead of just printing the error.
- **Real stack traces in ordinary error output.** An opt-in shadow stack (no UI, no
  pausing — the synthesis is designed in the debugger doc) upgrades
  `format_interpreter_error` from "one caret + macro chain" to a proper backtrace.
- **Procedure tracing** — chibi's `(trace f)` implemented at the interpreter level via
  `Apply`/`Return` (args in, value out, depth-indented), finally replacing the
  vestigial `DebugConfig` apply trace (§3.3) and giving the stubbed Scheme-level debug
  primitives something real to switch on.
- **Coverage.** Mark sourced `Step` events seen, aggregate per file. Which lines of
  `lib/scheme/**/*.scm` the chibi suite never executes is a question nobody can answer
  today — and the corpus-vs-conformance experience says that blind spot is exactly
  where defects hide.
- **Deterministic profiling.** `Apply`/`Return` counts and inclusive time per
  procedure; flame graphs; the Scheme-level complement to the existing Rust-level
  profiles for Track P.
- **Backend differential tracing** — the patina-specific one. The VM already emits a
  `StepTracer` stream; a tree-walker hook emitting comparable events (applies, set!s,
  defines, raises) makes divergence hunting automatic: run both backends, diff the
  streams, report the first divergent event with its source location. The GC
  differential lane and the `assert_divergence` quarantine show how much this project
  already leans on exactly this style of verification.
- **Step budgets and metering.** "Abort after N steps" gives deterministic
  infinite-loop detection in CI (no wall-clock flakes) and resource limits for
  embedding patina in a host application.
- **Engines.** A step-count-triggered park is the classic Scheme *engines* construct —
  preemption built on fuel — and is literally the scheduler primitive §9.2's green
  threads would need. The debugger and a future SRFI-18 are clients of the same
  mechanism.
- **Teaching and visualization.** `PRD/future/INTERACTIVE_TUTORIAL_DESIGN.md` and the
  phase-4 notebook could animate evaluation Python-Tutor-style from the same events.
- **Invariant checking.** `SetVar`/`Define` generalize watchpoints into test-lane
  assertions ("this global stays a fixnum") with no new machinery.

Two boundaries, so this list doesn't oversell: anything that must run always-on in
production wants sampling, not deterministic hooks (§7's budget assumes hooks are a
development mode); and expansion-time tooling is a different phase with its own hook —
`MacroTracer` (§3.3) — not this one.

---

## 9. Future: threads

Neither R7RS-small nor patina has threads today, so nothing in this section is in
scope — but the hook API should not foreclose the future, and the general practice is
well settled.

### 9.1 What debuggers generally do about threads

- **Trace hooks are per-thread almost everywhere.** `sys.settrace` affects only the
  calling thread (`threading.settrace` installs it for future ones); Lua hooks are per
  coroutine/`lua_State`; JVMTI events carry their thread. Consequently *stepping state
  is per-thread* — each thread owns its run state and its stack — while *breakpoint
  and watch tables are global*. The companion document's session keeps exactly that
  split, and it is the property to preserve.
- **Two stop models.** *All-stop* (GDB's default, JDWP `SUSPEND_ALL`, debugpy's
  default): any thread hitting a breakpoint suspends the world, and the user inspects
  one coherent frozen state. *Non-stop* (GDB non-stop mode, JDWP
  `SUSPEND_EVENT_THREAD`): only the hitting thread pauses while the rest keep running.
  DAP expresses both (`stopped` events carry a `threadId`; `continue` has
  `allThreadsContinued`). All-stop is the right default for a correctness-oriented
  debugger; non-stop exists for systems that must keep servicing work while inspected.
- **The *other* threads are stopped cooperatively, at safe points.** An in-process
  debugger cannot preempt its own threads, so runtimes reuse their existing yield
  points: the JVM suspends threads at GC safepoints, CPython via the eval-breaker flag
  every thread checks at bytecode boundaries, Go at runtime preemption points.
  **Patina already owns this mechanism** — the GC safe point at the top of every
  driver iteration (`maybe_collect`, a single cached-flag load, §7). A threaded
  patina would park threads for the debugger exactly the way it would park them for a
  collection: one flag, two clients.
- **Threads are what kill the blocking-console model.** `pdb`-in-the-callback works
  with one thread; with several, the paused thread's hook cannot own the terminal
  while other threads print and hit breakpoints concurrently. This is the
  architectural force that pushed real-world debuggers to the client/server split
  (debugpy's DAP server thread, JDWP, CDP): layers 1–2 stay in the debuggee, and the
  *UI* moves out of the hook — a paused hook parks its thread at a safe point and
  notifies a controller instead of reading the keyboard. Because the companion
  document keeps `should_pause` pure and the session separate from the UI, that
  migration changes the pause plumbing, not the debugger.

### 9.2 What threads would plausibly look like here

An `Rc`/`RefCell` runtime cannot take OS threads without an `Arc`-and-locks rewrite of
the heap — a runtime project that dwarfs any debugger. The realistic path, as in
several Schemes, is **SRFI-18-style green threads multiplexed on one OS thread** — and
on this machine that is a debugging gift, because a green thread *is* a `StepResult`
plus its dynamic state (§4), held by a scheduler in exactly the shape an observer
already snapshots. Then:

- a thread pane is a walk over the scheduler's thread set, each entry fully
  inspectable for free — the Smalltalk/Erlang experience of processes as first-class
  objects, not the pthread experience;
- all-stop is trivial: the scheduler stops scheduling. Per-thread stepping is keying
  run state and the shadow stack by thread id;
- the hook needs exactly one forward-compatible touch **now**: define events as firing
  on "the current machine", so adding a thread identity to `DebugEvent` later is an
  additive field, not a redesign.

That one guardrail, plus the global-tables/per-thread-state split and pure pause
predicates in the client, is the entire cost of keeping the threaded future open.
Nothing else in this design bets against it.

---

## 10. Scope: what happens when more backends arrive

The roadmap includes a Cranelift JIT soon, possibly AOT one day. Neither changes this
document's implementation scope (tree-walker fire sites), but they sharpen what the
durable contract is. The scope question decomposes into three pieces with different
lifetimes:

1. **The event vocabulary — backend-neutral, the durable part.** "A sourced statement
   is about to run", "a call entered", "a value returned", "a variable was mutated",
   "an exception is being raised" are *Scheme-semantics* facts, not tree-walker facts.
   This is the part that must outlive backends — it is what JVMTI and DAP are:
   contracts that survived multiple VM rewrites underneath them. Consequence for the
   design here: the session/policy layer (companion doc) must consume events at this
   semantic level and never pattern-match on `CpsExpr` beyond extracting
   `SourceLocation` — then a VM lowering (`StepTracer` → `DebugEvent` adapter) can
   drive the same session later. Per the rule of two, the Rust-level abstraction is
   *not* built now: H1's trait is the tree-walker binding of the contract, and the
   shared trait moves to `patina-runtime` when the second backend client actually
   appears.
2. **Delivery — per backend, by design.** The tree-walker fires from the trampoline
   (§5.2); the VM already fires from its dispatch loop (`StepTracer`, §3.1); a JIT
   mostly does not fire at all — that is the tier policy's job, below.
3. **Tier policy — the JIT question.** Industry practice is unanimous: **enabling
   hooks pins execution to a hook-capable tier.** LuaJIT never fires debug hooks from
   compiled traces — hooked code runs in its interpreter; CPython's specializing tiers
   stand down where `sys.monitoring` instrumentation is active (PEP 669 was designed
   for exactly this); V8 deoptimizes a function the moment a breakpoint lands in it
   and runs it in the bytecode tier with debug slots; Julia's debugger simply
   interprets. The one runtime that did the expensive alternative — HotSpot's
   deopt-on-demand, where compiled frames carry full metadata to reconstruct
   interpreter state at any safepoint — represents engineering effort nobody
   replicates at this scale.

### 10.1 The tier rule for patina

**Hooks attached ⇒ don't tier up.** One flag, consulted at tier-up/dispatch time, à la
PEP 669; code runs in the VM (which already has `StepTracer`) or the tree-walker.
Patina is unusually well-positioned here — it already has *two* hook-capable tiers,
where most runtimes have one. Attach timing: support attaching hooks at top-level-form
boundaries only (the same boundary where `prune_freed_locations` already runs), which
sidesteps mid-frame invalidation of compiled code entirely. Deopt-grade participation
of JIT'd frames (per-statement stepping *inside* compiled code) is **explicitly out of
scope** unless a real need proves it — the tier rule makes it unnecessary.

Two design notes for the JIT itself, cheap to honor from day one:

- **The event set partitions by what survives compilation.** Mutation and control
  events that funnel through runtime helpers — `SetVar`, `Define`, `Raise`, and
  `Apply`/`Return` at real call boundaries — can keep firing from JIT'd code for free
  if the helpers keep firing them. Only per-node `Step` (deterministic statement-level
  stepping) truly requires an interpreter tier. So watchpoints and break-on-exception
  could in principle stay live under the JIT; stepping is what pins the tier.
- **Safepoint metadata is designed once.** A Cranelift tier needs GC safepoints and
  stackmaps regardless; if debugging-grade deopt is ever wanted, it piggybacks on that
  metadata (the HotSpot pattern). Design the stackmap story for GC; do not promise it
  for debugging.

And one payoff in the other direction: when the JIT is being *built*, this layer is its
differential oracle. VM-vs-JIT event-stream diffing (§8's backend differential tracing,
three-way once the JIT exists) is the standard way to debug a new backend against a
trusted one. The JIT raises the hook system's value; it does not threaten it.

### 10.2 AOT

A different family entirely, and **out of scope for the hook system** — with no
interpreter present at runtime, there is no hook-capable tier to pin to. The
Cranelift-world answer is the Wasmtime one: emit DWARF so native debuggers (lldb/gdb)
debug the compiled code with source mapping. The pragmatic alternative is the C model:
"debug builds" run the VM tier with hooks available, release AOT builds debug via
DWARF or not at all. Either way, the AOT debug story is a property of the AOT design,
not of this layer; the only thing this document asks of a future AOT effort is to not
redefine the event vocabulary of point 1.

## 11. Work plan (if implemented)

- **H0-a — source coverage**: stamp `CoreExpr.source` onto all `CpsExpr` kinds in
  `cps_transform.rs`. Acceptance: `If`/`Set`/`Define` errors and breakpoints carry
  correct carets; chibi suite stays 1226/1226. (~days)
- **H0-b — shared dispatch**: factor the duplicated driver-loop match in `wind.rs:195`
  and `mod.rs:195` into one function. Acceptance: no behavior change; GC differential
  lane green. (~days)
- **H1 — hook layer**: `DebugHook` trait, the eight fire sites, `set_debug_hook`,
  `DebugAborted` plumbing; headless tests asserting fire order and completeness (a
  recording hook run over fixture programs — including a `dynamic-wind` fixture that
  fails if the second trampoline is unhooked). (~1 week)

Each lands independently; H1 is the go/no-go gate for everything in the companion
document. H0-b touches `cps_eval` and should not land concurrently with other
tree-walker surgery.

## 12. Open questions

- **Q1 — the pause contract.** This study recommends the standard in-process
  blocking-hook contract (`pdb`'s; gyrus's): hooks may block indefinitely; the
  evaluator makes no liveness assumption. The alternative — a
  command-channel/controller split — buys nothing while patina is single-threaded and
  adds `Send` bounds the `Rc`-based runtime cannot meet; §9 sketches the migration
  path if threads ever force it. Any objection before H1 bakes it in?

## References

- VM stepper: `docs/VM_STEPPER.md`, `crates/patina-vm/src/tracer.rs`
- Machine internals: `crates/patina-tree-walker/src/eval/cps_eval/{mod,step,application,continuation,wind,types,gc_roots}.rs`
- Source plumbing: `crates/patina-ir/src/cps_transform.rs`,
  `crates/patina-frontend/src/{source_map.rs,desugarer/mod.rs}`
- Archived pre-CPS design: `PRD/ARCHIVE/phase1_completed/DEBUGGER_HOOK_SYSTEM.md`
- Macro instrumentation: `crates/patina-macros/src/tracer.rs`,
  `crates/patina-core/src/scope_trace.rs`, `docs/MACRO_SYSTEM.md`
- gyrus hook API: `~/Project/gyrus/crates/gyrus/src/hooks/mod.rs`
- General practice (§2, §9): CPython `sys.settrace`/`bdb`/`pdb` and PEP 669
  (`sys.monitoring`); Ruby `TracePoint`; Lua `debug.sethook`; Racket
  `gui-debugger/annotator` (the annotation family); the Debug Adapter Protocol
  specification (layer 4); JDWP suspend policies and GDB's all-stop/non-stop
  documentation (threading models)
