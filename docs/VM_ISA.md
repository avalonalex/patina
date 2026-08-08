# Patina VM: Instruction Set Architecture

**Status:** Implemented — Phase 2A complete, 1163/1163 R7RS chibi tests passing
**See also:** [VM_COMPILER.md](./VM_COMPILER.md), [VM_RUNTIME.md](./VM_RUNTIME.md), [VM_DECISIONS.md](./VM_DECISIONS.md)

---

## 1. Overview

Patina VM is a **register machine**. Each function activation owns a contiguous
window of registers in a shared array. Instructions reference registers by index
within the current window. There is no operand stack.

**Value type:** `TaggedValue` throughout — same type used by the tree-walker and
`patina-primitives`. No conversion overhead.

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
#[derive(Clone)]   // ← non-negotiable invariant (stack-snapshot continuations)
pub struct CallFrame {
    pub code_id:       CodeObjectId,    // which function is executing
    pub pc:            usize,           // program counter (instruction index)
    pub register_base: usize,           // offset into global register array
    pub num_regs:      u16,             // size of register window
    pub closure:       Option<HeapIndex>, // heap index of VmClosure (if any)
    pub return_reg:    Reg,             // where to write result in caller's frame
}
```

`CallFrame` must always be `Clone`. This is the foundation for stack-snapshot
continuations.

### 2.3 Code Object

```rust
pub struct CodeObjectId(pub u32);  // global atomic counter

pub struct CodeObject {
    pub id:           CodeObjectId,
    pub name:         Option<Symbol>,       // for debugging/error messages
    pub instructions: Vec<Instruction>,
    pub constants:    Vec<TaggedValue>,     // constant pool
    pub num_regs:     u16,                  // registers needed
    pub arity:        Arity,                // fixed / variadic
    pub source_map:   Vec<(usize, SourceLocation)>, // pc → source loc
}

pub enum Arity {
    Fixed(u16),          // exactly N args
    Variadic(u16),       // at least N args; rest collected into a list
}
```

Code objects are stored in `VmState::code_store` (a dense `Vec<Option<Rc<CodeObject>>>`
indexed by the sequential `CodeObjectId`) and looked up by ID at runtime.

### 2.4 Flat Closures

```rust
// Stored in the heap as HeapObjectData::VmClosure
HeapObjectData::VmClosure {
    code_id: u32,                    // CodeObjectId
    free_vars: Vec<TaggedValue>,     // captured values, indexed by slot
}
```

Free-variable analysis at compile time determines which variables are captured.
Mutable captured variables (`set!` after capture) are boxed in a
`HeapObjectData::MutableCell` at the binding site; the cell pointer is what gets
captured.

---

## 3. Instruction Encoding

Each instruction is a Rust enum variant. The VM does not use a binary wire format
— instructions live as `Vec<Instruction>` in `CodeObject`.

```rust
pub type Reg = u16;         // register index (frame-relative)
pub type ConstIdx = u16;    // index into CodeObject::constants
```

---

## 4. Instruction Set

### 4.1 Load / Store

| Instruction | Operands | Semantics |
|---|---|---|
| `LoadConst` | `dst: Reg, idx: ConstIdx` | `dst ← constants[idx]` |
| `LoadImmediate` | `dst: Reg, val: TaggedValue` | `dst ← val` (fixnum/bool/char/null inline) |
| `Move` | `dst: Reg, src: Reg` | `dst ← src` (register copy, covers local loads) |
| `LoadClosure` | `dst: Reg, slot: u16` | `dst ← current_closure.free_vars[slot]` |
| `StoreClosure` | `slot: u16, src: Reg` | `current_closure.free_vars[slot] ← src` |
| `LoadGlobal` | `dst: Reg, name: Symbol` | `dst ← globals[name]` (error if unbound) |
| `StoreGlobal` | `name: Symbol, src: Reg` | `globals[name] ← src` |

Both global instructions consult a per-site inline cache (Track P P4): each
`CodeObject` carries a pc-indexed table of `(environment id, slot)` entries.
After the first execution against a given globals environment, the name
lookup collapses to an id compare plus a slot read into the environment's
append-only slot table — redefinition overwrites a binding's slot in place,
so hits always see the current value. Names that resolve through a parent
environment are never cached. The cache is invisible to the ISA: operands,
semantics, and error behavior are unchanged.

### 4.2 MutableCell Operations

For variables that are mutated after being captured by a closure (letrec*
semantics, `set!` on captured vars).

| Instruction | Operands | Semantics |
|---|---|---|
| `AllocCell` | `dst: Reg, src: Reg` | Allocate `MutableCell(src)` on heap, store pointer in `dst` |
| `ReadCell` | `dst: Reg, cell: Reg` | `dst ← *cell` (read through MutableCell) |
| `WriteCell` | `cell: Reg, src: Reg` | `*cell ← src` (write through MutableCell) |

### 4.3 Closure Creation

| Instruction | Operands | Semantics |
|---|---|---|
| `MakeClosure` | `dst: Reg, code_id: CodeObjectId, free_vars: Vec<Reg>` | Allocate `VmClosure { code_id, free_vars: regs.map(\|r\| reg[r]) }`, store heap pointer in `dst` |

`free_vars` is a list of register indices whose current values are captured.

### 4.4 Control Flow

| Instruction | Operands | Semantics |
|---|---|---|
| `Jump` | `target: usize` | `pc ← target` |
| `JumpIf` | `cond: Reg, target: usize` | `if reg[cond] != #f: pc ← target` |
| `JumpUnless` | `cond: Reg, target: usize` | `if reg[cond] == #f: pc ← target` |

