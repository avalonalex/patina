# Instruction-Level Control Operations Design

**Status:** Phase 1 + 3 complete, Phase 2 not needed
**Created:** 2026-03-19
**Motivation:** `run_thunk`-based control primitives break when `call/cc` captures across synchronous Rust call boundaries. A recent fix papers over this for `call-with-values`, but the fundamental issue affects all control operations that use `run_thunk`.

## Problem Statement

The VM implements control operations (`call-with-values`, `dynamic-wind`, `with-exception-handler`, etc.) as **runtime-intercepted primitives**: the compiler emits a normal `Call` instruction, and the execution loop detects the primitive at dispatch time and runs its logic inline using `run_thunk()`.

`run_thunk` is a synchronous Rust function that pushes a frame, calls `run_loop_until(state, depth_before)`, and returns the result. This creates an invisible Rust-stack frame that `call/cc` cannot capture:

```
Rust call stack                     VM frame stack
─────────────────                   ──────────────
run_loop_until (top-level)          [frame 0: user code]
  └─ handle_control_primitive
       └─ run_thunk                 [frame 1: producer thunk]
            └─ run_loop_until
                 └─ dispatch         [frame 2: call/cc body]
                      └─ call/cc captures here
```

When the continuation is later invoked, the VM frame stack is restored, but the Rust call stack is flat — `run_thunk` is not on the stack. Execution continues in the wrong `run_loop_until` context.

### Current workaround

After `try_invoke_continuation` in Call/TailCall dispatch, if `frames.len() <= exit_depth`, we return the primary value immediately. This handles the `call-with-values` case but is fragile:

- Only returns the primary value (multi-value delivery depends on `value_buffer` being checked by the *correct* `call-with-values` handler, which is no longer on the Rust stack).
- Doesn't handle `dynamic-wind` body thunks correctly — the after-thunk needs to run on normal return, but if `call/cc` escapes and re-enters, the Rust-level cleanup code is gone.
- Doesn't handle `with-exception-handler` thunks that capture and resume continuations.

### Affected control operations

| Operation | `run_thunk` calls | Risk |
|-----------|-------------------|------|
| `call-with-values` | 1 (producer) | **Fixed** with workaround |
| `dynamic-wind` | 3 (before, body, after) | Body thunk escape works; re-entry into body is broken |
| `with-exception-handler` | 1 (thunk) | Thunk escape works; handler re-invocation may break |
| `call-with-continuation-prompt` | 1 (body) | Currently uses `call_closure` (instruction-level), OK |
| wind transition | N (before/after thunks) | Thunks are small; unlikely to `call/cc` but not impossible — and one that does is why the thunks became stub frames (`ResumeWindJump`) rather than nested calls, 2026-09-02 |

## Proposed Design: Compile Control Ops as Instructions

### Core idea

Instead of intercepting control primitives at runtime, the **compiler** recognizes calls to these procedures and emits specialized instruction sequences. The execution loop handles everything at the instruction level — no `run_thunk`, no synchronous Rust nesting.

### Approach: CPS-like instruction sequences

The key insight: every `run_thunk` call is a "call this thunk, then do something with the result." In CPS terms, the "something" is the continuation. We can compile this as a normal Call + instructions that run after the thunk returns.

#### `call-with-values`

**Current** (runtime intercept):
```
Call  r0 ← call-with-values(r1, r2)    ; intercepted → run_thunk(producer) + call consumer
```

**Proposed** (instruction-level):
```
Call      r3 ← r1()          ; call producer, result in r3
; value_buffer populated if producer used (values ...)
CallWithValues r0 ← r2(r3)   ; new instruction: call consumer with value_buffer or [r3]
```

The new `CallWithValues` instruction:
1. Checks `state.value_buffer` — if non-empty, uses those as args to consumer
2. Otherwise, uses `[r3]` (single value from producer)
3. Calls consumer like a normal Call

This is fully instruction-level. `call/cc` inside the producer captures normal VM frames. Continuation invocation restores frames normally. No `run_thunk` needed.

**Compiler recognition:** When the compiler sees `(call-with-values <producer> <consumer>)`, it emits the two-instruction sequence instead of a regular Call.

#### `dynamic-wind`

**Current** (runtime intercept):
```
Call r0 ← dynamic-wind(before, body, after)  ; intercepted → 3x run_thunk
```

**Proposed** (instruction-level):
```
Call      r3 ← r1()                    ; call before-thunk
PushWind  r1, r4, wind_depth           ; push wind record (before, after, depth)
Call      r3 ← r2()                    ; call body-thunk, result in r3
PopWind   wind_depth                   ; pop wind record if still present
Call      r5 ← r4()                    ; call after-thunk
LoadReg   r0 ← r3                      ; result is body's return value
```

The `PushWind` and `PopWind` instructions manage the `dynamic_winds` stack. If `call/cc` escapes out of the body, the normal wind-transition logic (already instruction-level in `try_invoke_continuation`) handles running exit/entry thunks. When a continuation re-enters the body, it restores the frame state to just after `PushWind`, so the body re-executes correctly.

