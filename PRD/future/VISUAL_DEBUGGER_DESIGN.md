# Visual Step Debugger — Design and Feasibility

**Status:** Feasibility study / design. No implementation is scheduled by this document.
**Scope:** Tree-walker backend only (`--tree-walker`). The debugger *product* — session,
stepping policy, TUI, macro pane, watchpoints.
**Depends on:** `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` — the evaluation hook layer
(patina's `sys.settrace`) this debugger is a client of. That document owns the machine
model, the `DebugHook`/`DebugEvent` API, the source-coverage prerequisites (H0-a token
coverage, H0-b spans, H0-c library maps), the event-payload plumbing (H0-d), and the
performance budget. In the canonical stack for interpreted-language debuggers (hook
layer → policy library → UI → wire protocol; hook doc §2), this document is layers
2–4 — patina's `bdb` + `pdb`.
**Date:** 2026-08-31. File:line references were checked against commit `13e68a40`.

---

## 1. Summary and verdict

The goal: an interactive step debugger for R7RS programs. Load a program, show the
source with the expression currently under evaluation visually distinguished from the
rest, show live values in a side pane, show how macro uses expanded, and support
watchpoints (pause when a variable changes, or changes to a particular value).

**Verdict: feasible on top of the hook layer, with four client-side designs that need
care.** Each of these looked free on a first pass and is not:

1. **Stepping cannot be defined by stack depth** (§4). Proper tail calls mean a frame
   is *replaced*, never popped — so a depth-based "step out" would never fire for a
   tail-recursive procedure. The design below keys every mode on **continuation
   identity** instead, which requires continuations to carry a stable id.
2. **The session must own its own `SourceMap` and drive its own parse/eval loop** (§3).
   `eval_program_with_source_name` builds the map internally and returns it only when
   the whole program is done — at every pause the session would hold nothing.
3. **The locals pane cannot use `Environment::bindings()`** (§5). That method reads
   only the simple binding map; lambda parameters and `let` bindings normally land in
   the *scoped* table, so an ordinary paused frame would render empty.
4. **Watchpoints see macro-generated writes** (§7). `letrec`, `letrec*`, named `let`
   and `do` all expand to `(set! var init)` on the user's own identifier, so a watch on
   a loop variable fires on writes the user never typed and no gensym filter can
   remove.

The macro pane also starts from less than it appears: `MacroTracer` is a fire point
with the wrong payload (§6), not an 80%-built vehicle.

---

## 2. Prior art: the gyrus bf debugger (`~/Project/gyrus`)

The reference for the desired *experience*, and a working ratatui codebase to crib
from. Architecturally it is a compact instance of the canonical stack — hook trait,
pure policy, blocking console — not a separate model. What it teaches, and what
transfers:

- **Pause = the hook blocks, one thread.** "Paused" is not a state machine — the hook
  does not return: it redraws the TUI and blocks in `event::read()` (in the debugger's
  `ui.rs`, called from the hook) until the user resumes. This is also how CPython's
  `pdb` works inside the `sys.settrace` callback. **Transfers directly**; it is the
  contract the hook doc's Q1 fixes.
- **Pure widget crate.** `gyrus-tui` holds ratatui widgets with zero application logic;
  the debugger binary composes them; widgets are tested headlessly against ratatui's
  `TestBackend`. **Transfers as a pattern** (see Q3).
- **Control state as data.** One small enum
  `RunState { Step, Continue, RunTo(usize), Leave { start, end } }` plus one pure,
  terminal-free predicate `should_pause(run, at_breakpoint, index, starving_for_input)`
  (`~/Project/gyrus/crates/gyrus-debug/src/state.rs:14,426`). **Transfers.**
- **Step-over/step-out are *not* depth-based** — they are instruction ranges
  (`RunState::Leave { start, end }`), because at a loop head the depth before the next
  iteration and after the loop ends are identical (`state.rs:21–28`). This is the most
  important thing gyrus teaches, and §4 honors it: patina's equivalent of a range is a
  continuation identity.
- **Hot-path discipline.** The stopping rule is compiled outside the shared session
  (breakpoints as a dense bitmap, revision counters to re-sync) because locking a mutex
  per instruction measured ~20% of per-instruction cost; free running touches the UI
  only every 2048 instructions. **The lesson transfers** even though patina needs no
  mutex (hook doc §7).
- **Stop reason recorded at the decision point** (`StopReason { Stepped, Breakpoint,
  OutputWatch, NeedsInput }`), never reconstructed later. **Transfers.**
- **The macro warning.** gyrus *refuses* to debug `.bfm` macro-preprocessed source,
  because its symbol mapping is many-to-one. Patina cannot refuse — `let`, `cond`, `do`,
  `and`, `or` are all macros here — so the macro story (§6) is load-bearing.
- **The other model on the same API.** `gyrus-tutorial` drives the same hook trait in
  record-and-scrub mode. For patina that does not transfer cheaply — §8.

---

## 3. The debug session

```
patina-repl --tree-walker --debug <script.scm>       (flag or separate binary — Q2)
        │
  DebugSession                        new; owns:
    ├─ Interpreter<TreeWalker>          for the evaluator and global env
    ├─ Rc<RefCell<SourceMap>>           created and owned by the session (see below)
    ├─ Session state                    RunState, breakpoints, watchpoints, shadow
    │                                   stack, stop reason, macro retention table
    └─ UI                               ratatui panes (§5) or line-mode stepper (D1)
```

**The session drives its own parse/eval loop.** It cannot call
`eval_program_with_source_name`: that method creates the `SourceMap` internally and
returns it only on completion (`patina-interpreter/src/lib.rs:491–528`), so during
every pause — which is *inside* that call — the session would hold nothing, and the map
is the only thing that turns a `SourceLocation` into a displayable line
(`SourceMap::format_context`, `patina-frontend/src/source_map.rs:72`). Instead the
session owns an `Rc<RefCell<SourceMap>>`, parses with `Parser::new_with_source_map`,
and evaluates form by form through `TreeWalker::eval_with_source_map`
(`crates/patina-tree-walker/src/backend.rs:98`) — the same shape as the existing
runner, minus the discard. It must also call `prune_freed_locations` at each form
boundary, as the existing drivers do.

**`RunState` and `should_pause` are pure data + a pure function** — the policy layer,
patina's `bdb`, kept terminal-free as gyrus does. The modes are defined in §4, keyed on
frames and continuations rather than depth.

**Breakpoints are keyed by `(source, line, column)`** — the full triple, not
`(line, col)`: a program plus its libraries spans several source names, and CpsExpr
node addresses are unstable across top-level forms (hook doc §6). Match against
`expr.source` at `Step` events; snap a user's requested line to the first sourced node
at-or-after it, keeping gyrus's two-snapping-rules distinction (cursor snapping ≠
typed-location snapping). Until hook doc H0-a lands, only applications are sourced, so
breakpoints land on call sites only.

