# VM Backend: call/cc Architecture Decision

**Status**: Decision Required Before Implementation
**Created**: 2026-03-05
**Lesson**: The tree-walker was originally written as a direct recursive evaluator.
`call/cc` was added later, requiring a complete rewrite to CPS — the largest single
refactoring in the project. This document ensures the VM makes the right structural
choice upfront.

---

## 1. The Core Problem with call/cc

`call/cc` captures "the rest of the computation from this point". In a conventional
implementation the rest of the computation lives on the **native call stack** as
return addresses and local variables. The native stack cannot be captured, cloned,
or transferred to the heap — which is why the tree-walker needed its CPS refactoring.

The VM faces exactly the same choice, but we must decide now before writing a line
of execution code.

There are three viable strategies:

---

## 2. Strategy A — CPS Compilation (extend what we have)

**Idea**: Compile `CoreExpr → CpsExpr → Bytecode`. Every function in the program is
CPS-transformed: it takes an explicit continuation parameter `k` and calls it with
its result instead of returning.

```scheme
;; Source
(define (f x) (+ x 1))
(call/cc (lambda (k) (f 42)))

;; After CPS transform
(define (f x k) (+cps x 1 k))        ; k is explicit
(callcc (lambda (escape k_top)         ; escape is the captured k
          (f 42 k_top)))               ; call in tail position, pass continuation
```