**Key insight:** The before/after thunks in `dynamic-wind` are typically short (e.g., set!/restore a parameter). If they themselves use `call/cc`, the same instruction-level approach applies recursively.

#### `with-exception-handler`

**Current** (runtime intercept):
```
Call r0 ← with-exception-handler(handler, thunk)  ; intercepted → push handler + run_thunk
```

**Proposed** (instruction-level):
```
PushExceptionHandler r1, handler_depth    ; push handler record
Call      r0 ← r2()                       ; call thunk
PopExceptionHandler handler_depth         ; pop handler if still present
```

The `PushExceptionHandler` and `PopExceptionHandler` instructions manage the `exception_handlers` stack. `raise` already unwinds through instruction-level dispatch.

### New instructions summary

| Instruction | Semantics |
|-------------|-----------|
| `CallWithValues { dst, consumer, producer_result }` | Call consumer with value_buffer contents or single value |
| `PushWind { before, after }` | Push a dynamic-wind record |
| `PopWind` | Pop top dynamic-wind record, run after-thunk via Call |
| `PushExceptionHandler { handler }` | Push exception handler |
| `PopExceptionHandler` | Pop exception handler if depth matches |

### Compiler changes

The compiler needs to recognize calls to specific primitives and emit instruction sequences instead of regular Calls. This can be done in pass 5 (codegen) by checking the callee:

```rust
// In pass5, when compiling App:
if is_known_primitive(callee, "call-with-values") {
    // Emit: Call producer, CallWithValues consumer
} else if is_known_primitive(callee, "dynamic-wind") {
    // Emit: Call before, PushWind, Call body, PopWind, Call after
} else if is_known_primitive(callee, "with-exception-handler") {
    // Emit: PushExceptionHandler, Call thunk, PopExceptionHandler
} else {
    // Normal Call
}
```

**Challenge:** The compiler works on `RegExpr` (register-allocated IR), not on names. It needs to know that a particular variable refers to the `call-with-values` primitive. This requires either:

1. **Name-based recognition** in codegen — check if the callee is a `GlobalRef` to a known name. Simple but fragile (user can shadow the name).
2. **CoreExpr-level special forms** — add `CallWithValues`, `DynamicWind`, `WithExceptionHandler` as CoreExpr variants (like `If`, `Begin`). The desugarer recognizes these calls and emits the special forms. More robust but more invasive.
3. **Hybrid** — the desugarer marks these calls with a hint (e.g., wraps them in a `ControlOp` CoreExpr), and codegen uses the hint. Middle ground.

**Recommendation:** Option 1 (name-based recognition in codegen) is simplest and sufficient. Shadowing these names is pathological and we can fall back to the runtime intercept path if the callee isn't a known global.

### Tail position handling

All three operations need tail-position variants. When `(call-with-values producer consumer)` is in tail position:

```
TailCall  r3 ← r1()                  ; call producer (NOT tail — we need the result)
TailCallWithValues r2(r3)             ; tail-call consumer with values
```

Similarly for `dynamic-wind` — the after-thunk call is in tail position of the dynamic-wind expression only if the whole dynamic-wind is in tail position. The body is never in tail position (the after-thunk must run after it).

### Migration path

1. **Phase 1: `call-with-values`** — Complete. Compiler emits `Call producer` + `CallWithValues consumer`.
2. ~~**Phase 2: `with-exception-handler`**~~ — Already instruction-level. The runtime intercept uses `call_closure` (not `run_thunk`) and `pop_exception_handlers` on Return. No changes needed.
3. **Phase 3: `dynamic-wind`** — Complete. Compiler emits `Call before` + `PushWind` + `Call body` + `PopWind` + `Call after`. `PushWind` uses `stack_depth = 0` sentinel so `pop_resolved_winds` doesn't auto-pop; `PopWind` explicitly removes the record. Wind transition for continuation escape/re-entry compares wind record lists (no `stack_depth` dependency) and runs one thunk per step under a `ResumeWindJump` stub frame — `step_wind_jump`, which replaced `run_wind_transition` on 2026-09-02.

### Backward compatibility

The runtime intercept path (`VmControlPrimitive` + `handle_control_primitive`) should be kept as a fallback for cases where the compiler can't statically resolve the callee (e.g., `(apply call-with-values args)`). The instruction-level path is an optimization for the common case.

## Files to modify

- `crates/patina-vm/src/types/instruction.rs` — new instruction variants
- `crates/patina-vm/src/compiler/pass5_codegen.rs` — recognize control ops, emit instruction sequences
- `crates/patina-vm/src/runtime/vm_state.rs` — dispatch new instructions, eventually simplify `handle_control_primitive`

## Related

- `PRD/phase2/archive/VM_LIBRARY_LOADING_REDESIGN.md` — the library loading redesign that motivated this investigation
- `docs/VM_ISA.md` — instruction set documentation (update when instructions are added)
- The `call/cc` + `run_thunk` escape workaround (Call/TailCall dispatch, `frames <= exit_depth` check) — remove once Phase 1 is complete
