# VM Bytecode Stepper Guide

The patina VM includes a structured bytecode tracer for debugging execution
issues. It records typed events (instructions, register writes, frame
push/pop, control flow, watchpoints) and outputs them in human-readable or
machine-parseable formats.

## Quick Start

### CLI: Trace a script

```bash
# Trace every instruction to stderr
patina --trace script.scm

# Combine with dump to see bytecode first, then trace
patina --dump script.scm      # static disassembly
patina --trace script.scm     # dynamic execution trace
```

### CLI: Dump bytecode without executing

```bash
# From a file
patina --dump script.scm

# From stdin
echo '(+ 1 2)' | patina --dump
```

## Reading the Trace Output

Each line has the format:

```
[step] D=depth #code_id pc=N  instruction_text  (base=B regs=R)
       rN[abs] = new_value  (was old_value)
```

### Example: Simple arithmetic

```
$ echo '(+ 1 2)' > /tmp/t.scm && patina --trace /tmp/t.scm

[0001] D=1 #102 pc=0   LoadGlobal   r1 <- globals[+]  (base=0 regs=4)
       r1[1] = #<prim>  (was ())
[0002] D=1 #102 pc=1   LoadImm      r2 <- fixnum(1)  (base=0 regs=4)
       r2[2] = 1  (was ())
[0003] D=1 #102 pc=2   LoadImm      r3 <- fixnum(2)  (base=0 regs=4)
       r3[3] = 2  (was ())
[0004] D=1 #102 pc=3   Call         r0 <- r1(r2, r3)  (base=0 regs=4)
       r0[0] = 3  (was ())
[0005] D=1 #102 pc=4   Return       r0  (base=0 regs=4)
```

Key:
- `[0001]` — step number (monotonically increasing)
- `D=1` — frame stack depth (number of active CallFrames)
- `#102` — CodeObject ID
- `pc=0` — program counter before execution
- `r1[1]` — frame-relative register r1 at absolute index 1
- `(base=0 regs=4)` — this frame's register window: registers[0..4]

### Example: Nested function call

```
[0003] D=1 #102 pc=2   Call r0 <- r1(r2)  (base=0 regs=3)
       r0[3] = 0     (was ())       -- args copied to new frame
[0004] D=2 #103 pc=0   AllocCell r0 <- box(r0)  (base=3 regs=6)
       r0[3] = MutableCell(0)  (was 0)
```

When `Call` fires, the depth increases from D=1 to D=2 and the base jumps
from 0 to 3 (the callee's register window starts right after the caller's).

### Special event markers

```
>> PUSH    — new frame pushed
<< POP     — frame popped (Return)
** CTRL    — control primitive intercepted (dynamic-wind, call/cc, etc.)
-> THUNK   — run_thunk entered (nested execution)
<- THUNK   — run_thunk returned
CELL       — MutableCell alloc/read/write
!! WATCH   — watchpoint triggered (register changed unexpectedly)
EXN        — exception handler push/pop/raise
CONT       — continuation capture/invoke
```

## Library API (for tests)

The tracer is available as a library for writing diagnostic tests:

```rust
use patina_vm::tracer::{StepTracer, TraceFilter, TraceEventKind, TracerHandle};
use patina_vm::VmBackend;
use patina_interpreter::Interpreter;
use std::rc::Rc;
use std::cell::RefCell;

#[test]
fn test_register_stability() {
    let backend = VmBackend::new();

    // Create a tracer with watchpoints
    let tracer = Rc::new(RefCell::new(StepTracer::with_filter(TraceFilter {
        watchpoints: [3].into(),  // watch absolute register index 3
        ..Default::default()
    })));
    backend.set_tracer(Some(tracer.clone()));

    let interp = Interpreter::new(backend);
    let _ = interp.eval_program("(+ 1 2)");

    // Check for unexpected register changes
    let t = tracer.borrow();
    let clobbers = t.watchpoints();
    assert!(clobbers.is_empty(), "Register 3 was modified:\n{}", t.format_log());
}
```