**Its own driver loop.** `run_repl_loop`'s callback is `FnMut(&str) -> Option<String>`
(`crates/patina-repl/src/repl/mod.rs:123`) with no way to express "suspended", so the
debugger gets its own loop — which also sidesteps rustyline-vs-crossterm terminal
ownership.

**Program I/O** is redirected at session start (gyrus's `DebugOutput`/`DebugInput`
adapters): the current output/input ports point at buffers the output pane renders,
so a `display` cannot corrupt the TUI. Patina routes ports through the heap's port
machinery, so this is port substitution, not an evaluator change.

**Re-entrant evaluation** (`p <expr>`, conditional breakpoints) runs through the
session's own evaluator against the paused environment. Two constraints from the hook
layer: firing is suppressed while inside the hook by the dispatcher's `try_borrow_mut`
guard (hook doc §5.2), so the debugger cannot recurse into its own breakpoints; and no
GC runs for the duration of a pause, because the re-entrant `eval_in_env` is not the
outermost extent (hook doc §5.2). A pause that evaluates a few expressions is fine; a
client that evaluates in a loop while paused would grow the heap.

**Backend portability guardrail** (hook doc §10): the session and everything above it
consume events at the *semantic* level — statement/call/return/mutation/raise — and
never pattern-match on `CpsExpr` beyond extracting `SourceLocation` (plus an opaque
continuation id for the shadow stack). Kept to that discipline, the same session can
later be driven by a VM `StepTracer` adapter, and survives a Cranelift JIT via the tier
rule, with no changes to policy or UI.

---

## 4. Stepping semantics: frames and continuations, never depth

