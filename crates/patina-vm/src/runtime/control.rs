//! VM procedure calls and control transfers.
//!
//! This is the runtime boundary for a tier using [`VmState`]'s explicit Scheme
//! frames and registers. It is a Rust interface inside the VM crate, not a
//! native calling convention. The bytecode dispatcher and its driver stay in
//! [`super::vm_state`]; callers enter here rather than maintaining another
//! callable-type probe set or another wind/handler traversal. The compiler
//! reads only [`VM_INTERCEPTED_PRIMITIVES`]. Internal stub builders and probes
//! stay private; runtime-visible functions and resume operands serve the loop.
//!
//! # Entry and rooting contract
//!
//! - An executing caller has a live top frame, valid register windows, and a
//!   `pc` pointing *after* its call instruction. `dst`/`return_reg` are relative
//!   to that caller unless a function says otherwise. Tail calls require a
//!   caller below the departing frame; top-level expressions are not tail
//!   calls. `exit_depth` belongs to the driver, sampled before a nested call.
//! - Scheme frames and locals stay in `VmState`, not solely on the native
//!   stack. Calls may allocate, grow register storage, replace frames, or run
//!   arbitrary Scheme through a callback. Do not retain a frame reference,
//!   register pointer, or heap borrow across such a call. Re-read the top
//!   frame/base and refresh cached code before using them in the driver.
//! - These helpers do not collect. Allocations request collection; the driver
//!   services it only at a safe point. Rust argument vectors, capture copies,
//!   and globals-swap temporaries are not roots. Every driver loop holds a
//!   `GcDeferGuard`; nested loops cannot collect while those temporaries live.
//!   A compiled driver must use the same guard/safe-point discipline and the
//!   existing `VmState` root provider, and publish all live Scheme values before
//!   servicing a safe point. Hold an additional defer guard for temporaries
//!   across entry to a loop when there is no enclosing driver guard.
//! - Continuation side tables are weak. Carry the heap continuation *handle*
//!   in a rooted register across thunk execution, not just an `Rc` payload.
//!   Stub code is installed in `code_store` before its frame is made runnable.
//!
//! # Calls and normal results
//!
//! | Entry | Normal result | State that can change |
//! |---|---|---|
//! | [`call_value`] | `Ok(())`: value in `dst` or frames scheduled | All five dynamic components; heap/code stores |
//! | [`call_any`] | `Some(v)`: immediate value; `None`: drive scheduled frames | Same as `call_value` |
//! | [`tail_call_value`] | `Some(v)`: driver returns; `None`: continue dispatch | Reuses/pops caller; may change all dynamic components |
//! | [`vm_raise_value`] | Schedules handler and resumable raise remainder | Pops a handler; pushes frame/registers; leaves winds/prompts alone |
//! | [`capture_delimited`] | Detached snapshot; caller must root an allocated handle before yielding | Reads live state; clears the hole only in the copy |
//! | [`step_wind_jump`] | Schedules a wind step or installs target | May replace all five components; caller must signal transfer |
//! | [`abort_to_prompt`] | No normal result: return its `VmError` | Builds/installs a landing or schedules travel, then parks transfer |
//! | [`invoke_delimited`] | Identity or scheduled resumed region | Appends relocated frames/registers/prompts/handlers; re-enters winds |
//!
//! The five dynamic components are frames, registers, winds, prompts, and
//! exception handlers. An error is not a transaction rollback: a helper may
//! have changed them before failing. The driver owns error routing/recovery.
//! `Some` in the call APIs above is normal completion, never permission to
//! treat an escape as a procedure result. Closure-only fast paths require a
//! verified code id and preserve the same arity/register rules as dispatch.
//!
//! # Escape protocol
//!
//! 1. A full jump schedules its next wind thunk or restores its target. Its
//!    caller uses [`park_escape`] to store the delivered value in
//!    `state.pending_escape` and returns `VmError::ContinuationEscape`.
//!    [`abort_to_prompt`] additionally sets `pending_transfer`: its landing
//!    stub can have exactly the depth of an ordinary callback return.
//! 2. Propagate the error immediately. Do not store a callback result, run a
//!    pending consumer, or truncate dynamic stacks belonging to the abandoned
//!    call. Ordinary Rust resource cleanup is still required. All synchronous
//!    callback boundaries use [`across_reentry`], including parameter setters,
//!    `apply_proc`, `eval`, and library loading. It distinguishes an escape
//!    from a local continuation invocation that really returns to the callback.
//! 3. The driver checks `pending_escape` **before** classifying an error.
//!    Primitive error conversion can wrap the sentinel in a `Runtime` error,
//!    so matching only the error variant loses the transfer. If the frame depth
//!    is at or below this driver's `exit_depth`, leave the value parked and
//!    propagate `LoopExit::Escaped` to the enclosing driver. Otherwise consume
//!    the parked value, clear `pending_transfer`, refresh code/frame state, and
//!    resume dispatch. Never route a parked escape to a Scheme error handler.
//!
//! Only the driver that owns the landing consumes it. Top-level `execute`
//! resets transfer state between runs; helpers must not clear it speculatively.
//! A compiled tier must participate in this protocol, not catch the sentinel
//! and continue the native computation that was abandoned.
//!
//! # Replacing versus extending the machine
//!
//! A full jump (including an abort landing) uses the shared `next_wind_step`
//! policy: pop before running an exiting record's `after`; run an entering
//! record's `before` before pushing it. Those thunks use their record's saved
//! handlers; live prompts remain findable until arrival replaces the machine.
//! A composable invoke instead extends the live machine and returns to its
//! caller. Its entry thunks use the invoke site's handlers, then append the
//! captured region with all frame, wind, and handler depths relocated. It must
//! not be routed through the replacement traversal. A raise crosses no wind
//! extent: only a subsequent continuation jump performs unwinding.
//!
//! Work owed after a Scheme call must be representable in captured frames:
//! `wind_jump_stub` resumes replacement travel; `invoke_step_stub` resumes
//! extension; `value_wind_stub` completes ordinary dynamic-wind;
//! `abort_handler_stub` delivers the landing's result; `raise_step_stub`
//! reinstalls a continuable handler or raises a secondary exception. Their
//! register layouts are shared with the existing `Resume*` instruction arms.
//! Native Rust stack frames are not captured; moving a call into this module
//! does not make a synchronous native callback replayable.
//!
//! The detailed dynamic-state matrix is in `docs/VM_RUNTIME.md` §5.6, and GC
//! safe-point/deferral rules in `docs/GC_DESIGN.md` §7. The executable guards
//! are `control_flow_matrix.rs`, `escape_from_primitive.rs`, and the Scheme
//! control suites (including their existing backend-specific expectations).

use super::vm_state::{
    LoopExit, VmState, frame_globals, run_loop_until, run_loop_until_outcome, vm_eval_expr,
    vm_load_library,
};
use crate::error::VmError;
use crate::types::code_object::{Arity, CodeObject, GlobalCacheEntry};
use crate::types::continuation::{
    ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use crate::types::instruction::{Instruction, PrimitiveFnId};
use crate::types::{CallFrame, CodeObjectId};
use patina_core::continuation::{WindStep, next_wind_step};
use patina_core::core_expr::Symbol;
use patina_core::heap::SharedHeap;
use patina_core::procedure::Procedure;
use patina_core::tagged_value::TaggedValue;
use std::rc::Rc;
use std::sync::Arc;

/// Call a compiled closure whose code id was resolved by the dispatcher.
///
/// # State contract
///
/// Requires a live caller, a verified closure/code-id pair and a caller-relative
/// return register. On success appends one frame and its argument window; leaves
/// winds, prompts and handlers unchanged. Arity/code lookup errors precede the push.
fn call_closure_resolved(
    state: &mut VmState,
    closure_val: TaggedValue,
    code_id: CodeObjectId,
    args: &[TaggedValue],
    return_reg: u16,
) -> Result<(), VmError> {
    let code = state.code_object(code_id)?;

    check_arity(code.arity, args.len())?;

    let base = state.alloc_registers(code.num_regs);
    store_args_in_window(state, base, code.arity, args);

    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: closure_heap_index(closure_val),
        return_reg,
        code,
    });
    Ok(())
}

/// `call_closure_resolved` sourcing arguments directly from the caller's
/// registers — the `Call` instruction's closure fast path. The callee
/// window is freshly allocated above the caller's, so the copy cannot
/// overlap and no intermediate argument buffer of any kind is needed.
/// (Tail calls can't use this: they reuse the caller's window in place.)
///
/// # State contract
///
/// Requires the live caller base, valid argument registers and a verified
/// closure/code-id pair. Appends one frame/window on success, preserving all
/// three dynamic-context stacks; re-read frame/base before further dispatch.
pub(super) fn call_closure_from_regs(
    state: &mut VmState,
    closure_val: TaggedValue,
    code_id: CodeObjectId,
    caller_base: usize,
    arg_regs: &[u16],
    return_reg: u16,
) -> Result<(), VmError> {
    let code = state.code_object(code_id)?;

    check_arity(code.arity, arg_regs.len())?;

    let base = state.alloc_registers(code.num_regs);
    if let Arity::Variadic(fixed) = &code.arity {
        let fixed = *fixed as usize;
        for (i, &r) in arg_regs.iter().take(fixed).enumerate() {
            state.registers[base + i] = state.registers[caller_base + r as usize];
        }
        // Cons the rest list straight from the caller's registers — no
        // staging Vec. Variadic calls are hot in practice: `map` and every
        // rest-arg stdlib procedure land here per call.
        let regs = &state.registers;
        let rest = state.heap.borrow_mut().list_from_iter(
            arg_regs[fixed..]
                .iter()
                .map(|&r| regs[caller_base + r as usize]),
        );
        state.registers[base + fixed] = rest;
    } else {
        for (i, &r) in arg_regs.iter().enumerate() {
            state.registers[base + i] = state.registers[caller_base + r as usize];
        }
    }

    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: closure_heap_index(closure_val),
        return_reg,
        code,
    });
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

///
/// # State contract
///
/// Read-only type probe. Does not alter any VM stack or invoke Scheme.
fn resolve_closure(state: &VmState, val: TaggedValue) -> Result<CodeObjectId, VmError> {
    state
        .heap
        .borrow()
        .get_vm_closure_code_id(val)
        .map(CodeObjectId)
        .ok_or_else(|| VmError::TypeError {
            message: format!("expected a procedure, got {}", val.type_name()),
        })
}

/// Build `apply`'s argument list from a register window: every argument but
/// the last verbatim, then the last spread out of a proper list.
///
/// Shared by `Instruction::Apply`, `Instruction::TailApply` and the
/// `VmControlPrimitive::Apply` handler, which is what keeps the head-position
/// form and the value form accepting the same shapes.
///
/// # State contract
///
/// Requires the live caller base and a nonempty, valid register list.
/// Reads state and allocates a Rust argument vector; changes no VM stack.
pub(super) fn spread_apply_args(
    state: &VmState,
    base: usize,
    args: &[u16],
) -> Result<Vec<TaggedValue>, VmError> {
    let (&last, fixed) = args.split_last().expect("apply site has no arguments");
    let spread = spread_apply_tail(state, state.reg_at(base, last))?;
    // `(apply proc lst)` is the dominant shape and has no fixed prefix, so the
    // flattened list *is* the argument vector — returning it avoids allocating
    // a second one and memcpying into it.
    if fixed.is_empty() {
        return Ok(spread);
    }
    let mut arg_vals: Vec<TaggedValue> = Vec::with_capacity(fixed.len() + spread.len());
    arg_vals.extend(fixed.iter().map(|&r| state.reg_at(base, r)));
    arg_vals.extend(spread);
    Ok(arg_vals)
}

/// `apply`'s final argument, flattened. Errors if it is not a proper list.
///
/// # State contract
///
/// Reads only the heap; returns detached arguments or an improper-list error.
/// The caller must keep these values protected by the entry rooting contract.
fn spread_apply_tail(state: &VmState, last: TaggedValue) -> Result<Vec<TaggedValue>, VmError> {
    state
        .heap
        .borrow()
        .list_to_vec(last)
        .ok_or_else(|| VmError::Runtime {
            message: "apply: last argument is not a proper list".into(),
        })
}

/// Call any callable value from a site that has no instruction behind it.
/// Sites include: `call-with-values`' consumer (both the instruction and the
/// tail instruction), its producer, a prompt body, an exception-handler thunk,
/// `call/cc`'s procedure
/// argument, a jump's wind thunks ([`push_wind_step`]), a composable invoke's
/// re-entry thunks ([`push_invoke_step`] — a separate site with a different
/// handler-stack policy, not the same one), a higher-order primitive's
/// callback ([`run_apply_proc`]), and a parameter converter (through
/// [`call_any_sync`]).
///
/// **[`call_value`] plus the one thing those callers need and the dispatch
/// loop's callers get for free: whether the callee is finished.** `Some(v)` is
/// a callee that delivered `v` here and now — a primitive, a parameter object,
/// `values`, an identity composable continuation — and there is nothing left to
/// run. `None` is a frame on the stack, which the loop that owns this call must
/// drive to its `Return`. A control transfer out is an `Err`, as everywhere.
///
/// The frame depth is what answers that question, and it can: no `Ok` path
/// through `call_value` shortens the stack, and every one that resumes or
/// pushes lengthens it. That is the whole of this function — it holds no probe
/// of its own, so a callee is callable here exactly when it is callable from a
/// `Call` instruction.
///
/// **A live frame is a precondition**, for the read below and for the
/// `set_reg` inside `call_value` that it reads back. It holds because the only
/// dispatch loop whose `exit_depth` is 0 is [`super::vm_state::execute`]'s, and that loop's one
/// frame is never popped by a tail call: pass 3 marks top-level expressions as
/// **not** in tail position (`pass3_tail.rs`), so `TailCallWithValues` and
/// `tail_call_value_with_probe` — the two paths that pop before dispatching —
/// always leave a caller behind. Every other loop exits at a depth of 1 or
/// more.
///
/// It did hold its own until 2026-09-05, one probe short: primitive →
/// parameter → continuation → closure, with **no control primitive**, which is
/// claimed by name before any of those and so matched nothing and fell through
/// to a registry lookup that missed. `(call-with-values (lambda () (values +
/// '(1 2))) apply)` was `Undefined variable:
/// patina.internal.control/apply`, and every caller added since inherited it —
/// the prompt body most recently (issue #179). Issue #186, and the `exit_depth`
/// it named as the obstacle turned out to be a parameter nothing read; see
/// [`handle_control_primitive`].
///
/// # State contract
///
/// Requires a live caller and writable caller-relative return slot. On success
/// never shortens the frame stack: Some means immediate delivery, None means
/// frames remain to run. Can change all dynamic stacks; propagate Err before cleanup.
pub(super) fn call_any(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
    return_reg: u16,
) -> Result<Option<TaggedValue>, VmError> {
    let depth_before = state.frames.len();
    call_value(state, func_val, args, return_reg)?;
    if state.frames.len() != depth_before {
        return Ok(None);
    }
    Ok(Some(state.reg(return_reg)))
}

