# VM Bug: `run_thunk` Return-Reg Clobbering

**Status:** Open — root cause identified, fix not yet found
**Affects:** 5+ tests involving nested `dynamic-wind` with `set!`
**Discovered:** 2026-03-11 during A7 acceptance testing

---

## 1. Symptom

```
Error: ReadCell: not a MutableCell
```

Any program that uses `set!` on a captured variable inside a **nested**
`dynamic-wind` fails. Single-level `dynamic-wind` works fine. Using a regular
Scheme function (not a control primitive) in place of the inner `dynamic-wind`
also works fine.

### Minimal reproduction

```scheme
((lambda (x)
   (dynamic-wind
     (lambda () 'ok)
     (lambda ()
       (dynamic-wind
         (lambda () (set! x 99))   ;; inner-before: writes through MutableCell
         (lambda () x)             ;; inner-body: reads through MutableCell — FAILS
         (lambda () 'ok)))
     (lambda () 'ok))
   x)                              ;; also fails: outer lambda reads cell
 0)
```

Run with: `patina --vm /tmp/repro.scm`
Dump bytecode: `patina --vm-dump /tmp/repro.scm`

### What works

| Pattern | Result |
|---------|--------|
| Single `dynamic-wind` with `set!` | OK |
| Nested lambdas with `set!` (no control primitives) | OK |
| Nested thunk calls via regular Scheme function | OK |
| Nested `dynamic-wind` without `set!` | OK |
| Nested `dynamic-wind` where inner-before does `set!` | **FAILS** |

---

## 2. Bytecode Analysis

`patina --vm-dump` confirms the compiler output is **correct**:

```
┌─ CodeObject #103 (outer lambda, param x)
│   0  AllocCell    r0 ← box(r0)          ;; x becomes MutableCell in r0
│   1  LoadGlobal   r2 ← dynamic-wind
│   2  MakeClosure  r3 ← #104 []          ;; before thunk (no captures)
│   3  Move         r5 ← r0               ;; copy cell to scratch
│   4  MakeClosure  r4 ← #105 [r5]        ;; body thunk captures cell
│   5  MakeClosure  r5 ← #109 []          ;; after thunk (no captures)
│   6  Call         r1 ← r2(r3, r4, r5)   ;; call dynamic-wind
│   7  ReadCell     r3 ← *r0              ;; ← FAILS: r0 no longer MutableCell
│   8  Return       r3

┌─ CodeObject #105 (body thunk, captures cell via closure[0])
│   0-5  Creates 3 inner closures, each capturing closure[0]
│   6    TailCall dynamic-wind(inner-before, inner-body, inner-after)

┌─ CodeObject #106 (inner-before)
│   0  LoadImm      r1 ← 99
│   1  LoadClosure  r0 ← closure[0]       ;; cell pointer
│   2  WriteCell    *r0 ← r1              ;; write 99 into cell
│   3  LoadImm      r0 ← UNSPECIFIED
│   4  Return       r0

┌─ CodeObject #107 (inner-body)
│   0  LoadClosure  r0 ← closure[0]       ;; cell pointer
│   1  ReadCell     r0 ← *r0              ;; read through cell
│   2  Return       r0
```

Key observations:
- Pass 1 correctly identifies `x` as mutated and captured
- Pass 2 correctly boxes `x` (AllocCell) and uses ClosureBoxed for captures
- All `MakeClosure` instructions capture the cell pointer (verified at runtime)
- `WriteClosureCell` correctly emits `LoadClosure` + `WriteCell` (writes *through* cell)
- `ReadClosureCell` correctly emits `LoadClosure` + `ReadCell`

---

## 3. Root Cause: `run_thunk` Return-Reg Writes to Caller Frame

### The mechanism

`dynamic-wind` is a VM control primitive. Its handler in `handle_control_primitive`
calls `run_thunk(state, thunk)` for each of the three thunks (before, body, after).

`run_thunk` calls `call_closure(state, thunk, &[], return_reg)` to push a frame,
then `run_loop_until` to execute it. When the thunk's `Return` instruction fires,
it writes the result to `caller.registers[return_reg]`.