All Scheme values except `#f` are truthy (R7RS §6.3).

### 4.5 Function Calls

| Instruction | Operands | Semantics |
|---|---|---|
| `Call` | `func: Reg, args: Vec<Reg>, dst: Reg` | Non-tail call; push frame, copy args to callee params |
| `TailCall` | `func: Reg, args: Vec<Reg>` | Tail call; reuse frame, no stack growth |
| `Apply` | `func: Reg, args: Vec<Reg>, dst: Reg` | Like `Call` but last arg is spread as a list |
| `TailApply` | `func: Reg, args: Vec<Reg>` | Like `TailCall` but last arg is spread as a list |
| `Return` | `val: Reg` | Pop frame, write val to caller's return_reg |
| `CallPrimitive` | `func_id: PrimitiveFnId, name: Symbol, args: Vec<Reg>, dst: Reg` | Call primitive directly without pushing a frame; `name` supports the redefinition deopt (below) |

**Inline primitive opcodes** (Track P P3). Fixed-arity fast paths for the
hottest primitives, emitted only when the callee is a `GlobalRef` that
resolved to that primitive at compile time *and* the call site has the exact
arity; all other shapes use `CallPrimitive`. Every opcode carries the same
`(func_id, name)` pair as `CallPrimitive`.

| Instruction | Fast path | Falls back when |
|---|---|---|
| `Add` / `Sub` / `Mul` | both fixnums, no overflow → machine op | non-fixnum, overflow (handler promotes to bignum), type error |
| `Lt` / `NumEq` | both fixnums → machine compare | non-fixnum operand |
| `Eq` | always (`values_eq`) | rebound name only |
| `Cons` | always (`alloc_pair`) | rebound name only |
| `Car` / `Cdr` | native pair | non-pair (handler raises the type error) |
| `Not` | always (truthiness test) | rebound name only |
| `NullP` / `PairP` / `VectorP` | always (bit test) | rebound name only |
| `VectorRef` | vector + in-bounds fixnum index | wrong types or out of bounds (handler raises) |
| `VectorSet` | vector + in-bounds fixnum index; `dst ← unspecified` | wrong types or out of bounds |

The fallback for every opcode is `exec_call_primitive` — the same registry
handler the generic path calls — so results, numeric promotion, and error
messages are identical by construction, not by parallel implementation.

**Operand placement and fused forms** (Track P P5). Resolved-primitive call
sites whose arguments are all atoms read local-variable operands *in place*
(no staging `Move`s — `CallPrimitive` and the inline opcodes accept
arbitrary registers; staged temps remain for any call with a non-atomic
argument, preserving evaluation order). On top of that:

| Instruction | Operands | Semantics |
|---|---|---|
| `AddImm` / `SubImm` / `LtImm` / `NumEqImm` | `a: Reg, imm: TaggedValue, dst: Reg` + deopt pair | The register op with a fixnum-literal *right* operand absorbed (`(+ x 1)`, `(- x 1)`, `(< x n)`, `(= x 0)`); same fast/fallback split as the register form. Right side only, even for the commutative ops — the deopt passes `[a, imm]` to whatever the name is bound to, and a rebind need not be commutative. |
| `TestJumpUnless` | `test: TestOp, a: Reg, b: Reg, dst: Reg, target: usize` + deopt pair | Fused predicate + branch, always emitted with the plain `JumpUnless dst` still at the next pc. Fast path: write `dst`, then jump to `target` (test false) or over the `JumpUnless` (test true) — one dispatch instead of two. `TestOp` selects the predicate (`Not`, `NullP`, `PairP`, `VectorP`, `Eq`, `Lt`, `NumEq`); `b` is unused by the unary ones. |

One instruction covers every fusable predicate rather than one variant per
predicate, so there is a single fused-branch arm, deopt path, and emission
site as more predicates become fusable. Its slow paths fall through to the
kept `JumpUnless`, which is therefore both the deopt landing and the slow
path's branch.

Only a predicate feeding a branch fuses; the same predicate used as a value
(returned, bound, passed) keeps its plain opcode.

Pass 5 also threads branch tails to their `Return` in place (`Jump → Return`
becomes `Return`; `Move d←s; Jump → Return d` becomes `Return s`), replacing
instructions without moving any pc, so an `if` in tail position returns in
one dispatch per arm.

**Primitive redefinition deopt.** Emitting `CallPrimitive`/inline opcodes
assumes the global still binds the primitive at run time. That assumption is
checked, not trusted: `Define`/`StoreGlobal` set a per-primitive bit in
`VmState::shadowed_primitives` when they overwrite a primitive binding, and
every fast path tests its bit (one load+mask) before firing. A set bit routes
the call through `globals[name]` + full call dispatch, which is exactly the
pre-P2 behavior. See `PRD/TRACK_P_PERFORMANCE_PRD.md` §P3 for the design
space if this ever needs revisiting (immutable library bindings, optimize
levels, n-ary canonicalization).

**Call dispatch order** (for `Call`/`TailCall`):
1. Check for VM control primitive intercept (continuations, exceptions, values)
2. Try `try_call_primitive()` — immediate return
3. Try `try_call_parameter()` — parameter object
4. Try `try_invoke_continuation()` — continuation object
5. Fall through to `call_closure()` — push frame

**TailCall codegen**: passes `arg_tmps` directly to TailCall instruction (NOT
pre-moved to r0..rn). The VM's TailCall dispatch reads all arg values first,
then writes to callee base. This avoids clobbering `func.dst` if it falls in
the param slot range.

### 4.6 Multiple Values

Multiple values flow through `VmState::value_buffer`, not through dedicated
return/receive instructions. `(values …)` is intercepted as a control
primitive (§5) and refills the buffer in place; `call-with-values` compiles
to instruction-level sequences ending in `CallWithValues` /
`TailCallWithValues`:

| Instruction | Operands | Semantics |
|---|---|---|
| `CallWithValues` | `dst: Reg, consumer: Reg, producer_result: Reg` | Call consumer with the buffered values (or the single producer result); result → `dst` |
| `TailCallWithValues` | `consumer: Reg, producer_result: Reg` | Tail-position variant: pops the frame first |

Both consumer sides *take* the buffer (`mem::take`) rather than borrow it —
deliberately, so re-entrant producer/consumer code always sees a clean
channel. When the buffer is empty they fall back to unpacking a `#<values>`
heap object from the producer result — the carrier for values produced
outside the intercept (historically the bug-prone channel; see the archived
chibi-failure notes) — and finally to treating the result as a single
value. Continuation resume with multiple values refills the buffer the
same way.

### 4.7 Global Definitions

| Instruction | Operands | Semantics |
|---|---|---|
| `Define` | `name: Symbol, src: Reg` | `globals[name] ← reg[src]` (top-level `define`) |

### 4.8 Miscellaneous

