# Visual Step Debugger — Design and Feasibility

**Status:** Feasibility study / design. No implementation is scheduled by this document.
**Scope:** Tree-walker backend only (`--tree-walker`). The debugger *product* — session,
stepping policy, TUI, macro pane, watchpoints.
**Depends on:** `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` — the evaluation hook layer
(patina's `sys.settrace`) this debugger is a client of. That document owns the machine
model, the `DebugHook`/`DebugEvent` API, the source-coverage prerequisite (H0-a), the
shared-dispatch refactor (H0-b), and the performance budget. In the canonical stack for
interpreted-language debuggers (hook layer → policy library → UI → wire protocol; see
hook doc §2), this document is layers 2–4 — patina's `bdb` + `pdb`.
**Date:** 2026-08-31 (file:line references checked against commit `13e68a40`).

---

## 1. Summary and verdict

The goal: an interactive step debugger for R7RS programs. Load a program, show the
source with the expression currently under evaluation visually distinguished from the
rest, show live values in a side pane, show how macro uses expanded, and support
watchpoints (pause when a variable changes, or changes to a particular value).

**Verdict: feasible on top of the hook layer.** With the hook doc's H0/H1 in place,
everything here is client-side work. The two genuine gaps owned by this document:

1. **No call stack exists** — the CPS machine has no frame chain; the stack pane and
   step-over/step-out require a shadow stack maintained by the session, with a
   tail-call rule so it doesn't grow where Scheme guarantees it must not. (§4)
2. **Macro expansions are discarded** — the expanded tree is consumed by the desugarer
   and dropped; only macro *names* per call site survive. The expansion pane needs new
   (bounded) retention, for which `MacroTracer` is an existing 80%-built vehicle. (§6)

Everything else — pausing, environment inspection, watchpoints, breakpoints,
break-on-exception — consumes events and data that the hook layer and existing code
already provide.

---

## 2. Prior art: the gyrus bf debugger (`~/Project/gyrus`)

The reference for the desired *experience*, and a working ratatui codebase to crib
from. Architecturally it is a compact instance of the canonical stack — hook trait,
pure policy, blocking console — not a separate model. What it teaches, and what
transfers:

- **Pause = the hook blocks, one thread.** "Paused" is not a state machine — the hook
  simply does not return: it redraws the TUI and blocks in `event::read()` until the
  user resumes (`~/Project/gyrus/crates/gyrus-debug/src/hook.rs`). This is also
  exactly how CPython's `pdb` works inside the `sys.settrace` callback. **Transfers
  directly**; it is the contract the hook doc's Q1 fixes.
- **Pure widget crate.** `gyrus-tui` holds ratatui widgets (source pane, memory pane,
  watch list, status bar, help overlay) with zero application logic; the debugger
  binary composes them. Widgets are tested headlessly against ratatui's `TestBackend`.
  **Transfers as a pattern** (patina would not depend on the crate — see Q3 — but the
  source-pane highlighting and layout code is a working reference).
- **Control state as data.** One small enum
  `RunState { Step, Continue, RunTo(usize), Leave { start, end } }` plus one pure,
  terminal-free predicate `should_pause(run, at_breakpoint, index, starving_for_input)`
  (`~/Project/gyrus/crates/gyrus-debug/src/state.rs:14,426`). Step-over/step-out are
  expressed as *instruction ranges*, not loop depth, because depth is ambiguous at a
  loop head. **Transfers**, with the range notion replaced by continuation identity
  (§4).
- **Hot-path discipline.** The stopping rule is compiled outside the shared session
  (breakpoints as a dense `Vec<bool>` bitmap, revision counters to re-sync) because
  locking a mutex per instruction measured ~20% of per-instruction cost; during free
  running the hook touches the UI only every 2048 instructions. **The lesson
  transfers** even though patina needs no mutex (hook doc §7).
- **Stop reason recorded at the decision point** (`StopReason { Stepped, Breakpoint,
  OutputWatch, NeedsInput }`), never reconstructed later — it drives the header label
  and is the difference between a debugger that feels precise and one that feels
  haunted. **Transfers.**
- **The macro warning.** gyrus *refuses* to debug `.bfm` macro-preprocessed source,
  because its symbol mapping is many-to-one and carries no loop metadata. Patina
  cannot refuse — `let`, `cond`, `do`, `and`, `or` are all macros here — so the macro
  story (§6) is load-bearing, not optional.
- **The other model on the same API.** `gyrus-tutorial` drives the *same* hook trait
  in record-and-scrub mode: append a small `Frame` per step, let the user scrub
  backward instantly. For bf this is cheap (a tape copy per frame, capped). For patina
  it is **not** cheap in the obvious way — §8 explains why Scheme's mutable
  environments break the naive version — so live stepping is the default and
  record-and-scrub is a scoped follow-on.

---

## 3. The debug session

Layering (each layer usable without the ones above it):

```
patina-repl --tree-walker --debug <script.scm>       (flag or separate binary — Q2)
        │
  DebugSession                        new; owns the pieces the hook needs:
    ├─ Interpreter<TreeWalker>          eval_program_with_source_name(…) — this method
    ├─ Rc<RefCell<SourceMap>>           already returns the SourceMap the UI needs
    │                                   (patina-interpreter/src/lib.rs:491); today it is
    │                                   created per call and thrown away by the runner
    ├─ Session state                    RunState, breakpoints, watchpoints, shadow stack,
    │                                   stop reason, macro trace ring
    └─ UI                               ratatui panes (§5) or plain CLI stepper (phase D1)
```

- The session implements `DebugHook`, installs itself via
  `TreeWalker::set_debug_hook`, then calls `eval_program_with_source_name` and holds
  the returned `SourceMap` — that map is the only thing that turns a `SourceLocation`
  into a displayable line (`SourceMap::format_context`,
  `patina-frontend/src/source_map.rs:72`).
- **Backend portability guardrail** (hook doc §10): the session and everything above
  it consume events at the *semantic* level — statement/call/return/mutation/raise —
  and never pattern-match on `CpsExpr` beyond extracting `SourceLocation` (and, for
  the shadow stack, an opaque continuation identity). Kept to that discipline, the
  same session can later be driven by a VM `StepTracer` adapter, and survives a
  Cranelift JIT via the tier rule (hooks attached ⇒ code runs on a hook-capable
  tier), with no changes to policy or UI.
- **`RunState` and `should_pause` are pure data + a pure function** — this is the
  debugger-policy layer, patina's `bdb`, kept terminal-free as gyrus also does:

  ```rust
  enum RunState {
      Step,                       // pause at next sourced Step event
      Continue,                   // run to breakpoint/watch
      StepOver { frame: FrameId },// pause at next sourced Step with shadow-stack depth
                                  //   <= depth(frame)   (§4)
      StepOut  { frame: FrameId },// pause when `frame` is popped
  }
  ```

  `should_pause(run, event, stack, breakpoints, watch_hit) -> Option<StopReason>` is
  terminal-free and unit-testable without a TTY.
- **Breakpoints are keyed by `(file, line, column)`, never by node address** — CpsExpr
  node addresses are unstable across top-level forms (hook doc §6). Match against
  `expr.source` at `Step` events via an `FxHashSet<(line, col)>` (or a per-file line
  bitmap, per gyrus); snap a user's requested line to the first sourced node
  at-or-after it, reusing gyrus's two-snapping-rules lesson (cursor snapping ≠
  typed-location snapping).
