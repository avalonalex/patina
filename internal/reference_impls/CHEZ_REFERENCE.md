# Chez Scheme Reference

This document provides an analysis of Chez Scheme's implementation, focusing on architectural patterns and optimization strategies that may inform Patina's development. Chez Scheme is an R6RS-compliant Scheme implementation with decades of production use and performance tuning.

**Note**: Chez Scheme implements R6RS (not R7RS), so implementation details must be adapted appropriately. This document focuses on high-level architectural ideas that transcend the R6RS/R7RS distinction.

## Overview

**Repository**: `~/Project/reference/ChezScheme`

Chez Scheme consists of:
- **Scheme compiler** (in `s/` directory) - Most of the system is written in Scheme
- **C kernel** (in `c/` directory) - Runtime, GC, FFI, and primitive operations
- **Portable bytecode mode** - Cross-platform interpreter backend

The system is bootstrapped: you need Chez Scheme to build Chez Scheme.

Key architectural principle: **Self-hosting compiler with small C kernel**. The compiler and most primitives are implemented in Scheme, while the C kernel provides just the essential runtime support.

## String Representation

### Memory Layout

From `c/types.h:275` and `boot/pb/scheme.h:73-141`:

```c
// String element type (32-bit tagged chars for full Unicode)
typedef uint32_t string_char;

// Size calculation (aligned to ptr_align boundary)
#define size_string(n) ptr_align(header_size_string + (n)*string_char_bytes)

// String accessors (from scheme.h)
#define Sstring_length(x) ((iptr)((uptr)(*((iptr *)TO_VOIDP((uptr)(x)+1)))>>4))
#define Sstring_ref(x,i) Schar_value(((string_char *)TO_VOIDP((uptr)(x)+9))[i])
#define Sstring_set(x,i,c) ((void)((((string_char *)TO_VOIDP((uptr)(x)+9))[i]) = (string_char)(uptr)Schar(c)))
```

**Key insights:**

1. **32-bit Unicode storage**: Each character is stored as a `uint32_t` (4 bytes), supporting full Unicode scalar values
2. **O(1) indexing**: Direct array access using `string_char` array - no UTF-8 scanning required
3. **Header format**: Length stored in object header, shifted 4 bits (allows immutability flag in low bits)
4. **Memory overhead**: 4 bytes per character vs 1-4 bytes for UTF-8
5. **Trade-off**: Chez prioritizes **speed** (O(1) access) over **space** (4× memory for ASCII)

**Contrast with Patina:**
- Patina uses `String` (UTF-8) with O(n) indexing via `.chars().nth(k)` - R7RS compliant
- Chez uses `uint32_t` array with O(1) indexing - R6RS compliant
- Both are compliant with their respective standards; R7RS explicitly allows O(n) operations

### String Operations Inlining

From `s/cpprim.ss:6591-6625`:

```scheme
(define-inline 3 string-ref
  [(e-s e-i) `(inline ,(make-info-load ptr-type #f) ,%load ,e-s ,%zero (+ ,e-i string-data-disp))])
(define-inline 2 string-ref
  [(e-s e-i)
   (bind #t (e-s e-i)
     `(if ,(build-string-ref-check e-s e-i)
          (inline ,(make-info-load ptr-type #f) ,%load ,e-s ,%zero (+ ,e-i string-data-disp))
          ,(build-libcall #t src sexpr string-ref e-s e-i)))]))
```

**Compiler optimization strategy:**
- **Level 3 (unsafe)**: Direct memory access, no bounds checking
- **Level 2 (safe)**: Inline bounds check + fast path + slow path fallback
- **Partial inlining**: Only inline the fast path, complex error handling stays in library code

**Actionable for Patina**: When Patina adds a compiler, consider similar "inline fast path, call library for error" pattern.

## Object Representation and Tagging

From `IMPLEMENTATION.md:303-349` and `boot/pb/scheme.h:79-121`:

### Pointer Tagging Scheme

All Scheme objects are represented as tagged pointers (`ptr` type):

```c
// Low 3 bits indicate type
#define Sfixnump(x)    (((uptr)(x) & 0x7) == 0x0)  // xxx000
#define Spairp(x)      (((uptr)(x) & 0x7) == 0x1)  // xxx001
#define Sflonump(x)    (((uptr)(x) & 0x7) == 0x2)  // xxx010
#define Ssymbolp(x)    (((uptr)(x) & 0x7) == 0x3)  // xxx011
#define Sprocedurep(x) (((uptr)(x) & 0x7) == 0x5)  // xxx101
#define Sstringp(x)    (((uptr)(x) & 0x7) == 0x7) && /* additional checks */

// Fixnums: value * 8 (lowest 3 bits = 000)
#define Sfixnum(x) ((ptr)(uptr)((x)*8))
#define Sfixnum_value(x) ((iptr)(x)/8)

// Characters: value << 8 | 0x16
#define Schar(x) ((ptr)(uptr)((x)<<8|0x16))
#define Schar_value(x) ((string_char)((uptr)(x)>>8))

// Immediate constants
#define Snil    ((ptr)0x26)
#define Strue   ((ptr)0x0E)
#define Sfalse  ((ptr)0x06)
```

**BiBOP (Big Bag of Pages) Memory Management:**

Objects are aligned to 8-byte boundaries on 64-bit systems. Type tag uses low bits:
- Round *up* to nearest 8-byte boundary to find actual data
- Pair tagged with `0x1` → add 7 to get car, add 15 to get cdr
- Typed objects (`0x7`) → add 1 to get header word

**Reference**: *Don't Stop the BiBOP: Flexible and Efficient Storage Management for Dynamically Typed Languages* by Dybvig, Eby, Bruggeman (1994)

**Actionable for Patina**: Current `Value` enum is similar in spirit but uses Rust's enum tag. Future optimization could explore pointer tagging for frequently used types (fixnum, pair, nil).

## Garbage Collection Architecture

From `IMPLEMENTATION.md:375` and `c/types.h:143-185`:

### Generational GC with Segments

```c
typedef struct _seginfo {
  unsigned char space;          // space the segment is in
  unsigned char generation;     // generation (0 = youngest)
  unsigned char old_space : 1;  // being collected?
  unsigned char use_marks : 1;  // mark-in-place vs copy?
  octet dirty_bytes[cards_per_segment]; // card table for write barrier
  // ... more fields for ephemerons, guardians, sweep info
} seginfo;
```

**Key features:**

1. **Generational collection**: Objects promoted through generations (0 → static_generation)
2. **Segment-based**: Memory divided into fixed-size segments with metadata
3. **Write barrier**: Card table (`dirty_bytes`) tracks cross-generational pointers
4. **Mark-compact option**: Can mark-in-place instead of copying (controlled by `use_marks`)
5. **Parallel GC support**: `PTHREADS` enabled, `creator` field tracks which thread allocated

**Dirty tracking matrix** (from `c/types.h:216-251`):
```
DirtySegments[from_gen, to_gen] for gen pairs where from_gen > to_gen
Optimized triangular storage: from_g*(from_g-1)/2 + to_g
```

**Actionable for Patina**: Currently no GC (Rust's Rc handles it). If future phases require custom GC (e.g., for logic programming), study Chez's generational design.

## Compiler Architecture

From `IMPLEMENTATION.md:3-31` and `s/` directory structure:

### Compilation Pipeline

```
Source → cpnanopass.ss (nanopass framework)
      → cpprim.ss (primitive inlining)
      → cp0.ss (optimization passes ~308KB!)
      → Backend (x86_64.ss, arm64.ss, riscv64.ss, etc.)
      → Machine code
```

**Nanopass framework** (`s/cpnanopass.ss` - 596KB):
- Many small, single-purpose compiler passes
- Each pass is a single transformation on IR
- Easier to reason about and maintain than monolithic compiler

**Primitive inlining** (`s/cpprim.ss` - 423KB):
- Most primitives inlined directly by compiler
- Multiple safety levels (unsafe=3, safe=2, conservative=1)
- Bootstrapping trick: Primitives defined in Scheme (`prims.ss`) call themselves with special `#2%` prefix that triggers inlining

Example from `IMPLEMENTATION.md:509-535`:
```scheme
;; In prims.ss (runtime library):
(define (set-car! x v)
  (#2%set-car! x v))  ; #2% tells compiler to inline the safe version

;; In cpprim.ss (compiler):
(define-inline 2 set-car!
  [(e-pair e-val)
   `(if (pair? ,e-pair)
        (inline ,%store ,e-pair ,%zero car-disp ,e-val)
        (error 'set-car! "not a pair" ,e-pair))])
```

**Optimization passes** (`s/cp0.ss` - 308KB):
- Constant folding
- Dead code elimination
- Inline expansion
- Common subexpression elimination
- Cross-library optimization (whole-program mode)

**Actionable for Patina**:
- Keep interpreter simple for Phase 1
- When adding compiler (future), consider nanopass approach
- Study primitive inlining strategies for performance-critical operations

## Runtime Stack and Calling Convention

From `IMPLEMENTATION.md:381-461`:

### Heap-Allocated Stack Segments

```
Scheme code does NOT use C stack.
Continuation = linked list of heap-allocated stack segments.

Frame layout (stack grows UP in memory):
                ^
                |          (higher addresses)
          |------------|
          |   var N    |
          |------------|
          |     ...    |
          |------------|
          |   var 1    | SFP[1]
          |------------|
          |  ret addr  | SFP[0]
 SFP ->   |------------|
             previous frames
                |          (lower addresses)
                v
```

**Key features:**

1. **SFP register** (Scheme Frame Pointer): Points to base of current frame
2. **First-class continuations**: Stack is in heap, can be captured and restored
3. **Stack overflow check**: On function entry, check `SFP - segment_end > frame_size`
4. **Proper tail calls**: Reuse current frame, no stack growth

**Thread Context** (TC register):
- Virtual registers that may be assigned to machine registers
- SFP (frame pointer) and TC (thread context) must be in registers
- AP (allocation pointer) and TRAP also good candidates for registers

**Reference**: *Representing Control in the Presence of First-Class Continuations* by Hieb, Dybvig, Bruggeman (PLDI 1990)

**Actionable for Patina**:
- Current interpreter uses Rust call stack - fine for tree-walking
- Proper tail call optimization will require either:
  1. Trampoline technique (interpreter)
  2. Heap-allocated stack (compiler)
- Study Chez's approach if implementing first-class continuations (future)

## Macro System and Syntax Objects

From `s/syntax.ss` (483KB!):

Chez uses a **syntax-case** macro system (R6RS), which is more powerful than R7RS's **syntax-rules**:
- Hygenic macros with pattern matching
- Syntax objects carry lexical context
- Allows procedural macros (not just pattern-based)

**File size insight**: Macro expander is ~483KB of Scheme code - one of the largest single files. Macro systems are complex!

**Actionable for Patina**:
- Phase 1: Focus on syntax-rules (simpler, R7RS compliant)
- Future: Study Chez's syntax.ss if adding advanced macro features

## Primitive Operation Implementation

From `c/prim5.c`, `c/prim.c`, and `s/prims.ss`:

### Two-tier primitive system

1. **C kernel primitives** (`c/prim5.c`, `c/prim.c`):
   - Low-level operations that need C access: FFI, I/O, process control, memory management
   - Example: `S_strerror`, `s_intern`, `s_process`, file operations

2. **Scheme-level primitives** (`s/prims.ss` - 102KB):
   - Higher-level operations built on kernel primitives
   - Can be inlined by compiler
   - Easier to modify without recompiling C kernel

**Bootstrap process**:
1. Build C kernel with minimal primitives
2. Load boot files containing Scheme-level primitives
3. Compiler inlines Scheme primitives during compilation

**Actionable for Patina**:
- Current approach: All primitives in Rust (`eval/primitives.rs`)
- This is correct for interpreter phase
- Future compiler: Consider which primitives should inline vs call

## Foreign Function Interface (FFI)

From `c/foreign.c` and `s/foreign.ss`, `s/ftype.ss`:

Chez has sophisticated FFI for C interop:
- Foreign types defined in Scheme
- Automatic marshalling between Scheme and C types
- Callbacks from C to Scheme
- Portable across platforms

**Example**: `Swide_to_utf8` / `Sutf8_to_wide` for Windows Unicode support

**Actionable for Patina**: Future feature. Rust FFI is already excellent; may not need complex Scheme-level FFI.

## Testing and Validation

From `IMPLEMENTATION.md:193-301`:

### Comprehensive test matrix

Chez tests run in multiple configurations:
- **Compile vs interpret mode**
- **Safe vs unsafe** (optimization level)
- **With/without primitive inlining**
- **With/without cp0 optimization**

Test output compared against expected errors using `diff`.

**Test organization**:
- `mats/*.ms` - Individual test files
- `mats/*.mo` - Test output files
- `mats/root-experr-*` - Expected error baselines
- Parallel test execution with `zuo . -j N test`

**Actionable for Patina**:
- Current test organization (compliance/, integration/) is good
- Consider adding optimization level matrix when compiler is added
- Parallel test execution (already supported by `cargo test`)

## Cross-Platform Support

From `README.md:9-21` and `s/*.def` files:

Supported platforms:
- x86, x86_64, ARM32, AArch64, RV64G, LoongArch64, PowerPC32
- Windows, macOS, Linux, *BSD, Solaris, Android, iOS, WebAssembly

**Architecture-specific backends**:
- `s/x86_64.ss` (149KB)
- `s/arm64.ss` (148KB)
- `s/riscv64.ss` (117KB)
- Each ~100-150KB of code generation logic

**Platform definition files**:
- `s/a6.def`, `s/arm64.def`, `s/rv64.def` - Architecture constants
- `s/unix.def`, `s/nt.def` - OS-specific settings

**Actionable for Patina**: Focus on platform-independent interpreter first. Cross-compilation is future work.

## Code Size Insights

Key files by size reveal implementation priorities:

| File | Size | Purpose |
|------|------|---------|
| `s/cpnanopass.ss` | 596KB | Compiler nanopass framework |
| `s/syntax.ss` | 483KB | Macro expander (syntax-case) |
| `s/cpprim.ss` | 423KB | Primitive inlining rules |
| `s/cp0.ss` | 308KB | Optimization passes |
| `s/io.ss` | 297KB | I/O and port operations |
| `s/primdata.ss` | 191KB | Primitive metadata |
| `s/x86_64.ss` | 149KB | x86-64 code generator |

**Insight**: Compiler infrastructure (nanopass + prim inlining + optimization) = ~1.3MB of Scheme code. A production Scheme compiler is a major undertaking.

## Performance Philosophy

From code inspection and architecture:

1. **Inline critical paths**: Primitives like `string-ref` have multiple inline levels
2. **Lazy compilation**: Compile on first load, cache compiled code
3. **Whole-program optimization**: Can optimize across library boundaries
4. **Generational GC**: Assumes most objects die young
5. **32-bit chars for strings**: Trade space for O(1) indexing speed

**Chez prioritizes**: Speed > Space (within reason)

**Patina currently prioritizes**: Correctness > Speed (appropriate for Phase 1)

## Key Takeaways for Patina

### Immediately Applicable

1. **String representation is a choice**: Chez chose O(1) access with 4×memory. Patina chose O(n) access with UTF-8. Both valid.
2. **Primitive organization**: Two-tier system (C kernel + Scheme library) is powerful for bootstrapping
3. **Test organization**: Multiple test configurations, expected output diffing

### Future Considerations (Phase 2+)

1. **Compiler architecture**: Nanopass framework is proven approach for manageable compiler complexity
2. **Inline strategy**: Inline fast paths, call library for errors and edge cases
3. **GC design**: If custom GC needed, study generational + segment-based approach
4. **Calling convention**: Heap-allocated stack segments enable first-class continuations
5. **Pointer tagging**: Can optimize representation for fixnums, pairs, nil

### Not Applicable to Patina

1. **R6RS-specific features**: Libraries, phases, different macro system
2. **Complex FFI**: Rust already has excellent FFI
3. **Multi-platform native code**: Focus on interpreter portability first

## References

### Papers Cited in IMPLEMENTATION.md

1. **BiBOP Memory Management**:
   *Don't Stop the BiBOP: Flexible and Efficient Storage Management for Dynamically Typed Languages*
   Dybvig, Eby, Bruggeman - Indiana University TR #400, 1994

2. **Heap-allocated stacks**:
   *Representing Control in the Presence of First-Class Continuations*
   Hieb, Dybvig, Bruggeman - PLDI 1990

3. **Continuation marks**:
   *Compiler and Runtime Support for Continuation Marks*
   Flatt, Dybvig - PLDI 2020

### Code Structure

- **Scheme source**: `~/Project/reference/ChezScheme/s/`
- **C kernel**: `~/Project/reference/ChezScheme/c/`
- **Implementation notes**: `~/Project/reference/ChezScheme/IMPLEMENTATION.md`
- **Generated headers**: `~/Project/reference/ChezScheme/boot/pb/scheme.h`, `equates.h`

---

**Document created**: 2025-11-07
**Chez Scheme version analyzed**: 10.4.0-pre-release.1
**Analysis focus**: String representation, compiler architecture, GC design, runtime conventions
