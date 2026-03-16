# VM Library Loading Redesign

**Status:** Proposed
**Created:** 2026-03-15
**Motivation:** `call/cc` inside library body code doesn't work correctly because library bodies are evaluated in a temporary `VmState` that is discarded after loading.

## Problem Statement

The VM evaluates library bodies in a **temporary VmState** (`tmp_state`), then merges code objects and bindings back into the main state. This creates three classes of bugs:

### 1. Stale continuations

When `call/cc` captures a continuation during library loading, the `VmContinuation` snapshot contains:
- Frame stack from `tmp_state`
- Register values from `tmp_state`
- Dynamic wind records from `tmp_state`
- Exception handlers from `tmp_state`

When the resulting closure is later called from the main `VmState`, invoking the captured continuation tries to restore `tmp_state`'s frame layout into `main_state`'s execution context — the frames don't match, deliver registers are wrong, and the VM crashes or produces garbage.

**Example:** SRFI 1's `%cars+cdrs` uses `call/cc` for early abort. The closure is defined during library loading, captures a continuation in `tmp_state`, then is called from user code running in the main state. The abort continuation is invalid.

### 2. Missing global bindings

Library closures use `LoadGlobal` for internal helpers (e.g., `%any-null?`, `%map-cars`). These names are defined in `lib_env` during loading. When the closure later runs in the main state (which uses `global_env`), `LoadGlobal` fails unless we copy everything from `lib_env` → `global_env`.

**Current workaround:** We copy ALL `lib_env` bindings into `global_env` (lines 523–532 of `backend.rs`), which pollutes the global namespace with every library's internal helpers.

### 3. Missing code objects

Code objects compiled in `tmp_state` must be copied back to the main state, and vice versa. We now do this bidirectionally, but it adds overhead and complexity.

## Root Cause

All three problems stem from the same architectural choice: **library body code runs in a separate VmState that is discarded after loading**. This was originally done to avoid "corrupting the caller's frames" — but it creates a fundamental mismatch between the execution context where closures are *created* and where they're *called*.

## Proposed Design: Execute Library Bodies in the Main VmState

### Core idea

Instead of creating a temporary `VmState`, evaluate library body expressions **directly in the main `VmState`**, using `lib_env` as a scoped overlay. This eliminates all three classes of bugs because closures, continuations, and code objects all live in the single real execution context.

### Approach A: Frame-isolated execution in main state

Use the main `VmState` but push a "library loading frame" that uses `lib_env` as its globals:

```
evaluate_parsed_library(parsed):
    1. Create lib_env, resolve imports into it
    2. For each body expression:
       a. Compile with lib_env as the environment
       b. Push a sentinel frame that marks "library loading context"
       c. Execute in the MAIN VmState using run_loop_until(depth_before)
       d. The sentinel frame ensures we return here, not to the caller
    3. Collect exports from lib_env
    4. Merge lib_env bindings into global_env (for LoadGlobal compatibility)
```

**Advantages:**
- Continuations captured during library loading are valid main-state continuations
- Code objects are naturally in the main code_store
- No bidirectional copying needed
- `call/cc`, `dynamic-wind`, exception handlers all work correctly

**Challenge:**
- Need to handle the `lib_env` vs `global_env` distinction. Library body code should see `lib_env` bindings (its imports), not the full `global_env`. Two sub-approaches:

  **A1: Compile against lib_env, merge before execution.** Merge `lib_env` into `global_env` before running (current approach, but without tmp_state). After loading, internal names remain in `global_env`. Simple but pollutes globals.

  **A2: Environment-scoped LoadGlobal.** Add an `env` field to `CallFrame` so that `LoadGlobal`/`StoreGlobal` use the frame's environment rather than the VmState's `globals`. Library frames use `lib_env`; user frames use `global_env`. This is clean but requires changes to the instruction dispatch loop.

### Approach B: Per-closure environment pointer