/// Try to call a parameter object. Returns `Some(Ok(result))` if `func_val`
/// is a parameter, `None` otherwise.
///
/// # State contract
///
/// None leaves state unchanged. A getter reads the parameter; a setter may
/// run a converter and change all dynamic state. Store the parameter value only
/// after normal conversion; propagate an escape without writing it.
fn try_call_parameter(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
) -> Option<Result<TaggedValue, VmError>> {
    let heap = state.heap.borrow();
    let (values, converter) = heap.get_parameter(func_val)?;
    drop(heap);
    match args.len() {
        0 => {
            // Get current value (top of stack)
            let stack = values.borrow();
            let val = stack.last().copied().unwrap_or(TaggedValue::UNSPECIFIED);
            Some(Ok(val))
        }
        1 => {
            // Set value (replace top of stack, applying converter if present)
            let new_val = if let Some(conv) = converter {
                match call_any_sync(state, conv, &[args[0]]) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                }
            } else {
                args[0]
            };
            let mut stack = values.borrow_mut();
            if let Some(top) = stack.last_mut() {
                *top = new_val;
            }
            Some(Ok(TaggedValue::UNSPECIFIED))
        }
        _ => Some(Err(VmError::ArityMismatch {
            expected: "0 or 1".into(),
            got: args.len(),
        })),
    }
}

/// Synchronously call a callable value and return its result.
/// Used for parameter converters and similar internal callbacks.
///
/// [`call_any`] with the nested loop attached, so the callee set is not this
/// function's to state — it is whatever `call_any` takes. It probed primitive
/// → closure until 2026-09-05 and nothing else, which is `call_any`'s hole
/// (issue #186) one dispatcher over.
///
/// Only reached from [`try_call_parameter`], i.e. from *setting* a parameter
/// by calling it. `make-parameter`'s own initial conversion is a registry
/// primitive and goes through [`run_apply_proc`] instead.
///
/// # State contract
///
/// Requires a live caller. Allocates a scratch return slot beyond its window,
/// may run a nested loop, and may change all dynamic stacks. Returns a value
/// only on normal completion; across_reentry propagates an abandoned call.
fn call_any_sync(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
) -> Result<TaggedValue, VmError> {
    // Use a return_reg beyond the caller's window to avoid clobbering live regs.
    let depth_before = state.frames.len();
    let return_reg = state.frames.last().map(|f| f.num_regs).unwrap_or(0);
    if let Some(f) = state.frames.last() {
        let needed = f.register_base + return_reg as usize + 1;
        if state.registers.len() < needed {
            state.registers.resize(needed, TaggedValue::UNSPECIFIED);
        }
    }
    // A callee that needs no frame is finished here and now.
    if let Some(result) = call_any(state, func_val, args, return_reg)? {
        return Ok(result);
    }
    // run_loop_until returns the value directly from Return instruction dispatch.
    // Routed through the re-entry boundary: this is how a parameter converter
    // runs during `parameterize`, and a continuation can escape out of it.
    match across_reentry(
        state,
        depth_before,
        |s| run_loop_until(s, depth_before),
        |v| *v,
    ) {
        Ok(v) => Ok(v),
        Err(Reentry::Escaped) => Err(VmError::Runtime {
            message: "continuation escaped".into(),
        }),
        Err(Reentry::Failed(e)) => Err(e),
    }
}

fn closure_heap_index(val: TaggedValue) -> Option<patina_core::tagged_value::HeapIndex> {
    // VmClosures are TAG_OBJECT (generic heap objects).
    if val.is_object() {
        Some(val.heap_index())
    } else {
        None
    }
}

/// Outlined error constructor — `#[cold]` keeps the formatting machinery
/// out of the call paths that inline `check_arity`.
#[cold]
#[inline(never)]
fn arity_error(arity: Arity, n: usize) -> VmError {
    VmError::ArityMismatch {
        expected: match arity {
            Arity::Fixed(k) => format!("{}", k),
            Arity::Variadic(k) => format!("at least {}", k),
        },
        got: n,
    }
}

#[inline(always)]
fn check_arity(arity: Arity, n: usize) -> Result<(), VmError> {
    if arity.accepts(n) {
        Ok(())
    } else {
        Err(arity_error(arity, n))
    }
}

/// Fill the register window at `base` with a call's arguments per `arity`:
/// fixed args into `r0..`, a variadic rest collected into a list after them.
/// The caller has already checked arity.
///
/// # State contract
///
/// Requires an allocated destination window and already checked arity.
/// Writes argument/rest slots and may allocate a rest list; changes no stacks.
fn store_args_in_window(state: &mut VmState, base: usize, arity: Arity, arg_vals: &[TaggedValue]) {
    if let Arity::Variadic(fixed) = arity {
        let fixed = fixed as usize;
        for (dst, &val) in state.registers[base..base + fixed].iter_mut().zip(arg_vals) {
            *dst = val;
        }
        let rest = state
            .heap
            .borrow_mut()
            .list_from_iter(arg_vals[fixed..].iter().copied());
        state.registers[base + fixed] = rest;
    } else {
        for (dst, &val) in state.registers[base..base + arg_vals.len()]
            .iter_mut()
            .zip(arg_vals)
        {
            *dst = val;
        }
    }
}

/// How a thunk run at a synchronous boundary ended. The same distinction
/// [`LoopExit`] makes, restated for the boundary's benefit.
enum ThunkOutcome {
    Returned(TaggedValue),
    /// A continuation unwound past this boundary. The caller owns no live
    /// frame: it must not write a register, and must hand the unwind on.
    ///
    /// Nor may it run cleanup of its own. The one boundary that tried —
    /// the value form of `dynamic-wind`, deciding whether it still owed its
    /// after-thunk by comparing wind-stack *lengths* — was asking about a
    /// stack the jump had already replaced with the target's, and truncated
    /// the target's records to run its own thunk again (issue #157). Cleanup
    /// that must survive an escape belongs in an instruction, not here.
    Escaped(TaggedValue),
}

/// The values a producer handed to `call-with-values`: the elements of a
/// `#<values>` object (from `values` with other than one argument, from a
/// primitive such as `exact-integer-sqrt`, or from a continuation invoked
/// with several), or the single value itself.
///
/// # State contract
///
/// Reads the heap only; returns detached values and changes none of the five
/// dynamic components. Protect the returned vector until its consumer runs.
pub(super) fn unpack_values(state: &VmState, primary: TaggedValue) -> Vec<TaggedValue> {
    match state.heap.borrow().get_values_as_tagged(primary) {
        Some(vals) => vals,
        None => vec![primary],
    }
}

/// Run a zero-argument callable to completion, reporting whether it returned.
///
/// Pushes the thunk's frame, runs the execution loop until that frame returns,
/// then returns the result.
///
/// `call-with-values`' producer is the only caller left. There used to be a
/// `run_thunk` wrapper over it for the boundaries that ran *bookkeeping*
/// thunks and had no result to place — a jump's wind thunks, then the value
/// form of `dynamic-wind`, then an abort's exit winds and a composable
/// invoke's entry thunks. Each of those in turn stopped being a Rust frame
/// and became an instruction to come back to, which is what lets a
/// continuation captured inside one be resumed; with the last two gone
/// (#165) the wrapper had no callers.
///
/// # State contract
///
/// Requires a live caller. May grow scratch registers and run a nested loop,
/// changing all dynamic stacks. Escaped is a transfer, not a producer value;
/// the caller must park it and return without applying its consumer.
fn run_thunk_outcome(state: &mut VmState, thunk: TaggedValue) -> Result<ThunkOutcome, VmError> {
    let depth_before = state.frames.len();

    // Use a return_reg beyond the caller's register window so the thunk's
    // Return instruction doesn't clobber any live value (e.g. MutableCell in r0).
    let return_reg = state.frames.last().map(|f| f.num_regs).unwrap_or(0);
    // Ensure the register array has room for the scratch slot.
    if let Some(f) = state.frames.last() {
        let needed = f.register_base + return_reg as usize + 1;
        if state.registers.len() < needed {
            state.registers.resize(needed, TaggedValue::UNSPECIFIED);
        }
    }

    // If thunk is a primitive (rare but possible), call it directly.
    if let Some(result) = call_any(state, thunk, &[], return_reg)? {
        return Ok(ThunkOutcome::Returned(result));
    }
    // VM closure was pushed; run until it returns.
    Ok(match run_loop_until_outcome(state, depth_before)? {
        LoopExit::Returned(v) => ThunkOutcome::Returned(v),
        LoopExit::Escaped(v) => ThunkOutcome::Escaped(v),
    })
}

/// Handle a VM-intercepted control primitive call.
///
/// Returns `Ok(())` for normal completion — either the result is in `dst`, or
/// a frame is on the stack for the dispatch loop to run. Which of the two it
/// is, is the frame depth's to say, and [`call_any`] is where that question
/// gets asked; nothing here reports it.
///
/// And `Err(VmError::ContinuationEscape)` when the arm transferred control:
/// `abort-current-continuation` always does, so the close-out its callers run
/// after the call — `pop_resolved_extents`, the exit-depth check in
/// `tail_call_value_with_probe` — is unreachable for that one.
///
/// **No `exit_depth`.** One was threaded in from `call_value` until 2026-09-05
/// and read by nothing: the only arm that took it was `Apply`, which handed it
/// straight back to `call_value`, which had no other use for it either. It
/// could not have had one — the non-tail path pops no frames, so it can never
/// reach an exit depth — and its cost was to make this dispatcher look
/// unreachable from the callers that have no such depth to give, which is why
/// [`call_any`] went without control primitives for as long as it did
/// (issue #186). The tail path does make exit-depth decisions, and makes them
/// itself, around this call rather than inside it.
///
/// # State contract
///
/// Requires a live caller with a valid dst slot. May mutate any dynamic
/// component, heap and code stores. Ok schedules frames or delivers into dst;
/// Err must propagate before any caller-owned close-out or result write.
fn handle_control_primitive(
    state: &mut VmState,
    ctrl: VmControlPrimitive,
    args: &[TaggedValue],
    dst: u16,
) -> Result<(), VmError> {
    match ctrl {
        VmControlPrimitive::DynamicWind => {
            if args.len() != 3 {
                return Err(VmError::ArityMismatch {
                    expected: "3".into(),
                    got: args.len(),
                });
            }
            // Only the value form reaches here; head-position `dynamic-wind`
            // compiles to `PushWind`/`PopWind`. Run the same instructions in
            // a frame of the machine's own, so that everything this call
            // still owes — its record, its after-thunk, the delivery of its
            // body's value — is a pc a re-entering continuation restores
            // along with the frame, rather than a Rust frame it cannot
            // (issue #157, and PR #156's move for a jump's wind thunks).
            let code = value_wind_stub(state)?;
            let base = state.alloc_registers(value_wind::NUM_REGS);
            state.frames.push(CallFrame {
                pc: 0,
                register_base: base,
                num_regs: value_wind::NUM_REGS,
                closure: None,
                return_reg: dst,
                code,
            });
            // Through `set_reg_at`, not the raw slice: the frame is already
            // pushed, so `base` *is* `frame_base()`, and the debug assert
            // keeps that machine-checked if these writes are ever moved above
            // the push.
            state.set_reg_at(base, value_wind::BEFORE, args[0]);
            state.set_reg_at(base, value_wind::BODY, args[1]);
            state.set_reg_at(base, value_wind::AFTER, args[2]);
        }

        VmControlPrimitive::CallWithContinuationPrompt => {
            // (call-with-continuation-prompt body [tag [handler] arg ...])
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            let body = args[0];
            let tag = if args.len() > 1 {
                args[1]
            } else {
                // No tag: use a fresh default tag (not ideal but functional for A6)
                use patina_core::cps_expr::PromptTag;
                state
                    .heap
                    .borrow_mut()
                    .alloc_prompt_tag(std::rc::Rc::new(PromptTag::new("default")))
            };
            let handler = if args.len() > 2 {
                args[2]
            } else {
                TaggedValue::FALSE
            };
            let prompt_idx = state.prompt_stack.len();
            state.prompt_stack.push(PromptFrame {
                tag,
                stack_depth: state.frames.len(),
                dynamic_wind_depth: state.dynamic_winds.len(),
                exception_handler_depth: state.exception_handlers.len(),
                handler,
                dst,
            });
            // Anything past the handler goes to the body, as Racket's does.
            // These were dropped on the floor until the review of #175 — a
            // one-argument body was called with none.
            //
            // `call_any`, not `call_closure`: the body is any procedure, which
            // is what Racket, Guile and the tree-walker all take — a
            // primitive, a parameter object, a continuation. `call_closure`
            // took a compiled closure and nothing else (issue #179).
            // `Some` is a body that needed no frame and has already finished —
            // a primitive, a parameter object, a delimited continuation that
            // was the identity. No `Return` is coming to carry the prompt off
            // by depth, so this is the one call site that closes its own
            // prompt. `None` means a frame was pushed and the ordinary sweep
            // will do it.
            if let Some(result) = call_any(state, body, args.get(3..).unwrap_or(&[]), dst)? {
                // `truncate(prompt_idx)`, not `pop()`: the top of the stack is
                // not necessarily the frame this call pushed. A no-frame body
                // can still re-enter the VM — `assoc` with a comparator that
                // opens a prompt of its own, a parameter converter that does —
                // and a prompt left above ours is what `pop()` would take,
                // closing someone else's and leaving this one live for the
                // next abort to land on.
                state.prompt_stack.truncate(prompt_idx);
                state.set_reg(dst, result);
            }
        }

        VmControlPrimitive::AbortCurrentContinuation => {
            // (abort-current-continuation tag val...)
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            let tag = args[0];
            let val = if args.len() > 1 {
                args[1]
            } else {
                TaggedValue::UNSPECIFIED
            };

            let prompt_idx = find_prompt(state, tag)?;
            return Err(abort_to_prompt(state, prompt_idx, val, dst));
        }

        VmControlPrimitive::CallWithCurrentContinuation => {
            // (call/cc proc) — capture the current continuation and call proc with it
            if args.len() != 1 {
                return Err(VmError::ArityMismatch {
                    expected: "1".into(),
                    got: args.len(),
                });
            }
            let proc = args[0];
            // `dst` is dead at this point — the only writes it will ever
            // see are this call's result or a value delivered through the
            // continuation — so whatever it still holds must not go into
            // the snapshot. Left in, it chains: `guard`'s expansion is
            // `((call/cc …))`, so `dst` holds the thunk the *previous* guard
            // delivered, that thunk closes over its `handler-k`, and that
            // continuation's snapshot holds the one before. A loop catching
            // one raise per iteration retained nine heap objects per
            // iteration for the life of its frame (296 MB at 160k).
            state.set_reg(dst, TaggedValue::NULL);
            // Capture a full continuation: snapshot of entire current state
            let cont = VmContinuation {
                frames: state.frames.clone(),
                dynamic_winds: state.dynamic_winds.clone(),
                prompt_stack: state.prompt_stack.clone(),
                exception_handlers: state.exception_handlers.clone(),
                registers: state.registers.clone(),
                deliver_reg: dst,
            };
            let cont_tv = state.alloc_vm_continuation(cont);
            // Call proc with the continuation object.
            // Proc could be a primitive or a VM closure.
            if let Some(result) = call_any(state, proc, &[cont_tv], dst)? {
                state.set_reg(dst, result);
            }
        }

        VmControlPrimitive::Values => {
            // (values v1 v2 ...) — one value is itself; any other count is a
            // #<values> heap object, which is how multiple values travel
            // everywhere (call-with-values unpacks it, the display layer
            // shows it, the tree-walker does the same). There is no side
            // channel: a register-only protocol cannot go stale when a
            // `values` call is discarded.
            let packed = state.heap.borrow_mut().values_from(args.to_vec());
            state.set_reg(dst, packed);
        }

        VmControlPrimitive::CallWithValues => {
            // (call-with-values producer consumer)
            if args.len() != 2 {
                return Err(VmError::ArityMismatch {
                    expected: "2".into(),
                    got: args.len(),
                });
            }
            let producer = args[0];
            let consumer = args[1];
            // Run producer (0 args); its multiple values, if any, are a
            // #<values> object in the result.
            let primary = match run_thunk_outcome(state, producer)? {
                ThunkOutcome::Returned(v) => v,
                // The producer escaped. Running the consumer here would run it
                // on the escape value and then overwrite that value with the
                // consumer's result — the whole `call-with-values` call is
                // abandoned, not completed.
                ThunkOutcome::Escaped(v) => return Err(park_escape(state, v)),
            };
            let produced_vals = unpack_values(state, primary);
            // Consumer may be a primitive (e.g. `list`) or a VM closure.
            if let Some(result) = call_any(state, consumer, &produced_vals, dst)? {
                state.set_reg(dst, result);
            }
        }

        VmControlPrimitive::Apply => {
            // (apply proc arg ... arg-list) — R7RS §6.10.
            //
            // Nothing implements `apply` in the primitive registry, because
            // the work is spreading a list into a real call and only the VM
            // can do that. Head position does not need one: the desugarer
            // lowers `(apply f xs)` to `Instruction::Apply`. Every other route
            // arrives here — `apply` as a value, and also `(apply +)`, which
            // the desugarer's own arity check hands back to the value path.
            if args.len() < 2 {
                return Err(VmError::ArityMismatch {
                    expected: "at least 2".into(),
                    got: args.len(),
                });
            }
            let callee = args[0];
            // Fixed arguments, then the final list spread onto the end.
            let mut call_args: Vec<TaggedValue> = args[1..args.len() - 1].to_vec();
            call_args.extend(spread_apply_tail(state, *args.last().unwrap())?);
            // The same dispatcher `Instruction::Apply` uses, so the value form
            // and the head-position form accept the same callees.
            return call_value(state, callee, &call_args, dst);
        }

        // ── Exception handling ────────────────────────────────────────────
        VmControlPrimitive::WithExceptionHandler => {
            // (with-exception-handler handler thunk)
            if args.len() != 2 {
                return Err(VmError::ArityMismatch {
                    expected: "2".into(),
                    got: args.len(),
                });
            }
            let handler_proc = args[0];
            let thunk = args[1];

            // Verify both are callable. The handler may be a continuation —
            // `(call/cc (lambda (k) (with-exception-handler k thunk)))` is
            // R7RS's idiom for capturing a raised object.
            {
                let heap = state.heap.borrow();
                if !heap.is_callable(handler_proc) {
                    return Err(VmError::TypeError {
                        message: "with-exception-handler: first argument must be a procedure"
                            .into(),
                    });
                }
                if !heap.is_callable(thunk) {
                    return Err(VmError::TypeError {
                        message: "with-exception-handler: second argument must be a procedure"
                            .into(),
                    });
                }
            }

            // Push exception handler (captures the frame depth). The handler is
            // popped when the thunk returns (via pop_exception_handlers) or when
            // raise invokes it. It records no wind depth: a raise does not
            // unwind, so there is nothing to unwind *to*.
            let handler_index = state.exception_handlers.len();
            state.exception_handlers.push(ExceptionHandler {
                handler: handler_proc,
                stack_depth: state.frames.len(),
            });

            // A frameless result sends no Return to sweep this extent. Close
            // by the saved index: re-entry can leave entries above our own.
            // On an error/transfer, leave cleanup to the owning dispatch loop.
            if let Some(result) = call_any(state, thunk, &[], dst)? {
                state.exception_handlers.truncate(handler_index);
                state.set_reg(dst, result);
            }
        }

        VmControlPrimitive::Raise => {
            vm_raise(state, args, dst, false)?;
        }

        VmControlPrimitive::RaiseContinuable => {
            vm_raise(state, args, dst, true)?;
        }

        VmControlPrimitive::Error => {
            // (error message obj ...)
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            // R7RS 6.11 says the message *should* be a string — advice, not a
            // requirement. A non-string is displayed instead of refused; see
            // the `error` primitive in patina-primitives for why.
            let message = {
                let as_string = state.heap.borrow().get_string_contents(args[0]);
                match as_string {
                    Some(s) => s,
                    None => patina_primitives::primitives::io::datum_writer::format_display_tagged(
                        args[0],
                        &state.heap,
                    ),
                }
            };
            let irritants = args[1..].to_vec();

            // Create exception object on heap
            let exception_tv = state.heap.borrow_mut().alloc_exception(
                patina_core::ExceptionKind::Error,
                message,
                irritants,
            );

            // Raise it (non-continuable)
            vm_raise_value(state, exception_tv, dst, false)?;
        }
    }
    Ok(())
}

