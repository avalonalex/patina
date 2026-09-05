# Patina VM: Runtime Design

**Status:** Implemented — Phase 2A complete
**See also:** [VM_ISA.md](./VM_ISA.md), [VM_COMPILER.md](./VM_COMPILER.md), [VM_DECISIONS.md](./VM_DECISIONS.md)

---

## 1. Overview

The runtime executes bytecode produced by the compiler. It consists of:

1. **`VmState`** — the complete execution state
2. **Execution loop** — fetch-decode-execute cycle
3. **Control primitives** — continuations, exceptions, dynamic-wind, values
4. **Library loading** — R7RS library support

The runtime reuses `TaggedValue`, `SharedHeap`, and `patina-primitives` directly.
No value conversion is needed.

---

## 2. Core Data Structures

### 2.1 `CallFrame`

```rust
#[derive(Clone)]   // ← non-negotiable; required for stack-snapshot continuations
pub struct CallFrame {
    pub code_id:       CodeObjectId,    // index into VmState::code_store
    pub pc:            usize,           // program counter
    pub register_base: usize,           // offset into VmState::registers
    pub num_regs:      u16,             // register window size
    pub closure:       Option<HeapIndex>, // heap index of VmClosure (if any)
    pub return_reg:    Reg,             // where to write result in caller
}
```

Code objects are looked up in `VmState::code_store` by `CodeObjectId`, not
stored directly in the frame.

### 2.2 `VmState`

The complete runtime state.

```rust
pub struct VmState {
    /// Flat register array. Each CallFrame owns a slice via register_base + num_regs.
    pub registers: Vec<TaggedValue>,

    /// Call stack. frames.last() is the currently executing frame.
    pub frames: Vec<CallFrame>,

    /// Side channel for multiple return values (values / call-with-values).
    pub value_buffer: Vec<TaggedValue>,

    /// Prompt stack for delimited continuations.
    pub prompt_stack: Vec<PromptFrame>,

    /// Dynamic-wind records, outermost first.
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// Exception handler stack (with-exception-handler).
    pub exception_handlers: Vec<ExceptionHandler>,

    /// Compiled code objects, indexed by the sequential CodeObjectId.
    pub code_store: Vec<Option<Rc<CodeObject>>>,

    /// Global environment.
    pub globals: Rc<Environment>,

    /// Shared heap (pairs, strings, closures, etc.).
    pub heap: SharedHeap,

    /// Primitive function dispatch.
    pub primitive_registry: Rc<PrimitiveRegistry>,

    /// Full continuation side table (avoids circular deps with patina-core).
    /// Weak GC table (GC_DESIGN.md §9.5); ids are minted by the heap.
    pub continuation_store: RefCell<FxHashMap<u64, Rc<VmContinuation>>>,

    /// Delimited continuation side table. Weak, like `continuation_store`.
    pub delimited_continuation_store: RefCell<FxHashMap<u64, Rc<VmDelimitedContinuation>>>,

    /// Optional instruction-level tracer.
    pub tracer: Option<TracerHandle>,

    /// Library registries for library loading.
    pub library_registry: Option<Rc<RefCell<LibraryRegistry>>>,
    pub loader_registry: Option<Rc<RefCell<LibraryLoaderRegistry>>>,
}
```

### 2.3 `PromptFrame`

```rust
#[derive(Clone)]
pub struct PromptFrame {
    pub tag:                     TaggedValue,
    pub stack_depth:             usize,
    pub handler:                 TaggedValue,
    pub dst:                     Reg,
    pub dynamic_wind_depth:      usize,
    pub exception_handler_depth: usize,  // the boundary an abort unwinds to
}
```

### 2.4 `DynamicWindRecord`

```rust
#[derive(Clone)]
pub struct DynamicWindRecord {
    pub id:       u64,            // unique per `dynamic-wind` call
    pub before:   TaggedValue,
    pub after:    TaggedValue,
    pub handlers: Rc<[ExceptionHandler]>,
}
```

### 2.5 `ExceptionHandler`