Instead of a single `globals` on `VmState`, each `VmClosure` carries a reference to the environment it was compiled against. `LoadGlobal`/`StoreGlobal` look up the closure's environment, not the state's globals.

```rust
pub struct VmClosure {
    code_id: CodeObjectId,
    free_vars: Vec<TaggedValue>,
    globals: Rc<Environment>,  // NEW: the env this closure uses for LoadGlobal
}
```

When a closure is called, its `globals` is used for `LoadGlobal` resolution.

**Advantages:**
- Eliminates the need to merge lib_env → global_env entirely
- Each library's closures see only their own imports — proper encapsulation
- No global namespace pollution
- The VM's `globals` field becomes the *default* for top-level code only

**Challenges:**
- `Rc<Environment>` adds 8 bytes per VmClosure
- Every `LoadGlobal`/`StoreGlobal` instruction must look at the current frame's closure's env instead of `state.globals`
- Need to handle `define` in library bodies (writes to `lib_env`, not `global_env`)

### Approach C: Single-state with global_env scoping (hybrid)

A simpler variant: during library loading, temporarily swap `state.globals` to `lib_env`, run the code, then swap back:

```rust
fn evaluate_parsed_library(&self, parsed: ParsedLibrary) -> Result<Library> {
    let lib_env = Rc::new(Environment::with_heap(...));
    // resolve imports...

    let mut state = self.state.borrow_mut();
    let saved_globals = state.globals.clone();
    state.globals = lib_env.clone();

    for tv in parsed.body {
        // compile + execute in main state, using lib_env as globals
        let (top, nested) = compile_with_qq(&core_expr, &heap, &lib_env)?;
        state.load(top);
        state.load_all(nested);
        execute(&mut state, top_id)?;
    }

    state.globals = saved_globals;
    // merge lib_env bindings for LoadGlobal compat...
}
```

**Advantages:**
- Minimal code change — same execution model, just swap the globals pointer
- All continuations, code objects, etc. live in the main state naturally
- `call/cc` works correctly

**Challenges:**
- Re-entrant library loading (library A loads library B in its body) requires a stack of saved globals
- Closures compiled with `lib_env` as globals still need `lib_env` bindings available when called later (same merge problem as today)
- `run_loop_until` correctness with nested library loads

## Recommendation

**Approach B (per-closure environment)** is the cleanest long-term solution. It eliminates the global namespace pollution problem entirely and makes library encapsulation proper. However, it's the most invasive change.

**Approach C (globals swapping)** is the pragmatic near-term fix. It solves the `call/cc` problem with minimal changes and can be implemented incrementally. The `lib_env → global_env` merge is still needed but is a known, working pattern.

**Approach A2 (frame-scoped env)** is a good middle ground — less invasive than B, cleaner than C.

### Suggested implementation order

1. **Phase 1 (Approach C):** Swap globals during library loading. Fixes the `call/cc` bug. Keep the lib_env → global_env merge. Remove the SRFI 1 `%cars+cdrs` patch once this works.
2. **Phase 2 (Approach B):** Per-closure environment pointer. Eliminates global namespace pollution. Remove the merge step.

## Related: `vm_eval_expr` has the same problem

The `eval` primitive (`vm_eval_expr` in `vm_state.rs`) uses the same temporary-state pattern. The same fix should be applied there. With Approach C, `eval` would swap `state.globals` to the eval environment, execute, and swap back.

## Files to modify

- `crates/patina-vm/src/backend.rs` — `evaluate_parsed_library`
- `crates/patina-vm/src/runtime/vm_state.rs` — `vm_eval_expr`, `VmState`, `CallFrame`
- For Approach B: `crates/patina-core/src/heap/mod.rs` — `VmClosure` struct
- For Approach B: `crates/patina-vm/src/runtime/vm_state.rs` — `LoadGlobal`/`StoreGlobal` dispatch
