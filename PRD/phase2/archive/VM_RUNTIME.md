# Patina VM: Runtime Design

**Status:** Draft v0.1 — open for discussion
**Depends on:** [VM_ISA.md](./VM_ISA.md), [VM_DECISIONS.md](./VM_DECISIONS.md)
**Companion:** [VM_COMPILER.md](./VM_COMPILER.md)

---

## 1. Overview

The runtime is responsible for executing bytecode produced by the compiler. It
consists of three major components:

1. **`VmState`** — the complete execution state of the VM at any point in time
2. **Execution loop** — the fetch-decode-execute cycle
3. **Continuation machinery** — stack snapshots, prompt stack, dynamic-wind

The runtime reuses `TaggedValue`, `SharedHeap`, and `patina-primitives` directly.
No value conversion is needed — the same heap that the tree-walker uses is the VM's
heap.

**Key references:**
- `VM_CALLCC_DESIGN.md` §5–7 — stack-snapshot model, SRFI-226 semantics (settled)
- Gauche `vm.c` — `ScmContFrame`, `PUSH-CONT`/`POP-CONT` macros, prompt markers
- ChezScheme `pb.c` — fetch-decode loop structure

---

## 2. Core Data Structures

### 2.1 `CallFrame`

```rust
#[derive(Clone)]   // ← non-negotiable; enforced from day 1 (VM_DECISIONS.md §4)
pub struct CallFrame {
    /// Which function is executing.
    pub code: Rc<CodeObject>,

    /// Program counter: index into code.instructions.
    pub pc: usize,

    /// Base index into VmState::registers for this frame's register window.
    pub register_base: usize,

    /// Number of registers this frame owns.
    pub num_regs: u16,

    /// Closure this frame was entered through (None for top-level code).
    /// Provides access to free_vars via LoadClosure / StoreClosure.
    pub closure: Option<Rc<VmClosure>>,

    /// Number of arguments this frame was called with.
    /// Used by the callee prologue for variadic rest-arg collection.
    pub arg_count: usize,
}
```

`Rc<CodeObject>` keeps the compiled function alive as long as any frame references
it (e.g. a continuation holds a frame that is executing it).

### 2.2 `VmClosure`

```rust
/// Heap-allocated flat closure. Stored as HeapObjectData::VmClosure.
pub struct VmClosure {
    pub code:      Rc<CodeObject>,
    pub free_vars: Vec<TaggedValue>,  // indexed by closure slot
}
```

### 2.3 `VmState`

The complete runtime state. Cloning `VmState` captures a full continuation.

```rust
pub struct VmState {
    /// Shared register array. All CallFrames index into this.
    /// Frame i owns registers[frame.register_base .. frame.register_base + frame.num_regs].
    pub registers: Vec<TaggedValue>,

    /// Call stack. frames.last() is the currently executing frame.
    pub frames: Vec<CallFrame>,

    /// Multiple-value side buffer (VM_ISA.md §4.5).
    /// ReturnMulti fills this; ReceiveValues drains it.
    /// Empty when not in a multi-value return.
    pub value_buffer: Vec<TaggedValue>,

    /// Prompt stack for delimited continuations (SRFI-226).
    pub prompt_stack: Vec<PromptFrame>,

    /// Dynamic-wind records, outermost first.
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// Global environment (shared with the rest of the interpreter).
    pub globals: Rc<RefCell<Environment>>,

    /// Shared heap (pairs, strings, closures, continuations, etc.).
    pub heap: SharedHeap,
}
```

### 2.4 `PromptFrame`

```rust
#[derive(Clone)]
pub struct PromptFrame {
    /// The prompt tag. Must be a HeapObjectData::PromptTag (VM_ISA.md §4, open Q4).
    pub tag: TaggedValue,

    /// vm.frames.len() at the time this prompt was installed.
    /// AbortToPrompt truncates frames back to this depth.
    pub stack_depth: usize,

    /// vm.dynamic_winds.len() at installation. Used to restore wind state.
    pub dynamic_wind_depth: usize,

    /// Handler closure to invoke on AbortToPrompt.
    pub handler: TaggedValue,

    /// Register in the *caller's* frame where the prompt's result is written.
    pub return_reg: u16,
}
```

