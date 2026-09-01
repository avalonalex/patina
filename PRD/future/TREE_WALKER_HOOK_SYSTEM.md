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
**Date:** 2026-08-31. File:line references were checked against commit `13e68a40`; §11
lists the prerequisites this checking turned up, several of which were *not* obvious
from a reading of the evaluator.
**Supersedes:** `PRD/ARCHIVE/phase1_completed/DEBUGGER_HOOK_SYSTEM.md` (2025-11) — that
design targets the pre-CPS recursive evaluator and the deleted `Value` enum, and was
never implemented. Its survey of Racket continuation marks, chibi's `trace.scm`, and GDB
breakpoint strategy is still worth reading; none of its signatures are.

---

## 1. Summary and verdict

The goal is the trace-hook layer that mainstream interpreted runtimes expose — patina's
`sys.settrace`: a callback interface the evaluator fires on a small fixed event set,
itself knowing nothing about breakpoints, stepping, or UI.

**Verdict: feasible, but the event payloads cost more plumbing than the fire sites do.**
The evaluator is a trampoline whose machine state passes through a single `StepResult`
value on every step, so *observing* it is easy; what is not free is that the data a
debugger wants — a call site's location, a variable's previous value, a procedure's
name — is either dropped at the `StepResult` boundary or never recorded. The genuine
gaps owned by this document, with §11 sizing them:

1. **Source coverage is thin in three separate ways** (§6): only applications stamp a
   `SourceLocation` onto CPS nodes; `SourceLocation.length` is never populated by any
   non-test code, so sub-line highlighting has nothing to draw; and library Scheme is
   parsed with no source map at all, so `lib/scheme/**` is invisible to any
   location-driven client.
2. **Three of the six proposed events have no payload at their fire site** (§5.3) —
   `Apply` has no source or env, `SetVar` has no previous value or location, `Return`
   has neither env nor source. Each needs threading work in the evaluator.
3. **`Raise` has three bypass paths** (§5.3) — user-level `(raise obj)` and
   `(error …)` never reach the single policy point the naive design hooks.
4. **Retained heap values need a root seam that does not exist** (§5.2) — the safe
   point builds a closed literal array of root providers.
5. **Re-entrancy** (§5.2) — a hook that evaluates Scheme (the `p <expr>` command,
   conditional breakpoints) re-enters the evaluator underneath itself; both the
   `RefCell` discipline and the GC defer guard have to be designed for it.

What is *not* a gap, contrary to a plausible first reading: the `dynamic-wind`
trampoline needs no separate fire site (§4b).

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
(the companion document's watchpoint section explains why that is an advantage even
though §5.3 shows it is noisier here than it first appears).

---

## 3. Precedents

### 3.1 The VM's `StepTracer` — in-repo precedent

`crates/patina-vm/src/tracer.rs` already implements a per-instruction observer for the
*other* backend: a `TraceEvent` enum covering instructions, register writes, frame
push/pop, control primitives, thunk enter/exit, cell access, exceptions, continuations
and watchpoints; stored as `pub tracer: Option<TracerHandle>` on `VmState`
(`runtime/vm_state.rs:85`), fired pre/post instruction (`vm_state.rs:982`, `:1942`),
traced as a GC root through `impl GcRoots for StepTracer` (`tracer.rs:283`, called from
`vm/runtime/gc_roots.rs:79`), attached via the inherent method `VmBackend::set_tracer`
(`vm/backend.rs:159`), surfaced as `--trace`, documented in `docs/VM_STEPPER.md`. This
settles three questions by precedent:

1. Observer attachment is an **inherent backend method, not a `Backend` trait method**
   (the trait keeps its associated `Error` type and three methods — `eval`,
   `global_env`, and the provided `eval_global`, `patina-runtime/src/backend.rs:55–69`;
   backend-specific capability lives off-trait, exactly like `eval_with_source_map` and
   the REPL's local `LibraryPaths` shim).
2. `Option<Handle>` checked per step is an accepted hot-loop cost.
3. Anything the observer retains must be a **GC root**, and the way to get there is a
   `GcRoots` impl reachable from the safe point's root array (§5.2 — the tree-walker's
   array is closed today, which the VM's is not).

The tree-walker hook is the tree-walker's `set_tracer`, grown a return value. One
caveat the precedent does *not* cover: `StepTracer` never re-enters evaluation, so it
never has to think about §5.2's re-entrancy problem.

### 3.2 gyrus's `ExecutionHook` — the API shape, proven

The bf debugger in `~/Project/gyrus` runs on a hook trait of exactly this kind
(`ExecutionHook::before_instruction/after_instruction/on_loop_enter/on_loop_exit`,
returning `HookDecision { Continue, Break, Skip }` —
`~/Project/gyrus/crates/gyrus/src/hooks/mod.rs:436,367`), with an **immutable** context
struct, pausing implemented as the hook simply *not returning* (the `pdb` model, §2 —
the blocking `event::read()` lives in the debugger's `ui.rs`, called from the hook), and
`Break` unwinding the interpreter as an error only for quit/restart. Its measured
hot-path lesson transfers: the stopping rule must be cached in cheap
comparison-friendly form outside any shared cell — locking a mutex per instruction cost
~20% there. (Everything gyrus teaches about *UI and stepping policy* lives in the
companion document.)

### 3.3 Existing tree-walker debug machinery (mostly vestigial)

- `DebugConfig`/`DebugStage` (`crates/patina-tree-walker/src/eval/debug.rs:80,67`):
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
  selective, depth-capped, already called on every expansion
  (`macro_expander/mod.rs:482`). It is the *expansion-phase* hook — a separate
  mechanism from this run-phase one. Note its payload is narrow: `ExpansionStep` is
  `{ macro_name, rule_index, total_rules, input: String }` (`patina-macros/src/error.rs:10`)
  and the recording site stores a pre-rendered string of the *input* args only
  (`macro_expander/debug.rs:245`), never the expanded form. The companion document's
  macro pane therefore needs a new payload, not just a new consumer.

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

  This is the complete live *machine* state — the GC root providers say so
  (`cps_eval/gc_roots.rs:47`, `StepRoots { step, expr }`). It is **not** the complete
  *debugging* state: no source location survives into `ApplyProc` (§5.3), and there is
  no frame chain (the companion document synthesizes one).

- The three step functions:

  | Function | Location | Meaning for an observer |
  |---|---|---|
  | `eval_one_step` | `cps_eval/step.rs:21` | evaluate CPS nodes until a control transfer |
  | `apply_cps_step` | `cps_eval/application.rs:21` | a call is happening (proc + args in hand) |
  | `invoke_continuation_step` | `cps_eval/continuation.rs:115` | a value is returning into a continuation |

Four structural facts the hook design must absorb:

**(a) The inner loop.** `eval_one_step` does not return to the driver per node. It loops
locally (`step.rs:73`) over the forms that merely rewrite `current_expr`/`current_env` —
`LetVal`, `LetCont`, `If`, `Set`, `Define`, `Prompt` — and only returns a `StepResult`
for control transfers. A hook placed at the driver loop would never see an `If` test
resolve or a `set!` happen. **The per-node hook must sit at the top of the inner loop
(`step.rs:74`), where `current_expr`, `current_env`, and `cont_env` are all in hand.**

**(b) The `dynamic-wind` trampoline needs no second fire site.**
`apply_from_direct_tagged` (`cps_eval/wind.rs:164`) *does* contain a copy of the driver
loop (`wind.rs:195`) so that wind before/after thunks and primitive-initiated
applications can run nested — but that copy contains no `CpsExprKind` dispatch of its
own: every arm delegates straight back to `eval_one_step` (`wind.rs:209`),
`invoke_continuation_step`, and `apply_cps_step`. **A hook at `step.rs:74` therefore
already fires inside wind handlers**, and no `Unwind` site is needed there either
(that loop never catches `ContinuationEscape`). Factoring the two driver loops into one
shared function remains a reasonable cleanup, but it is *not* a hook prerequisite and
is not scheduled by this document. (An earlier draft of this study asserted the
opposite; the delegation was verified by reading `wind.rs:195–247`.)

**(c) Continuation escapes are `Err`.** Invoking a captured continuation runs the wind
handlers, stashes the payload in a thread-local, and returns
`Err(EvalError::ContinuationEscape)`, which the driver catches (`mod.rs:279`) and turns
into a resumed `invoke_continuation_step`, taking `(TaggedValue, Rc<CpsContinuation>)`
from the thread-local (`mod.rs:281`). **A hook observing step results must treat this
error as a control event, not a failure**, and the `Unwind` event should carry that
pair, since clients need the target continuation to do anything better than truncate
their stack view. (Re-entering a captured continuation also resets `prompt_stack` and
`exception_handlers` to empty — `cps_eval/mod.rs:312`, a known, quarantined divergence
from the VM — so clients must not assume those recover.)

**(d) Operands never reach the dispatch match.** `eval_trivial_tagged`
(`cps_eval/environment.rs:22`) is called from 14 sites in `step.rs` to consume trivial
operand nodes — variable references, literals, `ContRef`s — in place, without
re-entering `match &current_expr.kind`. So a `Step` hook at `step.rs:74` sees *forms*,
not every sub-expression: in `(+ (fib n) 1)`, the `Var` node for `n` fires nothing.
This is mostly harmless (those nodes carry no source anyway — §6) but it bounds how
fine a source highlight can be, and it makes "sees every CPS form" the honest claim
rather than "sees every node".

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
    /// Top of eval_one_step's inner loop: every CPS *form* (see §4d).
    Step      { expr: &'a CpsExpr, env: &'a Rc<Environment> },
    /// A procedure application — the step-into boundary.
    Apply     { proc: TaggedValue, args: &'a [TaggedValue], cont: &'a ContValue,
                env: &'a Rc<Environment>, source: Option<&'a SourceLocation> },
    /// A value returning into a continuation.
    Return    { value: TaggedValue, cont: &'a ContValue, env: &'a Rc<Environment> },
    /// A user-level set!.
    SetVar    { name: &'a str, scopes: &'a ScopeSet, old: Option<TaggedValue>,
                new: TaggedValue, env: &'a Rc<Environment>,
                source: Option<&'a SourceLocation> },
    /// A user-level define.
    Define    { name: &'a str, scopes: &'a ScopeSet, value: TaggedValue,
                env: &'a Rc<Environment>, source: Option<&'a SourceLocation> },
    /// An exception is being signalled — from (raise obj), (error …), or a Rust
    /// error being routed into Scheme. `object` is the raised value when there is
    /// one; `error` is set when a Rust-side error is being converted.
    Raise     { object: Option<TaggedValue>, error: Option<&'a EvalError>,
                handled: bool },
    /// A captured-continuation escape was caught by the driver (mod.rs:279).
    Unwind    { value: TaggedValue, target: &'a Rc<CpsContinuation> },
}

pub enum DebugDecision {
    Continue,        // proceed normally (pausing happened *inside* the hook, if at all)
    Abort,           // unwind evaluation (quit/restart), as an EvalError variant
}

/// `GcRoots` supertrait: a hook that retains heap values (watchpoint targets,
/// recorded macro datums, a trace log) must trace them — see §5.2.
pub trait DebugHook: GcRoots {
    fn on_event(&mut self, ev: &DebugEvent<'_>) -> DebugDecision;
}
```

Design decisions, with rationale:

- **Hooks may block.** The evaluator makes no liveness assumption about `on_event`; a
  debugger pauses by not returning until the user resumes (the `pdb` model — CPython's
  debugger runs its command loop inside the `sys.settrace` callback; gyrus works the
  same way). Single-threaded, no suspension machinery in the evaluator, no `Send` bound
  and no mutex, because patina's evaluator is single-threaded `Rc`-world already. (§9
  covers what changes if threads ever arrive.)
- **The context is immutable.** No `Skip`, no `ReplaceResult` (the archived 2025 design
  had both). Mutating evaluation from an observer is a can of worms (what does skipping
  an `If` test mean?), and *evaluating* things is possible the honest way — see the
  re-entrancy rules below.
- **`Abort` is an `EvalError` variant** (e.g. `DebugAborted`), following the existing
  `ContinuationEscape` sentinel pattern, caught by the client's driver above
  `eval_in_env`. **Known limitation:** unwinding this way does not run `dynamic-wind`
  after-thunks, so a quit/restart mid-program leaves `parameterize` swaps and
  redirected ports in place. For a debugger that tears down its session immediately
  this is acceptable; a client that keeps evaluating after an abort must run
  `run_wind_handlers(&current_winds, &[])` (`cps_eval/wind.rs:23`) itself.
- **One hook, not a handler list.** The archived design's `HookManager` with handler
  vectors is speculative generality; the VM tracer takes one handle and has needed
  nothing more. Composition, if ever needed, is a hook that fans out.

### 5.2 Storage, re-entrancy, and GC

Three constraints that a naive `RefCell<Option<Box<dyn DebugHook>>>` gets wrong:

**Re-entrant borrow.** Dispatch must call `on_event(&mut self, …)`, i.e. hold a mutable
borrow of the hook. A debugger hook evaluates Scheme while paused (`p <expr>`,
conditional breakpoints), which re-enters `eval_in_env`, which fires events, which
would re-borrow the same cell — `BorrowMutError`, exactly the panic class CLAUDE.md's
borrow rule exists to prevent. **Storing a "suspended" flag inside the hook does not
help: reading it needs the same borrow.** The fix is to make re-entrancy visible at the
cell:

```rust
// on Evaluator, beside debug: Rc<DebugConfig>
debug_hook: RefCell<Option<Rc<RefCell<dyn DebugHook>>>>,

// dispatch (inside cps_eval), the only place that fires events:
fn fire(&self, ev: DebugEvent<'_>) -> Result<(), EvalError> {
    let hook = self.evaluator.debug_hook.borrow().clone();   // short borrow, then drop
    let Some(hook) = hook else { return Ok(()) };
    // A re-entrant eval from inside the hook finds the cell already borrowed and
    // silently skips firing — the debugger's own `p <expr>` must not recurse into
    // its own breakpoints.
    let Ok(mut h) = hook.try_borrow_mut() else { return Ok(()) };
    match h.on_event(&ev) { DebugDecision::Continue => Ok(()),
                            DebugDecision::Abort => Err(EvalError::DebugAborted) }
}
```

`try_borrow_mut` is the re-entrancy guard, not a workaround: "already borrowed" *means*
"we are inside the hook", which is exactly when firing must be suppressed.

**GC rooting has no seam today.** The safe point builds a closed literal array —
`collect(&[evaluator, &*registry, &gc_roots::EscapeRoots, &step_roots])`
(`cps_eval/mod.rs:126`) — and there is no root-provider registration anywhere in the
tree-walker. Since `evaluator` is already a provider in that array, the cheap fix is to
have `Evaluator`'s `GcRoots` impl forward to the installed hook, which is why
`DebugHook: GcRoots` is a supertrait above. Without this, a watchpoint's retained target
value or a recorded macro datum is an unrooted heap value across a collection — a
use-after-free, not a leak.

**A paused hook cannot collect.** `eval_in_env` takes a `GcDeferGuard` and computes
`is_outermost` (`cps_eval/mod.rs:162,166`) to gate its safe point; `GcController::safe_point`
returns early when not outermost (`patina-core/src/heap/gc.rs:381`). A re-entrant
`eval_in_env` called from inside a hook runs under the outer trampoline's still-live
guard, so **no collection happens for the duration of a pause**. Every `p <expr>`,
every conditional-breakpoint predicate, and any fuel/parking client allocates without
collecting until the program resumes. For interactive use this is usually fine (a pause
is short and human-paced); for a client that evaluates in a loop while paused it is a
heap-growth bug. Fixing it properly means letting the debugger's re-entrant eval opt
into being its own outermost extent — worth designing only if a client needs it, but it
must not be described as free.

Attachment mirrors the VM tracer:

```rust
// crates/patina-tree-walker/src/backend.rs — inherent, like eval_with_source_map
impl TreeWalker {
    pub fn set_debug_hook(&self, hook: Option<Rc<RefCell<dyn DebugHook>>>);
}
```

### 5.3 Fire sites, and the payload each one still needs

The sites exist; several of the payloads do not. This table is the actual work list.

| Event | Site | Payload status |
|---|---|---|
| `Step` | `cps_eval/step.rs:74` (top of inner loop) | **Ready.** Covers wind thunks via delegation (§4b); does not see trivial operands (§4d). |
| `Apply` | `cps_eval/application.rs:21` | **Needs plumbing.** `apply_cps_step` receives no `SourceLocation`, and `StepResult::ApplyProc` has no source field (`types.rs:83`) — the call site's location is dropped at the `StepResult` boundary. Either add `source` to `ApplyProc` (threaded from the `App` arm, where `current_expr.source` is live) or fire the event at that arm instead. Also note `Procedure::CpsLambda` has **no name field** (`patina-core/src/procedure.rs:57`; only primitives carry names), so "procedure name" must come from the call site's `Var`, not the value. |
| `Return` | `cps_eval/continuation.rs:115` | **Needs plumbing** for `env`/`source` (available from `ContValue::Local`, absent otherwise). |
| `SetVar` | `cps_eval/environment.rs:91` (`set_var_tagged`) | **Needs plumbing.** No location parameter (the `Set` arm at `step.rs:163` has `current_expr.source` and discards it), and no previous value (`Environment::set` overwrites without reading, `patina-core/src/environment.rs:271`) — an `old` value costs an extra `get` under the watch flag. Carries `scopes`, which the event must pass through (`CpsExprKind::Set` pairs the symbol with a `ScopeSet`, `cps_expr.rs:327`) since two macro-introduced bindings can share a name. |
| `Define` | `cps_eval/step.rs:167` (the `Define` arm) | **Ready** apart from `scopes` threading. |
| `Raise` | `cps_eval/exceptions.rs:29` **plus** `apply_raise`/`apply_error` (`cps_eval/application.rs:518`, `:~630`) **plus** `continuation.rs:394` | **Needs three sites, not one.** `apply_raise` dispatches to `exception_handlers.last()` inline and synthesizes `EvalError::SchemeException` itself when no handler exists; it never routes through `maybe_route_error_through_cps`. Hooking only the latter makes break-on-exception silent for `(raise 'oops)` and `(error "boom")` — the ordinary way Scheme signals. The payload must be `Option<TaggedValue>`, since a raised object is an arbitrary value, not an `EvalError`. |
| `Unwind` | `cps_eval/mod.rs:279` (escape catch) | **Ready**, and should carry the `(value, target continuation)` pair already in hand at `:281`. |

**Why mutation events fire at the CPS arms and never at the `Environment` layer:**
`Environment::define` is *hot* — it runs on every `LetVal` binding (`step.rs:117`),
every continuation invocation (`continuation.rs:137`), and every lambda parameter bind
(`application.rs:87–115`), mostly for gensym'd names. The CPS `Set`/`Define` arms are
the user-*written* mutations. Note carefully that this is **not** the same as
user-*meaningful* mutations: `letrec`, `letrec*`, named `let` and `do` all expand to
`(set! var init)` on the user's own identifier (`lib/scheme/base/binding.scm:29,36`,
and the `do` expansion), so those arms fire for writes the user never wrote, under
names no gensym filter can exclude. The companion document's watchpoint section owns
that consequence.

---

## 6. Source coverage and mapping

Three independent gaps, all of which a location-driven client hits.

**(a) CPS nodes.** Only three sites in the CPS transform propagate source to `CpsExpr`
(`patina-ir/src/cps_transform.rs:509` for `App`, `:538` for `Apply`, `:587` for the
argument-evaluation `LetVal` wrappers). `If`, `Set`, `Define`, `Lambda` and the rest are
built with `source: None`, so today's effective granularity is *applications only*.
Extending this is largely mechanical — the transformer has the `CoreExpr` in hand — with
one caveat that stops it being uniformly so: **the desugarer stamps locations on pairs,
not atoms** (symbols return at `patina-frontend/src/desugarer/mod.rs:1019`, literals at
`:1030`), so `Var` and `Literal` `CoreExpr` nodes have no source to propagate. Combined
with §4d, this bounds highlighting to the enclosing form.

**(b) `SourceLocation.length` is never populated.** The struct has it
(`patina-core/src/error.rs:59`) and `format_context` uses it for caret width
(`source_map.rs:72`), but `with_length` has **zero non-test callers** — every location
is built by `new`, with `length: None`, so carets are one character wide and a sub-line
highlight has no extent to draw. Populating it means recording token spans in the
lexer/parser. This is a real prerequisite for the debugger's source pane, not an
existing capability.

**(c) Library Scheme has no source map at all.** `Parser::new_with_heap` hard-codes
`source_map: None, source_name: "<unknown>"` (`patina-frontend/src/parser/mod.rs:89`),
and library bodies are desugared without a source map, so every `CoreExpr` originating
in `lib/scheme/**` has `source: None` regardless of what H0-a does. Consequences: the
coverage client of §8 cannot answer "which lines of the Scheme stdlib does the suite
never execute" without fixing this first, and stepping silently behaves like step-over
through `map`, `assoc`, `member`, and the rest of `(scheme base)`'s Scheme half.

**Multi-chunk mapping is also unsound today**, which matters as soon as a client holds
one map across a program plus its libraries: `SourceMap` stores a single `source_text`,
and `format_context` renders `self.get_line(loc.line)` **without consulting
`loc.source`** (`source_map.rs:60,72`) — a location from another chunk silently renders
the wrong line of the primary text rather than failing. `expansion_records` is likewise
keyed `(u32, u32)` with no source name (`source_map.rs:30`), so macro-chain keys collide
across chunks. Any client spanning chunks needs a map keyed by source name.

Two further facts every location-driven client must design around:

- **Node addresses are unstable.** There is no CpsExpr cache — every top-level form is
  re-transformed (`cps_eval/mod.rs:367`) — so clients key by `(source, line, column)`
  and match against `expr.source`, never by node identity.
- **`SourceMap.locations` is keyed by raw NaN-box bits** and pruned at top-level-form
  boundaries (`prune_freed_locations`) because the GC reuses addresses. Clients store
  `SourceLocation` values, which are self-contained, never those keys.

---

## 7. Performance budget

Structural facts about the step loop, verified by reading it:

- `eval_one_step` **clones the expression** on entry (`step.rs:47`) and again per inner
  iteration for several arms; `CpsExpr::clone` is shallow for `Rc` children but
  deep-copies the `Vec<CpsExpr>` args of `App`/`Apply`/`PrimOp` — an allocation per
  application.
- Every `StepResult` moves three `Vec`s (`prompt_stack`, `dynamic_winds`,
  `exception_handlers`) plus an `Rc<Environment>` and a `ContEnv`.
- A GC safe point runs per driver iteration (`mod.rs:198` → `maybe_collect`), documented
  as a single cached-flag load when nothing is pending.
- A `tracing::debug!` with `std::mem::discriminant` already fires on the first 30 steps
  (`mod.rs:201`).

Against that baseline, `if let Some(hook) = …` is one predictable branch per inner-loop
iteration — the same shape as the VM tracer's accepted `Option<TracerHandle>` check.
This document does **not** put a number on it: the only in-repo profile
(`benchmark_reports/profiling_results.md`) is dated 2024-12-28 and describes the
superseded `HashMap`-based `cont_env` (its top entries are `RawTable` clone/drop at ~37%
combined, `PrimitiveRegistry::apply` at 19.3%, actual arithmetic at 0.05%), so its
ratios cannot be quoted for today's evaluator. The qualitative shape it establishes —
dispatch and state-shuffling dominate, real work is noise — is what the argument rests
on; anyone implementing H1 should re-profile rather than trust either that report or a
projection from it.

Rules for the enabled path, learned from `PATINA_SCOPE_TRACE`'s 14× cautionary tale:

1. Never format, `Display`, or collect environment bindings in a hook's fast path —
   only after deciding to act. The pause path can be arbitrarily slow; the continue path
   must be a few comparisons.
2. Fast-out order for a free-running client: `expr.source.is_none()` → return; then a
   cheap set/bitmap test; mutation tests only on `SetVar`/`Define` (already rare).
3. The `old`-value read for watchpoints (§5.3) costs an extra lookup — gate it on
   whether any watch is armed, not on the hook being installed.
4. A UI-owning client polls the terminal every N events, not every event (gyrus: 2048).
5. No mutex — single thread, `Rc<RefCell<…>>`, borrow discipline per CLAUDE.md and
   §5.2's `try_borrow_mut` rule.

---

## 8. Clients of the hook layer

General practice says a trace layer never stays single-purpose: `sys.settrace` carries
coverage.py and profiling tools as well as `pdb`; JVMTI carries JaCoCo and every Java
profiler as well as JDWP. Clients this layer enables, roughly in order of
effort-to-value:

- **The visual step debugger** — the driving client; designed in
  `PRD/future/VISUAL_DEBUGGER_DESIGN.md`.
- **Post-mortem debugging** (pdb's `pm()`): on an uncaught `Raise`, drop into the
  paused environment instead of just printing the error. Requires the three-site `Raise`
  of §5.3 to be worth anything.
- **Real stack traces in ordinary error output.** An opt-in shadow stack (no UI, no
  pausing — the synthesis is designed in the debugger doc) upgrades
  `format_interpreter_error` from "one caret + macro chain" to a proper backtrace.
- **Procedure tracing** — chibi's `(trace f)` implemented at the interpreter level via
  `Apply`/`Return`, finally replacing the vestigial `DebugConfig` apply trace (§3.3) and
  giving the stubbed Scheme-level debug primitives something real to switch on.
- **Coverage.** Mark sourced `Step` events seen, aggregate per file. Note this is
  gated on §6(c): stdlib coverage — the most interesting question — needs library
  parsing to carry a source map first.
- **Deterministic profiling.** `Apply`/`Return` counts and inclusive time per
  procedure; the Scheme-level complement to Rust-level profiles for Track P (and an
  opportunity to replace the stale profile cited in §7).
- **Backend differential tracing** — the patina-specific one. The VM already emits a
  `StepTracer` stream; a tree-walker hook emitting comparable events makes divergence
  hunting automatic: run both backends, diff the streams, report the first divergent
  event with its source location. The GC differential lane and the `assert_divergence`
  quarantine show how much this project already leans on this style of verification.
- **Step budgets and metering.** "Abort after N steps" gives deterministic
  infinite-loop detection in CI (no wall-clock flakes) and resource limits for
  embedding — subject to the `Abort`/wind caveat in §5.1.
- **Engines.** A step-count-triggered park is the classic Scheme *engines* construct —
  preemption built on fuel — and is the scheduler primitive §9.2's green threads would
  need.
- **Teaching and visualization.** The phase-4 notebook and
  `PRD/future/INTERACTIVE_TUTORIAL_DESIGN.md`'s successor could animate evaluation from
  the same events.
- **Invariant checking.** `SetVar`/`Define` generalize watchpoints into test-lane
  assertions ("this global stays a fixnum").

Two boundaries: anything that must run always-on in production wants sampling, not
deterministic hooks (§7 assumes hooks are a development mode); and expansion-time
tooling is a different phase with its own hook — `MacroTracer` (§3.3) — not this one.

---

## 9. Future: threads

Neither R7RS-small nor patina has threads today, so nothing here is in scope — but the
hook API should not foreclose the future, and the general practice is settled.

### 9.1 What debuggers generally do about threads

- **Trace hooks are per-thread almost everywhere.** `sys.settrace` affects only the
  calling thread; Lua hooks are per coroutine/`lua_State`; JVMTI events carry their
  thread. Consequently *stepping state is per-thread* while *breakpoint and watch tables
  are global*. The companion document's session keeps that split, and it is the property
  to preserve.
- **Two stop models.** *All-stop* (GDB's default, JDWP `SUSPEND_ALL`, debugpy's
  default) versus *non-stop* (GDB non-stop mode, JDWP `SUSPEND_EVENT_THREAD`). DAP
  expresses both (`stopped` carries a `threadId`; `continue` has
  `allThreadsContinued`). All-stop is the right default for a correctness-oriented
  debugger.
- **The *other* threads are stopped cooperatively, at safe points.** An in-process
  debugger cannot preempt its own threads, so runtimes reuse existing yield points: the
  JVM suspends at GC safepoints, CPython via the eval-breaker flag, Go at preemption
  points. **Patina already owns this mechanism** — the GC safe point at the top of every
  driver iteration. A threaded patina would park threads for the debugger the way it
  parks them for a collection: one flag, two clients. (The `is_outermost` interaction of
  §5.2 would need revisiting at the same time.)
- **Threads are what kill the blocking-console model.** `pdb`-in-the-callback works with
  one thread; with several, the paused thread's hook cannot own the terminal while
  others print and hit breakpoints. This is the force that pushed real debuggers to the
  client/server split (debugpy's DAP server thread, JDWP, CDP): layers 1–2 stay in the
  debuggee and the *UI* moves out of the hook. Because the companion document keeps its
  pause predicate pure and its session separate from the UI, that migration changes the
  pause plumbing, not the debugger.

### 9.2 What threads would plausibly look like here

An `Rc`/`RefCell` runtime cannot take OS threads without an `Arc`-and-locks rewrite of
the heap — a runtime project that dwarfs any debugger. The realistic path, as in several
Schemes, is **SRFI-18-style green threads multiplexed on one OS thread** — and on this
machine that is a debugging gift, because a green thread *is* a `StepResult` plus its
dynamic state (§4), held by a scheduler in the shape an observer already snapshots.
Then a thread pane is a walk over the scheduler's thread set; all-stop is the scheduler
declining to schedule; per-thread stepping is keying run state by thread id. The hook
needs one forward-compatible touch **now**: define events as firing on "the current
machine", so adding a thread identity to `DebugEvent` later is an additive field.

---

## 10. Scope: what happens when more backends arrive

The roadmap includes a Cranelift JIT soon, possibly AOT one day. Neither changes this
document's implementation scope (tree-walker fire sites), but they sharpen what the
durable contract is. The scope question decomposes into three pieces with different
lifetimes:

1. **The event vocabulary — backend-neutral, the durable part.** "A sourced statement is
   about to run", "a call entered", "a value returned", "a variable was mutated", "an
   exception is being signalled" are *Scheme-semantics* facts, not tree-walker facts.
   This is what must outlive backends — it is what JVMTI and DAP are: contracts that
   survived multiple VM rewrites underneath them. Consequence: the session/policy layer
   must consume events at this semantic level and never pattern-match on `CpsExpr`
   beyond extracting `SourceLocation`, so a VM lowering (`StepTracer` → `DebugEvent`
   adapter) can drive the same session later. Per the rule of two, the shared Rust
   abstraction is *not* built now: H1's trait is the tree-walker binding, and it moves
   to `patina-runtime` when a second backend client actually appears.
2. **Delivery — per backend, by design.** The tree-walker fires from the trampoline
   (§5.3); the VM already fires from its dispatch loop (`StepTracer`, §3.1); a JIT
   mostly does not fire at all — that is the tier policy's job.
3. **Tier policy — the JIT question.** Industry practice is unanimous: **enabling hooks
   pins execution to a hook-capable tier.** LuaJIT never fires debug hooks from compiled
   traces — hooked code runs in its interpreter; CPython's specializing tiers stand down
   where `sys.monitoring` instrumentation is active (PEP 669 was designed for this); V8
   deoptimizes a function the moment a breakpoint lands in it; Julia's debugger simply
   interprets. The one runtime that did the expensive alternative — HotSpot's
   deopt-on-demand, where compiled frames carry metadata to reconstruct interpreter
   state at any safepoint — represents effort nobody replicates at this scale.

### 10.1 The tier rule for patina

**Hooks attached ⇒ don't tier up.** One flag, consulted at tier-up/dispatch time, à la
PEP 669; code runs in the VM (which already has `StepTracer`) or the tree-walker.
Patina is unusually well-positioned — it already has *two* hook-capable tiers where most
runtimes have one. Attach at top-level-form boundaries only (the same boundary where
`prune_freed_locations` already runs), which sidesteps mid-frame invalidation of
compiled code entirely. Deopt-grade participation of JIT'd frames (per-statement
stepping *inside* compiled code) is **explicitly out of scope** unless a real need
proves it.

Two design notes for the JIT itself, cheap to honor from day one:

- **The event set partitions by what survives compilation.** Mutation and control events
  that funnel through runtime helpers — `SetVar`, `Define`, `Raise`, and `Apply`/`Return`
  at real call boundaries — can keep firing from JIT'd code if the helpers keep firing
  them. Only per-form `Step` truly requires an interpreter tier. So watchpoints and
  break-on-exception could stay live under the JIT; stepping is what pins the tier.
- **Safepoint metadata is designed once.** A Cranelift tier needs GC safepoints and
  stackmaps regardless; if debugging-grade deopt is ever wanted it piggybacks on that
  metadata (the HotSpot pattern). Design the stackmap story for GC; do not promise it
  for debugging.

And a payoff in the other direction: when the JIT is being *built*, this layer is its
differential oracle. VM-vs-JIT event-stream diffing (§8, three-way once the JIT exists)
is the standard way to debug a new backend against a trusted one.

### 10.2 AOT

A different family, and **out of scope for the hook system** — with no interpreter at
runtime there is no hook-capable tier to pin to. The Cranelift-world answer is the
Wasmtime one: emit DWARF so native debuggers (lldb/gdb) debug the compiled code with
source mapping. The pragmatic alternative is the C model: debug builds run the VM tier
with hooks available; release AOT builds debug via DWARF or not at all. Either way the
AOT debug story belongs to the AOT design; the only ask on a future AOT effort is not to
redefine the event vocabulary of point 1.

---

## 11. Work plan (if implemented)

Prerequisites, each an independent, plainly-titled PR:

- **H0-a — CPS source coverage** (§6a): stamp `CoreExpr.source` onto the remaining
  `CpsExpr` kinds in `cps_transform.rs`. Acceptance: `If`/`Set`/`Define` errors carry
  correct carets; chibi suite stays 1226/1226. (~days)
- **H0-b — token spans** (§6b): populate `SourceLocation.length` in the lexer/parser so
  carets and highlights have extent. Acceptance: a multi-character caret in an existing
  error test. (~days; independent of H0-a, required only by the UI client)
- **H0-c — library source maps** (§6c): give library parsing a real source map and
  source name. Acceptance: an error inside `lib/scheme/base/*.scm` reports that file,
  not `<unknown>`. Required by the coverage client and by step-into of stdlib code.
  (~days–1 week; touches library loading, so land it alone)
- **H0-d — event payload plumbing** (§5.3): `source` on `StepResult::ApplyProc`,
  `source`/`old` into `set_var_tagged`, `scopes` on the mutation events. This is the
  bulk of H1's real work and can land ahead of the trait.

Then:

- **H1 — hook layer**: the `DebugHook` trait with its `GcRoots` supertrait, the
  `fire` helper with `try_borrow_mut` re-entrancy handling (§5.2), the seven fire sites
  of §5.3 (including all three `Raise` paths), `Evaluator`'s root forwarding,
  `set_debug_hook`, and `DebugAborted` plumbing. Headless tests: a recording hook over
  fixture programs asserting event order and completeness, including a `dynamic-wind`
  fixture (thunk bodies must appear — verifying the §4b delegation rather than a
  separate site), a `(raise 'x)`/`(error "y")` fixture (all three `Raise` paths), a
  `guard` fixture (`Unwind` payload), and a re-entrancy fixture (a hook that evaluates
  Scheme must not panic or recurse). (~1–2 weeks, given H0-d)

H1 is the go/no-go gate for everything in the companion document.

## 12. Open questions

- **Q1 — the pause contract.** This study recommends the standard in-process
  blocking-hook contract (`pdb`'s; gyrus's): hooks may block indefinitely; the evaluator
  makes no liveness assumption. The alternative — a command-channel/controller split —
  buys nothing while patina is single-threaded and adds `Send` bounds the `Rc`-based
  runtime cannot meet; §9 sketches the migration if threads ever force it.
- **Q5 — how much payload plumbing is worth it up front.** H0-d threads source, previous
  values and scope sets through the evaluator for the debugger's benefit. A thinner
  first cut (fire `Apply` at the `App` arm in `step.rs`, where source is already live,
  and skip `old` values entirely) would get H1 landed sooner at the cost of a weaker
  first debugger. Worth deciding before H0-d starts.

## References

- VM stepper: `docs/VM_STEPPER.md`, `crates/patina-vm/src/tracer.rs`
- Machine internals: `crates/patina-tree-walker/src/eval/cps_eval/{mod,step,application,continuation,environment,exceptions,wind,types,gc_roots}.rs`
- Source plumbing: `crates/patina-ir/src/cps_transform.rs`,
  `crates/patina-frontend/src/{parser/mod.rs,source_map.rs,desugarer/mod.rs}`
- GC interaction: `docs/GC_DESIGN.md`, `crates/patina-core/src/heap/gc.rs`
- Archived pre-CPS design: `PRD/ARCHIVE/phase1_completed/DEBUGGER_HOOK_SYSTEM.md`
- Macro instrumentation: `crates/patina-macros/src/{tracer.rs,error.rs,macro_expander/debug.rs}`,
  `crates/patina-core/src/scope_trace.rs`, `docs/MACRO_SYSTEM.md`
- gyrus hook API: `~/Project/gyrus/crates/gyrus/src/hooks/mod.rs`
- General practice (§2, §9, §10): CPython `sys.settrace`/`bdb`/`pdb` and PEP 669
  (`sys.monitoring`); Ruby `TracePoint`; Lua `debug.sethook`; Racket
  `gui-debugger/annotator`; the Debug Adapter Protocol specification; JDWP suspend
  policies and GDB all-stop/non-stop; LuaJIT and V8 tiering behavior under debug
