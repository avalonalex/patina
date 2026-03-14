# Patina VM: Instruction Set Architecture

**Status:** Draft v0.1 — open for discussion
**Depends on:** [VM_DECISIONS.md](./VM_DECISIONS.md)
**Feeds into:** VM_COMPILER.md, VM_RUNTIME.md (drafts pending)

---

## 1. Overview

Patina VM is a **register machine**. Each function activation owns a contiguous
window of registers in a shared array. Instructions reference registers by index
within the current window. There is no operand stack.

**Value type:** `TaggedValue` throughout — same type used by the tree-walker and
`patina-primitives`. No conversion overhead. See VM_DECISIONS.md §2 and §10.

**Key references consulted:**
- Gauche `vminsn.scm` / `vm-opcode-map.scm` — instruction variant design, tail-call frame management
- Racket BC `eval.c` — 5-pass compiler pipeline, goto-based tail calls
- ChezScheme `pb.c` / `scheme.h` — single value type, portable bytecode model
- `VM_CALLCC_DESIGN.md` §5 — stack-snapshot strategy B (settled decision)

---

## 2. Machine Model

### 2.1 Register Windows

```
Global register array:  [ r0 | r1 | r2 | ... | rN ]
                          ^                        ^
                          frame.register_base       frame.register_base + frame.num_regs
```

- Each `CallFrame` owns a slice `[register_base .. register_base + num_regs]`.
- Register `r0` in a frame = global index `frame.register_base + 0`.
- Instructions always use **frame-relative** indices (u16).
- The compiler assigns registers at compile time; no runtime allocation.

### 2.2 Call Frame

```rust
#[derive(Clone)]   // ← non-negotiable invariant (VM_DECISIONS.md §4)
pub struct CallFrame {
    pub code:          CodeObjectId,   // which function is executing
    pub pc:            usize,          // program counter (instruction index)
    pub register_base: usize,          // offset into global register array
    pub num_regs:      u16,            // size of register window
    pub closure:       Option<HeapIndex>, // captured free vars (if closure)
}
```

`CallFrame` must always be `Clone`. This is the foundation for stack-snapshot
continuations (see §6).

### 2.3 Code Object

```rust
pub struct CodeObject {
    pub id:          CodeObjectId,
    pub name:        Option<Symbol>,       // for debugging/error messages
    pub instructions: Vec<Instruction>,
    pub constants:   Vec<TaggedValue>,     // constant pool
    pub num_regs:    u16,                  // registers needed
    pub arity:       Arity,                // fixed / variadic
    pub source_map:  Vec<(usize, SourceLocation)>, // pc → source loc
}

pub enum Arity {
    Fixed(u16),          // exactly N args
    Variadic(u16),       // at least N args; rest collected into a list
}
```

### 2.4 Flat Closures

```rust
// Stored in the heap as HeapObjectData::VmClosure
pub struct VmClosure {
    pub code:      CodeObjectId,
    pub free_vars: Vec<TaggedValue>,  // captured values, indexed by slot
}
```

Free-variable analysis at compile time determines which variables are captured.
Mutable captured variables (`set!` after capture) are boxed in a
`HeapObjectData::MutableCell` at the binding site; the cell pointer is what gets
captured.

---

## 3. Instruction Encoding

Each instruction is a Rust enum variant. The VM does not use a binary wire format
in Phase 2A — instructions live as `Vec<Instruction>` in `CodeObject`. A compact
binary encoding can be introduced later without changing semantics.

```rust
pub type Reg = u16;         // register index (frame-relative)
pub type ConstIdx = u16;    // index into CodeObject::constants
pub type CodeId = u32;      // CodeObjectId
pub type Label = usize;     // instruction index (absolute within CodeObject)
```

---

## 4. Instruction Set

### 4.1 Load / Store