```rust
pub struct ExceptionHandler {
    pub handler:       TaggedValue,
    pub dynamic_winds: Vec<DynamicWindRecord>,
    pub stack_depth:   usize,
}
```

Pushed by `with-exception-handler`, popped on thunk return. Exception handlers
are captured/restored by `call/cc`.

### 2.6 Continuation Objects

```rust
pub struct VmContinuation {
    pub frames:             Vec<CallFrame>,
    pub dynamic_winds:      Vec<DynamicWindRecord>,
    pub prompt_stack:       Vec<PromptFrame>,
    pub exception_handlers: Vec<ExceptionHandler>,
    pub registers:          Vec<TaggedValue>,
    pub deliver_reg:        Reg,    // where to write value on invocation
}

pub struct VmDelimitedContinuation {
    pub frames:             Vec<CallFrame>,
    pub dynamic_winds:      Vec<DynamicWindRecord>,
    pub registers:          Vec<TaggedValue>,
    pub base_at_capture:    usize,
    pub deliver_reg:        Option<Reg>,  // the hole, in frames.last();
                                          // None = nothing captured (identity)
    pub depth_at_capture:   usize,        // frame depth the slice started at;
                                          // the two below are relative to it
    pub prompt_stack:       Vec<PromptFrame>,
    pub exception_handlers: Vec<ExceptionHandler>,
}
```

**Storage:** Continuations are stored in **side tables** on VmState
(`continuation_store`, `delimited_continuation_store`), not directly on the heap.
Opaque handles (`VmContinuationRef(u64)`, `VmDelimitedContinuationRef(u64)`)
are stored as `HeapObjectData` variants. This avoids circular dependencies
between `patina-core` and `patina-vm`.

---

## 3. Execution Loop

### 3.1 Entry Point

```rust
pub fn execute(state: &mut VmState, code_id: CodeObjectId) -> Result<TaggedValue, VmError>
```

Allocates an initial frame and calls `run_loop_until(state, 0)`.

### 3.2 Main Loop

```rust
fn run_loop_until(state: &mut VmState, exit_depth: usize) -> Result<TaggedValue, VmError>
```

Runs `dispatch_one_instruction()` in a loop until `frames.len() == exit_depth`.
Catchable errors are routed through exception handlers via `vm_raise_value()`.

### 3.3 Instruction Dispatch

```rust
fn dispatch_one_instruction(
    state: &mut VmState,
    cur_code: &mut Rc<CodeObject>,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError>
```

Returns `Ok(Some(val))` to exit the loop, `Ok(None)` to continue.

`cur_code` is the loop's cached copy of the top frame's code object; the
dispatch prologue refreshes it (pointer compare) only when the frame's code
changed, so no `Rc` clone is paid per instruction. The prologue also reads
`pc` (folding in its advance) and the frame's `register_base` in the same
frame access.

### 3.4 Register Access

```rust
impl VmState {
    pub fn reg(&self, r: Reg) -> TaggedValue;
    pub fn set_reg(&mut self, r: Reg, val: TaggedValue);
    fn reg_at(&self, base: usize, r: Reg) -> TaggedValue;      // dispatch loop
    fn set_reg_at(&mut self, base: usize, r: Reg, val: TaggedValue);
}
```

All index `register_base + r` into the flat register array. The `_at`
variants take the dispatch prologue's hoisted base; only instruction arms
that cannot push or pop a frame before the access may use them (a plain
`set_reg` at a dispatch site signals "the frame changed here").

The primitive-call arms are the exception: they pass the hoisted base to a
callee that *can* pop frames, since a higher-order primitive re-enters the VM
and a continuation can escape out of its callback. They hold only because
`exec_call_primitive` re-reads the frame depth before writing.

---

## 4. Call and Return

### 4.1 Call Dispatch Order

When a `Call` or `TailCall` instruction is executed:

1. **VM control primitive check** — `vm_control_primitive()` detects if the
   function is a known control primitive (continuations, exceptions, values,
   dynamic-wind). If so, dispatch via `handle_control_primitive()`.
