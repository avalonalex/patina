# Chibi-Scheme VM Analysis

**Date**: 2025-11-11
**Purpose**: Understanding how chibi-scheme handles deep recursion

## Key Discovery: Chibi Uses a Bytecode VM

Chibi-scheme is **not a tree-walking interpreter** like Patina. Instead, it:

1. **Compiles Scheme code to bytecode** (in `eval.c`)
2. **Executes bytecode in a virtual machine** (in `vm.c`)
3. **Uses an explicit stack** rather than the C call stack

## Architecture Differences

### Patina (Tree-Walking Interpreter)
```
User Code → Parser → AST → Evaluator → Result
                            ↑
                            └─ Uses Rust call stack for recursion
```

**Stack usage per recursive call:**
- Rust stack frame for `eval_in_env()`
- Rust stack frame for `apply()`
- All local variables and temporaries
- **~100-500 bytes per call** (depending on optimization level)

### Chibi-Scheme (Bytecode VM)
```
User Code → Parser → AST → Compiler → Bytecode → VM → Result
                                                  ↑
                                                  └─ Uses heap-allocated stack
```

**Stack usage per recursive call:**
- VM uses a **heap-allocated stack** (`sexp_stack_data()`)
- C call stack only used for the VM loop itself
- **Stack grows on demand** via `sexp_ensure_stack()`
- **Much smaller per-call overhead** (just stack slots, no C frames)

## Evidence from Source Code

### VM Stack Management (`vm.c:1072`)
```c
sexp sexp_apply (sexp ctx, sexp proc, sexp args) {
  // ...
  sexp *stack = sexp_stack_data(sexp_context_stack(ctx));
  sexp_sint_t top = sexp_stack_top(sexp_context_stack(ctx));

  // Ensure stack space - grows heap-allocated stack as needed!
  sexp_ensure_stack(i + 64 +
    (sexp_procedurep(tmp1) ?
     sexp_bytecode_max_depth(sexp_procedure_code(tmp1)) : 0));

  // Main VM loop - single C function handles all bytecode execution
  loop:
    switch (*ip++) {
      case SEXP_OP_NOOP: break;
      case SEXP_OP_PUSH: /* ... */ break;
      case SEXP_OP_CALL: /* ... */ break;
      // ... many more opcodes ...
    }
}
```

### Key Insights:

1. **Heap-allocated stack**: `sexp_stack_data(sexp_context_stack(ctx))`
   - Stack is a heap object, not C call stack
   - Can grow dynamically without stack overflow
   - Only limited by heap memory

2. **Single VM loop**: The entire VM runs in one C function
   - Recursive Scheme calls don't create recursive C calls
   - All calls are "tail calls" from the VM's perspective
   - The `switch (*ip++)` loop processes bytecode sequentially

3. **Bytecode compilation**: Scheme is compiled before execution
   - Compiler can analyze and optimize
   - Tail calls are marked in bytecode
   - VM knows when to reuse stack frames

## Why Chibi Handles Deep Recursion Better

### Non-Tail-Recursive Example: `(power 2 10000)`

**In Patina (tree-walking):**
- Each recursive call adds a Rust stack frame
- 10,000 calls = ~1-5 MB of C stack (depending on frame size)
- Exceeds default stack size → **stack overflow**

**In Chibi (bytecode VM):**
- Each recursive call adds a few stack slots to heap-allocated stack
- 10,000 calls = ~100 KB of heap memory
- Heap can grow dynamically → **no overflow**

## VM vs Tree-Walking Trade-offs

### Advantages of Bytecode VM (Chibi):
✅ Much deeper recursion (heap-limited, not stack-limited)
✅ Better performance (bytecode is faster to execute)
✅ Can optimize during compilation
✅ Easier to implement green threads / continuations

### Advantages of Tree-Walking (Patina):
✅ Simpler implementation (no compiler needed)
✅ Easier to understand and debug
✅ Better error messages (direct AST access)
✅ Faster startup (no compilation phase)
✅ Better for an educational interpreter

## Implications for Patina

### Current Status:
- ✅ **Tail recursion works perfectly** (tested with 10,000 iterations)
- ✅ **R7RS compliant** (proper tail calls are required, non-tail recursion limits are not)
- ⚠️ **Non-tail recursion limited** by Rust stack size (~1,000 calls in release mode)

### Options for Future:

1. **Accept the limitation** (recommended for Phase 1)
   - Idiomatic Scheme uses tail recursion
   - Current behavior is correct per R7RS
   - Tree-walking is simpler and educational

2. **Increase stack size** (easy, partial solution)
   - Set `RUST_MIN_STACK` environment variable
   - E.g., `RUST_MIN_STACK=8388608 cargo run` (8 MB stack)
   - Helps but doesn't fundamentally solve the issue

3. **Implement a bytecode VM** (future phase, major undertaking)
   - Would match chibi's architecture
   - Better performance and recursion depth
   - Complexity: ~3,000-5,000 lines of code
   - Good candidate for Phase 3 or 4

4. **Trampoline-style evaluation** (complex, research project)
   - Convert tree-walking to use explicit stack
   - Hybrid between tree-walking and VM
   - Significant refactoring required

## Recommendation

**For Patina Phase 1**: Keep the tree-walking interpreter. The current tail call optimization is excellent and R7RS-compliant. The difference with chibi for non-tail-recursive calls is an **architectural difference**, not a bug or missing feature.

**For Future Phases**: Consider implementing a bytecode VM as a Phase 3 or 4 enhancement, especially if:
- Building a compiler
- Need better performance
- Want to support continuations (`call/cc`)
- Implementing green threads or async/await

## References

- **Chibi source**: `~/Project/reference/chibi-scheme/`
  - `vm.c` - Virtual machine implementation
  - `eval.c` - Compiler from AST to bytecode
  - `opt/opcode_names.h` - Bytecode opcodes

- **VM Literature**:
  - "Three Implementation Models for Scheme" (Clinger, 1998)
  - "An Incremental Approach to Compiler Construction" (Ghuloum, 2006)
  - "Compiling with Continuations" (Appel, 1992)

## Conclusion

Chibi-scheme's ability to handle deep non-tail recursion comes from its **bytecode VM architecture** with a **heap-allocated stack**, not from any special tail call optimization. Patina's tree-walking interpreter with proper tail call optimization is architecturally different but equally correct for R7RS compliance.