There is no call stack to display. `ContEnv` is a lexical map of continuation bindings
(reset to just `cont_param` on lambda entry,
`crates/patina-tree-walker/src/eval/cps_eval/application.rs:127–131`), not a dynamic
frame chain; `CpsContinuation` (`patina-core/src/continuation.rs:24`) captures what a
*continuation* needs, not what a *backtrace* needs. The session synthesizes a **shadow
stack** from hook events.

### 4.1 Continuation identity is a prerequisite

Every rule below compares continuations. **Comparing `ContValue::Local` by `Rc::ptr_eq`
on its `body` does not work**: `body` is an `Rc` clone of the *static* `LetCont` node
(`patina-core/src/cps_expr.rs:272`), re-cloned per activation at `step.rs:131`, so two
different activations of the same call site compare equal — under which the tail-call
rule below would collapse ordinary non-tail recursion (every inner `(fib (- n 1))`
would look like a tail call and the stack pane would pin at depth 2). Comparing `env`
pointers as well is closer but still incidental.

**Required work item D0: stamp a `u64` id on continuations at creation**, mirroring
`DynamicWindRecord.id`, and expose it through the `Return`/`Apply` events. Nothing in
the runtime has one today (`CpsContinuation`, `continuation.rs:21–63`). This is small
but it is a prerequisite, not an optional fallback.

### 4.2 The shadow stack

- **Push** on `Apply` of a `CpsLambda`: a frame recording `{ frame_id, return_cont_id
  (the `cont` the call was given), call-site SourceLocation, env_id, name }`.
  "Name" comes from the call site's `Var`, not the value — `Procedure::CpsLambda` has
  no name field (`patina-core/src/procedure.rs:57`), so an anonymous or
  computed-operator call is legitimately unnamed.
- **Replace, not push, on a tail call**: if the new application's `cont` id equals the
  top frame's `return_cont_id`, the callee inherits the caller's continuation — a tail
  call — so replace the top frame. This keeps the stack pane honest about proper tail
  calls (a self-tail-recursive loop shows one frame, matching R7RS) and bounds the
  shadow stack.
- **Pop** on `Return` into a frame's `return_cont_id`.
- **Invalidate** on `Unwind` (§4.4).

### 4.3 The modes

Because a tail call *replaces* a frame, a replaced frame is never popped — so any rule
phrased in terms of stack depth breaks for the most common shape in Scheme. All four
modes are therefore keyed on identity:

| Mode | Rule |
|---|---|
| step (into) | pause at the next `Step` event that carries a `SourceLocation` |
| step over | pause at the next sourced `Step` whose top frame is the recorded `frame_id`, **or** when the recorded frame's `return_cont_id` is invoked (it finished, possibly via a tail chain) |
| step out | pause when the recorded frame's `return_cont_id` is invoked |
| continue | pause on breakpoint / watch / `Raise` |

Step-out of a procedure that ends in a tail call therefore pauses when the *whole tail
chain* returns. That is not a bug to be worked around: in Scheme the tail-called
procedure genuinely is the continuation of the caller, and the status bar should say so
(`stepped out of fib → the tail chain returned`) rather than pretend a frame boundary
exists where the semantics say none does.

"Next *sourced* step" is what makes stepping feel statement-level rather than
CPS-node-level: unsourced administrative nodes are skipped silently. It is also the
free-run fast path — `expr.source.is_none()` returns immediately. Note the granularity
ceiling from the hook layer: trivial operands never fire `Step` at all (hook doc §4d),
and `Var`/`Literal` nodes carry no source even after H0-a (hook doc §6a), so the finest
achievable highlight is the enclosing form.

### 4.4 Non-local exits are ordinary, not exotic

`Unwind` fires whenever a captured continuation is invoked — and R7RS `guard` is
implemented as `call/cc` plus `with-exception-handler`
(`lib/scheme/base/exceptions.scm:13–41`), so **every caught exception in ordinary code
raises `Unwind`**, not just deliberate `call/cc` gymnastics. Truncating the shadow
stack there would blank the stack pane at exactly the moment a user most wants a
backtrace.

The design instead uses the event's payload (hook doc §5.3: the escape carries
`(value, target continuation)`): on `Unwind`, **pop frames until the frame whose
`return_cont_id` matches the target**, which is the correct answer for the common
`guard`/early-return shape, and only fall back to a `[stack unwound]` marker when the
target matches no live frame (re-entering a continuation captured in an extent that has
already returned — the genuinely exotic case). Re-entering a captured continuation also
resets `prompt_stack` and `exception_handlers` to empty (hook doc §4c), so the debugger
must not assume those recover.