2. **Primitive call** — `try_call_primitive()` checks if it's a
   `Procedure::Primitive` on the heap, calls it directly without pushing a frame.
3. **Parameter call** — `try_call_parameter()` checks if it's a parameter object.
4. **Continuation call** — `try_invoke_continuation()` checks if it's a
   continuation object.
5. **Closure call** — `call_closure()` resolves to `(code_id, free_vars)`,
   checks arity, pushes a new frame.

### 4.2 `call_closure`

```rust
fn call_closure(
    state: &mut VmState,
    closure_val: TaggedValue,
    args: &[TaggedValue],
    return_reg: u16,
) -> Result<(), VmError>
```

- Looks up code object in `code_store`
- Checks arity
- Allocates register window for new frame
- Copies args into callee parameter slots
- Handles variadic rest-arg collection via `build_list()`

### 4.3 Tail Call

For `TailCall`, the current frame is reused:
- Read all arg values into temporaries first
- Pop current frame, free registers
- Push new frame with callee's code
- Write args to param slots

### 4.4 Return

On `Return { val }`:
1. Read result value
2. Pop current frame, free registers
3. If at exit depth, return result
4. Write result to caller's `return_reg`
5. Run cleanup: `pop_resolved_prompts()`, `pop_exception_handlers()`. Wind
   records are not swept here — `PopWind` pops each one (§5.3)

### 4.5 Helper Functions

- **`call_any()`** — dispatches to primitive, parameter, or closure; used by
  control primitives for sub-calls
- **`call_any_sync()`** — calls and waits for result (primitives return
  immediately; closures run via `run_loop_until()`)
- **`run_thunk()`** — calls a 0-arg closure to completion; used by dynamic-wind
  and wind transitions

---

## 5. Control Primitives

All continuation and exception operations are handled as VM control primitives,
intercepted at call dispatch time.

### 5.1 `VmControlPrimitive` Enum

| Variant | Scheme form | Behavior |
|---|---|---|
| `DynamicWind` | `dynamic-wind` | Push wind record, run body, pop on return |
| `CallWithContinuationPrompt` | `call-with-continuation-prompt` | Push prompt, run body thunk |
| `AbortCurrentContinuation` | `abort-current-continuation` | Find prompt, capture delimited cont, unwind, call handler |
| `CallWithCurrentContinuation` | `call/cc` | Snapshot full stack, deliver to proc |
| `Values` | `values` | Store in `value_buffer`, return primary value |
| `CallWithValues` | `call-with-values` | Clear buffer, run producer, unpack values, call consumer |
| `WithExceptionHandler` | `with-exception-handler` | Push handler, run thunk, pop on return |
| `Raise` | `raise` | Pop handler, push `raise_step_stub` — **no unwind** (§5.2) |
| `RaiseContinuable` | `raise-continuable` | Like Raise but handler returns to raise site |
| `Error` | `error` | Construct error object, then Raise |

### 5.2 Exception Handling

- `ExceptionHandler` stack on VmState, pushed/popped by `WithExceptionHandler`
- `vm_raise_value()`: pops the handler and pushes `raise_step_stub` — three
  instructions, `Call handler` / `ResumeRaise` / `Return` — then returns. Both
  kinds of raise take that one shape; a register says which. **What the raise
  still owes is therefore a pc**, and a continuation captured inside the
  handler carries it: `ResumeRaise` re-pushes the handler for a continuable
  raise (R7RS 6.11 reinstalls it once the handler returns) and raises the
  secondary exception for a non-continuable one. Until 2026-09-05 both were
  Rust after a nested `run_loop_until_outcome`, which cost four divergences at
  once — a `guard` silently uninstalled after a declining clause, a composable
  continuation that lost its handler, a non-continuable handler allowed to
  return, and (because that call went through the narrow `call_any`) a
  continuation rejected as a handler for a primitive's error. Issue #178; the
  same "make the remainder a frame" move as #157, #158, #165 and #167
