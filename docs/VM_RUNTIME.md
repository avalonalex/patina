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

### 2.1 Frames and the register stack

```rust
pub struct FrameStack {
    words:    Vec<Cell<TaggedValue>>,  // headers and registers, bottom first
    codes:    Vec<FrameCode>,          // the frames' code: an Rc<CodeObject> a run
    runs:     usize,                   // how many of `codes` are the frames' runs
    base:     usize,                   // the top frame's first register, in words
    pc:       usize,                   // the top frame's next instruction
    top_code: usize,                   // the address of the top frame's code
}
```

The frames and their registers are one buffer of tagged words
(`types/frame_stack.rs`; GC stage 4d.7b, #908). A frame is a header of five
words followed by its window, in `PRD/GC_PRD.md` §11.1's layout:

```text
fp+0   ret      0 in an interpreter's frame
fp+8   link     distance to the caller in bytes [3–34] | nregs [35–50] | flags [51–62]
fp+16  code     the code object's address
fp+24  meta     pc [3–31] | return_reg [35–50]
fp+32  closure  the VmClosure, or CallFrame::NO_CLOSURE
fp+40  r0 … rₙ₋₁
```

- **Every word is a value.** `ret`, `link` and `meta` are fixnums, and so is
  `code` as a collector reads it, since an 8-aligned address has a fixnum's
  tag. A copy of the buffer is traced word by word.
- **`link` is a distance.** A pop subtracts it, and a run of frames copied
  somewhere else is still walked. The bottom frame's distance is its own
  header's size, so a pop is one subtraction for every frame.
- **The side vector keeps code loaded.** The `code` word is an address that
  nothing counts. `codes` holds the `Rc` of the frames' code, copied with
  the frames, in a newtype, `FrameCode`, whose trace reports the code's id,
  which the collector resolves to the unit's link table
  (`VmState::trace_code_ids`, #836); the constants are traced from the code
  store. It holds the object once for each *run*, a stretch of consecutive
  frames that run the same code, and the first frame of a run says so in
  the last flag of its `link` word (bit 62). A frame pushed in the top
  frame's code joins its run and touches the vector neither then nor when
  it is popped; a pop takes the frame's flag from `runs`. So a recursion is
  one run however deep, and a capture of it copies one reference. One
  reference a frame, measured first, cost a count up at every capture and a
  count down at every drop, 14% of `samedepth1000`'s instructions. A
  closure names its code by `CodeObjectId`, a slot in
  `VmState::code_store` and the generation of the code in it (VM_ISA.md
  §2.3), and a call looks it up there and pushes with the code borrowed
  (`VmState::loaded_code_and_execution`). 4e makes the word a reference to
  a code descriptor, and the vector and the flag go.
- **A run that ends is spent, not dropped.** When the first frame of a run
  returns, the run's entry stays in `codes`, past the `runs` entries of the
  frames there are. The next run started there takes it back if it is of
  the same code, as the callee of a loop is at every call, and then no
  reference is counted up or down; a run of other code takes the place and
  a reference, and the spent entry's goes. A tail call between two
  procedures, which is what a `let` in a loop compiles to, changes the top
  run's code with the spent entry's above it. A spent entry still holds
  its code, so the code store drops them before it asks whether a frame
  holds a code object (`FrameStack::drop_spent`, from
  `release_unit_if_unused` and `release_dead_code`): at a form's end and
  after every collection, so a unit is let go exactly when it was, and
  none is kept by a holder that reports nothing to the collection. No copy
  has a spent entry. Measured 2026-11-24 against a vector pushed and popped
  with every run, each counting its frames: that bookkeeping cost a call
  and its return about 20 instructions within one procedure and 30
  between two, and left a row of the benchmark set in fifteen over 1% in
  instructions against `main`.
- **The top frame's pc is a field, not a header word.** The dispatch loop
  reads and advances a pc once an instruction, so the top frame's lives in
  `pc`, beside `base` and `top_code`: three loads from fixed offsets. A
  frame's header takes its pc when another frame is pushed above it, and
  the top frame's header holds whatever it held last. Every reader of a pc
  goes through `FrameStack::frames` or `top`, which answer the field for
  the top frame, so nothing sees the stale word; a copy is a clone, and
  carries the field. `PRD/GC_PRD.md` §11.1 calls the word "pc of the last
  suspension point", and decision A3 on #782 had the interpreter write it
  at every instruction: a pc held in a Rust local would have needed a spill
  at each suspension point, and a missed one would have had the collector
  read the wrong map. Held in the stack's own type, with no reader outside
  it, there is no spill to miss.

`CallFrame` is a read-only view of one frame, which `frames()` and `top()`
answer and nothing stores:

```rust
#[derive(Clone, Copy)]
pub struct CallFrame<'a> {
    pub pc:            usize,           // program counter
    pub register_base: usize,           // the frame's r0, as a word of the stack
    pub num_regs:      u16,             // register window size
    pub closure:       TaggedValue,     // the VmClosure, or CallFrame::NO_CLOSURE
    pub return_reg:    Reg,             // where to write result in caller
    pub code:          &'a Rc<CodeObject>,
}
```

**What a frame costs.** 40 bytes of header and 8 a register on the stack,
which is what the stack's cap counts (§3.2), and 16 bytes of the side
vector for each run of frames that share code.

### 2.2 `VmState`

The complete runtime state, with no public fields. The lower-level Rust entry
points are `VmState::new`, `load_unit`, `release_unit_if_unused` and `execute`;
embedding applications normally use `VmBackend` through `Interpreter`.
`global_env()` and `primitive_registry()` supply compiler inputs, and
`set_tracer()` attaches the existing event/slice-based observer. The environment
and heap still have their own interior-mutability APIs; these accessors do not
make them immutable, but cannot replace the machine's heap/environment pair.
`install_primitives()` remains only for bare compiler/runtime test scaffolding.

The dynamic components in §5.6 live in
[`ExecutionState`](../crates/patina-vm/src/runtime/execution_state.rs): the
register stack, which holds the frames and the registers (§2.1), and the
prompt, wind and handler stacks. Its collections are private even to the
rest of `runtime`. Instruction handlers can inspect frames through views and
write register **slots**, but cannot grow or cut the stack or change a
frame's header. A word of the stack is a `Cell<TaggedValue>`, a register or
a frame's `closure`, so that the root provider reports each one where it
lives, through `&self` (#682). Frame entry, frame removal, tail replacement,
full capture/arrival, prompt landing, and composable append are named
operations on this owner. The transfer algorithms are unchanged.

Every word lies in a frame: the frames tile the register stack, each header
beginning where the window below ends, and the top window ends the stack
(`PRD/GC_PRD.md` §11.1, invariant 1; #714). A debug build asserts it at every
push, pop and `return_into`, and in full after a wind step and a higher-order
primitive's callback. Until #714 two places kept words outside every window:
a full jump's finished wind-step stub left its window on the file as the
only root of the jump's target and value (`finish_wind_step`), and
`run_apply_proc` took a scratch slot one past the caller's window for its
callee's value. A jump's stub frame now stays for its whole travel (§5.3),
and a callback has no destination (§4.4). A composable invoke pops its stub
frame with its window, since it appends to that file. Keep both rules when
adding drivers; see `GC_DESIGN.md` §5.2.

Beside the five components `ExecutionState` holds the id of the code the
innermost running dispatch loop runs (`RunningCode`, #836), traced as a
frame's code is: written where the loop's `cur_code` changes (§3.3), and put
back by `run_loop_until_outcome` when a nested loop ends, so that a
collection at the poll a tail call makes before the next dispatch (at the
callee's entry, or on the return of a tail-called primitive, parameter or
control primitive), while only `cur_code` holds the code it left, still
traces that code's unit's table.

The remaining state belongs to `VmState`: code-unit ownership/recycling,
weak continuation stores, global bindings and their invalidation bits, library
services, GC policy, and transfer bookkeeping. Storage and invalidation fields
are private; only the driver/control pair can see their shared transfer flags,
stub ids, and scratch argument pool. The machine's `globals` are the
program's environment, during a library's load as after it. A library's body
form is compiled in the library's environment, and its unit's link table is
linked there when it loads, so its frame reads, stores and defines the
library's bindings through that table wherever a continuation restores it
(#779, #836; §5.6); `eval`'s code the same way, in the environment it was
given. The body form runs with no closure (`execute` at a top-level
`import`, `execute_nested` on the nested loads), and the closures it makes
are of members of the same unit, which share its table: no closure holds an
environment (#836). Each such
environment is a namespace: the backend's entries refuse a frame with an
error naming its kind (`eval_datum`, #835), and so does the compile of
Scheme `eval`'s and `load`'s code and of a primitive's `Step::Eval`
(`compile_for_eval`, #853), for a frame a host named in a specifier it made
or in a step or `ApplyContext::eval_expr` call of its own primitive, before
that compile expands anything or binds an import. What its caller ran
before is not covered: for an immutable specifier, `eval`'s check for a
definition has already expanded the form in the frame, on either backend,
and a `define-syntax` inside a `begin` passes that check and leaves its
transformer bound there (#855); a `load` of a file with no forms compiles
nothing, and answers without a refusal. Nothing
swaps `globals` any more (#823): `with_globals`, which pointed them at the
library for the body's run and kept what it set aside on the traced
`saved_globals` (#677), is gone. So a program form that a continuation
resumes during a load reads the program's bindings through its own unit's
table, and
so do `interaction-environment` and a one-argument `load`, which takes its
environment from it, as on the tree-walker, chibi and Gauche.
`import_export` still installs shared bindings and invalidates primitive fast
paths in the same operation. Library loading keeps one machine and its existing
re-entry protocol.

#### Mutation audit (#596)

Audited from `9a04c90` (2026-09-30). `VmState` had 37 exposed fields: 15
public and 22 crate-visible. It now has **zero public or crate-visible fields**;
shared runtime bookkeeping is restricted to `runtime`, with the five coupled
collections private to `ExecutionState`. Its public inherent methods fell from
15 to 7; the free nested-execution entry also became runtime-only.

| Consumer before | Mutation or inspection | Access after |
|---|---|---|
| `backend.rs` | filesystem, two library registries, tracer; swap globals for a library body; inspect code slots and compiler registry | paired library configuration, tracer attachment, `with_globals` (removed by #823), count/compiler accessors |
| `runtime/vm_state.rs` | dispatch PC/register writes, returns, extent steps, top-level error reset, library/eval swaps | `ExecutionState` operations; scoped globals (until #823 removed the swap); private code, invalidation and continuation-store methods |
| `runtime/control.rs` | closure/stub frame allocation and tail reuse; prompt/handler/wind stacks; capture, full arrival, abort prefixes, composable relocation | same control policy over `ExecutionState` operations; fixed heap/registry handles |
| `runtime/gc_roots.rs` | read live roots; prune weak stores through `RefCell` | moved beneath `vm_state`; read-only execution slices, private weak-store pruning. One write since: the collection's walk of the frames clears each dead register through its `Cell` (#906, §3.2), as the retirement pass in this file did before it |
| `tests/callprimitive.rs` | configure compiler fixture environments; inspect primitive registry | `global_env()` / `primitive_registry()`; assertions unchanged |
| `tests/smoke.rs`, `tests/gc_weak_continuations.rs` | allocate/free raw register vectors; pin weak-table fixtures in registers and inspect store lengths | white-box fixtures moved beneath `vm_state`; matched frame/window setup, same weak-store assertions |
| `tracer.rs` and other crates | tracing events/slices, `VmBackend` methods | no mutable state access before or after |

### 2.3 `PromptFrame`

```rust
#[derive(Clone)]
pub struct PromptFrame {
    pub tag:                     TaggedValue,
    pub stack_depth:             Depth,  // the stack's top, a byte offset from its base
    pub handler:                 TaggedValue,
    pub dst:                     Reg,
    pub dynamic_wind_depth:      usize,
    pub exception_handler_depth: usize,  // the boundary an abort unwinds to
}
```

### 2.4 `DynamicWindRecord`

Both backends alias `patina_core::WindRecord<H>`, which owns the common
fields and constructor:

```rust
pub struct WindRecord<H> {
    pub id:       u64,            // unique per `dynamic-wind` call
    pub before:   TaggedValue,
    pub after:    TaggedValue,
    pub handlers: Rc<[H]>,
}
pub type DynamicWindRecord = WindRecord<ExceptionHandler>;
```

The VM's handler includes a frame depth; the tree-walker's does not. Handler
installation, clamping, and relocation remain backend-specific. Cloning a
record shares the saved handler slice. Identities may repeat in a composed
stack, so traversal compares prefixes positionally. Both backends share the
record's trace, derived (`patina_core::Trace`, #686): it reports both thunks
and each handler, through the handler type's own derived trace.

### 2.5 `ExceptionHandler`

```rust
pub struct ExceptionHandler {
    pub handler:       TaggedValue,
    pub stack_depth:   Depth,
}
```

Pushed by `with-exception-handler`, popped on thunk return. Exception handlers
are captured/restored by `call/cc`.

**A depth is a byte offset** (`types/depth.rs`; #903, #908): the register
stack's top, measured from its base, where a prompt or a handler was
established, where a delimited capture starts, and where a dispatch loop
stops. Until 4d.7b it counted frames. A depth read off a stack names a
place between two frames, and so does one moved with the frames it was
recorded among: a carried prompt's, and the handler's a `raise-continuable`
puts back a distance below its stub, whose distance is measured from a
floor that every region able to replay the stub contains (§5.2). One case
moves a depth past the frames it was recorded among: `Depth::relocate`'s
saturating one, for a handler recorded below the prompt its continuation
was delimited by, which `install_thunk_handlers`' clamp can produce. That
depth can fall inside a frame of the stack it lands on. It is only
compared, never cut at: the handler is dropped once the stack's top is at
or below it. Under a count of frames the same case named a frame boundary
of the invoking stack that was no more the handler's own, so where the two
readings differ is in how many of that stack's frames return before the
stale handler goes. No test in the suites tells them apart.

### 2.6 Continuation Objects

```rust
pub struct VmContinuation {
    pub stack:              FrameStack,   // a copy of the whole register stack
    pub dynamic_winds:      Vec<DynamicWindRecord>,
    pub prompt_stack:       Vec<PromptFrame>,
    pub exception_handlers: Vec<ExceptionHandler>,
    pub deliver_reg:        Reg,    // where to write value on invocation
}

pub struct VmDelimitedContinuation {
    pub stack:              FrameStack,   // the frames from the prompt's depth up
    pub dynamic_winds:      Vec<DynamicWindRecord>,
    pub deliver_reg:        Option<Reg>,  // the hole, in the top frame;
                                          // None = nothing captured (identity)
    pub depth_at_capture:   Depth,        // the depth the slice started at;
                                          // the two below are relative to it
    pub prompt_stack:       Vec<PromptFrame>,
    pub exception_handlers: Vec<ExceptionHandler>,
}
```

**A capture copies words** (#908). A full capture clones the register stack:
its words, as one run of stores, and the side vector's runs of code. An
arrival copies them back into the buffer the live stack already has, unless
that buffer is more than four times what the arrival needs: then the buffer
goes and the arrival takes one of its own size, so the stack a deep
recursion grew is freed at the escape out of it, as it was while an arrival
replaced the vectors. A delimited one copies
the words from the prompt's depth to the top, as a stack of its own, whose
outermost frame is written as a bottom frame. Invoking it appends those
words to the live stack and writes one of them, that frame's distance to the
frame it now sits above: the frames between need no fix-up, since each
records a distance and not a position. Until 4d.7b each frame's
`register_base` was rebased on every invoke. The prompts and handlers a
delimited continuation carries still have their depths moved
(`Depth::relocate`).

**Storage:** Continuations are stored in **side tables** on VmState
(`continuation_store`, `delimited_continuation_store`), not directly on the heap.
Opaque handles (`VmContinuationRef { id, bytes }`,
`VmDelimitedContinuationRef { id, bytes }`) are stored as `HeapObjectData`
variants. This avoids circular dependencies between `patina-core` and
`patina-vm`. `bytes` is the snapshot's size, measured at capture
(`VmContinuation::payload_bytes`) and charged to the handle, so a deep capture
costs the collection trigger what it copied (#606).

---

## 3. Execution Loop

### 3.1 Entry Point

```rust
pub fn execute(state: &mut VmState, unit: LoadedUnit) -> Result<TaggedValue, VmError>
```

Takes the unit `VmState::load_unit` answered, pushes its initial frame,
closes the unit's `AssertNoGc` window once the frame holds the code (#836;
`docs/GC_DESIGN.md` §7, the window S1), and runs the frame with
`run_top_frame`, which takes the carrier's driver if no run further out
holds it and calls `run_loop_until(state, 0, driver)`.

### 3.2 Main Loop

```rust
fn run_loop_until(
    state: &mut VmState,
    exit_depth: usize,
    driver: Option<GcDriver>,
) -> Result<TaggedValue, VmError>
```

Runs `dispatch_one_instruction()` in a loop until `frames.len() == exit_depth`.
Catchable errors are routed through exception handlers via `vm_raise_value()`.

`driver` is the carrier's capability to collect (#702). `execute` passes it
when no other run holds it, and the loop lends it to its
polls on `VmState::driver`. A nested entry passes `None`: its loop runs under
a `NoGcScope`, with the outer loop's driver hidden, and never collects
(`docs/GC_DESIGN.md` §7).

The loop does not poll before every instruction (#700). Its GC safe points
are the poll sites of `PRD/GC_PRD.md` §12, which collect only in the loop
that holds the driver, at depth 0:
- a closure's entry, polled by the call paths once the callee's frame is
  complete;
- a generic call's return;
- an escape's landing in a frame the loop owns;
- the loop's own start.

Collections that run inside dispatch:
- the polls themselves, at a closure's entry and a generic call's return;
- `CollectAtCall`'s, a resumable primitive's collection at its call (§4.5).

Each collection traces the register stack by walking the frames (#906,
`gc_roots::trace_frames`): for each frame it reports the frame's code and
closure and the registers the map at the frame's pc keeps and, in the same
pass, overwrites the rest, the
completed temporaries (`docs/GC_DESIGN.md` §5.2, "Temporary retirement").
The safe points themselves no longer retire registers first; they lend the
machine's state, and nothing is cleared where no collection runs. A capture
clears its copy by the same walk, with the hole its `deliver_reg` names
(`gc_roots::retire_snapshot`).

**Interrupts** (#701). A poll also services a posted `SIGNAL`: Ctrl-C's,
through the REPL's SIGINT handler, or a host's, through
`Interpreter::interrupt_handle()` from another thread. Every loop services
it, a nested one beneath a `NoGcScope` too, where a collection stays posted
(`Mutator::take_signal`). The poll answers `VmError::Interrupt`, which the
loop routes as it routes an error an instruction met (`route_error`): raised
non-continuably in the innermost handler as an `&interrupt` condition, or
returned when no handler is installed. The condition is not an error object
(`error-object?` answers `#f`); `(patina interrupt)`'s `interrupt?` answers
`#t`, and one that a declining `guard` raises again with no handler left
leaves as `VmError::Interrupt` (`vm_raise_value`). A form's last returns pass
no poll, so `run_top_frame` takes an interrupt posted during them where the
outermost loop returns, and the form, not the next one, answers it (#700).

**The stack's cap** (#907; `PRD/GC_PRD.md` §11.1; the owner's rulings of
2026-11-24 on #782). The poll is the frame-entry check as well. The poll
word holds the stack's limit in bytes while nothing is posted, and `poll`
compares the stack's top with it (`ExecutionState::top_bytes`: 40 bytes a
frame and 8 a register, what 4d.7's interleaved stack will hold, so the depth
at which the cap is met does not move when that lands).

- **The cap** is the backend's configuration's (`HeapConfig::max_stack`,
  `--stack-max`, `PATINA_STACK_MAX`) or the machine's default, min(8 GiB, a
  quarter of B, a quarter of `RLIMIT_AS`), never under 2 MiB
  (`VmState::set_stack_cap`). `(gc-stats)`'s `stack-limit` reads it.
- **The soft limit** is the cap less 1 MiB (`STACK_RESERVE`). The frame entry
  that takes the stack to it gets `VmError::StackExhausted` from its poll,
  which the loop routes as it routes an interrupt: raised non-continuably in
  the innermost handler as `&stack-exhausted`, an error object whose message
  is "stack exhausted" and whose irritant is the cap, which
  `(patina limits)`'s `stack-exhausted?` recognises; or returned when no
  handler is installed, an uncaught error like any other.
- **The reserve.** The raise moves the limit to the cap, so the handler,
  which runs at the raise site, has the last MiB. So does a `guard`: its
  handler captures a continuation there, which copies the stack (decision A7
  on #782), and escapes.
- **The hard cap.** A frame entry at the cap while the reserve is open is a
  handler that ran through it: `VmError::StackOverflow`, which nothing
  catches. The form is abandoned by the path an uncaught error takes
  (`run_top_frame` clears the machine), with no handler and no after thunk
  run, as #776 records for any abandoned form.
- **Re-arming.** The soft limit is armed again when the stack top is at or
  below it, with no margin (`VmState::rearm_stack_limit`), asked in three
  kinds of place: where a transfer has cut the stack (a continuation's
  arrival, an abort's landing), after a collection (`after_collection`),
  and where a form ends or is abandoned (`run_top_frame`). The after thunks
  of an escape run before it lands, so they still have the reserve; a
  `guard`'s clause runs after it, armed, and if it crosses the limit again
  the next handler out gets a new condition. A margin under the limit would
  leave a `guard` near it with one catchable exhaustion and then the hard
  cap, the design CPython withdrew after 3.9.9.
- **A full continuation carries the reserve's state**
  (`VmContinuation::stack_reserve_open`), and its arrival puts the limit
  back to it (`VmState::stack_limit_after_arrival`), as it puts back the
  handler stack. Captured in the extent of a handler for `&stack-exhausted`,
  it finds the reserve open again; captured anywhere else, the soft limit
  armed. A `guard` whose clauses all decline does the first, to raise the
  condition again from the raise site (R7RS 7.3): with the limit left
  armed, the poll at the re-entry would raise a second `&stack-exhausted`
  in place of the first. The state is carried and not read off the stack's
  height: that every stack above the soft limit is a handler's is an
  argument about where the polls are, and the snapshot's own word needs
  none. An abort's landing keeps the reserve open only where the stack
  under its prompt is above the soft limit, on both of its paths
  (`stack_reserve_open_over`).
- **Pushes that take no poll** ask the cap first (`check_stack_room`): a
  stub's frame, and a delimited continuation's re-entry, which appends its
  whole region (`finish_delimited_invoke`). What would not fit is an
  exhaustion at that call: the catchable condition where the soft limit is
  armed, raised with the limit still armed if the stack is under it, and
  the hard cap where the reserve is open.
- **Inside a primitive's callback** (a nested loop) nothing differs: the
  handler stack is the machine's, so the condition raised there reaches a
  handler installed outside the primitive's call, and the hard cap met
  there is no more catchable than anywhere else. A closure's entry polls
  after its push instead, so until 4d.7 puts the stack in a reservation a
  frame can sit one frame past the cap when the hard cap is raised.
- **The tree-walker** keeps no cap (decision A9 on #782).

`crates/patina-tests/tests/stack_cap_matrix.rs` holds each of these to a
row.

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
dispatch prologue refreshes it only when the frame's code changed, by
comparing its address with the stack's `top_code`, so no `Rc` clone is paid
per instruction. The prologue also reads `pc` (folding in its advance) and
the top frame's `base`. All three are fields of the register stack (§2.1),
so the prologue is three loads, a store and a compare, with no frame to
find; until #908 it computed the last frame's address in a vector of frames
at every instruction. Each change of `cur_code` is mirrored in `ExecutionState`'s
running code (§2.2): the refresh writes the new code's id, and so does
`current_code`, which the loop's start and its escape landing take
`cur_code` from. A self-call or self-return keeps the same code and writes
nothing.

### 3.4 Register Access

```rust
impl VmState {
    pub(super) fn reg(&self, r: Reg) -> TaggedValue;
    fn reg_at(&self, base: usize, r: Reg) -> TaggedValue;      // dispatch loop
}
impl ExecutionState {
    pub(super) fn registers(&self) -> Registers<'_>;           // reads only
    pub(super) fn set_top(&mut self, base: usize, r: Reg, val: TaggedValue);
    pub(super) fn return_into(&mut self, frame: ReturnFrame, r: Reg, val: TaggedValue);
}
```

All index word `register_base + r` of the register stack. `reg_at` takes
the dispatch prologue's hoisted base, and `registers()` lends the stack's
words as a read-only view. Only `execution_state.rs` writes a register, by two paths
and a push (#904):

- `set_top` is the top frame's own write, during its own instruction, with
  the hoisted base. Only instruction arms that cannot push or pop a frame
  before the write may use it.
- `return_into` writes a frame something has returned into, or one below
  the top: the value a `Return` or a tail call delivers after its pop, a
  callee's answer once its callback's frames may have run above the caller,
  a jump's or a composable invoke's arrival, and the register
  `deliver_before_collecting` clears. Its `ReturnFrame` is `Top`, `TopAt`
  (with the caller's hoisted base) or `Caller` (the frame below the top). At
  4d it only writes; stage 7 lowers the stack watermark in it first
  (`PRD/GC_PRD.md` §11.1).
- A push initializes its frame's window: `push_frame`, `push_abort_stub`, a
  call's arguments (`init_arg`, `init_args`), and a stub's operands through
  `set_top`, the stub being the top frame. Every register of the window
  that no argument or operand writes gets an immediate, `FILL` (#714,
  `PRD/GC_PRD.md` §11.1, invariant 1): `NULL` in a plain release build,
  `DEAD_SLOT` in a check build, which `reg_at` refuses, so a read of a
  register the activation never wrote panics there. A fresh window is
  filled whole. A reused one is filled past what it keeps: `tail_replace`
  (a tail call) and `restart_closure` (a self tail call) fill the new
  code's registers past its arguments' (`[0, n)` for `Fixed(n)`, `[0, k+1)`
  for `Variadic(k)`, which `store_args_in_window` writes), one store each,
  and `restart_stub` a stub's next round from the registers it keeps (a
  resume stub keeps the primitive's index, a wind step its target and
  value). Registers a larger earlier occupant left past the new code's
  count are not filled: the new code cannot name them, and retirement
  clears them at every collection and capture, as it walks the frames
  (`docs/GC_DESIGN.md` §5.2, #906).

A check build asserts the choice: `return_into` writes a frame below the top
or a top frame that awaits a value, and `set_top` and a push's writes never
write one that does. A top frame awaits a value from the pop, arrival or
call that makes it so until it dispatches its next instruction
(`ExecutionState`'s `awaiting`).

The primitive-call arms keep the hoisted base across a callee that *can* pop
frames, since a higher-order primitive re-enters the VM and a continuation
can escape out of its callback. An escape unwinds through the call
(`across_reentry`), so the write after it, `return_into(TopAt(base))`, finds
the frames the call started with, and a debug build checks the base.

---

## 4. Call and Return

### 4.1 Call Dispatch Order

`Call` and `Apply` use `call_value`; their tail forms use
`tail_call_value`. Both accept compiled closures, control primitives, ordinary
primitives, parameters, full continuations, and delimited continuations. The
probe orders may differ because these heap variants are mutually exclusive.
Non-instruction callers use `call_any`, which delegates to `call_value` and
reports whether a frame remains to run (§4.5).

### 4.2 `call_closure_resolved`

The dispatcher resolves a compiled closure's code id before reaching this
helper. It checks arity, pushes the frame, its header and its window in one
write at the stack's top (`FrameStack::push`), copies arguments and builds
any rest list. The push first writes the caller's pc into the caller's
header (§2.1). The code is borrowed from the store: the stack counts a
reference only for a run of code whose place holds other code. It is not a general procedure entry
point. The old closure-only `call_closure` wrapper and unused `CallWithPrompt`
instruction were removed with #190; prompt bodies and exception-handler
thunks both use `call_any`.

### 4.3 Tail Call

For `TailCall` to a closure, the current frame is reused:
- Read all arg values into temporaries first
- Give the frame the callee's code (`tail_replace`), or, for the frame's own
  code, its closure (`restart_closure`): pc 0, the window grown if the code
  needs more registers, and every register past the arguments' filled
  (§3.4). The header's `link`, `code` and `closure` words are written under
  one range check, and the frame starts a run of its own from there (§2.1)
- Write args to param slots
- Poll at the callee's entry

### 4.4 Return

On `Return { val }`:
1. Read result value
2. Pop the current frame, header and window together: the stack is cut at
   the frame's header, and the caller's pc and its code's address are taken
   up from its header; the side vector is not read, and the frame's run, if
   it started one, is spent (§2.1)
3. If at exit depth, return result
4. Write result to caller's `return_reg`, through `return_into` (§3.4), where
   stage 7 lowers the stack watermark when the caller is below it. A frame
   pushed with `return_reg` `CallFrame::NO_RETURN` (`u16::MAX`, #714) has no
   destination: a higher-order primitive's callback (`run_apply_proc`),
   whose value the loop it runs on hands back at its exit depth. No window
   reaches `NO_RETURN`, so `return_into` finds it past the end of the
   register stack with the bounds check it makes anyway, and writes no
   register; a continuation that re-enters the callback after the
   primitive has returned returns into nothing. A callee that answers such
   a call here and now (a primitive, `values`, a parameter) hands its value
   to `ExecutionState::answer`, which `call_any` takes back at once
5. Run cleanup: `pop_resolved_prompts()`, `pop_exception_handlers()`. Wind
   records are not swept here — `PopWind` pops each one (§5.3)

### 4.5 Helper Functions

There are **two** probe sets, one per call shape, and they differ in order:

- **`call_value()`** — the `Call` instruction's dispatcher. Closure first (a
  probe hoisted into the caller so the common path pays one type check), then
  control primitive → primitive → parameter → full continuation → delimited
  continuation. It either writes the callee's value into `dst` or leaves a
  frame for the dispatch loop; a control transfer out is an `Err`
- **`tail_call_value()`** — the `TailCall` shape, which pops the frame first.
  Same members, different order — closure, control primitive, primitive, full
  continuation, delimited continuation, **parameter last** — because each
  non-closure arm has its own frame bookkeeping to do. Both functions carry a
  comment on why the order is free to differ (the callable heap variants are
  mutually exclusive), so this is two probe sets by choice, not by drift
- **`call_any()`** — `call_value()` for call sites that have no
  instruction behind them: a `call-with-values` consumer (instruction and tail
  instruction), a prompt body, an exception-handler thunk,
  `call/cc`'s procedure, a jump's wind thunks, a composable invoke's re-entry thunks, a higher-order primitive's
  callback (with no destination, `CallFrame::NO_RETURN`, §4.4), and a
  parameter converter. It adds the one thing those callers
  cannot get from the dispatch loop: whether the callee finished. `Some(v)` is
  a callee that needed no frame, `None` a frame still to run — and the **frame
  depth** is what decides, since no `Ok` path through `call_value()` shortens
  the stack. It carried its own narrower probe set until 2026-09-05 (no control
  primitive: issue #186), so `(call-with-values (lambda () (values + '(1 2)))
  apply)` failed at a name lookup
- A parameter *set*, `(p v)`, is `%parameter-set!`, a resumable primitive
  (`patina_primitives::Step`): the call paths send it there before
  `try_call_parameter` sees it, and its converter runs in `resume_stub`'s
  frame. It ran on a nested loop through `call_any_sync()` until #478
- A resumable primitive that asks for a collection at its call
  (`Step::Collect`, #639; `(gc)`, and #607's opens that ran out of
  descriptors, `load`'s read among them, which collect and retry) runs it in `resume_stub`'s
  collecting variant, `collect_stub`: `CollectAtCall` /
  `ResumePrimitive` / `Return`, with the same register window, so one frame
  can go from a call to a collection and back. The caller waits beneath it
  at the call's return pc. `CollectAtCall` writes into the caller's register
  file before any `Return`: it clears the register the call's value goes to
  (deliver-first, `deliver_before_collecting`), because a liveness map keeps
  a call's destination as a root from the call on. Then it collects through
  `GcController::collect_at_call`, or posts the collection where it may not
  collect (a nested loop, a holder's extent). It skips the clearing when the
  stub frame sits at the loop's `exit_depth`, whose frame below belongs to
  another loop. The stub frame keeps the primitive's state and nothing of
  the call: its callee, argument and result registers are filled by the
  push or the restart (§3.4), so they keep nothing alive through the
  collection
- A `with-exception-handler` thunk that finishes without a frame closes its
  handler extent by truncating to the handler-stack length recorded before
  installation. A prompt body does the same for prompts. Neither can rely on
  a `Return`, and neither may blindly pop the last entry after re-entry.

### 4.6 Shared policy and runtime boundaries

`patina_core::continuation::next_wind_step` is the shared traversal policy for
full continuation jumps and abort landings. Given outermost-first stacks and
per-invocation identities, it selects one action: exit the innermost unshared
extent, enter the next target extent, or arrive. Both backends call this helper;
neither keeps a second common-prefix algorithm. It allocates no state and
calls no Scheme code. Recompute the action after each thunk, because a thunk
may replace the transfer being performed.

The backends own execution of that action: pop before calling `after`; call
`before` before pushing its record; run under the record's handlers; preserve
live prompts until arrival. The VM stores the remainder in a `ResumeWindJump`
frame and the tree-walker in `ContValue::Jump`. The remainder must be captured
with a continuation made inside the thunk. At arrival each backend installs
its own representation of the target's frames/chain, values, winds, prompts,
and handlers. Frame indices, CPS environments, prompt relocation, GC roots,
and escape signals remain backend responsibilities.

Composable invocation is a separate operation: it appends a captured region
and returns to its invoker. Its entry thunks run under the invoke site's
handlers, so it must not use the replacement traversal. A raise also does not
traverse winds: it calls the handler in the raise site's dynamic environment.

Rust callbacks are a runtime boundary. The VM's `across_reentry` and pending
transfer signal, and the tree-walker's trampoline identity, distinguish a
callback return from an escape. Native Rust computation is not itself saved
by a Scheme continuation. In particular, the tree-walker rejects re-entry to
an expired callback trampoline and invocation of a composable capture crossing
a callback;
sharing traversal does not remove these representation limits. Any operation
that promises replay of work after a Scheme call must encode that remainder
in machine frames or CPS continuation values, rather than only on the Rust
stack. `escape_from_primitive.rs` guards the callback boundaries; the matrix
in §5.6 guards wind ordering and continuation result delivery.

### 4.7 The VM control runtime boundary (#174)

[`runtime/control.rs`](../crates/patina-vm/src/runtime/control.rs) is the
contract for code calling VM procedures or transferring control. Its module
documentation specifies valid frame/register inputs, normal versus scheduled
results, the parked-escape protocol, and GC deferral obligations. Each
stateful helper states its entry assumptions and effects. This is an internal
Rust interface for a tier using the existing explicit Scheme frames and
registers; it is not a native-code ABI or an implementation of a compiled tier.

The boundary owns call dispatch (including closure and registry fast paths),
parameter and primitive callbacks, raise, prompts, continuation capture/invoke,
wind traversal, and runtime stub construction. Stub builders and internal
probes remain private. Only the entry points and resume-register layouts used
by the driver are visible within `runtime`; the compiler separately reads the
control-primitive identity table. `vm_state.rs` retains `VmState`, instruction
dispatch, loop ownership/error recovery, GC safe points, library evaluation,
and global-binding invalidation; `execution_state.rs` owns the five coupled
collections and their structural updates (§2.2). The control module calls its explicit loop
and host-evaluation services rather than owning a second driver.

The key obligation for another driver is to propagate an escaped call before
writing a result or cleaning up the abandoned dynamic extent. The owning
loop checks `pending_escape` before error classification, because primitive
error conversion can wrap the sentinel. Only the loop that will run the
landing consumes it and clears `pending_transfer`. The authoritative sequence
and the rooting requirements are together in the module documentation;
`VmState` field comments and `Resume*` instruction comments describe storage
and operands, not a separate protocol.

---

## 5. Control Primitives

All continuation and exception operations are handled as VM control primitives,
intercepted at call dispatch time.

### 5.1 `VmControlPrimitive` Enum

| Variant | Scheme form | Behavior |
|---|---|---|
| `DynamicWind` | `dynamic-wind` | Push wind record, run body, pop on return |
| `CallWithContinuationPrompt` | `call-with-continuation-prompt` | Push prompt, call the body through `call_any` — any procedure a `Call` instruction accepts, control primitives included (§5.5) |
| `AbortCurrentContinuation` | `abort-current-continuation` | Find prompt, capture delimited cont, unwind, call handler |
| `CallWithCurrentContinuation` | `call/cc` | Snapshot full stack, deliver to proc |
| `Values` | `values` | Store in `value_buffer`, return primary value |
| `CallWithValues` | `call-with-values` | Push a `value_cwv_stub` frame: call the producer, then tail-call the consumer with its values |
| `WithExceptionHandler` | `with-exception-handler` | Push handler, run thunk, pop on return |
| `Raise` | `raise` | Pop handler, push `raise_step_stub` — **no unwind** (§5.2) |
| `RaiseContinuable` | `raise-continuable` | Like Raise but handler returns to raise site |
| `Error` | `error` | Construct error object, then Raise |
| `Force` | `force` | A done promise's value, or a non-promise itself; a delayed promise's thunk runs in `force_stub`'s frame (`Call thunk` / `ResumeForce` / `Return`, #476) |
| `Exit` | `exit` | Jump to an empty target outside every extent: the travel runs each outstanding after thunk (§5.3), and arrival ends the process (#336) |

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
  the jump. The stub frame stays for the whole travel (#714): the first step
  pushes it with the target and value (`WindStub::Push`), each later step
  restarts it in place at pc 0, keeping both and filling the rest
  (`WindStub::Reuse`, `restart_stub`), and arrival replaces it with the
  target's frames. Until #714 `ResumeWindJump` popped it and kept its window
  on the register stack as the two values' only root, a word outside every
  frame's window, `NUM_REGS` of them a step until arrival. Thunks ran on a
  nested Rust call until 2026-09-02, and a continuation captured in one
  re-entered the *jump site's* frame instead — mid-sequence, at its
  `PopWind` (Track L §6, the `finally` rule)
- The handler stack a thunk runs under comes from its record, not from the
  machine. `install_thunk_handlers` clamps the recorded frame depths to the
  depth the stub frame was pushed at first, which is the same at every step;
  `pop_exception_handlers` reads them, and they belong to a stack that has
  moved on
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
- The value form of `call-with-values` does the same since 2026-09-23 (#442):
  a stub frame running `value_cwv_stub`'s `Call producer` /
  `TailCallWithValues consumer`, head position's tail sequence. It ran the
  producer on a nested loop until then, the last control primitive to run a
  thunk of the program's that way, and a continuation captured in the
  producer did not come back through the consumer — `(k 1 2)` inside it
  answered `#<procedure>`, and a re-entry after the consumer had run, `()`
- Wind records are therefore not swept by frame depth, and carry none. A
  `pop_resolved_winds()` that ran the after-thunk of a record whose body had
  returned existed until 2026-09-02 as the value form's last line of cleanup;
  with `PushWind`/`PopWind` doing the work it was reachable on no input

### 5.4 Values / Call-with-values

- `Values` stores all args in `value_buffer` unconditionally
- For single value, also writes to `dst` register directly
- For multiple values, allocates a heap `Values` object for display
- `CallWithValues` and `TailCallWithValues` unpack the producer's result: a
  `#<values>` heap object's elements, or the one value. The value form runs
  the same `TailCallWithValues` from `value_cwv_stub` (§5.3)

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
(`pop_resolved_extents`) — with one gap: that sweep does nothing at a dispatch
loop's own exit depth, by design, and the only backstop there is
`run_loop_until_outcome`'s `exception_handlers.truncate(handlers_at_entry)`,
which covers handlers and not prompts.

**A full continuation's snapshot can carry a prompt past its own body**, and
that one is closed on arrival (issue #176, `restore_continuation`). The
program that needs it is the one no depth can classify: `(call/cc …)` in
**tail position** of a prompt body. The tail call pops the body's frame before
the capture, so the prompt's `stack_depth` already equals `frames.len()` while
the body is still running — an abort at that moment must still find the
prompt, and Guile 3.0.11 and Racket 9.3 both answer that it does — and the
identical reading holds once the body's value has been delivered and the
prompt is over. It is the frame-depth twin of the recorded
`exception_handler_depth` in §5.6's last paragraph: a tail call erases the
frame a depth test needs.

Arrival is the one moment the reading *is* exact, which is why the sweep sits
there and not at a loop's exit. The value is being delivered right then, into
the frame the snapshot restored, so a prompt with no frame above it has
already delivered its own — the only thing that could still have delivered it
was the frame that is gone. Left in place, no later sweep reaches it, because
no `Return` will ever cross its depth again.

A loop-exit backstop was tried first and is not enough: it closes the
two-top-level-form spelling of the issue and leaves the same sequence inside a
single `let` body dying exactly as before, since no loop returns between the
re-entry and the abort. Both spellings are pinned in `tests/scheme/control/prompts.scm`.

#### Who closes a prompt

Three answers, and the third is the one that is easy to miss.

- **The depth sweep**, for a body with a frame: its `Return` runs
  `pop_resolved_extents`, which pops every prompt whose `stack_depth` the
  stack has shrunk past.
- **The dispatch loop**, for a prompt opened inside it: `run_loop_until_outcome`
  truncates `prompt_stack` to its length at entry, beside the same truncation
  for `exception_handlers`, because the sweep at a loop's own exit depth
  deliberately pops nothing. A parameter converter that opens a prompt reaches
  this with no prompt body in sight.
- **`call-with-continuation-prompt` itself**, for a body that finishes without
  a frame — a primitive, a parameter object, an identity delimited
  continuation (issue #179). No `Return` is coming, so nothing else can. It
  closes by **recorded index**, not by popping the top: such a body may
  re-enter the VM and leave a prompt above this call's, and popping blind took
  that one and left this call's live for the next abort to land on.

A prompt a *continuation's snapshot* carries past its own body is a fourth
case, closed on arrival — see §5.5's note on issue #176.

---

#### Aborting out of a Rust primitive's callback

A primitive that calls back into Scheme — `eval`, a `parameterize` converter,
and when #177 was found also `force`, `map` and `assoc` with a comparator
(Scheme since #471, and `force` a VM control primitive since #476) — runs a
**nested dispatch loop** under a Rust frame it will lose if control leaves.
`across_reentry` guards those boundaries. Since #475 a continuation that
arrives from outside one leaves it (`VmState::reentry`); when #177 was found
the test was frame depth alone — a stack shorter than the one the call
started with means the frames the Rust code owned are gone.

**An abort is the shape that depth cannot see** (issue #177). It cuts every
stack back to its prompt and pushes one stub frame that has yet to run, so
when the prompt sits one frame below the boundary the landing sits at exactly
the depth a callback returning normally would leave. Written in tail position
— `(call-with-continuation-prompt (lambda () (force (delay
(abort-current-continuation t 'v)))) t handler)`, where the tail call pops the
body's frame before `force` runs — `force` read that as its thunk returning,
cached the abort's value as the result, and ran on over the registers the
landing was about to use.

The frames genuinely cannot say which happened, because the opposite case
leaves the identical stack: a continuation captured **and** invoked inside a
callback delivers its value into the register the call is waiting on, which is
a return by another route, and the primitive must run on (`member` with such a
comparator, pinned in `escape_from_primitive.rs`). So the transfer says it
itself — `VmState::pending_transfer`, set by `abort_to_prompt` and cleared by
whichever loop resumes into the landing (and by `execute`, so a form cannot
leave one latched for the next).

**`across_reentry` reads it, which is every boundary at once.** It was read
per-boundary first, and that covered one of four: `force` worked while `eval`
kept the old failure verbatim and a tail-position parameter *set* stored the
abort's value into the parameter and never ran the handler. `parameterize` was
the wrong thing to measure it against — that reaches its converter through
`apply_proc`, the boundary already fixed. `across_reentry`'s own doc had
already said where new boundaries go, having been written after the *previous*
one-of-four fix in the same function.

**Every step of an abort's travel sets it, not only the first** (issue #342).
An abort with a `dynamic-wind` to leave cannot cut the machine back in place:
it describes the landing as a jump target and travels there, one after thunk
at a time. A dispatch loop that runs one of those thunks is resuming into a
frame it owns, so it clears `pending_transfer` on the way in, and the step
that arrived used to park as a plain escape. When the landing was below the
boundary, `across_reentry` saw a callback's return depth and no transfer:
`force` cached the abort's value in tail position, and in non-tail position
wrote its result into a register window the landing had removed, which
panicked. The landing is marked (`VmContinuation::abort_landing`), and
`ResumeWindJump` parks each step of a travel to one with `park_transfer`.

`escape_from_primitive.rs` sweeps every re-entrant primitive by **both**
transfers now, each crossing nothing and crossing a `dynamic-wind`, in tail
and non-tail position, with a prompt handler whose answer is not the value
the abort carries. That is what makes the next missed boundary visible.

---

### 5.6 The dynamic-state matrix

`VmState` owns an `ExecutionState` carrying five components that belong to a
*dynamic extent* rather than to the machine: `frames`, `registers`, `dynamic_winds`, `prompt_stack`,
`exception_handlers`. Every control transfer has to say what it does with each
one, and a transfer that forgets a component does not fail loudly — it runs
under somebody else's dynamic context.

This table is the whole list. It exists because the defects in this area were
found one at a time by hand-written repros (#157, #159, #160, #162, #163) while
the chibi suite stayed 1226/1226 through all of them; an empty cell is findable
by reading, before anyone writes a program that trips over it.

Its executable counterpart is
`crates/patina-tests/tests/control_flow_matrix.rs`, which enumerates the
*transfers* rather than the state: 64 shapes over how a `dynamic-wind` is
written, whether it is in tail position, and how control leaves or re-enters
it. Capture sites include ordinary body/before/after execution, thunks run
during full-jump and abort travel, and composable capture/resume from each
site; the matrix header maps all 16 transfer variants to those contexts. Each
row records **which** external implementations back its answer, from four
supporting oracles down to one. Unsupported programs and an observed Chibi
disagreement are documented in the matrix header rather than counted as
agreement. Where this table catches a
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
| resumable primitive suspends (`Step::Call`, `Eval`, `Collect`) | push stub | for a `Collect` only: the caller's destination cleared, unless the stub sits at `exit_depth` | — | — | — |
| wind thunk (jump, abort) | push stub | — | — | — | replaced by the record's own |
| wind thunk (invoke re-entry) | — | — | — | — | the invoke site's |
| wind thunk (ordinary call) | — | — | — | — | the live stack |
| `exit` | *travels to a target with nothing in it* — the `full invoke` row, except that arrival ends the process | | | | |

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
- **`exit` is a jump whose target is empty** (#336). Its travel leaves every
  extent the way any jump leaves them, one after thunk per step under its own
  record's handler stack, and `step_wind_jump` ends the process on arrival
  instead of installing a machine. So an after thunk that escapes abandons the
  exit, one that calls `exit` starts a new travel from where the first had got
  to, and an error nothing handles while the travel's stub frames are still on
  the stack is reported and still ends the process: `execute` finds the exit
  in the frames (`exit_in_progress`) and notes it for the runner.
- **An escape out of a callback is told by the continuation, not by the
  frames.** Every Rust primitive that calls back into the VM does it across
  a re-entry boundary (`across_reentry`), each with an id
  (`VmState::reentry`), and a full continuation records the boundaries it
  was captured inside (`VmContinuation::reentry`). On arrival, a
  continuation leaves every boundary it was not captured inside
  (`VmState::reentry_kept`), and each loop and boundary inside those unwinds
  rather than resuming, whatever frames it restored. Frame depth decided it
  until 2026-09-23 and could not in three shapes: a continuation captured
  before the primitive was called and one its callback captured after a tail
  call popped the callback's frame restore the very same machine — the first
  leaves, the second is the callback returning (#469, #472, and #474, where
  the second left its value parked for the next error to be taken for); and
  one captured outside at a deeper stack restores more frames than the
  callback's loop started with, which ran them inside the callback and
  panicked (#473). A continuation captured inside a boundary that has since
  returned is not an arrival from outside; the depths still decide where it
  resumes, which is right for an earlier form of the same `load` and cannot
  be right for a callback whose primitive is done: no continuation carries a
  Rust frame. So the procedures that call back into the program are Scheme
  (#471; `member`, `assoc`, `call-with-port` and the file variants, beside
  `map` and `for-each`) or, on the VM, run the callee as a frame of the
  machine (`force`, a control primitive with a stub since #476; a parameter's
  converter, the call of a resumable primitive in `resume_stub`'s frame since
  #478; `eval`'s and `load`'s code, a closure over the compiled datum that
  the same frame calls since #477). #420
  had already made `tail_call_value` run a primitive before popping the
  frame, as any tail callee runs, and routed a `call-with-values` consumer
  and `apply` as a value through it
- **A restored frame resolves its globals through its code's link table.** A
  continuation restores frames, not environments, so a frame reads, stores
  and defines the bindings of the namespace its code was compiled in through
  the table its unit linked there at load (#836), or, for a pinned entry,
  the record pass 5 preset in it (#837), wherever it is restored,
  closure or none. Top-level code runs with no closure wherever it was
  compiled — a program's form, a library's body form, a host namespace's
  form (`push_top_frame`) — and so do the stubs, and `MakeClosure` reads
  nothing but its registers: a closure holds no environment, and its code
  reads through its unit's table, which a unit's members share. Until #836
  a body form or a host form ran under a retired closure over its
  environment (`push_frame_in`), which `MakeClosure` read. A library body
  form re-entered after its load read the program's
  until #779's part 1 (`test_reentering_a_library_body_form_after_the_load`
  in `escape_from_primitive.rs`); what the rest of the body does after it is
  part 2's.
- **A resumable primitive's suspension touches no dynamic stack.** Its stub
  frame (`resume_stub`, or `collect_stub` for a collection, §4.5) is an
  ordinary frame over the caller, so a continuation captured in the call it
  makes carries the frame and resumes the primitive again (#477, #478). The
  one write to the caller's registers before the primitive's value arrives
  is a `Collect`'s: the caller's destination is cleared before the
  collection (#639). Nothing reads it before the value arrives, and a
  continuation that re-enters the caller restores its registers from its
  own snapshot.
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
same matrix rows and the same prompt rows (`tests/scheme/control/prompts.scm`,
from `cps_features.rs` until #193 Phase 2) score it, so the column is one
answer reached two ways.

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
done for winds. Both shapes are pinned in `tests/scheme/control/prompts.scm`,
"an abort uninstalls the handlers of the region it abandons" and the row after
it.

---

## 6. GC Roots

| Location | What it holds |
|---|---|
| `vm.execution`'s register stack | Each frame's closure and live registers, and its code by id; every word lies in a frame (§2.1, §2.2) |
| `vm.execution.answer` | The value a callee answered to a call with no destination, until `call_any` takes it (§4.4) |
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