### 2.5 `DynamicWindRecord`

```rust
#[derive(Clone)]
pub struct DynamicWindRecord {
    pub before: TaggedValue,  // thunk called on entry
    pub after:  TaggedValue,  // thunk called on exit
}
```

Pushed when `dynamic-wind` is entered, popped when the body returns. On
continuation jumps, `run_wind_transition` compares the current wind stack with the
target's wind stack and calls the appropriate `before`/`after` thunks.

### 2.6 Continuation Objects

Stored on the heap as `HeapObjectData::Continuation` / `HeapObjectData::DelimitedContinuation`.

```rust
/// Full continuation (non-composable, for call/cc).
/// Invoking it replaces the entire VM state.
#[derive(Clone)]
pub struct VmContinuation {
    pub frames:        Vec<CallFrame>,
    pub prompt_stack:  Vec<PromptFrame>,
    pub dynamic_winds: Vec<DynamicWindRecord>,
}

/// Delimited continuation (composable, for SRFI-226).
/// Invoking it appends its frames to the current stack.
#[derive(Clone)]
pub struct VmDelimitedContinuation {
    pub frames:        Vec<CallFrame>,
    pub dynamic_winds: Vec<DynamicWindRecord>,
    pub prompt_tag:    TaggedValue,
}
```

---

## 3. Execution Loop

The execution loop is a single `loop` with a `match` over the current instruction.
It runs until a `Return` at the bottom frame or an unhandled error.

```rust
pub fn run(vm: &mut VmState) -> Result<TaggedValue, EvalError> {
    loop {
        let frame = vm.frames.last_mut().expect("empty frame stack");
        let instr = frame.code.instructions[frame.pc].clone();
        frame.pc += 1;

        match instr {
            Instruction::LoadImmediate { dst, val } => {
                vm.reg_set(dst, val);
            }
            Instruction::LoadConst { dst, idx } => {
                let val = vm.current_code().constants[idx as usize];
                vm.reg_set(dst, val);
            }
            Instruction::Move { dst, src } => {
                let val = vm.reg_get(src);
                vm.reg_set(dst, val);
            }
            Instruction::LoadGlobal { dst, name } => {
                let val = vm.globals.borrow().get(&name)?;
                vm.reg_set(dst, val);
            }
            Instruction::StoreGlobal { name, src } => {
                let val = vm.reg_get(src);
                vm.globals.borrow_mut().set(name, val);
            }
            Instruction::LoadClosure { dst, slot } => {
                let val = vm.current_closure_var(slot);
                vm.reg_set(dst, val);
            }
            Instruction::StoreClosure { slot, src } => {
                let val = vm.reg_get(src);
                vm.set_closure_var(slot, val);
            }
            Instruction::Jump { target } => {
                vm.frames.last_mut().unwrap().pc = target;
            }
            Instruction::JumpIf { cond, target } => {
                if vm.reg_get(cond).is_truthy() {
                    vm.frames.last_mut().unwrap().pc = target;
                }
            }
            Instruction::JumpUnless { cond, target } => {
                if !vm.reg_get(cond).is_truthy() {
                    vm.frames.last_mut().unwrap().pc = target;
                }
            }
            Instruction::Call { func, args, dst } => {
                vm_call(vm, func, args, dst, /*tail=*/false)?;
            }
            Instruction::TailCall { func, args } => {
                vm_call(vm, func, args, /*dst ignored*/0, /*tail=*/true)?;
            }
            Instruction::Return { val } => {
                let result = vm.reg_get(val);
                if vm.frames.len() == 1 {
                    // Bottom of stack — done
                    return Ok(result);
                }
                vm_return(vm, result);
            }
            Instruction::ReturnMulti { vals } => {
                vm.value_buffer = vals.iter().map(|&r| vm.reg_get(r)).collect();
                let result = vm.value_buffer[0]; // convention: first value is primary
                if vm.frames.len() == 1 {
                    return Ok(result);
                }
                vm_return(vm, result);
            }
            Instruction::MakeClosure { dst, code_id, ref free_vars } => {
                let code = vm.lookup_code(code_id);
                let captured: Vec<TaggedValue> =
                    free_vars.iter().map(|&r| vm.reg_get(r)).collect();
                let closure = VmClosure { code, free_vars: captured };
                let val = vm.heap.borrow_mut().alloc_vm_closure(closure);
                vm.reg_set(dst, val);
            }
            Instruction::CallWithPrompt { body, tag, handler, dst } => {
                vm_call_with_prompt(vm, body, tag, handler, dst)?;
            }
            Instruction::AbortToPrompt { tag, val } => {
                vm_abort_to_prompt(vm, tag, val)?;
            }
            Instruction::CaptureComposable { dst, tag } => {
                vm_capture_composable(vm, dst, tag)?;
            }
            Instruction::InvokeContinuation { cont, val, composable } => {
                vm_invoke_continuation(vm, cont, val, composable)?;
            }
            Instruction::Define { name, src } => {
                let val = vm.reg_get(src);
                vm.globals.borrow_mut().define(name, val);
            }
            Instruction::CallPrimitive { func, ref args, dst } => {
                let arg_vals: Vec<TaggedValue> =
                    args.iter().map(|&r| vm.reg_get(r)).collect();
                let result = func(&vm.heap, arg_vals)?;
                vm.reg_set(dst, result);
            }
            Instruction::ReceiveValues { ref dsts } => {
                if vm.value_buffer.len() != dsts.len() {
                    return Err(EvalError::WrongNumberOfValues {
                        expected: dsts.len(),
                        actual: vm.value_buffer.len(),
                    });
                }
                for (&dst, &val) in dsts.iter().zip(vm.value_buffer.iter()) {
                    vm.reg_set(dst, val);
                }
                vm.value_buffer.clear();
            }
            Instruction::Nop => {}
        }
    }
}
```