- The handler's recorded depth is restored from a **distance below the stub's
  own frame**, not an absolute index: the stub can be captured and replayed
  somewhere else — resumed through a composable continuation, re-entered
  through `handler-k` — and only a distance relocates with it
- A raise does **not** touch the wind stack: R7RS 6.11 calls the handler "in the dynamic
  environment of the call to `raise`", and a raise crosses no dynamic extent,
  so no `after` thunk is due. Popping the handler is the only change a raise
  makes. `guard` unwinds by escaping through `guard-k`, an ordinary
  continuation, and re-raises by jumping back in through `handler-k` (see
  `lib/scheme/base/exceptions.scm`). Unwinding here instead ran an after thunk
  twice and left `guard` with no raise point to return to — Track L triage
  families 22 and 28, fixed 2026-09-01
- `ExceptionHandler` records only the handler and the frame depth. It used to
  carry the wind depth `raise` unwound to; nothing unwinds, so nothing reads it
- `pop_exception_handlers()`: cleanup on normal thunk return
- Error classification: `classify_error()` maps `VmError` →
  `ExceptionKind` (FileError, ReadError, etc.)
- Exception handlers captured/restored by `call/cc`

### 5.3 Dynamic Wind

- Wind records are *minted* only by `PushWind` — head position compiles to it,
  and the value form runs it too (see the last bullet below). Two other sites
  grow `dynamic_winds` by re-pushing records that already exist:
  `ResumeWindJump` (entering an extent of the jump target) and the
  composable-continuation invokes (appending the extents they captured). Each
  record carries the **exception handler stack of its own call**
  (`DynamicWindRecord::handlers`), which is the part of the dynamic
  environment R7RS 6.10 gives its thunks
- `step_wind_jump()`: one step of a jump to a full continuation. It finds the
  common prefix of the live and target wind stacks and runs **one** thunk —
  an `after` for the innermost extent being left (popped *before* it runs), or
  a `before` for the outermost being entered (its record pushed only *after*
  it returns) — then, with none left, restores the target's snapshot and
  delivers the value
- Each thunk runs as an ordinary frame under a stub frame whose single
  instruction is `ResumeWindJump`, which comes back to `step_wind_jump`. So
  "the rest of the jump" is a frame: a continuation captured inside a thunk
  captures it, and re-entering that continuation finishes the thunk and then
  the jump. Thunks ran on a nested Rust call until 2026-09-02, and a
  continuation captured in one re-entered the *jump site's* frame instead —
  mid-sequence, at its `PopWind` (Track L §6, the `finally` rule)
- The handler stack a thunk runs under comes from its record, not from the
  machine. `install_thunk_handlers` clamps the recorded frame depths to the
  live stack first; `pop_exception_handlers` reads them, and they belong to a
  stack that has moved on
- The common prefix is keyed on `DynamicWindRecord::id`, unique per
  `dynamic-wind` *call* and minted by `patina_core::next_dynamic_wind_id`. The
  `before` thunk is not an identity — two calls may share one closure
- Full `call/cc` invokes take the common prefix. A `force_reenter` flag used
  to force it to zero for them, so a continuation captured inside its own
  extent re-ran that extent's thunks for a jump that crossed nothing (fixed
  2026-09-01, Track L §6)
- **A thunk run by a *transfer* goes through `step_wind_jump`** — a jump, and
  since #165 an abort, which travels to a continuation whose top frame calls
  the prompt handler. Each such thunk runs in a stub frame and under its own
  record's handler stack