/// Implement `raise` / `raise-continuable`.
///
/// # State contract
///
/// Requires a live raise site and valid result slot. Checks one-argument
/// arity before delegating to vm_raise_value; has the same stack effects.
fn vm_raise(
    state: &mut VmState,
    args: &[TaggedValue],
    dst: u16,
    continuable: bool,
) -> Result<(), VmError> {
    if args.len() != 1 {
        return Err(VmError::ArityMismatch {
            expected: "1".into(),
            got: args.len(),
        });
    }
    vm_raise_value(state, args[0], dst, continuable)
}

/// Raise an exception value through the handler stack.
///
/// Pops the handler, then pushes the frame that calls it — `raise_step_stub`,
/// whose instructions after the call are what this raise still owes: putting
/// the handler back for a continuable raise, raising the secondary exception
/// for a non-continuable one. This returns as soon as that frame is pushed;
/// the caller's run loop drives it, exactly as it does for any other call.
///
/// **The debt is a frame because it has to be captured.** Until issue #178 it
/// was Rust: the continuable path ran the handler on a nested
/// `run_loop_until_outcome` and did its bookkeeping after, and the
/// non-continuable path could only notice a handler that returned when it was
/// a primitive that never pushed a frame at all. A continuation captured
/// inside the handler — `guard`'s `handler-k`, or a composable continuation
/// captured by an abort — replayed the handler's frames and returned straight
/// past all of it. Three divergences came from that one shape, and they are
/// one fix.
///
/// # State contract
///
/// Requires a live raise site and dst valid if the raise can return.
/// On handled success pops one handler and appends a stub frame/window;
/// winds and prompts stay unchanged. No handler yields a SchemeException.
pub(super) fn vm_raise_value(
    state: &mut VmState,
    exception: TaggedValue,
    dst: u16,
    continuable: bool,
) -> Result<(), VmError> {
    if let Some(handler_entry) = state.exception_handlers.pop() {
        // The wind stack is left exactly as the raise found it. R7RS 6.11
        // calls the handler "in the dynamic environment of the call to
        // `raise`, except that the current exception handler is the outer
        // one" — the pop above is that exception, and it is the only one.
        //
        // A raise crosses no dynamic extent, so no after-thunk is due. This
        // used to unwind to the handler's own installation depth first, which
        // ran an after-thunk nothing had asked for and left the extent before
        // the handler could see it. `guard` then had nowhere to go back to, so
        // a declining clause could not re-raise where R7RS says it must
        // (Track L triage families 22 and 28).
        //
        // The unwind still happens for `guard` — one level up, where it
        // belongs. `guard-k` is an ordinary continuation, and jumping to it
        // runs the after-thunks through the wind machinery that already
        // handles every other control transfer.
        // One shape for both kinds of raise: push the stub that calls the
        // handler, and let the run loop drive it like any other call. The
        // difference between them is a register the stub reads afterwards.
        let code = raise_step_stub(state)?;
        // How far below the stub's own frame the handler was installed.
        // Stored as a distance because the stub frame can be captured and
        // replayed at another depth; see `raise_step::DEPTH_BELOW`.
        // Measured from the floor of the smallest region that could replay
        // this frame, not from the handler's own installation.
        //
        // A delimited capture is bounded below by a prompt, so a handler
        // installed *outside* the innermost prompt sits at a depth no such
        // region contains. Reinstating at that depth on a replay puts the
        // entry below every frame the region owns, where
        // `pop_exception_handlers` never reaches it — the handler then
        // outlives the extent it belongs to, and which handler answers a
        // later raise depends on how many frames happened to separate the
        // `with-exception-handler` from the prompt. Clamping to the prompt's
        // own depth makes the entry go when the resumed region does, which is
        // what Guile answers.
        let floor = state
            .prompt_stack
            .last()
            .map_or(handler_entry.stack_depth, |prompt| {
                handler_entry.stack_depth.max(prompt.stack_depth)
            });
        let below = state.frames.len().saturating_sub(floor);
        let base = push_stub_frame(state, code, raise_step::NUM_REGS);
        // `push_stub_frame` leaves `return_reg` at 0, which is right for the
        // stubs that deliver their own value. This one returns through the
        // ordinary `Return`, so it needs the raise's own destination.
        state.frames.last_mut().expect("just pushed").return_reg = dst;
        state.set_reg_at(base, raise_step::HANDLER, handler_entry.handler);
        state.set_reg_at(base, raise_step::EXCEPTION, exception);
        state.set_reg_at(
            base,
            raise_step::DEPTH_BELOW,
            TaggedValue::fixnum(below as i64),
        );
        state.set_reg_at(
            base,
            raise_step::CONTINUABLE,
            TaggedValue::boolean(continuable),
        );
        Ok(())
    } else {
        // No handler — format and propagate as Rust error
        use patina_primitives::primitives::io::datum_writer::format_display_tagged;
        let display = format_display_tagged(exception, &state.heap);
        // Deliberately the same wording whether or not the raise was
        // continuable — the variant's `Display` supplies it. Continuability is
        // an implementation detail once nothing handles it, and since
        // `guard`'s re-raise is `raise-continuable` (R7RS 7.3), saying
        // "continuable" here reported a plain `(raise 'x)` whose guard
        // declined as `unhandled continuable exception: x` — naming a form
        // the user never wrote.
        Err(VmError::SchemeException { message: display })
    }
}

/// Classify whether a VmError should be catchable by Scheme exception handlers.
///
/// # State contract
///
/// Pure classification; no VM state is read or changed. The driver must check
/// pending_escape first, since a wrapped escape can have a catchable error type.
pub(super) fn is_catchable(err: &VmError) -> bool {
    match err {
        VmError::StackOverflow | VmError::Compile(_) | VmError::ContinuationEscape => false,
        VmError::WithLocation { error, .. } => is_catchable(error),
        _ => true,
    }
}

/// Convert a VmError to an ExceptionKind + message for Scheme-level exception objects.
///
/// # State contract
///
/// Pure error conversion; no VM state is read or changed. Use only after
/// excluding parked transfers and checking is_catchable in the driver.
pub(super) fn classify_error(err: &VmError) -> (patina_core::ExceptionKind, String) {
    use patina_core::ExceptionKind;
    match err {
        VmError::UnboundVariable { name } => (
            ExceptionKind::Error,
            format!("Undefined variable: {}", name),
        ),
        VmError::ArityMismatch { expected, got } => (
            ExceptionKind::Error,
            format!(
                "Wrong number of arguments: expected {}, got {}",
                expected, got
            ),
        ),
        VmError::TypeError { message } => {
            // Strip "Type error: " prefix if present (matches tree-walker behavior)
            let msg = message
                .strip_prefix("Type error: ")
                .unwrap_or(message)
                .to_string();
            (ExceptionKind::Error, msg)
        }
        VmError::NoMatchingPrompt => (ExceptionKind::Error, "no matching prompt tag".to_string()),
        VmError::DivideByZero => (ExceptionKind::Error, "Division by zero".to_string()),
        VmError::Runtime { message } => {
            // Classify sub-errors by message content (matches tree-walker behavior)
            let kind = if message.contains("Cannot open")
                || message.contains("Cannot delete")
                || message.contains("Cannot read")
                || message.contains("Cannot write")
                || message.contains("No such file")
                || message.contains("file")
            {
                ExceptionKind::FileError
            } else if message.contains("read:") || message.contains("parse") {
                ExceptionKind::ReadError
            } else {
                ExceptionKind::Error
            };
            // Strip "Type error: " prefix if present (primitive errors come
            // through here via e.to_string() which includes the prefix)
            let msg = message
                .strip_prefix("Type error: ")
                .unwrap_or(message)
                .to_string();
            (kind, msg)
        }
        VmError::SchemeException { message } => (ExceptionKind::Error, message.clone()),
        // Unwrap location wrapper and classify the inner error.
        VmError::WithLocation { error, .. } => classify_error(error),
        // Non-catchable (shouldn't reach here due to is_catchable check)
        VmError::StackOverflow | VmError::Compile(_) | VmError::ContinuationEscape => {
            (ExceptionKind::Error, err.to_string())
        }
    }
}

/// Pop any `PromptFrame`s whose `stack_depth` meets or exceeds the current frame depth
/// (i.e., the prompt's body returned normally).
///
/// # State contract
///
/// Use only after normal value delivery or full-jump arrival, never merely
/// because a tail call popped a frame. Mutates only the prompt stack.
fn pop_resolved_prompts(state: &mut VmState) {
    while let Some(pf) = state.prompt_stack.last() {
        if pf.stack_depth >= state.frames.len() {
            state.prompt_stack.pop();
        } else {
            break;
        }
    }
}

/// Close the extents keyed on frames that are no longer on the stack. Called
/// wherever the frame stack has just shrunk and the departing frame's value
/// has been delivered: `Return`, and each tail-position branch that pops the
/// frame itself because its callee (a control primitive, a primitive, a
/// parameter, a `call-with-values` consumer) delivers straight to the caller.
///
/// A handler is live exactly while the thunk it was installed for has a
/// frame, which is `stack_depth < frames.len()`; the same holds of prompts.
/// So the pops are safe at any such point, and skipping them at one leaves a
/// stale entry that the next raise finds — the tail-position branches used to
/// skip them, and `(with-exception-handler h (lambda () (values 1)))` left
/// `h` installed for the rest of the program.
///
/// Wind records are *not* among the extents closed here, and no longer carry
/// a depth to close them by. Every record is pushed by `PushWind` and popped
/// by the `PopWind` that follows it in the same instruction sequence, or by
/// the jump that leaves its extent. A sweep by depth existed until 2026-09-02
/// as the only cleanup the **value form** of `dynamic-wind` had left once a
/// continuation abandoned its Rust frame; the value form runs `PushWind` and
/// `PopWind` itself now ([`value_wind_stub`]), and the sweep was reachable on
/// no input.
///
/// **Nothing is popped at the run loop's exit depth.** The depth test cannot
/// decide there: a handler the loop was *started under* — its thunk's frame
/// tail-replaced by the control primitive that started the loop, so it sits
/// at exactly `exit_depth` — is live, while one a tail-called
/// `with-exception-handler` installed *inside* the loop sits at the same
/// depth and is dead. `run_loop_until_outcome` closes the second kind from
/// its own entry count. Prompts at that depth belong to the Rust caller that
/// started the loop (`run_thunk_outcome`, a prompt body), which closes them
/// itself.
///
/// # State contract
///
/// Requires completed value delivery after a frame return, not an in-flight
/// transfer. Pops resolved prompts/handlers except at the driver exit depth;
/// leaves frames, registers and winds untouched.
pub(super) fn pop_resolved_extents(state: &mut VmState, exit_depth: usize) {
    if state.frames.len() == exit_depth || state.frames.is_empty() {
        return;
    }
    pop_resolved_prompts(state);
    pop_exception_handlers(state);
}