| Instruction | Operands | Semantics |
|---|---|---|
| `LoadConst` | `dst: Reg, idx: ConstIdx` | `dst ← constants[idx]` |
| `LoadImmediate` | `dst: Reg, val: TaggedValue` | `dst ← val` (fixnum/bool/char/null inline) |
| `LoadLocal` | `dst: Reg, src: Reg` | `dst ← src` (register copy) |
| `LoadClosure` | `dst: Reg, slot: u16` | `dst ← current_closure.free_vars[slot]` |
| `StoreClosure` | `slot: u16, src: Reg` | `current_closure.free_vars[slot] ← src` (for `set!` on captured vars via MutableCell) |
| `LoadGlobal` | `dst: Reg, name: Symbol` | `dst ← globals[name]` (error if unbound) |
| `StoreGlobal` | `name: Symbol, src: Reg` | `globals[name] ← src` |
| `Move` | `dst: Reg, src: Reg` | `dst ← src` |

**Note:** Locals and temporaries are both plain registers. `LoadLocal`/`Move` cover
both. The compiler decides which slot to use.

### 4.2 Closure Creation

| Instruction | Operands | Semantics |
|---|---|---|
| `MakeClosure` | `dst: Reg, code: CodeId, free_vars: Vec<Reg>` | Allocate `VmClosure { code, free_vars: regs.map(|r| reg[r]) }`, store heap pointer in `dst` |

`free_vars` is a list of register indices whose current values are captured. The
compiler emits one `MakeClosure` per lambda expression. The register list is
determined by free-variable analysis.

### 4.3 Control Flow

| Instruction | Operands | Semantics |
|---|---|---|
| `Jump` | `target: Label` | `pc ← target` |
| `JumpIf` | `cond: Reg, target: Label` | `if reg[cond] != #f: pc ← target` |
| `JumpUnless` | `cond: Reg, target: Label` | `if reg[cond] == #f: pc ← target` |

All Scheme values except `#f` are truthy (R7RS §6.3).

### 4.4 Function Calls

#### `Call`

```
Call { func: Reg, args: Vec<Reg>, dst: Reg }
```

1. Evaluate `func` register — must be a closure or primitive.
2. Push a new `CallFrame` with `return_reg = dst` in the caller's frame.
3. Copy `args` registers into the new frame's parameter slots (`r0, r1, ...`).
4. Set `pc = 0`, begin executing callee.
5. On `Return { val }`: pop frame, write `val` into caller's `dst`.

Variadic functions: excess args beyond the fixed arity are collected into a list
and placed in the rest parameter slot.

#### `TailCall`

```
TailCall { func: Reg, args: Vec<Reg> }
```

1. Evaluate `func` register.
2. **Reuse the current `CallFrame`**: update `code`, `pc = 0`, rewrite parameter
   registers in-place (within the same register window).
3. Do **not** push a new frame. Stack depth stays constant for tail-recursive loops.

For tail calls to a different function (not self): the current frame's register
window may need to grow. If `func`'s `num_regs > current frame num_regs`, the VM
extends the window (details in VM_RUNTIME.md).

#### `Return`

```
Return { val: Reg }
```

1. Read `reg[val]`.
2. Pop the current `CallFrame`.
3. Write the value into the return register of the caller's frame.
4. Resume the caller at its `pc`.

#### `CallPrimitive`

```
CallPrimitive { func: PrimitiveFn, args: Vec<Reg>, dst: Reg }
```

Calls a `patina-primitives` function directly without pushing a `CallFrame`.
Primitives are Rust functions with signature `fn(&SharedHeap, Vec<TaggedValue>) ->
Result<TaggedValue, EvalError>`. The compiler emits `CallPrimitive` when the callee
is statically known to be a registered primitive.

### 4.5 Multiple Values

R7RS `values` / `call-with-values` support:

| Instruction | Operands | Semantics |
|---|---|---|
| `ReturnMulti` | `vals: Vec<Reg>` | Return multiple values to caller |
| `ReceiveValues` | `dsts: Vec<Reg>` | Bind incoming multiple values to registers; error if count mismatches |

### 4.6 Continuations (SRFI-226)

These three instructions are the complete continuation ISA. Everything else
(`call/cc`, exceptions, generators, `dynamic-wind`) is built on top of them.