- **A composable invoke's re-entry thunks are frames too, but not the jump's**
  (issue #167). Each runs under a `ResumeComposableInvoke` stub — the exact
  analogue of `ResumeWindJump`, so that "the rest of the invoke" is a pc a
  re-entering continuation restores rather than a Rust frame it abandons —
  and under the **invoke site's** handler stack, with no
  `install_thunk_handlers`. Making them go through `step_wind_jump` was tried
  and reverted: a jump's target *replaces* the machine, which is what makes
  installing the record's captured stack right there, while a composable
  target *extends* it, so the same call loses the invoke site's handlers and
  resurrects capture-site ones whose extent is over — measured against Guile
  and against the release before it. The stub keeps the frame and leaves the
  handler question alone, which is why it is a fourth mechanism and not a
  third use of the third
- **The thunks of an ordinary `dynamic-wind` are neither.** Head position
  compiles to plain `Call`s around `PushWind`/`PopWind`, and the value form
  runs the same instructions in `value_wind_stub`; both run under whatever
  handler stack is live, which is the right one, because no transfer is
  happening
- A composable invoke has two values to place, and `invoke_delimited` is the
  one place that places them. The delivered value goes into the innermost
  captured frame's `deliver_reg` — the hole the capturing call left, since
  those frames resume at the instruction *after* it — and the outermost
  captured frame's `return_reg` is re-pointed at the invoke's own destination,
  because the prompt it was captured returning to is not where `(k v)` was
  called and, after an abort, is not on the stack at all. The append on its
  own delivered nothing and returned nowhere (issue #160)
- A capture with no frames in it — an `abort-current-continuation` in tail
  position of the prompt body pops the body's frame before the abort runs —
  is the identity continuation, and `(k v)` is `v`. That is what
  `deliver_reg: None` means, and the invoke hands the value straight back to
  its caller instead of appending anything
- A composable invoke reports **a pushed frame**, never the escape sentinel a
  full one reports. The distinction is what the caller still owes: a jump
  replaces the stack and voids the Rust work above it, while a composable
  continuation *returns*, so a `call-with-values` consumer still has to be
  applied, a jump still has to be finished, a `raise-continuable` entry still
  has to be re-pushed. `call_any` returning the sentinel is why the value form
  of `call-with-values` dropped its consumer where the head form did not. That
  a resumed invoke can push **more than one** frame is the other half of the
  same rule: a caller that then runs its own dispatch loop must read the depth
  to run back down to *before* the call, not as `frames.len() - 1` after it
- The value form of `dynamic-wind` (`handle_control_primitive`) pushes a stub
  frame running `value_wind_stub`'s six instructions — `Call before` /
  `PushWind` / `Call body` / `PopWind` / `Call after` / `Return`, the same
  instructions in the same order that pass 5 emits for head position, differing
  only in that the stub always ends in `Return` (pass 5 emits one only in tail
  position) and discards the two thunk results into a dedicated slot rather
  than into `expr.dst` and the before-thunk's register — and returns to the
  dispatch loop. So the two forms share one implementation, and everything the
  call still owes is a **pc**: a continuation captured in the body restores
  this frame with it, and returning through it pops the record, runs this
  call's own after-thunk, and delivers the body's value. Until 2026-09-02 the
  arm ran the body on a nested Rust call and did that bookkeeping in Rust,
  which a re-entry abandoned: the call ran the *wrong* after-thunk (its own a
  second time, the target's not at all — it decided what it owed by comparing
  wind-stack lengths after the jump had replaced that stack) and left its
  caller's register holding the `NULL` `call/cc`'s capture had cleared it to
  (issue #157)
- Wind records are therefore not swept by frame depth, and carry none. A
  `pop_resolved_winds()` that ran the after-thunk of a record whose body had
  returned existed until 2026-09-02 as the value form's last line of cleanup;
  with `PushWind`/`PopWind` doing the work it was reachable on no input

### 5.4 Values / Call-with-values

- `Values` stores all args in `value_buffer` unconditionally
- For single value, also writes to `dst` register directly
- For multiple values, allocates a heap `Values` object for display
- `CallWithValues` clears stale `value_buffer` before running producer,
  then unpacks from buffer or from `#<values>` heap object

### 5.5 Continuation Invocation

**Full (call/cc):**
1. Snapshot frames, registers, winds, prompts, exception handlers
2. On invocation: travel the wind stacks one thunk per step (§5.3), each on a
   `ResumeWindJump` stub frame, then restore the entire snapshot
3. Deliver value to `deliver_reg`

**Delimited (abort/prompt):**
1. Find matching prompt by tag
2. Capture frames `[prompt.stack_depth..]`, registers, winds, and the dynamic
   environment above the prompt — the prompts nested inside it and the handlers
   installed inside it
3. Build the machine as it will be once the abort lands — every stack cut back
   to the prompt — with one stub frame on top whose two instructions call the
   prompt's handler and return its value to `prompt.dst`
4. **Jump** to it. The travel runs the after-thunks of every extent between
   here and the prompt, each under its own record's handler stack and in a
   frame of its own; arrival installs the rest

Step 4 is why the abort has no unwinding code of its own: leaving every extent
between here and a target is exactly a jump's travel, and the version that did
it by hand got the thunks' handler stack wrong and lost the abort's value when
a continuation captured in a thunk was re-entered (#165).

Invoking it appends all of that back, relocated, in place — not as the target
of a jump; see §5.3 for the one attempt at that and why it was reverted. A
`PromptFrame` records a position in **three** stacks —
frames, winds, handlers — and all three move: relocating only the frame depth
left an abort to a carried prompt truncating the invoke site's wind and
handler stacks at depths that meant nothing there.

They come off again by the ordinary depth sweep as the resumed frames return
(`pop_resolved_extents`), and by a backstop at the dispatch loop's own exit
depth, where that sweep does nothing by design:
`run_loop_until_outcome` truncates both `exception_handlers` and
`prompt_stack` to their lengths at entry, since a loop owns no extent it did
not open.

The prompt half of that was missing until 2026-09-05 (issue #176), and the
program that needed it is the one no depth can classify: `(call/cc …)` in
**tail position** of a prompt body. The body's frame is popped before the
capture, so the prompt's `stack_depth` equals `frames.len()` while the body is
still running — an abort then must still find the prompt, and every
implementation agrees it does — and the same reading holds once the body's
value has been delivered and the prompt is finished. A continuation captured
in that window carries the prompt in its snapshot, so re-entering it restores
a frame the depth sweep will never reach again, and the *next* form's abort
lands on a prompt whose body returned in the previous one. It is the frame
-depth twin of the `exception_handler_depth` problem in §5.6's last
paragraph, and it has the same shape of answer: record a length, and close
what the loop did not open.

### 5.6 The dynamic-state matrix

`VmState` carries five components that belong to a *dynamic extent* rather
than to the machine: `frames`, `registers`, `dynamic_winds`, `prompt_stack`,
`exception_handlers`. Every control transfer has to say what it does with each
one, and a transfer that forgets a component does not fail loudly — it runs
under somebody else's dynamic context.

This table is the whole list. It exists because the defects in this area were
found one at a time by hand-written repros (#157, #159, #160, #162, #163) while
the chibi suite stayed 1226/1226 through all of them; an empty cell is findable
by reading, before anyone writes a program that trips over it.

Its executable counterpart is
`crates/patina-tests/tests/control_flow_matrix.rs`, which enumerates the
*transfers* rather than the state: 24 shapes over how a `dynamic-wind` is
written, whether it is in tail position, and how control leaves or re-enters
it. Each row records **which** external implementations back its answer —
four for the prompt-free shapes, down to one for the shape only Guile can
express — rather than one number for the table. Where this table catches a
component nobody carried, that one catches a shape nobody tried.

| transfer | frames | registers | dynamic_winds | prompt_stack | exception_handlers |
|---|---|---|---|---|---|
| `call/cc` capture | save | save | save | save | save |
| full invoke (arrival) | restore | restore | restore | restore | restore |
| delimited capture | save | save | save | save | save |
| composable invoke | append | append | append (re-enter) | append | append |
| abort unwind | *builds a landing and travels to it* — the `full invoke` row, with a stub frame on top that calls the prompt handler | | | | |
| `raise` | push the stub that calls the handler (§5.2) | — | — | — | pop one; `ResumeRaise` re-pushes on a continuable return |
| normal `Return` | pop | free | by `PopWind` | by depth | by depth |
| wind thunk (jump, abort) | push stub | — | — | — | replaced by the record's own |
| wind thunk (invoke re-entry) | — | — | — | — | the invoke site's |
| wind thunk (ordinary call) | — | — | — | — | the live stack |

Notes on the cells that are not a plain yes:

- **`raise` touches nothing but the handler stack.** A raise crosses no
  dynamic extent, so no after-thunk is due; the unwind happens one level up,
  at `guard`'s continuation jump. See §5.2.
- **A thunk run by a transfer runs under its own record's handler stack**
  (`install_thunk_handlers`, whose only caller is `push_wind_step`), which is
  what R7RS 6.10 asks for: the thunk gets the dynamic environment of the
  `dynamic-wind` *call*. That is right precisely because a transfer's target
  replaces the machine, so there is no live stack worth returning to — and it
  is why a composable invoke's re-entry thunks are **not** routed here (see
  §5.3). Before #165 the abort's after-thunks ran on a nested loop under the
  live stack and had the two defects that follow: the wrong handler for a
  raise from a thunk, and a continuation captured in one restarting the thunk
  from the top on re-entry, losing the abort's value.
- **Parameter objects are not a sixth component.** `parameterize` expands to
  `dynamic-wind` around a swap (`lib/scheme/base/parameters.scm`), so
  parameter state rides on `dynamic_winds` and needs no snapshot of its own.
  Anything else built on `parameterize` — `current-output-port` and the
  redirecting `with-*` procedures — inherits that.
- **`execute` clears all five** when a form abandons the machine with an
  uncaught error. Winds are dropped rather than unwound: an abandoned
  after-thunk was never owed a run, and one that raised would abort the
  recovery.

#### What the reference implementations answer

The rows above are Patina's behaviour. These are the same questions put to
implementations that have had decades of scrutiny, measured 2026-09-03 — Chez
10.4.1 (`petite`), chibi 0.12, Gauche 0.9.15, Guile 3.0.11, Racket 9.3.
Larceny is a source checkout here with no built binary (the lane only borrows
its test suite) and has no delimited-continuation facility in its tree at all,
so it would only add a sixth mark to rows that are already unanimous.

| | question | Chez | chibi | Gauche | Guile | Racket | Patina |
|---|---|---|---|---|---|---|---|
| P1 | a full continuation carries the handler stack | ✓ | ✓ | ✓ | ✓ | — | ✓ (both backends) |
| P2 | escaping a handler's extent uninstalls it | ✓ | ✓ | ✓ | ✓ | — | ✓ (both backends) |
| P3 | re-entering that extent re-installs it | ✓ | ✓ | ✓ | ✓ | — | ✓ (both backends) |
| A1 | **aborting** out of that extent uninstalls it | n/a | n/a | n/a | ✓ | ✓ | ✓ (both backends) |
| D1 | a composable continuation carries the handler stack | n/a | n/a | ✓ | ✓ | ✓ | ✓ (both backends) |
| D2 | a composable continuation re-enters its wind extents | n/a | n/a | ✓ | ✓ | ✓ | ✓ (both backends) |
| D3 | a composable continuation carries a delimiter inside it | n/a | n/a | n/e | ✓ | ✓ | ✓ (both backends) |

`n/a` — no delimited-continuation facility. `n/e` — not expressible: Gauche's
`shift`/`reset` is untagged, so a `shift` cannot reach past the nearest
`reset`. `—` for Racket on P1–P3: it has no `raise-continuable`, and its
escaping `with-handlers` answers a different question.

The tree-walker's marks date from 2026-09-04 (issue #169). Its prompt is not
a depth in three stacks but a boundary *value* in the continuation, with the
wind and handler depths recorded on the frame (`cps_eval/prompts.rs`); the
same matrix rows and the same seven `cps_features.rs` tests score it, so the
column is one answer reached two ways.

Two things worth taking from this. **P1–P3 are unanimous across six
implementations**, so those cells are settled behaviour rather than a Patina
convention — and Patina matches on both backends. And **every delimited row
has at least two independent oracles**: Gauche's `gauche.partcont` and Guile's
`(ice-9 control)` both answer with real R7RS `with-exception-handler` and
`raise-continuable`, which is a stronger witness than Racket's for the handler
question, since Racket's handlers are continuation marks and the agreement
could otherwise be an artifact of that. All of them say a composable
continuation carries the dynamic context of its *capture*.

**Guile is the closest match to this VM's API** and the best oracle to reach
for: `call-with-prompt` / `abort-to-prompt` are tagged like Patina's, the
handler receives the continuation directly rather than needing it carried
through the abort as Racket's does, and R7RS exceptions are right there
(`guile --r7rs`, importing `(scheme base)` alongside `(ice-9 control)`, and
`#:unwind? #f` for a non-escaping handler). Programs transcribe one for one.

A1 was the sharpest of them, because it is #162 itself rather than an
analogue — the row read `✗` until the fix, and this is what it showed:

```scheme
(with-exception-handler (lambda (e) (list 'ESCAPED-TO-TOP e))
  (lambda ()
    (call-with-continuation-prompt
      (lambda ()
        (with-exception-handler (lambda (e) (list 'INNER e))
          (lambda () (list 'got (abort-current-continuation t 'ab)))))
      t
      (lambda (v k) (raise-continuable 'boom)))))

;; before => (INNER boom)            ← the abandoned handler catches
;; now    => (ESCAPED-TO-TOP boom)   = Racket
```

**The two holes interacted, which is why they were one change.** A raise
inside a resumed composable continuation used to find the handler live at
capture — the right answer, by accident, because the abort (#162) left that
handler installed rather than because the continuation carried it (#163).
Escaping the same continuation out of its prompt first, so the stale entry was
swept, made the identical raise unhandled. Fixing #162 alone would have turned
a working program into a broken one.

**The boundary is a recorded length, not a frame depth**, and this is the part
that is easy to get wrong twice. A `with-exception-handler` in *tail position
of a prompt body* installs at the prompt's own frame depth, because the body
frame is already popped; so does a handler whose thunk *tail-calls*
`call-with-continuation-prompt`. The first is inside the prompt and must be
uninstalled, the second encloses it and must survive, and no comparison
against `stack_depth` separates them — each choice of `<` or `<=` fixes one
shape and breaks the other. `PromptFrame::exception_handler_depth` records
`exception_handlers.len()` at push time, as `dynamic_wind_depth` has always
done for winds. Both shapes are pinned in
`test_a_prompt_transfers_carry_the_dynamic_environment`.

---

## 6. GC Roots

| Location | What it holds |
|---|---|
| `vm.registers` | All live register values |
| Closure `free_vars` (on heap) | Captured variables |
| `vm.value_buffer` | Multiple-value buffer |
| `vm.prompt_stack[*].tag/handler` | Prompt tags and handlers |
| `vm.dynamic_winds[*].before/after` | Wind thunks |
| `vm.exception_handlers[*].handler` | Exception handlers |
| `vm.globals` | All global bindings |
| `vm.continuation_store` | Full continuation snapshots |
| `vm.delimited_continuation_store` | Delimited continuation snapshots |
| `CodeObject::constants` | Constant pools |

Phase 2A uses `Rc` — cycles from `set-car!`/`set-cdr!` may leak.

---

## 7. Relationship to Existing Code

| Existing component | VM usage |
|---|---|
| `TaggedValue` | Used as-is throughout — no conversion |
| `SharedHeap` | Shared between tree-walker and VM |
| `patina-primitives` | Called via `try_call_primitive()` and `CallPrimitive` |
| `Environment` (globals) | Shared global environment |
| `SourceLocation` / `SourceMap` | Stored in `CodeObject` for error messages |
| `CpsExpr` / CPS transform | Not used — VM compiles direct-style |
| `patina-tree-walker` | Unchanged; VM is an independent `Backend` impl |