/// Pop exception handlers whose thunk has returned (stack shrank below their depth).
///
/// # State contract
///
/// Requires a completed thunk return at a depth the owning driver may sweep.
/// Mutates only the handler stack; no Scheme call or wind traversal.
fn pop_exception_handlers(state: &mut VmState) {
    while let Some(eh) = state.exception_handlers.last() {
        if eh.stack_depth >= state.frames.len() {
            state.exception_handlers.pop();
        } else {
            break;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Continuation jumps and their wind thunks
// ─────────────────────────────────────────────────────────────────────────────

/// The registers of a wind-step stub frame — the jump itself, held where a
/// `call/cc` inside the thunk will snapshot it along with every other
/// register. See [`Instruction::ResumeWindJump`].
pub(super) mod wind_step {
    /// The continuation being jumped to.
    pub(in crate::runtime) const TARGET: u16 = 0;
    /// The value it delivers on arrival.
    pub(in crate::runtime) const VALUE: u16 = 1;
    /// Index into the *target's* wind stack of the record whose `before`
    /// thunk this step is running, or `#f` for an `after` thunk (whose record
    /// was popped before it ran and is never pushed back).
    pub(in crate::runtime) const ENTERING: u16 = 2;
    /// Where the thunk returns. A wind thunk's value is discarded, but
    /// `Return` needs a slot to write into.
    pub(in crate::runtime) const THUNK_RESULT: u16 = 3;
    /// Window size of a stub frame.
    pub(in crate::runtime) const NUM_REGS: u16 = 4;
}

/// Take the next step of a jump to the full continuation `target`: run one
/// wind thunk between the live wind stack and the target's, or, with none
/// left, arrive — restore the target's state and deliver `value`.
///
/// This is chibi's "travel to point", one thunk per step: leave the innermost
/// extent not shared with the target (pop its record, run its `after`), until
/// the live stack is a prefix of the target's; then enter the target's
/// remaining extents outermost first (run `before`, push the record). Each
/// thunk runs under a stub frame whose only instruction comes back here
/// ([`Instruction::ResumeWindJump`]), so the thunks are ordinary frames of
/// the machine the jump was made on rather than nested Rust calls.
///
/// Each thunk also runs in the dynamic environment of its own `dynamic-wind`
/// call (R7RS §6.10): the wind stack below its record, and the handler stack
/// the record captured. So a raise in an after-thunk reaches the `guard`
/// whose escape is running it, the guard's handler fires a second time, and
/// its second jump — starting from the stack this one had got to — abandons
/// this jump and runs the after-thunks still outstanding. That is the
/// `finally` rule of Track L §6: the after-thunk's exception replaces the one
/// in flight, and unwinding continues.
///
/// Popping the record *before* running its after-thunk is what makes the
/// second jump terminate: the thunk is not on the stack it jumps from.
///
/// The caller signals the dispatch loop the way every continuation invoke
/// does — park the value, return the sentinel — and the loop decides whether
/// the frames now on the stack are its own to run.
///
/// # State contract
///
/// Requires a live full-continuation handle. A scheduled step mutates winds,
/// handlers, frames and registers; arrival replaces all five components and
/// delivers value. Ok does not mean an ordinary call returned: the caller
/// must park the transfer and yield to the owning driver.
pub(super) fn step_wind_jump(
    state: &mut VmState,
    target: TaggedValue,
    value: TaggedValue,
) -> Result<(), VmError> {
    let cc = state
        .get_vm_continuation(target)
        .ok_or_else(|| VmError::TypeError {
            message: "continuation jump: not a full continuation".into(),
        })?;
    let step = next_wind_step(&state.dynamic_winds, &cc.dynamic_winds, |r| r.id);

    // Leaving an extent: pop first, then run its after-thunk.
    if step == WindStep::Exit {
        let record = state
            .dynamic_winds
            .pop()
            .expect("longer than its own prefix");
        return push_wind_step(
            state,
            target,
            value,
            TaggedValue::FALSE,
            record.after,
            &record.handlers,
        );
    }

    // Entering one: run its before-thunk, and push the record only when that
    // returns — `ResumeWindJump` does it, from `ENTERING`.
    //
    if let WindStep::Enter(index) = step {
        let record = &cc.dynamic_winds[index];
        let entering = TaggedValue::fixnum(index as i64);
        return push_wind_step(
            state,
            target,
            value,
            entering,
            record.before,
            &record.handlers,
        );
    }

    // Arrived.
    state.registers = cc.registers.clone();
    state.frames = cc.frames.clone();
    state.dynamic_winds = cc.dynamic_winds.clone();
    state.prompt_stack = cc.prompt_stack.clone();
    state.exception_handlers = cc.exception_handlers.clone();
    // A snapshot can carry a prompt whose body is already finished, and this
    // is the one moment that can tell (issue #176).
    //
    // `pop_resolved_prompts` reads `stack_depth >= frames.len()` as "the
    // body's frame is gone, so the prompt is over". That reading is wrong at
    // *capture* time: a body whose tail expression is a control primitive —
    // `(call/cc …)` — has had its frame popped by the tail call while the
    // primitive is still to deliver its value, and an abort from inside it
    // must still find the prompt. Guile and Racket both answer that it does,
    // and `tests/scheme/control/prompts.scm` pins it.
    //
    // Here the same reading is exact, because the thing that made it wrong
    // cannot be pending: the value is being delivered *now*, by this arrival,
    // into the frame the snapshot restored. A prompt with no frame above it
    // has therefore already delivered its own value — the only thing that
    // could still have delivered it was the frame that is gone. Left in
    // place it is invisible to every later sweep, since no Return will ever
    // cross its depth again, and the next abort in the program lands on a
    // prompt whose body returned long before.
    pop_resolved_prompts(state);
    // Deliver into `deliver_reg` of the top frame. Any `base` the caller
    // hoisted is stale here — the whole register file was just replaced.
    if let Some(top) = state.frames.last() {
        let top_base = top.register_base;
        state.registers[top_base + cc.deliver_reg as usize] = value;
    }
    Ok(())
}

/// Push a stub frame carrying the rest of the jump, install the handler stack
/// the thunk's `dynamic-wind` call had, and call the thunk under it.
///
/// # State contract
///
/// Requires a valid target handle and a popped exiting record, or an
/// unpushed entering record. Installs the supplied handlers, roots transfer
/// operands in a stub, and calls the thunk. Calls can change all dynamic state.
fn push_wind_step(
    state: &mut VmState,
    target: TaggedValue,
    value: TaggedValue,
    entering: TaggedValue,
    thunk: TaggedValue,
    handlers: &[ExceptionHandler],
) -> Result<(), VmError> {
    install_thunk_handlers(state, handlers);
    let code = wind_jump_stub(state)?;
    let base = push_stub_frame(state, code, wind_step::NUM_REGS);
    // `set_reg_at` for the reason the value form's stub gives: the frame is
    // pushed, so the assert holds and keeps holding.
    state.set_reg_at(base, wind_step::TARGET, target);
    state.set_reg_at(base, wind_step::VALUE, value);
    state.set_reg_at(base, wind_step::ENTERING, entering);
    // A primitive thunk returns here and now; its value is discarded either
    // way, and the stub frame is left on top for `ResumeWindJump` to run.
    call_any(state, thunk, &[], wind_step::THUNK_RESULT)?;
    Ok(())
}

/// Make `handlers` — the stack of the `dynamic-wind` call a thunk belongs to
/// — the live handler stack for the length of that thunk.
///
/// The depths inside a record's handlers are frame depths of a stack that is
/// not the live one: the record may have been made far below the jump site,
/// or, on a re-entry, above it. `pop_exception_handlers` reads them
/// literally and would drop a handler as soon as a frame at or above its
/// recorded depth returned, so each is clamped to the depth the thunk starts
/// at. Clamping against one constant preserves the stack's non-decreasing
/// order, and nothing else reads the field.
///
/// There is no restore, and none is owed — *for a jump*, whose target replaces
/// the machine. Every step installs a stack and arrival installs the target's
/// own, so the jump site's stack is never the right one to come back to. That
/// argument does not carry to a transfer whose target would *extend* the live
/// machine, and a composable invoke is one: there the invoke site's stack is a
/// prefix of the right answer rather than an irrelevance, which is why its
/// re-entry thunks run under the live stack and not through here. R7RS 6.10
/// puts the thunk in its own
/// call's dynamic environment, not the jump's. That holds for a `VmError` the
/// thunk's *call* raises too (a `dynamic-wind` whose after is `42`): it is
/// routed through the record's handlers, which is where a raise from the thunk
/// belongs. A travel abandoned by an error that nothing catches abandons the
/// machine with it — `execute` clears the frames and every stack on the way
/// out.
///
/// # State contract
///
/// Requires the wind thunk's current frame depth. Replaces only handlers,
/// clamping their saved depths to the live stack; does not call Scheme.
fn install_thunk_handlers(state: &mut VmState, handlers: &[ExceptionHandler]) {
    let depth = state.frames.len();
    state.exception_handlers.clear();
    state
        .exception_handlers
        .extend(handlers.iter().map(|h| ExceptionHandler {
            handler: h.handler,
            stack_depth: h.stack_depth.min(depth),
        }));
}

/// A code object the runtime builds rather than compiles, memoised in `slot`.
///
/// Both callers want the same three properties, and stating them once is the
/// point of the helper. The object goes through `state.load`, so the GC's
/// "every frame's code came from the store" invariant (`gc_roots.rs`) holds
/// without qualification — none of its five stubs has constants to trace, but
/// the invariant is cheaper to keep than to caveat, and a stub that ever does
/// need them inherits the rule rather than having to discover it. It is built at most once per
/// `VmState`, and `slot` holds the id rather than the `Rc` because
/// `code_store` is the one owner. And its `source_map` is empty, which
/// `attach_source_location` reads as "not a place in the program" and steps
/// past — see there.
///
/// # State contract
///
/// Loads/caches a code object in the supplied VmState slot. Changes only
/// the code store/cache, not frames, registers or dynamic-context stacks.
fn runtime_stub(
    state: &mut VmState,
    slot: impl Fn(&VmState) -> Option<CodeObjectId>,
    set_slot: impl Fn(&mut VmState, CodeObjectId),
    name: &str,
    instructions: Vec<Instruction>,
    num_regs: u16,
) -> Result<Rc<CodeObject>, VmError> {
    if let Some(id) = slot(state) {
        return state.code_object(id);
    }
    let id = CodeObjectId::fresh();
    state.load(CodeObject {
        id,
        name: Some(Rc::from(name)),
        global_cache: GlobalCacheEntry::table(&instructions),
        instructions,
        constants: Vec::new(),
        num_regs,
        arity: Arity::Fixed(0),
        source_map: Vec::new(),
    });
    set_slot(state, id);
    state.code_object(id)
}

/// Push a frame running one of the runtime's own stubs, and return its
/// register base.
///
/// The boilerplate the two thunk-stepping stubs share — `ResumeWindJump` for a
/// jump and `ResumeComposableInvoke` for a composable invoke's extent
/// re-entry. What they do **not** share is deliberately left at the call
/// sites: the jump installs its record's handler stack and the invoke does
/// not, because a jump's target replaces the machine and an invoke's extends
/// it. Passing that as a flag would read as a knob; leaving it as one line at
/// one of two call sites reads as the decision it is.
///
/// The two thunk-stepping stubs leave `return_reg` at 0 and nothing ever
/// returns *into* their frames: the instruction pops the frame itself and
/// either starts the next thunk or finishes the transfer.
///
/// `raise_step_stub` is the third caller and breaks that: its `Call handler`
/// **does** return into the frame, and its own `Return` delivers the
/// handler's value to the raise's destination — so `vm_raise_value` patches
/// `return_reg` straight after this call. Anything here that assumed the
/// old invariant (dropping `return_reg` for stub frames, freeing the window
/// in a Resume arm) would break the raise path silently.
///
/// # State contract
///
/// Requires loaded stub code and its matching register count. Appends one
/// frame/window, initially returning to r0; caller fills operands before yielding.
/// Does not change winds, prompts or handlers.
fn push_stub_frame(state: &mut VmState, code: Rc<CodeObject>, num_regs: u16) -> usize {
    let base = state.alloc_registers(num_regs);
    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs,
        closure: None,
        return_reg: 0,
        code,
    });
    base
}

/// The one-instruction code object every wind step's frame runs.
///
/// # State contract
///
/// Returns loaded/cached stub code via runtime_stub. Changes only the code
/// store/cache; no frame is pushed and no Scheme code runs.
fn wind_jump_stub(state: &mut VmState) -> Result<Rc<CodeObject>, VmError> {
    runtime_stub(
        state,
        |s| s.wind_jump_code,
        |s, id| s.wind_jump_code = Some(id),
        "wind-jump",
        vec![Instruction::ResumeWindJump],
        wind_step::NUM_REGS,
    )
}

/// The registers of the stub frame the **value** form of `dynamic-wind` runs
/// in — its three thunks, its body's value, and one slot for the two thunks
/// whose values are discarded. See [`value_wind_stub`].
mod value_wind {
    /// The before-thunk.
    pub(super) const BEFORE: u16 = 0;
    /// The body thunk.
    pub(super) const BODY: u16 = 1;
    /// The after-thunk.
    pub(super) const AFTER: u16 = 2;
    /// The body's value, which is the call's value. Held across the
    /// after-thunk, so it cannot share a slot with it.
    pub(super) const RESULT: u16 = 3;
    /// Where the before- and after-thunks return. Their values are discarded,
    /// but `Return` needs a slot to write into.
    pub(super) const DISCARD: u16 = 4;
    /// Window size of the stub frame.
    pub(super) const NUM_REGS: u16 = 5;
}

/// The code object the value form of `dynamic-wind` runs — the six
/// instructions `pass5_codegen` emits for head position, in a frame of their
/// own because there is no call site to emit them into.
///
/// **Kept in step by hand with `pass5_codegen.rs`'s `dynamic-wind` case**,
/// which is the price of the two entry points; a change to one is a change to
/// the other, and each names the other so an editor of either finds it. The
/// two are not textually identical and are not meant to be: pass 5 emits
/// `Return` only in tail position and lets the discarded thunk results land in
/// `expr.dst` and the before-thunk's own register, where a frame of its own
/// can afford a dedicated slot and always returns. The *order and identity* of
/// the instructions is what has to match, because that is what the two forms
/// agreeing depends on.
///
/// The point is not to save the nested Rust loop but to make this call's
/// remaining obligations *resumable*. A continuation captured in the body
/// restores the VM's frames, and this frame's pc is one of them, so a
/// re-entry that returns through here still pops the record, still runs this
/// call's own after-thunk, and still delivers the body's value to the caller
/// — none of which a Rust frame abandoned by the jump could do (issue #157).
///
/// Built by [`runtime_stub`], like [`wind_jump_stub`] and for the reasons
/// stated there.
///
/// # State contract
///
/// Returns loaded/cached stub code via runtime_stub. Changes only the code
/// store/cache; no frame is pushed and no Scheme code runs.
fn value_wind_stub(state: &mut VmState) -> Result<Rc<CodeObject>, VmError> {
    let instructions = vec![
        Instruction::Call {
            func: value_wind::BEFORE,
            args: vec![],
            dst: value_wind::DISCARD,
        },
        Instruction::PushWind {
            before: value_wind::BEFORE,
            after: value_wind::AFTER,
        },
        Instruction::Call {
            func: value_wind::BODY,
            args: vec![],
            dst: value_wind::RESULT,
        },
        // Pops the record; the after-thunk is the `Call` that follows, so
        // that a jump out of *it* no longer sees this extent as entered.
        Instruction::PopWind,
        Instruction::Call {
            func: value_wind::AFTER,
            args: vec![],
            dst: value_wind::DISCARD,
        },
        Instruction::Return {
            val: value_wind::RESULT,
        },
    ];
    runtime_stub(
        state,
        |s| s.value_wind_code,
        |s, id| s.value_wind_code = Some(id),
        "dynamic-wind",
        instructions,
        value_wind::NUM_REGS,
    )
}

/// The handler stack a `dynamic-wind` record captures.
///
/// Shared when empty: `Rc<[T]>` allocates a header even for no elements, and
/// most `dynamic-wind` calls run with no handler installed at all.
///
/// # State contract
///
/// Reads handlers and clones their shared snapshot; leaves all dynamic
/// components unchanged. The returned handlers must be rooted before a safe point.
pub(super) fn captured_handlers(state: &VmState) -> Rc<[ExceptionHandler]> {
    if state.exception_handlers.is_empty() {
        return EMPTY_HANDLERS.with(Rc::clone);
    }
    Rc::from(state.exception_handlers.as_slice())
}

thread_local! {
    /// The one empty handler stack every handler-free wind record shares.
    static EMPTY_HANDLERS: Rc<[ExceptionHandler]> = Rc::from(Vec::new());
}

// ─────────────────────────────────────────────────────────────────────────────
// VmApplyContext — implements ApplyContext with higher-order proc support
// ─────────────────────────────────────────────────────────────────────────────

/// `ApplyContext` implementation for the VM.
///
/// Holds a raw pointer to the `VmState` so that `apply_proc` (which takes
/// `&self`) can mutably re-enter the VM execution loop.  This is sound because
/// `apply_proc` is only called synchronously during `call_primitive_proc`, which
/// already has exclusive `&mut VmState` access, and the pointer is never shared
/// across threads.
struct VmApplyContext {
    state: *mut VmState,
}

/// How a re-entry into the VM ended, when it did not end normally.
enum Reentry {
    /// A continuation captured outside the boundary was invoked inside it.
    /// The value it carries is on [`VmState::pending_escape`].
    Escaped,
    /// The body failed on its own terms.
    Failed(VmError),
}

/// Run `body` across a re-entry into the VM, detecting a continuation that
/// escaped past the boundary.
///
/// Every `ApplyContext` method, and every synchronous nested run, is such a
/// boundary: the Rust code below it holds a stack it loses if a continuation
/// is invoked inside. On a shrink the carried value is stashed on
/// [`VmState::pending_escape`] and the caller is told to unwind rather than
/// carry on with frames it no longer owns.
///
/// `depth_before` is the caller's own frame depth *before* it pushed anything
/// for this call — passed in rather than sampled here, because a caller that
/// has already pushed a frame (`call_any_sync`) would otherwise compare
/// against the pushed depth and read every normal return as an escape.
///
/// Route new boundaries through here. The first attempt at this fix covered
/// one boundary of four, and `load` and `parameterize` stayed broken because
/// nothing made the omission visible.
///
/// # State contract
///
/// Requires depth_before sampled before the callback pushes anything, and
/// no live heap borrow. body may change all dynamic state. On transfer returns
/// Escaped with the value parked, leaving the landing intact for its driver;
/// it performs no dynamic-stack cleanup or transaction rollback.
fn across_reentry<T>(
    state: &mut VmState,
    depth_before: usize,
    body: impl FnOnce(&mut VmState) -> Result<T, VmError>,
    value_of: impl FnOnce(&T) -> TaggedValue,
) -> Result<T, Reentry> {
    let result = body(state);
    // A transfer has abandoned this call whatever the depths say, and the
    // depths cannot say: an abort cuts every stack back to its prompt and
    // pushes one stub frame, which lands at exactly `depth_before` when the
    // prompt sits one frame below this boundary. See
    // [`VmState::pending_transfer`] for why the frames cannot be asked.
    //
    // Here rather than at each boundary, because this function is the one
    // place that decides it — its own doc has said so since the first attempt
    // at the escape fix "covered one boundary of four". Issue #177's first fix
    // covered one of four again: `force` (through `apply_proc`) worked while
    // `eval` and a tail-position parameter *set* (through `call_any_sync`,
    // which stored the abort's value into the parameter) did not.
    if state.pending_transfer {
        return Err(Reentry::Escaped);
    }
    if state.frames.len() < depth_before {
        // Any error here belongs to a call that is being abandoned; what
        // resumes is the continuation's value, not this one's outcome.
        let carried = result.as_ref().ok().map(value_of);
        state.pending_escape = Some(carried.unwrap_or(TaggedValue::UNSPECIFIED));
        return Err(Reentry::Escaped);
    }
    result.map_err(Reentry::Failed)
}

impl Reentry {
    /// The escape sentinel is deliberately `ContinuationEscape`: it is already
    /// non-catchable, so no `guard` between the primitive and the dispatch
    /// loop can swallow it.
    fn into_eval_error(self) -> patina_primitives::EvalError {
        match self {
            Reentry::Escaped => patina_primitives::EvalError::ContinuationEscape,
            Reentry::Failed(e) => patina_primitives::EvalError::InternalError(e.to_string()),
        }
    }
}

impl patina_primitives::ApplyContext for VmApplyContext {
    fn heap(&self) -> &SharedHeap {
        // SAFETY: pointer is valid for the lifetime of the primitive call.
        unsafe { &(*self.state).heap }
    }

    fn fs(&self) -> &Arc<dyn patina_core::FileSystem> {
        // SAFETY: pointer is valid for the lifetime of the primitive call.
        unsafe { &(*self.state).fs }
    }

    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, patina_primitives::EvalError> {
        // SAFETY: we have exclusive access (see struct doc comment).
        let state = unsafe { &mut *self.state };
        let depth_before = state.frames.len();
        across_reentry(
            state,
            depth_before,
            |s| run_apply_proc(s, proc, &args),
            |v| *v,
        )
        .map_err(Reentry::into_eval_error)
    }

    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<patina_core::environment::Environment>,
    ) -> Result<TaggedValue, patina_primitives::EvalError> {
        let state = unsafe { &mut *self.state };
        let depth_before = state.frames.len();
        across_reentry(state, depth_before, |s| vm_eval_expr(s, expr, env), |v| *v)
            .map_err(Reentry::into_eval_error)
    }

    fn load_scheme_library(
        &self,
        name: &[String],
    ) -> Result<Rc<patina_core::library::Library>, patina_primitives::EvalError> {
        let state = unsafe { &mut *self.state };
        // A library is not a `TaggedValue`; an escape out of a load resumes
        // with whatever the continuation carried, not with the library.
        let depth_before = state.frames.len();
        across_reentry(
            state,
            depth_before,
            |s| {
                vm_load_library(s, name).map_err(|e| VmError::Runtime {
                    message: e.to_string(),
                })
            },
            |_| TaggedValue::UNSPECIFIED,
        )
        .map(Rc::new)
        .map_err(Reentry::into_eval_error)
    }

    fn interaction_environment(&self) -> Rc<patina_core::environment::Environment> {
        // SAFETY: pointer is valid for the lifetime of the primitive call.
        unsafe { (*self.state).globals.clone() }
    }
}

