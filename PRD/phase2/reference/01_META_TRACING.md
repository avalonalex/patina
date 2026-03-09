# Meta-Tracing on Demand

**Priority:** ⭐⭐⭐ High
**Complexity:** Medium (6-8 weeks)
**Impact:** Very High (10-100x on hot loops)
**Status:** Research

---

## Overview

Build a lightweight tracing JIT that automatically identifies hot tail-recursive loops and compiles only those fragments to native code. Unlike traditional method-based JITs, this approach traces execution paths rather than compiling entire functions.

**Key Insight:** Most Scheme programs spend 90% of time in 10% of code (tight loops). Instead of compiling everything, trace and compile only the hot paths.

---

## What is Meta-Tracing?

**Traditional JIT:**
```
Function detected as hot → Compile entire function → Execute compiled code
```

**Meta-Tracing (PyPy-style):**
```
Execution path detected as hot → Trace path → Compile trace → Execute guard + compiled trace
```

**"On Demand" variant:**
- Don't trace everything eagerly
- Only start tracing when interpreter detects stable tail recursion
- Abort trace if path diverges (guards fail)

---

## Why It's Novel (2023-2025)

Recent research focus:
- **Lightweight tracing** without full Graal/LLVM infrastructure
- **Trace-based JITs** without hotspot detection overhead
- **Partial evaluation** approaches that are easier to implement
- **Meta-circular** tracing (trace the interpreter itself)

**Key Papers:**
- "Meta-Tracing Makes a Fast Racket" (Bolz & Tratt, 2021)
- "Pycket: A Tracing JIT for a Functional Language" (Bauman et al., 2015)
- "One VM to Rule Them All" (Würthinger et al., 2013)

---

## How It Fits Scheme

**Perfect match:**

1. **Tail recursion is common:** Scheme loops are tail-recursive functions
   ```scheme
   (define (sum n acc)
     (if (= n 0)
         acc
         (sum (- n 1) (+ acc n))))  ; ← Stable tail call path
   ```

2. **Natural trace boundaries:** Each tail call is a potential trace loop
3. **Continuations as trace abort:** `call/cc` forces trace exit

**Trace example:**
```
Input Scheme:
  (sum 1000000 0)

Detected hot path:
  n > 0 → subtract n → add to acc → tail call

Generated trace:
  guard: n is fixnum
  guard: n > 0
  r1 = n - 1          ; specialized integer subtract
  r2 = acc + n        ; specialized integer add
  loop if r1 > 0
```

---

## Concrete Implementation Plan

### Phase 1: Profiling Infrastructure (1 week)

**Add hotness counters to bytecode:**
```rust
pub struct Bytecode {
    instructions: Vec<Opcode>,
    hotness_counters: Vec<u32>,  // NEW: Count executions
}

// During VM execution:
fn execute_jump_back(&mut self, target: usize) {
    self.hotness_counters[target] += 1;
    if self.hotness_counters[target] > TRACE_THRESHOLD {
        self.start_tracing(target);
    }
}
```

**Constants:**
```rust
const TRACE_THRESHOLD: u32 = 100;  // Start tracing after 100 iterations
const MAX_TRACE_LENGTH: usize = 200;  // Max instructions in trace
```

---

### Phase 2: Trace Recording (2 weeks)

**Trace recorder:**
```rust
pub struct TraceRecorder {
    recording: bool,
    trace: Vec<TracedOp>,
    guards: Vec<Guard>,
    loop_header: usize,
}

pub enum TracedOp {
    LoadConst { reg: Register, value: Value },
    IntAdd { dst: Register, src1: Register, src2: Register },
    IntSub { dst: Register, src1: Register, src2: Register },
    Guard { guard: Guard },
    Jump { target: usize },
}

pub enum Guard {
    IsFixnum(Register),
    IsPositive(Register),
    TagCheck { reg: Register, expected_tag: u8 },
}
```

**Recording algorithm:**
```rust
impl VM {
    fn start_tracing(&mut self, loop_header: usize) {
        self.trace_recorder = Some(TraceRecorder {
            recording: true,
            trace: vec![],
            guards: vec![],
            loop_header,
        });
    }

    fn record_operation(&mut self, op: Opcode) {
        if let Some(recorder) = &mut self.trace_recorder {
            match op {
                Opcode::Add(dst, src1, src2) => {
                    // Check if we can specialize
                    if self.is_fixnum(src1) && self.is_fixnum(src2) {
                        recorder.add_guard(Guard::IsFixnum(src1));
                        recorder.add_guard(Guard::IsFixnum(src2));
                        recorder.add_op(TracedOp::IntAdd { dst, src1, src2 });
                    } else {
                        // Generic path - abort trace
                        self.abort_trace();
                    }
                }
                Opcode::JumpBack(target) if target == recorder.loop_header => {
                    // Found loop back-edge, compile trace!
                    self.compile_trace();
                }
                _ => { /* Record other ops */ }
            }
        }
    }

    fn abort_trace(&mut self) {
        self.trace_recorder = None;
        // Fallback to interpreter
    }
}
```

---

### Phase 3: Trace Compilation (3-4 weeks)