**How call/cc works**: In CPS, the current continuation `k` is already a first-class
value in scope (it's a parameter). `call/cc` just wraps it and passes it to its
argument. No stack copying, no special treatment — it's as cheap as creating a closure.

**What the VM state looks like**:
```rust
struct VmState {
    expr:     CpsBytecode,         // current instruction
    env:      Rc<Environment>,     // lexical environment
    cont_env: ContEnv,             // continuation variable bindings (linked list)
    // control structures (all explicit in CPS):
    dynamic_winds:      Vec<DynamicWindRecord>,
    exception_handlers: Vec<ExceptionHandler>,
    prompt_stack:       Vec<PromptFrame>,
}
```
This is nearly identical to what the tree-walker does — except `expr` is a bytecode
reference instead of a `Rc<CpsExpr>`. The trampoline becomes a fetch-decode loop.

**Pros**:
- We already have the CPS transformer, CpsExpr IR, and proven correctness
- call/cc is O(1) — literally just wrapping a value
- Dynamic-wind, exception handlers, delimited continuations all follow naturally
- Very small new VM surface: replace CpsExpr traversal with bytecode dispatch
- Less risk: the hard semantics are already validated by tree-walker tests

**Cons**:
- Every function call passes an explicit continuation argument (~1 extra pointer per call)
- Administrative continuations (for sequencing `(+ (f x) (g y))`) create extra closures
- Harder to do conventional compiler optimizations (inlining, register allocation)
- Benchmarks that don't use call/cc still pay CPS overhead

**Expected speedup over tree-walker**: 2–4× (eliminate CpsExpr tree traversal + Rc
chasing; keep continuation overhead).

---

## 3. Strategy B — Stack VM with Stack-Snapshot call/cc

**Idea**: Compile `CoreExpr → direct-style bytecode`. The VM has a conventional call
stack (`Vec<CallFrame>`). When `call/cc` is invoked, snapshot the entire call stack
into a heap-allocated continuation object.

```rust
struct CallFrame {
    code:    Rc<CompiledFn>,    // which function
    pc:      usize,             // where we are in it
    locals:  Vec<TaggedValue>,  // operand stack + local slots
    env:     Rc<Environment>,   // closed-over env (for closures)
}

struct VmState {
    frames:             Vec<CallFrame>,
    dynamic_winds:      Vec<DynamicWindRecord>,
    exception_handlers: Vec<ExceptionHandler>,
    prompt_stack:       Vec<PromptFrame>,
}

// Continuation = snapshot of VM state
struct VmContinuation {
    frames:             Vec<CallFrame>,         // cloned call stack
    dynamic_winds:      Vec<DynamicWindRecord>,
    exception_handlers: Vec<ExceptionHandler>,
    prompt_stack:       Vec<PromptFrame>,
}
```

**How call/cc works**:
1. Clone `VmState` → `VmContinuation`
2. Wrap in `Procedure::Continuation(Rc<VmContinuation>)`
3. Call the argument with this value

**How invoking a continuation works**:
1. Run dynamic-wind exit handlers for the current winds
2. Run dynamic-wind entry handlers for the captured winds
3. Replace `vm.frames` with `cont.frames.clone()`
4. Resume execution

**The non-negotiable design invariant**: Every `CallFrame` must be fully cloneable.
This means:
- No `&` references to stack data — all state must be owned values
- `Rc<CompiledFn>` not raw pointers (already satisfied by using Rc)
- `locals: Vec<TaggedValue>` — TaggedValue is Copy, so Vec<TaggedValue> is trivially cloneable
- `env: Rc<Environment>` — cheap Rc clone

**Pros**:
- Fast normal execution (no CPS overhead for the 99% of code that doesn't use call/cc)
- Standard compiler techniques apply: inlining, constant folding, unboxing fixnums
- Call stack is intuitive — one frame per function call
- Expected speedup over tree-walker: 5–10×

**Cons**:
- Stack clone on call/cc is O(stack depth) — expensive for deeply recursive `call/cc`
- More complex implementation than Strategy A
- Must encode dynamic-wind and exception handler correctness on top of the stack model
- Requires careful design so `CallFrame` stays cloneable as the VM evolves

**Optimization possible later**: "Lazy copying" — use `Rc<Vec<CallFrame>>` with copy-on-write
so multiple continuations sharing the same prefix don't duplicate frames. Requires either
GC or careful Rc ref-counting discipline.

---

## 4. Strategy C — Spaghetti Stack (heap-allocated frames)

**Idea**: Each `CallFrame` is heap-allocated (`Rc<RefCell<Frame>>`). The "stack" is a
linked list of Rc pointers. A continuation is just an Rc pointer to the current frame.

```rust
type Frame = Rc<RefCell<CallFrame>>;

struct VmState {
    current_frame: Option<Frame>,   // head of the frame linked list
    ...
}
```

**How call/cc works**: Just clone the `Rc` pointer — O(1), no copying.
**How frame return works**: `current_frame = frame.borrow().parent.clone()`

**Pros**:
- O(1) continuation capture
- Continuation invocation is also cheap (just swap the frame pointer)

**Cons**:
- Every function call allocates on the heap — terrible cache behavior
- Rc reference counting overhead on every frame operation
- ~3–5× slower than a conventional stack for non-continuation code
- Profiling shows Rc::drop_slow is already 22% of tree-walker time

**Verdict**: Not viable for Patina's performance goals.

---

## 5. Recommendation: Strategy B (Stack VM + Snapshot)

**Rationale**:

1. **Performance target**: The primary motivation for Phase 2 is 5–10× speedup over the
   tree-walker. Strategy A (CPS bytecode) gets 2–4× — insufficient. Strategy B gets 5–10×.

2. **call/cc usage in practice**: Profiling shows `callcc/simple` at 4.5µs and `callcc/loop/200`
   at 1ms — continuation benchmarks are fast even in the tree-walker. Stack snapshot cost
   is only paid when `call/cc` is actually called, which most programs don't do heavily.

3. **Architectural cleanliness**: A stack VM is the natural structure for a bytecode VM.
   Direct-style compilation is easier to reason about, optimize, and debug. CPS-style
   compilation is an optimization technique, not a VM structure.

4. **Avoiding the tree-walker's mistake in reverse**: The tree-walker's mistake was choosing
   a structure (direct recursion) incompatible with call/cc and retrofitting it. We must not
   make the analogous mistake of choosing CPS compilation when direct-style is better for
   performance and leave no room to optimize later.

**The single non-negotiable invariant (enforced from day 1)**:
> Every `CallFrame` must own all its state — no borrowed references, no raw pointers.
> `CallFrame` must implement `Clone`.

Add `#[derive(Clone)]` to `CallFrame` from the very first commit. Any PR that removes
`Clone` from `CallFrame` is architecturally wrong. This single constraint is the lesson
from the tree-walker refactoring.

---

## 6. Detailed VM State Design

### CallFrame

```rust
#[derive(Clone)]
pub struct CallFrame {
    /// The compiled function being executed
    pub code: Rc<CompiledFn>,

    /// Program counter: index into code.instructions
    pub pc: usize,

    /// Operand/value stack for this frame (grows up)
    /// All values are TaggedValue (8 bytes, Copy — cheap to clone)
    pub stack: Vec<TaggedValue>,

    /// Closed-over environment (for lambdas)
    pub env: Rc<Environment>,

    /// Number of arguments this frame was called with
    /// Used for rest-argument collection
    pub arg_count: usize,
}
```

### VmState

```rust
pub struct VmState {
    /// Call stack: frames[0] is the bottom (entry point), frames.last() is active
    pub frames: Vec<CallFrame>,

    /// Active dynamic-wind records (for dynamic-wind correctness on continuation jumps)
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// Active exception handlers (with-exception-handler)
    pub exception_handlers: Vec<ExceptionHandler>,

    /// Prompt stack (for call-with-continuation-prompt / delimited continuations)
    pub prompt_stack: Vec<PromptFrame>,
}
```

### Continuation (heap value)

```rust
/// A captured full continuation (from call/cc)
#[derive(Clone)]
pub struct VmContinuation {
    pub frames:             Vec<CallFrame>,
    pub dynamic_winds:      Vec<DynamicWindRecord>,
    pub exception_handlers: Vec<ExceptionHandler>,
    pub prompt_stack:       Vec<PromptFrame>,
}
```

Stored as `HeapObjectData::Procedure(Rc<Procedure::VmContinuation(Rc<VmContinuation>)>)`.
`DynamicWindRecord` already implements `Copy`; `ExceptionHandler` and `PromptFrame` need
`Clone` — verify before starting the VM crate.

### Delimited continuations (shift/reset / call-with-continuation-prompt)

A delimited continuation captures only a **slice** of the frame stack, from the current
point back to the nearest prompt:

```rust
/// A captured delimited continuation (from shift or call-with-continuation-prompt)
#[derive(Clone)]
pub struct VmDelimitedContinuation {
    pub frames: Vec<CallFrame>,          // only the frames between prompt and capture site
    pub dynamic_winds: Vec<DynamicWindRecord>,
    pub prompt_tag: Option<Rc<PromptTag>>,
}
```

Invoking a delimited continuation *appends* its frames to the current stack (unlike a full
continuation which *replaces* the stack). This is the core semantic distinction.

---

## 7. Delimited Continuations: SRFI-226

**Decision**: The VM implements **SRFI-226** (Final 2023) as its continuation API.
This is a superset of plain `call/cc` and subsumes exception handling, `dynamic-wind`,
generators, and future async/await — all via a single prompt-tag mechanism.

See also: [VM_DECISIONS.md §4](./VM_DECISIONS.md)

### Three Core ISA Instructions

```
CallWithPrompt  body_reg, prompt_tag_reg, handler_reg, dst_reg
AbortToPrompt   prompt_tag_reg, value_reg
CaptureComposable dst_reg, prompt_tag_reg
```

#### `CallWithPrompt body, tag, handler, dst`

Establishes a prompt tagged with `tag`, then calls `body` (a thunk).
If `body` returns normally, its return value goes to `dst`.
If `AbortToPrompt tag ...` is called within `body`, `handler` is invoked instead.

Maps to SRFI-226's `call-with-continuation-prompt`.

```rust
VmOpcode::CallWithPrompt { body, tag, handler, dst } => {
    // Push a new PromptFrame onto the prompt stack
    let prompt = PromptFrame {
        tag:               vm.registers[tag],
        stack_depth:       vm.frames.len(),
        dynamic_wind_depth: vm.dynamic_winds.len(),
        handler_reg:       handler,
        dst_reg:           dst,
    };
    vm.prompt_stack.push(prompt);
    // Tail-call into body (or Call if not in tail position)
    vm.call(vm.registers[body], &[], dst)?;
}
```

#### `AbortToPrompt tag, value`

Unwinds the frame stack back to the nearest `PromptFrame` matching `tag`.
The frames between the prompt and the abort site are bundled into a
`VmDelimitedContinuation` and passed to the prompt's handler.

Maps to SRFI-226's `abort-current-continuation`.

```rust
VmOpcode::AbortToPrompt { tag, value } => {
    let tag_val = vm.registers[tag];
    let prompt = vm.find_prompt(tag_val)?;

    // Capture the slice of frames between prompt and current depth
    let captured_frames = vm.frames[prompt.stack_depth..].to_vec();
    let cont = VmDelimitedContinuation {
        frames:        captured_frames,
        dynamic_winds: vm.dynamic_winds[prompt.dynamic_wind_depth..].to_vec(),
        prompt_tag:    tag_val,
    };

    // Run dynamic-wind exit handlers for unwound frames
    run_wind_transition(&mut vm.dynamic_winds, &vm.dynamic_winds[..prompt.dynamic_wind_depth]);

    // Restore execution to the prompt's handler
    vm.frames.truncate(prompt.stack_depth);
    vm.prompt_stack.pop();

    // Call the handler with (continuation, value)
    let cont_val = heap.alloc_delimited_continuation(cont);
    vm.call(vm.registers[prompt.handler_reg], &[cont_val, vm.registers[value]], prompt.dst_reg)?;
}
```

#### `CaptureComposable dst, tag`

Captures the frames from the current depth back to the nearest matching prompt as
a `VmDelimitedContinuation`, placing it in `dst`. Execution continues normally
(unlike `AbortToPrompt`, this does not unwind — it just takes a snapshot of the slice).

Maps to SRFI-226's `call-with-composable-continuation`.

```rust
VmOpcode::CaptureComposable { dst, tag } => {
    let tag_val = vm.registers[tag];
    let prompt = vm.find_prompt(tag_val)?;
    let cont = VmDelimitedContinuation {
        frames:        vm.frames[prompt.stack_depth..].to_vec(),
        dynamic_winds: vm.dynamic_winds[prompt.dynamic_wind_depth..].to_vec(),
        prompt_tag:    tag_val,
    };
    vm.registers[dst] = heap.alloc_delimited_continuation(cont);
}
```

### Non-Composable `call/cc` (the classic version)

Classic `call/cc` is a non-composable full continuation. It is implemented by wrapping
`CaptureComposable` at the default top-level prompt, and marking the continuation as
non-composable. Invoking it **replaces** the current frame stack (unlike composable,
which appends):

```
; (call/cc proc)  — compiler emits:
LoadPromptTag r0, DEFAULT_PROMPT
CaptureComposable r1, r0          ; r1 = continuation up to default prompt
MarkNonComposable r1               ; mark: invocation replaces stack, not appends
Call proc_reg, [r1], dst
```

`VmContinuation` (full) vs `VmDelimitedContinuation` (delimited):

```rust
/// Full continuation (non-composable, for call/cc)
#[derive(Clone)]
pub struct VmContinuation {
    pub frames:             Vec<CallFrame>,   // entire stack up to default prompt
    pub dynamic_winds:      Vec<DynamicWindRecord>,
    pub exception_handlers: Vec<ExceptionHandler>,
    pub prompt_stack:       Vec<PromptFrame>,
}

/// Delimited continuation (composable, for SRFI-226)
#[derive(Clone)]
pub struct VmDelimitedContinuation {
    pub frames:        Vec<CallFrame>,        // slice between prompt and capture site
    pub dynamic_winds: Vec<DynamicWindRecord>,
    pub prompt_tag:    VmVal,
}
```

### Invoking Continuations

**Non-composable (call/cc)** — replaces the entire execution state:
```rust
fn invoke_vm_continuation(vm: &mut VmState, cont: &VmContinuation, value: VmVal) {
    run_wind_transition(&mut vm.dynamic_winds, &cont.dynamic_winds);
    vm.frames             = cont.frames.clone();
    vm.dynamic_winds      = cont.dynamic_winds.clone();
    vm.exception_handlers = cont.exception_handlers.clone();
    vm.prompt_stack       = cont.prompt_stack.clone();
    vm.current_frame_mut().set_return(value);
}
```

**Composable (delimited)** — appends frames to the current stack:
```rust
fn invoke_delimited_continuation(vm: &mut VmState, cont: &VmDelimitedContinuation, value: VmVal) {
    run_wind_transition(&mut vm.dynamic_winds, &cont.dynamic_winds);
    // Append the captured frame slice — composable means it "returns" to its call site
    vm.frames.extend(cont.frames.iter().cloned());
    vm.dynamic_winds.extend(cont.dynamic_winds.iter().cloned());
    vm.current_frame_mut().set_return(value);
}
```

### Everything Is a Prompt

With SRFI-226, all control effects unify under prompts:

| Feature | Prompt tag | Mechanism |
|---------|-----------|-----------|
| `with-exception-handler` | `exception-prompt-tag` | `CallWithPrompt` |
| `raise` | `exception-prompt-tag` | `AbortToPrompt` |
| `dynamic-wind` | built-in hooks | fires on `CallWithPrompt` entry/exit |
| generators | generator-specific tag | `CaptureComposable` |
| future async/await | async-specific tag | `CaptureComposable` |

Adding new control abstractions = adding a new prompt tag. No ISA changes.

The `run_wind_transition` function mirrors the tree-walker's
`CpsEvaluator::jump_to_continuation` (`cps_eval/wind.rs`); the two backends keep
separate `DynamicWindRecord` types.

---

## 8. Tail Calls

Tail calls require that the compiler mark tail positions in `CoreExpr`. The `ExprVisitor`
trait in `patina-ir` can implement a tail-position analysis pass. The compiler emits:

- **Non-tail call**: `CALL n` — pushes a new `CallFrame` onto `vm.frames`
- **Tail call**: `TAIL_CALL n` — replaces the current `CallFrame` (in-place update of pc,
  stack, env) instead of pushing a new one

`TAIL_CALL` is essential for the `ctak` benchmark (which uses call/cc in tail position) and
for all of the standard library's recursion patterns.

The contract: **tail calls must never grow the frame stack**. This ensures:
- O(1) space for tail-recursive functions
- Continuations captured in tail position don't capture a growing stack

---

## 9. Relationship to Existing Code

| Component | Reused | Notes |
|-----------|--------|-------|
| `DynamicWindRecord` | ✓ as-is | `Copy`, already has unique IDs |
| `PromptTag` | ✓ as-is | already in patina-core |
| Dynamic-wind transition logic | ✓ refactored | extract from CpsEvaluator to a free fn in patina-core |
| Exception handler structs | ✓ with Clone added | need `#[derive(Clone)]` |
| `CpsContinuation` | ✗ not used | Strategy B doesn't use CPS continuations |
| `CpsExpr` / CPS transform | ✗ not used | Strategy B compiles CoreExpr directly |
| `ExprVisitor` | ✓ for compiler passes | tail-position marking, free-var analysis |
| `PrimitiveRegistry` | ✓ after refactoring | see VM_BACKEND_DESIGN.md |

---

## 10. What to Implement First (Phase 2A Checklist)

### Pre-VM Refactoring (do before any VM code)

- [ ] Extract `patina-primitives` crate — ~290 heap-only primitives with signature
      `fn(&SharedHeap, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>`
- [ ] Add `ApplyContext` trait for the ~6 higher-order primitives
- [ ] Refactor tree-walker's `PrimitiveRegistry` to use `patina-primitives`
- [ ] All existing tests still pass (tree-walker unchanged)

### VM Crate Skeleton

- [ ] Create `crates/patina-vm/` with `Cargo.toml`
- [ ] Define `VmVal` (NaN-boxing design — Phase 2A can use a simpler encoding first)
- [ ] Define `CallFrame` with `#[derive(Clone)]` — enforce in CI
- [ ] Define `VmState` with `frames`, `dynamic_winds`, `exception_handlers`, `prompt_stack`
- [ ] Define `VmContinuation` and `VmDelimitedContinuation` with `#[derive(Clone)]`

### Continuation Infrastructure (before execution loop)

- [ ] Extract `run_wind_transition` to a free function shared by tree-walker + VM
- [ ] Implement `invoke_vm_continuation` (non-composable) — test in isolation
- [ ] Implement `invoke_delimited_continuation` (composable) — test in isolation
- [ ] Write a test: `CallWithPrompt` → `AbortToPrompt` → handler fires with continuation
- [ ] Write a test: `CaptureComposable` → invoke continuation → dynamic-wind handlers fire

### Compiler

- [ ] `VmCoreExpr`: same as `CoreExpr` but with variable locations resolved to indices
- [ ] Free variable analysis pass (needed for flat closure compilation)
- [ ] Register allocator (simple linear scan)
- [ ] Compile: literals, variables, `if`, `begin`, function application, `define`
- [ ] Compile: `lambda` → `MakeClosure` with flat free-var capture
- [ ] Compile: tail positions → `TailCall` instruction
- [ ] Compile: `call/cc` → `CaptureComposable` + mark non-composable + `Call`
- [ ] Compile: `call-with-continuation-prompt` → `CallWithPrompt`

### Execution Loop

- [ ] Fetch-decode-execute loop
- [ ] `Call` / `TailCall` / `Return`
- [ ] `LoadLocal`, `StoreLocal`, `LoadClosure`, `StoreClosure`, `LoadGlobal`, `StoreGlobal`
- [ ] `CallWithPrompt`, `AbortToPrompt`, `CaptureComposable`
- [ ] Primitive dispatch via `patina-primitives`

### Acceptance

- [ ] `Interpreter<VmBackend>` passes all 1400 existing tests

Getting the continuation model right before the full execution loop is the lesson from
Phase 1. The tree-walker added `call/cc` to a working interpreter and needed a full CPS
rewrite. The VM designs continuations first, then builds the rest around them.