/// Apply a procedure (VmClosure or primitive) to args within the VM,
/// running the execution loop if needed. Used by `VmApplyContext::apply_proc`.
///
/// # State contract
///
/// Requires a live caller under across_reentry. Uses a scratch return slot
/// and may run a nested loop, changing all dynamic state. Its caller must
/// interpret transfer flags before accepting the returned value.
fn run_apply_proc(
    state: &mut VmState,
    proc: TaggedValue,
    args: &[TaggedValue],
) -> Result<TaggedValue, VmError> {
    let depth_before = state.frames.len();

    // Use a scratch return register beyond the current frame's live registers.
    let return_reg = state.frames.last().map(|f| f.num_regs).unwrap_or(0);
    if let Some(f) = state.frames.last() {
        let needed = f.register_base + return_reg as usize + 1;
        if state.registers.len() < needed {
            state.registers.resize(needed, TaggedValue::UNSPECIFIED);
        }
    }

    if let Some(result) = call_any(state, proc, args, return_reg)? {
        // Primitive — returned immediately.
        return Ok(result);
    }
    // VM closure was pushed; run until it returns.
    //
    // `Escaped` needs no special handling here: a loop exiting at its own
    // floor means the callback delivered its value — a continuation captured
    // and invoked *inside* it does exactly that — and the one case where it
    // does not is a transfer, which `across_reentry` catches for every
    // boundary at once.
    run_loop_until(state, depth_before)
}

/// Recognized VM-intercepted control primitives.
#[derive(Clone, Copy)]
pub(crate) enum VmControlPrimitive {
    DynamicWind,
    CallWithContinuationPrompt,
    AbortCurrentContinuation,
    CallWithCurrentContinuation,
    CallWithValues,
    Values,
    Apply,
    WithExceptionHandler,
    Raise,
    RaiseContinuable,
    Error,
}

/// The single source of truth for which qualified names the VM intercepts.
/// `resolve_primitive_calls` must never emit `CallPrimitive` for any of these
/// (its exclusion is cross-checked against this table by
/// `excluded_covers_every_intercepted_primitive` in
/// `compiler/primitive_calls.rs`).
///
/// The predicate is "the registry cannot implement this — it needs the VM's
/// own call machinery". For eleven of the twelve the reason is control flow: a
/// directly dispatched registry handler would bypass the VM's
/// continuation/exception cooperation. `apply` is the exception and is why the
/// predicate is worded that way rather than as "control primitives": spreading
/// a list into a call is not control flow, but it is equally impossible from
/// inside a registry handler, and there is no handler to bypass — the registry
/// entry `(patina internal control)` exports has no body at all.
pub(crate) const VM_INTERCEPTED_PRIMITIVES: &[(&str, VmControlPrimitive)] = &[
    (
        "patina.internal.control/dynamic-wind",
        VmControlPrimitive::DynamicWind,
    ),
    (
        "patina.internal.control/call-with-continuation-prompt",
        VmControlPrimitive::CallWithContinuationPrompt,
    ),
    (
        "patina.internal.control/abort-current-continuation",
        VmControlPrimitive::AbortCurrentContinuation,
    ),
    (
        "patina.internal.control/call-with-current-continuation",
        VmControlPrimitive::CallWithCurrentContinuation,
    ),
    (
        "patina.internal.control/call/cc",
        VmControlPrimitive::CallWithCurrentContinuation,
    ),
    (
        "patina.internal.control/call-with-values",
        VmControlPrimitive::CallWithValues,
    ),
    ("patina.internal.control/values", VmControlPrimitive::Values),
    ("patina.internal.control/apply", VmControlPrimitive::Apply),
    (
        "patina.internal.errors/with-exception-handler",
        VmControlPrimitive::WithExceptionHandler,
    ),
    ("patina.internal.errors/raise", VmControlPrimitive::Raise),
    (
        "patina.internal.errors/raise-continuable",
        VmControlPrimitive::RaiseContinuable,
    ),
    ("patina.internal.errors/error", VmControlPrimitive::Error),
];

/// If `func_val` is a VM-intercepted control primitive, return which one.
///
/// # State contract
///
/// Read-only callee classification. Does not invoke Scheme or change VM state.
fn vm_control_primitive(state: &VmState, func_val: TaggedValue) -> Option<VmControlPrimitive> {
    let proc = state.heap.borrow().get_procedure(func_val)?;
    let Procedure::Primitive { qualified_name, .. } = proc.as_ref() else {
        return None;
    };
    // Cheap prefix reject before the linear string scan: every intercepted
    // primitive lives under the excluded namespaces — enforced by
    // `excluded_covers_every_intercepted_primitive` in primitive_calls.rs.
    if !crate::compiler::primitive_calls::is_excluded(qualified_name) {
        return None;
    }
    VM_INTERCEPTED_PRIMITIVES
        .iter()
        .find(|(name, _)| *name == qualified_name.as_ref())
        .map(|&(_, ctrl)| ctrl)
}

/// Try to invoke `func_val` as a full (`call/cc`) continuation. Returns the
/// value it delivers if it was one — the stack has been replaced, or the
/// first thunk of the travel pushed — and `None` if it was not.
///
/// The travel may not finish here: a jump with a wind thunk to run pushes
/// that thunk's frames and comes back through `ResumeWindJump`. Either way
/// this call is over, and the caller signals the dispatch loop.
///
/// Delimited continuations are the other half of the same probe, and are
/// [`invoke_delimited`]'s business rather than this function's: a composable
/// invoke *returns*, so its caller has to say where — which this signature
/// has no room for and its three callers each answer differently.
///
/// # State contract
///
/// None leaves state unchanged. Some means step_wind_jump scheduled or
/// completed a transfer and can have replaced all dynamic state; the caller
/// must park the returned value and propagate the escape, not store a result.
fn try_invoke_full_continuation(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
) -> Result<Option<TaggedValue>, VmError> {
    if state.get_vm_continuation(func_val).is_none() {
        return Ok(None);
    }
    let deliver_val = deliver_value(state, args);
    step_wind_jump(state, func_val, deliver_val)?;
    Ok(Some(deliver_val))
}

/// The innermost prompt carrying `tag`.
///
/// # State contract
///
/// Reads prompts only. The returned index is valid until prompt-stack
/// mutation; consume it before calling arbitrary Scheme.
pub(super) fn find_prompt(state: &VmState, tag: TaggedValue) -> Result<usize, VmError> {
    state
        .prompt_stack
        .iter()
        .rposition(|p| p.tag == tag)
        .ok_or(VmError::NoMatchingPrompt)
}