**Option A: Cranelift (Recommended)**
```rust
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_codegen::ir::Function;

pub struct TraceCompiler {
    jit_module: JITModule,
}

impl TraceCompiler {
    fn compile_trace(&mut self, trace: &[TracedOp]) -> *const u8 {
        let mut func = Function::new();
        let entry_block = func.dfg.make_block();

        for op in trace {
            match op {
                TracedOp::IntAdd { dst, src1, src2 } => {
                    // Cranelift IR:
                    // v_dst = iadd v_src1, v_src2
                    let v1 = self.get_value(src1);
                    let v2 = self.get_value(src2);
                    let result = builder.ins().iadd(v1, v2);
                    self.set_value(dst, result);
                }
                TracedOp::Guard { guard } => {
                    // Emit guard check, branch to deopt on failure
                    match guard {
                        Guard::IsFixnum(reg) => {
                            let val = self.get_value(reg);
                            let tag = builder.ins().band_imm(val, TAG_MASK);
                            let is_fixnum = builder.ins().icmp_imm(
                                IntCC::Equal, tag, FIXNUM_TAG
                            );
                            builder.ins().brz(is_fixnum, deopt_block);
                        }
                    }
                }
                _ => { /* Other ops */ }
            }
        }

        // Compile to native code
        self.jit_module.define_function(func_id, &mut func).unwrap();
        self.jit_module.finalize_definitions();
        self.jit_module.get_finalized_function(func_id)
    }
}
```

**Option B: Simple x64 Backend (Lighter weight)**
```rust
pub struct SimpleJIT {
    code_buffer: Vec<u8>,
}

impl SimpleJIT {
    fn compile_trace(&mut self, trace: &[TracedOp]) -> *const u8 {
        // Emit x64 machine code directly
        for op in trace {
            match op {
                TracedOp::IntAdd { dst, src1, src2 } => {
                    // mov rax, [src1]
                    // add rax, [src2]
                    // mov [dst], rax
                    self.emit_mov_reg_mem(RAX, src1);
                    self.emit_add_reg_mem(RAX, src2);
                    self.emit_mov_mem_reg(dst, RAX);
                }
                TracedOp::Guard { guard } => {
                    // test rax, TAG_MASK
                    // jnz deopt_label
                    self.emit_test_imm(RAX, TAG_MASK);
                    self.emit_jnz(deopt_label);
                }
            }
        }

        // Make executable and return
        self.make_executable()
    }
}
```

---

### Phase 4: Deoptimization (1 week)

**When guard fails, return to interpreter:**
```rust
pub struct DeoptInfo {
    bytecode_pc: usize,
    register_map: HashMap<Register, Value>,
}

fn deoptimize(deopt_info: &DeoptInfo, vm: &mut VM) {
    // Restore VM state
    vm.pc = deopt_info.bytecode_pc;
    for (reg, val) in &deopt_info.register_map {
        vm.registers[*reg] = *val;
    }

    // Mark trace as invalid
    vm.invalidate_trace(deopt_info.bytecode_pc);

    // Continue in interpreter
    vm.interpret();
}
```

---

## Expected Performance

**Benchmark: Sum 1-1000000**

| Implementation | Time | Speedup |
|---------------|------|---------|
| Tree-walker | 30s | 1x |
| Bytecode VM | 3s | 10x |
| Traced JIT | 0.3s | 100x |
| Native C | 0.05s | 600x |

**Why 100x faster:**
- No interpreter loop overhead
- Specialized integer operations (no boxing)
- No type checks in hot loop
- Direct machine code execution

---

## Challenges & Mitigation

### Challenge 1: Trace Explosion
**Problem:** Too many traces consume memory

**Solution:**
- Limit max traces (e.g., 100)
- LRU eviction policy
- Only trace loops, not straight-line code

### Challenge 2: Guard Overhead
**Problem:** Too many guards slow down traces

**Solution:**
- Hoist loop-invariant guards
- Merge redundant guards
- Specialize common patterns (fixnum + fixnum)

### Challenge 3: Deopt Frequency
**Problem:** Frequent deoptimization kills performance

**Solution:**
- Track deopt reasons, blacklist unstable traces
- Polymorphic inline caching (2-3 specialized versions)
- Abort tracing for highly polymorphic code

---

## Scheme-Specific Optimizations

### Tail Call Optimization in Traces
```rust
// Detect tail recursion pattern:
// (define (loop n) (if (= n 0) done (loop (- n 1))))

// Normal trace: Records function call overhead
// Optimized: Unroll into direct loop

TracedOp::TailCall { func, args } if func == current_function => {
    // Don't emit call, just loop!
    TracedOp::Jump { target: loop_header }
}
```

### Continuation Detection
```rust
// call/cc forces trace abort
TracedOp::CallCC { .. } => {
    self.abort_trace();  // Can't trace through continuations
}

// But we can trace continuation captures if not invoked:
TracedOp::MakeContinuation { .. } => {
    // Just guard that it's never invoked in this trace
    self.add_guard(Guard::ContinuationNotInvoked);
}
```

---

## References

**Essential Reading:**
1. "Tracing the Meta-Level: PyPy's Tracing JIT Compiler" (Bolz et al., 2009)
   - Foundational paper on meta-tracing

2. "Meta-Tracing Makes a Fast Racket" (Bolz & Tratt, 2021)
   - Recent application to Scheme-like language

3. "Pycket: A Tracing JIT for a Functional Language" (Bauman et al., 2015)
   - Traces Racket bytecode

**Implementation References:**
- PyPy source code: rpython/jit/metainterp/
- Pycket source code: github.com/samth/pycket
- LuaJIT (Mike Pall) - Excellent trace compiler

---

## Next Steps

1. **Week 1-2:** Implement profiling counters in bytecode VM
2. **Week 3-4:** Build trace recorder for simple loops
3. **Week 5-6:** Integrate Cranelift, compile first trace
4. **Week 7-8:** Add guards, deoptimization, benchmarking

**Milestone:** Fibonacci or sum loop running 100x faster than interpreter

---

## Alternative: Start Even Simpler

**Micro-Tracing (2-3 weeks):**
- Only trace primitive operations (no function calls)
- Compile to simple bytecode (not native)
- Skip deoptimization (just invalidate on failure)

**Benefits:**
- Proves concept with less complexity
- Still gets 5-10x speedup
- Foundation for full tracing later

This might be the best starting point! 🎯
