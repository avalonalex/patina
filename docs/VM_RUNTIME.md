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
    pub tag:                TaggedValue,
    pub stack_depth:        usize,
    pub handler:            TaggedValue,
    pub dst:                Reg,
    pub dynamic_wind_depth: usize,
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
    pub frames:          Vec<CallFrame>,
    pub dynamic_winds:   Vec<DynamicWindRecord>,
    pub registers:       Vec<TaggedValue>,
    pub base_at_capture: usize,
    pub deliver_reg:     Option<Reg>,  // the hole, in frames.last();
                                       // None = nothing captured (identity)
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
| `Raise` | `raise` | Pop handler, push handler frame — **no unwind** (§5.2) |
| `RaiseContinuable` | `raise-continuable` | Like Raise but handler returns to raise site |
| `Error` | `error` | Construct error object, then Raise |

### 5.2 Exception Handling

- `ExceptionHandler` stack on VmState, pushed/popped by `WithExceptionHandler`
- `vm_raise_value()`: pops the handler and pushes the handler frame. It does
  **not** touch the wind stack: R7RS 6.11 calls the handler "in the dynamic
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
- Composable/delimited invokes do **not** travel at all: `invoke_delimited`
  runs every captured `before` thunk unconditionally, on a nested call and
  under the live handler stack. Shared extents are not skipped there. Each
  record is pushed as its own thunk returns, `step_wind_jump`'s rule and for
  its reason: a thunk that raises leaves the extents before it entered *and*
  recorded, so their `after` thunks are still owed, and its own neither
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
2. Capture frames `[prompt.stack_depth..]`, registers, winds
3. Run wind exit thunks
4. Unwind stack to prompt depth
5. Call handler with `(value, continuation)`

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