/// `abort-current-continuation`: capture the delimited continuation, leave
/// every extent between here and the prompt, and call the prompt's handler
/// with `(val, k)`.
///
/// One copy, two callers — the control primitive, and the `AbortToPrompt`
/// instruction no pass emits. They were two copies of forty lines, and both
/// of the last two corrections to this sequence had to be written twice.
/// Never returns normally. Its value is what the dispatch loop is handed: the
/// escape sentinel once the abort is under way, or a genuine failure. A caller
/// that forgot to park one used to go on dispatching into a frame stack the
/// abort had just rewritten, and the signature is the only thing that can make
/// that impossible.
///
/// # State contract
///
/// Requires a current prompt index and a valid capture-hole register if
/// frames lie above the prompt. May change every dynamic component and
/// continuation/code stores. Always return its error immediately; a parked
/// transfer belongs to the driver, not to a handler for ordinary Rust errors.
pub(super) fn abort_to_prompt(
    state: &mut VmState,
    prompt_idx: usize,
    val: TaggedValue,
    dst: u16,
) -> VmError {
    let prompt = state.prompt_stack[prompt_idx].clone();
    // `dst` is this abort call's own destination — dead as a result slot, and
    // for that reason the hole the captured continuation resumes into.
    let cont = capture_delimited(state, prompt_idx, dst);
    let cont_tv = state.alloc_vm_delimited_continuation(cont);

    // Build the machine as it will be once the abort has landed — every stack
    // cut back to the prompt — and put one stub frame on top of it whose two
    // instructions call the prompt's handler and return its value to
    // `prompt.dst`, in the frame that called `call-with-continuation-prompt`.
    // Then *jump* to it.
    //
    // Reusing the jump is the whole point (issue #165). An abort has to leave
    // every extent between here and the prompt, which is what a jump's travel
    // does, and doing it by hand instead is what made this path wrong twice
    // over: its after-thunks ran under the live handler stack rather than
    // their own record's (`push_wind_step` installs those, and had the only
    // caller), and they ran on a nested Rust loop, so a continuation captured
    // in one and re-entered restarted the thunk from the top and lost the
    // abort's value — #157's signature, on the third of the four places that
    // ran wind thunks. Travelling to a continuation gets both right, and the
    // handler call becomes a frame along with them: a continuation captured
    // in the *handler* is now an ordinary capture too.
    let stub = match abort_handler_stub(state) {
        Ok(code) => code,
        Err(e) => return e,
    };
    let landing_depth = prompt.stack_depth.min(state.frames.len());
    let wind_depth = prompt.dynamic_wind_depth.min(state.dynamic_winds.len());
    let handler_depth = prompt
        .exception_handler_depth
        .min(state.exception_handlers.len());
    let registers_end = match state.frames[..landing_depth].last() {
        Some(top) => top.register_base + top.num_regs as usize,
        None => 0,
    };

    // Nothing to leave — no extent between the abort and its prompt, which is
    // the common case — means there is no travel to run, and the landing can
    // be cut out of the live machine in place. Building it as a snapshot and
    // travelling to it instead costs a copy of every surviving register and
    // frame, plus a heap continuation and a weak-store entry, plus a second
    // copy on arrival: ~20% on a loop of 300-frame aborts, measured
    // interleaved against the previous release.
    if state.dynamic_winds.len() == wind_depth {
        state.frames.truncate(landing_depth);
        state.registers.truncate(registers_end);
        state.prompt_stack.truncate(prompt_idx);
        state.exception_handlers.truncate(handler_depth);
        push_abort_stub(
            &mut state.frames,
            &mut state.registers,
            stub,
            prompt.dst,
            prompt.handler,
            val,
            cont_tv,
        );
        return park_transfer(state, val);
    }

    // Otherwise the same landing has to be described rather than applied: the
    // travel needs the live wind stack intact to know what it is leaving, so
    // the machine cannot be cut back until it arrives.
    let mut frames = state.frames[..landing_depth].to_vec();
    let mut registers = state.registers[..registers_end].to_vec();
    push_abort_stub(
        &mut frames,
        &mut registers,
        stub,
        prompt.dst,
        prompt.handler,
        val,
        cont_tv,
    );
    let target = VmContinuation {
        frames,
        registers,
        dynamic_winds: state.dynamic_winds[..wind_depth].to_vec(),
        prompt_stack: state.prompt_stack[..prompt_idx].to_vec(),
        exception_handlers: state.exception_handlers[..handler_depth].to_vec(),
        // The jump delivers its value into the top frame's `deliver_reg` on
        // arrival. Nothing reads this one — the stub's `Call` overwrites it —
        // and the abort's value reaches the handler as an argument instead.
        deliver_reg: abort_step::RESULT,
    };
    let target_tv = state.alloc_vm_continuation(target);
    match step_wind_jump(state, target_tv, val) {
        Ok(()) => park_transfer(state, val),
        Err(e) => e,
    }
}

/// Put the frame an abort lands in on top of `frames`, with its window on the
/// end of `registers`.
///
/// One description of the landing frame, used by both of `abort_to_prompt`'s
/// paths — the one that cuts the live machine back in place and the one that
/// describes the same machine as a jump target. They differ in how they get
/// there and must not differ in where they end up.
///
/// # State contract
///
/// Requires a matched frame/register prefix and loaded abort-handler code.
/// Appends one initialized frame/window to those buffers, live or detached;
/// no other dynamic components are touched.
fn push_abort_stub(
    frames: &mut Vec<CallFrame>,
    registers: &mut Vec<TaggedValue>,
    code: Rc<CodeObject>,
    return_reg: u16,
    handler: TaggedValue,
    val: TaggedValue,
    cont: TaggedValue,
) {
    let base = registers.len();
    registers.resize(base + abort_step::NUM_REGS as usize, TaggedValue::NULL);
    registers[base + abort_step::HANDLER as usize] = handler;
    registers[base + abort_step::VAL as usize] = val;
    registers[base + abort_step::CONT as usize] = cont;
    frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: abort_step::NUM_REGS,
        closure: None,
        return_reg,
        code,
    });
}

/// The registers of the stub frame a raise's handler is called in.
/// See [`Instruction::ResumeRaise`] and [`raise_step_stub`].
pub(super) mod raise_step {
    /// The handler being called, held so that a continuable raise can put it
    /// back once it returns.
    pub(in crate::runtime) const HANDLER: u16 = 0;
    /// The raised object: the handler's argument, and the irritant of the
    /// secondary exception if the handler returns from a non-continuable
    /// raise.
    pub(in crate::runtime) const EXCEPTION: u16 = 1;
    /// How far *below this frame* the handler's own `stack_depth` sat when
    /// the raise popped it. A distance, not an index: the frame can be
    /// captured and replayed at another depth, and only a distance survives
    /// that (see the `ResumeRaise` arm).
    pub(in crate::runtime) const DEPTH_BELOW: u16 = 2;
    /// Whether this was `raise-continuable`.
    pub(in crate::runtime) const CONTINUABLE: u16 = 3;
    /// The handler's value. Returned to the raise's own destination for a
    /// continuable raise; for a non-continuable one the `Return` is never
    /// reached with it.
    pub(in crate::runtime) const RESULT: u16 = 4;
    /// Window size of the stub frame.
    pub(in crate::runtime) const NUM_REGS: u16 = 5;
}

/// `(handler exception)`, then whatever the raise still owes, then return —
/// the three instructions a raise's handler runs under.
///
/// A frame, for the same reason the value form of `dynamic-wind` became one
/// in #158 and an abort's handler call in #165: what the raise still owes is
/// then a **pc**, which a continuation captured inside the handler restores
/// along with the frames. See [`Instruction::ResumeRaise`] for the three
/// divergences that cost while it was Rust (issue #178).
///
/// # State contract
///
/// Returns loaded/cached stub code via runtime_stub. Changes only the code
/// store/cache; no frame is pushed and no Scheme code runs.
fn raise_step_stub(state: &mut VmState) -> Result<Rc<CodeObject>, VmError> {
    let instructions = vec![
        Instruction::Call {
            func: raise_step::HANDLER,
            args: vec![raise_step::EXCEPTION],
            dst: raise_step::RESULT,
        },
        Instruction::ResumeRaise,
        Instruction::Return {
            val: raise_step::RESULT,
        },
    ];
    runtime_stub(
        state,
        |s| s.raise_step_code,
        |s, id| s.raise_step_code = Some(id),
        "raise-step",
        instructions,
        raise_step::NUM_REGS,
    )
}

/// The registers of the stub frame each `before` thunk of a composable
/// invoke's extent re-entry runs under. See [`Instruction::ResumeComposableInvoke`].
pub(super) mod invoke_step {
    /// The delimited continuation being invoked. Held here, not only in a Rust
    /// local, because its payload lives in a **weak** store (`gc_roots.rs`):
    /// an unrooted ref at a collecting safe point would have its payload
    /// pruned while the thunks run.
    pub(in crate::runtime) const CONT: u16 = 0;
    /// The value the invoke delivers into the captured hole.
    pub(in crate::runtime) const VALUE: u16 = 1;
    /// Where the resumed computation returns, as a fixnum.
    pub(in crate::runtime) const DST: u16 = 2;
    /// Which of the continuation's extents this step is entering, as a fixnum.
    pub(in crate::runtime) const INDEX: u16 = 3;
    /// Where the thunk returns. Its value is discarded, but `Return` needs a
    /// slot to write into.
    pub(in crate::runtime) const THUNK_RESULT: u16 = 4;
    /// Window size of a stub frame.
    pub(in crate::runtime) const NUM_REGS: u16 = 5;
}

/// The one-instruction code object every step of a composable invoke runs.
///
/// # State contract
///
/// Returns loaded/cached stub code via runtime_stub. Changes only the code
/// store/cache; no frame is pushed and no Scheme code runs.
fn invoke_step_stub(state: &mut VmState) -> Result<Rc<CodeObject>, VmError> {
    runtime_stub(
        state,
        |s| s.invoke_step_code,
        |s, id| s.invoke_step_code = Some(id),
        "invoke-step",
        vec![Instruction::ResumeComposableInvoke],
        invoke_step::NUM_REGS,
    )
}

/// Push the stub carrying the rest of a composable invoke, and call the
/// `before` thunk of the extent at `index`.
///
/// The thunk runs under the **invoke site's** handler stack — no
/// `install_thunk_handlers` here, unlike `push_wind_step`. That difference is
/// the whole reason this exists rather than reusing the jump: a jump's target
/// replaces the machine, so the record's own stack is the only candidate; a
/// composable invoke's extends it, so the invoke site's stack is a prefix of
/// the right answer. Installing the record's raw list here loses the invoke
/// site's handlers and resurrects capture-site ones whose extent is over,
/// which is pinned in `tests/scheme/control/prompts.scm` ("a re-entry thunk runs under the invoke site's handler").
///
/// # State contract
///
/// Requires the delimited handle, a valid captured-wind index and its
/// before thunk. Appends a rooted stub and calls the thunk under live handlers;
/// the call may change all dynamic state. Push the wind record only on resume.
pub(super) fn push_invoke_step(
    state: &mut VmState,
    cont: TaggedValue,
    value: TaggedValue,
    dst: u16,
    index: usize,
    thunk: TaggedValue,
) -> Result<(), VmError> {
    let code = invoke_step_stub(state)?;
    let base = push_stub_frame(state, code, invoke_step::NUM_REGS);
    state.set_reg_at(base, invoke_step::CONT, cont);
    state.set_reg_at(base, invoke_step::VALUE, value);
    state.set_reg_at(base, invoke_step::DST, TaggedValue::fixnum(dst as i64));
    state.set_reg_at(base, invoke_step::INDEX, TaggedValue::fixnum(index as i64));

    // A primitive thunk returns here and now; its value is discarded either
    // way, and the stub frame is left on top for the instruction to run.
    call_any(state, thunk, &[], invoke_step::THUNK_RESULT)?;
    Ok(())
}

/// The registers of the stub frame an abort's prompt handler is called in.
/// See [`abort_handler_stub`].
mod abort_step {
    /// The prompt's handler procedure.
    pub(super) const HANDLER: u16 = 0;
    /// The value the abort carries.
    pub(super) const VAL: u16 = 1;
    /// The delimited continuation the handler is given.
    pub(super) const CONT: u16 = 2;
    /// The handler's result, which the stub returns to `prompt.dst`.
    pub(super) const RESULT: u16 = 3;
    /// Window size of the stub frame.
    pub(super) const NUM_REGS: u16 = 4;
}

/// `(handler val k)`, then return its value — the two instructions an abort
/// lands on.
///
/// A frame rather than a `call_any`, for the reason the value form of
/// `dynamic-wind` became a frame in #158: what the abort still owes once its
/// thunks have run is then a **pc**, which a continuation captured in the
/// handler restores along with it.
///
/// # State contract
///
/// Returns loaded/cached stub code via runtime_stub. Changes only the code
/// store/cache; no frame is pushed and no Scheme code runs.
fn abort_handler_stub(state: &mut VmState) -> Result<Rc<CodeObject>, VmError> {
    let instructions = vec![
        Instruction::Call {
            func: abort_step::HANDLER,
            args: vec![abort_step::VAL, abort_step::CONT],
            dst: abort_step::RESULT,
        },
        Instruction::Return {
            val: abort_step::RESULT,
        },
    ];
    runtime_stub(
        state,
        |s| s.abort_handler_code,
        |s, id| s.abort_handler_code = Some(id),
        "abort-handler",
        instructions,
        abort_step::NUM_REGS,
    )
}

/// Snapshot the frames between `prompt` and the live top as a delimited
/// continuation.
///
/// `hole` is where the capturing call would have written its own result —
/// what this continuation resumes into, in the innermost frame it captures.
/// Recorded only when there *is* an innermost frame: an
/// `abort-current-continuation` in tail position of the prompt body has
/// already popped that frame, and `hole` then names a register in a frame
/// below the prompt, which this continuation does not own.
///
/// # State contract
///
/// Requires a current prompt index and a valid hole in the innermost
/// captured frame, if any. Reads all five dynamic components and returns a
/// detached snapshot without changing them. Root its handle before any safe point.
pub(super) fn capture_delimited(
    state: &VmState,
    prompt_idx: usize,
    hole: u16,
) -> VmDelimitedContinuation {
    let prompt = &state.prompt_stack[prompt_idx];
    // A prompt's recorded depths can outrun the stacks they index: a `raise`
    // pops the handler entry it is running *before* calling it, so a handler
    // that aborts to a prompt established under itself arrives with
    // `exception_handlers` shorter than the prompt recorded. Clamped, not
    // sliced — nothing above the boundary is left to capture — and clamped for
    // all three, since the same "recorded against a stack that has since
    // shrunk" applies to a jump popping winds mid-travel. Slicing panicked.
    let depth_at_capture = prompt.stack_depth.min(state.frames.len());
    let wind_depth_at_capture = prompt.dynamic_wind_depth.min(state.dynamic_winds.len());
    let handler_depth_at_capture = prompt
        .exception_handler_depth
        .min(state.exception_handlers.len());
    let frames = state.frames[depth_at_capture..].to_vec();
    // An empty capture is the identity continuation, and it carries no dynamic
    // environment at all — no frames to run under means nothing for prompts,
    // handlers or extents to belong to, and an identity invoke appends no
    // frame whose return could sweep them back off. Established here rather
    // than assumed at the invoke: an extent open inside the prompt body should
    // always have a frame of its own (`PushWind` is followed by `PopWind` in
    // the same code object, so the frame running them cannot tail-call away),
    // but a release build that met a counter-example would silently re-enter
    // nothing and push records nothing pops.
    let dynamic_winds = if frames.is_empty() {
        Vec::new()
    } else {
        state.dynamic_winds[wind_depth_at_capture..].to_vec()
    };
    // Everything above the delimiting prompt is *inside* the captured region
    // and belongs to the continuation: for prompts the slice above
    // `prompt_idx`, for handlers the slice above the length the prompt
    // recorded.
    //
    // Except when nothing was captured. An empty capture is the identity
    // continuation — `(k v)` is `v`, no code runs — so there is no region for
    // a dynamic environment to belong to, and taking one would only leak it:
    // an identity invoke appends no frame whose return could sweep it back
    // off. (The set is not always empty: `with-exception-handler` tail-called
    // from a prompt body installs at the prompt's own depth.)
    let (prompt_stack, exception_handlers) = if frames.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (
            state.prompt_stack[prompt_idx + 1..].to_vec(),
            state.exception_handlers[handler_depth_at_capture..].to_vec(),
        )
    };
    let base_at_capture = frames
        .first()
        .map_or(state.registers.len(), |f| f.register_base);
    let mut registers = state.registers[base_at_capture..].to_vec();
    let deliver_reg = frames.last().map(|top| {
        // Cleared, not copied as it stands: the hole is dead by construction
        // — the capturing call never returns a value into it — so whatever it
        // still holds would be retained for the continuation's whole life.
        // `call/cc`'s capture clears its own `dst` for this reason, with 296
        // MB of measured retention behind the comment there.
        registers[top.register_base - base_at_capture + hole as usize] = TaggedValue::NULL;
        hole
    });
    VmDelimitedContinuation {
        frames,
        dynamic_winds,
        registers,
        base_at_capture,
        deliver_reg,
        depth_at_capture,
        wind_depth_at_capture,
        handler_depth_at_capture,
        prompt_stack,
        exception_handlers,
    }
}