#### `CallWithPrompt`

```
CallWithPrompt { body: Reg, tag: Reg, handler: Reg, dst: Reg }
```

Maps to SRFI-226 `call-with-continuation-prompt`.

1. Record a **prompt frame** on the prompt stack: `{ tag, stack_depth: frames.len(),
   handler, dst, dynamic_wind_depth }`.
2. Call `body` thunk (no arguments) as a normal `Call`.
3. If `body` returns normally: pop prompt frame, write result to `dst`.
4. If `AbortToPrompt` fires with matching `tag`: unwind frames to `stack_depth`,
   call `handler` with the aborted value and the captured delimited continuation.

#### `AbortToPrompt`

```
AbortToPrompt { tag: Reg, val: Reg }
```

Maps to SRFI-226 `abort-current-continuation`.

1. Search prompt stack for the nearest frame with matching `tag`. Panic if none.
2. Capture frames from `stack_depth` to current top as a `VmContinuation`
   (heap-allocated, cloned `Vec<CallFrame>` slice).
3. Unwind the call stack to `stack_depth`.
4. Invoke the prompt's `handler` with `(val, captured_continuation)`.

#### `CaptureComposable`

```
CaptureComposable { dst: Reg, tag: Reg }
```

Maps to SRFI-226 `call-with-composable-continuation`.

1. Find the nearest prompt with matching `tag`.
2. Snapshot frames from that prompt's `stack_depth` to current top into a
   `VmContinuation`.
3. Store continuation in `dst`. Execution continues normally (unlike
   `AbortToPrompt`, the stack is **not** unwound).

#### `InvokeContinuation`

```
InvokeContinuation { cont: Reg, val: Reg, composable: bool }
```

- **Composable** (`composable = true`): append the continuation's captured frames
  to the current stack, then push a `Return`-like frame that delivers `val`.
- **Non-composable** (`composable = false`): run `dynamic-wind` exit hooks for
  current winds not in the captured continuation, replace `vm.frames` with the
  cloned frames, run entry hooks, resume.

**Derived forms built from these four instructions:**

| Scheme form | Built from |
|---|---|
| `call/cc` | `CaptureComposable` at default top-level prompt + `InvokeContinuation` with `composable=false` |
| `call-with-composable-continuation` | `CaptureComposable` + `InvokeContinuation` with `composable=true` |
| `raise` / `error` | `AbortToPrompt` with the exception prompt tag |
| `with-exception-handler` | `CallWithPrompt` with the exception prompt tag |
| `dynamic-wind` | `before` / `after` thunks recorded on a per-frame wind stack; `InvokeContinuation` runs them |
| `shift` / `reset` | Standard encoding via composable continuations |

### 4.7 Global Definitions

| Instruction | Operands | Semantics |
|---|---|---|
| `Define` | `name: Symbol, src: Reg` | `globals[name] ← reg[src]` (top-level `define`) |

### 4.8 Miscellaneous

| Instruction | Semantics |
|---|---|
| `Nop` | No operation. Used for patching. |

---

## 5. Tail Call Semantics

Tail call elimination is **mandatory** per R7RS §6.11 — "implementations are
required to be properly tail-recursive."

The compiler detects tail position during `CoreExpr → bytecode` compilation:
- The body of a `Lambda` is in tail position.
- The last expression of a `Begin` in tail position is in tail position.
- The consequent and alternate of an `If` in tail position are in tail position.
- An `App` in tail position emits `TailCall`; otherwise emits `Call`.

`TailCall` reuses the current frame so the call stack does not grow for
tail-recursive loops. A correct mutual-tail-recursion between `f` and `g` runs
in O(1) stack space.

---

## 6. Continuation Semantics (call/cc and SRFI-226)

### 6.1 Stack Snapshot Model

Continuations are captured by **cloning the call stack** (Strategy B from
`VM_CALLCC_DESIGN.md`). This is why `CallFrame: Clone` is a non-negotiable
invariant.