### 3.1 Register Access Helpers

```rust
impl VmState {
    #[inline]
    pub fn reg_get(&self, reg: u16) -> TaggedValue {
        let frame = self.frames.last().unwrap();
        self.registers[frame.register_base + reg as usize]
    }

    #[inline]
    pub fn reg_set(&mut self, reg: u16, val: TaggedValue) {
        let base = self.frames.last().unwrap().register_base;
        self.registers[base + reg as usize] = val;
    }

    fn current_closure_var(&self, slot: u16) -> TaggedValue {
        let frame = self.frames.last().unwrap();
        frame.closure.as_ref()
            .expect("LoadClosure in non-closure frame")
            .free_vars[slot as usize]
    }
}
```

---

## 4. Call and Return

### 4.1 `vm_call`

```rust
fn vm_call(
    vm: &mut VmState,
    func_reg: u16,
    arg_regs: &[u16],
    dst_reg: u16,
    tail: bool,
) -> Result<(), EvalError> {
    let func = vm.reg_get(func_reg);

    match vm.heap.borrow().get_callable(func)? {
        Callable::Closure(closure) => {
            let code = closure.code.clone();
            let num_regs = code.num_regs as usize;
            let arg_count = arg_regs.len();

            // Check arity
            check_arity(&code.arity, arg_count)?;

            if tail {
                // Reuse current frame: overwrite register window in place
                let base = vm.frames.last().unwrap().register_base;
                for (i, &r) in arg_regs.iter().enumerate() {
                    let val = vm.registers[base + r as usize];  // read before overwrite
                    vm.registers[base + i] = val;
                }
                let frame = vm.frames.last_mut().unwrap();
                frame.code = code.clone();
                frame.pc = 0;
                frame.closure = Some(Rc::new(closure));
                frame.arg_count = arg_count;
                // num_regs may differ — extend register window if needed
                let new_end = base + num_regs;
                if new_end > vm.registers.len() {
                    vm.registers.resize(new_end, TaggedValue::UNSPECIFIED);
                }
                frame.num_regs = code.num_regs;
            } else {
                // Push a new frame
                let register_base = vm.next_register_base();
                vm.registers.resize(
                    register_base + num_regs,
                    TaggedValue::UNSPECIFIED,
                );
                // Copy args into new frame's parameter slots
                for (i, &r) in arg_regs.iter().enumerate() {
                    let val = vm.reg_get(r);
                    vm.registers[register_base + i] = val;
                }
                vm.frames.push(CallFrame {
                    code: code.clone(),
                    pc: 0,
                    register_base,
                    num_regs: code.num_regs,
                    closure: Some(Rc::new(closure)),
                    arg_count,
                });
                // Store dst_reg in caller frame for Return to write back
                vm.frames.last_mut().unwrap(); // (dst_reg stored in frame metadata)
            }

            // Callee prologue: collect rest args for variadic functions
            if let Arity::Variadic(fixed) = code.arity {
                collect_rest_args(vm, fixed as usize, arg_count)?;
            }

            Ok(())
        }

        Callable::Primitive(prim_fn) => {
            let args: Vec<TaggedValue> = arg_regs.iter()
                .map(|&r| vm.reg_get(r))
                .collect();
            let result = prim_fn(&vm.heap, args)?;
            if !tail {
                vm.reg_set(dst_reg, result);
            }
            // Primitive tail calls: result is the return value of the caller
            Ok(())
        }

        Callable::Continuation(cont) => {
            let val = if arg_regs.is_empty() {
                TaggedValue::UNSPECIFIED
            } else {
                vm.reg_get(arg_regs[0])
            };
            vm_invoke_continuation(vm, func_reg, 0 /*val already read*/, false)?;
            Ok(())
        }
    }
}
```