/// Move a depth recorded against one stack onto another: `value` sat `from`
/// units up that stack, and the same position is `to` units up this one.
///
/// Saturating rather than wrapping, and not hypothetically:
/// `install_thunk_handlers` clamps a record's handler depths down to the
/// thunk's frame depth, which can be *below* the prompt a continuation was
/// delimited by, so `value < from` happens. Wrapping would send such a depth
/// to about 2^64, where `pop_exception_handlers`' `stack_depth >= frames.len()`
/// test drops it on the next return instead of keeping it for as long as the
/// resumed frames run.
fn relocate_depth(value: usize, from: usize, to: usize) -> usize {
    (to + value).saturating_sub(from)
}

/// How an invoke of a delimited continuation ended.
enum DelimitedInvoke {
    /// Its frames are on the stack and running, the value delivered into the
    /// hole they were waiting on. Whatever they return goes to the `dst` the
    /// invoke named.
    ///
    /// Reported to every caller as a pushed frame, never as a parked escape.
    /// A composable continuation *returns*, so a caller with Rust work left
    /// — a `call-with-values` consumer to apply, a jump to finish, a handler
    /// entry to re-push — is still owed that work; the escape sentinel tells
    /// it the opposite.
    Resumed,
    /// It captured no frames, so it is the identity continuation: the value
    /// it was invoked with *is* its result, and the caller places that value
    /// the way it places any other immediate one.
    Identity,
}

/// Invoke a delimited (composable) continuation: re-enter the extents it
/// captured, append its frames to the live stack, and deliver `value` into
/// the hole the innermost one is waiting on.
///
/// `dst` is where the *resumed computation's* value goes — the register the
/// `(k v)` call would have written. The captured frames still return to the
/// prompt they were captured under, which is not where this invoke happened
/// and, after an abort, is not on the stack at all, so the outermost one is
/// re-pointed here.
///
/// Delivering into the hole and re-pointing that return are the two halves of
/// resuming `(list 'got ␣)` with `10` and getting `(got 10)` back. Appending
/// the frames alone gives neither: the resumed computation reads a register
/// nothing wrote, and its value returns to a register no one reads. That is
/// what this used to do, in two copies (issue #160).
///
/// **The extents are re-entered a step at a time, each under a stub frame**
/// (issue #167). The thunks used to run on a nested Rust loop, so a
/// continuation captured in one and re-entered restarted that thunk from the
/// top and lost the resumed value — #157's signature, on the last of the four
/// places that ran wind thunks. `cont` is the continuation's *handle*, not
/// just its payload, because the stub carries it across those steps.
///
/// # State contract
///
/// Requires a live caller, matching handle/payload and caller-relative dst.
/// Identity leaves state unchanged. Otherwise schedules entry thunks or appends
/// the captured region; a successful resume extends rather than replaces the
/// machine, while a thunk can escape and change every dynamic component.
fn invoke_delimited(
    state: &mut VmState,
    cont: TaggedValue,
    dc: Rc<VmDelimitedContinuation>,
    value: TaggedValue,
    dst: u16,
) -> Result<DelimitedInvoke, VmError> {
    if dc.deliver_reg.is_none() {
        // An empty capture is the identity continuation, and carries no
        // extents either: `capture_delimited` keys `deliver_reg` and
        // `dynamic_winds` on the same `frames.is_empty()` test twelve lines
        // apart, so this is established by construction rather than checked
        // here — an assert restating it could not fail.
        return Ok(DelimitedInvoke::Identity);
    }
    match dc.dynamic_winds.first() {
        // Nothing to re-enter, which is the common case: finish here rather
        // than pay a stub frame to discover there is no thunk to run.
        None => finish_delimited_invoke(state, &dc, value, dst)?,
        Some(record) => push_invoke_step(state, cont, value, dst, 0, record.before)?,
    }
    Ok(DelimitedInvoke::Resumed)
}

/// Append the captured region and deliver — everything a composable invoke
/// owes once its extents have been re-entered.
///
/// Reached directly when there were none, and from
/// [`Instruction::ResumeComposableInvoke`] when the last `before` thunk has
/// returned. `wind_base` is *derived* rather than carried: by the time this
/// runs, every captured record has been pushed, so the base they start at is
/// what is left when their count is taken off the top.
///
/// # State contract
///
/// Requires a nonempty captured region with a deliver_reg, and all of its
/// wind records already appended to the live wind stack. Appends relocated
/// frames/registers/prompts/handlers and delivers value; leaves winds in place.
pub(super) fn finish_delimited_invoke(
    state: &mut VmState,
    dc: &VmDelimitedContinuation,
    value: TaggedValue,
    dst: u16,
) -> Result<(), VmError> {
    // Checked, because arbitrary user code has run between the invoke and
    // here — that is what the stub frames are for — and this assumes every
    // captured record is still on top. Unchecked it would wrap in release and
    // hand `relocate_depth` a base near 2^64, producing carried prompts with
    // garbage wind depths: silent corruption of some later abort's travel
    // rather than a diagnosis here.
    let wind_base = state
        .dynamic_winds
        .len()
        .checked_sub(dc.dynamic_winds.len())
        .ok_or_else(|| VmError::Runtime {
            message: "composable invoke: a re-entered extent left the wind stack".into(),
        })?;
    // Relocate the captured register windows onto the end of the live array.
    let shift = state.registers.len().wrapping_sub(dc.base_at_capture);
    state.registers.extend_from_slice(&dc.registers);
    let outermost = state.frames.len();
    state.frames.extend(dc.frames.iter().cloned());
    for f in &mut state.frames[outermost..] {
        f.register_base = f.register_base.wrapping_add(shift);
    }

    // The dynamic environment the captured frames ran in comes back with them:
    // the prompts they established and the handlers they installed. Every
    // depth in it was recorded against a stack at capture and has to be moved
    // onto the live one — and a `PromptFrame` records **three**, one per stack
    // it delimits. Relocating only `stack_depth` left the other two indexing
    // the invoke site's stacks, so an abort to a carried prompt ran an
    // enclosing `after` thunk early and uninstalled handlers enclosing the
    // invoke.
    let handler_base = state.exception_handlers.len();
    for p in dc.prompt_stack.iter() {
        let stack_depth = relocate_depth(p.stack_depth, dc.depth_at_capture, outermost);
        debug_assert!(
            stack_depth >= outermost,
            "a carried prompt is inside the capture"
        );
        state.prompt_stack.push(PromptFrame {
            stack_depth,
            dynamic_wind_depth: relocate_depth(
                p.dynamic_wind_depth,
                dc.wind_depth_at_capture,
                wind_base,
            ),
            exception_handler_depth: relocate_depth(
                p.exception_handler_depth,
                dc.handler_depth_at_capture,
                handler_base,
            ),
            // A carried prompt sitting at the outermost appended frame — its
            // `call-with-continuation-prompt` was tail-called, so it shares
            // that frame's depth — has no captured frame below it to deliver
            // into any more. Its result is this invoke's result, by the same
            // reasoning that re-points `return_reg` below.
            dst: if stack_depth == outermost { dst } else { p.dst },
            ..p.clone()
        });
    }
    state
        .exception_handlers
        .extend(dc.exception_handlers.iter().map(|h| ExceptionHandler {
            stack_depth: relocate_depth(h.stack_depth, dc.depth_at_capture, outermost),
            ..h.clone()
        }));
    // Both stacks are swept by frame depth as the resumed frames return
    // (`pop_resolved_extents`), which is when they stop applying — except at a
    // dispatch loop's own exit depth, where that sweep does nothing by design
    // and `run_loop_until_outcome`'s `handlers_at_entry` truncation is the
    // only backstop, which covers handlers and not prompts. A prompt a *full*
    // continuation's snapshot carries past its own body is swept on arrival
    // instead (issue #176, `restore_continuation`); a composable invoke has no
    // equivalent, because it appends to the live stacks rather than replacing
    // them, and the frames it appends are the ones the depth sweep follows.

    state.frames[outermost].return_reg = dst;
    let top_base = state
        .frames
        .last()
        .expect("deliver_reg is Some only for a non-empty capture")
        .register_base;
    let deliver_reg = dc
        .deliver_reg
        .expect("the caller returns Identity when there is no hole");
    state.registers[top_base + deliver_reg as usize] = value;
    Ok(())
}

/// [`invoke_delimited`] for an invoke in tail position: the invoking frame is
/// done, so it is popped first and the resumed computation returns to *its*
/// caller — otherwise the appended frames would return into a frame whose
/// only remaining act is to return again.
///
/// Returns what a tail dispatch returns: `Ok(Some(v))` when the popped frame
/// was the loop's own, and `Ok(None)` when the value went to a live caller or
/// the continuation's frames are now running.
///
/// # State contract
///
/// Requires a live tail-call frame and matching handle/payload. Pops its
/// frame/window before invoking; may change all five dynamic components.
/// Some is normal driver completion, None schedules/returns into a caller;
/// propagate any escape before further cleanup.
pub(super) fn tail_invoke_delimited(
    state: &mut VmState,
    cont: TaggedValue,
    dc: Rc<VmDelimitedContinuation>,
    value: TaggedValue,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    let frame = state.frames.pop().expect("tail invoke with empty stack");
    let return_reg = frame.return_reg;
    state.free_top_registers(frame.register_base);
    match invoke_delimited(state, cont, dc, value, return_reg)? {
        DelimitedInvoke::Resumed => Ok(None),
        // The identity continuation passes the value straight through, so
        // this is an ordinary tail return — the same close-out a primitive in
        // tail position takes, `Return`'s empty-stack guard included: an
        // abort truncates to its prompt, which may be below this loop's exit
        // depth, and the frame just popped can be the last one.
        DelimitedInvoke::Identity => {
            if state.frames.len() == exit_depth || state.frames.is_empty() {
                return Ok(Some(value));
            }
            state.set_reg(return_reg, value);
            pop_resolved_extents(state, exit_depth);
            Ok(None)
        }
    }
}

/// What `(k …)` delivers: `(k v)` delivers `v`; `(k)` and `(k v1 v2 …)`
/// deliver a `#<values>` object, exactly as `(values …)` would return one — so
/// a producer that escapes through a continuation still hands
/// `call-with-values` its values.
///
/// Built only once the callee is known to be a continuation. It used to be the
/// first thing the continuation probe did, so every non-continuation callee
/// that reached it paid a `Vec` (and, for other than one argument, a heap
/// allocation) to decline.
///
/// # State contract
///
/// May allocate a multiple-values heap object; leaves all dynamic stacks
/// and registers unchanged. Protect the result until it is stored or parked.
fn deliver_value(state: &mut VmState, args: &[TaggedValue]) -> TaggedValue {
    state.heap.borrow_mut().values_from(args.to_vec())
}

/// Try to call `func_val` as a primitive. Returns `Some(result)` if it was a
/// primitive, `None` if it's a VM closure (caller should push a frame instead).
/// If `func_val` is a primitive procedure, return it. This is only a type
/// check — no argument copying — so call sites can hand their argument
/// vector to `call_primitive_proc` by move exactly when it will be consumed,
/// instead of cloning it defensively before knowing the callee's kind.
///
/// # State contract
///
/// Read-only heap classification; no Scheme call or VM state mutation.
fn primitive_procedure(state: &VmState, func_val: TaggedValue) -> Option<Rc<Procedure>> {
    let proc = state.heap.borrow().get_procedure(func_val)?;
    matches!(proc.as_ref(), Procedure::Primitive { .. }).then_some(proc)
}

/// Park `value` for the dispatch loop that owns the resumed frame and return
/// the sentinel that unwinds the Rust frames in between.
///
/// Always an unwind, never a return. The frames now on `state` may belong to
/// any enclosing dispatch loop — or to none of them, if the escape targets a
/// frame further out — and only `run_loop_until` knows its own `exit_depth`,
/// so it is the one place allowed to decide whether to resume or exit. Every
/// synchronous boundary in between (`run_thunk_outcome`, and through it
/// `call-with-values`) sees the sentinel and learns that the value is the
/// resumed computation's, not its own — which is the whole difference between
/// writing a register in a live frame and writing one in a frame that no
/// longer exists.
///
/// `value` is what the continuation *delivers*
/// ([`deliver_value`]), which for `(k)` and `(k v1 v2 …)` is a `#<values>`
/// object rather than the first argument. The two invoke paths used to park
/// different things — `ResumeWindJump` the delivered value, the direct path
/// `args[0]` — so the same `(k 1 2)` carried different values depending on
/// whether the jump happened to cross a wind.
///
/// # State contract
///
/// Sets only pending_escape and returns the nonlocal-transfer sentinel.
/// The caller must already have scheduled/restored the transfer target;
/// return the sentinel immediately, without additional dynamic-stack cleanup.
pub(super) fn park_escape(state: &mut VmState, value: TaggedValue) -> VmError {
    state.pending_escape = Some(value);
    VmError::ContinuationEscape
}

/// [`park_escape`] for a transfer that installed a landing to run, rather than
/// unwinding to one that was already there. See [`VmState::pending_transfer`].
///
/// # State contract
///
/// Sets pending_transfer as well as pending_escape for an abort landing.
/// Leaves all dynamic stacks as prepared by the caller and returns the sentinel.
fn park_transfer(state: &mut VmState, value: TaggedValue) -> VmError {
    state.pending_transfer = true;
    park_escape(state, value)
}