- The existing REPL loop cannot host this: `run_repl_loop`'s callback is
  `FnMut(&str) -> Option<String>` (`patina-repl/src/repl/mod.rs:123`) with no way to
  express "suspended". The debugger gets its own driver loop (as gyrus-debug has its
  own `main.rs`), which also sidesteps rustyline-vs-crossterm terminal ownership.
- Program I/O: a program that `display`s while the TUI owns the terminal would corrupt
  it. Same solution as gyrus (`DebugOutput`/`DebugInput` adapters): redirect the
  current output/input ports to buffers rendered in an output pane. Patina already
  routes ports through the heap's port machinery, so this is a port substitution at
  session start, not an evaluator change.
- The re-entrant `p <expr>` command evaluates a Scheme expression in the paused
  environment through the session's own `Evaluator`, with the session's
  "suspended" flag set so its hook returns `Continue` immediately (hook doc §5.1).

---

## 4. Stepping semantics and the shadow stack

There is no call stack to display. `ContEnv` is a lexical map of continuation bindings
(reset to just `cont_param` on lambda entry,
`crates/patina-tree-walker/src/eval/cps_eval/application.rs:127–131`), not a dynamic
frame chain; `CpsContinuation` (`patina-core/src/continuation.rs:24`) captures what a
*continuation* needs, not what a *backtrace* needs. The session therefore maintains a
**shadow stack** — and this is a feature, because the hook delivers exactly the right
events:

- **Push** on `Apply` of a `CpsLambda`: frame = `{ proc name if known, call-site
  SourceLocation, cont identity, env_id }`.
- **Pop** on `Return` into that frame's continuation.
- **Tail-call rule** (the part gyrus didn't need): in CPS, a tail call passes the
  caller's continuation through unchanged, so the callee's eventual `Return` fires
  once for the whole chain. On `Apply`, if the new application's `cont` is *identical*
  to the top frame's recorded continuation, **replace** the top frame instead of
  pushing. This makes the stack pane honest about proper tail calls (a
  self-tail-recursive loop shows one frame, matching R7RS semantics and user
  expectation) and bounds the shadow stack. Identity: `ContValue::Local` can be
  compared by `Rc::ptr_eq` on its body; the design should verify one comparable token
  exists on every variant and add one if not (a `u64` id stamped at continuation
  creation is the fallback, mirroring `DynamicWindRecord.id`).
- **Invalidate** on `Unwind`: a captured-continuation escape teleports the machine to
  a continuation captured who-knows-where; the honest response is to truncate/rebuild
  rather than pretend (`call/cc`-heavy code gets a `[stack unwound]` marker frame).
  Note re-entering a captured continuation resets `prompt_stack` and
  `exception_handlers` to empty (hook doc §4(c)), so the debugger must not assume
  those recover either.

Stepping modes, defined against the shadow stack:

| Mode | Rule |
|---|---|
| step (into) | pause at the next `Step` event that carries a `SourceLocation` |
| step over | record top frame; pause at next sourced `Step` whose shadow depth ≤ that frame's depth |
| step out | pause when the recorded frame is popped |
| continue | pause on breakpoint / watch / `Raise` |

"Next *sourced* step" is what makes stepping feel statement-level rather than
CPS-node-level: unsourced administrative nodes (`LetCont`, `Continue`, `Halt`, …) are
skipped silently. This drops out of the hook doc's H0-a source-coverage work for free,
and is also the free-run fast path: `expr.source.is_none()` ⇒ the hook's `Step` arm
returns immediately in `Continue`/`StepOver` modes.

`dynamic-wind` thunks (visible once the wind trampoline is hooked — hook doc H0-b)
appear as ordinary applies; the `Unwind`/`Raise` events give "break on exception" and a
truthful story during non-local exits. Delimited continuations are **out of scope**:
`make_delimited_continuation` is an explicit TODO stub returning a dummy continuation
(`cps_eval/wind.rs:124`) — nothing may be built on it.

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
  from all other statements, which render in the alternate ("brighter" / dimmed)
  style. Implementation: dim the whole document; paint
  `[line, col .. col+length)` of the current `SourceLocation` in the highlight style
  (REVERSED|BOLD per gyrus's per-character `char_style`). Because Scheme expressions
  nest, highlight the *innermost* sourced expression and optionally underline the
  enclosing application — both locations are in hand (`Step` vs. the current `Apply`).
  Gutter: `▶` current, `●` breakpoint, both `▶●`.
- **Locals pane** (the "value side pane"). Walk the paused `Rc<Environment>`:
  `bindings()` (`patina-core/src/environment.rs:772`) for the current frame, then
  `parent()` up the chain, rendering values with the existing `format_display_tagged`
  datum writer. Filter compiler-introduced names (gensyms, cont params) by prefix.
  `env_id()` (`environment.rs:208`) — process-unique, never reused — is the stable
  identity for "which frame am I looking at" across steps.
- **Return-value flash.** On `Return` events, show the value flowing into the
  continuation in the status area ("value ⇒ 1") — in a CPS machine this is the single
  most informative signal while stepping, and it costs nothing.
- **Stack pane** — the shadow stack of §4.
- **Macros pane** — §6.
- Free-run cadence: check keyboard/redraw only every N events (gyrus: 2048) so `c`
  stays responsive without per-step terminal work.
- REPL/eval sources are named `<repl-N>`/`<string>` — the source pane must handle a
  program assembled from multiple named chunks (tab per chunk, or restrict phase D2 to
  file-mode debugging).

Phase D1 ships **without** any of this: a plain line-mode stepper (print caret context
via `SourceMap::format_context`, read a command) proves the entire session layer with
no terminal management, and remains forever useful over ssh and in tests.

---

## 6. The macro expansion pane

The requirement: when the program uses macros, show how the definitions expanded.

What exists today:

- Expansion happens *inside the desugarer during eval*
  (`patina-frontend/src/desugarer/mod.rs:1140–1186`): `expand_macro_with_scope`
  produces the expanded `TaggedValue` form, which is stamped with the **call-site**
  location (`stamp_expansion_source`, `mod.rs:110`), recorded by *name only*
  (`SourceMap::record_expansion` → `expansion_records: HashMap<(line,col),
  Vec<String>>`), and then consumed by the recursive `desugar_tagged` call and
  **dropped**. No expanded tree survives to runtime.
- `MacroTracer` (`patina-macros/src/tracer.rs:46`) already hooks every expansion,
  thread-locally, with per-name selectivity and a depth cap, recording
  `ExpansionStep`s.

Design: **retain, per call site, the (pre-form, post-form) datum pair at expansion
time**, keyed by `(line, col)` — the same key `expansion_records` already uses — via
the `MacroTracer` entry point rather than a new mechanism. The session enables
retention before eval and renders the pair with the datum writer when the source
cursor sits on a call site whose `expansion_records` chain is non-empty. Chained
expansions (`my-or` → `let` → …) display as the recorded sequence, outermost first —
the data the "macro expansion chain: a → b" error formatting already proves out.

Two constraints, stated honestly:

- **GC.** Retained datums are heap values; the retention table must be a GC root
  (implement a root provider like `StepRoots`) and must be bounded (per-site cap +
  global cap; a macro in a hot loop expands once per *desugar*, not per iteration, so
  the natural volume is proportional to program size, not run time — but `eval` and
  the REPL re-desugar, so caps are still required).
- **Granularity.** Everything inside an expansion carries the call site's location, by
  design (`stamp_expansion_source`). So the pane answers *"what did this call site
  become"* — the stated requirement — but **stepping to template positions inside a
  macro definition is out of scope**. That would need per-template source tracking
  through expansion (SRFI-211-style syntax-object locations), a macro system change
  far beyond a debugger. The gyrus `.bfm` refusal is the cautionary precedent; this
  design draws the same line one level higher, where it is actually useful:
  step-level = call site, expansion shown on demand.

---

## 7. Watchpoints

The requirement: stop when some variable changes, or changes to a particular value.

- **Events**: `SetVar` (fired from `set_var_tagged`, one function covering both `set!`
  resolution paths) and `Define` (redefinition at the REPL counts as a change). Both
  are user-level-only by construction (hook doc §5.2), so there is no gensym noise to
  filter.
- **Spec**: `Watch { name: Rc<str>, mode: AnyChange | Becomes(TaggedValue),
  scope: Global | Env(u64 /* env_id */) }`. Comparison via the existing
  `Heap::values_eqv` (`&self`, no allocation) matches Scheme's notion of "same value"
  for the `Becomes` form; document that `Becomes` on strings/lists compares by `eqv?`,
  with an `equal?` option if wanted later.
- **Retained old/target values are GC roots** (same root provider as §6).
- **Conditional breakpoints** come almost free: the session owns the `Evaluator`, so a
  breakpoint predicate is a Scheme expression evaluated re-entrantly in the paused
  environment (with the hook suspended). Deferred to phase D4 but designed-for now:
  nothing in the hook API needs to change.
- The VM tracer already has a `Watchpoint` trace-event kind — naming and semantics
  should match it where they overlap, so the two backends' debug stories converge
  instead of forking.

Unwatched-but-displayed variables (the gyrus `Watch::Cell` display-only mode) are just
rows in the locals/watch pane and need no hook support at all.

General practice is worth a comparison here, because this is the one place patina
out-does the mainstream stack instead of catching up to it: `sys.settrace` has no
mutation event, so pdb-family watchpoints are emulated by re-evaluating the watched
expression on every line event, and GDB's software watchpoints single-step the entire
program to compare a value after each step. Patina gets true, cheap watchpoints
because all user-level mutation funnels through one function.

---

## 8. Why not time travel (yet)

Record-and-scrub (the gyrus-tutorial model) looks tempting but does not transfer
cheaply: patina environments are `Rc<RefCell<…>>` and mutable in place. Holding
`Rc<Environment>` per recorded frame does **not** snapshot values — a later `set!` or
`define` mutates the shared slots, and every "historical" frame silently shows the
final state. An honest recorder must copy the reachable bindings per frame (expensive,
unbounded) or patina must grow persistent environments (a language-runtime project,
not a debugger feature). A cheap middle ground — record only `(SourceLocation, event
kind, shallow locals-of-interest)` for a scrollable *trace log* without full state
reconstruction — is worthwhile and listed in phase D4.

---

## 9. Feasibility matrix

Rows marked *(hook doc)* are owned by `TREE_WALKER_HOOK_SYSTEM.md`; the rest is this
document's scope.

| Feature | Exists today | Gap | Effort | Risk |
|---|---|---|---|---|
| Pause points / hook *(hook doc)* | Trampoline + `StepResult` carries full state; VM tracer precedent | Hook trait, fire sites, wind-trampoline coverage | M | Low |
| Statement highlight w/ sub-line span *(hook doc H0-a)* | `SourceLocation.length`, `format_context`, desugarer stamps every pair | CPS transform stamps only App/Apply/LetVal → extend to all kinds | S–M | Low (mechanical, noisy diff) |
| Values side pane | `bindings()`/`parent()`/`env_id()`, datum writer | Gensym filtering, pane code | S | Low |
| Return-value display | `Return` event has value in hand | — | S | Low |
| Breakpoints | — | Location-keyed set + snapping; needs H0-a for non-call sites | S | Low |
| Watch: any-change / becomes-value | `SetVar` choke point; `values_eqv`; VM `Watchpoint` kind | `SetVar`/`Define` consumers, GC-rooted retained values | S | Low |
| Stack pane, step-over/out | Nothing (no call stack exists) | Shadow stack + tail-call replace rule + `Unwind` invalidation | M | **Medium** (tail-call and call/cc corner cases) |
| Macro expansion pane | `MacroTracer`, `expansion_records`, chain rendering | Retain (pre, post) datums per call site, GC-rooted, bounded | M | Medium (GC rooting; REPL re-expansion volume) |
| Break on exception | `Raise` event (single policy point) | consumer only | S | Low |
| Conditional breakpoints | Re-entrant eval available | Suspend-hook flag; UX | S | Low (deferred) |
| Time travel / scrub | gyrus-tutorial pattern | Mutable envs break naive snapshots — needs copying or persistent envs | L | **High** (excluded; trace-log lite listed instead) |
| Template-position macro stepping | — | Per-template source through expansion (macro-system change) | XL | **Excluded** (§6) |
| Delimited-continuation awareness | `Prompt` frames | Capture is a TODO stub (`wind.rs:124`) | — | **Excluded** |

---

## 10. Phasing

Depends on the hook doc's **H0-a** (source coverage), **H0-b** (shared dispatch), and
**H1** (hook layer) landing first; H1 is the go/no-go gate.

- **D1 — session + CLI stepper.** `DebugSession` (RunState, `should_pause`,
  breakpoints, watchpoints, shadow stack, stop reason) and a line-mode stepper (caret
  context + `s/n/o/c/p/b/w` commands) in a new binary or flag. Everything testable
  headlessly by scripting commands. (~1–2 weeks)
- **D2 — ratatui TUI.** Source pane with the dim/highlight scheme, locals pane, watch
  pane, stack pane, output-port redirection, status bar. Widget crate kept free of
  session logic (gyrus pattern), headless `TestBackend` tests. (~1–2 weeks)
- **D3 — macro pane.** `MacroTracer`-based retention + rendering; `e` to expand at
  cursor. (~1 week)
- **D4 — extras, in whatever order earns it:** conditional breakpoints; post-mortem
  entry on uncaught `Raise` (pdb's `pm()` — nearly free once D1 exists); trace-log
  lite (§8); DAP server for editor integration (the session/`should_pause` split is
  DAP-shaped already); REPL-chunk debugging.

Nothing here blocks or is blocked by the macro/hygiene track or the VM track.

---

## 11. Open questions

- **Q2 — delivery vehicle.** `patina --tree-walker --debug script.scm` (a flag in
  patina-repl, like `--trace`) vs. a separate `patina-debug` binary crate (gyrus keeps
  the debugger out of the main CLI). The flag is less machinery; the separate crate
  keeps ratatui/crossterm out of patina-repl's dependency tree until phase D2 forces
  the choice.
- **Q3 — widget reuse.** Copy the relevant `gyrus-tui` source-pane/layout code into
  patina (divergent evolution, no coupling) vs. publishing `gyrus-tui` as a shared
  dependency. Cross-repo coupling for ~a few hundred lines of widget code is probably
  not worth it, but it is the user's crate and the user's call.
- **Q4 — breakpoint UX.** Line-only (`b 12`) vs. line:column, and whether in-source
  markers (gyrus's `@`) have a Scheme analogue worth having (a `(debug-break)` form
  that expands to a primitive call would fall out of D1 for free).

(Q1 — the pause contract — lives in the hook doc, where the API it constrains lives.)

## References

- The hook layer this builds on: `PRD/future/TREE_WALKER_HOOK_SYSTEM.md`
- gyrus: `~/Project/gyrus/crates/{gyrus-debug,gyrus-tui,gyrus-tutorial}`,
  `~/Project/gyrus/docs/debugger.md` (esp. its explicit non-features list)
- TUI direction: `PRD/future/phase4/TUI_IMPLEMENTATION.md` (ratatui + crossterm)
- Session plumbing: `crates/patina-interpreter/src/lib.rs`
  (`eval_program_with_source_name`, `format_interpreter_error`),
  `crates/patina-repl/src/repl/mod.rs`
- Environment inspection: `crates/patina-core/src/environment.rs`
  (`bindings`, `parent`, `env_id`)
- Macro pane inputs: `crates/patina-macros/src/tracer.rs`,
  `crates/patina-frontend/src/{source_map.rs,desugarer/mod.rs}`
- VM analogue: `docs/VM_STEPPER.md`, `crates/patina-vm/src/tracer.rs`