| Instruction | Semantics |
|---|---|
| `Nop` | No operation. Used for patching. |

---

## 5. Control Primitives (Continuations, Exceptions, Values)

Continuation and exception operations are **not ISA instructions**. They are
handled as **VM control primitives** — intercepted at `Call`/`TailCall` dispatch
time before normal call resolution.

The `VmControlPrimitive` enum:

| Variant | Scheme form |
|---|---|
| `DynamicWind` | `dynamic-wind` |
| `CallWithContinuationPrompt` | `call-with-continuation-prompt` |
| `AbortCurrentContinuation` | `abort-current-continuation` |
| `CallWithCurrentContinuation` | `call/cc` |
| `Values` | `values` |
| `CallWithValues` | `call-with-values` |
| `WithExceptionHandler` | `with-exception-handler` |
| `Raise` | `raise` |
| `RaiseContinuable` | `raise-continuable` |
| `Error` | `error` |

These are detected via `vm_control_primitive()` which checks if a function value
matches a known control primitive, then dispatched via `handle_control_primitive()`.

---

## 6. Tail Call Semantics

Tail call elimination is **mandatory** per R7RS §6.11.

The compiler detects tail position during compilation:
- The body of a `Lambda` is in tail position.
- The last expression of a `Begin` in tail position is in tail position.
- The consequent and alternate of an `If` in tail position are in tail position.
- An `App`/`Apply` in tail position emits `TailCall`/`TailApply`.

`TailCall` reuses the current frame so the call stack does not grow for
tail-recursive loops.

---

## 7. Continuation Semantics

### 7.1 Stack Snapshot Model

Continuations are captured by **cloning the call stack** plus registers, winds,
prompts, and exception handlers.

```
capture:  VmContinuation { frames, registers, dynamic_winds, prompt_stack, exception_handlers, deliver_reg }
invoke:   restore all snapshots, run wind transitions, deliver value
```

### 7.2 Prompt Stack

```rust
pub struct PromptFrame {
    pub tag:                TaggedValue,
    pub stack_depth:        usize,
    pub handler:            TaggedValue,
    pub dst:                Reg,
    pub dynamic_wind_depth: usize,
}
```

### 7.3 Dynamic Wind

The VM maintains a `Vec<DynamicWindRecord>` (before/after thunk pairs with
`stack_depth`). Continuation invocation runs appropriate exit/entry thunks.

```rust
pub struct DynamicWindRecord {
    pub before:      TaggedValue,
    pub after:       TaggedValue,
    pub stack_depth: usize,
}
```

### 7.4 Continuation Storage

Continuations use **side tables** on VmState (not heap objects directly):
- `continuation_store: HashMap<u64, Rc<VmContinuation>>`
- `delimited_continuation_store: HashMap<u64, Rc<VmDelimitedContinuation>>`

Opaque handles (`VmContinuationRef(u64)`, `VmDelimitedContinuationRef(u64)`)
are stored as `HeapObjectData` variants, avoiding circular dependencies between
`patina-core` and `patina-vm`.

---

## 8. GC Interaction

All `TaggedValue` values reachable by the GC must be in one of:
- `vm.registers` (the shared register array)
- Closure `free_vars` for each live closure on heap
- `vm.code_store` constant pools
- `vm.globals` (global environment)
- `vm.prompt_stack` (handler closures, tags)
- `vm.dynamic_winds` (before/after thunks)
- `vm.exception_handlers` (handler closures)
- `vm.value_buffer` (multiple-value buffer)
- `vm.continuation_store` / `vm.delimited_continuation_store`

Phase 2A uses `Rc`-based reference counting (cycles from `set-car!`/`set-cdr!`
may leak).

---

## 9. What Is Deliberately Out of Scope (Phase 2A)

| Feature | Deferred to |
|---|---|
| NaN-boxing for immediate floats | Phase 2C (only if benchmarks justify) |
| Specialized opcodes (`FixnumAdd`, `Car`, `Cdr`, ...) | Phase 2B |
| Profiling / type feedback | Phase 2B |
| JIT compilation | Phase 2D+ |
| Binary bytecode serialization | Phase 2B+ |
| `syntax-case` procedural macros | Phase 3 |