/// Dispatch a call to an arbitrary callee value — the body of the `Call`
/// instruction, also used by `CallPrimitive`'s deopt path and, through
/// [`call_any`], by every caller that dispatches a callee without an
/// instruction behind it.
///
/// On return the callee has either delivered its value into `dst` or pushed a
/// frame for the dispatch loop to run; a control transfer out of it is an
/// `Err` ([`park_escape`]), never an `Ok`.
///
/// # State contract
///
/// Requires a live caller and valid caller-relative dst. Ok delivers a
/// value there or leaves frames scheduled; a callee may change all five dynamic
/// components. On Err, propagate before storing a result or sweeping extents.
pub(super) fn call_value(
    state: &mut VmState,
    func_val: TaggedValue,
    arg_vals: &[TaggedValue],
    dst: u16,
) -> Result<(), VmError> {
    // The callee is almost always a plain closure, and the callable heap
    // types are mutually exclusive — so probe the closure case first and
    // let the common path pay one type check instead of failing the four
    // rarer probes below (control primitive, primitive, parameter,
    // continuation) on every call. Keep the probed code id so the closure
    // branch doesn't resolve it a second time.
    let closure_code_id = state.heap.borrow().get_vm_closure_code_id(func_val);
    call_value_with_probe(state, func_val, closure_code_id, arg_vals, dst)
}

/// `call_value` for a callee whose closure probe already ran — the `Call`
/// arm's non-closure branch passes `None` so the probe isn't repeated.
///
/// # State contract
///
/// Same entry/exit contract as call_value. A supplied code id must be the
/// verified closure probe for func_val; None runs the non-closure probes.
pub(super) fn call_value_with_probe(
    state: &mut VmState,
    func_val: TaggedValue,
    closure_code_id: Option<u32>,
    arg_vals: &[TaggedValue],
    dst: u16,
) -> Result<(), VmError> {
    if closure_code_id.is_none() {
        // Intercept higher-order control primitives that need VM cooperation.
        if let Some(ctrl) = vm_control_primitive(state, func_val) {
            return handle_control_primitive(state, ctrl, arg_vals, dst);
        }
        if let Some(prim) = primitive_procedure(state, func_val) {
            let result = call_primitive_proc(state, &prim, arg_vals);
            state.set_reg(dst, result?);
            return Ok(());
        }
        if let Some(result) = try_call_parameter(state, func_val, arg_vals) {
            state.set_reg(dst, result?);
            return Ok(());
        }
        if let Some(delivered) = try_invoke_full_continuation(state, func_val, arg_vals)? {
            return Err(park_escape(state, delivered));
        }
        if let Some(dc) = state.get_vm_delimited_continuation(func_val) {
            let value = deliver_value(state, arg_vals);
            if matches!(
                invoke_delimited(state, func_val, dc, value, dst)?,
                DelimitedInvoke::Identity
            ) {
                // The identity continuation resumes nothing, so nothing will
                // deliver `value` but this.
                state.set_reg(dst, value);
            }
            return Ok(());
        }
    }
    let code_id = match closure_code_id {
        Some(id) => CodeObjectId(id),
        // Not callable: resolve_closure produces the standard type error.
        None => resolve_closure(state, func_val)?,
    };
    call_closure_resolved(state, func_val, code_id, arg_vals, dst)
}

/// Dispatch a call to an arbitrary callee value in tail position — the body
/// of the `TailCall` instruction, also used by the deopt path of tail-shaped
/// primitive sites (PRD P8.2). Control primitives, primitives, and
/// parameters pop the frame and deliver straight to the caller; closures
/// reuse the current frame's register window. Returns `Some(value)` when the
/// enclosing `run_loop_until` must exit with `value`.
///
/// # State contract
///
/// Requires a live tail-call frame above its caller and the driver exit
/// depth. May reuse/pop the frame and change all dynamic components. Some
/// means normal driver completion; None means continue with current state.
/// Transfers propagate as Err, never as a normal return value.
pub(super) fn tail_call_value(
    state: &mut VmState,
    func_val: TaggedValue,
    arg_vals: &[TaggedValue],
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    // Same closure-first probe as `call_value` — see there for why probe
    // order is safe.
    let closure_code_id = state.heap.borrow().get_vm_closure_code_id(func_val);
    tail_call_value_with_probe(state, func_val, closure_code_id, arg_vals, exit_depth)
}

/// `tail_call_value` for a callee whose closure probe already ran — the
/// `TailCall` arm's non-closure branch passes `None`.
///
/// # State contract
///
/// Same entry/exit contract as tail_call_value. The optional code id must
/// come from a verified closure probe for func_val.
pub(super) fn tail_call_value_with_probe(
    state: &mut VmState,
    func_val: TaggedValue,
    closure_code_id: Option<u32>,
    arg_vals: &[TaggedValue],
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    if closure_code_id.is_none() {
        // Intercept higher-order control primitives in tail position.
        // Strategy: pop the current frame first (simulating "already
        // returned"), then dispatch the primitive as if called from the
        // parent frame.
        if let Some(ctrl) = vm_control_primitive(state, func_val) {
            let frame = state.frames.pop().expect("tail call ctrl with empty stack");
            let return_reg = frame.return_reg;
            state.free_top_registers(frame.register_base);
            let depth = state.frames.len();
            // Now at depth N-1. Handle with dst = return_reg (slot in frame
            // N-2). (At exit depth, return_reg is still the right dst — not
            // 0, which could clobber live registers like MutableCell
            // pointers.)
            //
            // The extents keyed on the popped frame stay open across the
            // dispatch: a `raise` in tail position must still find the
            // handler its thunk was called under.
            handle_control_primitive(state, ctrl, arg_vals, return_reg)?;
            // The control primitive has completed. If it delivered its
            // result without pushing a frame, the tail call has returned and
            // the popped frame's extents close now, as after `Return`; a
            // frame it did push closes them when that frame returns. Then,
            // if this is the exit depth, the enclosing loop is done.
            if state.frames.len() == depth {
                pop_resolved_extents(state, exit_depth);
            }
            if state.frames.len() == exit_depth {
                let result = state.reg(return_reg);
                return Ok(Some(result));
            }
            return Ok(None);
        }

        // Primitives in tail position: call them, write result to current
        // frame's return_reg, then simulate a Return.
        //
        // Probed before continuations, matching `call_value_with_probe`'s
        // order. The two are interchangeable — the callable heap variants are
        // mutually exclusive, the same argument the closure-first probe above
        // rests on — and a primitive callee is far commoner than a
        // continuation, so this saves the two heap borrows
        // `try_invoke_continuation` spends before it can decline.
        if let Some(prim) = primitive_procedure(state, func_val) {
            let result = call_primitive_proc(state, &prim, arg_vals)?;
            let frame = state.frames.pop().expect("tail call with empty stack");
            if state.frames.len() == exit_depth {
                state.free_top_registers(frame.register_base);
                return Ok(Some(result));
            }
            let return_reg = frame.return_reg;
            state.set_reg(return_reg, result);
            state.free_top_registers(frame.register_base);
            // The popped frame's extents close now, as after `Return`.
            pop_resolved_extents(state, exit_depth);
            return Ok(None);
        }

        // Continuation invocation in tail position.
        if let Some(delivered) = try_invoke_full_continuation(state, func_val, arg_vals)? {
            return Err(park_escape(state, delivered));
        }
        if let Some(dc) = state.get_vm_delimited_continuation(func_val) {
            let value = deliver_value(state, arg_vals);
            return tail_invoke_delimited(state, func_val, dc, value, exit_depth);
        }

        // Parameters in tail position: same as primitives.
        if let Some(result) = try_call_parameter(state, func_val, arg_vals) {
            let result = result?;
            let frame = state
                .frames
                .pop()
                .expect("tail call param with empty stack");
            if state.frames.len() == exit_depth {
                state.free_top_registers(frame.register_base);
                return Ok(Some(result));
            }
            let return_reg = frame.return_reg;
            state.set_reg(return_reg, result);
            state.free_top_registers(frame.register_base);
            pop_resolved_extents(state, exit_depth);
            return Ok(None);
        }
    }

    let new_code_id = match closure_code_id {
        Some(id) => CodeObjectId(id),
        // Not callable: resolve_closure produces the standard type error.
        None => resolve_closure(state, func_val)?,
    };
    tail_call_closure_resolved(state, func_val, new_code_id, arg_vals)?;
    Ok(None)
}

/// The tail-position closure branch of `tail_call_value`, shared with the
/// `TailCall` instruction's closure fast path: reuse the current frame's
/// register window for the resolved callee.
///
/// # State contract
///
/// Requires a live tail-call frame and a verified closure/code-id pair.
/// Checks arity, reuses its window and resets its pc/closure/code; may grow
/// registers. Preserves frame count and winds/prompts/handlers.
pub(super) fn tail_call_closure_resolved(
    state: &mut VmState,
    func_val: TaggedValue,
    new_code_id: CodeObjectId,
    arg_vals: &[TaggedValue],
) -> Result<(), VmError> {
    let top = state
        .frames
        .last()
        .expect("tail call with empty frame stack");
    if top.code.id == new_code_id {
        let (arity, base) = (top.code.arity, top.register_base);
        return self_tail_call(state, base, arity, func_val, arg_vals);
    }

    let new_code = state.code_object(new_code_id)?;

    check_arity(new_code.arity, arg_vals.len())?;

    // Reuse the current frame's register window.
    // If the new code needs more registers, grow the window.
    let frame = state.frames.last_mut().unwrap();
    let old_base = frame.register_base;
    let old_num = frame.num_regs;
    let new_num = new_code.num_regs;

    if new_num > old_num {
        let extra = new_num - old_num;
        state
            .registers
            .resize(state.registers.len() + extra as usize, TaggedValue::NULL);
        state.frames.last_mut().unwrap().num_regs = new_num;
    }

    store_args_in_window(state, old_base, new_code.arity, arg_vals);

    // Update frame in-place — dispatch fetches instructions from `code`.
    let frame = state.frames.last_mut().unwrap();
    frame.pc = 0;
    frame.closure = closure_heap_index(func_val);
    frame.code = new_code;
    Ok(())
}

/// Tail call whose callee is the current frame's own code object — the
/// `(define (loop …) … (loop …))` shape. No store lookup, no `Rc` churn,
/// no window grow: the window is already sized for this code. The closure
/// field still updates — a different instance of the same code (different
/// captures) tail-calls itself under the same code id.
///
/// # State contract
///
/// Requires base and arity from the current frame and a callee with that
/// same code id. Updates its argument window, pc and closure; preserves
/// frame count, code and all three dynamic-context stacks.
pub(super) fn self_tail_call(
    state: &mut VmState,
    base: usize,
    arity: Arity,
    func_val: TaggedValue,
    arg_vals: &[TaggedValue],
) -> Result<(), VmError> {
    check_arity(arity, arg_vals.len())?;
    store_args_in_window(state, base, arity, arg_vals);
    let frame = state
        .frames
        .last_mut()
        .expect("tail call with empty frame stack");
    frame.pc = 0;
    frame.closure = closure_heap_index(func_val);
    Ok(())
}

/// Execute a compile-time-resolved primitive call: the shared back end of
/// `CallPrimitive` and every inline opcode's slow path. Checks the shadow
/// bit — a rebound name deoptimizes to name-lookup dispatch, preserving
/// redefinition semantics — otherwise dispatches through the registry by
/// index. Returns `Some(value)` when the tail deopt path completes this
/// driver's frame, as [`tail_call_value`] specifies. Transfers propagate as
/// errors with a parked value on both paths.
///
/// # State contract
///
/// Requires the current base, registry id, valid argument values and dst;
/// pc is already past the instruction. A rebound callee can change all five
/// components. Some finishes the driver normally, None continues; Err may
/// carry a parked escape and must be propagated before any result write.
#[allow(clippy::too_many_arguments)]
pub(super) fn exec_call_primitive(
    state: &mut VmState,
    base: usize,
    func_id: PrimitiveFnId,
    name: &Symbol,
    arg_vals: &[TaggedValue],
    dst: u16,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    if state.is_primitive_shadowed(func_id.0 as usize) {
        let globals = frame_globals(state);
        let func_val = globals
            .get(name)
            .ok_or_else(|| VmError::UnboundVariable { name: name.clone() })?;
        // Tail-shape detection (PRD P8.2): pass 5 lowers a tail-position
        // primitive site to `<prim-op> dst; Return dst`. When the deopt
        // replaces the primitive with a closure call, that shape must stay a
        // proper tail call (R7RS §3.5), or mutual tail recursion through the
        // rebound site grows the frame stack. The check reads the actual
        // next instruction — it cannot go stale, and a coincidental match is
        // semantically a tail call anyway. On the tail path the callee
        // returns directly to this frame's caller; the `Return` is never
        // executed.
        let frame = state.frames.last().expect("no active frame");
        let is_tail_site = matches!(
            frame.code.instructions.get(frame.pc),
            Some(Instruction::Return { val }) if *val == dst
        );
        if is_tail_site {
            return tail_call_value(state, func_val, arg_vals, exit_depth);
        }
        call_value(state, func_val, arg_vals, dst)?;
        return Ok(None);
    }
    exec_call_primitive_direct(state, base, func_id, arg_vals, dst)
}

/// Dispatch to the registry primitive at `func_id` by index, with no
/// shadow check: the `CallPrimitiveDirect` site names the procedure itself,
/// not a global that could since have been rebound, so there is nothing to
/// deoptimize to. Also the tail of [`exec_call_primitive`] once it has
/// established the site is not shadowed.
///
/// # State contract
///
/// Requires a valid immutable registry id and current base/dst. A callback
/// can change every dynamic component. Normal completion writes dst and
/// returns None; an escape propagates before that write, with landing intact.
pub(super) fn exec_call_primitive_direct(
    state: &mut VmState,
    base: usize,
    func_id: PrimitiveFnId,
    arg_vals: &[TaggedValue],
    dst: u16,
) -> Result<Option<TaggedValue>, VmError> {
    let registry = Rc::clone(&state.primitive_registry);
    let ctx = VmApplyContext {
        state: state as *mut VmState,
    };
    let result = registry
        .apply_by_index(func_id.0 as usize, arg_vals, &ctx)
        .map_err(|e| VmError::Runtime {
            message: e.to_string(),
        })?;
    // No frame check here: a continuation escaping out of a re-entrant
    // primitive is signalled at the boundary (`across_reentry`) and unwinds
    // through the `?` above, so by this point the frames are the ones this
    // call started with. An earlier fix guarded it here instead, which caught
    // only primitives in call position.
    state.set_reg_at(base, dst, result);
    Ok(None)
}

/// Call a primitive procedure (as returned by `primitive_procedure`).
/// Arguments are borrowed: heap-tier handlers read them in place; only the
/// higher-order tier copies them into an owned Vec at the registry boundary.
///
/// # State contract
///
/// Requires a registry primitive and a live calling frame. Invokes the
/// callback bridge, which may change every dynamic component. Normal return
/// is a result; on error, inspect parked transfer state before error routing.
fn call_primitive_proc(
    state: &mut VmState,
    proc: &Procedure,
    args: &[TaggedValue],
) -> Result<TaggedValue, VmError> {
    let Procedure::Primitive {
        qualified_name,
        registry_index,
        ..
    } = proc
    else {
        return Err(VmError::Runtime {
            message: "call_primitive_proc: not a primitive".into(),
        });
    };
    let ctx = VmApplyContext {
        state: state as *mut VmState,
    };
    state
        .primitive_registry
        .apply_cached(qualified_name, registry_index, args, &ctx)
        .map_err(|e| VmError::Runtime {
            message: e.to_string(),
        })
}