### Query methods

```rust
let t = tracer.borrow();

// All events of a specific kind
let pushes = t.events_of_kind(TraceEventKind::FramePush);

// All writes to a specific absolute register index
let r3_writes = t.writes_to_abs(3);

// All writes to a frame-relative register
let r0_writes = t.writes_to_reg(0);

// Events for a specific CodeObject
let code_103_events = t.events_for_code(103);

// All watchpoint triggers
let watches = t.watchpoints();

// Human-readable log
println!("{}", t.format_log());

// Machine-parseable JSONL
println!("{}", t.format_jsonl());
```

### Filtering

Reduce noise by filtering to specific code objects or event types:

```rust
let filter = TraceFilter {
    // Only trace CodeObject #103
    code_ids: [103].into(),
    // Only record instruction and register write events
    event_kinds: [TraceEventKind::Instr, TraceEventKind::RegWrite].into(),
    // Cap at 1000 events to prevent OOM
    max_events: 1000,
    ..Default::default()
};
let tracer = StepTracer::handle_with_filter(filter);
```

### Watchpoints

Watchpoints monitor absolute positions in the flat register array and trigger
`Watchpoint` events when the value changes. This catches cross-frame clobbering.

```rust
let filter = TraceFilter {
    // Monitor absolute register indices 3 and 9
    watchpoints: [3, 9].into(),
    ..Default::default()
};
```

Watchpoint output:
```
[0042]   !! WATCH regs[3] changed: MutableCell(0) -> 99 (in #106 pc=2)
```

## JSONL Output Format

For programmatic analysis (e.g., piping to `jq` or reading from Claude):

```bash
patina --trace script.scm 2>&1 | patina-trace-to-jsonl
```

Or use `format_jsonl()` from the library:

```json
{"type":"instr","step":1,"depth":1,"code":102,"pc":0,"text":"LoadGlobal r1 <- globals[+]"}
{"type":"reg_write","step":1,"reg":1,"abs":1,"old":"()","new":"#<prim>","source":"instr"}
{"type":"frame_push","step":3,"depth":2,"code":103,"base":3,"regs":6,"ret":0}
{"type":"watchpoint","step":42,"abs":3,"old":"MutableCell(0)","new":"99","code":106,"pc":2}
```

## Debugging the run_thunk Register Clobbering Bug

The stepper was designed to help diagnose the known bug where `run_thunk`'s
Return instruction clobbers live registers in the caller frame. Here's how to
use it:

### Step 1: Dump the bytecode

```bash
patina --dump failing_test.scm
```

Identify the CodeObject that holds the MutableCell (look for `AllocCell`).
Note its register base and which register holds the cell (usually r0).

### Step 2: Set a watchpoint

```rust
let filter = TraceFilter {
    // Watch the absolute index of the MutableCell register
    // (register_base + register_number)
    watchpoints: [3].into(),  // e.g., base=3, cell in r0 -> abs=3
    ..Default::default()
};
```

### Step 3: Run and check

```rust
let t = tracer.borrow();
for w in t.watchpoints() {
    if let TraceEvent::Watchpoint { step, old_val, new_val, code_id, pc, .. } = w {
        eprintln!("Clobber at step {}: #{} pc={}: {} -> {}", step, code_id, pc, old_val, new_val);
    }
}
```

## Architecture

The tracer is implemented in `crates/patina-vm/src/tracer.rs`:

- `TraceEvent` — enum of all event types
- `TraceFilter` — controls what gets recorded
- `StepTracer` — the main tracer struct, holds events and filter
- `TracerHandle = Rc<RefCell<StepTracer>>` — shared handle

Integration points in `crates/patina-vm/src/runtime/vm_state.rs`:

- `dispatch_one_instruction()` — pre/post instruction hooks
- `VmState.tracer: Option<TracerHandle>` — the tracer field

When `tracer` is `None`, the overhead is a single `if let Some(...)` branch
per instruction (no allocation, no formatting).