### 4.2 `vm_return`

```rust
fn vm_return(vm: &mut VmState, value: TaggedValue) {
    // Pop current frame
    let done_frame = vm.frames.pop().unwrap();

    // Shrink register array back
    let caller_frame = vm.frames.last().unwrap();
    vm.registers.truncate(caller_frame.register_base + caller_frame.num_regs as usize);

    // Write return value into caller's dst register
    // (dst_reg was saved in the frame before the call — see call convention)
    // For now: the calling instruction's dst is stored alongside the frame.
    // (Implementation detail: embed dst_reg in CallFrame or a parallel Vec)
}
```

### 4.3 Variadic Rest Arg Collection (callee-side, VM_ISA.md §4.4)

```rust
fn collect_rest_args(vm: &mut VmState, fixed: usize, actual: usize)
    -> Result<(), EvalError>
{
    if actual < fixed {
        return Err(EvalError::WrongArity { expected: format!("at least {fixed}"), actual });
    }
    // Build list from excess args (rightmost first)
    let mut rest = TaggedValue::NULL;
    let base = vm.frames.last().unwrap().register_base;
    for i in (fixed..actual).rev() {
        let val = vm.registers[base + i];
        rest = vm.heap.borrow_mut().alloc_pair(val, rest);
    }
    // Store rest list in the rest parameter slot (register index = fixed)
    vm.registers[base + fixed] = rest;
    Ok(())
}
```

---

## 5. Continuation Machinery

### 5.1 `CallWithPrompt`

```rust
fn vm_call_with_prompt(
    vm: &mut VmState,
    body_reg: u16,
    tag_reg: u16,
    handler_reg: u16,
    dst_reg: u16,
) -> Result<(), EvalError> {
    let tag     = vm.reg_get(tag_reg);
    let handler = vm.reg_get(handler_reg);
    let body    = vm.reg_get(body_reg);

    vm.prompt_stack.push(PromptFrame {
        tag,
        stack_depth:        vm.frames.len(),
        dynamic_wind_depth: vm.dynamic_winds.len(),
        handler,
        return_reg: dst_reg,
    });

    // Call body thunk with no arguments
    vm_call(vm, body_reg, &[], dst_reg, /*tail=*/false)
}
```