```
capture:  continuation.frames = vm.frames[prompt_depth..].to_vec()  // O(depth)
invoke:   vm.frames = continuation.frames.clone()                    // O(depth)
```

For programs that do not use `call/cc`, there is zero overhead — `CallFrame` is
only cloned when a continuation is explicitly captured.

### 6.2 Prompt Stack

The prompt stack is a `Vec<PromptFrame>` parallel to the call stack:

```rust
pub struct PromptFrame {
    pub tag:                TaggedValue,   // prompt tag (heap object or symbol)
    pub stack_depth:        usize,         // vm.frames.len() at prompt installation
    pub handler:            TaggedValue,   // handler closure
    pub return_reg:         Reg,           // where to write the result in caller
    pub dynamic_wind_depth: usize,         // for correct before/after execution
}
```

Prompts are pushed by `CallWithPrompt` and popped on normal return or
`AbortToPrompt`.

### 6.3 Dynamic Wind

Each `CallFrame` carries a `dynamic_wind_depth: usize` recording how many
dynamic-wind records were active at entry. The VM maintains a global
`Vec<DynamicWindRecord>` (before/after thunk pairs). `InvokeContinuation`
runs the appropriate exit/entry thunks when the wind stack changes.

---

## 7. GC Interaction

All `TaggedValue` values reachable by the GC must be in one of:
- `vm.registers` (the shared register array)
- `frame.closure.free_vars` for each live `CallFrame`
- `vm.constants` (constant pools of all loaded `CodeObject`s)
- `vm.globals` (global environment)
- `vm.prompt_stack` (handler closures, tags)
- `vm.dynamic_wind_stack` (before/after thunks)
- Any heap-allocated `VmContinuation` (captured frame slices)

Phase 2A uses `Rc`-based reference counting (cycles from `set-car!`/`set-cdr!`
may leak). Phase 2B introduces a tracing GC. The instruction set does not change
between phases — GC is a runtime concern, not an ISA concern.

---

## 8. What Is Deliberately Out of Scope (Phase 2A)

These are non-goals for the initial implementation. The ISA reserves no special
opcodes for them; they can be added later without breaking existing bytecode.

| Feature | Deferred to |
|---|---|
| NaN-boxing `VmVal` for immediate floats | Phase 2C (only if benchmarks justify) |
| Specialized opcodes (`FixnumAdd`, `Car`, `Cdr`, ...) | Phase 2B |
| Profiling / type feedback | Phase 2B |
| JIT compilation | Phase 2D+ |
| Binary bytecode serialization | Phase 2B+ |
| `syntax-case` procedural macros | Phase 3 |

---

## 9. Open Questions

1. ~~**`MakeClosure` free_vars encoding:**~~ ✅ Settled: `Vec<Reg>` inline in the
   instruction. Instructions are Rust enum variants in a `Vec`, so variable-length
   operands have no cost in Phase 2A. Binary encoding is a Phase 2B+ concern.

2. ~~**Variadic argument collection:**~~ ✅ Settled: callee-side. The caller passes
   all args flat via `Call`/`TailCall`. The callee prologue inspects `arg_count`,
   checks against its fixed arity, and builds the rest list from excess args.
   Keeps call instructions uniform. Consistent with Gauche, Chibi, and Racket BC.

3. ~~**Multiple return values:**~~ ✅ Settled: side buffer on `VmState`. `ReturnMulti`
   fills `vm.value_buffer: Vec<TaggedValue>`; `ReceiveValues` drains it. Regular
   `Return` puts one value in the buffer — the buffer is always the source of a
   function result. No heap allocation. Zero cost when `values` is not used.
   Consistent with Gauche (`vm->numVals` + `vm->vals[]`).

4. ~~**Prompt tag representation:**~~ ✅ Settled: dedicated opaque heap type.
   `HeapObjectData::PromptTag { id: u64 }` with a monotonically increasing id
   assigned at creation. `make-continuation-prompt-tag` allocates one. VM matches
   on id, not pointer equality. Prevents accidental aliasing with symbols.
   Required since exception handling is built on top of prompts.