`dynamic-wind` thunks appear as ordinary applies, since the wind trampoline delegates
to the hooked step functions (hook doc §4b). Delimited continuations are **out of
scope**: `make_delimited_continuation` is an explicit TODO stub
(`cps_eval/wind.rs:124`).

One teardown caveat inherited from the hook layer: `DebugDecision::Abort` unwinds
without running `dynamic-wind` after-thunks (hook doc §5.1), so quit/restart leaves
`parameterize` swaps and port redirections in place. The session must run
`run_wind_handlers` itself on restart, or tear down and rebuild the interpreter.

---

## 5. UI design

Ratatui + crossterm, consistent with the phase-4 notebook plan
(`PRD/future/phase4/TUI_IMPLEMENTATION.md`) and with gyrus. Layout:

```
┌─ fib.scm ────────────────────────────────┬─ Locals ────────────────┐
│    1  (define (fib n)                    │ n        2              │
│    2    (if (< n 2)                      │ ─ parent ───────────────│
│ ●  3        n                            │ fib      #<procedure>   │
│ ▶  4        (+ (fib (- n 1))             ├─ Watch ─────────────────┤
│    5           (fib (- n 2)))))          │ counter  12   (any)     │
│    6  (display (fib 6))                  ├─ Stack ─────────────────┤
│                                          │ ▶ fib        fib.scm:4  │
│                                          │   fib        fib.scm:4  │
│                                          │   (toplevel) fib.scm:6  │
├─ Macros ─────────────────────────────────┴─────────────────────────┤
│ fib.scm:2  if ← (my-if …)   [e to expand]                          │
├─ Output ───────────────────────────────────────────────────────────┤
│                                                                    │
├────────────────────────────────────────────────────────────────────┤
│ PAUSED · breakpoint fib.scm:3 │ s step  n over  o out  c cont      │
│ value ⇒ 1                     │ b break  w watch  p print  q quit  │
└────────────────────────────────────────────────────────────────────┘
```