### 5.2 `AbortToPrompt`

```rust
fn vm_abort_to_prompt(
    vm: &mut VmState,
    tag_reg: u16,
    val_reg: u16,
) -> Result<(), EvalError> {
    let tag = vm.reg_get(tag_reg);
    let val = vm.reg_get(val_reg);

    // Find matching prompt
    let prompt_idx = vm.prompt_stack.iter().rposition(|p| p.tag == tag)
        .ok_or(EvalError::NoMatchingPrompt)?;
    let prompt = vm.prompt_stack[prompt_idx].clone();

    // Capture the frames between the prompt and the current top
    let captured_frames = vm.frames[prompt.stack_depth..].to_vec();
    let captured_winds  = vm.dynamic_winds[prompt.dynamic_wind_depth..].to_vec();
    let cont = VmDelimitedContinuation {
        frames:        captured_frames,
        dynamic_winds: captured_winds,
        prompt_tag:    tag,
    };

    // Run dynamic-wind exit thunks for the unwound segment
    run_wind_exit(vm, prompt.dynamic_wind_depth)?;

    // Unwind
    vm.frames.truncate(prompt.stack_depth);
    vm.prompt_stack.truncate(prompt_idx);

    // Allocate continuation on heap
    let cont_val = vm.heap.borrow_mut().alloc_delimited_continuation(cont);

    // Call the handler with (val, continuation)
    let handler = prompt.handler;
    // push handler frame with [val, cont_val] as args, dst = prompt.return_reg
    // ... (vm_call into handler)
    Ok(())
}
```

### 5.3 `CaptureComposable`

```rust
fn vm_capture_composable(
    vm: &mut VmState,
    dst_reg: u16,
    tag_reg: u16,
) -> Result<(), EvalError> {
    let tag = vm.reg_get(tag_reg);
    let prompt = vm.prompt_stack.iter().rposition(|p| p.tag == tag)
        .ok_or(EvalError::NoMatchingPrompt)?;
    let prompt = &vm.prompt_stack[prompt];

    // Snapshot — does NOT unwind, execution continues normally
    let cont = VmDelimitedContinuation {
        frames:        vm.frames[prompt.stack_depth..].to_vec(),
        dynamic_winds: vm.dynamic_winds[prompt.dynamic_wind_depth..].to_vec(),
        prompt_tag:    tag,
    };
    let val = vm.heap.borrow_mut().alloc_delimited_continuation(cont);
    vm.reg_set(dst_reg, val);
    Ok(())
}
```

### 5.4 `InvokeContinuation`

```rust
fn vm_invoke_continuation(
    vm: &mut VmState,
    cont_reg: u16,
    val_reg: u16,
    composable: bool,
) -> Result<(), EvalError> {
    let cont_val = vm.reg_get(cont_reg);
    let value    = vm.reg_get(val_reg);

    if composable {
        // Append captured frames to current stack
        let cont = vm.heap.borrow().get_delimited_continuation(cont_val)?;
        run_wind_transition(vm, &cont.dynamic_winds)?;
        vm.frames.extend(cont.frames.iter().cloned());
        vm.dynamic_winds.extend(cont.dynamic_winds.iter().cloned());
        // Deliver value into the first frame of the appended segment
        // (it's waiting for a return value)
    } else {
        // Replace entire frame stack (call/cc)
        let cont = vm.heap.borrow().get_continuation(cont_val)?;
        run_wind_transition(vm, &cont.dynamic_winds)?;
        vm.frames        = cont.frames.clone();
        vm.prompt_stack  = cont.prompt_stack.clone();
        vm.dynamic_winds = cont.dynamic_winds.clone();
        // Deliver value into the restored frame
    }
    Ok(())
}
```

### 5.5 Dynamic Wind Transition

Called whenever the wind stack changes (continuation invocation or prompt abort).
Runs `after` thunks for records being exited, then `before` thunks for records
being entered, in the correct order.