The **caller** is whichever frame is below the thunk's frame on the stack. When
control flow is straightforward, this is the frame that called `dynamic-wind`.

### The problem with TailCall

The body thunk (#105) ends with `TailCall dynamic-wind(...)` for the inner
`dynamic-wind`. The TailCall handler:

1. **Pops the body thunk's frame** (it's a tail call — frame is reused)
2. Reads `return_reg` from the popped frame
3. Calls `handle_control_primitive(DynamicWind, ...)` for the inner dynamic-wind

After step 1, the **outer lambda** (#103) becomes the top frame. The inner
dynamic-wind handler then calls `run_thunk(state, inner_before)`. This pushes
inner-before's frame. When inner-before Returns, it writes to
`caller.registers[return_reg]` — where "caller" is now the **outer lambda**.

If `return_reg = 0`, this clobbers `r0` of #103, which holds the MutableCell.
The cell pointer is replaced with the thunk's return value (e.g. `'ok` or
`#<unspecified>`).

After all inner thunks complete, the outer lambda resumes at instruction 7:
`ReadCell r3 ← *r0`. But `r0` no longer contains a MutableCell — it contains
whatever the last `run_thunk` Return wrote. Since inner-before wrote 99 into
the cell before returning, and the cell was then read successfully by
inner-body, the cell itself is fine — but the *pointer to it* in r0 was
destroyed.

### Visual timeline

```
Frame stack         r0 of #103        Action
─────────────────   ─────────────     ─────────────────────────────
[top, #103]         MutableCell(x)    AllocCell boxes x
[top, #103]         MutableCell(x)    Call dynamic-wind → intercepted
[top, #103, #105]   MutableCell(x)    body thunk pushed by run_thunk
[top, #103]         MutableCell(x)    body TailCall pops #105 ← frame gone
[top, #103, #106]   MutableCell(x)    inner-before pushed by run_thunk
[top, #103]         ??? clobbered     inner-before Returns, writes to r0!
[top, #103, #107]   ??? clobbered     inner-body pushed, reads closure (OK)
[top, #103]         ??? clobbered     inner-body Returns
[top, #103]         ??? clobbered     #103 resumes, ReadCell *r0 → ERROR
```

---

## 4. What Was Tried

### Attempt 1: `return_reg = caller.num_regs` (scratch beyond live window)

Changed `run_thunk` to use `return_reg = state.frames.last().num_regs` (one past
the caller's last live register). This should make Returns write to a scratch
slot beyond the caller's register window.

**Result:** Still fails. Investigation showed that `set_reg` and
`set_reg_in_frame` are NOT writing to r0 of #103. The clobbering source is
*not* the Return instruction's `set_reg_in_frame` call.

### Attempt 2: Extend caller's `num_regs` temporarily

Changed `run_thunk` to bump `caller.num_regs += 1` before pushing the thunk
frame, then restore after.

**Result:** Caused regressions in other tests (11 compliance, 9 define-values
failures). The `free_top_registers` assertion and register layout assumptions
broke.

### Relaxing `free_top_registers`

Changed `debug_assert_eq!` to allow `registers.len() >= base + num_regs`.
This is a correct relaxation regardless of the bug fix.

---

## 5. Mystery: Where Does the Clobber Happen?

Debug instrumentation showed:
- `set_reg(r0)` on code 103 is NEVER called during the dynamic-wind execution
- `set_reg_in_frame(103_frame, 0, ...)` is NEVER called
- All `Return` instructions use `return_reg = 6` (the scratch slot)
- The register at `registers[outer_base + 0]` IS the MutableCell after AllocCell

Yet when #103 resumes at pc=7, `registers[outer_base + 0]` contains `fixnum(99)`.

**Open question:** Something writes directly to `registers[outer_base + 0]`
without going through `set_reg` or `set_reg_in_frame`. Possible sources:

1. `call_closure` — copies args into new frame's registers:
   `state.registers[base + i] = arg`. If `base` happens to equal `outer_base`,
   this would overwrite r0. But `base` is allocated by `alloc_registers` at the
   end of the register array, not at the caller's base.

2. `free_top_registers` — truncates the array. If a subsequent `alloc_registers`
   reuses the same positions, old values are overwritten. But `alloc_registers`
   uses `resize(..., NULL)` which writes NULL, not fixnum(99).

3. `registers[base + fixed] = rest` in variadic prologue — unlikely for 0-arg
   thunks.

4. Some interaction between `alloc_registers` and `free_top_registers` where
   the base calculation goes wrong, causing two frames to overlap in the
   register array.

**Most likely:** Frame overlap due to `free_top_registers` truncating too far,
then `alloc_registers` placing the next frame at an offset that overlaps with
the outer lambda's window. This would explain why `set_reg` on a *different*
frame (e.g. inner-before writing to its own r0) actually writes to the same
physical register position as #103's r0.

---

## 6. Proposed Fix: Sentinel Frame

Instead of writing the thunk's return value into the caller's register, push
a tiny **sentinel frame** that absorbs the Return write:

```rust
fn run_thunk(state: &mut VmState, thunk: TaggedValue) -> Result<TaggedValue, VmError> {
    // Push sentinel frame: 1 register, no code, just absorbs Return.
    let sentinel_base = state.alloc_registers(1);
    state.frames.push(CallFrame {
        code_id: SENTINEL_CODE_ID,  // special no-op CodeObject
        pc: 0,
        register_base: sentinel_base,
        num_regs: 1,
        closure: None,
        return_reg: 0,  // writes into sentinel's own r0
    });

    let depth_before = state.frames.len();
    call_closure(state, thunk, &[], 0)?;
    let result = run_loop_until(state, depth_before)?;

    // Pop sentinel frame (thunk already popped by Return)
    // Result was already captured by run_loop_until
    if let Some(f) = state.frames.last() {
        if f.code_id == SENTINEL_CODE_ID {
            let frame = state.frames.pop().unwrap();
            state.free_top_registers(frame.register_base, frame.num_regs);
        }
    }
    Ok(result)
}
```

This guarantees:
- Thunk's `Return` writes to sentinel's r0 (not the real caller's registers)
- The sentinel frame insulates the caller from any `return_reg` writes
- TailCall from inside the thunk pops the thunk but leaves the sentinel

**Challenge:** Need a `SENTINEL_CODE_ID` that maps to a valid CodeObject
(even if it's just a single `Return` instruction). Or modify `run_loop_until`
to handle sentinel frames specially.

### Alternative: Bypass Return write

Modify `run_loop_until` to accept a flag that says "don't write the return
value to the caller — just return it via Ok". This is cleaner but requires
threading the flag through the dispatch path, which is more invasive.

---

## 7. Workaround

Programs can avoid the bug by not using `set!` on captured variables inside
nested `dynamic-wind`. Use a mutable box (pair or vector) instead:

```scheme
;; Instead of:
(let ((x 0))
  (dynamic-wind ... (lambda () (dynamic-wind ... (lambda () (set! x 1)) ...)) ...)
  x)

;; Use:
(let ((x (vector 0)))
  (dynamic-wind ... (lambda () (dynamic-wind ... (lambda () (vector-set! x 0 1)) ...)) ...)
  (vector-ref x 0))
```

---

## 8. Affected Tests

From `cargo test --package patina-tests --features vm-backend`:

- `test_dynamic_wind_nested_two_levels` — ReadCell failure
- `test_dynamic_wind_nested_three_levels` — ReadCell failure
- `test_dynamic_wind_callcc_exit` — ReadCell failure
- `test_callcc_escape_runs_dynamic_wind_after` — ReadCell failure
- `test_callcc_with_values` — ReadCell failure

Plus related tests that use `set!` with nested control primitives.

---

## 9. Diagnostic Tools

- `patina --vm-dump file.scm` — compile and disassemble without executing
- `echo '(expr)' | patina --vm-dump` — stdin support
- Unit tests in `pass1_analysis::tests` and `pass2_closure::tests` for
  verifying capture/boxing decisions