- **Source pane.** The stated scheme: the expression under evaluation is distinguished
  from all other statements, which render dimmed. Implementation: dim the document;
  paint `[line, col .. col + length)` of the current `SourceLocation` in the highlight
  style (REVERSED|BOLD, per gyrus's per-character `char_style`). **This depends on hook
  doc H0-b**: `SourceLocation.length` is `None` everywhere today (`with_length` has no
  non-test callers), so without span recording the highlight degrades to a
  single-character caret or a whole-line highlight. Gutter: `▶` current, `●`
  breakpoint, both `▶●`.
- **Locals pane** (the "value side pane"). **Do not use `Environment::bindings()`** —
  it iterates only the simple map (`patina-core/src/environment.rs:772`), while lambda
  parameters bind through `define_with_scopes`/`define_scoped_definition` whenever the
  lambda's `binding_scopes` are non-empty (`cps_eval/application.rs:87–115`), which is
  the normal case for desugared lambdas. Since `let`, `let*` and named `let` all expand
  to lambdas, a pane built on `bindings()` would show no parameters and no let-bound
  variables — only globals. `get_all_names()` (`environment.rs:752`) unions the simple
  and scoped tables, but it also recurses into parents (`:759`), so a *per-frame* pane
  needs either a local-only variant of it or a diff against the parent's name set —
  a small addition to `Environment`, and the one place this pane touches core. Read
  values with `get`/`get_with_scopes`, walk frames with `parent()`
  (`environment.rs:785`). Render with the existing `format_display_tagged` datum
  writer; filter compiler-introduced names by prefix; use `env_id()`
  (`environment.rs:208`, process-unique, never reused) as the stable frame identity.
- **Return-value flash.** On `Return` events, show the value flowing into the
  continuation in the status area ("value ⇒ 1") — in a CPS machine this is the single
  most informative signal while stepping, and it costs nothing.
- **Stack pane** — the shadow stack of §4, with anonymous rows where the call site
  gives no name.
- **Macros pane** — §6.
- Free-run cadence: poll the keyboard and redraw every N events (gyrus: 2048).
- **Multi-chunk source.** A program plus its libraries spans several source names, and
  `SourceMap` today holds one `source_text` whose `format_context` ignores
  `loc.source` (hook doc §6) — it would render the wrong line rather than fail. Until
  that is fixed, the source pane must key its own document store by source name and
  never rely on `format_context` for a chunk other than the primary one. Stepping into
  library code additionally needs hook doc H0-c.

Phase D1 ships **without** any of this: a line-mode stepper (print caret context, read
a command) proves the session layer with no terminal management, and remains useful
over ssh and in tests.

---

## 6. The macro expansion pane

The requirement: when the program uses macros, show how the definitions expanded.

What exists today:

- Expansion happens *inside the desugarer during eval*
  (`patina-frontend/src/desugarer/mod.rs:1140–1186`): `expand_macro_with_scope` produces
  the expanded `TaggedValue`, which is stamped with the **call-site** location
  (`stamp_expansion_source`, `mod.rs:110`), recorded by *name only*
  (`SourceMap::record_expansion` → `expansion_records`, keyed `(line, col)`), and then
  consumed by the recursive `desugar_tagged` call and **dropped**. No expanded tree
  survives to runtime.
- `MacroTracer` fires on every expansion — but its payload is
  `ExpansionStep { macro_name, rule_index, total_rules, input: String }`
  (`patina-macros/src/error.rs:10`), and the recording site stores a *pre-rendered
  string of the input args* (`macro_expander/debug.rs:245`); the expanded form is in
  scope at the call site and is never passed to it (`macro_expander/mod.rs:110–131`).

So the tracer supplies a fire point with the wrong payload — not, as an earlier draft of
this study claimed, an 80%-built vehicle. Of what the pane needs (post-form, datum
retention, GC rooting, keying, caps) it supplies none.

Design: add a retention table to the debug session, populated at the desugarer's
existing expansion site (beside `record_expansion`, which already has both forms and the
call-site location in hand), storing the **(pre-form, post-form) datum pair** keyed by
`(source, line, column)`. Render with the datum writer when the cursor sits on a call
site whose `expansion_records` chain is non-empty; chained expansions display as the
recorded sequence, outermost first — the data the "macro expansion chain: a → b" error
formatting already proves out.

Four constraints:

- **GC.** Retained datums are heap values; the table must be traced (hook doc §5.2's
  `DebugHook: GcRoots` seam) and bounded (per-site and global caps).
- **Keying.** `expansion_records` is keyed `(u32, u32)` with no source name
  (`source_map.rs:30`), so keys collide across chunks; the retention table must key on
  the full triple even though the existing map does not.
- **Depth cap.** `stamp_expansion_source` stops after `MAX_DEPTH = 64` cons cells
  (`desugarer/mod.rs:110`), so deep expansion bodies carry no location at all and
  cannot be attributed to a call site.
- **Granularity.** Everything inside an expansion carries the call site's location by
  design, so the pane answers *"what did this call site become"* — the stated
  requirement — but **stepping to template positions inside a macro definition is out
  of scope**. That needs per-template source tracking through expansion
  (SRFI-211-style syntax-object locations), a macro-system change far beyond a
  debugger. gyrus's `.bfm` refusal is the precedent; this design draws the same line
  one level higher, where it is still useful.

---

## 7. Watchpoints

The requirement: stop when some variable changes, or changes to a particular value.

- **Events**: `SetVar` (from `set_var_tagged`, the single choke point for both `set!`
  resolution paths) and `Define`. Both require hook doc H0-d, which threads the
  previous value and the source location — `Environment::set` overwrites without
  reading (`patina-core/src/environment.rs:271`), so `old` costs a deliberate extra
  lookup, gated on a watch being armed.
- **Identity is name + scopes, not name.** `CpsExprKind::Set` pairs the symbol with a
  `ScopeSet` (`cps_expr.rs:327`) and `set_var_tagged` branches on `scopes.is_empty()`
  to choose `env.set` vs `env.set_with_scopes` (`cps_eval/environment.rs:98`). A watch
  keyed on a bare name cannot distinguish two macro-introduced bindings that share it,
  and would over- or under-fire. Spec:
  `Watch { name, scopes: Option<ScopeSet>, mode: AnyChange | Becomes(TaggedValue),
  scope: Global | Env(u64) }`.
- **Macro-generated writes are visible, and cannot be filtered by name.** `letrec` and
  `letrec*` expand to `(set! var init)` on the user's own variable
  (`lib/scheme/base/binding.scm:29,36`), named `let` expands through `letrec`, and `do`
  emits `set!` per step variable. So watching a loop variable or any `letrec`-bound
  name fires on writes the user never typed, under the user's own identifier. There is
  no gensym prefix to exclude. Two honest responses, both cheap: label each hit with
  its source location and the macro chain from `expansion_records` (so the user sees
  "set from `do` at line 12"), and offer a filter on "writes attributed to a macro
  expansion" rather than pretending they do not occur. Arguably the variable *is*
  changing, so firing is correct — but it must be explained, not hidden.
- **Retained old/target values are GC roots** (§6's seam).
- **Conditional breakpoints** evaluate a Scheme predicate in the paused environment
  through the session's evaluator, subject to §3's re-entrancy and no-GC-while-paused
  constraints. Deferred to D4.
- The VM tracer already has a watchpoint trace-event kind — naming and semantics should
  match where they overlap, so the backends' debug stories converge.

Comparison for `Becomes` uses `Heap::values_eqv` (`&self`, no allocation), matching
Scheme's `eqv?`; an `equal?` mode can follow.

Even with the macro noise, this remains cheaper than mainstream practice: `sys.settrace`
has no mutation event, so pdb-family watchpoints re-evaluate the watched expression on
every line event, and GDB's software watchpoints single-step the whole program to
compare after each step. Patina at least has a single mutation choke point to hook.

---

## 8. Why not time travel (yet)

Record-and-scrub (the gyrus-tutorial model) does not transfer cheaply: patina
environments are `Rc<RefCell<…>>` and mutable in place. Holding `Rc<Environment>` per
recorded frame does **not** snapshot values — a later `set!` mutates the shared slots,
and every "historical" frame silently shows the final state. An honest recorder must
copy reachable bindings per frame (expensive, unbounded) or patina must grow persistent
environments (a runtime project). A cheap middle ground — record only
`(SourceLocation, event kind, shallow locals-of-interest)` for a scrollable *trace log*
without state reconstruction — is worthwhile and listed in D4.

---

## 9. Feasibility matrix

Rows marked *(hook doc)* are owned by `TREE_WALKER_HOOK_SYSTEM.md`.

| Feature | Exists today | Gap | Effort | Risk |
|---|---|---|---|---|
| Hook layer *(hook doc H1)* | Trampoline surfaces full machine state per step | Trait, 7 fire sites incl. 3 `Raise` paths, re-entrancy, root seam | M | Low |
| Event payloads *(hook doc H0-d)* | — | source on `ApplyProc`, old value + source into `set_var_tagged`, scopes | M | Low |
| Statement highlight, sub-line *(hook doc H0-a/H0-b)* | `format_context` uses `length`; desugarer stamps pairs | CPS transform stamps 3 kinds; `length` never populated; atoms unsourced | M | Low (mechanical, wide diff) |
| Session + own source map | `eval_with_source_map`, `Parser::new_with_source_map` | Session drives its own loop; multi-chunk map keying | S | Low |
| Values side pane | `get_all_names`, `get_with_scopes`, `parent`, `env_id`, datum writer | Union walk (not `bindings()`), gensym filtering | S | Low |
| Return-value display | `Return` carries the value | — | S | Low |
| Breakpoints | — | `(source,line,col)` set + snapping; call sites only until H0-a | S | Low |
| Watch: any-change / becomes | Single `set!` choke point; `values_eqv` | H0-d payload; scope-set identity; macro-write labelling | M | Medium (semantics, not code) |
| Stack pane + step over/out | Nothing (no call stack) | Continuation ids (D0); shadow stack; identity-keyed modes | M | Medium |
| Macro expansion pane | Call-site chain names; expansion site has both forms | New retention table (tracer payload unusable), GC-rooted, capped, triple-keyed | M | Medium |
| Break on exception | `Raise` (3 sites, hook doc) | consumer only | S | Low |
| Step into stdlib | — | hook doc H0-c (library source maps) | M | Low |
| Conditional breakpoints | Re-entrant eval | No-GC-while-paused caveat | S | Low (deferred) |
| Time travel / scrub | gyrus-tutorial pattern | Mutable envs break naive snapshots | L | **High** (excluded) |
| Template-position macro stepping | — | Per-template source through expansion | XL | **Excluded** |
| Delimited-continuation awareness | `Prompt` frames | Capture is a TODO stub | — | **Excluded** |

---

## 10. Phasing

Depends on the hook doc's H0-a/H0-d and **H1** landing first; H1 is the go/no-go gate.
H0-b (spans) is required by D2, H0-c (library maps) by step-into of stdlib code.

- **D0 — continuation ids.** Stamp a `u64` on continuations at creation and expose it
  on `Apply`/`Return` events (§4.1). Small, but every stepping mode depends on it.
- **D1 — session + CLI stepper.** `DebugSession` (own source map and eval loop,
  RunState, `should_pause`, breakpoints, watchpoints, shadow stack, stop reason) and a
  line-mode stepper. Headless tests script commands and assert stop points — including
  a tail-recursive fixture (step-out must terminate), a `guard` fixture (stack survives
  a caught exception), and a `do`-loop watch fixture (macro-generated writes are
  labelled, not hidden). (~2 weeks)
- **D2 — ratatui TUI.** Source pane with the dim/highlight scheme, locals, watch,
  stack, output panes, port redirection, status bar. Widget code kept free of session
  logic; headless `TestBackend` tests. (~1–2 weeks)
- **D3 — macro pane.** Retention at the desugarer's expansion site + rendering; `e` to
  expand at cursor. (~1–1.5 weeks, more than the earlier estimate now that the tracer
  payload is known unusable)
- **D4 — extras:** post-mortem entry on uncaught `Raise` (pdb's `pm()`); conditional
  breakpoints; trace-log lite (§8); DAP server (the session/`should_pause` split is
  DAP-shaped already); REPL-chunk debugging.

---

## 11. Open questions

- **Q2 — delivery vehicle.** `patina --tree-walker --debug script.scm` (a flag, like
  `--trace`) vs. a separate `patina-debug` binary crate (gyrus keeps the debugger out
  of the main CLI). The flag is less machinery; the separate crate keeps
  ratatui/crossterm out of patina-repl's dependencies until D2 forces the choice.
- **Q3 — widget reuse.** Copy the relevant `gyrus-tui` source-pane/layout code
  (divergent evolution, no coupling) vs. publishing `gyrus-tui` as a shared dependency.
  Cross-repo coupling for a few hundred lines is probably not worth it, but it is the
  user's crate and the user's call.
- **Q4 — breakpoint UX.** Line-only (`b 12`) vs. line:column, and whether a
  `(debug-break)` form (the Scheme analogue of gyrus's in-source `@` markers) is worth
  having — it falls out of D1 for free.
- **Q6 — macro-write watch default.** Do watchpoints fire on macro-generated `set!`s by
  default (correct, noisy) or are they filtered by default (quiet, hides real
  mutation)? §7 argues for firing with attribution; the opposite default is defensible
  for a teaching-oriented tool.

(Q1 — the pause contract — and Q5 — how much payload plumbing is worth up front — live
in the hook doc, where the API they constrain lives.)

## References

- The hook layer this builds on: `PRD/future/TREE_WALKER_HOOK_SYSTEM.md`
- gyrus: `~/Project/gyrus/crates/{gyrus-debug,gyrus-tui,gyrus-tutorial}`,
  `~/Project/gyrus/docs/debugger.md` (esp. its explicit non-features list)
- TUI direction: `PRD/future/phase4/TUI_IMPLEMENTATION.md` (ratatui + crossterm)
- Session plumbing: `crates/patina-interpreter/src/lib.rs`,
  `crates/patina-tree-walker/src/backend.rs`, `crates/patina-repl/src/repl/mod.rs`
- Environment inspection: `crates/patina-core/src/environment.rs`
  (`get_all_names`, `get_with_scopes`, `parent`, `env_id`)
- Stepping internals: `crates/patina-tree-walker/src/eval/cps_eval/{application,continuation}.rs`,
  `crates/patina-core/src/{continuation.rs,cont_value.rs,procedure.rs}`
- Macro pane inputs: `crates/patina-macros/src/{tracer.rs,error.rs}`,
  `crates/patina-frontend/src/{source_map.rs,desugarer/mod.rs}`
- Macro-expanded mutation: `lib/scheme/base/binding.scm`, `lib/scheme/base/exceptions.scm`
- VM analogue: `docs/VM_STEPPER.md`, `crates/patina-vm/src/tracer.rs`