```rust
fn run_wind_transition(
    vm: &mut VmState,
    target_winds: &[DynamicWindRecord],
) -> Result<(), EvalError> {
    // Find the common prefix between current and target wind stacks
    let common = common_prefix_len(&vm.dynamic_winds, target_winds);

    // Run 'after' thunks for records we're leaving (innermost first)
    for record in vm.dynamic_winds[common..].iter().rev() {
        vm_call_thunk(vm, record.after)?;
    }

    // Run 'before' thunks for records we're entering (outermost first)
    for record in &target_winds[common..] {
        vm_call_thunk(vm, record.before)?;
    }

    Ok(())
}
```

---

## 6. GC Roots

All `TaggedValue`s that must be kept alive by the garbage collector:

| Location | What it holds |
|---|---|
| `vm.registers` | All live register values across all frames |
| `frame.closure.free_vars` | Captured variables for each live closure |
| `vm.value_buffer` | Multiple-value return buffer |
| `vm.prompt_stack[*].tag` | Prompt tags |
| `vm.prompt_stack[*].handler` | Handler closures |
| `vm.dynamic_winds[*].before/after` | Wind thunks |
| `vm.globals` | All global bindings |
| heap-allocated `VmContinuation` | Cloned frame slices + closures within |
| heap-allocated `VmDelimitedContinuation` | Same |
| `CodeObject::constants` | Quoted data, nested code objects |

Phase 2A uses `Rc` — anything reachable via `Rc` is kept alive automatically.
Cycles from `set-car!`/`set-cdr!` may leak; acceptable for Phase 2A.

Phase 2B introduces a tracing GC. At that point, the GC walk starts from the
roots listed above and traces through all heap objects. The instruction set does
not change.

---

## 7. Relationship to Existing Code

| Existing component | VM usage |
|---|---|
| `TaggedValue` | Used as-is throughout — no conversion |
| `SharedHeap` | Shared between tree-walker and VM |
| `patina-primitives` | Called directly via `CallPrimitive` |
| `Environment` (globals) | Shared — VM and tree-walker see the same globals |
| `DynamicWindRecord` | Reused; needs `#[derive(Clone)]` confirmed |
| `SourceLocation` / `SourceMap` | Stored in `CodeObject` for error messages |
| `CpsExpr` / CPS transform | Not used — VM compiles `CoreExpr` direct-style |
| `patina-tree-walker` | Unchanged; VM is an independent `Backend` impl |

---

## 8. Open Questions

1. ~~**Return address storage:**~~ ✅ Settled: store `return_reg: u16` directly in
   `CallFrame`. All per-call state stays in one place, and continuation clones
   get return registers automatically since they clone `Vec<CallFrame>`.

2. ~~**Register array growth strategy:**~~ ✅ Settled: dynamic `Vec<TaggedValue>`.
   Rust `Vec` amortizes growth; `register_base` offsets remain valid across
   reallocation. Switch to a fixed arena only if profiling shows it matters.

3. ~~**Tail call register overlap:**~~ ✅ Settled: compiler responsibility, not
   runtime. Pass 4 (register allocation) ensures tail call args are materialized
   into fresh temporaries before being moved into parameter slots. The runtime
   does a simple sequential copy and trusts the compiler invariant.

   **Invariant:** The compiler must never emit a `TailCall` where an arg source
   register aliases a parameter destination register of the callee. This must be
   covered by compiler tests:
   - A test that `(define (f a b) (f b a))` compiles to a `TailCall` with
     non-overlapping source and destination registers.
   - A test that mutual tail recursion `(f a b) → (g b a) → (f a b)` similarly
     produces non-overlapping register assignments.

   **Bytecode construction policy:** The compiler is the only supported path for
   producing bytecode in production. Hand-constructed `CodeObject`s in unit tests
   are the test author's responsibility and must explicitly respect this invariant.
   The runtime does not add overlap checks (would slow every tail call).
